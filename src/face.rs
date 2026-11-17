//! The Portraits mode's face finder (v1.14.0): the bridge to `python/face.py`,
//! which runs MediaPipe's Face Landmarker over one picture and writes a small
//! JSON file with every face it found — a box, a score, the face outline and
//! the two eye outlines, in the picture's own pixels (see that script's
//! docstring for the format and the model).
//!
//! What this side does with the outlines is the whole of the mask story: each
//! face's outline is filled and feathered into a soft FACE mask, each eye's
//! into a soft EYES mask, and several faces are UNIONED (per-pixel maximum)
//! into one mask of each kind, because the first version edits every face at
//! once. Two more roles are built on top: SKIN is the face mask narrowed by a
//! colour range seeded from the face's own median colour, and BACKGROUND is the
//! subject matte `segment.py --target subject` already makes, inverted. All
//! four become ordinary bitmap masks carrying a [`MaskRole`], so every slider
//! the masks already have works on them unchanged.
//!
//! The AI picks WHERE; every edit stays a deterministic recipe slider, as with
//! the segmentation masks ([`crate::segment`]).

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use image::GrayImage;

use crate::recipe::{LocalAdjustment, MaskGeometry, MaskRole, RangeMask};

/// A real result is a few kilobytes per face; this bounds a broken or hostile
/// sidecar before anything is parsed.
pub const FACES_MAX_BYTES: u64 = 1024 * 1024;
/// The most faces one result may carry (the sidecar's own ceiling is 32).
pub const MAX_FACES: usize = 32;
/// The most points one outline may carry. MediaPipe's face outline is 36
/// points and each eye 16.
const MAX_OUTLINE_POINTS: usize = 64;

/// The face mask's feather, as a share of the face box's longer side: a soft
/// edge about as wide as the jaw's own shading, so a skin edit fades out over
/// the cheek rather than stopping on a line.
pub const FACE_FEATHER: f32 = 0.025;
/// The eye outline traces the lid opening; the mask grows it about its own
/// centre by this factor so the lashes and the lid crease are inside.
pub const EYE_GROW: f32 = 1.3;
/// The eyes mask's feather, as a share of the (grown) eye's width.
pub const EYE_FEATHER: f32 = 0.1;
/// The skin colour range's tolerance (`RangeMask::Color::amount`, 0..=1):
/// wider than Lightroom's 0.5 default, because skin under one light still
/// spans a broad band of chromaticity from the lit cheek to the shaded jaw.
pub const SKIN_RANGE_AMOUNT: f32 = 0.6;

/// One face the sidecar found, in the pixels of [`FaceReport::width`] ×
/// [`FaceReport::height`].
#[derive(Debug, Clone, PartialEq)]
pub struct Face {
    /// The detector's score, 0..=1, or `None` when no detection matched the
    /// outline (the landmarker itself returns no score).
    pub confidence: Option<f32>,
    /// `[x0, y0, x1, y1]` around the face outline.
    pub bbox: [f32; 4],
    /// The face outline, in order.
    pub oval: Vec<[f32; 2]>,
    /// The person's left eye (on the image's right), in order.
    pub left_eye: Vec<[f32; 2]>,
    /// The person's right eye (on the image's left), in order.
    pub right_eye: Vec<[f32; 2]>,
}

/// One validated sidecar result.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceReport {
    /// Which model answered, as the sidecar names it.
    pub backend: String,
    /// The frame the coordinates are in: the INPUT image's size.
    pub width: u32,
    pub height: u32,
    /// Every face found; empty when there is none, which is not an error.
    pub faces: Vec<Face>,
}

