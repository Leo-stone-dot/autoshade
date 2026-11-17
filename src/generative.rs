//! Generative image editing (V2_PLAN §5) — a SEPARATE, EXPERIMENTAL concern from
//! the parametric develop pipeline. Default backend: OpenAI's Images `edits`
//! endpoint (gpt-image-*), which RE-GENERATES pixels. With
//! `AUTOSHADE_GENERATIVE=comfy`, [`comfy_reimagine`] runs the same contract on
//! the local ComfyUI server through comfy-cli (see `crate::comfy`).
//!
//! Phase 4 raises the pixel quality of this path ("give GPT higher-level pixels"):
//!   * **Flexible high-res sizing** — models with arbitrary-size support
//!     (gpt-image-2: any WIDTHxHEIGHT, edges ×16, ratio ≤3:1, ≤8 294 400 px) get
//!     the largest aspect-correct size inside `Config::openai_image_max_px`;
//!     models that reject it fall back to the fixed 1024/1536 enum on the API's
//!     400, so no model list is hard-coded. For a RAW whose embedded preview is
//!     smaller than the flexible target, the input is a full-sensor neutral
//!     develop (sharp real detail in, instead of an upscaled ~1.6 MP preview).
//!     The size is planned from the frame that is SENT: a RAW's sensor frame
//!     (`decode::frame_size`), never its embedded preview, which may carry an
//!     in-camera aspect crop (a 4:3 preview over a 3:2 sensor) — and such a
//!     preview is never the input either.
//!   * **Aspect-correct enum fallback** — 1536×1024 / 1024×1536 / 1024×1024 by
//!     orientation instead of squashing every photo into a 1:1 square.
//!   * **Configurable quality tier** (`low|medium|high|auto`, default `high`).
//!   * **`retouch` composites back onto the engine's own develop** — only the
//!     masked (inpainted) region carries generative pixels; the rest keeps the
//!     base pixels, with a feathered seam. For a RAW the base is the neutral
//!     develop (≤2048px, or the full sensor with `full_res`) — never the
//!     camera's baked 8-bit preview, so the master stays on the same tone
//!     chain as the canvas; a baked PNG/TIFF is its own base.
//!
//! `reimagine` = full-frame restyle (no mask) → still a generative re-render at
//! the chosen size, so it stays a low-res experiment / preview, NOT a master.
//! `retouch` = object removal / generative fill (RGBA mask; transparent pixels =
//! the region to regenerate; a blank prompt is the removal instruction,
//! [`REMOVE_PROMPT`]) → preview-resolution composite where only the masked
//! region is generative; the rest is the untouched source preview.
//! `adjust_onto` = prompt-only edit of a caller-developed image, no mask →
//! the whole returned frame, resized to the base dimensions, with measured D.

use std::path::Path;

use anyhow::{anyhow, Context, Result};
use image::imageops::FilterType;
use image::{DynamicImage, GenericImageView, RgbImage, Rgba, RgbaImage};

use crate::config::Config;
use crate::{decode, pipeline};


/// The faithfulness scaffold every `fidelity = "high"` reimagine prompt is
/// composed onto. The REQUEST parameter that used to carry this intent
/// (`input_fidelity`) is negotiated AWAY on models that reject it — which
/// gpt-image-2, the flexible-size flagship, does — so on the primary model
/// the prompt is the ONLY faithfulness signal that reaches the wire; the
/// parameter still rides along where it is accepted. `fidelity = "low"`
/// deliberately skips the scaffold: that mode is documented free rein.
const FAITHFUL_SCAFFOLD: &str =
    "Re-develop this exact photograph. Keep the camera angle, framing, geometry \
     and every physical object exactly as they are: do not add, remove, move, or \
     invent any content, structure, or texture. Change only the photographic \
     development - exposure, contrast, white balance, colour grading, tone \
     curves, local light and atmosphere. The result must read as the same \
     photograph, developed differently.";

/// Compose the wire prompt for a whole-frame reimagine. The user's direction
/// used to REPLACE the faithfulness sentence (the GUI's empty-field fallback
/// was the only place it existed), so any typed style wish silently dropped
/// the one preservation instruction — composed here, one mechanism for every
/// caller, the preservation clause is unconditional under "high".
fn hardened_prompt(user: &str, fidelity: &str) -> String {
    if fidelity.eq_ignore_ascii_case("low") {
        return user.to_string();
    }
    let user = user.trim();
    if user.is_empty() {
        FAITHFUL_SCAFFOLD.to_string()
    } else {
        format!("{FAITHFUL_SCAFFOLD}\nStyle direction: {user}")
    }
}

/// The instruction a generative fill sends when the user typed nothing. An
/// empty prompt used to be refused at both front ends ("write what should
/// fill the painted area") while the mask alone already said what the user
/// meant: the painted area is what should GO. Blank = remove, one rule for
/// the CLI, the browser and the GUI (user decision 2026-09-15): the model
/// continues the surroundings into the area and invents nothing new there.
pub const REMOVE_PROMPT: &str =
    "Remove everything inside the masked area and fill it with a seamless \
     continuation of the surrounding scene - the same background, surfaces, \
     lighting, texture, perspective and grain - so the area reads as if \
     nothing had ever been there. Do not add any new object, person, animal, \
     text or watermark. Leave everything outside the mask exactly as it is.";

/// The wire prompt for a fill: the user's own words, or [`REMOVE_PROMPT`]
/// when they wrote none. Trimmed, so a field of spaces is a blank field.
pub fn fill_prompt(user: &str) -> String {
    let user = user.trim();
    if user.is_empty() {
        REMOVE_PROMPT.to_string()
    } else {
        user.to_string()
    }
}

/// What one [`reimagine`] measured about its own result.
#[derive(Debug)]
pub struct ReimagineReport {
    /// Structural divergence between the input actually sent and the KEPT
    /// result — the same statistic (same function, same raster, same neutral
    /// base) the reverse-fit's mode selector will compute if the user asks
    /// for a recipe of this artifact, so `d >= fit::DIVERGENCE_GLOBAL` here
    /// means that fit will run in atmosphere mode.
    pub divergence: crate::fit::Divergence,
    /// `Some(d)` of the DISCARDED attempt when the opt-in retry bought a
    /// second generation (whichever of the two measured farther).
    pub first_divergence: Option<f32>,
}

/// The generation-side fidelity reading: decode both PNGs and measure the
/// reverse-fit's structural-divergence statistic between them. The two frames
/// need not share dimensions — `canonical_generated_png` admits a same-aspect
/// endpoint cap below the requested size (`size_is_an_endpoint_cap`) — and
/// they align because `fit::divergence_raster` resamples BOTH onto its fixed
/// 384×256 grid. What makes that alignment meaningful is the aspect, which
/// that door does refuse to let differ: a different aspect, or a LARGER
/// frame, is a different picture.
fn generation_divergence(sent_png: &[u8], generated_png: &[u8]) -> Result<crate::fit::Divergence> {
    let sent = image::load_from_memory(sent_png)
        .context("decode the sent input for the fidelity reading")?;
    let generated = image::load_from_memory(generated_png)
        .context("decode the generated result for the fidelity reading")?;
    // The analysis raster is a fixed 384x256 with an all-ones mask, so the
    // instrument resolves it; an abstention would mean the statistic changed
    // shape under this caller, and that is an error rather than a fidelity of
    // zero the caller would report as a perfect match.
    crate::fit::structure_divergence_for(
        &sent,
        &generated,
        &crate::recipe::EditRecipe::default(),
        None,
    )
    .context("the fidelity reading found no resolvable structural core")
}


// --- GUI worker wiring (progress heartbeats + cooperative cancel) -----------

/// Hooks a GUI worker installs on ITS OWN thread before a generative call:
/// `progress` receives the human-readable liveness lines (partial-image
/// heartbeats, negotiation notes) that otherwise reach only stderr; `cancel`
/// is a cooperative stop flag checked between negotiation attempts, per
/// stream event, and before the composite. Thread-local by design — the whole
/// call runs on the caller's thread, the web server threads requests, and a
/// process-wide global would cross-wire concurrent callers.
pub struct WorkerHooks {
    pub progress: Box<dyn FnMut(String)>,
    pub cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
}

thread_local! {
    static WORKER_HOOKS: std::cell::RefCell<Option<WorkerHooks>> =
        const { std::cell::RefCell::new(None) };
}

/// Install this thread's worker hooks. Workers run on a fresh thread each
/// (`spawn_worker`), so a hook can never leak onto another task — it dies
/// with the thread.
pub fn set_worker_hooks(hooks: Option<WorkerHooks>) {
    WORKER_HOOKS.with(|h| *h.borrow_mut() = hooks);
}

/// Mirror a liveness line into the installed GUI hook (no-op elsewhere).
/// Callers keep their own stderr/stdout prints — CLI output stays identical.
fn gui_progress(line: &str) {
    WORKER_HOOKS.with(|h| {
        if let Some(hooks) = h.borrow_mut().as_mut() {
            (hooks.progress)(line.trim_start().to_string());
        }
    });
}

/// Cooperative cancel: true once the user hit Cancel in the GUI.
fn cancelled() -> bool {
    WORKER_HOOKS.with(|h| {
        h.borrow().as_ref().is_some_and(|hooks| {
            hooks.cancel.load(std::sync::atomic::Ordering::Relaxed)
        })
    })
}

