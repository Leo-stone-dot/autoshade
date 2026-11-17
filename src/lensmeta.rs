//! In-camera lens-correction metadata → engine-space [`LensProfile`].
//!
//! Sony bodies write the manufacturer's exact correction profile for the
//! mounted lens at the shot's focal length/aperture into every ARW as three
//! spline-knot arrays (`0x7032` vignetting, `0x7035` chromatic aberration,
//! `0x7037` distortion — the same 0x70xx block rawler models as `ExifTag`
//! variants). That makes a lensfun database unnecessary for Sony files: the
//! per-shot profile ships inside the file.
//!
//! Conversion formulas follow RawTherapee's shipped implementation
//! (`rtengine/lensmetadata.cc`), cross-checked against the independent
//! reverse-engineering write-up at <https://stannum.io/blog/0PwljB>:
//!
//! * vignette gain  `1 / 2^(0.5 − 2^(v·2⁻¹³ − 1))`  (v = 0 ⇒ exactly 1.0)
//! * distortion radius factor  `v·2⁻¹⁴ + 1`
//! * CA radius factor  `v·2⁻²¹ + 1` (16 red knots then 16 blue; multiplies
//!   the distortion/green map per channel)
//!
//! Vignette and CA retain the engine's established canonical knot convention.
//! The Sony distortion array is converted here, at the source boundary, twice:
//!
//! * for the RENDER, the way Adobe Camera Raw reads it
//!   ([`sony_render_distortion`]: knots at `i/(n−1)` of the SENSOR
//!   half-diagonal, a least-squares odd polynomial through the camera's
//!   radius map, evaluated at the zoomed output radius), resampled onto the
//!   engine's canonical `(i+0.5)/(n−1)` grid;
//! * for the Lightroom mask-transport solve, D2's native `(i+1)/16` reading,
//!   resampled onto the same canonical grid ([`resample_sony_distortion`]) —
//!   that solve was measured on its own and is left as measured.
//!
//! Each array's FIRST element is the value count; a malformed count degrades
//! that component to "absent", never an error (a photo without correction
//! data is a normal photo).
//!
//! # The MASK WARP, and why it is solved here (R29 Batch-3)
//!
//! [`LensProfile::mask_warp`] is the radial magnification between the frame
//! Lightroom STORES a mask's coordinates in and the frame it EXPORTS that mask
//! into (`render`'s mask-warp block header carries the measurements). It has
//! two possible sources and this function is where the choice is made, because
//! this is the one place that already holds the camera's own knots:
//!
//! * **Source A** — the in-camera `0x7037` spline just read above, inverted by
//!   [`crate::render::mask_warp_from_camera_knots`]. Preferred whenever it
//!   exists: it is the maker's calibration for THIS shot, needs no external
//!   file, and scored 2.30 px against Adobe's own 2.11 px on the 138-patch
//!   pixel field.
//! * **Source B** — an Adobe `.lcp` on this machine ([`crate::lcp`]), for
//!   bodies that write no knots at all.
//!
//! Every failure is TAGGED, never silent: [`crate::recipe::MaskWarpSource`]
//! names which source answered or which of the five refusals applies.
//!
//! **The warp depends on the frame's ASPECT only, not its pixel count.** Both
//! solves normalise radius by the half-diagonal and the `.lcp` reference length
//! is proportional to the width, so the frame's size cancels out of `m(r)`. The
//! dimensions read below are therefore a shape, and a preview-sized render and
//! a full-resolution export share one map — which is what lets it be solved
//! once, here, and stored.

use std::path::Path;

use rawler::formats::tiff::reader::TiffReader;
use rawler::formats::tiff::{GenericTiffReader, Value};
use rawler::tags::{DngTag, ExifTag};

use crate::recipe::{LensProfile, MASK_WARP_KNOTS, MaskWarpCenter, MaskWarpSource};

/// Dense enough that representing either measured 16-knot Sony distortion
/// spline on the engine's offset canonical grid has <1e-5 maximum factor
/// error, including the awkward r=1 endpoint between the last two canonical
/// nodes. The contract test records the measured maximum.
pub(crate) const SONY_DISTORTION_CANONICAL_KNOTS: usize = 2048;