/// Parse and validate the JSON `face.py` writes. Anything malformed rejects
/// the whole result: a mask is never built from a guess.
pub fn parse_faces(text: &str) -> Result<FaceReport> {
    #[derive(serde::Deserialize)]
    struct Raw {
        version: u32,
        backend: String,
        width: u32,
        height: u32,
        faces: Vec<RawFace>,
    }
    #[derive(serde::Deserialize)]
    struct RawFace {
        confidence: Option<f32>,
        #[serde(rename = "box")]
        bbox: [f32; 4],
        oval: Vec<[f32; 2]>,
        left_eye: Vec<[f32; 2]>,
        right_eye: Vec<[f32; 2]>,
    }
    let raw: Raw = serde_json::from_str(text.trim()).context("face result is not the expected JSON")?;
    if raw.version != 1 {
        bail!("face result version {} is unsupported", raw.version);
    }
    if raw.width == 0 || raw.height == 0 || raw.width > 65_535 || raw.height > 65_535 {
        bail!("face result has an invalid frame {}x{}", raw.width, raw.height);
    }
    if raw.faces.len() > MAX_FACES {
        bail!("face result lists {} faces (cap {MAX_FACES})", raw.faces.len());
    }
    if raw.backend.len() > 256 {
        bail!("face result backend name is too long");
    }
    // Outlines may run a little past the frame (a face cut by the edge); a
    // point further out than one frame is not a face of this picture.
    let (w, h) = (raw.width as f32, raw.height as f32);
    let in_reach = |p: &[f32; 2]| p[0].is_finite() && p[1].is_finite() && p[0] >= -w && p[0] <= 2.0 * w && p[1] >= -h && p[1] <= 2.0 * h;
    let mut faces = Vec::with_capacity(raw.faces.len());
    for (i, f) in raw.faces.into_iter().enumerate() {
        if let Some(c) = f.confidence
            && !(c.is_finite() && (0.0..=1.0).contains(&c))
        {
            bail!("face {i}: confidence {c} is not in 0..=1");
        }
        let b = f.bbox;
        if !(in_reach(&[b[0], b[1]]) && in_reach(&[b[2], b[3]]) && b[0] <= b[2] && b[1] <= b[3]) {
            bail!("face {i}: the box is not a box in this frame");
        }
        for (what, poly) in [("face outline", &f.oval), ("left eye", &f.left_eye), ("right eye", &f.right_eye)] {
            if !(3..=MAX_OUTLINE_POINTS).contains(&poly.len()) {
                bail!("face {i}: the {what} has {} points (want 3..={MAX_OUTLINE_POINTS})", poly.len());
            }
            if !poly.iter().all(in_reach) {
                bail!("face {i}: the {what} has a point outside this frame");
            }
        }
        faces.push(Face { confidence: f.confidence, bbox: b, oval: f.oval, left_eye: f.left_eye, right_eye: f.right_eye });
    }
    Ok(FaceReport { backend: raw.backend, width: raw.width, height: raw.height, faces })
}

/// Run the sidecar `cfg.face_script` names: `input` (any image file) →
/// `output` (the faces JSON), and return the validated result. Through the
/// shared model-sidecar run, so the model slot, the bounded wait, "exit 0 is
/// not success" and the size cap are the family's.
pub fn detect_faces(cfg: &crate::config::Config, input: &Path, output: &Path) -> Result<FaceReport> {
    let script = Path::new(&cfg.face_script);
    if !script.exists() {
        bail!(
            "face sidecar not found at {} — run from the project dir or set AUTOSHADE_FACE_SCRIPT.",
            script.display()
        );
    }
    let args: Vec<std::ffi::OsString> =
        vec!["-E".into(), script.into(), "--input".into(), input.into(), "--output".into(), output.into()];
    let text = crate::run_model_sidecar_bounded("face sidecar", &cfg.sidecar_python(), args, output, Some(FACES_MAX_BYTES))?;
    parse_faces(&text).with_context(|| format!("face sidecar produced an unusable result {}", output.display()))
}

/// Fill `poly` (pixel coordinates of a `w`×`h` frame) into `mask` at 255,
/// even-odd, sampling pixel CENTRES — the convention every mask here reads.
fn fill_polygon(mask: &mut GrayImage, poly: &[[f32; 2]]) {
    let (w, h) = (mask.width() as usize, mask.height() as usize);
    if poly.len() < 3 || w == 0 || h == 0 {
        return;
    }
    let (lo, hi) = poly.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p[1]), hi.max(p[1])));
    let y0 = (lo - 0.5).ceil().max(0.0) as usize;
    let y1 = ((hi - 0.5).floor().min(h as f32 - 1.0)).max(-1.0);
    if y1 < 0.0 {
        return;
    }
    let mut xs: Vec<f32> = Vec::with_capacity(poly.len());
    for y in y0..=y1 as usize {
        let cy = y as f32 + 0.5;
        xs.clear();
        for (i, a) in poly.iter().enumerate() {
            let b = poly[(i + 1) % poly.len()];
            // Half-open in y, so a vertex on the scanline counts once.
            if (a[1] <= cy) != (b[1] <= cy) {
                xs.push(a[0] + (cy - a[1]) / (b[1] - a[1]) * (b[0] - a[0]));
            }
        }
        xs.sort_by(f32::total_cmp);
        for pair in xs.chunks_exact(2) {
            let xa = (pair[0] - 0.5).ceil().max(0.0) as usize;
            let xb = (pair[1] - 0.5).floor().min(w as f32 - 1.0);
            if xb < xa as f32 {
                continue;
            }
            for x in xa..=xb as usize {
                mask.put_pixel(x as u32, y as u32, image::Luma([255]));
            }
        }
    }
}

