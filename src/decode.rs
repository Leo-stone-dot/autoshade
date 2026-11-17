//! RAW decode + feature extraction (Milestone M1, decode half).
//!
//! Backed by `rawler` 0.7.2 — chosen for Sony A7R IV/IVA coverage, embedded
//! preview extraction, and full EXIF (see `docs/M1_PLAN.md` §1 and §9; the
//! older pure-Rust `rawloader` froze its camera DB before these bodies). One
//! backend: a `Decoder` trait abstraction would only pay for itself with a
//! second backend, and the user shoots a single camera family.
//!
//! All `rawler` calls here were written against the crate's real source
//! (`RawSource::new`, `get_decoder`, the `Decoder` trait, `RawMetadata.exif`),
//! not from memory.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use image::{DynamicImage, GenericImageView};
use rawler::decoders::RawDecodeParams;
use rawler::formats::tiff::reader::TiffReader;
use rawler::formats::tiff::{GenericTiffReader, Rational, SRational, Value};
use rawler::get_decoder;
use rawler::rawsource::RawSource;
use rawler::tags::TiffCommonTag;

/// Camera + capture metadata pulled from the RAW, for display and for feeding
/// the AI advisor later.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Meta {
    pub make: String,
    pub model: String,
    pub lens: Option<String>,
    pub iso: Option<u32>,
    /// Human shutter string, e.g. "1/1250" or "2s".
    pub shutter: Option<String>,
    /// f-number, e.g. 4.0.
    pub aperture: Option<f32>,
    pub focal_length_mm: Option<f32>,
    pub exposure_bias_ev: Option<f32>,
    pub date_time: Option<String>,
    /// Full sensor dimensions (from the raw image, not the preview).
    pub width: usize,
    pub height: usize,
    /// As-shot white-balance multipliers [R, G1, B, G2].
    pub as_shot_wb_coeffs: [f32; 4],
}

/// 256-bin per-channel + luma histogram with clipping fractions.
///
/// Computed from the camera-processed embedded preview (tone-mapped), so it is
/// a *display-referred* histogram — good for framing/clipping hints, not a
/// linear raw histogram. A raw-linear version can replace this in a later
/// milestone if exposure decisions need it.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Histogram {
    pub luma: Vec<u32>,
    pub r: Vec<u32>,
    pub g: Vec<u32>,
    pub b: Vec<u32>,
    /// % of pixels with luma in 0..=1 (crushed blacks).
    pub clip_black_pct: f32,
    /// % of pixels with luma in 254..=255 (blown highlights).
    pub clip_white_pct: f32,
    pub sample_pixels: u64,
}

/// Everything `decode_raw` produces for one RAW file.
pub struct Decoded {
    /// Full-resolution embedded preview (already white-balanced by the camera).
    pub preview: DynamicImage,
    pub meta: Meta,
    pub histogram: Histogram,
    /// Embedded XMP packet, if the RAW carries one.
    pub embedded_xmp: Option<String>,
}

fn ratio(r: &Rational) -> f32 {
    if r.d == 0 { 0.0 } else { r.n as f32 / r.d as f32 }
}
fn sratio(r: &SRational) -> f32 {
    if r.d == 0 { 0.0 } else { r.n as f32 / r.d as f32 }
}

/// Format a shutter-speed Rational as "1/x" for fast speeds, "Ns" otherwise.
fn fmt_shutter(r: &Rational) -> String {
    let v = ratio(r);
    if v > 0.0 && v < 1.0 {
        format!("1/{}", (1.0 / v).round() as i64)
    } else {
        format!("{v}s")
    }
}

/// Every camera-RAW extension the app opens — THE list `is_raw` is, and the
/// one every hand-copied extension list in the tree derives from (the GUI file
/// dialog, the web `accept` attribute, the library scanners). A second copy is
/// drift waiting to happen: `.orf`/`.rw2`/`.raw` were openable for four
/// releases while the file dialog refused them.
///
/// **What is in it.** One entry per rawler 0.7.2 decoder that has a real
/// filename extension. rawler dispatches on CONTENT (magic bytes, then the
/// TIFF `Make` string — `decoders/mod.rs:847-994`), never on the extension, so
/// this list only has to name files worth handing it; the decoder choice is
/// rawler's. `3fr`/`fff` are Hasselblad (the `tfr` decoder), `mos` is Leaf,
/// `nrw` is Nikon's compact line, `mrw` Minolta, `ari` ARRI.
///
/// **What is deliberately NOT in it.**
///   * `x3f` (Sigma Foveon) — PERMANENTLY excluded. rawler 0.7.2's
///     `decoders/x3f.rs:138` (`format_dump`) and `:146` (`raw_metadata`) are
///     literal `todo!()`, i.e. a guaranteed panic, and we call `raw_metadata`
///     directly rather than through rawler's own `catch_unwind` wrapper. The
///     CLI guard added alongside this list ([`guard_parser_panic`]) turns that
///     into a named error instead of an abort, but a format whose metadata
///     reader cannot run at all has nothing to offer a photographer — listing
///     it would only promise support that provably does not exist. Revisit
///     when the upstream `todo!()`s become real code, not before.
///   * `nkd` / "unwrapped" — rawler decoders with no extension of their own
///     (CHDK-style naked dumps matched by FILE SIZE, `mod.rs:989-992`).
///   * `qtk` — QuickTake, matched by magic bytes; the extension is `.qtk` but
///     the format is a 1994 Apple curiosity with no develop path worth
///     claiming.
pub const RAW_EXTS: [&str; 24] = [
    "arw", "dng", "raw", "raf", "nef", "cr2", "cr3", "orf", "rw2", "pef", "srw", "3fr", "fff",
    "iiq", "mef", "mos", "erf", "kdc", "dcr", "dcs", "crw", "nrw", "mrw", "ari",
];

/// Does this path look like a camera RAW (vs an already-baked raster like a
/// LR/PS-exported PNG/TIFF/JPEG)? Drives the raw-vs-baked dispatch.
///
/// Accepting an extension is NOT a promise that the body decodes: rawler
/// carries 725 camera models and refuses anything outside them
/// ([`describe_decoder_failure`] turns that into a sentence a photographer can
/// act on). It IS a promise that the file reaches the RAW engine rather than
/// the baked one, which is the only decision this predicate makes.
pub fn is_raw(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| RAW_EXTS.iter().any(|x| e.eq_ignore_ascii_case(x)))
}

/// Transform profiled pixels into the sRGB working space the whole pipeline
/// assumes. A baked import that carries an ICC profile (LR "Edit in…" exports
/// ProPhoto 16-bit TIFFs by default) used to be read as if it were sRGB, so
/// the histogram, tone and HSL decisions were all computed on wrong numbers.
/// Unsupported layouts and unparseable profiles are hard errors, not a silent
/// fall-through to "assume sRGB" — that fall-through IS the bug, and in a
/// batch run it would pay for a grade computed from incorrect colors.
fn apply_icc_profile(img: &mut DynamicImage, profile: &[u8], path: &Path) -> Result<()> {
    // The engine's own sRGB tag (`render::SRGB_ICC`, on the app's sRGB exports
    // and its masters, as JPEG, TIFF or PNG) names the working space itself:
    // those numbers ARE this pipeline's sRGB, so the read is the identity by
    // definition. Transformed anyway, qcms renders that compact profile's
    // sampled curve against its own parametric sRGB (575 212 of an 8-bit
    // cube's 50 331 648 samples moved, by up to 2 codes, 2026-09-26), and a
    // file the app wrote did not read back as the pixels it wrote.
    if profile == crate::render::SRGB_ICC {
        return Ok(());
    }
    let input = qcms::Profile::new_from_slice(profile, false)
        .ok_or_else(|| anyhow!("invalid ICC profile in {}", path.display()))?;
    let output = qcms::Profile::new_sRGB();
    let color_type = img.color();
    let (pixels, data_type): (&mut [u8], qcms::DataType) = match img {
        DynamicImage::ImageRgb8(rgb) => {
            (rgb.as_flat_samples_mut().samples, qcms::DataType::RGB8)
        }
        DynamicImage::ImageRgba8(rgba) => {
            (rgba.as_flat_samples_mut().samples, qcms::DataType::RGBA8)
        }
        // 16-bit is the MAIN customer: LR "Edit in…" hands over ProPhoto
        // 16-bit TIFFs, and refusing them here reintroduced the exact
        // workflow this function exists to serve (they opened fine — with
        // wrong colours — before the ICC pass landed).
        DynamicImage::ImageRgb16(_) | DynamicImage::ImageRgba16(_) => {
            return apply_icc_profile_16(img, &input, &output, path);
        }
        _ => anyhow::bail!(
            "ICC profile in {} accompanies {color_type:?} pixels, but qcms has no matching \
             transform that preserves this image's channel layout and bit depth",
            path.display()
        ),
    };
    let transform = qcms::Transform::new(
        &input,
        &output,
        data_type,
        qcms::Intent::Perceptual,
    )
    .ok_or_else(|| {
        anyhow!(
            "ICC profile in {} cannot transform {color_type:?} pixels into sRGB",
            path.display()
        )
    })?;
    transform.apply(pixels);
    Ok(())
}

/// The 16-bit arm of [`apply_icc_profile`]. qcms transforms 8-bit samples
/// only (DataType is RGB8/RGBA8/BGRA8/Gray8/GrayA8 — checked against qcms
/// 0.3's source), and rounding the IMAGE to 8 bits would trade the
/// colour-space error for permanent banding in every later tone move. So:
/// run the profile pair ONCE over a 52³ RGB lattice at qcms's native 8-bit
/// precision, then map the 16-bit samples through that lattice by trilinear
/// interpolation in f32. The colour mapping is 8-bit-accurate (≤1/255 per
/// lattice value — the transform's own output precision) while the DATA
/// keeps its 16-bit smoothness, because interpolation is continuous between
/// lattice points. ICC display-class transforms are smooth by construction,
/// so 52 points per axis track them closely.
///
/// 52, because a node's INPUT is an 8-bit integer while the lookup places
/// node i at exactly i/(N − 1) of full scale: N − 1 must divide 255, and
/// 51 × 5 = 255 puts every node on a code. The 33 nodes this used until
/// 2026-09-26 sat 255/32 = 7.97 codes apart, so the inputs truncated to
/// 8·i − 1 where the lookup assumed 7.97·i, and every 16-bit profiled read
/// came back darker: by 0.48 codes on average and 0.97 at worst through an
/// sRGB → sRGB transform qcms itself applies exactly.
fn apply_icc_profile_16(
    img: &mut DynamicImage,
    input: &qcms::Profile,
    output: &qcms::Profile,
    path: &Path,
) -> Result<()> {
    const N: usize = 52;
    let mut lattice = vec![0u8; N * N * N * 3];
    for r in 0..N {
        for g in 0..N {
            for b in 0..N {
                let i = ((r * N + g) * N + b) * 3;
                lattice[i] = (r * 255 / (N - 1)) as u8;
                lattice[i + 1] = (g * 255 / (N - 1)) as u8;
                lattice[i + 2] = (b * 255 / (N - 1)) as u8;
            }
        }
    }
    let transform = qcms::Transform::new(
        input,
        output,
        qcms::DataType::RGB8,
        qcms::Intent::Perceptual,
    )
    .ok_or_else(|| {
        anyhow!(
            "ICC profile in {} cannot transform RGB pixels into sRGB",
            path.display()
        )
    })?;
    transform.apply(&mut lattice);
    let sample = |rgb: [u16; 3]| -> [u16; 3] {
        let mut idx = [0usize; 3];
        let mut frac = [0f32; 3];
        for (c, v) in rgb.iter().enumerate() {
            let t = f32::from(*v) / 65535.0 * (N - 1) as f32;
            let i = (t as usize).min(N - 2);
            idx[c] = i;
            frac[c] = t - i as f32;
        }
        let at = |dr: usize, dg: usize, db: usize, ch: usize| -> f32 {
            let i = (((idx[0] + dr) * N + idx[1] + dg) * N + idx[2] + db) * 3 + ch;
            f32::from(lattice[i])
        };
        let mut out = [0u16; 3];
        for (ch, o) in out.iter_mut().enumerate() {
            let lerp = |a: f32, b: f32, t: f32| a + (b - a) * t;
            let c00 = lerp(at(0, 0, 0, ch), at(1, 0, 0, ch), frac[0]);
            let c10 = lerp(at(0, 1, 0, ch), at(1, 1, 0, ch), frac[0]);
            let c01 = lerp(at(0, 0, 1, ch), at(1, 0, 1, ch), frac[0]);
            let c11 = lerp(at(0, 1, 1, ch), at(1, 1, 1, ch), frac[0]);
            let c0 = lerp(c00, c10, frac[1]);
            let c1 = lerp(c01, c11, frac[1]);
            let v = lerp(c0, c1, frac[2]) / 255.0;
            *o = (v.clamp(0.0, 1.0) * 65535.0).round() as u16;
        }
        out
    };
    match img {
        DynamicImage::ImageRgb16(rgb) => {
            for px in rgb.as_flat_samples_mut().samples.chunks_exact_mut(3) {
                let [r, g, b] = sample([px[0], px[1], px[2]]);
                px.copy_from_slice(&[r, g, b]);
            }
        }
        DynamicImage::ImageRgba16(rgba) => {
            // Alpha is coverage, not colour — it rides through untouched.
            for px in rgba.as_flat_samples_mut().samples.chunks_exact_mut(4) {
                let [r, g, b] = sample([px[0], px[1], px[2]]);
                px[..3].copy_from_slice(&[r, g, b]);
            }
        }
        _ => unreachable!("routed here only for Rgb16/Rgba16"),
    }
    Ok(())
}

const MAX_ALLOC: u64 = 4 * 1024 * 1024 * 1024;

/// The accept/reject decision for a decode's peak allocation, extracted so a
/// test can pin the BOUNDARY: equality refuses (`>` instead of `>=` would
/// admit an exact-ceiling allocation and abort the process).
fn allocation_over_ceiling(need: u64) -> bool {
    need >= MAX_ALLOC
}

/// Peak output-buffer bytes for a decode followed by `apply_orientation`: the
/// four rotating variants allocate a SECOND full buffer (`rotate90()` /
/// `rotate270()` return new images; 180°/flips are in-place), so the old
/// `total_bytes()`-only check let peak storage reach twice the 4 GiB ceiling.
fn decode_peak_bytes(need: u64, orientation: image::metadata::Orientation) -> u64 {
    if matches!(
        orientation,
        image::metadata::Orientation::Rotate90
            | image::metadata::Orientation::Rotate270
            | image::metadata::Orientation::Rotate90FlipH
            | image::metadata::Orientation::Rotate270FlipH
    ) {
        need.saturating_mul(2)
    } else {
        need
    }
}

