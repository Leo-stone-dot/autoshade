//! Adobe `.lcp` lens profiles → the engine's MASK WARP map.
//!
//! # What this is for
//!
//! Lightroom rasterises a BRUSH mask in the frame the lens correction has not
//! been applied to yet, and draws a RADIAL/LINEAR shape in the frame after it
//! (R29 Batch-8 + the 2026-08-20 `D` adjudication). Either way the quantity
//! that separates the two frames is one radial magnification `m(r)` — the map
//! from a stored mask radius to where that radius LANDS in Lightroom's own
//! export. This module produces `m` as a knot vector shaped exactly like
//! [`crate::recipe::LensProfile::distortion`], from Adobe's own profile file,
//! for bodies whose RAWs carry no in-camera correction knots.
//!
//! [`crate::render::mask_warp_from_camera_knots`] produces the SAME vector from
//! the in-camera knots (source A); this module is source B. Both write
//! [`crate::recipe::LensProfile::mask_warp`] and stamp
//! [`crate::recipe::MaskWarpSource`] so a reader can always tell which one — or
//! why neither — produced the map in front of them.
//!
//! # The model, and what is READ rather than derived
//!
//! Each `<rdf:li>` in `photoshop:CameraProfiles` holds one calibration node.
//! The distortion half is Adobe's `PerspectiveModel`:
//!
//! ```text
//! P(rho) = 1 + k1·rho² + k2·rho⁴ + k3·rho⁶          rho = r_out / D
//! r_raw  = S · r_out · P(rho_out)                    (the RENDER map)
//! ```
//!
//! `D` is the model's reference length in pixels: `FocalLengthX · W` when the
//! file states it, else `f_mm · SensorFormatFactor / 36 · W`. **The file's
//! `FocalLengthX` wins whenever present** — the focal fallback is an estimate
//! of the same quantity and the two disagree by more than the map's own
//! precision on real profiles.
//!
//! A mask needs the INVERSE of the render map (stored radius → exported
//! radius), which [`invert_p`] gets by Newton on the monotone `x·P(x)`.
//!
//! **`ScaleFactor` is READ, never derived.** On the profile this batch was
//! calibrated against — `SONY (Sony FE 24-105mm F4 G OSS) - RAW.lcp` — `S`
//! equals `1 / max_boundary P` to 2.2e-4 at all five infinity-focus nodes,
//! which is decisive enough to look like a law. It is not one: the SAME file's
//! close-focus 51 mm node misses that identity by 2.0e-3, an order of magnitude
//! worse, and the pool's median is ~1 %. Deriving `S` would therefore be right
//! on the frames this batch measured and wrong elsewhere, silently.
//! `parser_self_check_holds_at_the_far_focus_nodes` pins both halves.
//!
//! # Both spellings, and why an attribute-only parser is not enough
//!
//! Adobe writes the same properties two ways and both ship in the same
//! installed pool (measured on this machine, 3,576 files):
//! `stCamera:RadialDistortParam1="…"` as an ATTRIBUTE in 3,344 files, and
//! `<stCamera:RadialDistortParam1>…</stCamera:RadialDistortParam1>` as an
//! ELEMENT in 159 (`rdf:parseType="Resource"` entries — the phone profiles).
//! The remaining 73 carry a `FisheyeModel` and no rectilinear model at all.
//!
//! The element form nests `ChromaticGreenModel` / `ChromaticRedGreenModel` /
//! `ChromaticBlueGreenModel` / `VignetteModel` INSIDE `PerspectiveModel`, and
//! those children repeat `FocalLengthX`, `ImageXCenter` and
//! `RadialDistortParam1` with their own values. A parser that searched the
//! whole entry would read a chromatic sub-model's coefficients as the
//! geometry's — so the scope is cut at the first nested sub-model
//! ([`perspective_scope`]), and `both_spellings_read_the_same_geometry` pins it.
//!
//! # Fisheye entries are REFUSED, not approximated
//!
//! A `FisheyeModel` is a different projection, not a rectilinear model with
//! larger coefficients. Applying `P` to one would move every mask on such a
//! photo by a plausible-looking, wholly invented amount. Those entries are
//! skipped and, when a profile has nothing else, the whole file is refused with
//! [`Refusal::Fisheye`] — a named degradation, never a silent identity.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

/// One calibration node's rectilinear distortion model.
///
/// `focal_mm` is `Option` because prime and phone profiles omit
/// `stCamera:FocalLength` entirely; such a file is a single node and
/// [`LensProfileFile::model_at`] hands it back whatever focal it is asked for.
#[derive(Debug, Clone, PartialEq)]
pub struct PerspectiveModel {
    pub focal_mm: Option<f32>,
    /// `stCamera:FocusDistance` — carried for the node TIE-BREAK only (see
    /// [`LensProfileFile::nodes`]), never used in the map.
    pub focus_distance: Option<f32>,
    /// `stCamera:ScaleFactor`, verbatim. Absent ⇒ 1.0 (no zoom), which is what
    /// a file that states no scale is saying.
    pub scale: f32,
    /// `RadialDistortParam1..3`. Absent params are 0.0 — a lower-order model,
    /// not a missing one (the phone profiles ship `k1` alone).
    pub k: [f32; 3],
    /// `stCamera:FocalLengthX` as a FRACTION OF THE WIDTH. When present this is
    /// the reference length and the focal fallback is not consulted.
    pub focal_x: Option<f32>,
    /// `stCamera:SensorFormatFactor`; 1.0 when the file omits it.
    pub sensor_format_factor: f32,
    /// This node's `VignetteModel`, when it carries one. Absent on 1,709 of
    /// the 3,576 profiles installed on this machine — a distortion-only
    /// profile is ordinary, not damaged.
    pub vignette: Option<VignetteModel>,
}

/// Adobe's vignette model for one node: `V(rho) = 1 + a1·rho² + a2·rho⁴ +
/// a3·rho⁶`, on the same normalised radius the distortion half uses.
///
/// **`V` is the FALLOFF — what the lens did to the light — so the CORRECTION
/// is its RECIPROCAL.** That is measured, not assumed: over the 126,049
/// vignette nodes in this machine's Adobe pool that belong to
/// interchangeable-lens profiles, `V` at a 3:2 frame's corner has median
/// 0.7751 and sits below 1 on 94.7 % of them (min 0.0569, p10 0.4546, p90
/// 0.9603). The reciprocal's median, 1.29, is +0.37 EV of corner lift and its
/// p10, 2.20, is +1.14 EV — which is what a fast lens wide open actually
/// needs. Read the other way round the profile would DARKEN every corner it
/// touched, and by up to 4 stops.
///
/// `focal_x` is this block's own `stCamera:FocalLengthX`, present on 21,917 of
/// those 172,260 nodes; when absent the node's perspective reference length
/// answers, which is the same quantity measured by the same file.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VignetteModel {
    pub a: [f32; 3],
    pub focal_x: Option<f32>,
}