/// Fill one outline into its own hard mask, feather it by `sigma` pixels and
/// fold it into `out` by per-pixel maximum (the union). Only the outline's
/// padded box is blurred, so ten faces on a large frame cost ten small blurs.
fn union_feathered(out: &mut GrayImage, poly: &[[f32; 2]], sigma: f32) {
    let (w, h) = (out.width() as f32, out.height() as f32);
    let pad = (3.0 * sigma).ceil() + 2.0;
    let (mut x0, mut y0, mut x1, mut y1) = (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY);
    for p in poly {
        x0 = x0.min(p[0]);
        y0 = y0.min(p[1]);
        x1 = x1.max(p[0]);
        y1 = y1.max(p[1]);
    }
    let bx0 = (x0 - pad).floor().clamp(0.0, w) as u32;
    let by0 = (y0 - pad).floor().clamp(0.0, h) as u32;
    let bx1 = (x1 + pad).ceil().clamp(0.0, w) as u32;
    let by1 = (y1 + pad).ceil().clamp(0.0, h) as u32;
    if bx1 <= bx0 || by1 <= by0 {
        return;
    }
    let local: Vec<[f32; 2]> = poly.iter().map(|p| [p[0] - bx0 as f32, p[1] - by0 as f32]).collect();
    let mut hard = GrayImage::new(bx1 - bx0, by1 - by0);
    fill_polygon(&mut hard, &local);
    let soft = if sigma > 0.0 { crate::render::feather_mask(&hard, sigma) } else { hard };
    for (x, y, p) in soft.enumerate_pixels() {
        let o = out.get_pixel_mut(bx0 + x, by0 + y);
        o[0] = o[0].max(p[0]);
    }
}

/// `poly` from the report's frame into a `w`×`h` frame.
fn scaled(report: &FaceReport, poly: &[[f32; 2]], w: u32, h: u32) -> Vec<[f32; 2]> {
    let (sx, sy) = (w as f32 / report.width as f32, h as f32 / report.height as f32);
    poly.iter().map(|p| [p[0] * sx, p[1] * sy]).collect()
}

/// Every face's outline, filled, feathered by [`FACE_FEATHER`] of its box and
/// unioned, as a `w`×`h` soft mask (white = face). Any size: the outlines are
/// scaled from the report's frame, so the mask can be drawn at the preview's
/// resolution and still lands where the faces are.
pub fn face_mask(report: &FaceReport, w: u32, h: u32) -> GrayImage {
    let mut out = GrayImage::new(w, h);
    for f in &report.faces {
        let poly = scaled(report, &f.oval, w, h);
        let (x0, y0, x1, y1) = poly.iter().fold(
            (f32::INFINITY, f32::INFINITY, f32::NEG_INFINITY, f32::NEG_INFINITY),
            |(a, b, c, d), p| (a.min(p[0]), b.min(p[1]), c.max(p[0]), d.max(p[1])),
        );
        union_feathered(&mut out, &poly, FACE_FEATHER * (x1 - x0).max(y1 - y0));
    }
    out
}

/// Both eyes of every face, each grown by [`EYE_GROW`] about its centre,
/// feathered by [`EYE_FEATHER`] of its width and unioned, as a `w`×`h` soft
/// mask (white = eyes).
pub fn eyes_mask(report: &FaceReport, w: u32, h: u32) -> GrayImage {
    let mut out = GrayImage::new(w, h);
    for f in &report.faces {
        for eye in [&f.left_eye, &f.right_eye] {
            let poly = scaled(report, eye, w, h);
            let n = poly.len() as f32;
            let c = poly.iter().fold([0.0f32; 2], |a, p| [a[0] + p[0] / n, a[1] + p[1] / n]);
            let grown: Vec<[f32; 2]> =
                poly.iter().map(|p| [c[0] + (p[0] - c[0]) * EYE_GROW, c[1] + (p[1] - c[1]) * EYE_GROW]).collect();
            let (lo, hi) = grown.iter().fold((f32::INFINITY, f32::NEG_INFINITY), |(lo, hi), p| (lo.min(p[0]), hi.max(p[0])));
            union_feathered(&mut out, &grown, EYE_FEATHER * (hi - lo));
        }
    }
    out
}

/// The skin colour range's seed: the per-channel median of `reference` (sRGB)
/// over the pixels the face mask holds at ≥ 90 %, and where they are (their
/// centroid, normalised — Lightroom's sample marker). `None` when the mask
/// holds no such pixel. The mask is read at `reference`'s pixel centres, so
/// the two need not be the same size.
pub fn skin_seed(reference: &image::RgbImage, face: &GrayImage) -> Option<([f32; 3], [f32; 2])> {
    let (w, h) = reference.dimensions();
    let mut ch: [Vec<u8>; 3] = [Vec::new(), Vec::new(), Vec::new()];
    let (mut sx, mut sy) = (0.0f64, 0.0f64);
    for (x, y, p) in reference.enumerate_pixels() {
        let nx = (x as f32 + 0.5) / w as f32;
        let ny = (y as f32 + 0.5) / h as f32;
        if crate::render::sample_gray_norm(face, nx, ny) >= 0.9 {
            for c in 0..3 {
                ch[c].push(p[c]);
            }
            sx += nx as f64;
            sy += ny as f64;
        }
    }
    let n = ch[0].len();
    if n == 0 {
        return None;
    }
    let median = |v: &mut Vec<u8>| {
        v.sort_unstable();
        v[v.len() / 2] as f32 / 255.0
    };
    let [mut r, mut g, mut b] = ch;
    Some(([median(&mut r), median(&mut g), median(&mut b)], [(sx / n as f64) as f32, (sy / n as f64) as f32]))
}