/// Bytes per SOURCE pixel the baked develop chain holds ON TOP of the decoded
/// buffer at its peak (`render_baked_to_image`). The f32 planes
/// (`Vec<[f32; 3]>`, 12 B/px) are alive from the transcode to the end of the
/// function; what rides ALONGSIDE them is what varies, and the peak is the
/// worst of those moments — enumerated as data (not prose) by
/// `develop_peak_accounts_for_every_pass`, which is where a new stage lands.
///
/// Several of those moments tie at 24: a spatial pass (12 + at most three
/// working f32 planes 12 — luma and `blur_plane`'s two chained planes, or the
/// Detail panel's own three, v1.5.0), a geometric resample (12 + the Rgb16
/// frame 6 + the resampler's fresh output 6) and the AI-denoise round trip.
/// The chain has been tuned to that ceiling deliberately (the A7 batch), so
/// "12 + 6 + 6" is one path to 24 rather than the whole account.
///
/// A further full-frame stage raises this constant only if it is alive AT THE
/// SAME TIME as the planes: SEQUENTIAL spatial passes (clarity, texture, the
/// three Detail passes) each drop their planes before the next allocates
/// (`render.rs`'s memory note on `apply_masks`), so adding one costs nothing
/// here. A stage that holds a new buffer concurrently costs its full width —
/// and a new spatial pass that needs more than three working planes at once
/// is a new row in `develop_peak_accounts_for_every_pass`, not a free one.
///
/// **R27 R7 — re-derived, not re-assumed, when the RAW format list went from 9
/// extensions to 24.** The worry raised was "this was tuned on a 61 MP Bayer
/// A7; what about a 100 MP GFX or a Phase One back?". Two facts answer it:
///
/// 1. **It never applies to a RAW at all.** The only consumer is
///    [`develop_peak_bytes`], reached only from `load_image_gated(develop =
///    true)` — and that gate REFUSES a camera RAW by name a few lines in. A
///    RAW's develop is budgeted by the decode-scope lifetimes in
///    `render::render_to_image_in`, not here. So sensor size and CFA layout
///    (Bayer vs X-Trans vs 4-colour) are outside this constant's world; what
///    it budgets is a baked PNG/TIFF/JPEG, which has no CFA by definition.
/// 2. **It is per-SOURCE-PIXEL, so it does not carry a resolution
///    assumption.** Every term above is a fixed number of bytes for one pixel
///    (12 B of `[f32; 3]`, 4 B luma, 6 B of Rgb16, …); the pixel COUNT is the
///    other multiplicand in `develop_peak_bytes` and comes from the file's own
///    header. Doubling the megapixels doubles the product, which is the
///    intended behaviour — it is what makes the 4 GiB ceiling bite on a
///    200 MP panorama and not on a 24 MP export.
///
/// What "tuned to that ceiling deliberately (the A7 batch)" refers to is WHICH
/// STAGES exist and which of them overlap — the enumeration in
/// `develop_peak_accounts_for_every_pass` — not a pixel count. That
/// enumeration is what a new stage must update; the 61 MP number never enters
/// the arithmetic.
const PIPELINE_BYTES_PER_PIXEL: u64 = 24;

/// [`decode_peak_bytes`] plus the develop chain's own full-frame buffers —
/// what the baked develop entry point really allocates for a source of
/// `pixels` px. Pure, so the boundary test can pin the accounting.
fn develop_peak_bytes(
    decoded_bytes: u64,
    orientation: image::metadata::Orientation,
    pixels: u64,
) -> u64 {
    decode_peak_bytes(decoded_bytes, orientation)
        .saturating_add(pixels.saturating_mul(PIPELINE_BYTES_PER_PIXEL))
}

/// Bytes of COMMIT charge one SOURCE pixel of a camera RAW costs at the PEAK of
/// `render::render_to_image_in` — the RAW twin of [`PIPELINE_BYTES_PER_PIXEL`],
/// and what lets the same 4 GiB per-file ceiling be charged to a RAW at all.
///
/// **MEASURED, not derived.** 1,771 MB of peak process commit over a
/// 9504x6336 (60.2 MP) A7R V ARW = 30.8 B/px, read as `PeakPagefileUsage` by
/// `jobs::tests::probe_per_photo_peak_commit` (release profile, one stage per
/// process — that probe's own docs carry the method, `jobs`' module docs the
/// stage table). Rounded UP to 31: this number multiplies a pixel count into a
/// REFUSAL, and rounding a peak DOWN is how a file that really does blow the
/// ceiling gets admitted.
///
/// v1.17.0's signed-black Bayer develop (`render::develop_cfa_signed`) no
/// longer goes through rawler's input clone: the same probe on a 60.2 MP
/// ILCE-7RM4A frame reads 1,394 MB = 24.3 B/px, against 1,745 MB = 30.4 B/px
/// before the change on that frame. The constant keeps the older, larger
/// reading; lowering it would admit files, which is a decision, not a
/// measurement.
///
/// It is the WHOLE develop's peak, not one buffer's width. The demosaiced f32
/// frame (`Vec<[f32; 3]>`, 12 B/px) is only its largest single term; rawler's
/// own sensor buffers, the mapped file, the orientation transient and the
/// 16-bit pack are all inside the measured figure. That is deliberate — the
/// baked side can enumerate its overlapping stages because it OWNS them
/// (`PIPELINE_BYTES_PER_PIXEL`'s doc does exactly that), while a RAW's peak is
/// mostly inside rawler, where an enumeration would be a guess about somebody
/// else's allocations. A measurement of the whole is the honest form.
///
/// **Corpus caveat, stated rather than hidden**: one body, one CFA, one
/// compression mode. A format whose decoder holds more than rawler's ARW path
/// does would peak higher per pixel, and this constant would then admit a file
/// it should refuse. Re-run the probe when the RAW format list grows a decoder
/// with a different shape (the same discipline `PIPELINE_BYTES_PER_PIXEL`'s
/// R27 R7 note records for the baked side).
pub const RAW_DEVELOP_BYTES_PER_PIXEL: u64 = 31;

/// What developing a `pixels`-pixel camera RAW peaks at. Pure, so the boundary
/// test can pin the accounting the way it pins the baked one.
fn raw_develop_peak_bytes(pixels: u64) -> u64 {
    pixels.saturating_mul(RAW_DEVELOP_BYTES_PER_PIXEL)
}

/// THE RAW DEVELOP CEILING — the twin of the baked gate inside
/// [`load_image_gated`], and the closing of what the R28 adjudication (F2)
/// called the deeper root: the baked path has refused an over-ceiling file
/// since L02, while **the RAW path had no per-file limit at all**. A 150 MP
/// IIQ on the default `batch --jobs 3` was therefore a worse instance of the
/// same defect than the constructed TIFF scenario that raised it, with nothing
/// opt-in about it.
///
/// ONE call site, `render::render_to_image_in` — the single funnel every RAW's
/// pixels pass through (`render_to_file`'s RAW arm, `source_pixels`' RAW arm,
/// `render_to_image`, and through those three the CLI, the GUI and `serve`).
/// Patching the callers instead would have been the shape this file already
/// refuses for `load_image` (see that gate's doc): a per-caller copy is a
/// per-caller chance to forget.
///
/// REFUSE, never degrade (user ruling 2026-08-20, plan D2): the alternative —
/// silently developing at a reduced resolution — would hand back a deliverable
/// that is not the one asked for.
fn refuse_raw_develop_over_ceiling(path: &Path, pixels: u64) -> Result<()> {
    let need = raw_develop_peak_bytes(pixels);
    if allocation_over_ceiling(need) {
        // Everything a person can act on: the measured basis (so the estimate
        // is auditable rather than an oracle), and — because the obvious guess
        // is wrong — that the concurrency flag is not the answer.
        anyhow::bail!(
            "{} is a {pixels}-pixel camera RAW; developing it peaks at about {need} bytes \
             ({pixels} px x {RAW_DEVELOP_BYTES_PER_PIXEL} B/px, measured) — at or over the \
             {MAX_ALLOC}-byte ceiling this build will commit for ONE file. `--jobs 1` does not \
             help: this is a single file's own peak, not a concurrency budget. This build cannot \
             develop a photo this large without paging, so it refuses instead of stalling the \
             machine or aborting on a failed allocation",
            path.display()
        );
    }
    Ok(())
}

/// [`refuse_raw_develop_over_ceiling`] against a decoder's DUMMY `RawImage` —
/// the metadata-only probe `render::render_to_image_in` takes before it
/// decompresses a single sensor row.
///
/// The frame rule ([`default_crop`]) stays on THIS side of the wall so the
/// ceiling is charged against the same rectangle the develop will actually
/// produce, decided in the one place that owns that rule — rather than the
/// render re-deriving "which pixels are the picture" for the purpose of a
/// budget and drifting from the answer it uses for pixels.
pub(crate) fn refuse_raw_develop_over_ceiling_for(
    path: &Path,
    probe: &rawler::RawImage,
) -> Result<()> {
    let d = default_crop(probe).d;
    refuse_raw_develop_over_ceiling(path, (d.w as u64).saturating_mul(d.h as u64))
}

/// Grain return develops an original in addition to the clean RAW. Reserve
/// one RGB (12 B), one luminance (4 B) and one mosaic clone (2 B) per source
/// pixel above the measured ordinary develop budget. The two RGB frames are
/// never live together; this conservative allowance also covers allocator
/// retention between the passes. The 60.2 MP captured-mosaic probe measured
/// 1765 MiB at strength 1, 1996 MiB at 0.71 (jobs.rs); the conservative 49 B/px
/// gate also leaves room for other decoders. Neither includes the sidecar.
pub(crate) fn refuse_raw_grain_return_over_ceiling_for(path: &Path, probe: &rawler::RawImage) -> Result<()> {
    let d = default_crop(probe).d;
    let pixels = (d.w as u64).saturating_mul(d.h as u64);
    refuse_raw_grain_return_over_ceiling(path, pixels)
}

fn refuse_raw_grain_return_over_ceiling(path: &Path, pixels: u64) -> Result<()> {
    let need = raw_develop_peak_bytes(pixels).saturating_add(pixels.saturating_mul(18));
    if allocation_over_ceiling(need) {
        anyhow::bail!("{} needs about {need} bytes for RAW develop plus luminance grain return, \
            at or over the {MAX_ALLOC}-byte per-file ceiling; strength 1 skips the original develop",
            path.display());
    }
    Ok(())
}

/// The per-file develop peak in MB for a source whose HEADER is CHEAP to read,
/// or `None` when it is not — the admission-time half of the same accounting
/// the ceilings above enforce.
///
/// **Baked**: `image`'s reader parses a header and decodes no pixel, so the
/// real per-file peak is available for the price of an `open` +
/// `into_decoder`. `jobs.rs`' "reading each RAW's dimensions to size the
/// budget would cost a decode per photo" is simply not true here, which is why
/// the planner may consult this.
///
/// **Camera RAW**: `None`. Answering would cost `RawSource::new` — the WHOLE
/// file mapped (~120 MB for a 61 MP ARW) — per photo before the pool even
/// starts, which is exactly the cost that reasoning was about. The RAW side is
/// bounded instead by [`refuse_raw_develop_over_ceiling`] at the develop door,
/// plus the corpus constant in the planner.
///
/// Best-effort by construction: an unreadable header answers `None` rather than
/// failing, because a PLAN must not be the thing that fails a run. The photo's
/// own develop will surface the real error, loudly, where the diagnosis lives.
pub fn cheap_develop_peak_mb(path: &Path) -> Option<u64> {
    if is_raw(path) {
        return None;
    }
    // baked-by-construction: the !is_raw arm, decided one line up.
    Some(baked_header_peak_bytes(path).ok()?.div_ceil(1024 * 1024))
}

/// [`develop_peak_bytes`] from a HEADER alone — no pixel decoded.
///
/// Reads through [`baked_reader`], the same raised limits the pixel path uses:
/// a probe running under the crate's 512 MB default `max_alloc` would refuse
/// to build a decoder for exactly the big exports this estimate exists for,
/// and answering "unreadable" for them would silently plan as if they were
/// small.
fn baked_header_peak_bytes(path: &Path) -> Result<u64> {
    use image::ImageDecoder as _;
    let mut decoder = baked_reader(path)?
        .into_decoder()
        .with_context(|| format!("read the header of {}", path.display()))?;
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let (dw, dh) = decoder.dimensions();
    Ok(develop_peak_bytes(
        decoder.total_bytes(),
        orientation,
        u64::from(dw).saturating_mul(u64::from(dh)),
    ))
}

/// Load a baked raster (PNG/TIFF/JPEG) with a RAISED (not lifted) decoder
/// memory limit — a 60 MP export trips the image crate's default cap, but
/// `no_limits()` let a corrupt header with absurd declared dimensions drive an
/// unbounded allocation straight into OOM. 61 MP 16-bit RGBA is ~0.5 GiB;
/// 4 GiB leaves headroom without trusting arbitrary headers; guided-mask
/// refinement keeps its derived planes tile-bounded, so the two limits no
/// longer stack into an unbudgeted full-frame peak.
/// Also applies the EXIF orientation: phone/Lightroom JPEGs store rotation as
/// metadata the decoder does NOT apply — imported photos rendered sideways
/// (the RAW path already orients via the sensor metadata).
///
/// BAKED ONLY: a camera RAW is REFUSED here (see [`load_image_gated`]) — it has
/// no `image`-crate decoder, so the honest gate is a named error, not a probe
/// failure. Callers that may hold either kind of source want the one dispatch,
/// [`crate::render::source_pixels`].
// not-a-consumer-call: the gate's own declaration.
pub fn load_image(path: &Path) -> Result<DynamicImage> {
    load_image_gated(path, false) // not-a-consumer-call: dispatch inside the gate
}

/// [`load_image`] for the full-frame baked DEVELOP path
/// (`render_baked_to_image`): charges each source pixel the develop chain's
/// downstream footprint ([`PIPELINE_BYTES_PER_PIXEL`]) on top of the decode
/// buffer, so the ceiling bounds the true pipeline peak — the plain gate
/// admitted an L8 source whose develop then peaked at ~25× the ceiling (L02).
/// Thumbnail consumers (GUI open, denoise/retouch/fit pre-shrink) stay on
/// [`load_image`]: they never build those planes, and charging them would
/// refuse sources they legitimately shrink.
// not-a-consumer-call: the gate's develop-charged twin.
pub fn load_image_for_develop(path: &Path) -> Result<DynamicImage> {
    load_image_gated(path, true) // not-a-consumer-call: dispatch inside the gate
}

/// A baked raster's reader, opened with the RAISED (not lifted) decoder limits
/// and its format already probed.
///
/// ONE construction, because there are now THREE consumers and they must
/// agree: the pixel path ([`load_image_gated`]), the header-only peak estimate
/// ([`baked_header_peak_bytes`]) and the header-only frame read
/// ([`source_window`]). Under the crate's own `Limits::default()`
/// the TIFF codec's `set_limits` refuses to build a decoder whose frame is
/// larger than the default 512 MB `max_alloc` — so an estimate that opened its
/// own plain reader would answer "unreadable" for precisely the 60 MP-plus
/// exports it exists to size, and the caller would plan as if they were small.
/// The 65,536-px dimension bounds and the 4 GiB allocation bound below are the
/// ones that gate's doc argues for; they live here so neither consumer can
/// drift off them.
#[allow(clippy::disallowed_methods)] // the one baked-image construction door
fn baked_reader(
    path: &Path,
) -> Result<image::ImageReader<std::io::BufReader<std::fs::File>>> {
    let mut reader = image::ImageReader::open(path)
        .with_context(|| format!("open image {}", path.display()))?;
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(65_536);
    limits.max_image_height = Some(65_536);
    limits.max_alloc = Some(MAX_ALLOC);
    reader.limits(limits);
    reader
        .with_guessed_format()
        .with_context(|| format!("probe image {}", path.display()))
}

