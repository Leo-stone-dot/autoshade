//! Cross-image correspondence bridge — Rust side of `python/correspond.py`.
//!
//! Fourth sidecar, same shell-out pattern as [`crate::denoise`],
//! [`crate::segment`] and [`crate::embed`]: a local Python process runs the
//! DIFT featurizer (Stable Diffusion 2.1, one UNet pass per noise draw) over
//! TWO renditions of the same frame and writes a JSON *correspondence field* —
//! for every cell of a 48×48 grid over the source, where that content sits in
//! the target and how much the match can be trusted. The weights auto-download
//! to `python/weights` on first run (~2.6 GB, digest pinned) — nothing is
//! stored in this repo.
//!
//! **Why it exists (plan step 7).** The reverse-fit's atmosphere mode fires
//! when the pair's CONTENT diverges (structure divergence `D ≥ 0.35`) — a
//! generated target that replaced the sky or invented a cloud deck. Today that
//! mode compares grid cells at the SAME coordinates, which is exactly what a
//! content-divergent pair breaks. This field is the missing input: which cells
//! still correspond (and where), and which have no counterpart at all. Step 7a
//! ships the instrument + a CLI diagnostic door; the estimator wiring is 7b.
//!
//! **It never generates pixels** — the output is coordinates and confidences.
//! And like the embedding, it is ADDITIVE: absence of the sidecar degrades to
//! today's behaviour, it never turns a working fit off.
//!
//! The run itself is [`crate::run_model_sidecar`] — the shared
//! spawn/bound/exit-0-is-not-success executor — under the process-wide
//! [`crate::with_model_slot`], so this model and the style embedding are
//! never resident together.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::Config;

/// The feature-grid edge this build expects — `python/correspond.py`'s
/// `GRID`, which is 768 px input / 16 (the UNet `up_blocks[1]` stride). Not a
/// preference: [`parse_field`] refuses any other size, because a checkpoint
/// answering a different grid would silently change which pixels every cell
/// of a saved field refers to.
pub const GRID: usize = 48;

/// Everything one correspondence run needs; built from [`Config`] like
/// [`crate::embed::EmbedOpts`].
pub struct CorrespondOpts {
    pub python_bin: String,
    pub script: PathBuf,
}

impl CorrespondOpts {
    pub fn from_config(cfg: &Config) -> Self {
        CorrespondOpts {
            python_bin: cfg.sidecar_python(),
            script: PathBuf::from(&cfg.correspond_script),
        }
    }

    /// Is the sidecar even present? The fit will ask BEFORE staging pixels,
    /// so a machine without it degrades once, with a sentence, instead of
    /// erroring per pair.
    pub fn available(&self) -> bool {
        crate::sidecar_script_present(&self.script)
    }
}

/// The sidecar's argv, as a pure function of what decides it — pinned by a
/// test, like [`crate::embed`]'s. No precision flag: the sidecar picks fp16
/// on CUDA and fp32 on CPU by itself (the field is coordinates, not pixels,
/// and its gates are scale-free — there is no fp32 escape hatch to preserve).
fn sidecar_args(script: &Path, source: &Path, target: &Path, output: &Path) -> Vec<std::ffi::OsString> {
    vec![
        // `-E`: the second layer against a PYTHON* import hijack (the env
        // allowlist in `dotenv_child_env` is the first).
        "-E".into(),
        script.into(),
        "--source".into(),
        source.into(),
        "--target".into(),
        target.into(),
        "--output".into(),
        output.into(),
    ]
}

/// One correspondence field, parsed and gated. Flat arrays over source cells
/// in row-major order (`cell = y * grid_w + x`); `map_x`/`map_y` are TARGET
/// grid coordinates, `confidence` is the sidecar's cyclic×smoothness product
/// in [0, 1].
#[derive(Debug)]
pub struct CorrespondenceField {
    pub model: String,
    pub revision: String,
    pub grid_w: usize,
    pub grid_h: usize,
    pub input_size: u32,
    pub map_x: Vec<f32>,
    pub map_y: Vec<f32>,
    pub confidence: Vec<f32>,
}