/// Full-frame generative restyle (the user's experiment). `fidelity` = "high"
/// composes the prompt onto [`FAITHFUL_SCAFFOLD`] so the model re-develops
/// the same photograph; "low" gives the model free rein (the user's prompt
/// goes to the wire verbatim). `quality` is the output tier
/// (low|medium|high|auto). After generation the structural divergence D
/// against the sent input is measured and disclosed; `retry_on_divergence`
/// (an explicit opt-in — it buys a SECOND generation) retries once when
/// `D >= fit::DIVERGENCE_GLOBAL` and keeps whichever result measures closer.
pub fn reimagine(
    cfg: &Config,
    raw_path: &Path,
    prompt: &str,
    fidelity: &str,
    quality: &str,
    retry_on_divergence: bool,
    out: &Path,
) -> Result<ReimagineReport> {
    // S5.11: the local ComfyUI backend, gated until its real-photo acceptance
    // run is green. Same report/contract as the paid-API path below.
    if crate::comfy::enabled() {
        return comfy_reimagine(cfg, raw_path, prompt, fidelity, quality, retry_on_divergence, out);
    }
    // FIRST, before any decode and before the BILLED image call (L09#1):
    // the parent used to be created 8 lines after paying, and its failure
    // dropped the generated bytes on the floor — pure money loss. Living in
    // the library fn covers every caller; for GUI/web it is a no-op
    // (unique_out already prepared the path).
    pipeline::preflight_out(out, raw_path)?;
    let src = decode::preview_only(raw_path)?;
    let preview = src.dimensions();
    // The plan sizes the frame that is SENT, because the pixels are resized
    // to it exactly. A RAW sends its sensor frame — `frame_size`'s
    // DefaultCropSize rectangle in display orientation, the frame the develop
    // below and `match` both use — never the embedded preview's: a body set
    // to a 4:3 aspect writes a 4:3 preview over its 3:2 sensor, and a plan
    // sized from that preview stretched the 3:2 develop into 4:3 on the wire
    // (v1.2.2; the target then came back 4:3 against a 3:2 develop).
    let frame = sent_frame(raw_path, preview)?;
    let sizes = SizePlan::for_source(cfg, frame.0, frame.1);
    let (sw, sh) = parse_size(sizes.try_first());

    // Input pixels: when the flexible target outresolves the embedded preview of
    // a RAW — or the preview is not the sensor frame at all — feed a full-sensor
    // neutral develop instead: real detail in, not an upscaled ~1.6 MP preview
    // (input quality bounds faithful-region output), in the frame the plan sized.
    let base = if develop_is_sent(decode::is_raw(raw_path), (sw, sh), preview, frame) {
        println!("  developing full sensor for a sharp high-res input …");
        gui_progress("developing full sensor for a sharp high-res input …");
        // Capped at 2× the API target edge: visually native for the final
        // Lanczos downsample, ~16× lighter than a full 61 MP develop.
        crate::render::render_to_image(
            raw_path,
            &crate::recipe::EditRecipe::default(),
            None,
            Some(sw.max(sh).saturating_mul(2)),
        )?
    } else {
        src
    };
    let small = DynamicImage::ImageRgb8(base.resize_exact(sw, sh, FilterType::Lanczos3).to_rgb8());
    let png = encode_png(&small)?;
    // Enum-size fallback input — aspect must match the size requested (L09-2).
    let fallback = sizes
        .flexible
        .is_some()
        .then(|| -> Result<Vec<u8>> {
            let (ew, eh) = parse_size(sizes.enum_size);
            encode_png(&DynamicImage::ImageRgb8(
                base.resize_exact(ew, eh, FilterType::Lanczos3).to_rgb8(),
            ))
        })
        .transpose()?;
    let primary_size = sizes.try_first().to_string();
    println!(
        "⚠ EXPERIMENTAL generative re-render via {} ({}, quality={quality} — regenerated pixels, not a master)",
        cfg.openai_image_model,
        sizes.try_first(),
    );
    let wire_prompt = hardened_prompt(prompt, fidelity);
    // Named so the opt-in retry below can ask for inputs a second time.
    let mut provide = |size: &str| -> Result<SizedInputs> {
        if size != primary_size
            && let Some(fb) = &fallback
        {
            return Ok((fb.clone(), None));
        }
        Ok((png.clone(), None))
    };
    let GeneratedImage { bytes: mut result, requested: used, accepted: mut on_disk } =
        call_images_edit(cfg, &mut provide, &wire_prompt, fidelity, &sizes, quality)?;
    if cancelled() {
        // The epoch already discards the UI-side result — writing the
        // artifact anyway left a persistent pixel master for an operation
        // the user abandoned (and kept its claimed name occupied; the
        // caller's 0-byte probe releases it once we bail here).
        return Err(anyhow!("cancelled by user"));
    }

    // The fidelity reading: D against the input the model was actually sent
    // (the enum-fallback bytes when that size was the one accepted).
    let sent_for = |used: &str| -> &[u8] {
        if used != primary_size
            && let Some(fb) = &fallback
        {
            return fb.as_slice();
        }
        png.as_slice()
    };
    let threshold = crate::fit::DIVERGENCE_GLOBAL;
    let mut divergence = generation_divergence(sent_for(&used), &result)?;
    let line = format!(
        "  fidelity: structural divergence D={:.3} vs the sent input (reverse-fit \
         global threshold {threshold:.2})",
        divergence.d
    );
    println!("{line}");
    gui_progress(&line);
    if divergence.d >= threshold {
        let warn = format!(
            "  ⚠ D={:.3} ≥ {threshold:.2}: the generation altered structure/content — a \
             reverse-fit of this result will fall back to atmosphere mode",
            divergence.d
        );
        println!("{warn}");
        gui_progress(&warn);
    }

    let mut first_divergence = None;
    if retry_on_divergence && divergence.d >= threshold {
        if cancelled() {
            return Err(anyhow!("cancelled by user"));
        }
        // A DELIBERATE second purchase, not a re-POST of a lost response:
        // the first generation is kept, measured, and only REPLACED when the
        // second measures closer to the input. Bounded to ONE retry, behind
        // an explicit opt-in. The no-re-POST-past-2xx rule inside
        // `call_images_edit` guards against paying twice for ONE image; this
        // is the user explicitly buying a second one.
        let line = format!(
            "  fidelity retry (opt-in): buying ONE more generation (D={:.3} ≥ \
             {threshold:.2})",
            divergence.d
        );
        println!("{line}");
        gui_progress(&line);
        let second =
            call_images_edit(cfg, &mut provide, &wire_prompt, fidelity, &sizes, quality)?;
        // Measured against what the SECOND call was sent, not the first.
        let d2 = generation_divergence(sent_for(&second.requested), &second.bytes)?;
        let line = format!(
            "  fidelity retry measured D={:.3} (first attempt D={:.3})",
            d2.d, divergence.d
        );
        println!("{line}");
        gui_progress(&line);
        if d2.d < divergence.d {
            first_divergence = Some(divergence.d);
            result = second.bytes;
            on_disk = second.accepted;
            divergence = d2;
        } else {
            first_divergence = Some(d2.d);
        }
        let keep = format!("  fidelity retry: keeping the D={:.3} result", divergence.d);
        println!("{keep}");
        gui_progress(&keep);
        if cancelled() {
            return Err(anyhow!("cancelled by user"));
        }
    }
    // (Parent already prepared by the pre-pay preflight at the top, L09#1.)
    // Stage + rename over the claimed name (the batch-13 export rule): a
    // crash mid-write must never leave a PARTIAL image wearing a name
    // pixels.json or a browser session may already reference.
    let tmp = out.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    if let Err(e) = std::fs::write(&tmp, &result) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("write {}", tmp.display()));
    }
    // durable_replace, not bare rename (L03): the pixel-source JSON that
    // will reference this master commits durably — the payload first.
    if let Err(e) = crate::store::durable_replace(&tmp, out) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("publish {}", out.display()));
    }
    // The size on disk, which is not the requested size when the endpoint capped.
    println!("generative -> {} ({on_disk}, generative re-render)", out.display());
    Ok(ReimagineReport { divergence, first_divergence })
}