/// One `VignetteModel` with the shot it was calibrated at — EVERY entry that
/// carries one, whatever its focus distance and aperture.
///
/// Separate from [`LensProfileFile::nodes`] because the two are chosen by
/// different rules. The distortion nodes are one per focal (the calibrated
/// Sony file states identical distortion at every aperture), but vignetting is
/// a function of the aperture and the focus distance as well, and the
/// calibrated FE 24–105 file states it ONLY in its close-focus block: keeping
/// one node per focal dropped every vignette model that file has.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VignetteNode {
    pub focal_mm: Option<f32>,
    /// `stCamera:FocusDistance`, metres.
    pub focus_distance: Option<f32>,
    /// `stCamera:ApertureValue`, APEX: `Av = 2·log2(N)`.
    pub aperture_av: Option<f32>,
    pub sensor_format_factor: f32,
    pub model: VignetteModel,
}

/// What a profile's vignetting is interpolated by: the shot's focal length,
/// aperture and focus distance.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Shot {
    pub focal_mm: Option<f32>,
    /// APEX aperture value, `2·log2(N)`.
    pub aperture_av: Option<f32>,
    /// Metres. Sony ARWs state none; a sidecar's ApproximateFocusDistance can.
    pub focus_distance_m: Option<f32>,
}

impl Shot {
    /// The APEX value of an f-number, `None` for anything that is not one.
    pub fn av_of_f_number(n: f32) -> Option<f32> {
        (n.is_finite() && n > 0.0).then(|| 2.0 * n.log2())
    }
}

/// One parsed `.lcp`: the nodes it holds and who it is for.
#[derive(Debug, Clone, PartialEq)]
pub struct LensProfileFile {
    pub make: String,
    pub lens: String,
    /// Distortion nodes, one per distinct `FocalLength`, sorted ascending.
    ///
    /// **One node per focal, tie-broken on the LARGEST `FocusDistance`.** The
    /// calibrated Sony file holds seven entries per focal (one per aperture,
    /// all carrying identical distortion coefficients — verified: 1 distinct
    /// `(S, k1, k2, k3)` per focal) plus a whole close-focus block at
    /// `FocusDistance="3"`. Keying on focal alone would let document order pick
    /// the close-focus model for a focal that also has an infinity one; the
    /// tie-break makes the choice a stated rule instead of an accident.
    pub nodes: Vec<PerspectiveModel>,
    /// Every vignette model the file states ([`VignetteNode`]), in document
    /// order — read by [`LensProfileFile::vignette_gains`].
    pub vignette_nodes: Vec<VignetteNode>,
    /// How many entries were skipped for carrying a `FisheyeModel`. Non-zero
    /// with an empty `nodes` is what [`Refusal::Fisheye`] is made of.
    pub fisheye_entries: usize,
    /// The profile's own name (`stCamera:ProfileName`), the identity a
    /// sidecar names it by (crs:LensProfileName). Empty when it states none.
    pub profile_name: String,
    /// The file it was read from, by name; empty from [`parse`], set by
    /// [`open`] and [`open_named`].
    pub filename: String,
}

/// Why no `.lcp` map is available — the NAMED half of a named degradation.
///
/// Every variant means the same pixels (an identity warp); they differ in what
/// the photographer can do about it, which is the whole reason they are not one
/// `None`. Mirrors the [`crate::xmp::MaskImportReason`] idiom: an exhaustive
/// [`Refusal::ALL`] that the iteration surfaces walk, and an [`Refusal::en`]
/// whose match stops the BUILD when a variant is added.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Refusal {
    /// No Adobe profile directory exists on this machine. On Windows that means
    /// Camera Raw was never installed; on every other platform it is the
    /// ordinary state, because the roots are Windows paths
    /// ([`roots`]) — the reader degrades, it does not pretend.
    NoRoots,
    /// The roots exist but hold no profile for this camera + lens.
    NotFound,
    /// A file was named or matched and could not be read off the disk.
    Unreadable,
    /// The file parsed and holds ONLY fisheye entries. Refused rather than
    /// approximated with the rectilinear polynomial (see the module header).
    Fisheye,
    /// The file parsed and holds no distortion model of any kind — a
    /// vignette-only profile.
    NoPerspectiveModel,
    /// A model was found but the shot states no focal length and the file needs
    /// one to choose a node (a zoom profile with more than one node).
    NoFocalLength,
    /// The chosen model's Newton inverse did not converge, or its reference
    /// length is not a positive number. A crafted or corrupt profile lands
    /// here rather than producing a map nobody can defend.
    Unsolvable,
}

impl Refusal {
    /// Every refusal — the ONE list the disclosure surfaces iterate, exactly as
    /// [`crate::xmp::MaskImportReason::ALL`] serves the mask half. Pinned by
    /// `refusal_all_covers_every_variant`.
    pub const ALL: [Refusal; 7] = [
        Refusal::NoRoots,
        Refusal::NotFound,
        Refusal::Unreadable,
        Refusal::Fisheye,
        Refusal::NoPerspectiveModel,
        Refusal::NoFocalLength,
        Refusal::Unsolvable,
    ];

    /// English label for the prose channel (CLI stderr / batch warnings).
    pub fn en(self) -> &'static str {
        match self {
            Refusal::NoRoots => "no Adobe lens-profile directory on this machine",
            Refusal::NotFound => "no Adobe lens profile for this camera and lens",
            Refusal::Unreadable => "the Adobe lens profile could not be read",
            Refusal::Fisheye => "the Adobe lens profile is a fisheye model - refused, not applied",
            Refusal::NoPerspectiveModel => "the Adobe lens profile carries no distortion model",
            Refusal::NoFocalLength => "the photo states no focal length to pick a profile node",
            Refusal::Unsolvable => "the Adobe lens profile's distortion model did not invert",
        }
    }
}

/// The refusal a RECIPE records. Seven reader-side reasons fold onto five
/// stored ones, and each fold is a statement rather than a rounding:
///
/// * `NotFound`, `NoPerspectiveModel` and `NoFocalLength` all mean *this photo
///   has no distortion model anyone could apply* — a profile that models only
///   vignetting is, for a mask frame, the same news as no profile at all.
/// * `Unreadable` and `Unsolvable` both mean *a model was there and this build
///   could not use it*, which is the one state worth a bug report.
/// * `NoRoots` and `Fisheye` keep their own variants because they are the two a
///   photographer can act on: install Camera Raw, or accept that a fisheye is
///   not going to be faked.
impl From<Refusal> for crate::recipe::MaskWarpSource {
    fn from(r: Refusal) -> Self {
        use crate::recipe::MaskWarpSource as S;
        match r {
            Refusal::NoRoots => S::NoProfileRoots,
            Refusal::Fisheye => S::FisheyeRefused,
            Refusal::NotFound | Refusal::NoPerspectiveModel | Refusal::NoFocalLength => S::Absent,
            Refusal::Unreadable | Refusal::Unsolvable => S::Unparseable,
        }
    }
}

