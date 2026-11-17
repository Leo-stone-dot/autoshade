//! AI denoise bridge — Rust side of the SCUNet sidecar (`python/denoise.py`).
//!
//! Like [`crate::generative`] (which shells out to OpenAI), this shells out to a
//! local Python process that runs a real-photo denoiser on the GPU. It is a
//! manual, opt-in pixel pre-process for high-ISO / astro / low-light frames — the
//! AI never decides *edits* here, it only removes sensor noise. The deterministic
//! develop pipeline (tone/colour/sharpen) runs in Rust afterward, so denoise
//! always happens BEFORE sharpening (the order that matters).
//!
//! Three entry points:
//!   * [`denoise_mosaic`] — denoise a decoded RAW's SENSOR MOSAIC in place,
//!     before demosaic (`python/denoise_raw.py`, DRUNet, the noise level
//!     measured on the frame; 2026-09-15). The render engine's RAW path.
//!   * [`denoise_buffer`] — denoise an in-memory full-res RGB buffer through
//!     SCUNet (`python/denoise.py`): the baked-source path, and the fallback
//!     for a sensor without a 2×2 Bayer mosaic.
//!   * [`denoise_file`]   — denoise an image file in place to another file (used
//!     when the source is an already-baked PNG/TIFF/JPEG).

use std::path::{Path, PathBuf};

use anyhow::{anyhow, bail, Context, Result};
use image::{DynamicImage, GenericImageView, ImageBuffer, Rgb};

use crate::config::Config;

pub(crate) mod hot_pixels;

const SIDECAR_DEFAULT_TIMEOUT_SECS: u64 = 30 * 60;
const SIDECAR_OUTPUT_CAP: usize = 1024 * 1024;

/// Where the SCUNet path starts when nothing chooses a strength — a BAKED
/// source on the CLI's `--strength` / `--denoise-strength`, the web export's
/// `denoise_strength`, and the sidecar's own `--strength` default (pinned to
/// this value textually in the tests below). ONE number for that path.
///
/// 0.5, not 1.0, and the reason is measured (2026-09-13, the user's 61 MP
/// ISO-640 frame): at 1.0 every SCUNet tier the sidecar ships keeps 1–7 % of
/// the frame's high-frequency energy (luma HF std 2.27 → 0.12/255 for
/// `color_real_psnr`) — the rock texture went with the noise. Strength is the
/// sidecar's luma/chroma split (`blend_luma_chroma` in `python/denoise.py`):
/// at 0.5 the colour speckle is gone in full (chroma HF 4.5 → 0.07/255) and
/// half the luminance grain comes back, and the texture with it.
pub const DEFAULT_STRENGTH: f32 = 0.5;

/// RAW luminance-noise removal after demosaic and calibration, in linear
/// light. Every positive value uses the complete clean mosaic at honest sigma;
/// the renderer returns (1-strength) of the original luminance along RGB grey.
/// 0 is the untouched input without a spawn; 1 skips the original's develop.
/// Baked sources keep SCUNet and [`DEFAULT_STRENGTH`].
pub const DEFAULT_STRENGTH_RAW: f32 = 0.71;

/// The default strength for `src`'s path: [`DEFAULT_STRENGTH_RAW`] for a RAW
/// (the mosaic-domain denoiser) and [`DEFAULT_STRENGTH`] for a baked source
/// (SCUNet). The CLI and the web export both answer an absent strength here,
/// so the two surfaces cannot drift.
pub fn default_strength_for(src: &Path) -> f32 {
    if crate::decode::is_raw(src) { DEFAULT_STRENGTH_RAW } else { DEFAULT_STRENGTH }
}

/// The sidecar budget is its OWN variable, not `AUTOSHADE_HTTP_TIMEOUT_SECS`:
/// that one tunes API latency, while a sidecar's first run legitimately spends
/// many minutes downloading a model — a user shortening API timeouts must not
/// silently cap the model download at the same number.
pub(crate) fn sidecar_timeout() -> std::time::Duration {
    // env_or_dotenv: .env-set sidecar budgets kept working when the
    // dotenv stopped writing the process environment (L16#3).
    let seconds = crate::config::env_or_dotenv("AUTOSHADE_SIDECAR_TIMEOUT_SECS")
        .and_then(|s| s.parse().ok())
        .filter(|s: &u64| *s > 0)
        .unwrap_or(SIDECAR_DEFAULT_TIMEOUT_SECS);
    std::time::Duration::from_secs(seconds)
}