/// The local-ComfyUI reimagine (S5.11). Same contract as the paid path — same
/// preflight, same PNG/size canonicalisation, same D measurement and report —
/// with a Comfy-friendly output size and the local server doing the model call.
#[allow(clippy::too_many_arguments)]
fn comfy_reimagine(
    _cfg: &Config,
    raw_path: &Path,
    prompt: &str,
    fidelity: &str,
    quality: &str,
    retry_on_divergence: bool,
    out: &Path,
) -> Result<ReimagineReport> {
    pipeline::preflight_out(out, raw_path)?;
    let src = decode::preview_only(raw_path)?;
    let preview = src.dimensions();
    let frame = sent_frame(raw_path, preview)?;
    let (sw, sh) = crate::comfy::plan_size_for_model(frame, crate::comfy::max_edge());
    let base = if develop_is_sent(decode::is_raw(raw_path), (sw, sh), preview, frame) {
        println!("  developing full sensor for a sharp high-res input …");
        gui_progress("developing full sensor for a sharp high-res input …");
        crate::render::render_to_image(
            raw_path,
            &crate::recipe::EditRecipe::default(),
            None,
            Some(sw.max(sh).saturating_mul(2)),
        )?
    } else {
        src
    };
    let small = DynamicImage::ImageRgb8(base.resize_exact(sw, sh, FilterType::Lanczos3).to_rgb8());
    let png = encode_png(&small)?;
    let wire_prompt = hardened_prompt(prompt, fidelity);
    let seed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as u64)
        .unwrap_or(42);
    let mut opts = crate::comfy::Img2Img {
        checkpoint: crate::comfy::checkpoint(),
        negative: "blurry, low quality, watermark, text, extra limbs".into(),
        steps: crate::comfy::steps_for(quality),
        cfg: 6.0,
        denoise: crate::comfy::denoise_for(fidelity),
        seed,
    };
    println!(
        "⚠ EXPERIMENTAL generative re-render via local ComfyUI ({}x{}, {} steps, denoise {:.2})",
        sw, sh, opts.steps, opts.denoise
    );
    gui_progress("comfyui: running img2img …");
    let bytes = crate::comfy::whole_frame(&png, &wire_prompt, &opts, quality)?;
    if cancelled() {
        return Err(anyhow!("cancelled by user"));
    }
    let primary = format!("{sw}x{sh}");
    let (mut result, mut accepted) = canonical_generated_png(&bytes, &primary)?;

    let threshold = crate::fit::DIVERGENCE_GLOBAL;
    let mut divergence = generation_divergence(&png, &result)?;
    let line = format!(
        "  fidelity: structural divergence D={:.3} vs the sent input (reverse-fit \
         global threshold {threshold:.2})",
        divergence.d
    );
    println!("{line}");
    gui_progress(&line);
    if divergence.d >= threshold {
        let warn = format!(
            "  ⚠ D={:.3} ≥ {threshold:.2}: the generation altered structure/content — a \
             reverse-fit of this result will fall back to atmosphere mode",
            divergence.d
        );
        println!("{warn}");
        gui_progress(&warn);
    }

    let mut first_divergence = None;
    if retry_on_divergence && divergence.d >= threshold {
        if cancelled() {
            return Err(anyhow!("cancelled by user"));
        }
        let line = format!(
            "  fidelity retry (opt-in): one more local generation (D={:.3} ≥ {threshold:.2})",
            divergence.d
        );
        println!("{line}");
        gui_progress(&line);
        opts.seed = opts.seed.wrapping_add(1);
        let second_bytes = crate::comfy::whole_frame(&png, &wire_prompt, &opts, quality)?;
        let (second, second_accepted) = canonical_generated_png(&second_bytes, &primary)?;
        let d2 = generation_divergence(&png, &second)?;
        let line = format!(
            "  fidelity retry measured D={:.3} (first attempt D={:.3})",
            d2.d, divergence.d
        );
        println!("{line}");
        gui_progress(&line);
        if d2.d < divergence.d {
            first_divergence = Some(divergence.d);
            result = second;
            accepted = second_accepted;
            divergence = d2;
        } else {
            first_divergence = Some(d2.d);
        }
        let keep = format!("  fidelity retry: keeping the D={:.3} result", divergence.d);
        println!("{keep}");
        gui_progress(&keep);
        if cancelled() {
            return Err(anyhow!("cancelled by user"));
        }
    }

    // Same stage-then-replace protocol as the paid path: a crash mid-write must
    // never leave a partial image wearing a claimed name.
    let tmp = out.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    if let Err(e) = std::fs::write(&tmp, &result) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("write {}", tmp.display()));
    }
    if let Err(e) = crate::store::durable_replace(&tmp, out) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("publish {}", out.display()));
    }
    println!(
        "generative -> {} ({}x{}, ComfyUI re-render)",
        out.display(),
        accepted.0,
        accepted.1
    );
    Ok(ReimagineReport { divergence, first_divergence })
}

/// Object removal / generative fill on the SOURCE's own pixels. `mask_path`
/// is an RGBA PNG; transparent (alpha=0) pixels mark the region to
/// regenerate. A blank `prompt` is the removal instruction ([`fill_prompt`]);
/// typed words ride verbatim. The generative result is composited back over
/// the base so only the masked region is re-rendered.
///
/// For a RAW the base is the engine's OWN base develop of `part` (v1.17.0,
/// [`crate::render::base_develop`]) — a ≤2048px
/// thumbnail by default, the full sensor (e.g. 61 MP) with `full_res` so the
/// untouched area keeps native resolution (slow; the regenerated patch is
/// upscaled). It is deliberately NOT the camera's baked 8-bit preview, which
/// this used to composite onto: that swapped the canvas onto camera-curve
/// pixels mid-session and put later edits/exports on a different tone chain
/// (see `retouch::heal`). For a baked PNG/TIFF the base is the full image
/// either way, so `full_res` changes nothing. This is the CLI's and the
/// browser's fill; the GUI hands [`retouch_onto`] the active card's
/// developed picture instead.
#[allow(clippy::too_many_arguments)] // the fill's own inputs: config, source, its base part, mask, words, quality, resolution, output
pub fn retouch(
    cfg: &Config,
    raw_path: &Path,
    part: &crate::render::BasePart,
    mask_path: &Path,
    prompt: &str,
    quality: &str,
    full_res: bool,
    out: &Path,
) -> Result<()> {
    let raw = decode::is_raw(raw_path);
    // ONE dispatch for both source kinds (`render::source_pixels`). The cap is
    // RAW-only here: preview mode develops a RAW AT ≤2048 (the cap runs before
    // tone/geometry) instead of developing 61 MP and thumbnailing the result,
    // while a baked source is composited at its OWN resolution either way (the
    // doc above) — capping it would shrink the delivered master.
    retouch_onto(
        cfg,
        raw_path,
        || crate::render::master_pixels_for(raw_path, part, (raw && !full_res).then_some(2048)),
        if full_res && raw { "full-res" } else { "preview" },
        &FillJob { mask_path, prompt, quality, out, layer: false },
    )
}

/// The four things every fill needs whatever its base: the painted mask, the
/// user's words (blank = remove), the quality tier and the claimed output path.
pub struct FillJob<'a> {
    /// RGBA PNG; transparent (alpha=0) pixels mark the region to regenerate.
    pub mask_path: &'a Path,
    /// What should fill the region; blank is the removal instruction.
    pub prompt: &'a str,
    /// `low` / `medium` / `high` / `auto`.
    pub quality: &'a str,
    /// The claimed `./out` master this fill publishes.
    pub out: &'a Path,
    /// Publish a LAYER instead of a composite (2026-10-01, the GUI): the
    /// generated pixels with the feathered painted area as their alpha and
    /// nothing elsewhere, for `render::PixelLayer` to lay over the card's
    /// develop. The same paste onto a transparent frame instead of the base.
    pub layer: bool,
}

/// The fill on a base its CALLER produces. `base()` runs INSIDE the
/// full-resolution section (it is the whole-frame local phase), the model
/// call runs outside it, and the composite re-enters it. [`retouch`] hands
/// this the source's neutral develop; the GUI hands it the active card's
/// developed picture, so the model is shown what the user sees and the
/// answer is that picture with the painted area regenerated. On the neutral
/// base (every build through v1.3.4) the model was shown a develop 0.6–1.4 EV
/// under the card's Before with none of the card's sliders or masks, and the
/// card's content-keyed corrections then landed on content they were never
/// solved for — the user's 「不是基于当前变体生成的，而是原图」(2026-09-15).
///
/// `source` is the photo the output is guarded against (the read-only-library
/// preflight) and named after; `base_note` is the word the progress line
/// prints for the base; `job` is the fill itself ([`FillJob`]). The base comes
/// with the frame it is in ([`crate::render::Master`]): the painted mask is
/// mapped onto it, and the master published is stamped with it.
pub fn retouch_onto(
    cfg: &Config,
    source: &Path,
    base: impl FnOnce() -> Result<crate::render::Master>,
    base_note: &str,
    job: &FillJob<'_>,
) -> Result<()> {
    // FIRST, before the develop and the BILLED call (L09#1).
    pipeline::preflight_out(job.out, source)?;
    // Blank = remove, shared by the CLI, browser and GUI.
    let prompt = fill_prompt(job.prompt);
    edit_onto(cfg, base, base_note, &ImageEditJob {
        keep: Keep::Painted(job.mask_path), prompt: &prompt, quality: job.quality, out: job.out, layer: job.layer,
    })?;
    Ok(())
}

/// A fill whose area the MODEL chooses (user request 2026-10-12: 「跟原先的涂抹后ai生成一个道理，
/// 只不过省去了涂抹这一步骤」): the words say what to change — "remove the tree on the
/// left", "make the sky bluer" — and no mask is painted.
pub struct AutoFillJob<'a> {
    /// What to change; a blank instruction is refused (there is no painted
    /// area to read "remove" off).
    pub prompt: &'a str,
    /// `low` / `medium` / `high` / `auto`.
    pub quality: &'a str,
    /// The claimed `./out` layer this edit publishes.
    pub out: &'a Path,
}

/// What one [`auto_fill_onto`] found.
#[derive(Debug)]
pub struct AutoFillReport {
    /// The share of the frame the model changed and the layer covers, 0..=1.
    pub coverage: f32,
}

/// The wire prompt of an [`AutoFillJob`]: the user's instruction under a
/// clause that keeps the rest of the photograph as it is, because everything
/// the model changes beyond the instruction is read back as part of the edit.
fn auto_fill_prompt(user: &str) -> String {
    format!(
        "Edit this photograph exactly as instructed and change nothing else: keep the framing, \
         geometry, light, colours, grain and every detail the instruction does not concern \
         exactly as they are.\nInstruction: {}",
        user.trim()
    )
}

/// The edit with no painted area: the whole picture goes to the model with
/// no mask, and the area it CHANGED — found by comparing its answer with the
/// picture it was sent ([`found_region`]) — is published as a layer, levelled
/// onto its surroundings like a painted fill. Everything the model left alone
/// stays the card's own develop.
pub fn auto_fill_onto(
    cfg: &Config,
    source: &Path,
    base: impl FnOnce() -> Result<crate::render::Master>,
    base_note: &str,
    job: &AutoFillJob<'_>,
) -> Result<AutoFillReport> {
    pipeline::preflight_out(job.out, source)?;
    if job.prompt.trim().is_empty() {
        return Err(anyhow!("an edit without a painted area needs words saying what to change"));
    }
    let prompt = auto_fill_prompt(job.prompt);
    let outcome = edit_onto(cfg, base, base_note, &ImageEditJob {
        keep: Keep::Found, prompt: &prompt, quality: job.quality, out: job.out, layer: true,
    })?;
    Ok(AutoFillReport { coverage: outcome.coverage.context("an edit without a painted area found no area")? })
}