// --- the model -------------------------------------------------------------

/// `P(rho) = 1 + k1·rho² + k2·rho⁴ + k3·rho⁶`, in `f64`.
///
/// `f64` throughout the solve, `f32` only at the knot vector's edge: `k3`
/// reaches −8.2 on the 105 mm node and `rho⁶` at the corner multiplies it by a
/// number small enough that the single-precision cancellation is visible in the
/// fifth decimal of the answer.
fn p_poly(rho: f64, k: [f64; 3]) -> f64 {
    let s = rho * rho;
    1.0 + s * (k[0] + s * (k[1] + s * k[2]))
}

/// `d/dx [x·P(x)] = 1 + 3k1·x² + 5k2·x⁴ + 7k3·x⁶`.
fn dp_poly(rho: f64, k: [f64; 3]) -> f64 {
    let s = rho * rho;
    1.0 + s * (3.0 * k[0] + s * (5.0 * k[1] + s * 7.0 * k[2]))
}

/// Solve `x·P(x) = t` for `x` by Newton — the inverse of Adobe's render map.
///
/// Monotone on every real profile (the derivative above is positive over the
/// frame), so the root is unique and Newton from `x = t` converges in a handful
/// of steps. `None` rather than a shrug when it does not: a non-convergent
/// solve means the polynomial folds inside the frame, and a folded map has no
/// single answer to give. Pinned to 1e-6 over the frame by
/// `newton_inverse_round_trips_over_the_whole_frame`.
pub fn invert_p(t: f64, k: [f64; 3]) -> Option<f64> {
    if !t.is_finite() {
        return None;
    }
    if t == 0.0 {
        return Some(0.0);
    }
    let mut x = t;
    for _ in 0..64 {
        let f = x * p_poly(x, k) - t;
        let d = dp_poly(x, k);
        // A non-positive derivative is the fold: stop rather than step across
        // it onto a second preimage.
        if !d.is_finite() || d <= 1e-9 {
            return None;
        }
        let step = f / d;
        x -= step;
        if !x.is_finite() {
            return None;
        }
        if step.abs() <= 1e-12 * (1.0 + x.abs()) {
            break;
        }
    }
    // Verified, not assumed: a converged-looking iterate that does not satisfy
    // the equation is refused, which is what makes `Unsolvable` truthful.
    if (x * p_poly(x, k) - t).abs() > 1e-9 * (1.0 + t.abs()) {
        return None;
    }
    Some(x)
}

impl PerspectiveModel {
    /// The model's reference length `D`, in pixels of a `width_px`-wide frame.
    ///
    /// `FocalLengthX · W` when the file states it; otherwise the focal
    /// fallback `f_mm · SensorFormatFactor / 36 · W`. The file's own value WINS
    /// — see the module header.
    pub fn reference_px(&self, width_px: f32) -> Option<f32> {
        let d = match self.focal_x {
            Some(fx) if fx > 0.0 => fx * width_px,
            _ => {
                let f = self.focal_mm?;
                if f <= 0.0 {
                    return None;
                }
                f * self.sensor_format_factor / 36.0 * width_px
            }
        };
        (d.is_finite() && d > 0.0).then_some(d)
    }

    /// `1 / max P` over the frame BOUNDARY — the quantity `ScaleFactor` is
    /// observed to equal on the calibrated profile, and which this module
    /// deliberately does NOT use in place of the file's own value.
    ///
    /// Exposed only so a test can state the identity and its counter-example in
    /// the same breath (see the module header).
    pub fn boundary_scale(&self, dims: (f32, f32)) -> Option<f32> {
        let (w, h) = dims;
        let d = self.reference_px(w)? as f64;
        let k = [self.k[0] as f64, self.k[1] as f64, self.k[2] as f64];
        // The maximum of a radial polynomial over a RECTANGLE's boundary sits
        // on the boundary, so walking the two half-edges (the frame is
        // symmetric in both axes) covers it. 2,048 samples per edge puts the
        // sampling error four orders below the 3e-4 the identity is stated at.
        const N: usize = 2048;
        let mut m = f64::NEG_INFINITY;
        for i in 0..=N {
            let t = i as f64 / N as f64;
            for (x, y) in
                [(w as f64 / 2.0, t * h as f64 / 2.0), (t * w as f64 / 2.0, h as f64 / 2.0)]
            {
                let p = p_poly(x.hypot(y) / d, k);
                if p > m {
                    m = p;
                }
            }
        }
        (m.is_finite() && m > 1e-6).then(|| (1.0 / m) as f32)
    }

    /// The RENDER distortion knots for a `dims` frame: the factor the engine
    /// multiplies an OUTPUT radius by to find the source radius it samples,
    /// `f(r_out) = P(r_out / D)` — Adobe's own render map with `ScaleFactor`
    /// LEFT OUT.
    ///
    /// Leaving `S` out is the point, not an omission. `S` is Adobe's choice of
    /// how far to zoom in so the corrected frame has no empty border, and this
    /// engine makes that choice itself, once, for every map it renders
    /// (`render::profile_fill_scale`, which the in-camera knots already go
    /// through). Folding Adobe's zoom in as well would apply two. The module
    /// header records that `S ≈ 1 / max P` on the calibrated file's
    /// infinity-focus nodes — i.e. Adobe's own `S` IS essentially that fill
    /// scale — which is why dropping it and recomputing lands in the same
    /// place rather than somewhere new.
    ///
    /// Same knot placement as [`Self::mask_warp_knots`] and as the in-camera
    /// spline, for the same reason: one interpolator reads the field.
    pub fn distortion_knots(&self, dims: (f32, f32), n: usize) -> Option<Vec<f32>> {
        if n < 2 {
            return None;
        }
        let (w, h) = dims;
        if !(w > 0.0 && h > 0.0) {
            return None;
        }
        let d = self.reference_px(w)? as f64;
        let k = [self.k[0] as f64, self.k[1] as f64, self.k[2] as f64];
        let half_diag = 0.5 * ((w as f64).hypot(h as f64));
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let rho = (i as f64 + 0.5) / (n - 1) as f64 * half_diag / d;
            let f = p_poly(rho, k);
            if !f.is_finite() || f <= 0.0 {
                return None; // a folded spline is a corrupt node, not a lens
            }
            out.push(f as f32);
        }
        Some(out)
    }

    /// The MASK WARP knots for a `dims` frame: `m(r) = r_export / r_stored` at
    /// the `n` radii [`crate::recipe::LensProfile`] places its knots on —
    /// `rho_i = (i + 0.5)/(n − 1)` of the HALF-DIAGONAL, RawTherapee's
    /// placement, which is the placement the in-camera knots already use.
    ///
    /// Sharing the placement is the point: source A and source B write the same
    /// field, so exactly one interpolator reads it and the two sources cannot
    /// drift into different conventions.
    pub fn mask_warp_knots(&self, dims: (f32, f32), n: usize) -> Option<Vec<f32>> {
        if n < 2 {
            return None;
        }
        let (w, h) = dims;
        if !(w > 0.0 && h > 0.0) {
            return None;
        }
        let d = self.reference_px(w)? as f64;
        let s = self.scale as f64;
        if !(s.is_finite() && s > 1e-6) {
            return None;
        }
        let k = [self.k[0] as f64, self.k[1] as f64, self.k[2] as f64];
        let half_diag = 0.5 * ((w as f64).hypot(h as f64));
        let mut out = Vec::with_capacity(n);
        for i in 0..n {
            let rho = (i as f64 + 0.5) / (n - 1) as f64;
            let r_stored = rho * half_diag;
            // r_export = D · invP( r_stored / (S·D) ) — the inverse of
            // r_stored = S · r_export · P(r_export/D).
            let r_export = d * invert_p(r_stored / (s * d), k)?;
            if !r_export.is_finite() || r_stored <= 0.0 {
                return None;
            }
            out.push((r_export / r_stored) as f32);
        }
        Some(out)
    }
}