/// Wait for a piped child without allowing either time or captured bytes to
/// grow without bound (`Command::output()` offered neither — a stalled helper
/// hung the CLI or GUI worker forever). Keeping the TAIL of each stream
/// preserves the traceback line `sidecar_tail` reports.
///
/// `budget_env` names the env var the timeout message tells the user to
/// raise — hard-coding AUTOSHADE_SIDECAR_TIMEOUT_SECS here sent the claude
/// verifier's users to the wrong knob (it reads AUTOSHADE_HTTP_TIMEOUT_SECS).
/// `group` is the child's [`crate::KillGroup`]: on the kill paths the WHOLE
/// tree dies (before the reaping `wait`, the unix pgid rule), and on every
/// path the group drops here — on Windows that close reaps any straggler
/// still holding the pipes after a normal exit.
pub(crate) fn bounded_child_output(
    mut child: std::process::Child,
    who: &str,
    budget: std::time::Duration,
    budget_env: &str,
    group: Option<crate::KillGroup>,
) -> Result<std::process::Output> {
    #[allow(clippy::disallowed_methods)] // the sidecar output drainer: one thread per pipe
    fn drain<R: std::io::Read + Send + 'static>(
        reader: Option<R>,
    ) -> std::thread::JoinHandle<(Vec<u8>, bool)> {
        std::thread::spawn(move || {
            let Some(mut reader) = reader else {
                return (Vec::new(), false);
            };
            let mut tail = std::collections::VecDeque::with_capacity(SIDECAR_OUTPUT_CAP);
            // 8 KiB reads: each chunk is far below the cap, so the tail logic
            // below never sees a single over-cap read.
            let mut chunk = [0u8; 8192];
            let mut truncated = false;
            loop {
                let count = match reader.read(&mut chunk) {
                    Ok(0) | Err(_) => break,
                    Ok(count) => count,
                };
                let excess =
                    tail.len().saturating_add(count).saturating_sub(SIDECAR_OUTPUT_CAP);
                if excess > 0 {
                    tail.drain(..excess);
                    truncated = true;
                }
                tail.extend(&chunk[..count]);
            }
            (tail.into_iter().collect(), truncated)
        })
    }

    fn mark_truncated(mut bytes: Vec<u8>, truncated: bool) -> Vec<u8> {
        if !truncated {
            return bytes;
        }
        const MARKER: &[u8] = b"[... output exceeded 1 MiB; tail follows ...]\n";
        let trim = bytes
            .len()
            .saturating_add(MARKER.len())
            .saturating_sub(SIDECAR_OUTPUT_CAP);
        if trim > 0 {
            bytes.drain(..trim);
        }
        let mut marked = Vec::with_capacity(MARKER.len() + bytes.len());
        marked.extend_from_slice(MARKER);
        marked.extend_from_slice(&bytes);
        marked
    }

    // Join a drain thread, but never forever: the DIRECT child being gone
    // does not close the pipes — a descendant it spawned (a launcher script's
    // python, a library worker) inherits them and can hold them open
    // indefinitely, and an unconditional join then hung this caller exactly
    // like the un-deadlined child this function exists to bound. Past the
    // grace the thread is DETACHED (it parks in a blocking read and dies
    // with the pipe or the process); its output is forfeited, disclosed as
    // truncated, to keep the deadline honest. std has no join-with-timeout,
    // so this polls is_finished the same way the try_wait loop below polls
    // the child.
    let join_bounded = |handle: std::thread::JoinHandle<(Vec<u8>, bool)>,
                        stream: &str|
     -> (Vec<u8>, bool) {
        let grace = std::time::Duration::from_secs(2);
        let start = std::time::Instant::now();
        while !handle.is_finished() {
            if start.elapsed() >= grace {
                eprintln!(
                    "⚠ a descendant process still holds the sidecar's {stream} after it exited — \
                     abandoning the drain (output truncated)"
                );
                return (Vec::new(), true);
            }
            std::thread::sleep(std::time::Duration::from_millis(50));
        }
        handle.join().unwrap_or_default()
    };

    let stdout_thread = drain(child.stdout.take());
    let stderr_thread = drain(child.stderr.take());
    let start = std::time::Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if start.elapsed() >= budget => {
                // The TREE first (before the reaping `wait` — the unix pgid
                // rule), then the direct child as the belt.
                if let Some(g) = &group {
                    g.kill_tree();
                }
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_bounded(stdout_thread, "stdout");
                let _ = join_bounded(stderr_thread, "stderr");
                bail!(
                    "{who} timed out after {:.1}s and was killed \
                     (raise {budget_env} if your runs are legitimately slower)",
                    budget.as_secs_f32()
                );
            }
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(200)),
            Err(error) => {
                if let Some(g) = &group {
                    g.kill_tree();
                }
                let _ = child.kill();
                let _ = child.wait();
                let _ = join_bounded(stdout_thread, "stdout");
                let _ = join_bounded(stderr_thread, "stderr");
                return Err(error).with_context(|| format!("poll {who}"));
            }
        }
    };

    let (stdout, stdout_truncated) = join_bounded(stdout_thread, "stdout");
    let (stderr, stderr_truncated) = join_bounded(stderr_thread, "stderr");
    Ok(std::process::Output {
        status,
        stdout: mark_truncated(stdout, stdout_truncated),
        stderr: mark_truncated(stderr, stderr_truncated),
    })
}

/// A failed run may remove a fresh file or a zero-byte claim, but must never
/// delete a pre-existing nonempty deliverable the user could still recover.
pub(crate) fn discard_failed_output(
    output: &Path,
    before: Option<(u64, Option<std::time::SystemTime>)>,
) {
    if matches!(before, None | Some((0, _))) {
        let _ = std::fs::remove_file(output);
    }
}