/// THE ROUTING TABLE for the refusal below, kept in the doc rather than in the
/// error text (R24 batch 2): a RAW that arrived here belongs in
/// [`crate::render::render_to_image`] or [`crate::render::source_pixels`] (the
/// one raw-vs-baked dispatch), or in [`decode_any`] when the caller wants the
/// sensor data rather than a picture.
// not-a-consumer-call: the gate's body — where a camera RAW is refused by name.
fn load_image_gated(path: &Path, develop: bool) -> Result<DynamicImage> {
    use image::ImageDecoder as _;
    // The "RAW → develop engine / baked → here" dispatch, enforced at the
    // GATE instead of trusting every caller to hand-copy an `is_raw` branch.
    // A missed branch used to reach `ImageReader` with a .ARW and surface as
    // an unrelated format/probe error (v0.22's mask-refine worker: "The image
    // format could not be determined" for a photo the app had just developed
    // on screen) — the class this refuses by name, wherever it happens.
    if is_raw(path) {
        // The sentence a USER may read. It used to end in three Rust paths
        // (`render::render_to_image / render::source_pixels`, `decode::
        // decode_any`), which is a developer's routing table shown in a
        // desktop toast and a web error body — the two surfaces this gate
        // actually reaches (the CLI routes RAWs before they arrive here). The
        // routing table now lives one screen up, in this function's own doc,
        // where the developer who needs it is already reading; what stays here
        // is the fact and the way out, in words the person holding the photo
        // can act on.
        anyhow::bail!(
            "{} is a camera RAW, and this step reads finished images \
             (PNG/TIFF/JPEG) only — a RAW has to be developed before anything \
             can read it as a picture",
            path.display()
        );
    }
    let reader = baked_reader(path)?;
    let format = reader.format();
    // R11: a camera RAW wearing a `.tif` extension reaches HERE, because
    // `is_raw` is extension-based and a DNG (or a CR2, or a NEF) really is a
    // TIFF container. The `image` crate would then decode whichever IFD it
    // finds first — on a DNG that is the small embedded THUMBNAIL, so the
    // photo would open, look right at a glance, and be developed at a few
    // hundred pixels. That is worse than a refusal, so it IS a refusal, and it
    // names the way out. Placed BEFORE `into_decoder` so the answer is this
    // sentence rather than whatever the TIFF codec makes of a sensor plane.
    // Costs one header parse on a baked TIFF open, next to nothing beside the
    // pixel decode below.
    if format == Some(image::ImageFormat::Tiff)
        && let Some(kind) = raw_in_tiff_clothing(path)
    {
        anyhow::bail!(
            "{} is named .tif but is really a camera RAW ({kind}) — rename it to its real \
             extension (e.g. .dng) so AutoShade develops the sensor instead of reading the \
             thumbnail the `image` crate would find first",
            path.display()
        );
    }
    let mut decoder = reader
        .into_decoder()
        .with_context(|| format!("decode image {}", path.display()))?;
    // into_decoder enforces the DIMENSION limits but skips decode()'s
    // total_bytes reservation — without this check max_alloc never bounded
    // the OUTPUT buffer (a 65536² 16-bit RGBA header passes the dimension
    // gate yet decodes to ~32 GiB).
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let decoded_bytes = decoder.total_bytes();
    let decoded_color = decoder.color_type();
    let (dw, dh) = decoder.dimensions();
    let need = if develop {
        develop_peak_bytes(
            decoded_bytes,
            orientation,
            u64::from(dw).saturating_mul(u64::from(dh)),
        )
    } else {
        decode_peak_bytes(decoded_bytes, orientation)
    };
    if allocation_over_ceiling(need) {
        anyhow::bail!(
            "image {} needs {need} bytes at peak (decode {decoded_bytes}, then orientation{}) \
             — at or over the {MAX_ALLOC}-byte ceiling",
            path.display(),
            if develop { ", then the develop chain's full-frame buffers" } else { "" }
        );
    }
    let icc_profile = decoder
        .icc_profile()
        .with_context(|| format!("read ICC profile {}", path.display()))?;
    // image 0.25's TiffDecoder::set_limits breaks IFD tag-value reads: the
    // tiff crate accounts a tag read as count × size_of::<Value>() (~32 bytes
    // per profile BYTE) against `decoding_buffer_size`, which set_limits pins
    // to the image's own byte size — probed empirically ("decoder limits
    // exceeded", swallowed by `.ok()` inside the codec into a silent None).
    // Big LR exports clear the budget; small profiled TIFFs would silently
    // skip the transform — the exact assume-sRGB bug this function fixes. Ask
    // a fresh, header-only decoder instead; its tag reads run under the tiff
    // crate's own defaults, where the one budget a tag value meets is the
    // 256 MiB `decoding_buffer_size` at those ~32 bytes per profile byte —
    // room for an ~8 MiB profile, far past any ICC file (`ifd_value_size`, the
    // "1 MiB per value" this comment used to cite, is declared by tiff 0.11 and
    // enforced nowhere) — and NO pixel is ever decoded here. The read goes
    // through the `tiff` crate itself, not image's `TiffDecoder::icc_profile`,
    // which folds EVERY tiff error into `None` (image 0.25.10
    // `codecs/tiff.rs`) — see the arm below for what that hid until
    // 2026-09-24.
    let icc_profile = match icc_profile {
        Some(p) => Some(p),
        None if format == Some(image::ImageFormat::Tiff) => {
            // The re-probe obeys apply_icc_profile's own rule (its doc):
            // unreadable colour management is a HARD error, never a silent
            // assume-sRGB fall-through — "that fall-through IS the bug".
            // The old .ok() pair folded a failed profile READ into "no
            // profile" (L05-2). Ok(None) is the real no-profile case.
            //
            // Through the `tiff` crate DIRECTLY (2026-09-24): image's probe is
            // `Ok(get_tag_u8_vec(IccProfile).ok())`, so a profile tag of the
            // wrong type or an I/O error inside the tag read came back as
            // "untagged" and the file opened as sRGB. Here only "no such tag"
            // is untagged; every other answer is the hard error the rule asks.
            let file = std::fs::File::open(path)
                .with_context(|| format!("open image {}", path.display()))?;
            let mut probe = tiff::decoder::Decoder::new(std::io::BufReader::new(file))
                .with_context(|| format!("read the ICC profile of {}", path.display()))?;
            match probe.get_tag_u8_vec(tiff::tags::Tag::IccProfile) {
                Ok(profile) => Some(profile),
                Err(tiff::TiffError::FormatError(tiff::TiffFormatError::RequiredTagNotFound(_))) => None,
                Err(e) => {
                    return Err(anyhow::Error::new(e)
                        .context(format!("read the ICC profile of {}", path.display())));
                }
            }
        }
        None => None,
    };
    // R10: the assume-sRGB fall-through `apply_icc_profile` refuses is only
    // refusable when a profile EXISTS. An UNTAGGED file has nothing to refuse,
    // and is read as sRGB — right for essentially every 8-bit JPEG (the web's
    // default and what phones write), and a real risk for 16-bit, which is
    // what an editor produces: Lightroom's "Edit in…" hands over ProPhoto,
    // Photoshop happily saves untagged AdobeRGB. So the disclosure is aimed at
    // exactly that population instead of warning on every ordinary snapshot —
    // a warning that fires on the common correct case is one nobody reads.
    if icc_profile.is_none()
        && matches!(
            decoded_color,
            image::ColorType::Rgb16 | image::ColorType::Rgba16 | image::ColorType::La16
                | image::ColorType::L16
        )
    {
        eprintln!(
            "⚠ {} is 16-bit but carries no ICC profile, so its pixels are being read as sRGB. \
             If it was exported as ProPhoto or Adobe RGB (Lightroom's \"Edit in…\" default is \
             ProPhoto), every tone and colour decision below is computed on the wrong numbers — \
             re-export it with the profile embedded",
            path.display()
        );
    }
    let mut img = DynamicImage::from_decoder(decoder)
        .with_context(|| format!("decode image {}", path.display()))?;
    if let Some(profile) = icc_profile {
        apply_icc_profile(&mut img, &profile, path)?;
    }
    img.apply_orientation(orientation);
    Ok(img)
}

/// Is this TIFF-container file actually a camera RAW (R11)? Names the marker
/// that gave it away, or `None` for an ordinary baked TIFF.
///
/// Header-only, and deliberately conservative: it looks for the two tags that
/// no photo editor writes into a delivery TIFF — `DNGVersion` (0xC612), which
/// only a DNG carries, and `SubIFDs` combined with a `Make` string, which is
/// the shape every TIFF-based RAW (CR2/NEF/ARW/ORF/PEF/SRW/…) uses to hang the
/// sensor plane off the root IFD. A file that will not even parse as TIFF is
/// not our problem here — the decoder below reports it.
fn raw_in_tiff_clothing(path: &Path) -> Option<&'static str> {
    /// `DNGVersion`, the tag that DEFINES a DNG (rawler dispatches on it —
    /// `decoders/mod.rs:919-921`).
    const DNG_VERSION: u16 = 0xC612;
    let file = std::fs::File::open(path).ok()?;
    let mut reader = std::io::BufReader::new(file);
    let tiff = GenericTiffReader::new(&mut reader, 0, 0, Some(16), &[]).ok()?;
    // Same panic-shaped `root_ifd()` as `baked_exif`; a metadata probe must
    // never be the thing that ends the process.
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let root = tiff.root_ifd();
        if root.get_entry(DNG_VERSION).is_some() {
            return Some("it carries the DNGVersion tag");
        }
        let has_make = root
            .get_entry(TiffCommonTag::Make)
            .and_then(|e| e.value.as_string().cloned())
            .is_some_and(|s| !s.trim().is_empty());
        if has_make && root.get_sub_ifd(TiffCommonTag::SubIFDs).is_some() {
            return Some("it names a camera and hangs its sensor data off a SubIFD");
        }
        None
    }))
    .ok()
    .flatten()
}

/// Decode any supported source — a camera RAW or an already-baked image. The
/// baked path (the "PNG source" mode: edit an LR/PS-denoised export) has no
/// sensor metadata, so [`Meta`] is filled with neutral defaults and the
/// histogram is computed from the pixels.
pub fn decode_any(path: &Path) -> Result<Decoded> {
    decode_any_turned(path, 0)
}

/// [`decode_any`] in the frame the photographer's quarter turns produce — the
/// dispatch twin of [`decode_raw_turned`]. The baked arm turns the decoded
/// pixels directly: `load_image` has already applied their EXIF orientation,
/// so only the user's half is left, and `Meta`'s dims are re-read from the
/// turned image rather than swapped by hand.
pub fn decode_any_turned(path: &Path, quarter_turns: u8) -> Result<Decoded> {
    if is_raw(path) {
        return decode_raw_turned(path, quarter_turns);
    }
    let mut d = decode_baked(path)?;
    match crate::render::quarter_turn_orientation(quarter_turns) {
        rawler::Orientation::Normal | rawler::Orientation::Unknown => {}
        o => {
            d.preview = crate::render::oriented(d.preview, o);
            d.meta.width = d.preview.width() as usize;
            d.meta.height = d.preview.height() as usize;
        }
    }
    Ok(d)
}

/// The seven capture facts [`Meta`] carries that come from EXIF rather than
/// from the sensor — ONE extraction shared by the RAW arm and the baked arm
/// (P2). Splitting them was the old bug in miniature: the RAW side grew an
/// APEX `ApertureValue` fallback and two finiteness filters that a
/// hand-written baked copy would not have had, so the same JPEG would have
/// reported a different f-number depending on which door it came through.
struct ExifFacts {
    lens: Option<String>,
    iso: Option<u32>,
    shutter: Option<String>,
    aperture: Option<f32>,
    focal_length_mm: Option<f32>,
    exposure_bias_ev: Option<f32>,
    date_time: Option<String>,
}

fn exif_facts(exif: &rawler::exif::Exif) -> ExifFacts {
    ExifFacts {
        lens: exif.lens_model.clone().or_else(|| exif.lens_make.clone()),
        iso: exif.iso_speed_ratings.map(u32::from).or(exif.iso_speed),
        shutter: exif.exposure_time.as_ref().map(fmt_shutter),
        aperture: exif
            .fnumber
            .as_ref()
            .map(ratio)
            // An INVALID FNumber (0-denominator → 0.0, or non-finite) must
            // fall through to the Av fallback, not suppress it: filtering
            // only at the end let f/0 shadow a perfectly valid ApertureValue.
            .filter(|v| v.is_finite() && *v > 0.0)
            // ApertureValue is an APEX Av, not an f-number: N = 2^(Av/2)
            // (Av 4 ⇒ f/4, Av 5 ⇒ f/5.7). Feeding it raw overstated fast
            // lenses in metadata + the AI prompt whenever FNumber was absent.
            .or_else(|| exif.aperture_value.as_ref().map(|v| (ratio(v) / 2.0).exp2()))
            // Same physical-validity rule for the fallback (huge Av → ∞;
            // serde_json refuses non-finite floats — the WB-coeff rule).
            .filter(|v| v.is_finite() && *v > 0.0),
        focal_length_mm: exif
            .focal_length
            .as_ref()
            .map(ratio)
            .filter(|v| v.is_finite() && *v > 0.0),
        exposure_bias_ev: exif.exposure_bias.as_ref().map(sratio).filter(|v| v.is_finite()),
        date_time: exif.date_time_original.clone(),
    }
}

/// Largest EXIF block this reader will take from a baked file. A JPEG APP1
/// segment cannot exceed 65533 payload bytes by construction (the length field
/// is 16-bit); a TIFF's header chain is walked, not buffered. 1 MiB is
/// therefore pure belt-and-braces against a hand-built file, and it is a
/// REFUSAL rather than a truncation — half an IFD parses to different numbers,
/// which is the failure mode this whole module refuses everywhere else.
const MAX_BAKED_EXIF: usize = 1024 * 1024;

/// How far into a JPEG this reader will hunt for the EXIF segment before
/// giving up. EXIF is required to be the FIRST APP segment, so anything past
/// a few hundred KB of headers is not a file that follows the spec; the cap
/// stops a crafted file from turning a metadata read into a whole-file scan.
const MAX_JPEG_HEADER_SCAN: u64 = 4 * 1024 * 1024;