/// A prompt-only edit of the caller's whole picture, without a painted mask.
pub struct AdjustJob<'a> {
    /// What to change. Unlike a fill, a whole-image adjust refuses blank words.
    pub prompt: &'a str,
    /// `low` / `medium` / `high` / `auto`.
    pub quality: &'a str,
    /// The claimed `./out` master this adjust publishes.
    pub out: &'a Path,
}

/// What one whole-image [`adjust_onto`] measured against its SENT input.
#[derive(Debug)]
pub struct AdjustReport {
    /// The same structural D as reimagine, before resizing back to the base.
    pub divergence: f32,
}

/// Edit the caller's picture WITHOUT a mask; keep the whole returned frame,
/// resized back to the base's dimensions. The GUI supplies the generated
/// card's live develop and lands the result as a new, neutral ✨ card.
/// Preflight, admission slots, size negotiation, cancel and durable output
/// are shared with [`retouch_onto`]; neither path buys a divergence retry.
pub fn adjust_onto(
    cfg: &Config,
    source: &Path,
    base: impl FnOnce() -> Result<crate::render::Master>,
    base_note: &str,
    job: &AdjustJob<'_>,
) -> Result<AdjustReport> {
    pipeline::preflight_out(job.out, source)?;
    let prompt = job.prompt.trim();
    if prompt.is_empty() {
        return Err(anyhow!("a whole-image adjust needs a non-blank prompt"));
    }
    let outcome = edit_onto(cfg, base, base_note, &ImageEditJob {
        keep: Keep::Whole, prompt, quality: job.quality, out: job.out, layer: false,
    })?;
    Ok(AdjustReport { divergence: outcome.divergence.context("whole-image adjust has no divergence reading")? })
}

/// Which of the model's pixels an edit keeps.
#[derive(Clone, Copy)]
enum Keep<'a> {
    /// The painted area of this RGBA mask (transparent = regenerate); the
    /// mask rides on the request.
    Painted(&'a Path),
    /// The area the model's answer changed ([`found_region`]); no mask is sent.
    Found,
    /// The whole returned frame.
    Whole,
}

/// One shared request/size ladder and output path; `keep` decides what of
/// the answer is published.
struct ImageEditJob<'a> {
    keep: Keep<'a>,
    prompt: &'a str,
    quality: &'a str,
    out: &'a Path,
    layer: bool,
}

/// What [`edit_onto`] measured: D for a whole frame, the found area's share
/// for [`Keep::Found`].
struct EditOutcome {
    divergence: Option<f32>,
    coverage: Option<f32>,
}

fn edit_onto(
    cfg: &Config,
    base: impl FnOnce() -> Result<crate::render::Master>,
    base_note: &str,
    job: &ImageEditJob<'_>,
) -> Result<EditOutcome> {
    let ImageEditJob { keep, prompt, quality, out, layer } = *job;
    // A17: the LOCAL full-resolution phase, one at a time process-wide
    // (`crate::full_res_slot`). It is released explicitly before the model call
    // below and re-entered for the composite, because those are the two phases
    // that hold whole frames — holding an admission slot across a call that
    // runs for minutes would stall every export for that entire time.
    let heavy = crate::full_res_slot();
    let master = base()?;
    // The painted mask is SENSOR-frame; the base may be lens-corrected
    // (v1.17.0), so the mask is read in the base's frame.
    let mask_img = match keep {
        Keep::Painted(path) => Some(DynamicImage::ImageRgba8(
            master.rgba_mask_to_frame(
                &crate::render::open_mask_raster(path).with_context(|| format!("open mask {}", path.display()))?,
                path,
            )?,
        )),
        Keep::Found | Keep::Whole => None,
    };
    let crate::render::Master { image: base, baked, .. } = master;
    let (bw, bh) = base.dimensions();
    // A generative tile larger than the base is pointless (it only gets downscaled
    // back onto it) — cap the flexible budget at the base's own pixel count.
    let budget = cfg.openai_image_max_px.min(bw.saturating_mul(bh));
    let sizes = SizePlan::for_budget(bw, bh, budget);
    let (sw, sh) = parse_size(sizes.try_first());

    // API input must be 8-bit (the full-res base is 16-bit). Derive the small
    // image from THIS base so the generated pixels match its look (no seam shift).
    let small = DynamicImage::ImageRgb8(base.resize_exact(sw, sh, FilterType::Lanczos3).to_rgb8());
    let png = encode_png(&small)?;
    drop(small);
    let mask_at = |w, h| {
        mask_img.as_ref().map(|m| encode_png(&m.resize_exact(w, h, FilterType::Nearest))).transpose()
    };
    let mask_png = mask_at(sw, sh)?;
    // The enum-size fallback pair, built EAGERLY from the same 16-bit base:
    // the base is dropped just below (the A7 memory staging), and a retry
    // must send an input whose aspect matches the size it requests (L09-2).
    let fallback = sizes
        .flexible
        .is_some()
        .then(|| -> Result<(Vec<u8>, Option<Vec<u8>>)> {
            let (ew, eh) = parse_size(sizes.enum_size);
            let fpng = encode_png(&DynamicImage::ImageRgb8(
                base.resize_exact(ew, eh, FilterType::Lanczos3).to_rgb8(),
            ))?;
            Ok((fpng, mask_at(ew, eh)?))
        })
        .transpose()?;
    let primary_size = sizes.try_first().to_string();

    // The composite below needs only 8-bit pixels — take that form NOW and
    // let the 16-bit master go, so the minutes-long model call holds the
    // ~240 MB composite buffer instead of the ~366 MB master, and the
    // post-call peak loses a full 16-bit frame (A7). A cancel that already
    // arrived is honoured BEFORE paying the conversion; an immediately
    // failing call (bad key, rejected request) does pay this sub-second
    // pass first — accepted against the minutes-scale win (Codex batch-35).
    if cancelled() {
        return Err(anyhow!("cancelled by user"));
    }
    // A whole-image adjust keeps none of the base pixels, so only a fill
    // retains this full-frame buffer across the model call.
    // A layer pastes onto a TRANSPARENT frame (`FillJob::layer`): the
    // premultiplied paste below then leaves exactly the generated pixels
    // with the feathered weight as alpha, and the base is not kept at all.
    let composite = (!matches!(keep, Keep::Whole))
        .then(|| if layer { RgbaImage::new(bw, bh) } else { base.to_rgba8() });
    drop(base);
    // Out of the full-resolution section: what survives across the model call
    // is the ~240 MB composite (the A7 staging above), and waiting on the
    // network is not what the slot exists to serialise.
    drop(heavy);

    let (verb, mode) = match keep {
        Keep::Painted(_) => ("fill", "composite"),
        Keep::Found => ("edit", "changed area"),
        Keep::Whole => ("adjust", "whole image"),
    };
    let backend = if crate::comfy::enabled() {
        "ComfyUI (local)"
    } else {
        cfg.openai_image_model.as_str()
    };
    println!(
        "⚠ EXPERIMENTAL generative {verb} via {backend} ({}, quality={quality}, base={bw}x{bh} {}; {mode})",
        sizes.try_first(),
        base_note
    );
    // `requested` identifies the input ACTUALLY sent, including the enum
    // fallback. A capped return's delivered size must never select it.
    let result = call_images_edit(
        cfg,
        &mut |size| {
            if size != primary_size
                && let Some(fb) = &fallback
            {
                return Ok(fb.clone());
            }
            Ok((png.clone(), mask_png.clone()))
        },
        prompt,
        "high",
        &sizes,
        quality,
    )?;
    if cancelled() {
        // The user gave up while the model ran; skip the (full-res) composite
        // — at 61 MP it is real work whose result would be discarded anyway.
        return Err(anyhow!("cancelled by user"));
    }
    // The input actually sent, and its mask (the enum fallback's when the
    // negotiation fell back) — what the model's answer is measured against.
    let (sent_png, sent_mask) = match &fallback {
        Some((fpng, fmask)) if result.requested != primary_size => (fpng, fmask.as_ref()),
        _ => (&png, mask_png.as_ref()),
    };

    // Composite the regenerated region back onto the base. Upscale the generative
    // tile to base dimensions; the user's mask (alpha=0 = regenerate) becomes the
    // blend weight, feathered for a soft seam. Buffer LIFETIMES are staged so
    // a 61 MP full-res fill never holds every plane at once (the old
    // one-expression flow peaked ~1.8 GB; the staged flow halved that, and
    // dropping the 16-bit master BEFORE the model call — see above — takes
    // another full frame off this peak): the full-res mask exists only long
    // enough to become the weight plane, the generative frame is never
    // copied to RGBA, and the blur runs in place.
    //
    // STILL FULL-FRAME, deliberately: the generative upscale and the weight
    // plane are each ~241 MB at 61 MP. Banding them top-to-bottom would need
    // a hand-written replica of `image`'s Lanczos3 two-pass that is
    // byte-identical to it (a resampler that differs in the last bit is a
    // rendering change wearing an optimisation's clothes), which is its own
    // piece of work with its own oracle tests — not a comment away.
    // `to_rgb8`, not `to_rgba8`: the blend reads only RGB and writes an
    // opaque alpha, so the RGBA copy was 241 MB of pure waste at 61 MP —
    // and it coexisted with the 181 MB RGB frame it was copied from.
    // Back into the full-resolution section for the COMPOSITE: the generative
    // upscale and the weight plane are ~241 MB each at 61 MP, and this is the
    // phase the slot was scoped to.
    let _heavy = crate::full_res_slot();
    let answer = image::load_from_memory(&result.bytes).context("decode generative result")?;
    let mut outcome = EditOutcome { divergence: None, coverage: None };
    let Some(mut composite) = composite else {
        outcome.divergence = Some(generation_divergence(sent_png, &result.bytes)?.d);
        // No composite and no mask: keep the WHOLE returned frame, at the
        // base's dimensions, using the fill's Lanczos3 filter and RGBA8 writer.
        let whole = answer.resize_exact(bw, bh, FilterType::Lanczos3).to_rgba8();
        publish_master(&whole, out, baked.as_ref())?;
        println!("generative {verb} -> {} ({bw}x{bh}, {mode})", out.display());
        return Ok(outcome);
    };

    // The model never answers ONLY inside the area: it redraws the whole
    // frame, and the frame it draws is shifted from the one it was sent —
    // measured on a user's fill (2026-10-12) at +42 / +40 / +24 levels in
    // R/G/B right at the feathered edge, which is the seam the user saw. The
    // pixels it was meant to leave alone say by how much, so the kept area is
    // levelled by that shift before it is pasted ([`level_field`]), and the
    // area a maskless edit changed is read off the same comparison.
    let sent = image::load_from_memory(sent_png).context("decode the sent input")?.to_rgb8();
    let (tw, th) = sent.dimensions();
    let answer_at_sent = answer.resize_exact(tw, th, FilterType::Triangle).to_rgb8();
    let region = match keep {
        Keep::Painted(_) => {
            let m = image::load_from_memory(sent_mask.context("a painted fill sends its mask")?)
                .context("decode the sent mask")?
                .to_rgba8();
            Region::from_mask(&m)
        }
        Keep::Found => {
            let found = found_region(&sent, &answer_at_sent);
            let share = found.share();
            if share <= 0.0 {
                return Err(anyhow!(
                    "the model's answer changed nothing this edit could find — paint the area and use Remove / Fill instead"
                ));
            }
            outcome.coverage = Some(share);
            found
        }
        Keep::Whole => unreachable!("a whole-frame edit keeps no composite"),
    };
    let level = level_field(&sent, &answer_at_sent, &region);
    drop(answer_at_sent);
    drop(sent);
    let mut gen_img = answer.resize_exact(bw, bh, FilterType::Lanczos3).to_rgb8();
    drop(answer);
    // Cancel checkpoints through the composite: at 61 MP the decode/weight/
    // blend stages take seconds each, and a cancel arriving during them was
    // ignored — the abandoned result then landed in ./out anyway.
    if cancelled() {
        return Err(anyhow!("cancelled by user"));
    }
    let frame_feather = ((bw.min(bh) as usize) / 100).clamp(2, 64); // ~1% of short side, capped
    let weight = {
        let w = region.weight_at(bw, bh);
        // Feather scales with the SELECTION, not only the frame (L09-3):
        // the frame rule alone put a 20 px blur on a 10 px selection at a
        // 2048 frame — centre weight ~6%, and the fill "looked like it did
        // nothing". The frame rule stays as the CEILING, so large
        // selections keep their exact look.
        let feather = frame_feather.min(selection_feather(&w, bw as usize));
        if feather > 0 { box_blur(w, bw as usize, bh as usize, feather) } else { w }
    };
    if cancelled() {
        return Err(anyhow!("cancelled by user"));
    }
    level.apply(&mut gen_img, &weight);
    composite_in_place(&mut composite, &gen_img, &weight);
    drop(gen_img);
    if cancelled() {
        return Err(anyhow!("cancelled by user"));
    }
    publish_master(&composite, out, baked.as_ref())?;
    println!("generative {verb} -> {} ({bw}x{bh}, {mode})", out.display());
    Ok(outcome)
}