/// The unique staged sibling a denoise run writes before publishing: the
/// promised deliverable name is shared and deterministic
/// (`out/<stem>.denoised.tif`, keyed by stem alone), so the sidecar must
/// never write it directly. The REAL extension stays LAST — the python
/// sidecar picks cv2's encoder from it. Litter, disclosed: a hard kill can
/// orphan one of these beside the deliverable — the same exposure class as
/// the pre-existing `.part` siblings, and no sweeper exists for either.
fn staged_sibling(out: &Path) -> PathBuf {
    let ext = out.extension().and_then(|e| e.to_str()).unwrap_or("png");
    let stem = out.file_stem().and_then(|s| s.to_str()).unwrap_or("denoise");
    out.with_file_name(format!(
        "{stem}.dnstage.{}-{}.{ext}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ))
}

/// Release the caller's 0-byte claim at the promised name — and ONLY a
/// 0-byte claim (the GUI's `unique_out` pre-creates one; failed runs used to
/// eat the 999-name cap). A NONEMPTY file there is a concurrent run's
/// published deliverable and must survive this run's failure — exactly the
/// loss `discard_failed_output` inflicted at the shared name whenever its
/// pre-spawn snapshot predated that publish. The GUI helper's lib-side twin.
///
/// Registered residual (Codex R12 #4): the probe and the remove are two
/// steps, so a concurrent publish renaming onto the name BETWEEN them is
/// still deleted. Closing that fully needs a handle-owned delete
/// disposition, which stable std does not expose on Windows; this window is
/// microseconds where the old shared-name cleanup deleted unconditionally
/// across the whole sidecar runtime.
fn release_empty_claim(p: &Path) {
    if std::fs::metadata(p).is_ok_and(|m| m.len() == 0) {
        let _ = std::fs::remove_file(p);
    }
}

/// Everything the sidecar needs for one run. Built from [`Config`] so the render
/// engine stays decoupled from config/env.
pub struct DenoiseOpts {
    pub python_bin: String,
    /// `python/denoise.py` — SCUNet on developed pixels (the baked-source
    /// path and the non-Bayer fallback).
    pub script: PathBuf,
    /// `python/denoise_raw.py` — DRUNet on the sensor mosaic (the RAW path,
    /// 2026-09-15).
    pub raw_script: PathBuf,
    pub cache: PathBuf,
    /// The SCUNet tier; the mosaic path has one model and ignores it.
    pub model: String,
    /// 0..1, whose LAW depends on the path. On the mosaic ([`denoise_mosaic`])
    /// the renderer returns the original's luminance after demosaic: 0 the
    /// untouched input, every positive value keeps clean chroma, 1 the model's
    /// whole output, default [`DEFAULT_STRENGTH_RAW`]. On developed pixels
    /// ([`denoise_buffer`], since 2026-09-13) the sidecar blends the
    /// LUMINANCE by this amount and takes the model's chroma at
    /// `min(1, 2·strength)` — colour noise is the ugly half and colour detail
    /// is low-frequency, so it goes first (in full from 0.5 up), while
    /// luminance grain returns linearly and brings the texture with it;
    /// default [`DEFAULT_STRENGTH`]. 0 is the identity at every entry (no
    /// spawn; a bare sidecar run reaches the same bytes), 1 the model's whole
    /// output.
    pub strength: f32,
}

impl DenoiseOpts {
    /// `model_override` lets the CLI pick a SCUNet tier; `None` uses the config
    /// default (`color_real_psnr`). Callers with nothing chosen pass
    /// [`DEFAULT_STRENGTH`] — never their own literal.
    pub fn from_config(cfg: &Config, model_override: Option<String>, strength: f32) -> Self {
        DenoiseOpts {
            python_bin: cfg.sidecar_python(),
            script: PathBuf::from(&cfg.denoise_script),
            raw_script: PathBuf::from(&cfg.denoise_raw_script),
            cache: PathBuf::from(&cfg.weights_dir),
            model: model_override.unwrap_or_else(|| cfg.denoise_model.clone()),
            // clamp KEEPS NaN — a non-finite strength would ship "--strength
            // NaN" to the sidecar instead of honouring the 0..1 contract.
            strength: if strength.is_finite() { strength.clamp(0.0, 1.0) } else { 1.0 },
        }
    }
}

/// Denoise a full-resolution working buffer (sRGB-ENCODED values, nominally
/// `[0,1]` per channel) in place by round-tripping it through the sidecar as
/// a 16-bit PNG (see `temp_path` for why PNG, not TIFF). Boundary, disclosed:
/// the pack clamps to [0,1], so wide-gamut base colours carried as
/// out-of-range values do not survive an AI-denoise round trip — a denoised
/// export of such a photo clips those colours toward sRGB (recorded with the
/// single-working-space item).
#[allow(clippy::disallowed_methods)] // reads the sidecar's staged product
pub fn denoise_buffer(opts: &DenoiseOpts, data: &mut [[f32; 3]], w: usize, h: usize) -> Result<()> {
    if data.len() != w * h {
        bail!("denoise_buffer: buffer {} != {}x{}", data.len(), w, h);
    }
    // Strength 0 is the IDENTITY, not "denoise then blend nothing": the
    // 16-bit round trip below clamps to [0,1] and quantises, so a
    // zero-strength pass changed bytes it promised not to touch — wide-gamut
    // values carried out of range were clipped by a no-op (L09-10).
    if opts.strength <= 0.0 {
        return Ok(());
    }
    let tmp_in = temp_path("autoshade_dn_in")?;
    let tmp_out = match temp_path("autoshade_dn_out") {
        Ok(path) => path,
        Err(error) => {
            let _ = std::fs::remove_file(&tmp_in);
            return Err(error);
        }
    };

    // pack [f32;3] -> 16-bit RGB PNG (see temp_path) — clamps to [0,1]
    let mut buf16: Vec<u16> = Vec::with_capacity(w * h * 3);
    for px in data.iter() {
        buf16.push(to_u16(px[0]));
        buf16.push(to_u16(px[1]));
        buf16.push(to_u16(px[2]));
    }
    let img: ImageBuffer<Rgb<u16>, _> = ImageBuffer::from_raw(w as u32, h as u32, buf16)
        .ok_or_else(|| anyhow!("denoise: pack buffer size mismatch"))?;
    let result = (|| -> Result<DynamicImage> {
        DynamicImage::ImageRgb16(img)
            .save(&tmp_in)
            .with_context(|| format!("write denoise input {}", tmp_in.display()))?;
        run_sidecar(opts, &tmp_in, &tmp_out)?;
        // read 16-bit result back into the buffer. The expected dimensions
        // are already known, so derive real limits from them instead of
        // lifting every limit — a wrong-shaped helper output is rejected by
        // the decoder instead of allocated first (the old `no_limits()` let a
        // lying header drive an aborting allocation before the size check).
        let mut reader = image::ImageReader::open(&tmp_out)
            .with_context(|| format!("open denoise output {}", tmp_out.display()))?;
        reader.limits(denoise_output_limits(w, h));
        reader
            .decode()
            .with_context(|| format!("decode denoise output {}", tmp_out.display()))
    })();
    // BOTH temp files go regardless of WHERE the pipeline failed — a partial
    // input/output from a failed save / sidecar / decode used to leak
    // full-resolution frames into the temp dir on every error path.
    let _ = std::fs::remove_file(&tmp_in);
    let _ = std::fs::remove_file(&tmp_out);
    let out = result?;
    let (ow, oh) = out.dimensions();
    if ow as usize != w || oh as usize != h {
        bail!("denoise changed dimensions: {ow}x{oh} != {w}x{h}");
    }
    let rgb16 = out.to_rgb16();
    for (i, px) in rgb16.pixels().enumerate() {
        data[i] = [
            px[0] as f32 / 65535.0,
            px[1] as f32 / 65535.0,
            px[2] as f32 / 65535.0,
        ];
    }
    Ok(())
}

/// Denoise an image file to another file (the sidecar reads/writes the
/// pixels, preserving bit depth). For already-baked PNG/TIFF/JPEG. The
/// input's ICC profile is carried onto the STAGED product before the one
/// publish (review R12-07) — see `carry_icc_onto_staged`.
pub fn denoise_file(opts: &DenoiseOpts, input: &Path, output: &Path) -> Result<()> {
    crate::pipeline::ensure_parent(output)?;
    run_sidecar_carrying(opts, input, output, true, sidecar_timeout())
}

/// The sidecar copies pixel NUMBERS, not colour metadata (its python side
/// has no ICC handling at all) — so a ProPhoto/AdobeRGB input used to come
/// back UNTAGGED and every downstream reader interpreted the wide-gamut
/// numbers as sRGB (L09-11). Re-encodes the already-ACCEPTED decoded pixels
/// onto the STAGED path with the input's profile attached (review R12-07:
/// post-publish re-tagging could fail and leave an untagged product at the
/// promised name). The numbers are still the input-space numbers, so the
/// input's profile is their correct description. `ext` comes from the
/// PROMISED name — the staging sibling's suffix is machinery.
#[allow(clippy::disallowed_methods)] // reads the input's ICC profile
fn carry_icc_onto_staged(
    input: &Path,
    staged: &Path,
    output: &Path,
    img: &image::DynamicImage,
) -> Result<()> {
    use image::ImageEncoder as _;
    let profile = {
        let mut dec = image::ImageReader::open(input)
            .with_context(|| format!("open {}", input.display()))?
            .into_decoder()
            .with_context(|| format!("read image header {}", input.display()))?;
        image::ImageDecoder::icc_profile(&mut dec)
            .with_context(|| format!("read the colour profile of {}", input.display()))?
    };
    let Some(profile) = profile else { return Ok(()) };
    let ext = output.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    if !matches!(ext.as_str(), "png" | "jpg" | "jpeg" | "tif" | "tiff") {
        eprintln!(
            "⚠ the denoise product .{ext} cannot carry the source ICC profile — its colours \
             may be read as sRGB"
        );
        return Ok(());
    }
    // An encoder that refuses the profile (none of the three does today — the
    // refusal is the trait's default answer, for codecs that cannot carry one)
    // is disclosed the way an incapable extension is above, and the product is
    // still written: the create below truncates `staged`, so bailing here would
    // destroy the very product it was about to re-tag.
    let attach = |refused: Result<(), image::error::UnsupportedError>| {
        if let Err(e) = refused {
            eprintln!(
                "⚠ the denoise product .{ext} cannot carry the source ICC profile ({e}) — its \
                 colours may be read as sRGB"
            );
        }
    };
    let file = std::fs::File::create(staged)
        .with_context(|| format!("re-create {}", staged.display()))?;
    let mut w = std::io::BufWriter::new(file);
    match ext.as_str() {
        "png" => {
            let mut enc = image::codecs::png::PngEncoder::new(&mut w);
            attach(enc.set_icc_profile(profile.clone()));
            enc.write_image(img.as_bytes(), img.width(), img.height(), img.color().into())
                .context("re-encode the denoise product with the source profile")?;
        }
        "jpg" | "jpeg" => {
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(&mut w, 95);
            attach(enc.set_icc_profile(profile.clone()));
            enc.write_image(img.as_bytes(), img.width(), img.height(), img.color().into())
                .context("re-encode the denoise product with the source profile")?;
        }
        _ => {
            let mut enc = image::codecs::tiff::TiffEncoder::new(&mut w);
            attach(enc.set_icc_profile(profile.clone()));
            enc.write_image(img.as_bytes(), img.width(), img.height(), img.color().into())
                .context("re-encode the denoise product with the source profile")?;
        }
    }
    use std::io::Write as _;
    w.flush().context("flush the re-encoded product")
}

/// Denoise a source into a full-resolution 16-bit master at `out` (the GUI's
/// 「AI Denoise now」, which lands the master as a new ◈ card, and the CLI's
/// baked arm). A RAW goes through the engine's base develop of `part`
/// (v1.17.0, [`crate::render::base_develop`] — the profile the photograph
/// starts from, which the card's preview develop cannot add later) with the
/// denoise on its sensor mosaic ([`denoise_mosaic`], before demosaic) — the
/// full sensor, always: the ≤2048 px working-copy tier retired on 2026-09-15
/// (user decision), because a master baked from it capped every later export
/// of that card at 2048 px. A baked PNG/TIFF/JPEG is denoised as-is through
/// SCUNet.
pub fn denoise_active(
    opts: &DenoiseOpts,
    input: &Path,
    part: &crate::render::BasePart,
    out: &Path,
) -> Result<()> {
    if crate::decode::is_raw(input) {
        // The only arm that needs no pixels in memory: the full-res develop
        // denoises INSIDE the engine and writes `out` itself (16-bit),
        // stamped with the lens geometry the develop bakes (v1.17.0).
        crate::render::render_master_to_file(
            input,
            part,
            out,
            Some(opts),
            // The shipped channel (R29-1): this is the GUI's interactive
            // "denoise now", one photo at a time on a worker thread of its
            // own — there is no pooled transcript for it to be ordered into.
            crate::diag::stderr(),
        )?;
        return Ok(());
    }
    // A baked source takes the ONE source dispatch (`render::source_pixels`)
    // at its own resolution: decoded rather than handed straight to the
    // sidecar because cv2 ignores EXIF orientation (and imwrite drops the
    // tag), so a portrait phone JPEG came back as a permanently sideways
    // master.
    let img = crate::render::source_pixels(input, None)?;
    let tmp = temp_path("autoshade_denoise_base")?;
    // A DEEP working copy carries the working space's tag
    // (`render::write_working_space`) and the product inherits it
    // (`carry_icc_onto_staged`): `img` is this pipeline's sRGB (the decode
    // transformed any profile the source carried), and an untagged 16-bit
    // master re-read as "16-bit but carries no ICC profile", the warning
    // meant for an editor's ProPhoto export (2026-09-26). An 8-bit copy stays
    // untagged: it reads as sRGB everywhere without one, and carrying a tag
    // onto a JPEG product would re-encode it.
    let deep = !matches!(
        img.color(),
        image::ColorType::Rgb8 | image::ColorType::Rgba8 | image::ColorType::L8 | image::ColorType::La8
    );
    let written = if deep {
        crate::render::write_working_space(&tmp, &img, image::ImageFormat::Png)
    } else {
        img.save(&tmp).map_err(anyhow::Error::from)
    };
    // The working copy carries the source's lens-geometry marker (v1.17.0),
    // which the sidecar's product takes over with its colour profile
    // (`run_sidecar_carrying`): a denoised master is in its source's frame.
    let written = written.and_then(|()| crate::render::carry_baked_geometry(input, &tmp));
    if let Err(e) = written {
        // A failed save can still have created a partial file — don't leak it
        // into the temp dir.
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("write denoise input {}", tmp.display()));
    }
    // On disk now: holding the frame through the sidecar kept a second
    // whole-frame copy committed for minutes (2026-09-30, a 60 MP master).
    drop(img);
    let res = denoise_file(opts, &tmp, out);
    let _ = std::fs::remove_file(&tmp);
    res
}

/// What [`denoise_mosaic`] did with a decoded RAW.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MosaicDenoise {
    /// The mosaic was denoised in place — or the strength was 0 and the
    /// identity needed no sidecar.
    Denoised,
    /// This RAW carries no 2×2 Bayer mosaic to denoise (X-Trans, a four-colour
    /// or monochrome sensor, a linear DNG, floating-point samples): the caller
    /// discloses the reason and takes the developed-frame path instead.
    NotApplicable(String),
}