impl LensProfileFile {
    /// The node to use at `focal_mm`: the exact node when one matches, else a
    /// LINEAR blend of the two bracketing nodes' `(S, k1, k2, k3)`, clamped to
    /// the end node outside the range.
    ///
    /// **The blend is UNVERIFIED and registered as such.** The only frame that
    /// can discriminate blends — 34 mm, between the 24 and 35 nodes — separates
    /// a linear blend from the alternatives by 0.5 px, which is inside the
    /// 4.19 px tangential noise floor of the measurement that would judge it.
    /// Linear is chosen because it is the interpolation the sibling in-camera
    /// spline already uses (`render::profile_knot_interp`), not because it won
    /// a comparison.
    ///
    /// A single-node file (a prime, a phone) answers with that node whatever it
    /// is asked, including `None` — a profile that states one model states it
    /// for every focal it covers.
    pub fn model_at(&self, focal_mm: Option<f32>) -> Result<PerspectiveModel, Refusal> {
        match self.nodes.len() {
            0 if self.fisheye_entries > 0 => return Err(Refusal::Fisheye),
            0 => return Err(Refusal::NoPerspectiveModel),
            1 => return Ok(self.nodes[0].clone()),
            _ => {}
        }
        let Some(f) = focal_mm.filter(|v| v.is_finite() && *v > 0.0) else {
            return Err(Refusal::NoFocalLength);
        };
        // Nodes are sorted; a file that reached here has ≥2 of them, and any
        // node with no focal at all cannot take part in a focal blend.
        let known: Vec<&PerspectiveModel> = self.nodes.iter().filter(|n| n.focal_mm.is_some()).collect();
        match known.len() {
            0 => return Err(Refusal::NoFocalLength),
            1 => return Ok(known[0].clone()),
            _ => {}
        }
        let first = known[0];
        let last = known[known.len() - 1];
        if f <= first.focal_mm.unwrap_or(f) {
            return Ok(first.clone());
        }
        if f >= last.focal_mm.unwrap_or(f) {
            return Ok(last.clone());
        }
        let hi = known.iter().position(|n| n.focal_mm.unwrap_or(f) >= f).unwrap_or(known.len() - 1);
        let (a, b) = (known[hi.saturating_sub(1)], known[hi]);
        let (fa, fb) = (a.focal_mm.unwrap_or(f), b.focal_mm.unwrap_or(f));
        let t = if (fb - fa).abs() < 1e-6 { 0.0 } else { (f - fa) / (fb - fa) };
        let mix = |x: f32, y: f32| x + t * (y - x);
        Ok(PerspectiveModel {
            focal_mm: Some(f),
            // The blended node is not either endpoint's calibration state, so
            // it carries neither's focus distance.
            focus_distance: None,
            scale: mix(a.scale, b.scale),
            k: [mix(a.k[0], b.k[0]), mix(a.k[1], b.k[1]), mix(a.k[2], b.k[2])],
            // `FocalLengthX` is a LENGTH, not a coefficient: blending it would
            // invent a reference frame neither node states. Taken from the
            // nearer node, or dropped so the focal fallback answers.
            focal_x: if t < 0.5 { a.focal_x } else { b.focal_x },
            sensor_format_factor: mix(a.sensor_format_factor, b.sensor_format_factor),
            // Blended on the distortion coefficients' terms and registered as
            // just as UNVERIFIED — and only when BOTH ends state a model, so a
            // node that says nothing about vignetting cannot be averaged into
            // half a correction.
            vignette: match (a.vignette, b.vignette) {
                (Some(x), Some(y)) => Some(VignetteModel {
                    a: [mix(x.a[0], y.a[0]), mix(x.a[1], y.a[1]), mix(x.a[2], y.a[2])],
                    focal_x: if t < 0.5 { x.focal_x } else { y.focal_x },
                }),
                _ => None,
            },
        })
    }
}