/// The TIFF/EXIF block of a JPEG: the payload of the first `APP1` segment
/// whose six leading bytes are `Exif\0\0`, with that introducer stripped so
/// what comes back starts at the TIFF header (`II`/`MM`) rawler's reader
/// expects.
///
/// Markers are walked, not searched for: scanning the file for the byte pair
/// would hit `FFE1` inside compressed scan data. `D0..=D7` (restart), `01` and
/// `D8` are the standalone markers that carry no length; `DA` (start of scan)
/// ends the header region — everything after it is entropy-coded.
/// Read-only by construction — no `Seek`. Skipping a segment copies its bytes
/// to a sink rather than seeking past them, so the walk cannot depend on how
/// `Take`/`BufReader` compose their cursors; the cost is reading header bytes
/// that were about to be read anyway, bounded by [`MAX_JPEG_HEADER_SCAN`].
fn jpeg_exif_block(file: &mut std::fs::File) -> Result<Option<Vec<u8>>> {
    use std::io::Read as _;
    let mut r = std::io::BufReader::new(file.by_ref().take(MAX_JPEG_HEADER_SCAN));
    let mut soi = [0u8; 2];
    if r.read_exact(&mut soi).is_err() || soi != [0xFF, 0xD8] {
        return Ok(None);
    }
    loop {
        // Marker prefixes may be padded with any number of 0xFF fill bytes.
        let mut b = [0u8; 1];
        if r.read_exact(&mut b).is_err() {
            return Ok(None);
        }
        if b[0] != 0xFF {
            return Ok(None); // desynchronised — not our business to repair
        }
        while b[0] == 0xFF {
            if r.read_exact(&mut b).is_err() {
                return Ok(None);
            }
        }
        let marker = b[0];
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue; // standalone, no length field
        }
        if marker == 0xDA || marker == 0xD9 {
            return Ok(None); // scan data / end of image — no EXIF in this file
        }
        let mut len = [0u8; 2];
        if r.read_exact(&mut len).is_err() {
            return Ok(None);
        }
        let payload = u64::from(u16::from_be_bytes(len)).saturating_sub(2);
        if marker != 0xE1 {
            // A short copy (truncated file) is not an error here — the next
            // read_exact fails and the walk ends with "no EXIF", which is the
            // honest answer for a file that stops mid-header.
            if std::io::copy(&mut r.by_ref().take(payload), &mut std::io::sink()).is_err() {
                return Ok(None);
            }
            continue;
        }
        if payload > MAX_BAKED_EXIF as u64 {
            anyhow::bail!("the EXIF segment is larger than the {MAX_BAKED_EXIF}-byte limit");
        }
        let mut buf = vec![0u8; payload as usize];
        if r.read_exact(&mut buf).is_err() {
            return Ok(None);
        }
        // An APP1 that is not EXIF is almost always the XMP packet
        // (`http://ns.adobe.com/xap/1.0/`), which is not ours to read here.
        if buf.starts_with(b"Exif\0\0") {
            return Ok(Some(buf.split_off(6)));
        }
    }
}

/// The XMP packet of a JPEG: the `http://ns.adobe.com/xap/1.0/\0` APP1
/// segment, with any **ExtendedXMP** continuation chunks reassembled onto it.
///
/// **Why the second half is not optional** (R27, `P5-cropped-mask-frame.md`
/// §8). A JPEG APP1 segment holds at most 65533 payload bytes, and a Lightroom
/// develop block with masks routinely exceeds that: **13 exports in the user's
/// own library** are split this way, including `P30_1.jpg` — one of the
/// seven photographs P3's crop model rests on. The continuation lives in
/// further APP1 segments introduced by `http://ns.adobe.com/xmp/extension/\0`,
/// each carrying the 32-hex GUID of the extension it belongs to, the total
/// length, and its own byte offset. A reader that simply concatenates
/// `<x:xmpmeta>…</x:xmpmeta>` out of the raw bytes bridges the segment headers
/// and produces XML that is not well-formed; a reader that takes only the
/// standard segment gets a truncated document. Both were observed on this
/// library, in P5's own first pass.
///
/// The chunks are placed BY OFFSET, not by arrival order, and a chain with a
/// hole in it is `Err` rather than a quietly short document — the same rule the
/// caller applies to a packet it cannot decode. Only the GUID the standard
/// packet names (`xmpNote:HasExtendedXMP`) is accepted, so a second edit
/// generation's leftover chunks cannot splice themselves into this one.
///
/// The standard packet comes back FIRST and whole: `crs:` settings live in it,
/// and the extension carries the overflow (usually the AI-mask rasters).
fn jpeg_xmp_packet(file: &mut std::fs::File) -> Result<Option<Vec<u8>>> {
    use std::io::Read as _;
    const STD: &[u8] = b"http://ns.adobe.com/xap/1.0/\0";
    const EXT: &[u8] = b"http://ns.adobe.com/xmp/extension/\0";
    /// GUID (32) + total length (4) + offset (4).
    const EXT_HEADER: usize = 40;
    let mut r = std::io::BufReader::new(file.by_ref().take(MAX_JPEG_HEADER_SCAN));
    let mut soi = [0u8; 2];
    if r.read_exact(&mut soi).is_err() || soi != [0xFF, 0xD8] {
        return Ok(None);
    }
    let mut standard: Option<Vec<u8>> = None;
    let mut chunks: Vec<(u32, Vec<u8>)> = Vec::new();
    let mut total: Option<u32> = None;
    loop {
        let mut b = [0u8; 1];
        if r.read_exact(&mut b).is_err() {
            break;
        }
        if b[0] != 0xFF {
            break; // desynchronised — not our business to repair
        }
        while b[0] == 0xFF {
            if r.read_exact(&mut b).is_err() {
                return Ok(standard);
            }
        }
        let marker = b[0];
        if marker == 0xD8 || marker == 0x01 || (0xD0..=0xD7).contains(&marker) {
            continue; // standalone, no length field
        }
        if marker == 0xDA || marker == 0xD9 {
            break; // scan data / end of image — the header region is over
        }
        let mut len = [0u8; 2];
        if r.read_exact(&mut len).is_err() {
            break;
        }
        let payload = u64::from(u16::from_be_bytes(len)).saturating_sub(2);
        if marker != 0xE1 {
            if std::io::copy(&mut r.by_ref().take(payload), &mut std::io::sink()).is_err() {
                break;
            }
            continue;
        }
        let mut buf = vec![0u8; payload as usize];
        if r.read_exact(&mut buf).is_err() {
            break;
        }
        if buf.starts_with(STD) {
            standard.get_or_insert_with(|| buf[STD.len()..].to_vec());
        } else if buf.starts_with(EXT) && buf.len() >= EXT.len() + EXT_HEADER {
            let head = &buf[EXT.len()..EXT.len() + EXT_HEADER];
            let guid = head[..32].to_ascii_uppercase();
            // The standard packet NAMES the extension it owns; anything else
            // belongs to some other generation of this file.
            let owned = standard.as_deref().is_some_and(|s| {
                twoway_contains(s, &guid)
            });
            if !owned {
                continue;
            }
            let len = u32::from_be_bytes([head[32], head[33], head[34], head[35]]);
            let off = u32::from_be_bytes([head[36], head[37], head[38], head[39]]);
            if total.is_some_and(|t| t != len) {
                anyhow::bail!("its ExtendedXMP chunks disagree about the total length");
            }
            total = Some(len);
            chunks.push((off, buf[EXT.len() + EXT_HEADER..].to_vec()));
        }
    }
    let Some(std_packet) = standard else { return Ok(None) };
    if chunks.is_empty() {
        return Ok(Some(std_packet));
    }
    chunks.sort_by_key(|(off, _)| *off);
    let mut ext = Vec::new();
    for (off, body) in chunks {
        if off as usize != ext.len() {
            anyhow::bail!(
                "its ExtendedXMP chain has a gap at byte {off} (had {} bytes)",
                ext.len()
            );
        }
        ext.extend_from_slice(&body);
    }
    if total.is_some_and(|t| t as usize != ext.len()) {
        anyhow::bail!("its ExtendedXMP chain is short of the length it declares");
    }
    // Two packets, one after the other: each is a complete `<x:xmpmeta>`
    // document, and every reader in this crate scans for the settings rather
    // than assuming one root (the same shape a RAW's own multi-packet files
    // have — `P3-cropangle-model.md` §3.1 found a develop block in the SECOND
    // packet of a real export).
    let mut out = std_packet;
    out.push(b'\n');
    out.extend_from_slice(&ext);
    Ok(Some(out))
}

/// `haystack.contains(needle)` for bytes, without pulling in a dependency.
fn twoway_contains(haystack: &[u8], needle: &[u8]) -> bool {
    needle.len() <= haystack.len()
        && haystack.windows(needle.len()).any(|w| w.eq_ignore_ascii_case(needle))
}

/// Capture metadata for an already-baked raster (P2). Returns the maker, the
/// model and rawler's own parsed [`rawler::exif::Exif`] — the SAME struct the
/// RAW arm reads, so [`exif_facts`] serves both.
///
/// No new dependency, and no hand-rolled tag decoding: rawler's
/// [`GenericTiffReader`] already parses a TIFF header chain (this module uses
/// it for `embedded_xmp` and `lensmeta` uses it for the Sony spline tags), and
/// `Exif::new` already walks the root IFD plus the `ExifIFD` sub-IFD that
/// `wellknown_sub_ifd_tags` parses by default (tag 0x8769, spelled
/// `ExifIFDPointer` there and `ExifOffset` in the EXIF table — verified the
/// same number in `rawler/src/tags.rs:100` and `:369`). A JPEG needs one extra
/// step, [`jpeg_exif_block`], because its TIFF block is wrapped in an APP1
/// segment; a TIFF IS the container already.
///
/// PNG is deliberately NOT read: its EXIF lives in an `eXIf` chunk that needs
/// its own chunk walk, and the format's own metadata story (`tEXt`) is not
/// EXIF at all. A PNG import keeps the neutral stub, exactly as before.
///
/// Absent metadata is `Ok(None)`, not an error — an untagged export is the
/// normal case, not a defect.
fn baked_exif(path: &Path) -> Result<Option<(String, String, rawler::exif::Exif)>> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("open image {}", path.display()))?;
    let mut magic = [0u8; 2];
    if file.read_exact(&mut magic).is_err() {
        return Ok(None);
    }
    use std::io::Seek as _;
    // Both arms re-read from byte 0: the magic probe above consumed two
    // bytes, and both readers below expect to start at the file header (the
    // JPEG walker re-reads SOI itself, the TIFF reader wants `II`/`MM`).
    file.rewind().with_context(|| format!("rewind {}", path.display()))?;
    let tiff = if magic == [0xFF, 0xD8] {
        // JPEG: the TIFF block rides inside APP1.
        let Some(block) = jpeg_exif_block(&mut file)
            .with_context(|| format!("read the EXIF segment of {}", path.display()))?
        else {
            return Ok(None);
        };
        GenericTiffReader::new_with_buffer(&block, 0, 0, Some(16)).ok()
    } else if magic == *b"II" || magic == *b"MM" {
        let mut reader = std::io::BufReader::new(&mut file);
        // Same chain cap as `embedded_xmp` and `lensmeta::read` — a header
        // parse, no pixel decode.
        GenericTiffReader::new(&mut reader, 0, 0, Some(16), &[]).ok()
    } else {
        return Ok(None); // PNG and anything else: no EXIF route (see the doc)
    };
    let Some(tiff) = tiff else { return Ok(None) };
    // `root_ifd()` PANICS on an empty chain (rawler's own `unwrap`-shaped
    // guard, `formats/tiff/reader.rs:36-38`) and `Exif::new` walks
    // third-party-parsed IFDs — both are exactly what `guard_parser_panic`
    // exists for, and metadata is never worth aborting a batch over.
    guard_parser_panic(path, "baked EXIF", || {
        let root = tiff.root_ifd();
        let text = |t: TiffCommonTag| {
            root.get_entry(t)
                .and_then(|e| e.value.as_string().cloned())
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
        };
        let exif = rawler::exif::Exif::new(root)
            .map_err(|e| anyhow!("parse the EXIF of {}: {e}", path.display()))?;
        Ok(Some((
            text(TiffCommonTag::Make).unwrap_or_default(),
            text(TiffCommonTag::Model).unwrap_or_default(),
            exif,
        )))
    })
}

fn decode_baked(path: &Path) -> Result<Decoded> {
    // baked-by-construction: decode_any sends a RAW to decode_raw; this is the baked arm.
    let preview = load_image(path)?;
    let (w, h) = preview.dimensions();
    // P2: a phone or Lightroom JPEG carries ISO, shutter, aperture, focal
    // length and capture time in plain EXIF, and this arm used to throw all of
    // it away and hand the AI advisor `model: "imported image"` and nothing
    // else. A read FAILURE is disclosed and degrades to the old stub — the
    // pixels are fine and refusing the photo over its metadata would be the
    // wrong trade — but it is never folded into silence.
    let facts = match baked_exif(path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("⚠ {e:#}");
            None
        }
    };
    let (make, model, exif) = match facts {
        Some((make, model, exif)) => {
            let f = exif_facts(&exif);
            // "imported image" stays the answer for a file that names no
            // camera: it is what every downstream consumer already reads as
            // "no body", and an empty model string would print as a blank.
            (make, if model.is_empty() { "imported image".to_string() } else { model }, Some(f))
        }
        None => (String::new(), "imported image".to_string(), None),
    };
    let meta = Meta {
        make,
        model,
        lens: exif.as_ref().and_then(|f| f.lens.clone()),
        iso: exif.as_ref().and_then(|f| f.iso),
        shutter: exif.as_ref().and_then(|f| f.shutter.clone()),
        aperture: exif.as_ref().and_then(|f| f.aperture),
        focal_length_mm: exif.as_ref().and_then(|f| f.focal_length_mm),
        exposure_bias_ev: exif.as_ref().and_then(|f| f.exposure_bias_ev),
        date_time: exif.as_ref().and_then(|f| f.date_time.clone()),
        // DISPLAY-frame dims: `load_image` has already applied the EXIF
        // orientation, so these describe the pixels actually delivered.
        width: w as usize,
        height: h as usize,
        // A baked raster is not camera-referred — the develop anchors on the
        // recipe's 5500 K instead (see `render::render_baked_to_image`).
        as_shot_wb_coeffs: [1.0, 1.0, 1.0, 1.0],
    };
    let histogram = compute_histogram(&hist_copy(&preview));
    Ok(Decoded { preview, meta, histogram, embedded_xmp: None })
}

/// The RAW's EXIF orientation (IFD0 tag 0x0112) — the ONE source of truth for
/// which way is up, and the only value any consumer may read.
///
/// **Not** `RawImage.orientation`: rawler 0.7.2 hard-codes that field to
/// `Orientation::Normal` in both `RawImage` constructors
/// (`rawimage.rs:389` and `rawimage.rs:478`, verbatim
/// `orientation: Orientation::Normal, //cam.orientation, // TODO fixme`), so
/// every decoder except DNG and QTK — ARW included — reported "Normal" for a
/// portrait frame and the whole chain rendered it sideways. The real value
/// rides in `RawMetadata.exif`, which rawler DOES populate from tag 0x0112
/// (`exif.rs:16` `pub orientation: Option<u16>`, filled at `exif.rs:124`);
/// the A7RIV portrait samples read `Some(8)` = `Rotate270`.
///
/// An absent tag answers `Normal`. rawler's own `Orientation::from_tiff`
/// answers `Unknown` for that case, but the two are indistinguishable to
/// every consumer: `to_flips()` and [`crate::render::oriented`] treat
/// `Unknown` exactly as `Normal`. That equivalence is asserted, not assumed —
/// see `unknown_and_normal_are_the_same_no_op`.
pub fn raw_orientation_of(md: &rawler::decoders::RawMetadata) -> rawler::Orientation {
    md.exif
        .orientation
        .map(rawler::Orientation::from_u16)
        .unwrap_or(rawler::Orientation::Normal)
}

/// [`raw_orientation_of`] for a path we have not decoded yet: metadata only,
/// no sensor decompression and no preview extraction. Errors only when the
/// file cannot be opened or has no decoder — a RAW without the tag is
/// `Normal`, not a failure.
pub fn raw_orientation(path: &Path) -> Result<rawler::Orientation> {
    guard_tiff_chain(path)?;
    let src = RawSource::new(path).with_context(|| format!("open RAW {}", path.display()))?;
    let decoder = decoder_for(path, &src)?;
    let md = guard_parser_panic(path, "raw_metadata", || {
        decoder
            .raw_metadata(&src, &RawDecodeParams { image_index: 0 })
            .map_err(|e| anyhow!("raw_metadata: {e}"))
    })?;
    Ok(raw_orientation_of(&md))
}