/// Canonical knots the Sony RENDER distortion is resampled onto: the most
/// [`LensProfile::clamp`] keeps (it truncates every profile spline to 32, and
/// a longer one would lose its outer half). The target is a smooth even
/// polynomial, so linear interpolation on this grid departs from it by an
/// eighth of its curvature times the node spacing squared; the contract test
/// (`the_render_resample_is_faithful_to_the_camera_raw_law`) records the
/// measured maximum on the five fixture lenses.
pub(crate) const SONY_RENDER_DISTORTION_KNOTS: usize = 32;

/// What a sidecar adds to the lens lookup: the Adobe profile Lightroom named
/// and the focus distance it recorded (ApproximateFocusDistance). A RAW alone
/// states neither.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LensQuery {
    pub named: Option<NamedProfile>,
    pub focus_distance_m: Option<f32>,
}

/// A profile a sidecar names (`crate::lcp::open_named`). `custom` is
/// crs:LensProfileSetup="Custom": when the name resolves to nothing Camera Raw
/// then draws no profile at all, where under LensDefaults or Auto it falls
/// back to the profile it would choose by itself.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct NamedProfile {
    pub name: String,
    pub filename: Option<String>,
    pub custom: bool,
}

/// Read the in-camera lens correction profile from a RAW file. Every failure
/// path returns an EMPTY component (profile with nothing to apply) — absent
/// metadata is the common case for non-Sony files, not an error.
pub fn read(path: &Path) -> LensProfile {
    read_for(path, &LensQuery::default())
}