/// The three facts the mosaic sidecar needs about a sensor, read off the
/// decoded RAW by [`mosaic_args`]: the CFA letters at the mosaic origin, the
/// black level under each of those four sites and the white level.
#[derive(Debug, Clone, PartialEq)]
pub struct MosaicArgs {
    /// `RGGB` / `BGGR` / `GRBG` / `GBRG` — the letters at (0,0), (0,1), (1,0), (1,1).
    pub pattern: String,
    /// The black level at each of those four sites, in that order.
    pub black: [f32; 4],
    /// The white level (the smallest, when a file declares one per channel).
    pub white: f32,
}

/// Read the mosaic facts from a RAW's CFA and levels, or say in one sentence
/// why this sensor has no 2×2 Bayer mosaic to pack. Pure, so the refusals are
/// testable without a sensor fixture.
pub fn mosaic_args(
    cfa: &rawler::cfa::CFA,
    black: &rawler::rawimage::BlackLevel,
    white: &rawler::rawimage::WhiteLevel,
) -> std::result::Result<MosaicArgs, String> {
    use rawler::cfa::{CFA_COLOR_B, CFA_COLOR_G, CFA_COLOR_R};
    if (cfa.width, cfa.height) != (2, 2) {
        return Err(format!(
            "a {}×{} {} colour filter array is not a 2×2 Bayer mosaic",
            cfa.width, cfa.height, cfa
        ));
    }
    let mut pattern = String::with_capacity(4);
    for (row, col) in [(0, 0), (0, 1), (1, 0), (1, 1)] {
        pattern.push(match cfa.color_at(row, col) {
            c if c == CFA_COLOR_R => 'R',
            c if c == CFA_COLOR_G => 'G',
            c if c == CFA_COLOR_B => 'B',
            _ => return Err(format!("a {cfa} mosaic carries a colour other than R, G and B")),
        });
    }
    let g_on_a_diagonal = {
        let p = pattern.as_bytes();
        (p[0] == b'G' && p[3] == b'G') || (p[1] == b'G' && p[2] == b'G')
    };
    let mut sorted: Vec<char> = pattern.chars().collect();
    sorted.sort_unstable();
    if sorted != ['B', 'G', 'G', 'R'] || !g_on_a_diagonal {
        return Err(format!("a {pattern} mosaic is not one R, one B and two G on a diagonal"));
    }
    // The black level is a `width × height` (× cpp) pattern of its own — one
    // value for the frame, or one per CFA site.
    let bw = black.width.max(1);
    let bh = black.height.max(1);
    let cpp = black.cpp.max(1);
    let level_at = |row: usize, col: usize| -> Option<f32> {
        black
            .levels
            .get(((row % bh) * bw + (col % bw)) * cpp)
            .map(|r| r.as_f32())
    };
    let mut levels = [0.0f32; 4];
    for (i, (row, col)) in [(0, 0), (0, 1), (1, 0), (1, 1)].into_iter().enumerate() {
        levels[i] = level_at(row, col)
            .ok_or_else(|| format!("the file declares no black level for CFA site ({row},{col})"))?;
    }
    let white = white.0.iter().copied().min().ok_or("the file declares no white level")? as f32;
    if levels.iter().any(|b| !b.is_finite() || *b >= white) {
        return Err(format!("the black levels {levels:?} do not sit below the white level {white}"));
    }
    Ok(MosaicArgs { pattern, black: levels, white })
}