/// The pixels a skin seed is SAMPLED from: `frame` (the photo's neutral frame —
/// a RAW's sensor develop, a baked source's own pixels) developed with
/// `recipe`'s masks BEFORE index `before` and no geometry stage. `lens` is the
/// composed lens profile when `frame` is already lens-corrected through it
/// (a marked baked source whose pixels carry the correction, v1.17.0),
/// so the masks before the skin mask are sampled in that frame
/// (`MaskFrame::Corrected`); `None` for a sensor-frame picture.
///
/// A Range Mask is matched on the pixel as it stands when its mask runs —
/// after the global white balance, tone and colour, and after the masks
/// before it (`render::apply_masks` stacks them) — so the seed must come from
/// those same pixels, which is the eyedropper's convention
/// ([`crate::render::develop_preview_framed`] with
/// [`crate::render::MaskFrame::AsRendered`]; the GUI's
/// `range_reference_recipe`). Seeded from the undeveloped frame (until
/// 2026-10-26), a recipe that moved the white balance seeded a colour no skin
/// pixel of the render carries. `film_short_edge` / `raw_source` are
/// `develop_preview_framed`'s.
pub fn skin_seed_reference(
    frame: &image::DynamicImage,
    recipe: &crate::recipe::EditRecipe,
    before: usize,
    film_short_edge: Option<u32>,
    raw_source: bool,
    lens: Option<&crate::recipe::LensProfile>,
) -> image::RgbImage {
    let mut pre = recipe.clone();
    pre.masks.truncate(before);
    pre.straighten_deg = 0.0;
    pre.lens_distortion = 0.0;
    pre.ca_r = 0.0;
    pre.ca_b = 0.0;
    pre.crop = None;
    let mask_frame = match lens {
        Some(profile) => crate::render::MaskFrame::Corrected { profile },
        None => crate::render::MaskFrame::AsRendered,
    };
    crate::render::develop_preview_framed(frame, &pre, &crate::diag::pixels(), mask_frame, film_short_edge, raw_source)
        .to_rgb8()
}

/// A bitmap mask over `raster` carrying `role` — the FACE and EYES masks. The
/// row's label comes from [`MaskRole::en_name`], so `name` stays empty.
pub fn portrait_adjustment(role: MaskRole, raster: String) -> LocalAdjustment {
    LocalAdjustment { mask: MaskGeometry::Bitmap { path: raster }, role, ..Default::default() }
}

/// The SKIN mask: the face raster intersected with a colour range around
/// `seed` (from [`skin_seed`]), sampled at `at`.
pub fn skin_adjustment(face_raster: String, seed: [f32; 3], at: [f32; 2]) -> LocalAdjustment {
    LocalAdjustment {
        range: Some(RangeMask::Color { r: seed[0], g: seed[1], b: seed[2], amount: SKIN_RANGE_AMOUNT, px: at[0], py: at[1] }),
        ..portrait_adjustment(MaskRole::Skin, face_raster)
    }
}

/// The BACKGROUND mask: the subject matte (`segment.py --target subject`),
/// inverted.
pub fn background_adjustment(subject_raster: String) -> LocalAdjustment {
    LocalAdjustment { inverted: true, ..portrait_adjustment(MaskRole::Background, subject_raster) }
}

// ── "Keep skin natural" (design-portraits-mode.md §4) ───────────────────────
//
// One switch in the Portraits panel, on by default. Inside the masks that
// cover skin — the SKIN mask and the FACE mask, since a proposal's `face`
// target is the face mask and its edits land on skin too — three hard limits:
// colour (saturation, hue rotation, warmth), softening (texture, clarity) and
// no sharpening, the "smoothed then sharpened" plastic look. The values are
// the first version's, set to Lightroom's usual portrait ranges; the final
// check on real portraits tunes them. The GUI applies them to every write of
// the recipe while the switch is on — a slider, a paste, an AI landing — so
// the user's hand and the model's answer meet the same limits at one place.