impl CorrespondenceField {
    /// Share of source cells whose match clears `threshold` — the one-number
    /// summary the diagnostic prints and 7b's coverage reasoning will read.
    pub fn coverage(&self, threshold: f32) -> f32 {
        if self.confidence.is_empty() {
            return 0.0;
        }
        self.confidence.iter().filter(|&&c| c >= threshold).count() as f32
            / self.confidence.len() as f32
    }
}

/// Run the sidecar on ONE pair and return the parsed field.
///
/// `output` is the JSON file the sidecar writes — and unlike the embedding's
/// scratch it is NOT deleted on success: for the CLI diagnostic the field
/// file IS the deliverable (the caller names it), and a fit caller that
/// wants it gone owns that lifetime. An UNPARSEABLE field is discarded,
/// though — a file that failed its own gates must not be left behind looking
/// like a result.
pub fn correspond_file(
    opts: &CorrespondOpts,
    source: &Path,
    target: &Path,
    output: &Path,
) -> Result<CorrespondenceField> {
    if !opts.script.exists() {
        bail!(
            "correspondence sidecar not found at {} — run from the project dir or set \
             AUTOSHADE_CORRESPOND_SCRIPT.",
            opts.script.display()
        );
    }
    let before = crate::artifact_state(output);
    let text = crate::run_model_sidecar(
        "correspondence sidecar",
        &opts.python_bin,
        sidecar_args(&opts.script, source, target, output),
        output,
    )?;
    let field = parse_field(&text).with_context(|| {
        format!("correspondence sidecar wrote an unusable field at {}", output.display())
    });
    if field.is_err() {
        crate::denoise::discard_failed_output(output, before);
    }
    field
}

/// The three FRESH temp names one [`fit_provider`] call stages under: the
/// source frame, the target frame and the field.
///
/// pid + the process-wide `store::next_tmp_seq`, the shape every other tmp
/// minter in this tree draws from. pid alone was one name per PROCESS, so any
/// two fits in one process — a job pool is enough — would have staged over
/// each other's frames and deleted each other's field mid-run, and the
/// symptom would have been one pair's field computed on another pair's
/// pixels. Latent today, like `segment::staging_path`'s sequence, which is
/// precisely why it is named here and not in an incident report.
fn fit_stage_paths(dir: &Path) -> [PathBuf; 3] {
    let tag = format!("{}-{}", std::process::id(), crate::store::next_tmp_seq());
    [
        dir.join(format!("autoshade-corr-fit-src-{tag}.png")),
        dir.join(format!("autoshade-corr-fit-tgt-{tag}.png")),
        dir.join(format!("autoshade-corr-fit-field-{tag}.json")),
    ]
}

/// The fit's correspondence provider (step 7b), shared by the CLI and the
/// GUI: stages both renditions as bounded PNGs the sidecar can read, runs
/// it, and hands back the parsed field. Everything staged — including the
/// field JSON — is an intermediate here and is removed; the fit's report
/// carries the derived per-pair data instead. Failures return `Err` and the
/// fit discloses the reason (`FIT_CORRESPONDENCE_UNAVAILABLE`); by the
/// user's ruling (2026-08-26, 完全自动) this runs unconditionally on
/// content-divergent pairs, first-run weight download included.
pub fn fit_provider(
    opts: CorrespondOpts,
) -> impl Fn(&image::DynamicImage, &image::DynamicImage) -> Result<CorrespondenceField> {
    move |src, target| {
        let [s, t, out] = fit_stage_paths(&std::env::temp_dir());
        // 1024 px is comfortably above the sidecar's own 768² working size;
        // staging the full frame would spend more on the PNG encode than on
        // the model.
        let stage = |img: &image::DynamicImage, p: &std::path::Path| -> Result<()> {
            crate::render::thumbnail(img, 1024, 1024)
                .save(p)
                .with_context(|| format!("stage {} for the correspondence sidecar", p.display()))
        };
        let field = stage(src, &s)
            .and_then(|()| stage(target, &t))
            .and_then(|()| correspond_file(&opts, &s, &t, &out));
        for p in [&s, &t, &out] {
            let _ = std::fs::remove_file(p);
        }
        field
    }
}