/// Cheap applicability check, shared with the renderer BEFORE it develops
/// an original luminance plane. No pixels are copied and no sidecar is run.
pub(crate) fn mosaic_args_for(raw: &rawler::RawImage) -> std::result::Result<MosaicArgs, String> {
    use rawler::rawimage::{RawImageData, RawPhotometricInterpretation as Photo};
    let cfa = match &raw.photometric {
        Photo::Cfa(c) => &c.cfa,
        Photo::LinearRaw => return Err("the file carries demosaiced (linear) data, not a sensor mosaic".into()),
        Photo::BlackIsZero => return Err("a monochrome sensor has no colour mosaic".into()),
    };
    if raw.cpp != 1 {
        return Err(format!("the sensor data has {} samples per pixel, not a mosaic", raw.cpp));
    }
    if !matches!(raw.data, RawImageData::Integer(_)) {
        return Err("the sensor data is floating point".into());
    }
    mosaic_args(cfa, &raw.blacklevel, &raw.whitelevel)
}

/// AI-denoise a decoded RAW's sensor mosaic IN PLACE, before demosaic — the
/// render engine's RAW path (`render_to_image_in`, 2026-09-15).
///
/// Why the mosaic and not the developed frame: measured on the user's
/// ILCE-7RM4A frames, the noise on the mosaic is white per CFA plane and
/// follows `var = a·x + b`, which the sidecar measures on the frame itself
/// (12 000 textureless blocks on a 61 MP frame) and hands to a NON-BLIND
/// model; after demosaic it is spatially correlated, and no sRGB-domain model
/// — the blind SCUNet or a non-blind one — separated it from texture. On the
/// ground-truth benchmark (`scripts/denoise_bench.py`, the measured ISO-640
/// model) this path scored 3.2–4.9 dB above SCUNet 1.0 on the whole frame
/// and 3.1–5.9 dB on the most-detailed blocks.
///
/// The contract with `python/denoise_raw.py` is one 16-bit GRAYSCALE PNG in
/// (the uncropped mosaic, samples as decoded) and one out (same dimensions,
/// same depth), plus the CFA letters and levels from [`mosaic_args`]; the
/// staged-output, exit-0-is-not-success and full-decode acceptance rules are
/// the ones every sidecar bridge follows. Strength 0 is the identity and
/// spawns nothing. A sensor without a 2×2 Bayer mosaic returns
/// [`MosaicDenoise::NotApplicable`] with the reason, and the caller falls back
/// to [`denoise_buffer`] on the developed frame.
#[allow(clippy::disallowed_methods)] // reads the RAW cleaner's staged product
pub fn denoise_mosaic(opts: &DenoiseOpts, raw: &mut rawler::RawImage) -> Result<MosaicDenoise> {
    use rawler::rawimage::RawImageData;
    if opts.strength <= 0.0 {
        return Ok(MosaicDenoise::Denoised);
    }
    let args = match mosaic_args_for(raw) {
        Ok(a) => a,
        Err(why) => return Ok(MosaicDenoise::NotApplicable(why)),
    };
    let (w, h) = (raw.width, raw.height);
    let pixels: Vec<u16> = match &raw.data {
        RawImageData::Integer(v) => v.clone(),
        RawImageData::Float(_) => {
            return Ok(MosaicDenoise::NotApplicable("the sensor data is floating point".into()));
        }
    };
    if pixels.len() != w * h {
        bail!("denoise: the mosaic holds {} samples for a {w}x{h} frame", pixels.len());
    }
    if !opts.raw_script.exists() {
        bail!(
            "RAW denoise sidecar not found at {} — run from the project dir or set \
             AUTOSHADE_DENOISE_RAW_SCRIPT.",
            opts.raw_script.display()
        );
    }
    let tmp_in = temp_path("autoshade_dn_mosaic_in")?;
    let staged = match temp_path("autoshade_dn_mosaic_out") {
        Ok(path) => path,
        Err(error) => {
            let _ = std::fs::remove_file(&tmp_in);
            return Err(error);
        }
    };
    // The claim `temp_path` made is a 0-byte file: `sidecar_wrote` reads
    // that snapshot and refuses a run that exited 0 without replacing it.
    let before = crate::artifact_state(&staged);
    let who = "RAW denoise sidecar";
    let result = (|| -> Result<Vec<u16>> {
        let plane = ImageBuffer::<image::Luma<u16>, Vec<u16>>::from_raw(w as u32, h as u32, pixels)
            .ok_or_else(|| anyhow!("denoise: pack mosaic size mismatch"))?;
        DynamicImage::ImageLuma16(plane)
            .save(&tmp_in)
            .with_context(|| format!("write the mosaic {}", tmp_in.display()))?;
        let mut cmd = crate::sidecar_command(&opts.python_bin);
        // `-E`: ignore PYTHON* environment variables (see `run_sidecar_carrying`).
        cmd.arg("-E")
            .arg(&opts.raw_script)
            .arg("--input")
            .arg(&tmp_in)
            .arg("--output")
            .arg(&staged)
            .arg("--pattern")
            .arg(&args.pattern)
            .arg("--black")
            .arg(
                args.black
                    .iter()
                    .map(|b| format!("{b}"))
                    .collect::<Vec<_>>()
                    .join(","),
            )
            .arg("--white")
            .arg(format!("{}", args.white))
            .arg("--strength")
            .arg("1.0000")
            .arg("--cache")
            .arg(&opts.cache);
        // Under the model slot, as every model sidecar (`crate::with_model_slot`).
        let out = crate::with_model_slot(|| {
            crate::run_sidecar_child(
                &mut cmd,
                who,
                &format!("{} {}", opts.python_bin, opts.raw_script.display()),
                sidecar_timeout(),
            )
        })?;
        if !out.status.success() {
            return Err(crate::sidecar_exit_error(who, &out));
        }
        crate::sidecar_wrote(who, &staged, before)?;
        // The product must FULLY decode as a 16-bit grayscale plane of the
        // frame's own dimensions — the same acceptance the developed-frame
        // bridge applies, for the same reason (a header check let a
        // truncated IDAT through).
        let mut reader = image::ImageReader::open(&staged)
            .with_context(|| format!("open the denoised mosaic {}", staged.display()))?;
        reader.limits(denoise_output_limits(w, h));
        let decoded = reader.decode().context("the denoised mosaic does not fully decode")?;
        let DynamicImage::ImageLuma16(plane) = decoded else {
            bail!("the denoised mosaic is not a 16-bit grayscale image ({:?})", decoded.color());
        };
        if (plane.width() as usize, plane.height() as usize) != (w, h) {
            bail!(
                "the denoised mosaic is {}x{} but the frame is {w}x{h}",
                plane.width(),
                plane.height()
            );
        }
        Ok(plane.into_raw())
    })();
    // BOTH temp files go regardless of where the pipeline failed.
    let _ = std::fs::remove_file(&tmp_in);
    let _ = std::fs::remove_file(&staged);
    let pixels = result.with_context(|| format!("{who} product rejected"))?;
    raw.data = RawImageData::Integer(pixels);
    Ok(MosaicDenoise::Denoised)
}