/// The local hue slider's full travel (±100) in degrees: `render::masks`
/// rotates every hue inside the mask by `hue / 100 × 30°`.
pub const LOCAL_HUE_FULL_DEG: f32 = 30.0;
/// Skin saturation may be lifted by at most this much (fading it is free).
pub const SKIN_SATURATION_MAX: f32 = 15.0;
/// Skin hues rotate by at most this many degrees either way.
pub const SKIN_HUE_MAX_DEG: f32 = 8.0;
/// Skin warmth moves by at most this much either way (the relative ±100 scale).
pub const SKIN_TEMPERATURE_MAX: f32 = 20.0;
/// Softening stops here: texture no lower than this…
pub const SKIN_TEXTURE_MIN: f32 = -60.0;
/// …and clarity no lower than this.
pub const SKIN_CLARITY_MIN: f32 = -40.0;
/// Skin is never sharpened: local sharpness at most 0 (negative softens).
pub const SKIN_SHARPNESS_MAX: f32 = 0.0;

/// The local hue limit in the field's own units (±100 = ±30°).
pub fn skin_hue_max() -> f32 {
    SKIN_HUE_MAX_DEG / LOCAL_HUE_FULL_DEG * 100.0
}

/// Does "Keep skin natural" hold a mask with this role?
pub fn guards_skin(role: MaskRole) -> bool {
    matches!(role, MaskRole::Skin | MaskRole::Face)
}

/// Pull one mask's skin controls back inside the limits; `true` when anything
/// moved. The role is the caller's question ([`guards_skin`]); this only
/// clamps.
pub fn keep_skin_natural(m: &mut LocalAdjustment) -> bool {
    let before = [m.saturation, m.hue, m.temperature, m.texture, m.clarity, m.sharpness];
    let hue = skin_hue_max();
    m.saturation = m.saturation.min(SKIN_SATURATION_MAX);
    m.hue = m.hue.clamp(-hue, hue);
    m.temperature = m.temperature.clamp(-SKIN_TEMPERATURE_MAX, SKIN_TEMPERATURE_MAX);
    m.texture = m.texture.max(SKIN_TEXTURE_MIN);
    m.clarity = m.clarity.max(SKIN_CLARITY_MIN);
    m.sharpness = m.sharpness.min(SKIN_SHARPNESS_MAX);
    before != [m.saturation, m.hue, m.temperature, m.texture, m.clarity, m.sharpness]
}

/// How many skin values of `recipe` lie past the limits — the count
/// [`keep_skin_natural_in`] would move, counted without moving anything. The
/// GUI reads it when a photo opens: a saved edit that already goes further
/// opens with the switch off, because opening never changes a value.
pub fn skin_values_beyond(recipe: &crate::recipe::EditRecipe) -> usize {
    let fields = |m: &LocalAdjustment| [m.saturation, m.hue, m.temperature, m.texture, m.clarity, m.sharpness];
    recipe
        .masks
        .iter()
        .filter(|m| guards_skin(m.role))
        .map(|m| {
            let mut held = m.clone();
            keep_skin_natural(&mut held);
            fields(m).iter().zip(fields(&held)).filter(|(a, b)| *a != b).count()
        })
        .sum()
}

/// [`keep_skin_natural`] over every skin-covering mask of `recipe`; `true`
/// when anything moved.
pub fn keep_skin_natural_in(recipe: &mut crate::recipe::EditRecipe) -> bool {
    recipe.masks.iter_mut().filter(|m| guards_skin(m.role)).fold(false, |moved, m| keep_skin_natural(m) | moved)
}

// ── The face masks a proposal asks for ──────────────────────────────────────
//
// A proposal may name a part of the picture by what it is (`kind=ai`); its
// `face` and `eyes` targets arrive here as a FACE or EYES mask that names no
// raster yet (`advisor::proposer::expand_ai_mask_intents`) and are outlined on
// this computer — the model's coordinates are never read. `background` needs
// no help from here: it is the subject matte, inverted, which the segmenter
// already cuts (`segment::resolve_ai_masks`).

/// What [`resolve_face_masks`] did, for the caller's disclosure.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FaceMaskResolution {
    /// Masks that now name a raster — cache hits included.
    pub resolved: usize,
    /// Of those, the ones whose rasters were already on disk.
    pub cached: usize,
    /// Masks taken out of the recipe, each with the reason, in recipe order:
    /// a face mask with nothing to stand on would edit nothing, or — worse —
    /// fail an export that cannot read its raster.
    pub dropped: Vec<(String, String)>,
}

impl FaceMaskResolution {
    /// One English line for the CLI, or `None` when there was nothing to do.
    pub fn describe(&self) -> Option<String> {
        if self.resolved == 0 && self.dropped.is_empty() {
            return None;
        }
        let mut line = format!("{} face/eye mask(s) outlined on this computer by the face finder", self.resolved);
        if self.cached > 0 {
            line.push_str(&format!(" ({} from the photo's cache)", self.cached));
        }
        for (name, why) in &self.dropped {
            line.push_str(&format!("; \"{name}\" left out ({why})"));
        }
        Some(line)
    }
}