/// The rectangle of the sensor that IS the picture — the DNG
/// `DefaultCropOrigin` / `DefaultCropSize` pair, which is what the camera, the
/// DNG spec and Lightroom all call the frame. Falls back to the active area
/// and then to the whole sensor for a RAW that declares neither.
fn default_crop(raw: &rawler::RawImage) -> rawler::imgop::Rect {
    raw.crop_area.or(raw.active_area).unwrap_or_else(|| {
        rawler::imgop::Rect::new(
            rawler::imgop::Point::new(0, 0),
            rawler::imgop::Dim2::new(raw.width, raw.height),
        )
    })
}

/// Move a decoded `RawImage`'s develop window onto that rectangle. Returns
/// [`CropAlignment`] — which of the three outcomes happened — and DISCLOSES
/// the refusal on stderr (A5).
///
/// **The defect this closes.** Block registration of eight AutoShade renders
/// against their Lightroom exports put every one of them
/// `(+31 ± 6, +20 ± 1)` full-resolution pixels off, a pure translation with no
/// scale component (`PROBE2-VERDICT.md` §9, re-confirmed at `(+34.4, +19.9)`
/// in `PROBE4-FINAL.md` §3). The ARWs carry
/// `DefaultCropOrigin = (32, 20)`, `DefaultCropSize = (9504, 6336)` inside a
/// `9600 × 6376` raw frame: AutoShade emitted the right SIZE from the wrong
/// ORIGIN, so recipe coordinates and Lightroom coordinates disagreed by 0.34 %
/// of the width at the edges — and every mask this batch teaches the importer
/// to place correctly would have landed 32 px right of where Lightroom draws
/// it.
///
/// **Why rawler does not do it.** rawler 0.7.2 (read at
/// `~/.cargo/registry/src/…/rawler-0.7.2/`) measures the default crop against
/// the window its demosaic read, and two shapes of `(active_area, crop_area)`
/// defeat that:
///   * **no active area at all** — the MEASURED shape of the body the defect
///     was found on. `AUTOSHADE_RAW_ZOO` over four ILCE-7RM4A files
///     (2026-09-21) prints `sensor 9600x6376 active none crop
///     9504x6336@32,20` for every one: the files carry no `SonyRawImageSize`
///     tag and the camera database declares no borders
///     (`decoders/arw.rs:191-196`). The demosaic then reads the whole sensor,
///     and `imgop/develop.rs:204-209` adapts the crop to
///     `active_area.unwrap_or(crop)` — to ITSELF — so its origin collapses to
///     `(0, 0)` and the crop's SIZE is cut from the sensor's corner;
///   * **an active area of the crop's own size at another origin** — what
///     `arw.rs:707-713` builds when the tag IS present and reports the trimmed
///     size (`Rect::new(Point::default(), …)`: the tag carries a size, so the
///     origin is pinned at `(0, 0)`). `develop.rs:216` applies the default
///     crop only `if crop.d != intermediate.dim()`, so a default crop that is
///     a pure TRANSLATION has exactly the same size and is skipped. No file in
///     hand has this shape: it is a source read, kept because rawler answers
///     it wrongly by the same arithmetic.
///
/// **What v0.32.0 missed.** It shipped the second arm alone, from the source
/// read, with a test table of synthetic rectangles. On the real files this
/// function therefore answered [`CropAlignment::NothingToMove`] and every
/// render kept starting at the sensor's corner. It was caught on 2026-09-21 by
/// the hot-site map (101 of 101 mapped photosites sat at the SAME coordinates
/// in the mosaic and in the render) and then read off the render itself: an
/// unconstrained ±48 px search of a full-resolution render against Lightroom's
/// export of the same RAW answers `(32, 20)` in three windows of three without
/// the first arm, and `(0, 0)` with it.
///
/// **The fix, and why it is this one.** Moving the demosaic ROI (i.e.
/// `active_area`) to the default-crop origin costs nothing — the developed
/// buffer is the same size, just read from the right place — where cropping
/// afterwards would pay a second full-frame copy (~720 MB at 61 MP). rawler
/// shifts the CFA pattern by the ROI origin itself
/// (`imgop/sensor/bayer/ppg.rs:50`, `cfa.shift(roi.p.x, roi.p.y)`), so the
/// Bayer phase follows; `apply_scaling` has already run over the whole frame
/// in raw coordinates and is untouched by this.
///
/// Deliberately NARROW: it fires only on the two shapes above, the ones where
/// rawler cuts the right SIZE from the wrong ORIGIN. When an active area is
/// declared and the sizes differ, rawler's own `CropDefault` step does the
/// right thing and this leaves it alone. A rectangle that would run off the
/// sensor is refused rather than clamped — a RAW whose tags disagree with its
/// own dimensions is not a frame to guess at.
pub fn align_default_crop(raw: &mut rawler::RawImage) -> CropAlignment {
    let verdict = aligned_demosaic_roi(raw.crop_area, raw.active_area, raw.width, raw.height);
    match verdict {
        CropAlignment::Moved(moved) => raw.active_area = Some(moved),
        // A5: the refusal used to be an indistinguishable `None`, discarded at
        // the call site — so a non-Sony file with inconsistent tags rendered
        // from the un-aligned window and said NOTHING, leaving any later
        // diagnosis with zero telemetry. It is now a sentence on the same
        // stderr channel every other render disclosure uses
        // (`ValidatedRecipe::disclose`, the embedded-XMP read warning).
        CropAlignment::OffSensor { crop, width, height } => eprintln!(
            "⚠ this RAW's DefaultCrop rectangle ({}×{} at {},{}) runs off its own {width}×{height} \
             sensor — developing from the un-aligned window instead. The frame is still the size \
             the camera declares; its ORIGIN may differ from Lightroom's by the tag disagreement",
            crop.d.w, crop.d.h, crop.p.x, crop.p.y
        ),
        CropAlignment::NothingToMove => {}
    }
    verdict
}

/// What [`align_default_crop`] decided. Three outcomes, not two: "moved" and
/// "nothing to move" are both normal, and "the tags run off the sensor" is a
/// fact about the FILE that a photographer chasing a misplaced mask needs to
/// hear (A5). Carried as a value as well as printed so the `AUTOSHADE_RAW_ZOO`
/// probe can record a verdict per make without scraping stderr.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropAlignment {
    /// The demosaic window moved onto the default-crop origin.
    Moved(rawler::imgop::Rect),
    /// Sizes differ (rawler's own `CropDefault` handles it), origins already
    /// agree, or the RAW declares no default crop. Every non-Sony make in the
    /// zoo lands here — see the table in
    /// `the_demosaic_window_moves_onto_the_default_crop_rectangle`.
    NothingToMove,
    /// The rectangle would index past the sensor buffer. Refused, never
    /// clamped — a RAW whose tags disagree with its own dimensions is not a
    /// frame to guess at.
    OffSensor { crop: rawler::imgop::Rect, width: usize, height: usize },
}

/// The decision half of [`align_default_crop`], as a function of the numbers
/// alone — a `RawImage` needs a whole `Camera` and a sensor buffer to build,
/// and the thing worth pinning is which rectangles move and which do not.
fn aligned_demosaic_roi(
    crop: Option<rawler::imgop::Rect>,
    active: Option<rawler::imgop::Rect>,
    width: usize,
    height: usize,
) -> CropAlignment {
    let Some(crop) = crop else {
        return CropAlignment::NothingToMove;
    };
    // Where rawler WILL cut the crop's size from, against where the crop says
    // it starts. With an active area of another size its own `CropDefault`
    // measures the difference and is right; the two shapes below are the ones
    // it answers from the wrong origin (see `align_default_crop`).
    let misplaced = match active {
        Some(active) => crop.d == active.d && crop.p != active.p,
        None => (crop.p.x, crop.p.y) != (0, 0),
    };
    if !misplaced {
        return CropAlignment::NothingToMove;
    }
    if crop.p.x + crop.d.w > width || crop.p.y + crop.d.h > height {
        return CropAlignment::OffSensor { crop, width, height };
    }
    CropAlignment::Moved(crop)
}

/// The pixel frame a develop of `path` lands in — the frame every normalised
/// coordinate in a recipe, and in a Lightroom sidecar, is measured against.
///
/// For a RAW that is [`default_crop`]'s rectangle, turned into the DISPLAY
/// frame by the EXIF orientation, which is `render::render_to_image`'s own
/// order (orientation first, everything else after). For a baked image it is
/// the oriented header dimensions — `load_image` applies the orientation the
/// same way.
///
/// Metadata only: `dummy = true` on the RAW arm means no sensor
/// decompression, and the baked arm decodes no pixel at all.
pub fn frame_size(path: &Path) -> Result<(usize, usize)> {
    frame_size_turned(path, 0)
}

/// [`frame_size`] in the frame the photographer's own quarter turns produce.
pub fn frame_size_turned(path: &Path, quarter_turns: u8) -> Result<(usize, usize)> {
    let ((w, h), exif) = source_frame(path)?;
    let orientation = crate::render::compose_orientation(exif, quarter_turns);
    Ok(if orientation_transposes(orientation) { (h, w) } else { (w, h) })
}

/// The SHORT edge of [`frame_size`]: the full-resolution develop's, which is
/// what `render::FilmScale` measures a downscaled working raster against (a
/// turn does not change it). `None` when the header cannot be read — the
/// develop then treats its own raster as the film.
pub fn film_short_edge(path: &Path) -> Option<u32> {
    frame_size(path).ok().and_then(|(w, h)| u32::try_from(w.min(h)).ok())
}

/// The window a develop of `path` lands in: [`source_frame`]'s two halves, and
/// how far that window sits from the one every release up to v1.5.1 cut.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SourceWindow {
    /// The frame the file STORES, un-turned — [`default_crop`]'s size for a
    /// RAW, the header's own dimensions for a baked image.
    pub size: (usize, usize),
    /// The turn the file's own metadata asks for.
    pub exif: rawler::Orientation,
    /// Sensor pixels between this build's develop window and the window every
    /// release up to v1.5.1 developed the same file from — `(0, 0)` for every
    /// file whose window never moved, and for a baked image. It is the
    /// DefaultCrop origin on the shape [`align_default_crop`] moves off the
    /// sensor's corner (the measured ILCE-7RM4A files: `(32, 20)`); see
    /// [`legacy_window_shift`] for why the v0.32.0 shape reads zero.
    pub legacy_shift: (i32, i32),
}

/// How far, in sensor pixels, [`align_default_crop`]'s window sits from where
/// releases up to v1.5.1 cut the same file — the vector
/// `pipeline::migrate_recipe_coord_frame` moves an era-≤1 recipe's geometry
/// by (`recipe::COORD_ERA` 2).
///
/// Only the shape rawler cuts from the sensor's CORNER counts: no active area,
/// a declared crop origin off `(0, 0)`. Every release before 2026-09-21
/// developed that shape from the corner, so a recipe saved by any of them is
/// drawn against the corner window and the shift is the declared origin. The
/// other shape `aligned_demosaic_roi` moves — an active area of the crop's
/// size at another origin — has been developed from the declared origin since
/// v0.32.0, so a recipe saved since then is already in this frame and reads
/// zero. (A recipe saved by v0.30–v0.31 on THAT shape would be
/// `crop.p − active.p` off and its stamp cannot tell; no file of that shape
/// exists in hand — it is a source read, row 1b of the decode table.) A
/// refused window (off-sensor tags) develops from the un-aligned window
/// before and after, so it reads zero too.
pub(crate) fn legacy_window_shift(
    crop: Option<rawler::imgop::Rect>,
    active: Option<rawler::imgop::Rect>,
    width: usize,
    height: usize,
) -> (i32, i32) {
    match aligned_demosaic_roi(crop, active, width, height) {
        CropAlignment::Moved(r) if active.is_none() => (r.p.x as i32, r.p.y as i32),
        _ => (0, 0),
    }
}

/// The frame the FILE STORES, un-turned, together with the turn its own
/// metadata asks for — the two halves [`frame_size_turned`] folds together,
/// and the pair `xmp::FrameAspect` needs whole (R27 A7/A8).
///
/// For a RAW that is [`default_crop`]'s rectangle in SENSOR orientation (the
/// DNG `DefaultCropSize`, long axis left→right on an A7R IV whichever way the
/// camera was held); for a baked image it is the header's own dimensions
/// before its EXIF orientation is applied. **This — not the display frame — is
/// what every normalised `crs:` coordinate is measured against**
/// (`P1-portrait-mask-frame.md` §1 for the mask block,
/// `P3-cropangle-model.md` §2 for the crop rectangle, both HIGH), and it is
/// what a Lightroom sidecar's `tiff:ImageWidth/ImageLength` declare (F3's
/// public census, 72/72).
///
/// ONE metadata read for both answers: the caller that needs the aspect needs
/// the orientation too, and asking twice paid for the same header walk twice.
///
/// Metadata only: `dummy = true` on the RAW arm means no sensor
/// decompression, and the baked arm decodes no pixel at all.
///
/// Since v1.6.0 the answer also carries [`SourceWindow::legacy_shift`], read
/// off the same dummy decode: [`legacy_window_shift`] needs only the
/// rectangles `align_default_crop` judges on the real one.
pub fn source_window(path: &Path) -> Result<SourceWindow> {
    if !is_raw(path) {
        use image::ImageDecoder as _;
        use image::metadata::Orientation as ImgO;
        use rawler::Orientation as O;
        // baked-by-construction: the `is_raw` gate above is this function's
        // own dispatch, the one `load_image_gated` enforces for pixels. The
        // reader is `baked_reader`'s, limits and all: a plain `ImageReader`
        // under the crate's default 512 MB `max_alloc` refuses to build the
        // TIFF decoder for a 60 MP-plus 16-bit export at `into_decoder`, so
        // this header-only read answered "unreadable" for a file the pixel
        // path develops.
        let reader = baked_reader(path)?;
        let mut decoder = reader
            .into_decoder()
            .with_context(|| format!("read the header of {}", path.display()))?;
        // `image`'s eight states ARE the EXIF eight, named after the motion
        // that CORRECTS the file rather than after the tag number; the pairing
        // below is that table (`Rotate90` = tag 6, `Rotate270` = tag 8), which
        // is exactly how `rawler::Orientation::from_u16` spells them too.
        let exif = match decoder.orientation().unwrap_or(ImgO::NoTransforms) {
            ImgO::NoTransforms => O::Normal,
            ImgO::FlipHorizontal => O::HorizontalFlip,
            ImgO::Rotate180 => O::Rotate180,
            ImgO::FlipVertical => O::VerticalFlip,
            ImgO::Rotate90FlipH => O::Transpose,
            ImgO::Rotate90 => O::Rotate90,
            ImgO::Rotate270FlipH => O::Transverse,
            ImgO::Rotate270 => O::Rotate270,
        };
        let (w, h) = decoder.dimensions();
        return Ok(SourceWindow { size: (w as usize, h as usize), exif, legacy_shift: (0, 0) });
    }
    guard_tiff_chain(path)?;
    guard_raw_plane_extent(path)?;
    let src = RawSource::new(path).with_context(|| format!("open RAW {}", path.display()))?;
    let decoder = decoder_for(path, &src)?;
    let params = RawDecodeParams { image_index: 0 };
    let (md, raw) = guard_parser_panic(path, "frame size", || {
        let md = decoder
            .raw_metadata(&src, &params)
            .map_err(|e| anyhow!("raw_metadata: {e}"))?;
        let raw = decoder
            .raw_image(&src, &params, true)
            .map_err(|e| anyhow!("raw_image(dummy): {e}"))?;
        Ok((md, raw))
    })?;
    let d = default_crop(&raw).d;
    let legacy_shift = legacy_window_shift(raw.crop_area, raw.active_area, raw.width, raw.height);
    Ok(SourceWindow { size: (d.w, d.h), exif: raw_orientation_of(&md), legacy_shift })
}