fn run_sidecar(opts: &DenoiseOpts, input: &Path, output: &Path) -> Result<()> {
    run_sidecar_carrying(opts, input, output, false, sidecar_timeout())
}

#[allow(clippy::disallowed_methods)] // reads the sidecar's staged product
fn run_sidecar_carrying(
    opts: &DenoiseOpts,
    input: &Path,
    output: &Path,
    carry_icc: bool,
    budget: std::time::Duration,
) -> Result<()> {
    if !opts.script.exists() {
        bail!(
            "denoise sidecar not found at {} — run from the project dir or set \
             AUTOSHADE_DENOISE_SCRIPT.",
            opts.script.display()
        );
    }
    // The sidecar writes a STAGED sibling, never the promised name: that
    // name is shared and deterministic, so two concurrent runs used to
    // interleave into ONE file — and a failing run's cleanup deleted the
    // deliverable a concurrent run had JUST published (the failer's
    // pre-spawn snapshot predated the publish, so it read as removable).
    // The snapshot now guards the staged path — fresh by construction, so
    // the exit-0-no-write refusal keeps its full strength.
    let staged = staged_sibling(output);
    let before = crate::artifact_state(&staged);
    // `crate::sidecar_command` carries the whole child preamble — the `.env`
    // ALLOWLIST, captured stdio, no console flash, a kill group — so this
    // bridge and the three others cannot drift apart. See it for why each of
    // those is not optional.
    let mut cmd = crate::sidecar_command(&opts.python_bin);
    // `-E`: ignore PYTHON* environment variables — a cwd .env's
    // `PYTHONPATH=.` beside a hostile `numpy.py` is code execution at
    // import time (config.rs also protects those vars; two layers).
    cmd.arg("-E")
        .arg(&opts.script)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(&staged)
        .arg("--model")
        .arg(&opts.model)
        .arg("--strength")
        .arg(format!("{:.4}", opts.strength))
        .arg("--cache")
        .arg(&opts.cache);
    // Cost of the shared capture, stated where a reader of THIS bridge will
    // look for it: the CLI no longer sees live model-download progress, only
    // the tail on failure.
    // Under the model slot, as every model sidecar (`crate::with_model_slot`).
    let run = crate::with_model_slot(|| {
        crate::run_sidecar_child(
            &mut cmd,
            "denoise sidecar",
            &format!("{} {}", opts.python_bin, opts.script.display()),
            budget,
        )
    });
    let out = match run {
        Ok(out) => out,
        Err(error) => {
            // A killed/failed run cleans its OWN staging and releases the
            // caller's 0-byte claim at the promised name — never a nonempty
            // file there, which is a concurrent run's published deliverable
            // now that the sidecar cannot write the shared name.
            discard_failed_output(&staged, before);
            release_empty_claim(output);
            return Err(error);
        }
    };
    if !out.status.success() {
        discard_failed_output(&staged, before);
        release_empty_claim(output);
        return Err(crate::sidecar_exit_error("denoise sidecar", &out));
    }
    // Exit 0 alone is NOT success: THIS run must have produced the artifact
    // (`crate::sidecar_wrote`). Without the pre-spawn snapshot, a sidecar
    // that wrote nothing still "succeeded" whenever the deterministic
    // deliverable name already held an EARLIER export — the check passed on
    // the stale file and the CLI presented last week's pixels as this run's.
    let wrote = crate::sidecar_wrote("denoise sidecar", &staged, before);
    if let Err(e) = wrote {
        discard_failed_output(&staged, before);
        release_empty_claim(output);
        return Err(e);
    }
    // A written artifact is still not a DELIVERABLE (L09-12, tightened by
    // review R12-07): the product must FULLY DECODE (a truncated IDAT walks
    // straight past a header check), match the input's dimensions, keep its
    // CHANNEL COUNT (RGB16-for-RGBA16 silently destroys alpha) and its
    // per-channel bit depth. The decoded pixels are reused for the ICC
    // carriage below, so the full decode is not a second pass.
    let accepted = (|| -> Result<image::DynamicImage> {
        let (iw, ih) = image::image_dimensions(input)
            .with_context(|| format!("read dimensions of {}", input.display()))?;
        let img =
            image::open(&staged).context("the denoise product does not fully decode")?;
        if (img.width(), img.height()) != (iw, ih) {
            bail!(
                "the denoise product is {}x{} but the input is {iw}x{ih}",
                img.width(),
                img.height()
            );
        }
        let in_color = {
            let dec = image::ImageReader::open(input)?.into_decoder()?;
            image::ImageDecoder::color_type(&dec)
        };
        let out_color = img.color();
        if out_color.channel_count() < in_color.channel_count() {
            bail!(
                "the denoise product has {} channel(s) where the input has {} — dropped \
                 channels (alpha) must not be published",
                out_color.channel_count(),
                in_color.channel_count()
            );
        }
        let per = |c: image::ColorType| c.bits_per_pixel() / u16::from(c.channel_count());
        if per(out_color) < per(in_color) {
            bail!(
                "the denoise product dropped to {}-bit channels from the input's {}-bit — \
                 the sidecar contract preserves bit depth",
                per(out_color),
                per(in_color)
            );
        }
        Ok(img)
    })();
    let decoded = match accepted {
        Ok(img) => img,
        Err(e) => {
            discard_failed_output(&staged, before);
            release_empty_claim(output);
            return Err(e.context("denoise sidecar product rejected"));
        }
    };
    // ICC carriage happens on the STAGED file, BEFORE the one publish
    // (review R12-07): the old post-publish re-encode could fail and leave
    // an untagged product at the promised name behind an error return.
    if carry_icc
        && let Err(e) = carry_icc_onto_staged(input, &staged, output, &decoded)
    {
        discard_failed_output(&staged, before);
        release_empty_claim(output);
        return Err(e.context("denoise product could not carry the source colour profile"));
    }
    drop(decoded);
    // The lens-geometry marker, on the STAGED file for the same reason
    // (v1.17.0): a denoise is pixel for pixel in its input's frame, and a
    // product published without the marker would be read in the sensor frame.
    if let Err(e) = crate::render::carry_baked_geometry(input, &staged) {
        discard_failed_output(&staged, before);
        release_empty_claim(output);
        return Err(e.context("denoise product could not carry the source's lens geometry"));
    }
    // Publish: ONE rename moves the finished bytes onto the promised name,
    // replacing the caller's 0-byte claim (or an older deliverable) in a
    // single step — concurrent runs can no longer interleave into the
    // shared name, and this run's failure paths never touch it.
    // durable_replace (L03): the staged result is fsynced before the
    // rename and the dir entry after — a denoise master referenced by a
    // durably-saved develop must survive the same power cut the JSON does.
    if let Err(e) = crate::store::durable_replace(&staged, output) {
        // The staged result SURVIVES a failed publish and the error names
        // it (Codex R12 #5): on Windows a viewer holding the destination
        // open without delete sharing fails this rename, and deleting the
        // staging here turned that transient conflict into total loss of a
        // minutes-long GPU result.
        release_empty_claim(output);
        return Err(e).with_context(|| {
            format!(
                "publish denoise output {} — the finished result is kept at {}",
                output.display(),
                staged.display()
            )
        });
    }
    Ok(())
}