impl LensProfileFile {
    /// The render VIGNETTE GAINS for a shot, read the way Adobe Camera Raw
    /// reads the profile (2026-10-28) — `n` gains at radii `(i+0.5)/(n−1)` of
    /// the `crop` frame's half-diagonal, in the frame BEFORE the lens geometry
    /// (where `render::develop` applies them), each `1 / V(x)`.
    ///
    /// The law, measured on flat-field frames (the raw data replaced by a
    /// uniform field and read against its own raw-gain ladder, so no tone
    /// curve enters) through Camera Raw, every reading below chosen among its
    /// alternatives by those renders:
    ///
    /// * `x` is the RAW-frame radius over the node's focal length in pixels,
    ///   `FocalLength · SensorFormatFactor / 36` of the SENSOR's width
    ///   (`sensor_width`; the default crop's width read 1.5 % worse), or the
    ///   block's own `FocalLengthX` of that width when it states one;
    /// * nodes are blended in EV (`log2` of the gain), never in coefficients:
    ///   linearly in focal length between the bracketing focal nodes (at 60
    ///   mm between 51 and 70 the linear weight fits within 0.08 %, the
    ///   reciprocal 0.16 %), linearly in the APEX aperture between the
    ///   bracketing apertures (f/4.5 between f/4 and f/5.6: 0.22 %, linear in
    ///   the f-number 0.63 %), and linearly in the reciprocal focus distance
    ///   between the bracketing distances (3 m and 30 m between 1.4 m and
    ///   infinity: 0.18 %, linear in metres 0.29 % and 0.57 %);
    /// * with no focus distance — every Sony ARW measured, which states none
    ///   — the closest-focus distance the profile has; a sidecar's
    ///   ApproximateFocusDistance moved Camera Raw onto that distance's
    ///   nodes;
    /// * outside the range of any dimension the nearest end (not measured,
    ///   and the only reading that invents nothing).
    ///
    /// Measured agreement on an FE 24–105 frame (80 mm f/4, and the probes at
    /// 60 mm and at f/4.5) and a 14–24 mm frame (20.1 mm f/2.8, with no
    /// distance and at 3 m): within 0.22 % of Camera Raw's gain out to 0.94 of
    /// the half-diagonal, 0.5–0.6 % in the extreme corner.
    ///
    /// `None` when the file states no vignetting, when the shot leaves a
    /// dimension the file varies unanswered (two focal or aperture nodes and
    /// no focal or aperture to choose between them), or when a model leaves
    /// the plausible band anywhere on the frame — the pool's tail holds nodes
    /// whose coefficients run to 1e12.
    pub fn vignette_gains(&self, shot: Shot, crop: (f32, f32), sensor_width: f32, n: usize) -> Option<Vec<f32>> {
        let (w, h) = crop;
        if n < 2 || !(w > 0.0 && h > 0.0 && sensor_width > 0.0 && sensor_width.is_finite()) {
            return None;
        }
        let nodes = &self.vignette_nodes;
        if nodes.is_empty() {
            return None;
        }
        let half_diag = 0.5 * f64::from(w).hypot(f64::from(h));
        let radii: Vec<f64> = (0..n).map(|i| (i as f64 + 0.5) / (n - 1) as f64 * half_diag).collect();
        // One node's EV at every radius.
        let ev = |v: &VignetteNode| -> Option<Vec<f64>> {
            let d = match v.model.focal_x {
                Some(fx) if fx > 0.0 => f64::from(fx) * f64::from(sensor_width),
                _ => f64::from(v.focal_mm?) * f64::from(v.sensor_format_factor) / 36.0 * f64::from(sensor_width),
            };
            if !(d.is_finite() && d > 0.0) {
                return None;
            }
            let a = v.model.a.map(f64::from);
            radii
                .iter()
                .map(|r| {
                    let x2 = (r / d) * (r / d);
                    let falloff = 1.0 + x2 * (a[0] + x2 * (a[1] + x2 * a[2]));
                    (falloff.is_finite() && (0.05..=20.0).contains(&falloff)).then(|| -falloff.log2())
                })
                .collect()
        };
        // Focus distance: the reciprocal blend, or the closest focus.
        let mut fds: Vec<f32> = nodes.iter().map(|v| v.focus_distance.unwrap_or(f32::INFINITY)).collect();
        fds.sort_by(f32::total_cmp);
        fds.dedup();
        let fd_set = match shot.focus_distance_m.filter(|d| d.is_finite() && *d > 0.0) {
            Some(d) => bracket(&fds, d, |x| if x.is_finite() { 1.0 / f64::from(x) } else { 0.0 }),
            None => vec![(fds[0], 1.0)],
        };
        let mut total = vec![0f64; n];
        for (fd, wd) in fd_set {
            let at_fd: Vec<&VignetteNode> =
                nodes.iter().filter(|v| v.focus_distance.unwrap_or(f32::INFINITY) == fd).collect();
            let mut focals: Vec<f32> = at_fd.iter().map(|v| v.focal_mm.unwrap_or(0.0)).collect();
            focals.sort_by(f32::total_cmp);
            focals.dedup();
            let f_set = match (focals.len(), shot.focal_mm.filter(|f| f.is_finite() && *f > 0.0)) {
                (1, _) => vec![(focals[0], 1.0)],
                (_, Some(f)) => bracket(&focals, f, f64::from),
                (_, None) => return None,
            };
            for (f, wf) in f_set {
                let at_f: Vec<&&VignetteNode> = at_fd.iter().filter(|v| v.focal_mm.unwrap_or(0.0) == f).collect();
                let mut avs: Vec<f32> = at_f.iter().map(|v| v.aperture_av.unwrap_or(0.0)).collect();
                avs.sort_by(f32::total_cmp);
                avs.dedup();
                let a_set = match (avs.len(), shot.aperture_av.filter(|a| a.is_finite())) {
                    (1, _) => vec![(avs[0], 1.0)],
                    (_, Some(a)) => bracket(&avs, a, f64::from),
                    (_, None) => return None,
                };
                for (av, wa) in a_set {
                    let node = at_f.iter().find(|v| v.aperture_av.unwrap_or(0.0) == av)?;
                    let e = ev(node)?;
                    let wt = wd * wf * wa;
                    for (t, x) in total.iter_mut().zip(&e) {
                        *t += wt * x;
                    }
                }
            }
        }
        Some(total.iter().map(|e| e.exp2() as f32).collect())
    }
}

/// The bracketing pair of `x` in the sorted, distinct `vals` with its weights,
/// linear in `key(value)`; a value outside the range is the nearest end, and a
/// value on a node is that node alone.
fn bracket(vals: &[f32], x: f32, key: impl Fn(f32) -> f64) -> Vec<(f32, f64)> {
    let (first, last) = (vals[0], vals[vals.len() - 1]);
    if x <= first {
        return vec![(first, 1.0)];
    }
    if x >= last {
        return vec![(last, 1.0)];
    }
    let hi = vals.partition_point(|v| *v < x);
    let (a, b) = (vals[hi - 1], vals[hi]);
    if b == x {
        return vec![(b, 1.0)];
    }
    let t = (key(x) - key(a)) / (key(b) - key(a));
    if !t.is_finite() {
        return vec![(a, 1.0)];
    }
    vec![(a, 1.0 - t), (b, t)]
}

// --- parsing ---------------------------------------------------------------

/// Read one `stCamera:` property in EITHER spelling from `scope`.
///
/// Attribute first (the majority form), then the element form. Both are looked
/// for in the SAME scope, so a caller that has already cut the scope correctly
/// cannot pick up a nested sub-model's copy of the name.
fn prop(scope: &str, key: &str) -> Option<String> {
    let attr = format!("stCamera:{key}");
    let mut from = 0;
    while let Some(i) = scope[from..].find(&attr) {
        let at = from + i;
        let rest = &scope[at + attr.len()..];
        // The name must END here: `FocalLength` must not match
        // `FocalLengthX`, which is a different property on the same element.
        let mut it = rest.char_indices().skip_while(|(_, c)| c.is_whitespace());
        match it.next() {
            Some((j, '=')) => {
                let after = &rest[j + 1..];
                let q = after.find('"')?;
                let end = after[q + 1..].find('"')?;
                return Some(after[q + 1..q + 1 + end].to_string());
            }
            Some((_, '>')) => {
                let open_end = at + attr.len() + rest.find('>')? + 1;
                let close = format!("</stCamera:{key}>");
                let end = scope[open_end..].find(&close)?;
                return Some(scope[open_end..open_end + end].trim().to_string());
            }
            _ => {}
        }
        from = at + attr.len();
    }
    None
}

fn prop_f32(scope: &str, key: &str) -> Option<f32> {
    prop(scope, key)?.trim().parse::<f32>().ok().filter(|v| v.is_finite())
}