/// [`source_window`]'s frame and turn alone, for the callers that need no
/// more — one header walk either way.
pub fn source_frame(path: &Path) -> Result<((usize, usize), rawler::Orientation)> {
    source_window(path).map(|w| (w.size, w.exif))
}

/// Does this orientation swap width and height in the display frame?
/// `pub(crate)`: the render side's quarter-turn contract test asserts that
/// THIS predicate and `render::oriented`'s actual output agree for all eight
/// states — two modules, one answer, checked rather than assumed.
pub(crate) fn orientation_transposes(o: rawler::Orientation) -> bool {
    matches!(
        o,
        rawler::Orientation::Rotate90
            | rawler::Orientation::Rotate270
            | rawler::Orientation::Transpose
            | rawler::Orientation::Transverse
    )
}

/// Histogram working copy: DOWNSCALE-only to ≤1024 (`resize` also UPSCALES a
/// smaller input, and interpolating a small thumbnail up distorted clipping
/// percentages before histogramming).
fn hist_copy(img: &DynamicImage) -> DynamicImage {
    if img.width().max(img.height()) > 1024 {
        img.resize(1024, 1024, image::imageops::FilterType::Triangle)
    } else {
        img.clone()
    }
}

/// Process-wide cap on CONCURRENT full-res embedded-preview decodes. A 61 MP
/// ARW preview lands as a 9504×6336 Rgb8 frame (~181 MB), and a portrait
/// rotation holds a second copy while it turns. The old style-index
/// build's worker pool bounded ONE build; two builds in the same process (the
/// web server's request threads) used to
/// stack 8 decodes ≈ 1.4 GB. This bounds them together. Taken at the BUILD
/// call site only: gating inside `decode_raw` would silently throttle the
/// GUI thumbnail path, whose own cap deliberately keeps RAW thumbs
/// concurrent.
pub const MAX_CONCURRENT_DECODES: usize = 4;
const _: () = assert!(MAX_CONCURRENT_DECODES > 0);

fn decode_gate() -> &'static (std::sync::Mutex<usize>, std::sync::Condvar) {
    static GATE: std::sync::OnceLock<(std::sync::Mutex<usize>, std::sync::Condvar)> =
        std::sync::OnceLock::new();
    GATE.get_or_init(Default::default)
}

/// One decode slot, released on DROP — mandatory, not tidiness: rawler runs
/// third-party parsers over untrusted files, and a permit leaked by an
/// unwind inside `thread::scope` would park every other worker forever
/// instead of letting the scope join and re-panic. Poison is recovered,
/// never re-panicked — a panic inside Drop during an unwind aborts the
/// process.
pub struct DecodePermit(());

impl DecodePermit {
    pub fn acquire() -> Self {
        let (lock, cv) = decode_gate();
        let mut n = lock.lock().unwrap_or_else(|p| p.into_inner());
        while *n >= MAX_CONCURRENT_DECODES {
            n = cv.wait(n).unwrap_or_else(|p| p.into_inner());
        }
        *n += 1;
        DecodePermit(())
    }
}

impl Drop for DecodePermit {
    fn drop(&mut self) {
        let (lock, cv) = decode_gate();
        let mut n = lock.lock().unwrap_or_else(|p| p.into_inner());
        *n = n.saturating_sub(1);
        cv.notify_one();
    }
}

/// One TIFF scalar read, shared by the two header guards below so the second
/// one is not a second copy of the first's byte-order handling.
fn tiff_u16(r: &mut impl std::io::Read, le: bool) -> std::io::Result<u16> {
    let mut b = [0u8; 2];
    r.read_exact(&mut b)?;
    Ok(if le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) })
}

fn tiff_u32(r: &mut impl std::io::Read, le: bool) -> std::io::Result<u32> {
    let mut b = [0u8; 4];
    r.read_exact(&mut b)?;
    Ok(if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) })
}

/// Refuse a TIFF-container RAW whose top-level IFD chain CYCLES, before
/// rawler ever walks it.
///
/// Upstream defect (rawler 0.7.2, `formats/tiff/reader.rs:164-179`): the
/// chain walker keeps no visited-offset set, and its only `break` is inside
/// `if let Some(max) = max_chained` — which never fires for `None`. Every
/// path into `get_decoder` passes `None` (`decoders/mod.rs:909`, hard-coded
/// and unreachable from here), so an IFD whose `next_ifd` points at itself
/// spins forever pushing a fresh IFD per iteration: unbounded time AND
/// memory. Nothing downstream can contain it — there is no panic for
/// `spawn_worker`'s `catch_unwind` to catch, allocation failure aborts
/// rather than unwinds, and Rust cannot kill a spinning thread.
///
/// CONTAINMENT, NOT A TERMINATION PROOF. This walk mirrors the straight
/// `count × 12` entry stride; rawler's `IFD::new` can abandon its entry loop
/// early on a malformed entry and then read `next_ifd` from wherever its
/// reader stopped, so a file crafted against that divergence could follow a
/// different chain and still hang. It stops the natural crafted cycle. The
/// complete cure is upstream (a visited-offset set, plus a sane default cap
/// for `None`) or decoding in a subprocess.
///
/// NOT COVERED, same upstream file class: `Entry::parse`
/// (`formats/tiff/entry.rs:106-112` and its siblings) sizes
/// `vec![0; count as usize]` from a file-supplied u32 BEFORE reading, so a
/// SINGLE entry can demand up to 4 GiB (34 GiB for DOUBLE). No app-side
/// interception exists short of vendoring rawler; a chain of length one
/// reaches it, and this guard does nothing about it.
///
/// Called ahead of every rawler entry in the crate — `raw_orientation`,
/// `source_window` (the frame / window memo), `decode_raw_turned`, the
/// CR3/RAF embedded-XMP read (`embedded_xmp`) and `camera_rendition`: the
/// five callers that reach `decoder_for`, the one door a decoder is made
/// at. A new rawler entry point needs this line too.
///
/// Rejects ONLY on proof; any IO error or non-TIFF magic passes through, so
/// no file that decodes today changes behaviour (truncation is end-of-chain
/// to rawler, not an error, and CR3/RAF/X3F never reach the TIFF probe).
pub(crate) fn guard_tiff_chain(path: &Path) -> Result<()> {
    use std::io::{Read as _, Seek as _};
    /// Far above any real file (a RAW carries ≤4) and above rawler's own
    /// internal caps of 10/16 — a chain this long is already pathological.
    const MAX_IFDS: usize = 64;
    let Ok(file) = std::fs::File::open(path) else { return Ok(()) };
    let mut r = std::io::BufReader::new(file);
    let mut bom = [0u8; 2];
    if r.read_exact(&mut bom).is_err() {
        return Ok(());
    }
    let le = match &bom {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(()), // not a TIFF container — rawler probes it elsewhere
    };
    // The magic word is READ but not enforced: rawler's own check is
    // commented out (reader.rs:151-154), which is how Panasonic RW2's 0x55
    // reaches the same walker. Enforcing 42 here would leave RW2 unguarded.
    if tiff_u16(&mut r, le).is_err() {
        return Ok(());
    }
    // base = corr = 0 at every call site we guard (decoders/mod.rs:909), so
    // there is no offset correction to model.
    let Ok(mut next) = tiff_u32(&mut r, le) else { return Ok(()) };
    let mut seen = std::collections::HashSet::new();
    let mut walked = 0usize;
    while next != 0 {
        if !seen.insert(next) {
            anyhow::bail!(
                "{} has a cyclic TIFF IFD chain (offset {next} repeats) — refusing to decode",
                path.display()
            );
        }
        walked += 1;
        if walked > MAX_IFDS {
            anyhow::bail!(
                "{} has a TIFF IFD chain longer than {MAX_IFDS} entries — refusing to decode",
                path.display()
            );
        }
        if r.seek(std::io::SeekFrom::Start(u64::from(next))).is_err() {
            return Ok(());
        }
        let Ok(entries) = tiff_u16(&mut r, le) else { return Ok(()) };
        if r.seek(std::io::SeekFrom::Current(i64::from(entries) * 12)).is_err() {
            return Ok(());
        }
        let Ok(n) = tiff_u32(&mut r, le) else { return Ok(()) };
        next = n;
    }
    Ok(())
}

/// Refuse a TIFF-container RAW whose SENSOR PLANE is over the third-party
/// decoder's own allocation ceiling, before that decoder is asked to allocate
/// it.
///
/// **The root cause this closes.** AutoShade's per-file RAW ceiling
/// ([`refuse_raw_develop_over_ceiling`]) is charged against the pixel count in
/// a DUMMY `RawImage` — a number only rawler can produce. rawler's allocator
/// has a LOWER ceiling of its own and it PANICS at it: `alloc_image_plain!`
/// (`pixarray.rs:546-556`) refuses `w * h > 500_000_000 || w > 50_000 ||
/// h > 50_000`, and it checks that BEFORE the `dummy` short-circuit, so even
/// the metadata-only probe trips it. Worse, `plain_image_from_ifd`
/// (`decoders/mod.rs:598`) charges the ceiling with `decode_width * cpp` as the
/// WIDTH, so a three-sample linear frame reaches it at a third of the pixel
/// count a Bayer frame would. Between the two ceilings AutoShade's own,
/// carefully worded refusal was therefore UNREACHABLE: the probe that would
/// have measured the file panicked first, and [`guard_parser_panic`] reported
/// "a defect in the third-party decoder for this format" — which is not what
/// happened. The frame is simply larger than that decoder builds for.
///
/// The natural instance is an upscaler's output, not a corrupt file: a 2x pass
/// over a 60 MP frame writes a 19008x12672 LinearRaw DNG at 3 samples per
/// pixel in 416x416 tiles, which rounds up to 19136 tile-columns and asks for
/// `19136 * 3 = 57408` by `12896` — over BOTH limits at once.
///
/// Mirrors rawler's arithmetic instead of approximating it: the same tile
/// round-up (`decoders/mod.rs:588-594`), the same `* cpp` on the width, the
/// same three comparisons. An approximation would either admit files that
/// still panic or refuse files that decode today.
///
/// **Rejects ONLY on proof**, exactly like [`guard_tiff_chain`] and for the
/// same reason: any IO error, any non-TIFF magic, any IFD that does not
/// DECLARE itself sensor data passes through untouched. The declaration is
/// `PhotometricInterpretation` CFA or LinearRaw — a preview or an embedded
/// JPEG is BlackIsZero / RGB / YCbCr — so a file that decodes today cannot
/// start failing here.
///
/// Called from the four PIXEL doors and no others: [`source_frame`],
/// [`decode_raw_turned`], `render::render_to_image_in` and
/// `render::as_shot_wb` — every place that asks for a `RawImage`. Deliberately
/// NOT from [`raw_orientation`], [`embedded_xmp`] or `camera_rendition`: those
/// read metadata and the embedded preview, which an over-ceiling file answers
/// perfectly well, and refusing there would blank the gallery thumbnail of a
/// photo whose only problem is that it cannot be developed.
pub(crate) fn guard_raw_plane_extent(path: &Path) -> Result<()> {
    use std::io::{Read as _, Seek as _};
    /// `alloc_image_plain!` (rawler `pixarray.rs:546-556`), verbatim.
    const MAX_SIDE: u64 = 50_000;
    const MAX_ELEMS: u64 = 500_000_000;
    /// The two `PhotometricInterpretation` values that MEAN "this IFD is
    /// sensor data": CFA (a Bayer / X-Trans mosaic, one sample per pixel) and
    /// LinearRaw (already demosaiced — what a DNG converter or an upscaler
    /// writes, three samples per pixel).
    const PHOTOMETRIC_CFA: u64 = 32803;
    const PHOTOMETRIC_LINEAR_RAW: u64 = 34892;
    /// Bounded for the same reason [`guard_tiff_chain`] bounds its walk: this
    /// guard must never become the thing that hangs on a crafted file.
    const MAX_IFDS: usize = 64;
    const MAX_SUB_IFDS: u32 = 16;

    const TAG_IMAGE_WIDTH: u16 = 0x0100;
    const TAG_IMAGE_LENGTH: u16 = 0x0101;
    const TAG_PHOTOMETRIC: u16 = 0x0106;
    const TAG_SAMPLES_PER_PIXEL: u16 = 0x0115;
    const TAG_TILE_WIDTH: u16 = 0x0142;
    const TAG_TILE_LENGTH: u16 = 0x0143;
    const TAG_SUB_IFDS: u16 = 0x014A;

    /// A SHORT or LONG whose `count` is 1 lives INSIDE the entry's own 4-byte
    /// value field, left-justified in both byte orders (TIFF 6.0 section 2).
    /// Every tag this walk reads is that shape; anything else is not our tag.
    fn scalar(typ: u16, count: u32, v: [u8; 4], le: bool) -> Option<u64> {
        if count != 1 {
            return None;
        }
        match typ {
            3 => Some(u64::from(if le {
                u16::from_le_bytes([v[0], v[1]])
            } else {
                u16::from_be_bytes([v[0], v[1]])
            })),
            4 => Some(u64::from(if le { u32::from_le_bytes(v) } else { u32::from_be_bytes(v) })),
            _ => None,
        }
    }

    let Ok(file) = std::fs::File::open(path) else { return Ok(()) };
    let mut r = std::io::BufReader::new(file);
    let mut bom = [0u8; 2];
    if r.read_exact(&mut bom).is_err() {
        return Ok(());
    }
    let le = match &bom {
        b"II" => true,
        b"MM" => false,
        _ => return Ok(()), // not a TIFF container — nothing here to measure
    };
    // Read but not enforced, for the reason guard_tiff_chain records: rawler
    // does not enforce it either, which is how RW2 0x55 reaches the walker.
    if tiff_u16(&mut r, le).is_err() {
        return Ok(());
    }
    let Ok(first) = tiff_u32(&mut r, le) else { return Ok(()) };

    // Breadth-first over the root chain AND the SubIFDs hung off it — the
    // shape every TIFF-based RAW uses to carry its sensor plane, and the same
    // one [`raw_in_tiff_clothing`] keys on. [`guard_tiff_chain`] runs before
    // this at every call site, so the chain is already known acyclic; the
    // visited set is here so a standalone call cannot spin either.
    let mut queue = std::collections::VecDeque::from([first]);
    let mut seen = std::collections::HashSet::new();
    let mut walked = 0usize;
    while let Some(off) = queue.pop_front() {
        if off == 0 || !seen.insert(off) {
            continue;
        }
        walked += 1;
        if walked > MAX_IFDS {
            return Ok(());
        }
        if r.seek(std::io::SeekFrom::Start(u64::from(off))).is_err() {
            return Ok(());
        }
        let Ok(entries) = tiff_u16(&mut r, le) else { return Ok(()) };
        let mut buf = vec![0u8; usize::from(entries) * 12];
        if r.read_exact(&mut buf).is_err() {
            return Ok(());
        }
        // next_ifd follows the entries — read it BEFORE any seek below moves
        // the cursor away to chase a SubIFD offset list.
        if let Ok(next) = tiff_u32(&mut r, le) {
            queue.push_back(next);
        }

        let (mut width, mut height, mut spp, mut photometric) = (None, None, None, None);
        let (mut tile_w, mut tile_h) = (None, None);
        for e in buf.chunks_exact(12) {
            let rd16 = |a: usize| {
                let b = [e[a], e[a + 1]];
                if le { u16::from_le_bytes(b) } else { u16::from_be_bytes(b) }
            };
            let tag = rd16(0);
            let typ = rd16(2);
            let count = {
                let b = [e[4], e[5], e[6], e[7]];
                if le { u32::from_le_bytes(b) } else { u32::from_be_bytes(b) }
            };
            let inline = [e[8], e[9], e[10], e[11]];
            // IFD (13) is the type a DNG writer is free to use for SubIFDs
            // instead of LONG (4); both carry the same offsets.
            if tag == TAG_SUB_IFDS && matches!(typ, 4 | 13) {
                if count == 1 {
                    if let Some(o) = scalar(4, 1, inline, le) {
                        queue.push_back(o as u32);
                    }
                } else if count <= MAX_SUB_IFDS
                    && let Some(list) = scalar(4, 1, inline, le)
                    && r.seek(std::io::SeekFrom::Start(list)).is_ok()
                {
                    for _ in 0..count {
                        let Ok(o) = tiff_u32(&mut r, le) else { break };
                        queue.push_back(o);
                    }
                }
                continue;
            }
            let Some(v) = scalar(typ, count, inline, le) else { continue };
            match tag {
                TAG_IMAGE_WIDTH => width = Some(v),
                TAG_IMAGE_LENGTH => height = Some(v),
                TAG_PHOTOMETRIC => photometric = Some(v),
                TAG_SAMPLES_PER_PIXEL => spp = Some(v),
                TAG_TILE_WIDTH => tile_w = Some(v),
                TAG_TILE_LENGTH => tile_h = Some(v),
                _ => {}
            }
        }

        let (Some(w), Some(h)) = (width, height) else { continue };
        if w == 0 || h == 0 {
            continue;
        }
        if !matches!(photometric, Some(PHOTOMETRIC_CFA) | Some(PHOTOMETRIC_LINEAR_RAW)) {
            continue; // a preview or an embedded JPEG, not the sensor plane
        }
        let cpp = spp.unwrap_or(1).max(1);
        // A TILED plane is rounded UP to whole tiles before it is allocated
        // (`decoders/mod.rs:588-594`); a stripped one allocates its own size.
        let (dw, dh) = match (tile_w, tile_h) {
            (Some(tw), Some(tl)) if tw > 0 && tl > 0 => {
                (w.div_ceil(tw).saturating_mul(tw), h.div_ceil(tl).saturating_mul(tl))
            }
            _ => (w, h),
        };
        let alloc_w = dw.saturating_mul(cpp);
        if alloc_w > MAX_SIDE || dh > MAX_SIDE || alloc_w.saturating_mul(dh) > MAX_ELEMS {
            // Everything a person can act on: the frame as the file declares
            // it, the allocation it becomes (so the verdict is auditable
            // rather than an oracle), that the FILE is fine, and the workflow
            // that does work — upscaling AFTER the develop, where it belongs.
            anyhow::bail!(
                "{} carries a {w}x{h} sensor plane at {cpp} sample(s) per pixel, which this \
                 build's RAW decoder allocates as {alloc_w} x {dh} elements — past its own \
                 {MAX_SIDE}-per-side and {MAX_ELEMS}-element ceiling, so asking it to decode \
                 this frame aborts instead of returning. Nothing is wrong with the file; it is \
                 simply a larger frame than this build develops. Develop the ORIGINAL frame in \
                 AutoShade and run the upscaler on the result",
                path.display()
            );
        }
    }
    Ok(())
}