/// A face, eyes or skin mask that names no raster yet — the shape a
/// proposal's `face` / `eyes` target takes until [`resolve_face_masks`] runs.
pub fn is_face_intent(m: &LocalAdjustment) -> bool {
    matches!(m.role, MaskRole::Face | MaskRole::Eyes | MaskRole::Skin)
        && matches!(&m.mask, MaskGeometry::Bitmap { path } if path.is_empty())
}

/// Bump when the masks drawn from one face result change shape (a feather, a
/// grow factor), so cached rasters from an older build are drawn again.
///
/// 2: a RAW's masks are drawn on its SENSOR develop, no longer on its embedded
/// preview (`segment::mask_source_frame`), so a raster cached from the old
/// frame is drawn again.
const FACE_MASK_GENERATION: u32 = 2;
/// The longest edge the photo is shown to the face finder at, and the masks
/// drawn at: the masks are read in normalised coordinates at any render size.
const RESOLVE_EDGE: u32 = 2048;

/// The two cached rasters of one photo in one frame, keyed and homed on the
/// PHOTO like the segmenter's (`segment::resolve_ai_masks`): the photo's key,
/// the quarter turns the frame is in, and [`FACE_MASK_GENERATION`].
fn cached_rasters(raw: &Path, turns: u8) -> (PathBuf, PathBuf) {
    let ident = format!("{}|t{}|g{FACE_MASK_GENERATION}", crate::store::photo_key(raw), turns % 4);
    // FNV-1a: stable across runs, platforms and releases.
    let h = ident.bytes().fold(0xcbf2_9ce4_8422_2325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));
    (
        crate::store::raster_target(raw, &format!("mask-face-ai-{h:016x}")),
        crate::store::raster_target(raw, &format!("mask-eyes-ai-{h:016x}")),
    )
}

/// The photo's frame as the masks see it: `src`'s pixels in the recipe's
/// quarter turns, no longer than [`RESOLVE_EDGE`] — for a RAW its SENSOR
/// develop, never its embedded preview ([`crate::segment::mask_source_frame`]:
/// an in-camera 4:3 preview is a crop of the 3:2 frame the render samples in).
fn source_frame(src: &Path, turns: u8) -> Result<image::RgbImage> {
    let p = crate::segment::mask_source_frame(src, turns, RESOLVE_EDGE)
        .with_context(|| format!("decode {}", src.display()))?;
    Ok(p.to_rgb8())
}

/// Write a mask to `target` whole or not at all (a sibling, then a rename).
fn write_mask_atomically(mask: &GrayImage, target: &Path) -> Result<()> {
    if let Some(dir) = target.parent() {
        std::fs::create_dir_all(dir).with_context(|| format!("create {}", dir.display()))?;
    }
    let part = target.with_extension("part.png");
    mask.save_with_format(&part, image::ImageFormat::Png).with_context(|| format!("write {}", part.display()))?;
    std::fs::rename(&part, target).with_context(|| format!("publish {}", target.display()))?;
    crate::store::durable_adopt(target).with_context(|| format!("sync {}", target.display()))
}

/// Run the face finder on the photo's frame and draw the two rasters; the
/// frame comes back for the skin seed.
fn outline_faces(
    src: &Path,
    turns: u8,
    face_path: &Path,
    eyes_path: &Path,
    detect: impl FnOnce(&Path, &Path) -> Result<FaceReport>,
) -> Result<image::RgbImage> {
    static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    let frame = source_frame(src, turns)?;
    let n = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let tmp = std::env::temp_dir().join(format!("autoshade-face-src-{}-{n}", std::process::id()));
    let (input, json) = (tmp.with_extension("png"), tmp.with_extension("json"));
    frame.save(&input).with_context(|| format!("stage {}", input.display()))?;
    let report = detect(&input, &json);
    let _ = std::fs::remove_file(&input);
    let _ = std::fs::remove_file(&json);
    let report = report?;
    if report.faces.is_empty() {
        bail!("no face was found in this photo");
    }
    let (w, h) = frame.dimensions();
    write_mask_atomically(&face_mask(&report, w, h), face_path)?;
    write_mask_atomically(&eyes_mask(&report, w, h), eyes_path)?;
    Ok(frame)
}