/// [`read`] with what a sidecar adds ([`LensQuery`]).
///
/// The VIGNETTING is the Adobe profile's whenever this machine has one for the
/// lens (`lcp::LensProfileFile::vignette_gains`), and the camera's own knots
/// only when it has none. Camera Raw never renders the camera's vignette knots
/// (tag 0x7032 and its SR2 copy rewritten to constants render bit-identically
/// under every lens setup, with the profile on or off, and DNG Converter writes
/// no vignette opcode); with the profile on, its gain is the profile's
/// VignetteModel. The camera's knots stay as the fallback because a machine
/// with no Adobe profile has no Camera Raw reading to follow, and they are the
/// maker's own correction for the shot.
pub fn read_for(path: &Path, query: &LensQuery) -> LensProfile {
    let mut out = LensProfile::default();
    if !crate::decode::is_raw(path) {
        return out;
    }
    let Ok(file) = std::fs::File::open(path) else { return out };
    let mut reader = std::io::BufReader::new(file);
    // `Some(16)`, not `None`: with no chain cap, rawler 0.7.2's walker
    // (formats/tiff/reader.rs:164-179) keeps no visited-offset set and its
    // only `break` sits inside `if let Some(max)` — a self-referential
    // next_ifd loops forever, pushing an IFD per iteration. 16 is rawler's
    // own test value and above its internal `new_root` cap of 10; real RAWs
    // carry ≤4 top-level IFDs, and this reader only consults `root_ifd()`,
    // so a truncated chain tail is unobservable here.
    let Ok(tiff) = GenericTiffReader::new(&mut reader, 0, 0, Some(16), &[]) else {
        return out;
    };
    let root = tiff.root_ifd();
    let knots = |tag: ExifTag| -> Vec<i16> {
        let Some(entry) = root.get_entry_recursive(tag) else { return Vec::new() };
        let vals: Vec<i16> = match &entry.value {
            Value::SShort(v) => v.clone(),
            Value::Short(v) => v.iter().map(|&x| x as i16).collect(),
            _ => return Vec::new(),
        };
        // First element = value count; guard the shape instead of trusting it.
        match vals.first().map(|&n| n as usize) {
            Some(n) if (2..=64).contains(&n) && vals.len() > n => vals[1..=n].to_vec(),
            _ => Vec::new(),
        }
    };

    out.vignette = knots(ExifTag::VignettingCorrParams)
        .iter()
        .map(|&v| vignette_gain(v))
        .collect();
    let sony_distortion: Vec<f32> = knots(ExifTag::DistortionCorrParams)
        .iter()
        .map(|&v| v as f32 * (-14f32).exp2() + 1.0)
        .collect();
    // The render spline is Camera Raw's reading of these knots, which needs
    // the frame (`DefaultCropSize`) and the sensor array it was cut from. A
    // file that states no frame keeps the knots as they are — the legacy
    // reading, for a file that is malformed anyway.
    out.distortion = match frame_dims(root) {
        Some(crop) if !sony_distortion.is_empty() => sony_render_distortion(
            &sony_distortion,
            crop,
            sensor_dims(root),
            SONY_RENDER_DISTORTION_KNOTS,
        ),
        _ => sony_distortion.clone(),
    };
    let lr_mask_distortion =
        resample_sony_distortion(&sony_distortion, SONY_DISTORTION_CANONICAL_KNOTS);
    let ca = knots(ExifTag::ChromaticAberrationCorrParams);
    // The pair split needs an even count AND enough knots per channel for a
    // meaningful radial spline (real bodies write 16+16; a malformed 2-value
    // array would otherwise become two 1-knot "constants" instead of
    // degrading to absent, per the malformed-metadata contract).
    if ca.len() % 2 == 0 && ca.len() >= 8 {
        let half = ca.len() / 2;
        let f = |v: &i16| *v as f32 * (-21f32).exp2() + 1.0;
        let (r, b): (Vec<f32>, Vec<f32>) =
            (ca[..half].iter().map(f).collect(), ca[half..].iter().map(f).collect());
        // Camera Raw's reading of the pair (see `sony_render_ca`) needs the
        // frame, like the distortion's; no frame keeps the knots as they are.
        match frame_dims(root) {
            Some(crop) => {
                let sensor = sensor_dims(root);
                let n = SONY_RENDER_DISTORTION_KNOTS;
                out.ca_r = sony_render_ca(&r, &sony_distortion, crop, sensor, n);
                out.ca_b = sony_render_ca(&b, &sony_distortion, crop, sensor, n);
            }
            None => (out.ca_r, out.ca_b) = (r, b),
        }
    }

    // --- the mask warp (R29 Batch-3) -------------------------------------
    let text = |tag: ExifTag| -> String {
        root.get_entry_recursive(tag)
            .and_then(|e| e.value.as_string().cloned())
            .unwrap_or_default()
            .trim()
            .to_string()
    };
    let number = |tag: ExifTag| -> Option<f32> {
        root.get_entry_recursive(tag)?.value.get_f32(0).ok().flatten().filter(|v| v.is_finite())
    };
    // `DefaultCropSize` FIRST, and for the reason `xmp`'s frame doc spells out
    // at length: it is the frame every normalised coordinate in a sidecar is
    // measured against, where `ImageWidth/Length` is the raw sensor array
    // including the masked border. Only the ASPECT of this reaches the answer,
    // but the two aspects differ by ~0.3 % on a real ARW and the fallback is
    // the second-best shape, not an equal one.
    let dims = frame_dims(root);
    out.mask_warp_center = mask_warp_center(root, dims);
    // The Adobe profile for this lens, looked up once: source B of the mask
    // warp for a body with no knots, and the vignetting for every body.
    let lens = {
        let m = text(ExifTag::LensModel);
        if m.is_empty() { text(ExifTag::LensMake) } else { m }
    };
    let auto = || crate::lcp::open(None, &text(ExifTag::Make), &lens);
    let adobe = match &query.named {
        None => auto(),
        Some(n) => match crate::lcp::open_named(&n.name, n.filename.as_deref()) {
            Err(_) if !n.custom => auto(),
            found => found,
        },
    };
    out.adobe = adobe.as_ref().ok().map(|f| crate::recipe::AdobeLensProfile {
        name: f.profile_name.clone(),
        filename: f.filename.clone(),
    });
    match dims {
        Some(dims) if !lr_mask_distortion.is_empty() => {
            let w = crate::render::mask_warp_from_camera_knots(
                &lr_mask_distortion,
                dims,
                MASK_WARP_KNOTS,
            );
            if w.len() >= 2 {
                out.mask_warp = w;
                out.mask_warp_src = MaskWarpSource::CameraMetadata;
            } else {
                // The inversion is only unsolvable for a spline that folds
                // inside the frame — a corrupt array, not a real lens.
                out.mask_warp_src = MaskWarpSource::Unparseable;
            }
        }
        Some(dims) => {
            // No in-camera knots: this is exactly the body source B exists for.
            let focal = number(ExifTag::FocalLength);
            match adobe.as_ref().map_err(|r| *r).and_then(|f| crate::lcp::solve_in(f, focal, dims, MASK_WARP_KNOTS)) {
                Ok(maps) => {
                    out.mask_warp = maps.mask_warp;
                    out.mask_warp_src = MaskWarpSource::Lcp;
                    // v1.5.0: the same node also RENDERS. Only into a component
                    // the camera left empty — an in-camera spline is this
                    // photo's own measurement and outranks a lens-model average
                    // from a profile database, so this fills a hole and never
                    // overwrites. A body with no correction metadata at all
                    // used to render with no lens correction while Lightroom
                    // rendered one; that was the gap, and it closes here rather
                    // than at a render surface, because every surface already
                    // reads these two fields.
                    //
                    // The RENDER spline is solved at the clamp's 32 knots, not
                    // the mask warp's 64: `LensProfile::clamp` keeps 32, and a
                    // 64-knot spline lost its outer half there — the map then
                    // held its mid-frame value out to the corner.
                    let render = adobe
                        .as_ref()
                        .ok()
                        .and_then(|f| f.model_at(focal).ok())
                        .and_then(|m| m.distortion_knots(dims, SONY_RENDER_DISTORTION_KNOTS));
                    if out.distortion.is_empty()
                        && let Some(d) = render
                    {
                        out.distortion = d;
                    }
                }
                Err(r) => out.mask_warp_src = r.into(),
            }
        }
        // A file that declares no frame declares no aspect, and the map is a
        // function of the aspect. Refusing beats guessing 3:2.
        None => out.mask_warp_src = MaskWarpSource::Absent,
    }

    // --- the vignetting (see `read_for`) ----------------------------------
    // Gains over the default crop, in the frame before the lens geometry —
    // the frame `render::develop` applies them in — normalised by the SENSOR
    // width, which is what Camera Raw's reading of the profile measures.
    let shot = crate::lcp::Shot {
        focal_mm: number(ExifTag::FocalLength),
        aperture_av: number(ExifTag::FNumber).and_then(crate::lcp::Shot::av_of_f_number),
        focus_distance_m: query.focus_distance_m,
    };
    if let (Ok(file), Some(crop)) = (&adobe, dims) {
        let sensor_width = sensor_dims(root).map_or(crop.0, |s| s.0);
        if let Some(g) = file.vignette_gains(shot, crop, sensor_width, SONY_RENDER_DISTORTION_KNOTS) {
            out.vignette = g;
        }
    }

    out.clamp(); // same defensive ranges as a hand-edited recipe
    out
}