fn temp_path(tag: &str) -> Result<PathBuf> {
    for _ in 0..1024 {
        let mut path = std::env::temp_dir();
        let stamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        // 16-bit PNG interchange: unambiguous for both cv2 and the image crate
        // (no TIFF predictor-tag mismatch), still lossless and full bit depth.
        // `create_new` CLAIMS the name atomically — a predictable PID+counter
        // path in the shared temp dir could be pre-created by another local
        // account (and a crash-orphaned twin from a recycled PID could be
        // read back as this photo's result).
        path.push(format!(
            "{tag}_{}_{}_{}.png",
            std::process::id(),
            stamp,
            unique()
        ));
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(0o600);
        }
        match options.open(&path) {
            Ok(_) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("claim denoise temp {}", path.display()));
            }
        }
    }
    bail!("could not claim a unique denoise temporary file after 1024 attempts")
}

/// Monotonic-ish suffix so two buffers in one process don't collide (no RNG dep).
fn unique() -> u64 {
    use std::sync::atomic::{AtomicU64, Ordering};
    static N: AtomicU64 = AtomicU64::new(0);
    N.fetch_add(1, Ordering::Relaxed)
}

/// Decoder limits derived from the frame we EXPECT back — a wrong-shaped or
/// lying-header helper output is refused by the decoder instead of allocated.
fn denoise_output_limits(w: usize, h: usize) -> image::Limits {
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(u32::try_from(w).unwrap_or(u32::MAX));
    limits.max_image_height = Some(u32::try_from(h).unwrap_or(u32::MAX));
    let pixels = u64::try_from(w)
        .unwrap_or(u64::MAX)
        .saturating_mul(u64::try_from(h).unwrap_or(u64::MAX));
    // Six bytes hold RGB16; the remaining allowance covers decoder staging
    // without returning to the old unlimited allocation policy.
    limits.max_alloc = Some(
        pixels
            .saturating_mul(16)
            .saturating_add(16 * 1024 * 1024),
    );
    limits
}

fn to_u16(v: f32) -> u16 {
    (v.clamp(0.0, 1.0) * 65535.0).round() as u16
}