/// The `PerspectiveModel` block inside one entry, CUT AT the first nested
/// sub-model.
///
/// The cut is the whole reason this is a function. In the element spelling the
/// chromatic and vignette sub-models live INSIDE `PerspectiveModel` and repeat
/// `FocalLengthX`, `ImageXCenter` and `RadialDistortParam1` with their own
/// values; reading the enclosing element whole would take a chromatic
/// channel's coefficients for the geometry's.
fn perspective_scope(entry: &str) -> Option<&str> {
    const OPEN: &str = "<stCamera:PerspectiveModel";
    let start = entry.find(OPEN)?;
    let rest = &entry[start..];
    let tag_end = rest.find('>')?;
    // Self-closing attribute form: the tag IS the scope.
    if rest[..tag_end].ends_with('/') {
        return Some(&rest[..=tag_end]);
    }
    let close = rest.find("</stCamera:PerspectiveModel>").unwrap_or(rest.len());
    let mut scope = &rest[..close];
    for nested in [
        "<stCamera:ChromaticGreenModel",
        "<stCamera:ChromaticRedGreenModel",
        "<stCamera:ChromaticBlueGreenModel",
        "<stCamera:VignetteModel",
    ] {
        if let Some(i) = scope.find(nested) {
            scope = &scope[..i];
        }
    }
    Some(scope)
}

/// The `VignetteModel` block inside one entry, in either spelling.
///
/// Cut the same way and for the same reason as [`perspective_scope`]: the
/// block repeats `FocalLengthX` and `RadialDistortParam1` with values of its
/// own, so reading either from the enclosing element would cross the two
/// models. Nothing nests inside a vignette model, so its own close tag (or the
/// self-closing form) is the whole cut.
///
/// The closing-tag arm is a BOUND that currently never binds, and that is
/// measured rather than assumed: over the 243,394 vignette blocks in this
/// machine's 3,576 installed profiles the cut shortens the scope 7,101 times
/// and changes a read value ZERO times, because Adobe writes the enclosing
/// model's own properties before the nested one. It stays because it is the
/// correct scope for one `find` — an unbounded read would start depending on
/// that ordering — but no mutation of it can falsify anything, which is why the
/// falsification set points elsewhere in this module.
fn vignette_scope(entry: &str) -> Option<&str> {
    const OPEN: &str = "<stCamera:VignetteModel";
    let start = entry.find(OPEN)?;
    let rest = &entry[start..];
    let tag_end = rest.find('>')?;
    if rest[..tag_end].ends_with('/') {
        return Some(&rest[..=tag_end]);
    }
    let close = rest.find("</stCamera:VignetteModel>").unwrap_or(rest.len());
    Some(&rest[..close])
}

/// Parse an `.lcp` document.
///
/// Hand-rolled string scanning rather than an XML dependency, for the same
/// reason [`crate::xmp`] is: these files are machine-written by one producer in
/// two known shapes, and the properties this module needs are a closed list of
/// eleven names. What that costs is stated where it is paid — the scope cut in
/// [`perspective_scope`] is the one place a real XML parser would have been
/// free.
pub fn parse(xml: &str) -> Result<LensProfileFile, Refusal> {
    let mut make = String::new();
    let mut lens = String::new();
    let mut profile_name = String::new();
    let mut nodes: Vec<PerspectiveModel> = Vec::new();
    let mut vignette_nodes: Vec<VignetteNode> = Vec::new();
    let mut fisheye_entries = 0usize;
    let mut from = 0usize;
    while let Some(i) = xml[from..].find("<rdf:li") {
        let at = from + i;
        let Some(close) = xml[at..].find("</rdf:li>") else { break };
        let entry = &xml[at..at + close];
        from = at + close + "</rdf:li>".len();
        // Head = everything before the first model block: `FocalLength`,
        // `Make`, `Lens` and friends live there, and never inside a model, so
        // this keeps the entry-level reads out of the sub-model namespace too.
        let head_end = entry
            .find("<stCamera:PerspectiveModel")
            .or_else(|| entry.find("<stCamera:FisheyeModel"))
            .unwrap_or(entry.len());
        let head = &entry[..head_end];
        if make.is_empty() {
            make = prop(head, "Make").unwrap_or_default();
        }
        if lens.is_empty() {
            lens = prop(head, "LensPrettyName").or_else(|| prop(head, "Lens")).unwrap_or_default();
        }
        if profile_name.is_empty() {
            profile_name = prop(head, "ProfileName").unwrap_or_default();
        }
        // A fisheye entry is COUNTED and skipped — see the module header. The
        // check comes first: a file carrying both models for one entry would
        // otherwise be read as rectilinear on the strength of a sibling tag.
        if entry.contains("<stCamera:FisheyeModel") {
            fisheye_entries += 1;
            continue;
        }
        let Some(scope) = perspective_scope(entry) else { continue };
        // The vignette block is cut OUT of `scope` above (it repeats
        // `FocalLengthX` and `RadialDistortParam1` with its own values), so it
        // is read from the entry with its own cut.
        let vignette = vignette_scope(entry).and_then(|v| {
            Some(VignetteModel {
                a: [
                    prop_f32(v, "VignetteModelParam1")?,
                    prop_f32(v, "VignetteModelParam2").unwrap_or(0.0),
                    prop_f32(v, "VignetteModelParam3").unwrap_or(0.0),
                ],
                focal_x: prop_f32(v, "FocalLengthX").filter(|x| *x > 0.0),
            })
        });
        let sensor_format_factor = prop_f32(head, "SensorFormatFactor").filter(|v| *v > 0.0).unwrap_or(1.0);
        if let Some(model) = vignette {
            vignette_nodes.push(VignetteNode {
                focal_mm: prop_f32(head, "FocalLength").filter(|v| *v > 0.0),
                focus_distance: prop_f32(head, "FocusDistance").filter(|v| *v > 0.0),
                aperture_av: prop_f32(head, "ApertureValue"),
                sensor_format_factor,
                model,
            });
        }
        // No `k1` at all = no distortion model in this entry (a vignette-only
        // node). Not a refusal by itself; the file may hold others.
        let Some(k1) = prop_f32(scope, "RadialDistortParam1") else { continue };
        nodes.push(PerspectiveModel {
            focal_mm: prop_f32(head, "FocalLength").filter(|v| *v > 0.0),
            focus_distance: prop_f32(head, "FocusDistance"),
            scale: prop_f32(scope, "ScaleFactor").filter(|v| *v > 0.0).unwrap_or(1.0),
            k: [k1, prop_f32(scope, "RadialDistortParam2").unwrap_or(0.0), prop_f32(scope, "RadialDistortParam3").unwrap_or(0.0)],
            focal_x: prop_f32(scope, "FocalLengthX").filter(|v| *v > 0.0),
            sensor_format_factor,
            vignette,
        });
    }
    // A file with vignette models and no distortion one still renders its
    // vignetting; `model_at` refuses its distortion by name.
    if nodes.is_empty() && vignette_nodes.is_empty() {
        return Err(if fisheye_entries > 0 { Refusal::Fisheye } else { Refusal::NoPerspectiveModel });
    }
    // One node per focal, largest focus distance wins (see `nodes`). Sorted by
    // focal so `model_at`'s bracket search is a scan.
    nodes.sort_by(|a, b| {
        a.focal_mm
            .unwrap_or(0.0)
            .total_cmp(&b.focal_mm.unwrap_or(0.0))
            .then(b.focus_distance.unwrap_or(0.0).total_cmp(&a.focus_distance.unwrap_or(0.0)))
    });
    nodes.dedup_by(|a, b| a.focal_mm == b.focal_mm);
    Ok(LensProfileFile { make, lens, nodes, vignette_nodes, fisheye_entries, profile_name, filename: String::new() })
}