/// The frame this photo's normalised coordinates are measured against:
/// `DefaultCropSize` when the file states one, else the raw array.
///
/// Split out so the precedence is one statement rather than a nested `match`
/// inside `read` — and so a test can state it.
fn frame_dims(root: &rawler::formats::tiff::IFD) -> Option<(f32, f32)> {
    let pair = |a: Option<f32>, b: Option<f32>| -> Option<(f32, f32)> {
        let (w, h) = (a?, b?);
        (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
    };
    let dng = root.get_entry_recursive(DngTag::DefaultCropSize).map(|e| &e.value);
    if let Some(v) = dng
        && let Some(d) = pair(v.get_f32(0).ok().flatten(), v.get_f32(1).ok().flatten())
    {
        return Some(d);
    }
    pair(
        root.get_entry_recursive(ExifTag::ImageWidth)?.value.get_f32(0).ok().flatten(),
        root.get_entry_recursive(ExifTag::ImageHeight)?.value.get_f32(0).ok().flatten(),
    )
}

/// The raw sensor array the default crop was cut from (`ImageWidth/Length`),
/// the frame the camera's own lens maps are stated in.
fn sensor_dims(root: &rawler::formats::tiff::IFD) -> Option<(f32, f32)> {
    let w = root.get_entry_recursive(ExifTag::ImageWidth)?.value.get_f32(0).ok().flatten()?;
    let h = root.get_entry_recursive(ExifTag::ImageHeight)?.value.get_f32(0).ok().flatten()?;
    (w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0).then_some((w, h))
}

/// Sony `0x7037` distortion knots → the engine's render spline, read the way
/// Adobe Camera Raw reads them (ledger item 13, 2026-10-28).
///
/// The law, from causal probes through Adobe DNG Converter 18.7 (which writes
/// Camera Raw's own reading of the knots into the file as the LensDistortInfo
/// polynomial) and through Camera Raw renders — reproduced, not fitted:
///
/// * Camera Raw reads the copy of the knots in the encrypted SR2 private
///   block (tag 0x7982, the same sixteen values as `0x7037`); knot `i` sits at
///   `t_i = i/(n−1)`, `t` in units of the SENSOR half-diagonal;
/// * the camera's map is `source = t·K(t)`, and Camera Raw replaces it by the
///   least-squares odd polynomial `t·Q(t)`, `Q = c₁ + c₃t² + c₅t⁴ + c₇t⁶`,
///   through the knots `i ≥ 1` (unweighted; the centre knot's row is zero);
/// * the output is zoomed by `Z` so that no edge pixel samples outside the
///   frame: output radius `u`, `source = Z·u·Q(Z·u)`, with the maximum of
///   `Z·Q(Z·u)` over the frame edge equal to 1 — a constant map therefore
///   renders as the identity, and a barrel map pins the short edge's midpoint
///   where a pincushion one pins the corner.
///
/// Measured: the DNG's polynomial `(Z·c₁, Z³·c₃, Z⁵·c₅, Z⁷·c₇)` for the
/// sensor frame matches this within 2.1e-6 per coefficient on five real
/// frames and 1e-5 on fourteen synthetic knot sets (6e-5 on a single-knot
/// step); its constant term sits 0.5–4.5e-5 below (2.1e-5 above on the
/// step), as if Adobe's zoom stopped short of the exact root by up to
/// 1.5·2⁻¹⁵ — 0.005 % of radius, not modelled. A Camera Raw render of a single-knot step shows the polynomial's
/// ringing, not the knots' step, so the polynomial is what renders.
///
/// The old reading evaluated the knots themselves at the UNZOOMED output
/// radius, `K(u)/max K`: for a map rising to `K = 1.054` at the corner that
/// indexes every knot about 5 % too far out — the 0.06–0.26 % magnification
/// the five-frame registration measured on the FE 24–105 frames.
///
/// The engine's render spline (`render::profile_knot_interp`: canonical
/// `(j+0.5)/(N−1)` in DEFAULT-CROP half-diagonals, divided by its own edge
/// maximum) draws that map when its knots are `k(r) = Q(Z·ρ·r)` with
/// `ρ = |crop| / |sensor|` and `Z` solved against the crop's edge — the
/// division by the edge maximum then IS the zoom. What it cannot carry is the
/// centre: Camera Raw's map is centred on the sensor, which on the ILCE-7RM4A
/// sits 16 px right of the default crop's centre; that offset moves pixels by
/// under a pixel and no radius, and is left to the frame-centre work.
pub(crate) fn sony_render_distortion(
    native: &[f32],
    crop: (f32, f32),
    sensor: Option<(f32, f32)>,
    n: usize,
) -> Vec<f32> {
    if n < 2 {
        return Vec::new();
    }
    let Some(q) = sony_radius_polynomial(native) else {
        // Too few knots to fit four terms: keep them as they are.
        return native.to_vec();
    };
    let (rho, rmin) = crop_in_sensor(crop, sensor);
    let z = sony_render_zoom(&q, rho, rmin);
    (0..n)
        .map(|j| {
            let r = (j as f64 + 0.5) / (n - 1) as f64;
            even_poly(&q, z * rho * r) as f32
        })
        .collect()
}

/// `(ρ, rmin)` for a default crop cut from `sensor`: `ρ = |crop| / |sensor|`,
/// the crop's half-diagonal in sensor half-diagonals, and `rmin` the crop's
/// short-edge midpoint in crop half-diagonals. A sensor smaller than the crop
/// is not a sensor; the crop is read as the whole array then (`ρ = 1`).
fn crop_in_sensor(crop: (f32, f32), sensor: Option<(f32, f32)>) -> (f64, f64) {
    let diag = |(w, h): (f32, f32)| f64::from((w * w + h * h).sqrt());
    let rho = sensor
        .map(|s| diag(crop) / diag(s).max(1e-6))
        .filter(|r| r.is_finite() && *r > 0.0 && *r <= 1.0)
        .unwrap_or(1.0);
    let rmin = (f64::from(crop.0.min(crop.1)) / diag(crop).max(1e-6)).clamp(0.0, 1.0);
    (rho, rmin)
}

/// One channel of the Sony `0x7035` chromatic-aberration pair (red's sixteen
/// knots, or blue's) → the engine's CA spline, read the way Adobe Camera Raw
/// reads it (2026-10-28).
///
/// The law, from Adobe DNG Converter 18.7, which writes Camera Raw's reading
/// of the pair into the file as a WarpRectilinear opcode (red and blue planes;
/// green the identity) centred on the sensor — reproduced, not fitted:
///
/// * the knots sit where the distortion knots sit, `t_i = i/(n−1)` of the
///   SENSOR half-diagonal, and Camera Raw replaces them by the same
///   least-squares odd polynomial `t·P(t)` through the knots `i ≥ 1` that
///   [`sony_render_distortion`] fits — no zoom of its own;
/// * `t` is the radius of the GREEN sample in the raw frame, where the
///   opcode runs before the distortion map: the red sample for an output
///   pixel whose green sample lies at `t` lies at `t·P(t)`.
///
/// Measured: the polynomial matches the DNG's planes within 7.6e-6 of a
/// factor over the whole radius on ten channels of five real frames; the old
/// reading (knots at `(i+0.5)/(n−1)` of the output radius) missed them by
/// 2.9e-5 to 1.1e-4 — up to 0.1 px at a 61 MP corner, the colour fringe a
/// registration of Camera Raw's render against the engine's measured.
///
/// The engine multiplies its green map by this spline at the OUTPUT radius
/// (`render::apply_lens_geometry`), so knot `j` is `P` at the green source
/// radius of canonical output radius `r_j`: `t_j = Z·ρ·r_j·Q(Z·ρ·r_j)` under
/// Camera Raw's distortion map ([`sony_render_distortion`]'s `Q` and `Z`),
/// `ρ·r_j` for a frame with no distortion knots. That bakes the distortion
/// into the CA spline: with the distortion toggled off the spline is read a
/// few per cent of radius off, which moves `P` by under 1e-5 on these lenses.
pub(crate) fn sony_render_ca(
    native: &[f32],
    distortion: &[f32],
    crop: (f32, f32),
    sensor: Option<(f32, f32)>,
    n: usize,
) -> Vec<f32> {
    if n < 2 {
        return Vec::new();
    }
    let Some(p) = sony_radius_polynomial(native) else {
        return native.to_vec();
    };
    let (rho, rmin) = crop_in_sensor(crop, sensor);
    let green = sony_radius_polynomial(distortion).map(|q| (sony_render_zoom(&q, rho, rmin), q));
    (0..n)
        .map(|j| {
            let u = rho * (j as f64 + 0.5) / (n - 1) as f64;
            let t = match &green {
                Some((z, q)) => z * u * even_poly(q, z * u),
                None => u,
            };
            even_poly(&p, t) as f32
        })
        .collect()
}

/// `Q`'s coefficients `(c₁, c₃, c₅, c₇)` of [`sony_render_distortion`]: the
/// least-squares fit of `t·K_i` by `c₁t + c₃t³ + c₅t⁵ + c₇t⁷` at the knots
/// `t_i = i/(n−1)`, `i ≥ 1`, solved by the normal equations in f64. `None`
/// for fewer than five knots (four terms need four points beyond the centre)
/// or a singular system.
fn sony_radius_polynomial(knots: &[f32]) -> Option<[f64; 4]> {
    let n = knots.len();
    if n < 5 {
        return None;
    }
    let mut a = [[0f64; 5]; 4]; // the normal equations, augmented
    for (i, k) in knots.iter().enumerate().skip(1) {
        let t = i as f64 / (n - 1) as f64;
        let basis = [t, t.powi(3), t.powi(5), t.powi(7)];
        let y = t * f64::from(*k);
        for r in 0..4 {
            for c in 0..4 {
                a[r][c] += basis[r] * basis[c];
            }
            a[r][4] += basis[r] * y;
        }
    }
    // Gauss–Jordan with partial pivoting: a 4×4 system, symmetric positive
    // definite whenever the points are distinct.
    for col in 0..4 {
        let piv = (col..4).max_by(|&x, &y| a[x][col].abs().total_cmp(&a[y][col].abs()))?;
        a.swap(col, piv);
        if a[col][col].abs() < 1e-15 {
            return None;
        }
        let pivot = a[col];
        for (r, row) in a.iter_mut().enumerate() {
            if r != col {
                let f = row[col] / pivot[col];
                for (x, p) in row.iter_mut().zip(pivot.iter()).skip(col) {
                    *x -= f * p;
                }
            }
        }
    }
    let q = [a[0][4] / a[0][0], a[1][4] / a[1][1], a[2][4] / a[2][2], a[3][4] / a[3][3]];
    q.iter().all(|v| v.is_finite()).then_some(q)
}

/// `Q(x) = c₁ + c₃x² + c₅x⁴ + c₇x⁶`.
fn even_poly(q: &[f64; 4], x: f64) -> f64 {
    let x2 = x * x;
    q[0] + x2 * (q[1] + x2 * (q[2] + x2 * q[3]))
}

/// The zoom `Z` of [`sony_render_distortion`]: the root of
/// `Z · max Q(Z·ρ·r) = 1` over the edge radii `r ∈ [rmin, 1]`, the maximum
/// taken on a 1024-step sweep (`Q` is a smooth sextic; the sweep's error is
/// below 1e-9 of a factor). The left side rises with `Z` for every map whose
/// `t·Q(t)` is itself monotone, so bisection on a wide bracket finds it; a
/// map with no root in the bracket falls back to `Z = 1 / Q(ρ)`, the zoom
/// that pins the corner.
fn sony_render_zoom(q: &[f64; 4], rho: f64, rmin: f64) -> f64 {
    const SWEEP: usize = 1024;
    let edge_max = |z: f64| -> f64 {
        (0..=SWEEP)
            .map(|i| rmin + (1.0 - rmin) * i as f64 / SWEEP as f64)
            .map(|r| even_poly(q, z * rho * r))
            .fold(f64::MIN, f64::max)
    };
    let h = |z: f64| z * edge_max(z) - 1.0;
    let (mut a, mut b) = (0.25f64, 4.0f64);
    if h(a) > 0.0 || h(b) < 0.0 {
        return 1.0 / even_poly(q, rho).max(1e-3);
    }
    for _ in 0..80 {
        let m = 0.5 * (a + b);
        if h(m) < 0.0 {
            a = m;
        } else {
            b = m;
        }
    }
    0.5 * (a + b)
}

/// Resample the Sony distortion spline from its native `(i+1)/n` radii onto
/// the canonical grid consumed by `render::profile_knot_interp`.
pub(crate) fn resample_sony_distortion(native: &[f32], n: usize) -> Vec<f32> {
    if native.is_empty() || n < 2 {
        return Vec::new();
    }
    (0..n)
        .map(|i| {
            let r = (i as f32 + 0.5) / (n - 1) as f32;
            sony_distortion_interp(native, r)
        })
        .collect()
}

/// Sony `0x7037` piecewise-linear interpolation at native radius `(i+1)/n`,
/// clamped before the first and after the last sample.
fn sony_distortion_interp(knots: &[f32], r: f32) -> f32 {
    let n = knots.len();
    if n == 0 {
        return 1.0;
    }
    if n == 1 {
        return knots[0];
    }
    let t = r * n as f32 - 1.0;
    if t <= 0.0 {
        return knots[0];
    }
    if t >= (n - 1) as f32 {
        return knots[n - 1];
    }
    let i = t.floor() as usize;
    let f = t - i as f32;
    knots[i] * (1.0 - f) + knots[i + 1] * f
}

/// Full-raw-frame centre expressed in the stored/default-crop frame.
/// Requires both first-party facts; missing either preserves legacy centre
/// behaviour through `LensProfile::mask_warp_center = None`.
fn mask_warp_center(
    root: &rawler::formats::tiff::IFD,
    stored_dims: Option<(f32, f32)>,
) -> Option<MaskWarpCenter> {
    let pair = |value: &Value| -> Option<(f32, f32)> {
        let x = value.get_f32(0).ok().flatten()?;
        let y = value.get_f32(1).ok().flatten()?;
        (x.is_finite() && y.is_finite()).then_some((x, y))
    };
    let number = |tag: ExifTag| -> Option<f32> {
        root.get_entry_recursive(tag)?.value.get_f32(0).ok().flatten().filter(|v| v.is_finite())
    };
    let full = (number(ExifTag::ImageWidth)?, number(ExifTag::ImageHeight)?);
    let origin = pair(&root.get_entry_recursive(DngTag::DefaultCropOrigin)?.value)?;
    let centre = [full.0 * 0.5 - origin.0, full.1 * 0.5 - origin.1];
    let (stored_w, stored_h) = stored_dims?;
    (centre[0] >= 0.0 && centre[1] >= 0.0).then_some(MaskWarpCenter {
        stored_px: centre,
        stored_dims: [stored_w, stored_h],
    })
}

/// Sony vignetting knot → linear-light gain (RawTherapee's formula).
fn vignette_gain(v: i16) -> f32 {
    1.0 / (0.5 - (v as f32 * (-13f32).exp2() - 1.0).exp2()).exp2()
}