/// Give every face, eyes and skin mask that names no raster yet
/// ([`is_face_intent`]) its raster: the face finder runs once on the photo's
/// own frame (`src`'s pixels, in the recipe's quarter turns), and its masks
/// are cached beside the develop, so a re-render costs nothing. A skin mask
/// without a colour range gets one, seeded from the face. When no face is
/// found, or the face finder cannot run, those masks are taken out and the
/// reason is returned.
///
/// `raw` is the photo the cache is keyed and homed on; `src` the pixels the
/// render uses (a saved retouch master, when there is one) — the same split
/// as `segment::resolve_ai_masks`.
pub fn resolve_face_masks(
    cfg: &crate::config::Config,
    raw: &Path,
    src: &Path,
    recipe: &mut crate::recipe::EditRecipe,
) -> FaceMaskResolution {
    resolve_face_masks_with(raw, src, recipe, |input, output| detect_faces(cfg, input, output))
}

fn resolve_face_masks_with(
    raw: &Path,
    src: &Path,
    recipe: &mut crate::recipe::EditRecipe,
    detect: impl FnOnce(&Path, &Path) -> Result<FaceReport>,
) -> FaceMaskResolution {
    let mut out = FaceMaskResolution::default();
    if !recipe.masks.iter().any(is_face_intent) {
        return out;
    }
    let turns = recipe.quarter_turns % 4;
    let (face_path, eyes_path) = cached_rasters(raw, turns);
    let readable = |p: &Path| p.exists() && crate::render::open_mask_bounded(p).is_ok();
    let from_cache = readable(&face_path) && readable(&eyes_path);
    let (mut frame, failure) = if from_cache {
        (None, None)
    } else {
        match outline_faces(src, turns, &face_path, &eyes_path, detect) {
            Ok(frame) => (Some(frame), None),
            Err(e) => (None, Some(format!("{e:#}"))),
        }
    };
    let (face_s, eyes_s) = (face_path.to_string_lossy().into_owned(), eyes_path.to_string_lossy().into_owned());
    // The skin seed's reference recipe: the masks before the first skin mask
    // that needs a seed, with the face/eyes intents among them pointing at the
    // rasters they are about to get — the pixels that mask's range is judged
    // on ([`skin_seed_reference`]). One seed serves every such skin mask.
    let seed_prefix = recipe
        .masks
        .iter()
        .position(|m| is_face_intent(m) && m.role == MaskRole::Skin && m.range.is_none())
        .map(|at| {
            let mut pre = recipe.clone();
            pre.masks.truncate(at);
            for m in pre.masks.iter_mut().filter(|m| is_face_intent(m)) {
                m.mask = MaskGeometry::Bitmap { path: if m.role == MaskRole::Eyes { eyes_s.clone() } else { face_s.clone() } };
            }
            (pre, at)
        });
    let mut seed: Option<Option<([f32; 3], [f32; 2])>> = None;
    recipe.masks.retain_mut(|m| {
        if !is_face_intent(m) {
            return true;
        }
        let name = if m.name.is_empty() { m.role.en_name().unwrap_or("unnamed").to_string() } else { m.name.clone() };
        if let Some(why) = &failure {
            out.dropped.push((name, why.clone()));
            return false;
        }
        if m.role == MaskRole::Skin && m.range.is_none() {
            let s = seed.get_or_insert_with(|| {
                let frame = match frame.take() {
                    Some(f) => f,
                    None => source_frame(src, turns).ok()?,
                };
                let face = crate::render::open_mask_bounded(&face_path).ok()?.to_luma8();
                let (pre, at) = seed_prefix.as_ref()?;
                let developed = skin_seed_reference(
                    &image::DynamicImage::ImageRgb8(frame),
                    pre,
                    *at,
                    crate::decode::film_short_edge(src),
                    crate::decode::is_raw(src),
                    // The sensor-frame picture the face was found on.
                    None,
                );
                skin_seed(&developed, &face)
            });
            let Some((c, at)) = *s else {
                out.dropped.push((name, "the face holds no pixel to take the skin colour from".into()));
                return false;
            };
            m.range = Some(RangeMask::Color { r: c[0], g: c[1], b: c[2], amount: SKIN_RANGE_AMOUNT, px: at[0], py: at[1] });
        }
        m.mask = MaskGeometry::Bitmap { path: if m.role == MaskRole::Eyes { eyes_s.clone() } else { face_s.clone() } };
        out.resolved += 1;
        if from_cache {
            out.cached += 1;
        }
        true
    });
    out
}

/// Write a mask under a freshly claimed raster name beside `src`'s develop
/// (`mask-face`, `mask-eyes`, …), durable before anything references it —
/// the same three steps every raster the app makes goes through.
pub fn save_mask(mask: &GrayImage, src: &Path, prefix: &str) -> Result<PathBuf> {
    let path = crate::store::claim_raster(src, prefix).with_context(|| format!("claim a {prefix} raster"))?;
    mask.save(&path).with_context(|| format!("write {}", path.display()))?;
    crate::store::durable_adopt(&path).with_context(|| format!("sync {}", path.display()))?;
    Ok(path)
}