/// One sidecar JSON record → the field, with every invariant a consumer would
/// otherwise have to re-derive. Refuses, rather than repairs: a record that
/// carries an `error`, a grid other than [`GRID`]², an array of the wrong
/// length, a coordinate outside the target grid, a confidence outside [0, 1],
/// and any non-finite element. The coordinate gate is the load-bearing one —
/// 7b will INDEX target rasters with these values, and an out-of-grid entry
/// would be an out-of-bounds read waiting in a saved-recipe path.
pub fn parse_field(text: &str) -> Result<CorrespondenceField> {
    let rec: serde_json::Value =
        serde_json::from_str(text.trim()).context("correspondence output is not JSON")?;
    if let Some(e) = rec.get("error").and_then(|v| v.as_str()) {
        bail!("correspondence sidecar declined this pair: {e}");
    }
    let usize_of = |k: &str| -> Result<usize> {
        rec.get(k)
            .and_then(|v| v.as_u64())
            .with_context(|| format!("correspondence output has no `{k}`"))
            .map(|v| v as usize)
    };
    let grid_w = usize_of("grid_w")?;
    let grid_h = usize_of("grid_h")?;
    if grid_w != GRID || grid_h != GRID {
        bail!(
            "correspondence sidecar answered a {grid_w}x{grid_h} grid; this build reads \
             {GRID}x{GRID} fields, and mixing grids changes which pixels every cell means"
        );
    }
    let cells = grid_w * grid_h;
    let arr = |k: &str| -> Result<Vec<f32>> {
        let a = rec
            .get(k)
            .and_then(|v| v.as_array())
            .with_context(|| format!("correspondence output has no `{k}` array"))?;
        if a.len() != cells {
            bail!("`{k}` holds {} entries for a {cells}-cell grid", a.len());
        }
        let mut v = Vec::with_capacity(cells);
        for x in a {
            let f = x.as_f64().with_context(|| format!("`{k}` holds a non-number"))? as f32;
            if !f.is_finite() {
                bail!("`{k}` holds a non-finite element");
            }
            v.push(f);
        }
        Ok(v)
    };
    let map_x = arr("map_x")?;
    let map_y = arr("map_y")?;
    let confidence = arr("confidence")?;
    let sim = arr("sim")?;
    for (name, v, hi) in [("map_x", &map_x, grid_w as f32), ("map_y", &map_y, grid_h as f32)] {
        if v.iter().any(|&c| !(0.0..hi).contains(&c)) {
            bail!("`{name}` holds a coordinate outside the {grid_w}x{grid_h} target grid");
        }
    }
    if confidence.iter().any(|&c| !(0.0..=1.0).contains(&c)) {
        bail!("`confidence` holds a value outside [0, 1]");
    }
    // Raw cosine of two unit vectors; 1e-3 of slack for the f32 round-trip.
    if sim.iter().any(|&s| !(-1.001..=1.001).contains(&s)) {
        bail!("`sim` holds a value no cosine can reach");
    }
    let str_of = |k: &str| -> Result<String> {
        rec.get(k)
            .and_then(|v| v.as_str())
            .with_context(|| format!("correspondence output names no `{k}`"))
            .map(str::to_string)
    };
    Ok(CorrespondenceField {
        model: str_of("model")?,
        revision: str_of("revision")?,
        grid_w,
        grid_h,
        input_size: rec
            .get("input_size")
            .and_then(|v| v.as_u64())
            .context("correspondence output has no `input_size`")? as u32,
        map_x,
        map_y,
        confidence,
    })
}