// --- discovery -------------------------------------------------------------

/// Where Adobe keeps lens profiles.
///
/// The environment reading, the macOS literals and the `is_dir` filter live in
/// [`crate::adobe::camera_raw_roots`] — camera profiles ([`crate::dcp`]) are
/// found in a sibling directory of the same tree by the same rules, and one
/// copy of those rules cannot drift from the other.
///
/// `%AUTOSHADE_LCP_DIR%` is consulted FIRST when set: it is how the acceptance
/// tests point at a fixture without an Adobe install, and how a user with
/// profiles somewhere else names that place.
///
/// On a build with neither Adobe variable in the environment this answers empty
/// and every caller degrades through [`Refusal::NoRoots`].
pub fn roots() -> Vec<PathBuf> {
    crate::adobe::camera_raw_roots("AUTOSHADE_LCP_DIR", "LensProfiles")
}

/// Every `.lcp` under [`roots`], walked ONCE per process.
///
/// The installed pool is ~3,600 files across ~40 vendor directories; walking it
/// per photograph in a `batch --jobs 3` run would be three redundant directory
/// trees per frame. `OnceLock` because the answer is a property of the machine,
/// not of the run — and because a `batch` worker must not have to hold a lock
/// to ask.
///
/// **Deliberately NOT invalidated.** Installing a lens profile mid-run and
/// expecting the run to notice is not a workflow; restarting is. A cache that
/// re-walked on a timer would trade a stated limit for an unpredictable one.
fn index() -> &'static [PathBuf] {
    static INDEX: OnceLock<Vec<PathBuf>> = OnceLock::new();
    // Bounded inside the walk: a symlink loop under a profile root must not
    // hang a render.
    INDEX.get_or_init(|| crate::adobe::walk_extension(roots(), "lcp", 20_000))
}

/// Lowercase alphanumerics only — the comparison key for a lens name.
///
/// Adobe's own spellings of one lens differ by punctuation and spacing between
/// the EXIF `LensModel` (`"FE 24-105mm F4 G OSS"`), the profile's
/// `LensPrettyName` (`"Sony FE 24-105mm F4 G OSS"`) and the FILE NAME
/// (`"SONY (Sony FE 24-105mm F4 G OSS) - RAW.lcp"`). Folding all three to
/// alphanumerics is what lets a containment test hold across them.
fn fold(s: &str) -> String {
    s.chars().filter(|c| c.is_alphanumeric()).flat_map(|c| c.to_lowercase()).collect()
}

/// Find the `.lcp` for this camera and lens.
///
/// `filename` is `crs:LensProfileFilename` when the sidecar names one — the
/// exact answer, because Lightroom wrote down the file IT used. Matching is on
/// the file NAME, case-insensitively, because the sidecar states a name and not
/// a path and the pool is vendor-partitioned.
///
/// The fallback is a containment match of the folded lens model against the
/// folded file stem, tie-broken toward the SHORTEST stem: `"FE 24-105mm F4 G
/// OSS"` is contained in both the RAW and the non-RAW profile's stem, and the
/// shorter of two matching names is the less-qualified, more general one.
/// `make` narrows it first so a Sigma profile whose name embeds a Sony body
/// cannot win on a Sony file.
pub fn locate(filename: Option<&str>, make: &str, lens: &str) -> Result<PathBuf, Refusal> {
    let files = index();
    if files.is_empty() {
        return Err(Refusal::NoRoots);
    }
    if let Some(want) = filename.map(str::trim).filter(|s| !s.is_empty()) {
        let want = fold(want);
        if let Some(hit) = files
            .iter()
            .find(|p| p.file_name().is_some_and(|n| fold(&n.to_string_lossy()) == want))
        {
            return Ok(hit.clone());
        }
    }
    let (fm, fl) = (fold(make), fold(lens));
    if fl.is_empty() {
        return Err(Refusal::NotFound);
    }
    // The profile's own lens identity first: `stCamera:Lens` is the EXIF lens
    // model the profile was made for, verbatim (the Sigma 14–24 mm's is
    // "14-24mm F2.8 DG DN | Art 019", which no FILE NAME contains — its stems
    // spell the lens "SIGMA 14-24mm F2.8 DG DN A019").
    let by_identity = choose(heads().iter().filter(|h| h.lens == fl && (fm.is_empty() || h.make.contains(&fm))));
    if let Some(h) = by_identity {
        return Ok(h.path.clone());
    }
    choose(heads().iter().filter(|h| h.stem.contains(&fl) && (fm.is_empty() || h.stem.contains(&fm))))
        .map(|h| h.path.clone())
        .ok_or(Refusal::NotFound)
}

/// What [`locate`] knows of one installed profile without parsing it: read
/// from the file's first 16 KiB, where the first entry states it.
struct Head {
    path: PathBuf,
    /// Folded file stem.
    stem: String,
    /// Folded `stCamera:Make` and `stCamera:Lens`.
    make: String,
    lens: String,
    /// `stCamera:CameraRawProfile="True"`: made from raw captures.
    raw: bool,
    /// The stem's ` vN` revision; 1 when it states none.
    version: u32,
    /// Folded `stCamera:ProfileName`.
    name: String,
}

/// [`Head`] for every file under [`roots`], read once per process (the same
/// stance as [`index`]). 3,576 files × 16 KiB is about 57 MiB of reading, paid
/// only by the first lookup that needs it.
fn heads() -> &'static [Head] {
    static HEADS: OnceLock<Vec<Head>> = OnceLock::new();
    HEADS.get_or_init(|| {
        use std::io::Read;
        index()
            .iter()
            .map(|p| {
                let mut buf = Vec::new();
                if let Ok(f) = std::fs::File::open(p) {
                    // Ignored because a short or failed read leaves a head that
                    // matches no lens, which is the right answer for that file.
                    let _ = f.take(16 * 1024).read_to_end(&mut buf);
                }
                let text = String::from_utf8_lossy(&buf);
                let stem_raw = p.file_stem().unwrap_or_default().to_string_lossy().to_string();
                Head {
                    path: p.clone(),
                    stem: fold(&stem_raw),
                    make: fold(&prop(&text, "Make").unwrap_or_default()),
                    lens: fold(&prop(&text, "Lens").unwrap_or_default()),
                    raw: prop(&text, "CameraRawProfile").is_some_and(|v| v.eq_ignore_ascii_case("true")),
                    version: profile_version(&stem_raw),
                    name: fold(&prop(&text, "ProfileName").unwrap_or_default()),
                }
            })
            .collect()
    })
}