/// Stage + rename over the claimed name — same rule as reimagine. The
/// staged name cannot carry the real extension, so the format comes from
/// the TARGET path explicitly (PNG for every claimed pixel master). (Parent
/// already prepared by the pre-pay preflight, L09#1.) Stamped with the lens
/// geometry in the base's pixels before the publish (v1.17.0).
fn publish_master(img: &RgbaImage, out: &Path, baked: Option<&crate::render::BakedGeometry>) -> Result<()> {
    let tmp = out.with_extension(format!(
        "tmp.{}.{}",
        std::process::id(),
        crate::store::next_tmp_seq()
    ));
    let fmt = image::ImageFormat::from_path(out).unwrap_or(image::ImageFormat::Png);
    if let Err(e) = img.save_with_format(&tmp, fmt) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("write {}", tmp.display()));
    }
    if let Some(g) = baked
        && let Err(e) = crate::render::stamp_baked_geometry(&tmp, g)
    {
        let _ = std::fs::remove_file(&tmp);
        return Err(e);
    }
    // durable_replace, not bare rename (L03): the pixel-source JSON that
    // will reference this master commits durably — the payload first.
    if let Err(e) = crate::store::durable_replace(&tmp, out) {
        let _ = std::fs::remove_file(&tmp);
        return Err(e).with_context(|| format!("publish {}", out.display()));
    }
    Ok(())
}

/// The area an edit regenerates, at the resolution of the input it SENT:
/// 1 = take the model's pixel, 0 = keep the base's. A painted mask gives it
/// texel for texel; [`found_region`] gives it per cell.
struct Region {
    w: usize,
    h: usize,
    take: Vec<f32>,
}

impl Region {
    /// From the RGBA mask on the wire: transparent = regenerate.
    fn from_mask(m: &RgbaImage) -> Self {
        let take = m.pixels().map(|p| 1.0 - p[3] as f32 / 255.0).collect();
        Region { w: m.width() as usize, h: m.height() as usize, take }
    }

    /// The share of the frame the model's pixels are taken over.
    fn share(&self) -> f32 {
        self.take.iter().sum::<f32>() / self.take.len().max(1) as f32
    }

    /// The weight plane at the base's `bw`×`bh`, by nearest texel — the
    /// painted mask's own rule (`FilterType::Nearest`); the caller feathers.
    fn weight_at(&self, bw: u32, bh: u32) -> Vec<f32> {
        let (bw, bh) = (bw as usize, bh as usize);
        let mut out = Vec::with_capacity(bw * bh);
        for y in 0..bh {
            let sy = (y * self.h / bh.max(1)).min(self.h - 1);
            out.extend((0..bw).map(|x| self.take[sy * self.w + (x * self.w / bw.max(1)).min(self.w - 1)]));
        }
        out
    }
}

/// The cell edge of the levelling and finding grids on a `w`×`h` sent frame:
/// ~1/96 of the short side — coarse enough that the model re-drawing grass or
/// rock in its own texture averages out, fine enough to follow a sky's
/// gradient into the painted area.
fn cell_edge(w: usize, h: usize) -> usize {
    (w.min(h) / 96).max(2)
}

/// A smooth per-channel offset over the frame, in 8-bit levels, stored on a
/// coarse grid of `cell`-px cells of the sent frame.
struct LevelField {
    gw: usize,
    gh: usize,
    v: Vec<[f32; 3]>,
}

/// How far the model's answer sits from the sent input where it was meant to
/// change nothing — `region.take == 0` — as a smooth field that runs on into
/// the area it did change. Each cell holds the mean of (sent − answer) over its
/// kept texels; cells with none are filled from their neighbours by
/// pull-push ([`pull_push`]), the membrane-like interpolation Poisson-style
/// blending reduces to at low frequency. Added to the model's pixels, it puts
/// the regenerated area on the level of what surrounds it while leaving the
/// structure the model drew inside untouched.
fn level_field(sent: &RgbImage, answer: &RgbImage, region: &Region) -> LevelField {
    let (w, h) = (sent.width() as usize, sent.height() as usize);
    let cell = cell_edge(w, h);
    let (gw, gh) = (w.div_ceil(cell), h.div_ceil(cell));
    let mut sum = vec![[0f32; 3]; gw * gh];
    let mut kept = vec![0f32; gw * gh];
    for y in 0..h {
        for x in 0..w {
            let k = 1.0 - region.take[y * w + x];
            if k <= 0.0 {
                continue;
            }
            let i = (y / cell) * gw + x / cell;
            let (s, a) = (sent.get_pixel(x as u32, y as u32), answer.get_pixel(x as u32, y as u32));
            for c in 0..3 {
                sum[i][c] += k * (s[c] as f32 - a[c] as f32);
            }
            kept[i] += k;
        }
    }
    let area = (cell * cell) as f32;
    let mut v = vec![[0f32; 3]; gw * gh];
    let mut conf = vec![0f32; gw * gh];
    for i in 0..gw * gh {
        if kept[i] > 0.0 {
            v[i] = sum[i].map(|s| s / kept[i]);
            conf[i] = (kept[i] / area).min(1.0);
        }
    }
    LevelField { gw, gh, v: pull_push(v, conf, gw, gh) }
}