/// The one sentence that turns "your camera is not supported" into something a
/// photographer can DO (A8, the DNG on-ramp). Every body AutoShade cannot read
/// natively has this route, and it is not a downgrade: rawler's DNG decoder
/// builds its whole `Camera` — colour matrices included — from the file's own
/// tags (`decoders/dng.rs:270-289`), so a converted file needs no entry in the
/// 725-model camera database at all. Kept as ONE constant so the CLI, the GUI
/// toast and the web error body cannot drift into three different offers.
pub const DNG_ONRAMP: &str = "Adobe DNG Converter (free, from Adobe) rewrites it as a .dng, \
                              which AutoShade develops from the file's own colour tags — that \
                              route works for any body";

/// Turn rawler's `get_decoder` refusal into a sentence that names WHICH of the
/// three quite different failures happened (A7). They used to arrive as one
/// opaque `no rawler decoder for {path}: {e}` line, and the three want three
/// different actions from the user:
///
///   * **unknown make** — the file IS a TIFF-shaped RAW, but its `Make` string
///     matches none of rawler's table (`decoders/mod.rs:963-971`). Usually a
///     body newer than the decoder crate.
///   * **unknown model** — the make is known and the container parsed, but the
///     exact model is missing from the 725-file camera database
///     (`check_supported_with_everything`, `mod.rs:1003-1009`). This is the
///     common case for a recent body, and the one the DNG on-ramp fixes best.
///   * **no decoder at all** — nothing matched: not a RAW, or a container this
///     rawler does not know. A `.tif`-named RAW lands here too (see
///     [`load_image_gated`]'s DNG-in-TIFF refusal for the reverse direction).
///
/// The path is always named because a batch scan reports many files at once.
fn describe_decoder_failure(path: &Path, e: &rawler::RawlerError) -> anyhow::Error {
    let rawler::RawlerError::Unsupported { what, make, model, .. } = e else {
        // DecoderFailed — the container was recognised and then would not
        // read. That is a file-integrity story, not a camera-support one, so
        // it keeps rawler's own words and gains no on-ramp advice.
        return anyhow!("{} could not be decoded: {e}", path.display());
    };
    let (make, model) = (make.trim(), model.trim());
    if !make.is_empty() && !model.is_empty() {
        anyhow!(
            "{} is a {make} {model}, and this build's RAW decoder has no calibration for that \
             exact model (it carries 725). {DNG_ONRAMP}",
            path.display()
        )
    } else if !make.is_empty() {
        anyhow!(
            "{} names its maker as \"{make}\", which this build's RAW decoder does not know at \
             all — usually a body newer than the decoder. {DNG_ONRAMP}",
            path.display()
        )
    } else {
        anyhow!(
            "{} does not read as any RAW format this build knows ({what}) — if it really is a \
             camera RAW, {DNG_ONRAMP}",
            path.display()
        )
    }
}

/// [`get_decoder`] with [`describe_decoder_failure`]'s wording — the ONE place
/// a decoder refusal is turned into a sentence, so `decode_raw`, the preview
/// path, `frame_size`, `raw_orientation`, the CR3/RAF XMP-packet read and the
/// full-res render cannot answer the same file three different ways (they did:
/// "no rawler decoder for …", "no decoder for …", and rawler's raw Display).
#[allow(clippy::disallowed_methods)] // the one decoder construction door
pub(crate) fn decoder_for<'a>(
    path: &Path,
    src: &'a RawSource,
) -> Result<Box<dyn rawler::decoders::Decoder + 'a>> {
    get_decoder(src).map_err(|e| describe_decoder_failure(path, &e))
}

/// Run a THIRD-PARTY parser call so that a panic inside it becomes a named
/// error instead of an aborted process.
///
/// The GUI has had this since v0.22 — every worker body runs inside
/// `catch_unwind` (`src/bin/gui/workers.rs:29-38`) — and the CLI had nothing:
/// the same malformed file that showed a toast in the app killed
/// `autoshade batch` mid-library, losing the run. Widening [`RAW_EXTS`] to 24
/// formats widens the surface exactly where the parsers are least exercised,
/// so the guard goes in at the same time as the extensions.
///
/// `AssertUnwindSafe` is correct and not a shortcut here: the closure borrows
/// a `RawSource` and a decoder that are DROPPED on the error path — nothing
/// observes a half-updated value after a caught panic, because nothing
/// survives the call. What crosses the boundary is the returned `T` alone.
///
/// This does NOT make a panicking decoder supported (see [`RAW_EXTS`] on
/// `.x3f`); it makes one file's defect one file's error.
pub(crate) fn guard_parser_panic<T>(
    path: &Path,
    what: &str,
    call: impl FnOnce() -> Result<T>,
) -> Result<T> {
    // The panic below is CONTAINED. Say so on this thread, so the GUI's
    // panic hook reports it as the one file's failure it is instead of
    // raising a modal claiming the app must close ([`crate::panic_guard`]).
    let _contained = crate::panic_guard::Recoverable::enter();
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(call)) {
        Ok(r) => r,
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| (*s).to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".into());
            Err(anyhow!(
                "the RAW parser panicked reading {} ({what}: {msg}) — that is a defect in the \
                 third-party decoder for this format, not something wrong with the photograph. \
                 {DNG_ONRAMP}",
                path.display()
            ))
        }
    }
}

/// The three-level embedded-rendition fallback: a level that ERRORS is a
/// level that cannot answer — the chain's whole purpose is "this one can't,
/// try the next" (L05-4/5: a corrupt mid-size preview used to abort files
/// whose thumbnail or full-size JPEG was intact). Lazy, so a cheaper hit
/// skips the larger decodes; the terminal failure names every reason.
type RenditionLevel<'a, T> = (&'static str, &'a mut dyn FnMut() -> Result<Option<T>>);

/// [`first_usable_rendition`], with the two "no" answers kept APART (A4).
///
/// `Ok(None)` means every level said "I do not carry one" — a fact about the
/// FORMAT, not about this file. rawler 0.7.2 overrides none of the three
/// rendition methods for ORF, SRW, NRW, MEF, MOS, KDC, DCR, DCS, ERF, IIQ,
/// CRW or ARI (the trait defaults return `Ok(None)`, `decoders/mod.rs:340`,
/// `:346`, `:351`), so an Olympus file has no embedded rendition to find no
/// matter how healthy it is. Folding that into the same error as "the preview
/// is corrupt" is what made `.orf` fail outright on the CLI while the very
/// same file developed fine in the GUI.
///
/// `Err` still means the levels that COULD have answered tried and failed —
/// a corrupt preview, an unreadable JPEG blob — and every reason is named.
fn usable_rendition<T>(path: &Path, levels: &mut [RenditionLevel<'_, T>]) -> Result<Option<T>> {
    let mut why: Vec<String> = Vec::new();
    for (name, get) in levels.iter_mut() {
        match get() {
            Ok(Some(v)) => return Ok(Some(v)),
            Ok(None) => {}
            Err(e) => why.push(format!("{name}: {e:#}")),
        }
    }
    if why.is_empty() {
        return Ok(None);
    }
    anyhow::bail!("no usable embedded rendition in {} ({})", path.display(), why.join("; "))
}

/// Long edge for the stand-in develop [`neutral_rendition`] produces. 2048 px
/// is the working edge the rest of the tree already treats as "preview-sized"
/// (`pipeline`'s base-look estimation, `denoise`'s and `generative`'s
/// pre-shrink), and it is chosen for the same reason here: the histogram this
/// feeds is a 256-bin summary and the AI advisor is shown a downscaled image
/// regardless, so paying a full-resolution develop would buy nothing.
const NEUTRAL_PREVIEW_EDGE: u32 = 2048;

/// The stand-in for a camera rendition on a format that embeds none (A4): our
/// OWN neutral develop at working resolution, already in the display frame
/// (`render_to_image` orients before anything else).
///
/// It says so, on stderr, every time — the GUI/web route their own copy of
/// this line through their toast/warning channels. That disclosure is not
/// decoration: the base-look estimator's whole method is comparing the
/// CAMERA's rendering against a neutral one, so a neutral develop silently
/// posing as a camera JPEG would make it measure zero difference and report a
/// confident "no base look". [`embedded_preview`] — the estimator's own door —
/// therefore keeps the strict "camera pixels or nothing" contract and never
/// comes here.
fn neutral_rendition(path: &Path, quarter_turns: u8) -> Result<DynamicImage> {
    eprintln!(
        "⚠ {} carries no embedded preview or thumbnail (its format does not store one), so \
         AutoShade is showing its own neutral develop instead of the camera's rendering",
        path.display()
    );
    crate::render::render_to_image(
        path,
        // NEUTRAL except for the turn: the develop must land in the same frame
        // as an extracted rendition would have, and `render_to_image` is the
        // one place that applies the composed orientation.
        &crate::recipe::EditRecipe { quarter_turns, ..Default::default() },
        None,
        Some(NEUTRAL_PREVIEW_EDGE),
    )
    .with_context(|| {
        format!(
            "{} has no embedded rendition, and developing the sensor as a stand-in failed too",
            path.display()
        )
    })
}

/// Decode a RAW file: embedded preview + metadata + histogram. Reads the file
/// only; never writes near the source.
///
/// The EXIF orientation only — the door for a caller that holds no recipe (the
/// `decode` CLI command, a reference-library build). A caller
/// that DOES hold one takes [`decode_raw_turned`].
pub fn decode_raw(path: &Path) -> Result<Decoded> {
    decode_raw_turned(path, 0)
}

/// [`decode_raw`] with the photographer's own quarter turns folded in
/// ([`crate::render::compose_orientation`]), so `Meta`'s display dims and the
/// oriented preview describe the frame the render and the export will produce
/// — not the one the camera happened to write.
///
/// A SEPARATE entry point rather than a parameter on `decode_raw` because most
/// callers genuinely have no recipe to read a turn from, and defaulting them
/// to 0 at each site is how the pre-v0.30 orientation drift happened. Here the
/// turn is impossible to forget: the un-turned door SAYS 0 in one place.
pub fn decode_raw_turned(path: &Path, quarter_turns: u8) -> Result<Decoded> {
    guard_tiff_chain(path)?;
    guard_raw_plane_extent(path)?;
    let src = RawSource::new(path).with_context(|| format!("open RAW {}", path.display()))?;
    let decoder = decoder_for(path, &src)?;
    let params = RawDecodeParams { image_index: 0 };

    // Prefer the mid-size embedded preview; fall back to the thumbnail, then
    // to the embedded FULL-SIZE camera JPEG (`full_image` extracts the
    // JPEGInterchangeFormat blob — it never develops the sensor; rawler's ARW
    // decoder in fact implements ONLY this level, so Sony files always
    // resolve here). Evaluated lazily to skip the larger JPEG decodes when a
    // cheaper level exists. `None` = the FORMAT embeds none (the ORF class) —
    // degraded to our own develop below, never a hard failure (A4).
    let camera_preview = guard_parser_panic(path, "embedded rendition", || {
        usable_rendition(
            path,
            &mut [
                ("preview_image", &mut || {
                    decoder.preview_image(&src, &params).map_err(|e| anyhow!("{e}"))
                }),
                ("thumbnail_image", &mut || {
                    decoder.thumbnail_image(&src, &params).map_err(|e| anyhow!("{e}"))
                }),
                ("full_image", &mut || {
                    decoder.full_image(&src, &params).map_err(|e| anyhow!("{e}"))
                }),
            ],
        )
    })?;

    let md = guard_parser_panic(path, "raw_metadata", || {
        decoder.raw_metadata(&src, &params).map_err(|e| anyhow!("raw_metadata: {e}"))
    })?;
    // Which way is up: EXIF tag 0x0112 off the metadata already in hand, NOT
    // the dummy raw's hard-coded field (see [`raw_orientation_of`]), COMPOSED
    // with the photographer's quarter turns (R27). Free — `md` is decoded one
    // line above, and the composition is a bit-twiddle.
    //
    // This composition is also what an imported sidecar's `tiff:Orientation`
    // is solved AGAINST (`render::quarter_turns_between`): the frame delivered
    // here is the frame Lightroom's own export has, because the importer chose
    // `quarter_turns` to make the two equal.
    let orientation = crate::render::compose_orientation(raw_orientation_of(&md), quarter_turns);

    // `dummy = true`: populate dimensions / WB / levels without decompressing
    // the full sensor data — we only need the structural metadata here.
    let raw = guard_parser_panic(path, "raw_image(dummy)", || {
        decoder.raw_image(&src, &params, true).map_err(|e| anyhow!("raw_image(dummy): {e}"))
    })?;

    // ONE EXIF extraction for both source kinds ([`exif_facts`]) — the APEX
    // aperture fallback and the finiteness filters live there now.
    let facts = exif_facts(&md.exif);
    let mut meta = Meta {
        make: md.make.trim().to_string(),
        model: md.model.trim().to_string(),
        lens: facts.lens,
        iso: facts.iso,
        shutter: facts.shutter,
        aperture: facts.aperture,
        focal_length_mm: facts.focal_length_mm,
        exposure_bias_ev: facts.exposure_bias_ev,
        date_time: facts.date_time,
        // DISPLAY-frame dims, agreeing with the oriented preview below:
        // sensor width/height made every rotated (portrait) shot's aspect —
        // and the style index's portrait feature — read as landscape.
        width: if orientation_transposes(orientation) { raw.height } else { raw.width },
        height: if orientation_transposes(orientation) { raw.width } else { raw.height },
        // Sony stores only 3 WB multipliers, so rawler leaves the 4th (second
        // green) as NaN. Replace any non-finite coeff with the neutral 1.0 —
        // otherwise serde_json refuses to serialise Meta when we hand it to the
        // advisor (JSON has no NaN).
        as_shot_wb_coeffs: {
            let mut wb = raw.wb_coeffs;
            for c in wb.iter_mut() {
                if !c.is_finite() {
                    *c = 1.0;
                }
            }
            wb
        },
    };

    // Orient the preview into the display frame (embedded previews are stored
    // in sensor orientation; see preview_only for the full rationale). Uses
    // the EXIF orientation resolved above — the same value Meta's dims used.
    // The A4 stand-in comes back from `render_to_image` ALREADY in the display
    // frame (it orients before the develop), so it must not be oriented twice.
    let preview = match camera_preview {
        Some(p) => crate::render::oriented(p, orientation),
        None => {
            // The decoder + source go first: `render_to_image` maps the whole
            // RAW again (~15-25 MB for the zoo's files, ~120 MB for a 61 MP
            // ARW), and holding this one underneath would double that peak for
            // no reason. Order matters — the decoder borrows the source.
            drop(decoder);
            drop(src);
            // The stand-in develops through `render_to_image`, which applies
            // the COMPOSED orientation itself — so it is handed the turn, not
            // oriented afterwards (that would turn it twice).
            neutral_rendition(path, quarter_turns)?
        }
    };
    // The DELIVERED pixels are the embedded preview, and its dims are the
    // camera's choice — they can differ from the sensor math above (a DNG
    // can report 4024×6048 while its preview is 4000×6000). These numbers
    // feed the AI prompt and the style index's aspect feature, and every
    // pixel this build ever serves comes from this preview — so they must
    // describe it (L05-6). The oriented image is already display-frame.
    meta.width = preview.width() as usize;
    meta.height = preview.height() as usize;

    // Histogram on a downscale-only copy of the preview — representative and
    // fast even for a 60 MP embedded JPEG (see hist_copy).
    let histogram = compute_histogram(&hist_copy(&preview));

    // ONE implementation ([`embedded_xmp`]): the per-decoder `xpacket` this
    // call used covers only CR2/CR3/RAF/TFR (rawler 0.7.2's default is
    // `Ok(None)`), so a DNG — the format that actually bakes develops into
    // the file — always reported "none" here. A read failure is disclosed,
    // never folded into absent (the old `.ok()` did exactly that).
    let embedded_xmp = match embedded_xmp(path) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("⚠ {e}");
            None
        }
    };

    Ok(Decoded { preview, meta, histogram, embedded_xmp })
}