/// The revision a profile's FILE NAME states — `"SONY (SIGMA 14-24mm F2.8 DG
/// DN A019) v3 - RAW"` is 3 — or 1 when it states none.
fn profile_version(stem: &str) -> u32 {
    stem.split(|c: char| c.is_whitespace() || c == '-' || c == ')')
        .filter_map(|t| t.strip_prefix('v').or_else(|| t.strip_prefix('V')))
        .filter_map(|d| d.parse::<u32>().ok())
        .max()
        .unwrap_or(1)
}

/// The profile Camera Raw takes among several for one lens: a RAW profile
/// over a non-RAW one, then the NEWEST revision, then the shortest (least
/// qualified) name. Measured on the Sigma 14–24 mm, installed as the plain
/// file, v2 and v3: with no profile named, Camera Raw's vignetting is v2's
/// and v3's (they agree) and 22 % off the plain file's at the corner.
fn choose<'a>(cands: impl Iterator<Item = &'a Head>) -> Option<&'a Head> {
    cands.max_by(|a, b| {
        a.raw
            .cmp(&b.raw)
            .then(a.version.cmp(&b.version))
            .then(b.stem.len().cmp(&a.stem.len()))
    })
}

/// [`locate`], read (bounded) and [`parse`]d: one profile, whole.
pub fn open(filename: Option<&str>, make: &str, lens: &str) -> Result<LensProfileFile, Refusal> {
    open_path(&locate(filename, make, lens)?)
}

/// Is there an Adobe profile pool on this machine at all? With none, a
/// profile a sidecar names is unknown here rather than absent in Camera Raw.
pub fn pool_present() -> bool {
    !index().is_empty()
}

/// The profile a sidecar NAMES — crs:LensProfileName with crs:LensProfileFilename
/// beside it — resolved the way Camera Raw resolves it. Measured on flat-field
/// renders of a 14–24 mm frame with the three installed revisions of its
/// profile (the plain file, v2 and v3, whose vignetting differs by 22 % at the
/// corner), so which file was taken shows in the render:
///
/// * a name with a file name: the FILE, whatever the name says (the plain
///   file's name with v3's file name renders v3; a name no profile carries
///   with v3's file name renders v3 too);
/// * a name alone, or with a file name no profile has: the profile of that
///   name (the plain file's name renders the plain file, and the 24–105 mm
///   profile's name renders that profile's vignetting on the 14–24 mm frame —
///   the name is not checked against the lens);
/// * no name: nothing, even with a file name — the lookup does not start.
///
/// `Err(NotFound)` when it resolves to nothing; what Camera Raw does then
/// depends on the setup (`pipeline::fresh_lens_profile_for_sidecar`).
pub fn open_named(name: &str, filename: Option<&str>) -> Result<LensProfileFile, Refusal> {
    if index().is_empty() {
        return Err(Refusal::NoRoots);
    }
    let path = resolve_named(heads(), name, filename).ok_or(Refusal::NotFound)?;
    open_path(&path)
}

/// [`open_named`]'s rule over a given pool.
fn resolve_named(pool: &[Head], name: &str, filename: Option<&str>) -> Option<PathBuf> {
    let name = fold(name.trim());
    if name.is_empty() {
        return None;
    }
    let by_file = filename.map(|f| fold(f.trim())).filter(|f| !f.is_empty()).and_then(|want| {
        pool.iter().find(|h| h.path.file_name().is_some_and(|n| fold(&n.to_string_lossy()) == want))
    });
    by_file.or_else(|| choose(pool.iter().filter(|h| h.name == name))).map(|h| h.path.clone())
}

/// One profile file, read (bounded) and [`parse`]d, with its file name kept.
fn open_path(path: &Path) -> Result<LensProfileFile, Refusal> {
    // BOUNDED read (the R28 B2 rule): the pool directory is not this app's to
    // trust — `AUTOSHADE_LCP_DIR` and `%APPDATA%` are user-writable, so a file
    // there can be anything. The largest of the 3,576 profiles in the local
    // Adobe pool measures 4.3 MiB; 16 MiB is the same ceiling every
    // sidecar-class read in this codebase carries, and an over-cap file is an
    // `Unreadable` refusal — named, like every other degradation on this path.
    const MAX_LCP_BYTES: u64 = 16 * 1024 * 1024;
    let xml = crate::store::read_text_capped(path, MAX_LCP_BYTES).map_err(|_| Refusal::Unreadable)?;
    let mut file = parse(&xml)?;
    file.filename = path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    Ok(file)
}

/// Everything one `.lcp` node has to say about this frame.
///
/// `mask_warp` is the Lightroom-frame quantity R29 Batch-3 added; `distortion`
/// and `vignette` are what v1.5.0 renders from the same node. They come back
/// together because they come from ONE parse of one file: solving them
/// separately would read a 4 MiB document twice and could disclose two
/// different refusals for one lookup.
///
/// `distortion` / `vignette` are EMPTY when this node states no such model, or
/// states one this engine will not solve — an ordinary outcome for a
/// distortion-only profile, and never a refusal of the whole lookup.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Maps {
    pub mask_warp: Vec<f32>,
    pub distortion: Vec<f32>,
}

/// The whole source-B path: find the profile, pick the node, solve the knots.
///
/// One entry point so the refusal a caller discloses is the refusal that
/// actually happened — a caller assembling this from the pieces would have to
/// re-derive which step failed, which is exactly how a named degradation turns
/// back into a silent one.
pub fn solve(
    filename: Option<&str>,
    make: &str,
    lens: &str,
    focal_mm: Option<f32>,
    dims: (f32, f32),
    n: usize,
) -> Result<Maps, Refusal> {
    solve_in(&open(filename, make, lens)?, focal_mm, dims, n)
}

/// [`solve`] on a profile already [`open`]ed — the form a caller that also
/// reads the file's vignetting ([`LensProfileFile::vignette_gains`]) uses, so
/// one lookup reads the file once.
pub fn solve_in(profile: &LensProfileFile, focal_mm: Option<f32>, dims: (f32, f32), n: usize) -> Result<Maps, Refusal> {
    let model = profile.model_at(focal_mm)?;
    Ok(Maps {
        // The mask warp is the one that can REFUSE: it is the quantity every
        // existing caller was built around, and a frame with no map for it is
        // a named degradation. The two render maps are optional by nature.
        mask_warp: model.mask_warp_knots(dims, n).ok_or(Refusal::Unsolvable)?,
        distortion: model.distortion_knots(dims, n).unwrap_or_default(),
    })
}