// ── The four roles, and the panel sliders that write them ──────────────────
//
// The recipe-side half of the Portraits feature: which mask a control writes,
// what a fresh AI look keeps, and the slider→field mappings. These live in the
// library, not the panel: the GUI must not own a second copy of a rule about
// the recipe.

/// The four masks the find step makes.
pub const PORTRAIT_ROLES: [MaskRole; 4] =
    [MaskRole::Face, MaskRole::Eyes, MaskRole::Skin, MaskRole::Background];

/// How much the skin and eyes sliders move their fields. Softening is texture
/// down by the slider and clarity down by half of it — the guardrail's two
/// floors (−60 / −40) meet at the same slider position. The eyes' "Crisp" is
/// sharpness by the slider, clarity by half. The background's "Darken" is
/// exposure down, 100 = two stops.
const SOFTEN_CLARITY_SHARE: f32 = 0.5;
const CRISP_CLARITY_SHARE: f32 = 0.5;
const DARKEN_EV_AT_FULL: f32 = 2.0;

/// The first mask with `role`.
pub fn role_mask(r: &mut crate::recipe::EditRecipe, role: MaskRole) -> Option<&mut LocalAdjustment> {
    r.masks.iter_mut().find(|m| m.role == role)
}

/// The mask the skin group writes: the skin mask, or — when only an AI portrait
/// look's face mask is there — the face mask.
pub fn skin_target(r: &mut crate::recipe::EditRecipe) -> Option<&mut LocalAdjustment> {
    let i = r
        .masks
        .iter()
        .position(|m| m.role == MaskRole::Skin)
        .or_else(|| r.masks.iter().position(|m| m.role == MaskRole::Face))?;
    r.masks.get_mut(i)
}

/// The subject matte a "keep the person sharp" blur protects with: the
/// background mask's own raster (the subject, before its inversion).
pub fn subject_raster(r: &crate::recipe::EditRecipe) -> Option<String> {
    r.masks.iter().filter(|m| m.role == MaskRole::Background).find_map(|m| match &m.mask {
        MaskGeometry::Bitmap { path } if !path.is_empty() => Some(path.clone()),
        MaskGeometry::AiMask { raster: Some(path), .. } => Some(path.clone()),
        _ => None,
    })
}

/// The raster of the first face mask — what 🤖 Remove blemishes holds the heal
/// to — or `None` when there is no face mask with one.
pub fn face_raster(r: &crate::recipe::EditRecipe) -> Option<PathBuf> {
    r.masks
        .iter()
        .filter(|m| m.role == MaskRole::Face)
        .find_map(|m| crate::render::geometry_raster_path(&m.mask).filter(|p| !p.is_empty()).map(PathBuf::from))
}

/// An AI portrait look replaces the recipe whole: the masks the photo had come
/// back under it — with their sliders at rest, the look is the AI's — wherever
/// the look has no mask of that role, and the lens blur where the look carries
/// none. Done on the landed recipe BEFORE it is written, so the saved file is
/// the look the panel shows.
pub fn restore_portrait_masks(
    recipe: &mut crate::recipe::EditRecipe,
    kept: Vec<LocalAdjustment>,
    blur: Option<crate::render::LensBlur>,
) {
    for m in kept {
        if recipe.masks.iter().any(|n| n.role == m.role) {
            continue;
        }
        recipe.masks.push(LocalAdjustment {
            mask: m.mask,
            range: m.range,
            inverted: m.inverted,
            name: m.name,
            role: m.role,
            ..Default::default()
        });
    }
    if recipe.lens_blur.is_none() {
        recipe.lens_blur = blur;
    }
}

// The Portraits panel's slider read/write pairs. A read returns what the
// control shows; a write sets the field(s) that control owns.
pub fn soften_of(m: &LocalAdjustment) -> f32 {
    (-m.texture).max(0.0)
}
pub fn set_soften(m: &mut LocalAdjustment, s: f32) {
    m.texture = -s;
    m.clarity = -s * SOFTEN_CLARITY_SHARE;
}
pub fn crisp_of(m: &LocalAdjustment) -> f32 {
    m.sharpness.max(0.0)
}
pub fn set_crisp(m: &mut LocalAdjustment, c: f32) {
    m.sharpness = c;
    m.clarity = c * CRISP_CLARITY_SHARE;
}
pub fn darken_of(m: &LocalAdjustment) -> f32 {
    (-m.exposure_ev / DARKEN_EV_AT_FULL * 100.0).max(0.0)
}
pub fn set_darken(m: &mut LocalAdjustment, d: f32) {
    m.exposure_ev = -d / 100.0 * DARKEN_EV_AT_FULL;
}
pub fn fade_of(m: &LocalAdjustment) -> f32 {
    (-m.saturation).max(0.0)
}
pub fn set_fade(m: &mut LocalAdjustment, f: f32) {
    m.saturation = -f;
}