/// The RAW's embedded XMP packet — the develop Lightroom bakes INTO a DNG
/// ("Store presets with this catalog" workflows write it for CR2/RAF too) —
/// bounded and honest: `Ok(None)` means no packet this reader can prove
/// exists (including a container that does not walk as TIFF — such a photo
/// fails loudly at decode, where the real diagnosis lives); a packet that
/// provably EXISTS but cannot be used (over-cap, non-text, unrecognised
/// type) is `Err`, never folded into "absent".
///
/// TIFF containers (DNG/ARW/NEF/ORF/RW2/CR2/…) are read through the SAME
/// bounded header walk the lens-metadata reader uses (`lensmeta::read`): a
/// `BufReader` + [`GenericTiffReader`] with a chain cap — roughly a root-IFD
/// parse, cheap enough for the open path. rawler's per-format `xpacket`
/// overrides cover only CR2/CR3/RAF/TFR, which would MISS the DNG tag
/// 0x02BC entirely; only the non-TIFF containers (CR3's BMFF, RAF's
/// proprietary header) go through the decoder — that branch maps the whole
/// file (`RawSource::new`), the cost `decode_raw` itself already pays, and
/// is reached for those two extensions alone.
pub fn embedded_xmp(path: &Path) -> Result<Option<String>> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let bytes: Option<Vec<u8>> = if matches!(ext.as_deref(), Some("jpg" | "jpeg" | "jpe")) {
        // A JPEG is not a TIFF container: its packet rides in APP1 segments,
        // and the walk below is the same marker walk `jpeg_exif_block` does
        // for EXIF. Before R27 this fell into the TIFF arm, `GenericTiffReader`
        // failed on the `FFD8` header and the answer came back as ABSENT — the
        // silent-absence this function's own contract forbids.
        let mut file = std::fs::File::open(path)
            .with_context(|| format!("open image {}", path.display()))?;
        jpeg_xmp_packet(&mut file)
            .with_context(|| format!("read the XMP packet in {}", path.display()))?
    } else if matches!(ext.as_deref(), Some("cr3" | "raf")) {
        guard_tiff_chain(path)?;
        let src =
            RawSource::new(path).with_context(|| format!("open RAW {}", path.display()))?;
        let decoder = decoder_for(path, &src)?;
        decoder
            .xpacket(&src, &RawDecodeParams { image_index: 0 })
            .map_err(|e| anyhow!("read the XMP packet in {}: {e}", path.display()))?
    } else {
        let file = std::fs::File::open(path)
            .with_context(|| format!("open RAW {}", path.display()))?;
        let mut reader = std::io::BufReader::new(file);
        // Same chain cap and rationale as `lensmeta::read` — see that call
        // site. A file whose header does not walk as TIFF has no TIFF tag to
        // hold a packet: that is absence, not unreadability (the photo
        // itself will fail loudly elsewhere if it is genuinely corrupt).
        let Ok(tiff) = GenericTiffReader::new(&mut reader, 0, 0, Some(16), &[]) else {
            return Ok(None);
        };
        match tiff.root_ifd().get_entry(TiffCommonTag::Xmp) {
            // BYTE is the spelling the XMP spec prescribes and rawler's own
            // tfr decoder reads; UNDEFINED is the one other shape real
            // writers use. Anything else is a packet we cannot account for —
            // said, not skipped.
            Some(entry) => match &entry.value {
                Value::Byte(b) | Value::Undefined(b) => Some(b.clone()),
                other => {
                    return Err(anyhow!(
                        "the XMP packet in {} has an unrecognised TIFF type ({})",
                        path.display(),
                        other.value_type()
                    ));
                }
            },
            None => None,
        }
    };
    let Some(buf) = bytes else { return Ok(None) };
    // The same 16 MiB ceiling every sidecar read enforces — refused, not
    // truncated (a partial packet would parse to a DIFFERENT develop).
    if buf.len() as u64 > crate::store::MAX_STORE_JSON {
        return Err(anyhow!(
            "the XMP packet in {} is larger than the {}-byte limit",
            path.display(),
            crate::store::MAX_STORE_JSON
        ));
    }
    let text = String::from_utf8(buf)
        .map_err(|_| anyhow!("the XMP packet in {} is not UTF-8 text", path.display()))?;
    // Packets are xpacket-padded with trailing whitespace; blank-after-trim
    // carries nothing to restore.
    Ok((!text.trim().is_empty()).then_some(text))
}

/// Just the embedded preview, skipping metadata/histogram — for the UI grid and
/// before/after, where only the image is needed.
pub fn preview_only(path: &Path) -> Result<DynamicImage> {
    preview_only_turned(path, 0)
}

/// [`preview_only`] in the frame the photographer's quarter turns produce —
/// what the gallery grid and the web thumbnails show, so a rotated photo reads
/// the way the export will.
pub fn preview_only_turned(path: &Path, quarter_turns: u8) -> Result<DynamicImage> {
    if !is_raw(path) {
        // baked-by-construction: the !is_raw arm of preview_only.
        let img = load_image(path)?;
        return Ok(crate::render::oriented(
            img,
            crate::render::quarter_turn_orientation(quarter_turns),
        ));
    }
    // A4: a format that embeds no rendition (the ORF class — see
    // `usable_rendition`) is DEGRADED to our own neutral develop, with a word
    // about it, instead of failing. This is the door the gallery grid and the
    // web thumbnails use, and the old error blanked their cells after three
    // silent retries (`bin/gui/workers.rs`), which reads as a broken file.
    match camera_rendition(path, quarter_turns)? {
        Some(img) => Ok(img),
        None => neutral_rendition(path, quarter_turns),
    }
}

/// The camera's own rendition of a RAW, oriented — `None` when it carries
/// none. The 3-level fallback (embedded preview → thumbnail → the embedded
/// full-size camera JPEG) shared by `preview_only` and `embedded_preview`,
/// which differ only in whether "none" is an error.
///
/// All three levels are camera-baked EXTRACTIONS, never a sensor develop
/// (see `decode_raw` for the rawler-source verification), so the base-look
/// estimator's camera-vs-neutral CDF comparison stays meaningful.
///
/// Embedded previews come back in SENSOR orientation (rawler's ARW path
/// decodes the JPEG bytes verbatim — verified in the crate source), so they
/// are oriented here with the render engine's own function: masks, crop and
/// straighten are all defined against the displayed preview and must mean the
/// same thing in the full-res render. The orientation comes from
/// [`raw_orientation_of`] on the RAW's metadata — strictly cheaper than the
/// dummy `raw_image` this used to build, and the only field of it we ever
/// read was the one rawler hard-codes.
fn camera_rendition(path: &Path, quarter_turns: u8) -> Result<Option<DynamicImage>> {
    guard_tiff_chain(path)?;
    let src = RawSource::new(path).with_context(|| format!("open RAW {}", path.display()))?;
    let decoder = decoder_for(path, &src)?;
    let params = RawDecodeParams { image_index: 0 };
    guard_parser_panic(path, "camera rendition", || {
        let img = if let Some(p) = decoder
            .preview_image(&src, &params)
            .map_err(|e| anyhow!("preview_image: {e}"))?
        {
            p
        } else if let Some(t) = decoder
            .thumbnail_image(&src, &params)
            .map_err(|e| anyhow!("thumbnail_image: {e}"))?
        {
            t
        } else if let Some(f) = decoder
            .full_image(&src, &params)
            .map_err(|e| anyhow!("full_image: {e}"))?
        {
            f
        } else {
            return Ok(None);
        };
        let md = decoder
            .raw_metadata(&src, &params)
            .map_err(|e| anyhow!("raw_metadata: {e}"))?;
        Ok(Some(crate::render::oriented(
            img,
            crate::render::compose_orientation(raw_orientation_of(&md), quarter_turns),
        )))
    })
}

/// The camera's OWN rendition only — `None` for a non-RAW or a RAW that
/// carries none.
///
/// `full_image` reads the JPEGInterchangeFormat blob for ARW (and the
/// analogous embedded image for CR2/CR3/DNG), and its DEFAULT impl returns
/// `Ok(None)` — verified in the rawler 0.7.2 source. That level-3 call is
/// essential in practice: rawler's ARW decoder implements ONLY `full_image`
/// (no preview/thumbnail overrides), so every A7RIV file answers there alone.
/// Callers (the base-look estimator) rely on this "camera pixels or nothing"
/// contract — a neutral develop standing in would make the camera-vs-neutral
/// CDF comparison meaningless.
pub fn embedded_preview(path: &Path) -> Result<Option<DynamicImage>> {
    embedded_preview_turned(path, 0)
}

/// [`embedded_preview`] in the frame the photographer's quarter turns produce.
/// The base-look estimator pairs it with a develop of the same photo, and a
/// luma CDF is orientation-blind — but the two images are also compared for
/// SIZE, and handing it the un-turned twin of a turned develop would be a
/// silent frame disagreement in the one place this module exists to prevent.
pub fn embedded_preview_turned(path: &Path, quarter_turns: u8) -> Result<Option<DynamicImage>> {
    if !is_raw(path) {
        return Ok(None);
    }
    camera_rendition(path, quarter_turns)
}

fn compute_histogram(img: &DynamicImage) -> Histogram {
    let rgb = img.to_rgb8();
    let (mut r, mut g, mut b, mut luma) = (vec![0u32; 256], vec![0u32; 256], vec![0u32; 256], vec![0u32; 256]);
    let (mut clip_black, mut clip_white, mut n) = (0u64, 0u64, 0u64);
    for px in rgb.pixels() {
        let (rr, gg, bb) = (px[0], px[1], px[2]);
        r[rr as usize] += 1;
        g[gg as usize] += 1;
        b[bb as usize] += 1;
        let y = (0.299 * rr as f32 + 0.587 * gg as f32 + 0.114 * bb as f32)
            .round()
            .clamp(0.0, 255.0) as usize;
        luma[y] += 1;
        if y <= 1 {
            clip_black += 1;
        }
        if y >= 254 {
            clip_white += 1;
        }
        n += 1;
    }
    let pct = |c: u64| if n > 0 { 100.0 * c as f32 / n as f32 } else { 0.0 };
    Histogram {
        luma,
        r,
        g,
        b,
        clip_black_pct: pct(clip_black),
        clip_white_pct: pct(clip_white),
        sample_pixels: n,
    }
}

impl Decoded {
    /// The preview downscaled so its long edge is at most `max_edge` px, for
    /// saving / sending to the advisor. Returns a borrow-free owned image.
    pub fn preview_resized(&self, max_edge: u32) -> DynamicImage {
        let (w, h) = self.preview.dimensions();
        if w.max(h) <= max_edge {
            self.preview.clone()
        } else {
            self.preview
                .resize(max_edge, max_edge, image::imageops::FilterType::Lanczos3)
        }
    }
}