impl LevelField {
    /// The offset at normalised frame position (u, v), bilinear between cell
    /// centres.
    fn at(&self, u: f32, v: f32) -> [f32; 3] {
        let fx = (u * self.gw as f32 - 0.5).clamp(0.0, (self.gw - 1) as f32);
        let fy = (v * self.gh as f32 - 0.5).clamp(0.0, (self.gh - 1) as f32);
        let (x0, y0) = (fx as usize, fy as usize);
        let (x1, y1) = ((x0 + 1).min(self.gw - 1), (y0 + 1).min(self.gh - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let p = |x: usize, y: usize| self.v[y * self.gw + x];
        let (a, b, c, d) = (p(x0, y0), p(x1, y0), p(x0, y1), p(x1, y1));
        std::array::from_fn(|k| {
            (a[k] * (1.0 - tx) + b[k] * tx) * (1.0 - ty) + (c[k] * (1.0 - tx) + d[k] * tx) * ty
        })
    }

    /// Level the model's pixels wherever the composite will take any of them.
    fn apply(&self, img: &mut RgbImage, weight: &[f32]) {
        let (w, h) = img.dimensions();
        for y in 0..h {
            for x in 0..w {
                if weight[(y * w + x) as usize] <= 0.0001 {
                    continue;
                }
                let off = self.at((x as f32 + 0.5) / w as f32, (y as f32 + 0.5) / h as f32);
                let p = img.get_pixel_mut(x, y);
                for c in 0..3 {
                    p[c] = (p[c] as f32 + off[c]).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
    }
}

/// Fill a grid's unknown cells from its known ones: pull — halve the grid,
/// each coarse cell the confidence-weighted mean of its four — until one cell
/// is left, then push — every fine cell keeps its own value in proportion to
/// its confidence and takes the rest from the coarser level, read bilinearly.
/// A fully known cell is returned exactly; a hole gets a smooth blend of the
/// values around it, the nearer ones weighing more.
fn pull_push(v: Vec<[f32; 3]>, conf: Vec<f32>, w: usize, h: usize) -> Vec<[f32; 3]> {
    if w <= 1 && h <= 1 {
        return v;
    }
    let (w2, h2) = (w.div_ceil(2), h.div_ceil(2));
    let mut cv = vec![[0f32; 3]; w2 * h2];
    let mut cc = vec![0f32; w2 * h2];
    for y in 0..h {
        for x in 0..w {
            let (i, j) = (y * w + x, (y / 2) * w2 + x / 2);
            for c in 0..3 {
                cv[j][c] += conf[i] * v[i][c];
            }
            cc[j] += conf[i];
        }
    }
    for j in 0..w2 * h2 {
        if cc[j] > 0.0 {
            cv[j] = cv[j].map(|s| s / cc[j]);
        }
        cc[j] = cc[j].min(1.0);
    }
    let coarse = pull_push(cv, cc, w2, h2);
    let mut out = v;
    for y in 0..h {
        for x in 0..w {
            let i = y * w + x;
            let c = conf[i].clamp(0.0, 1.0);
            if c >= 1.0 {
                continue;
            }
            let fx = ((x as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (w2 - 1) as f32);
            let fy = ((y as f32 + 0.5) / 2.0 - 0.5).clamp(0.0, (h2 - 1) as f32);
            let (x0, y0) = (fx as usize, fy as usize);
            let (x1, y1) = ((x0 + 1).min(w2 - 1), (y0 + 1).min(h2 - 1));
            let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
            let p = |x: usize, y: usize| coarse[y * w2 + x];
            for (k, v) in out[i].iter_mut().enumerate() {
                let up = (p(x0, y0)[k] * (1.0 - tx) + p(x1, y0)[k] * tx) * (1.0 - ty)
                    + (p(x0, y1)[k] * (1.0 - tx) + p(x1, y1)[k] * tx) * ty;
                *v = c * *v + (1.0 - c) * up;
            }
        }
    }
    out
}



/// Per cell of a `cell`-px grid over `sent` and `answer`: how far the answer
/// sits from `expect` — the sent frame moved by a drift — in mean colour, and
/// how far its texture (luma spread) changed, folded into one score.
fn cell_scores(
    sent: &RgbImage,
    answer: &RgbImage,
    cell: usize,
    gw: usize,
    drift: &dyn Fn(usize) -> [f32; 3],
) -> Vec<f32> {
    let (w, h) = (sent.width() as usize, sent.height() as usize);
    let luma = |p: [f32; 3]| 0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2];
    // Per cell: Σ(answer − expected) per channel, Σ and Σ² of each luma, count.
    let mut acc = vec![[0f64; 8]; gw * h.div_ceil(cell)];
    for y in 0..h {
        for x in 0..w {
            let i = (y / cell) * gw + x / cell;
            let d = drift(i);
            let (s, a) = (sent.get_pixel(x as u32, y as u32), answer.get_pixel(x as u32, y as u32));
            // Clamped like the answer is: a drift cannot lift a clipped white.
            let e: [f32; 3] = std::array::from_fn(|c| (s[c] as f32 + d[c]).clamp(0.0, 255.0));
            let a: [f32; 3] = std::array::from_fn(|c| a[c] as f32);
            let (le, la) = (luma(e) as f64, luma(a) as f64);
            let t = &mut acc[i];
            for c in 0..3 {
                t[c] += (a[c] - e[c]) as f64;
            }
            t[3] += le;
            t[4] += le * le;
            t[5] += la;
            t[6] += la * la;
            t[7] += 1.0;
        }
    }
    acc.iter()
        .map(|t| {
            let k = t[7].max(1.0);
            let colour = (0..3).map(|c| (t[c] / k).powi(2)).sum::<f64>().sqrt();
            let sd = |sum: f64, sq: f64| (sq / k - (sum / k).powi(2)).max(0.0).sqrt();
            (colour + 0.5 * (sd(t[5], t[6]) - sd(t[3], t[4])).abs()) as f32
        })
        .collect()
}

/// The area a maskless edit CHANGED, read off its answer against the input
/// it was sent, cell by cell ([`cell_edge`]).
///
/// The model moves the whole frame a little (see [`edit_onto`]) and redraws
/// every edge and texture a pixel or two off, so "changed" is read against
/// both. The drift is modelled as little as it can be — one shift per
/// channel, the median over the half of the cells that moved least, then a
/// smooth field ([`pull_push`]) learnt ONLY from the cells that shift leaves
/// unchanged — because a model
/// any freer learns the edit itself: measured on a real "make the sky bluer"
/// (2026-10-12; sky −28 / −9 / +14 levels, ground +7 / +2 / −1), a per-level
/// tone curve absorbed the whole sky as drift and found 1 % of the frame.
/// Texels at 0 or 255 are left out of the drift's estimate, and the expected
/// frame is clamped, so a clipped white is not read as a change.
///
/// Thresholds are the larger of a floor in 8-bit levels and the scatter of
/// the cells taken for drift (median + k·MAD); a seed must ALSO reach a fixed share of the
/// strongest change in the frame, which is what keeps the redrawn edges
/// (scores ~10–25 on the same real answer, the removed tree 69) out of the
/// area. Seeds grow through weaker connected cells, specks of one or two
/// cells are dropped, enclosed holes are filled and the area is grown by one
/// cell so the feather starts outside what changed.
fn found_region(sent: &RgbImage, answer: &RgbImage) -> Region {
    const SEED_FLOOR: f32 = 14.0;
    const GROW_FLOOR: f32 = 7.0;
    const PROMINENCE: f32 = 0.4;
    let (w, h) = (sent.width() as usize, sent.height() as usize);
    let cell = cell_edge(w, h);
    let (gw, gh) = (w.div_ceil(cell), h.div_ceil(cell));
    let n = gw * gh;
    // Per cell: the mean shift over its unclipped texels, and how many.
    let mut shift = vec![[0f64; 4]; n];
    for y in 0..h {
        for x in 0..w {
            let (s, a) = (sent.get_pixel(x as u32, y as u32), answer.get_pixel(x as u32, y as u32));
            if (0..3).any(|c| s[c] <= 2 || s[c] >= 253 || a[c] <= 2 || a[c] >= 253) {
                continue;
            }
            let t = &mut shift[(y / cell) * gw + x / cell];
            for c in 0..3 {
                t[c] += a[c] as f64 - s[c] as f64;
            }
            t[3] += 1.0;
        }
    }
    let measured: Vec<usize> = (0..n).filter(|&i| shift[i][3] > 0.0).collect();
    let mean_shift = |i: usize| -> [f32; 3] { std::array::from_fn(|c| (shift[i][c] / shift[i][3].max(1.0)) as f32) };
    let median = |mut v: Vec<f32>| -> f32 {
        if v.is_empty() {
            return 0.0;
        }
        v.sort_by(f32::total_cmp);
        v[v.len() / 2]
    };
    // The scatter of the cells taken for drift, not of the frame: an edit as
    // large as the sky makes the scores bimodal, and a MAD over both modes put
    // the thresholds above the edit itself.
    let thresholds = |score: &[f32], drift_cells: &[usize]| -> (f32, f32) {
        let quiet: Vec<f32> = drift_cells.iter().map(|&i| score[i]).collect();
        let med = median(quiet.clone());
        let mad = 1.4826 * median(quiet.iter().map(|s| (s - med).abs()).collect());
        ((med + 8.0 * mad).max(SEED_FLOOR), (med + 4.0 * mad).max(GROW_FLOOR))
    };
    // Pass 1: one shift for the frame, the median over the half of the cells
    // that moved LEAST — the model's drift is the smaller movement (2–7
    // levels on the real answers above, an edit 30 and more), and over all
    // cells a sky-sized edit would be the median itself.
    // By RANK, not "at or below the median": on a frame whose edit moved
    // every one of its cells alike, the ties put the whole edit in.
    let size = |d: [f32; 3]| d.iter().map(|v| v * v).sum::<f32>();
    let mut still = measured.clone();
    still.sort_by(|&i, &j| size(mean_shift(i)).total_cmp(&size(mean_shift(j))));
    still.truncate(still.len().div_ceil(2));
    let global: [f32; 3] = std::array::from_fn(|c| median(still.iter().map(|&i| mean_shift(i)[c]).collect()));
    let first = cell_scores(sent, answer, cell, gw, &|_| global);
    let (_, grow1) = thresholds(&first, &still);
    // Pass 2: a smooth field from the cells pass 1 left alone.
    let conf: Vec<f32> = (0..n).map(|i| if first[i] < grow1 && shift[i][3] > 0.0 { 1.0 } else { 0.0 }).collect();
    let field = if conf.iter().any(|&c| c > 0.0) {
        pull_push((0..n).map(mean_shift).collect(), conf.clone(), gw, gh)
    } else {
        vec![global; n]
    };
    let score = cell_scores(sent, answer, cell, gw, &|i| field[i]);
    let calm: Vec<usize> = (0..n).filter(|&i| conf[i] > 0.0).collect();
    let (seed, grow) = thresholds(&score, if calm.is_empty() { &still } else { &calm });
    let seed = seed.max(PROMINENCE * score.iter().copied().fold(0.0, f32::max));
    // Seeds, then 4-connected growth through the weaker cells.
    let mut on: Vec<bool> = score.iter().map(|&s| s > seed).collect();
    let mut stack: Vec<usize> = (0..n).filter(|&i| on[i]).collect();
    while let Some(i) = stack.pop() {
        let (x, y) = (i % gw, i / gw);
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx < gw && ny < gh {
                let j = ny * gw + nx;
                if !on[j] && score[j] > grow {
                    on[j] = true;
                    stack.push(j);
                }
            }
        }
    }
    // Specks: a component of one or two cells is a redrawn edge or a fleck of
    // texture that crossed the seed line, not an edit (the real "sky bluer"
    // answer left six of them on the ground). Anything an edit can be asked
    // to remove spans more cells than that at ~1/96 of the short side.
    let mut seen = vec![false; n];
    for start in 0..n {
        if !on[start] || seen[start] {
            continue;
        }
        let mut comp = vec![start];
        seen[start] = true;
        let mut k = 0;
        while k < comp.len() {
            let (x, y) = (comp[k] % gw, comp[k] / gw);
            for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
                if nx < gw && ny < gh {
                    let j = ny * gw + nx;
                    if on[j] && !seen[j] {
                        seen[j] = true;
                        comp.push(j);
                    }
                }
            }
            k += 1;
        }
        if comp.len() <= 2 {
            comp.iter().for_each(|&i| on[i] = false);
        }
    }
    // Holes: every off cell the frame's border cannot reach through off cells.
    let mut outside = vec![false; n];
    let mut stack: Vec<usize> = (0..n)
        .filter(|&i| {
            let (x, y) = (i % gw, i / gw);
            !on[i] && (x == 0 || y == 0 || x + 1 == gw || y + 1 == gh)
        })
        .collect();
    stack.iter().for_each(|&i| outside[i] = true);
    while let Some(i) = stack.pop() {
        let (x, y) = (i % gw, i / gw);
        for (nx, ny) in [(x.wrapping_sub(1), y), (x + 1, y), (x, y.wrapping_sub(1)), (x, y + 1)] {
            if nx < gw && ny < gh {
                let j = ny * gw + nx;
                if !on[j] && !outside[j] {
                    outside[j] = true;
                    stack.push(j);
                }
            }
        }
    }
    // One cell of growth, so the feather starts outside what changed.
    let grown: Vec<bool> = (0..n)
        .map(|i| {
            let (x, y) = (i % gw, i / gw);
            let near = |dx: isize, dy: isize| {
                let (nx, ny) = (x as isize + dx, y as isize + dy);
                nx >= 0 && ny >= 0 && (nx as usize) < gw && (ny as usize) < gh && {
                    let j = ny as usize * gw + nx as usize;
                    on[j] || !outside[j]
                }
            };
            (-1..=1).any(|dy| (-1..=1).any(|dx| near(dx, dy)))
        })
        .collect();
    let take = (0..w * h).map(|p| if grown[((p / w) / cell) * gw + (p % w) / cell] { 1.0 } else { 0.0 }).collect();
    Region { w, h, take }
}

/// The frame `reimagine` puts on the wire for `path`: a RAW's sensor frame
/// ([`decode::frame_size`] — DefaultCropSize in display orientation, what the
/// full develop renders), a baked image's own pixels. The size plan is drawn
/// from THIS frame and from nothing else, because the input is resized to the
/// plan exactly: sizing from a RAW's embedded preview while sending its
/// develop stretched the develop whenever the body had written the preview
/// at an in-camera aspect crop.
fn sent_frame(path: &Path, preview: (u32, u32)) -> Result<(u32, u32)> {
    if !decode::is_raw(path) {
        return Ok(preview);
    }
    let (w, h) = decode::frame_size(path)?;
    Ok((w as u32, h as u32))
}

/// Whether the full develop, not the embedded preview, is the input: when the
/// planned size outresolves the preview, and whenever the preview is not the
/// sensor frame (an in-camera aspect crop) — resizing a 4:3 preview onto a
/// 3:2 plan would stretch it exactly as sizing from it stretched the develop.
fn develop_is_sent(raw: bool, planned: (u32, u32), preview: (u32, u32), frame: (u32, u32)) -> bool {
    if !raw {
        return false;
    }
    let outresolved = planned.0.max(planned.1) > preview.0.max(preview.1);
    outresolved || !crate::fit::same_frame_plausible_dims(preview, frame)
}

/// The output-size request strategy: try the flexible high-res size first (when
/// the budget allows one), fall back to the universally-supported enum size when
/// the model 400s it. Carrying both here keeps the retry logic in
/// [`call_images_edit`] mechanical.
struct SizePlan {
    /// Flexible WIDTHxHEIGHT (gpt-image-2-style), when one fits the budget.
    flexible: Option<String>,
    /// The fixed enum size every gpt-image model accepts.
    enum_size: &'static str,
}

impl SizePlan {
    fn for_source(cfg: &Config, w: u32, h: u32) -> Self {
        Self::for_budget(w, h, cfg.openai_image_max_px)
    }

    fn for_budget(w: u32, h: u32, max_px: u32) -> Self {
        Self { flexible: flex_size(w, h, max_px), enum_size: pick_size(w, h) }
    }

    /// The size to request on the first attempt.
    fn try_first(&self) -> &str {
        self.flexible.as_deref().unwrap_or(self.enum_size)
    }
}

/// Largest flexible output size matching the source aspect, under the documented
/// gpt-image-2 constraints (verified 2026-07 API docs): both edges multiples of
/// 16, long edge ≤ 3840, long:short ratio ≤ 3:1, total pixels within
/// [655 360, 8 294 400] — further capped by the user's `max_px` budget. `None`
/// when no size satisfies all of that (caller uses the enum size).
fn flex_size(w: u32, h: u32, max_px: u32) -> Option<String> {
    const API_MIN_PX: f64 = 655_360.0;
    const API_MAX_PX: f64 = 8_294_400.0;
    const MAX_EDGE: f64 = 3840.0;
    if w == 0 || h == 0 {
        return None;
    }
    let budget = (max_px as f64).min(API_MAX_PX);
    if budget < API_MIN_PX {
        return None;
    }
    let r = (w as f64 / h as f64).clamp(1.0 / 3.0, 3.0);
    // Largest (ow, oh) with ow/oh = r and ow·oh = budget, then the edge cap.
    let mut oh = (budget / r).sqrt();
    let mut ow = r * oh;
    let scale = (MAX_EDGE / ow.max(oh)).min(1.0);
    ow *= scale;
    oh *= scale;
    // Round DOWN to ×16 — keeps every ≤ constraint satisfied except the
    // ratio: floored on its own, each edge loses up to 15 px, and at 3:1 the
    // short edge can lose proportionally more (3000×1000 floors to 2992×992,
    // which is 3.016:1, and the API answers such a size with a 400 after a
    // wasted round trip). So the long edge is capped at three times the
    // floored short edge afterwards — a multiple of 16 whenever the short
    // edge is one — which touches only the sizes that broke the bound and
    // can only shrink, so the edge cap and the pixel budget still hold.
    let ow = ((ow / 16.0).floor() * 16.0) as u32;
    let oh = ((oh / 16.0).floor() * 16.0) as u32;
    let (long, short) = (ow.max(oh).min(3 * ow.min(oh)), ow.min(oh));
    if short == 0 || long == 0 || (long as f64) * (short as f64) < API_MIN_PX {
        return None;
    }
    let (ow, oh) = if ow >= oh { (long, short) } else { (short, long) };
    Some(format!("{ow}x{oh}"))
}

/// Pick the supported gpt-image output size whose aspect best matches the source,
/// so we stop squashing every photo into a 1:1 square. Every gpt-image model
/// accepts exactly 1024×1024, 1536×1024 (landscape 3:2) and 1024×1536 (portrait
/// 2:3); newer models additionally take arbitrary sizes (see [`flex_size`]).
fn pick_size(w: u32, h: u32) -> &'static str {
    if h == 0 {
        return "1024x1024";
    }
    let r = w as f32 / h as f32;
    if r >= 1.2 {
        "1536x1024"
    } else if r <= 0.833 {
        "1024x1536"
    } else {
        "1024x1024"
    }
}

fn parse_size(s: &str) -> (u32, u32) {
    s.split_once('x')
        .and_then(|(a, b)| Some((a.parse().ok()?, b.parse().ok()?)))
        .unwrap_or((1024, 1024))
}


/// Feather radius from the painted SELECTION's bounding box — an eighth of
/// its short side, at least 1 (see the weight-plane construction in
/// [`retouch`]). An empty selection returns usize::MAX so the caller's frame
/// rule stays the effective bound.
fn selection_feather(weight: &[f32], w: usize) -> usize {
    let (mut x0, mut y0, mut x1, mut y1) = (usize::MAX, usize::MAX, 0usize, 0usize);
    for (i, &v) in weight.iter().enumerate() {
        if v > 0.0 {
            let (x, y) = (i % w, i / w);
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
    }
    if x0 == usize::MAX {
        return usize::MAX;
    }
    let short = (x1 - x0 + 1).min(y1 - y0 + 1);
    (short / 8).max(1)
}

/// The blend pass over an owned base copy. Each pixel is written at most once,
/// so reading back from `out` is exact — no separate source plane needed (the
/// memory-staged full-res path in [`retouch`] rides on that).
fn composite_in_place(out: &mut RgbaImage, gen_img: &RgbImage, weight: &[f32]) {
    let (w, h) = out.dimensions();
    let wu = w as usize;
    for y in 0..h {
        for x in 0..w {
            let a = weight[(y as usize) * wu + x as usize].clamp(0.0, 1.0);
            if a <= 0.0001 {
                continue; // outside the (feathered) mask → keep the full-res original
            }
            let b = *out.get_pixel(x, y);
            let g = gen_img.get_pixel(x, y);
            // PREMULTIPLIED source-over (L09-5 + review R12-09): blending
            // straight RGB and alpha independently made a 1%-weight white
            // edge over a TRANSPARENT base come out (3,3,3,3) — nearly
            // black once composited downstream; the correct straight
            // result is (255,255,255,3). Generated content is opaque, so
            // the source term needs no unpremultiply, and out_a >= a > 0
            // here, so the division is safe. An OPAQUE base (ab = 1)
            // reduces to the old linear mix exactly — no rendering change
            // on normal photos.
            let ab = b[3] as f32 / 255.0;
            let out_a = a + (1.0 - a) * ab;
            let mix = |bc: u8, gc: u8| {
                let premul = a * gc as f32 + (1.0 - a) * bc as f32 * ab;
                (premul / out_a).round().clamp(0.0, 255.0) as u8
            };
            out.put_pixel(
                x,
                y,
                Rgba([
                    mix(b[0], g[0]),
                    mix(b[1], g[1]),
                    mix(b[2], g[2]),
                    (out_a * 255.0).round().clamp(0.0, 255.0) as u8,
                ]),
            );
        }
    }
}

/// Separable box blur with prefix sums — cost is O(w·h), independent of
/// `radius`, so a wide feather on a full-res frame stays cheap. Runs entirely
/// IN PLACE on the caller's plane: the only scratch is one row prefix and one
/// column prefix, so a 61 MP weight field no longer pays a second ~241 MB
/// plane (L02). Both passes are safe in place, and neither changes a value:
///   · horizontal — a row's whole prefix is read before that row is written,
///     and no other row is touched;
///   · vertical — a column's whole prefix is read before that column is
///     written, in the same y=0-upward accumulation order as before.
fn box_blur(mut src: Vec<f32>, w: usize, h: usize, radius: usize) -> Vec<f32> {
    if radius == 0 || w == 0 || h == 0 {
        return src;
    }
    let mut prefix = vec![0.0f32; w + 1];
    for y in 0..h {
        let row = y * w;
        for x in 0..w {
            prefix[x + 1] = prefix[x] + src[row + x];
        }
        for x in 0..w {
            let lo = x.saturating_sub(radius);
            let hi = (x + radius + 1).min(w);
            src[row + x] = (prefix[hi] - prefix[lo]) / (hi - lo) as f32;
        }
    }
    let mut col = vec![0.0f32; h + 1];
    for x in 0..w {
        for y in 0..h {
            col[y + 1] = col[y] + src[y * w + x];
        }
        for y in 0..h {
            let lo = y.saturating_sub(radius);
            let hi = (y + radius + 1).min(h);
            src[y * w + x] = (col[hi] - col[lo]) / (hi - lo) as f32;
        }
    }
    src
}

pub(crate) fn encode_png(img: &DynamicImage) -> Result<Vec<u8>> {
    let mut buf = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut buf), image::ImageFormat::Png)
        .context("encode png")?;
    Ok(buf)
}

/// Is `declared` this endpoint's CAP on `expected` rather than a different
/// picture?
///
/// A subscription-tier endpoint (the local Codex bridge the GUI's OAuth mode
/// installs) honours the requested aspect and clamps the pixel count: 3520x2352
/// comes back as 1534x1025 — 1.4966:1 against 1.4966:1, measured 2026-08-30.
/// Smaller in BOTH axes at the same aspect is a resolution loss, which is what
/// that mode documents; anything else (a different aspect, a LARGER frame) is a
/// different picture and still refused, because the caller writes these bytes
/// out as the master.
const MIN_CAPPED_LONG_EDGE: u32 = 1024;

fn size_is_an_endpoint_cap(declared: (u32, u32), expected: (u32, u32)) -> bool {
    if declared.0 == 0 || declared.1 == 0 || expected.1 == 0 {
        return false;
    }
    if declared.0 >= expected.0 || declared.1 >= expected.1 {
        return false;
    }
    // A cap still has to be a usable picture. Without this floor the rule
    // admits ANY smaller frame at the same aspect — the existing contract test
    // caught it by handing 1x1 for a requested 1024x1024, which is not a tier
    // limit but a broken response. 1024 is the smallest edge the image models
    // document, so a genuine cap never falls below it.
    if declared.0.max(declared.1) < MIN_CAPPED_LONG_EDGE {
        return false;
    }
    let (a, b) = (
        f64::from(declared.0) / f64::from(declared.1),
        f64::from(expected.0) / f64::from(expected.1),
    );
    // Half a percent: enough for the rounding an integer clamp introduces
    // (1534/1025 vs 3520/2352 differ by 0.02%), far too tight to admit a
    // neighbouring aspect (3:2 vs 4:3 differ by 12%).
    (a - b).abs() <= b * 0.005
}

/// Returns the canonical PNG and the dimensions it ACTUALLY carries, which may
/// be an endpoint cap below `expected_size` (see [`size_is_an_endpoint_cap`]).
fn canonical_generated_png(bytes: &[u8], expected_size: &str) -> Result<(Vec<u8>, (u32, u32))> {
    let format = image::guess_format(bytes).context("identify generated image")?;
    if format != image::ImageFormat::Png {
        return Err(anyhow!(
            "image API returned {format:?}; the generated-image contract requires PNG"
        ));
    }
    let expected = parse_size(expected_size);
    // Dimensions from the HEADER, before any pixel decode: a tiny hostile
    // PNG can declare a ~480-megapixel frame, and fully decoding it just to
    // reject the mismatch cost hundreds of MiB per response (16-lane scan
    // L02/L16). The full decode below then runs only at the agreed size.
    let declared = image::ImageReader::with_format(
        std::io::Cursor::new(bytes),
        image::ImageFormat::Png,
    )
    .into_dimensions()
    .context("read generated PNG header")?;
    if declared != expected && !size_is_an_endpoint_cap(declared, expected) {
        return Err(anyhow!(
            "image API returned {}x{} for requested {}x{}",
            declared.0,
            declared.1,
            expected.0,
            expected.1
        ));
    }
    let accepted = declared;
    let image = image::load_from_memory_with_format(bytes, image::ImageFormat::Png)
        .context("fully decode generated PNG")?;
    // The header is not authority over the pixels: re-check the DECODED size
    // against what the header declared, so a frame that lies about itself is
    // refused even when the declared value passed the cap test above.
    if image.dimensions() != accepted {
        return Err(anyhow!(
            "image API returned {}x{} for requested {}x{}",
            image.width(),
            image.height(),
            expected.0,
            expected.1
        ));
    }
    if !matches!(
        &image,
        DynamicImage::ImageRgb8(_) | DynamicImage::ImageRgba8(_)
    ) {
        return Err(anyhow!(
            "image API returned {:?}; generated output must be 8-bit RGB or RGBA",
            image.color()
        ));
    }
    Ok((encode_png(&image)?, accepted))
}







/// POST /images/edits, negotiating capability drift instead of hard-coding a
/// model list. Three request features are droppable, each at most once, on the
/// API's own 400 for *that specific parameter*:
///   * STREAMING (`stream` + `partial_images`) — the liveness channel: partial
///     frames keep proving the server is working, so the call runs under an
///     INACTIVITY deadline ([`IMAGES_EDIT_STALL_SECS`]) and a healthy long
///     generation is never killed mid-run. Models/bridges without image
///     streaming reject the parameter; the retry falls back to one blocking
///     POST under the legacy overall deadline.
///   * `input_fidelity` — a gpt-image-1.x knob; newer models (gpt-image-2)
///     reject it (`invalid_input_fidelity_model`).
///   * the FLEXIBLE `size` — a gpt-image-2 capability; older models reject a
///     non-enum size, so we retry with the fixed enum size from the [`SizePlan`].
///
/// Returns the image bytes and BOTH sizes (see [`GeneratedImage`]). `inputs` builds
/// the image (+ optional mask) PNG pair FOR a requested size string — called
/// once per size the negotiation attempts (memoize in the provider if
/// rebuilding is expensive).
type SizedInputs = (Vec<u8>, Option<Vec<u8>>);

/// One billed images/edits call's result.
///
/// The two sizes are DIFFERENT facts and are named apart because they were once
/// a single string, which is how a capped generation came to be measured against
/// pixels it was never sent:
///   * `requested` — the size the negotiation settled on. It selects which input
///     PNG went up the wire, so anything comparing the result to its INPUT must
///     use this one.
///   * `accepted` — the size the endpoint actually returned, which a
///     subscription tier can cap below the request. This is the size of the file
///     on disk, so anything reporting the result uses this one.
///
/// They are equal in the ordinary case.
struct GeneratedImage {
    bytes: Vec<u8>,
    requested: String,
    accepted: String,
}
















/// The shared fill/adjust model call: the local ComfyUI backend (S5.11). The
/// provider closure has already sized the base (and mask, when painted); the
/// returned PNG is the whole edited frame, and the caller composites the kept
/// pixels (painted mask, found region, or the whole frame).
fn call_images_edit(
    _cfg: &Config,
    inputs: &mut dyn FnMut(&str) -> Result<SizedInputs>,
    prompt: &str,
    _fidelity: &str,
    sizes: &SizePlan,
    quality: &str,
) -> Result<GeneratedImage> {
    let size = sizes.try_first();
    let (image_png, mask_png) = inputs(size)?;
    let bytes = crate::comfy::edit_png(&image_png, mask_png.as_deref(), prompt, quality)?;
    let (w, h) = image::load_from_memory(&bytes)
        .context("decode the ComfyUI edit result")?
        .dimensions();
    Ok(GeneratedImage {
        bytes,
        requested: size.to_string(),
        accepted: format!("{w}x{h}"),
    })
}
