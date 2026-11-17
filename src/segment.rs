//! AI segmentation bridge — Rust side of the sidecar (`python/segment.py`).
//!
//! Same shell-out pattern as [`crate::denoise`] (SCUNet): a local Python
//! process does the model inference and writes an 8-bit grayscale mask PNG
//! (white = selected, soft edges = the model's own alpha), which the app then
//! attaches to the recipe as a [`crate::recipe::MaskGeometry::Bitmap`] local
//! adjustment. The AI picks *where*; every actual edit stays a deterministic
//! recipe slider. Models auto-download on first run — into `python/weights`
//! (BiRefNet, OneFormer, SAM 2.1, each behind a sha256 gate) or `~/.u2net` for
//! the subject fallback tier, which gates its own — no weights in the repo.

use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use image::GrayImage;

use crate::config::Config;

/// Everything one segmentation run needs; built from [`Config`] like
/// [`crate::denoise::DenoiseOpts`].
pub struct SegmentOpts {
    pub python_bin: String,
    pub script: PathBuf,
    /// `"subject"` (BiRefNet, with U²-Net as the fallback tier), `"sky"`
    /// (OneFormer ADE20K Swin-L), or `"object"` (SAM 2.1). All backends are
    /// licence-pinned on the
    /// Python side — see `python/segment.py`'s module docstring: the sky model
    /// was SegFormer-B0 until R27 Batch-4, whose weights are licensed for
    /// "research or evaluation purposes only", and the subject fallback names
    /// its rembg session explicitly so an upstream default change cannot swap a
    /// pay-to-use model into a public build.
    ///
    /// **`"subject"` has TWO possible backends and the sidecar says which one
    /// ran** (R29 B4): the general BiRefNet checkpoint, or U²-Net when
    /// torchvision/timm/einops are missing or the pinned weights do not verify.
    /// The sidecar prints the backend on stdout and warns on stderr when it
    /// degraded; [`segment_file`] forwards that stderr on the success path so
    /// the two states cannot be read as one.
    pub target: String,
    /// `crs:ReferencePoint` as the sidecar spells it — two space-separated
    /// normalised floats. REQUIRED by `--target object` and ignored by the
    /// other two. A gesture-free object run sends exactly this legacy argv.
    pub reference_point: Option<String>,
    /// The complete positive SAM prompt, ReferencePoint first, followed by
    /// every ordered `d x y` from a subtype-0 gesture. `None` preserves the
    /// legacy process contract; `Some` is staged as a bounded JSON sidecar.
    pub prompt_points: Option<Vec<[f32; 2]>>,
}

/// One validated plane from a multi-class semantic segmentation manifest.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiClassPlane {
    pub class_id: u16,
    pub label: String,
    pub mean_confidence: f32,
    pub share: f32,
    pub path: PathBuf,
    pub mask: GrayImage,
}

/// Validated output of one OneFormer multi-class inference.
#[derive(Debug, Clone, PartialEq)]
pub struct MultiClassManifest {
    pub width: u32,
    pub height: u32,
    pub planes: Vec<MultiClassPlane>,
}

/// Real manifests are a few hundred bytes.  Keep malformed or hostile output
/// from allocating an unbounded buffer before deserialization.
pub const MULTI_MANIFEST_MAX_BYTES: u64 = 64 * 1024;

pub fn read_multi_manifest(path: &Path) -> Result<MultiClassManifest> {
    let len = std::fs::metadata(path)
        .with_context(|| format!("stat semantic manifest {}", path.display()))?
        .len();
    if len > MULTI_MANIFEST_MAX_BYTES {
        bail!("semantic manifest is too large ({} bytes; cap {} bytes)", len, MULTI_MANIFEST_MAX_BYTES);
    }
    let file = std::fs::File::open(path)
        .with_context(|| format!("open semantic manifest {}", path.display()))?;
    let mut bytes = Vec::with_capacity(len as usize);
    file.take(MULTI_MANIFEST_MAX_BYTES + 1).read_to_end(&mut bytes)
        .with_context(|| format!("read semantic manifest {}", path.display()))?;
    if bytes.len() as u64 > MULTI_MANIFEST_MAX_BYTES {
        bail!("semantic manifest exceeded {} bytes", MULTI_MANIFEST_MAX_BYTES);
    }
    let text = std::str::from_utf8(&bytes).context("semantic manifest is not UTF-8")?;
    parse_multi_manifest(text, path.parent().unwrap_or_else(|| Path::new(".")))
}

/// Parse and validate the small JSON manifest emitted by `segment.py --multi`.
/// Plane files are resolved relative to `base_dir`; malformed metadata or any
/// bad raster rejects the complete semantic layer.
pub fn parse_multi_manifest(text: &str, base_dir: &Path) -> Result<MultiClassManifest> {
    #[derive(serde::Deserialize)]
    struct RawManifest { version: u32, width: u32, height: u32, planes: Vec<RawPlane> }
    #[derive(serde::Deserialize)]
    struct RawPlane { class_id: u16, label: String, mean_confidence: f32, share: f32, path: String }
    let raw: RawManifest = serde_json::from_str(text.trim()).context("semantic manifest is not JSON")?;
    if raw.version != 1 { bail!("semantic manifest version {} is unsupported", raw.version); }
    if raw.width == 0 || raw.height == 0 || raw.planes.is_empty()
        || raw.planes.len() > crate::fit_zoned::semantic::MAX_SEMANTIC_REGIONS
    {
        bail!("semantic manifest has invalid dimensions or plane count");
    }
    let mut planes = Vec::with_capacity(raw.planes.len());
    let mut ids = std::collections::HashSet::new();
    for p in raw.planes {
        if !ids.insert(p.class_id) || p.label.len() > 256 || !p.mean_confidence.is_finite()
            || !p.share.is_finite() || !(0.0..=1.0).contains(&p.mean_confidence)
            || !(0.0..=1.0).contains(&p.share) || p.path.is_empty()
        { bail!("semantic manifest contains an invalid plane"); }
        let relative = Path::new(&p.path);
        if relative.components().count() != 1
            || !matches!(relative.components().next(), Some(std::path::Component::Normal(_)))
        {
            bail!("semantic manifest plane path must be one relative file name");
        }
        let path = base_dir.join(relative);
        let metadata = std::fs::symlink_metadata(&path)
            .with_context(|| format!("stat semantic plane {}", path.display()))?;
        if !metadata.file_type().is_file() {
            bail!("semantic plane {} is not a regular file", p.class_id);
        }
        let mask = crate::render::open_mask_bounded(&path)
            .with_context(|| format!("read semantic plane {}", path.display()))?.to_luma8();
        if mask.dimensions() != (raw.width, raw.height) {
            bail!("semantic plane {} has size {:?}, expected {}x{}", p.class_id, mask.dimensions(), raw.width, raw.height);
        }
        // PNG masks are u8 by construction; this explicit walk keeps the
        // bridge contract honest if another decoder is introduced later.
        if mask.pixels().any(|v| !((v.0[0] as f32 / 255.0).is_finite())) {
            bail!("semantic plane {} contains a non-finite value", p.class_id);
        }
        planes.push(MultiClassPlane { class_id: p.class_id, label: p.label, mean_confidence: p.mean_confidence, share: p.share, path, mask });
    }
    planes.sort_by_key(|p| p.class_id);
    Ok(MultiClassManifest { width: raw.width, height: raw.height, planes })
}

/// Run one OneFormer inference and load its multi-plane manifest.  The sidecar
/// writes all plane rasters beside `manifest`; a malformed manifest or plane
/// makes the entire semantic attempt fail-open to the caller.
pub fn segment_multiclass_file(
    opts: &SegmentOpts,
    input: &Path,
    manifest: &Path,
    max_regions: usize,
) -> Result<MultiClassManifest> {
    if opts.target != "sky" {
        bail!("multi-class segmentation requires the sky OneFormer target");
    }
    // The Python side keeps its own 1..4 CLI validation; Rust owns the cap.
    let max_regions = max_regions.clamp(1, crate::fit_zoned::semantic::MAX_SEMANTIC_REGIONS);
    let sidecar = crate::run_model_sidecar_bounded(
        "multi-class segmentation sidecar",
        &opts.python_bin,
        vec![
            "-E".into(), opts.script.clone().into(),
            "--input".into(), input.into(), "--output".into(), manifest.into(),
            "--target".into(), "sky".into(), "--multi".into(),
            "--regions".into(), max_regions.to_string().into(),
        ],
        manifest,
        Some(MULTI_MANIFEST_MAX_BYTES),
    );
    let parsed = sidecar.and_then(|_| read_multi_manifest(manifest));
    cleanup_manifest_planes(manifest);
    parsed
}

/// Remove everything the sidecar wrote beside `manifest` for this call: the
/// class planes (`<stem>.class-<id>.png`) and any atomic-write temporaries a
/// killed sidecar left behind (`<manifest>.<pid>.tmp.json`,
/// `<stem>.class-<id>.png.<pid>.tmp.png`). Only regular files under the
/// manifest's own directory, never through a link.
fn cleanup_manifest_planes(manifest: &Path) {
    let Some(parent) = manifest.parent() else { return };
    let Some(stem) = manifest.file_stem().and_then(|s| s.to_str()) else { return };
    let Some(name) = manifest.file_name().and_then(|s| s.to_str()) else { return };
    let plane_prefix = format!("{stem}.class-");
    let tmp_prefix = format!("{name}.");
    let Ok(entries) = std::fs::read_dir(parent) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        let ours = path
            .file_name()
            .and_then(|s| s.to_str())
            .is_some_and(|s| s.starts_with(&plane_prefix) || s.starts_with(&tmp_prefix));
        let regular = std::fs::symlink_metadata(&path).is_ok_and(|m| m.file_type().is_file());
        if ours && regular {
            let _ = std::fs::remove_file(path);
        }
    }
}

impl SegmentOpts {
    pub fn from_config(cfg: &Config, target: &str) -> Self {
        SegmentOpts {
            python_bin: cfg.sidecar_python(),
            script: PathBuf::from(&cfg.segment_script),
            target: target.to_string(),
            reference_point: None,
            prompt_points: None,
        }
    }

    /// [`from_config`](SegmentOpts::from_config) for one imported AI mask: the
    /// backend its `crs:MaskSubType` names, initially prompted at its own click.
    /// [`resolve_ai_masks`] adds measured subtype-0 gesture dabs when present.
    ///
    /// The subtype mapping is the measured one — **0 = Object/Background,
    /// 1 = Subject, 2 = Sky** — and the 0 case is where the honesty lives:
    /// Object and Background share that value and the sidecar does not
    /// separate them, so the segmenter is pointed at the object under the
    /// click in both cases and the possibility that the photographer meant its
    /// complement is DISCLOSED rather than guessed at.
    pub fn for_ai_mask(cfg: &Config, subtype: u32, ref_x: f32, ref_y: f32) -> Option<Self> {
        let target = match subtype {
            0 => "object",
            1 => "subject",
            2 => "sky",
            // Not reachable from an imported mask (`xmp::parse_ai_mask`
            // refuses anything else), but this is a public constructor and a
            // subtype we cannot route is not one to guess a backend for.
            _ => return None,
        };
        Some(SegmentOpts {
            python_bin: cfg.sidecar_python(),
            script: PathBuf::from(&cfg.segment_script),
            target: target.to_string(),
            // Space-separated with 6 decimals: the sidecar's own spelling
            // (`"0.517578 0.260997"`), so the value that reaches the model is
            // the value the file wrote to the precision the file wrote it at.
            reference_point: Some(format!("{ref_x:.6} {ref_y:.6}")),
            prompt_points: None,
        })
    }
}

/// Maximum TOTAL point count in a gesture prompt, including ReferencePoint.
/// The observed corpus gestures are single-Paint and roughly 12 dabs; this is
/// a safety valve, not a sampling policy. Evidence over the bound is refused.
pub const MAX_GESTURE_PROMPT_POINTS: usize = 2048;

/// Prompt mapping and IPC-schema version. It also names the scoped cache-key
/// component, so a future coordinate/label policy cannot reuse today's alpha.
const GESTURE_PROMPT_MAPPING_VERSION: &str = "gp1";
const GESTURE_PROMPT_REFUSAL: &str = "gesture point prompt refused";

/// Build the exact point list sent to SAM, or `None` when the legacy path must
/// remain untouched. Only subtype 0 has measured gesture semantics.
fn gesture_prompt_points(
    subtype: u32,
    ref_x: f32,
    ref_y: f32,
    gesture: &[crate::recipe::BrushStroke],
) -> Result<Option<Vec<[f32; 2]>>> {
    if subtype != 0 {
        return Ok(None);
    }

    let mut dabs = Vec::new();
    for (stroke_i, stroke) in gesture.iter().enumerate() {
        for (token_i, token) in stroke.dabs.split('\n').enumerate() {
            let mut fields = token.split_whitespace();
            match fields.next() {
                Some("d") => {
                    let (Some(x), Some(y)) = (fields.next(), fields.next()) else {
                        bail!(
                            "{GESTURE_PROMPT_REFUSAL}: stroke {} token {} is not `d x y`",
                            stroke_i + 1,
                            token_i + 1
                        );
                    };
                    if fields.next().is_some() {
                        bail!(
                            "{GESTURE_PROMPT_REFUSAL}: stroke {} token {} is not `d x y`",
                            stroke_i + 1,
                            token_i + 1
                        );
                    }
                    let x = x.parse::<f32>().with_context(|| {
                        format!(
                            "{GESTURE_PROMPT_REFUSAL}: stroke {} token {} has a non-numeric x",
                            stroke_i + 1,
                            token_i + 1
                        )
                    })?;
                    let y = y.parse::<f32>().with_context(|| {
                        format!(
                            "{GESTURE_PROMPT_REFUSAL}: stroke {} token {} has a non-numeric y",
                            stroke_i + 1,
                            token_i + 1
                        )
                    })?;
                    if !x.is_finite() || !y.is_finite() {
                        bail!(
                            "{GESTURE_PROMPT_REFUSAL}: stroke {} token {} has a non-finite coordinate",
                            stroke_i + 1,
                            token_i + 1
                        );
                    }
                    if dabs.len() + 1 >= MAX_GESTURE_PROMPT_POINTS {
                        bail!(
                            "{GESTURE_PROMPT_REFUSAL}: {} points exceeds MAX_GESTURE_PROMPT_POINTS={MAX_GESTURE_PROMPT_POINTS}",
                            dabs.len() + 2
                        );
                    }
                    dabs.push([x, y]);
                }
                // These are brush STATE, never SAM evidence. MaskValue, Flow,
                // hardness and Radius likewise stay ignored: no weights,
                // negative labels, boxes, centroids, sampling or dense-mask
                // prompt is established by the Lightroom measurements.
                Some("r" | "f" | "h") => {}
                Some(kind) => bail!(
                    "{GESTURE_PROMPT_REFUSAL}: stroke {} token {} has unknown kind {kind:?}",
                    stroke_i + 1,
                    token_i + 1
                ),
                None => {}
            }
        }
    }
    if dabs.is_empty() {
        return Ok(None);
    }

    // Use the same six-place value the legacy `--reference-point` carries, so
    // the JSON list and its scoped cache identity describe what reaches Python.
    let reference_text = format!("{ref_x:.6} {ref_y:.6}");
    let mut reference = reference_text.split_whitespace().map(str::parse::<f32>);
    let reference = [reference.next().unwrap()?, reference.next().unwrap()?];
    if !reference[0].is_finite() || !reference[1].is_finite() {
        bail!("{GESTURE_PROMPT_REFUSAL}: ReferencePoint has a non-finite coordinate");
    }
    dabs.insert(0, reference);
    Ok(Some(dabs))
}

/// The word the sidecar puts inside [`SegmentReport::backend`] when the run
/// took the fallback tier instead of the model the user ruled for.
///
/// It is `python/segment.py`'s own `U2NET_LABEL`
/// (`"U^2-Net (FALLBACK - BiRefNet did not run)"`), and this matches the ONE
/// word in it that states the condition rather than the model — so a future
/// fallback under a different name still reads as a fallback here.
/// `the_backend_label_survives_the_sidecar_line` pins the pairing from this
/// side; the constant's comment in `segment.py` names this file from the other.
const FALLBACK_MARK: &str = "FALLBACK";

/// What the sidecar said about the run that produced the mask.
///
/// Returned instead of `()` because `--target subject` has TWO possible
/// answers (R29 B4) and every surface downstream — the GUI's status line, the
/// alpha cache's provenance marker — needs to know which one ran. Before this,
/// the only carrier was a stderr line the Rust side forwarded to a console the
/// windowed GUI does not have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SegmentReport {
    /// The backend named in the sidecar's own brackets, e.g.
    /// `"BiRefNet e2bf8e4460fc"`. EMPTY when the line could not be found —
    /// an older `segment.py` on disk, which this bridge tolerates elsewhere
    /// too (see the `durable_adopt` belt at the end of [`segment_file`]).
    pub backend: String,
    /// Whether BiRefNet's imports resolved on this machine for this run.
    /// `None` for the single-tier backends, which have no such question, and
    /// for a sidecar too old to print the line.
    pub birefnet_deps: Option<bool>,
}

/// Does this backend label name the fallback tier? See [`FALLBACK_MARK`].
///
/// Free-standing so the GUI can ask it of the bare label it carries in
/// `Msg::Segmented` without reconstituting a [`SegmentReport`] — the one
/// definition of "this is a degraded run", used on both sides.
pub fn backend_is_fallback(backend: &str) -> bool {
    backend.contains(FALLBACK_MARK)
}

impl SegmentReport {
    /// Did this run take the fallback tier? See [`backend_is_fallback`].
    pub fn is_fallback(&self) -> bool {
        backend_is_fallback(&self.backend)
    }

    /// Parse the sidecar's stdout. Two lines matter and both are contracts
    /// (`python/segment.py`'s `main`):
    ///
    /// ```text
    /// segment.py: subject mask [BiRefNet e2bf8e4460fc] -> <folder>\mask.png
    /// segment.py: subject backend deps [ok]
    /// ```
    ///
    /// Split on `\r` as well as `\n` for the reason the stderr forwarder does:
    /// a progress bar rewrites one physical line, so a newline-only split can
    /// hand back a single enormous "line".
    ///
    /// A path with a `]` in it cannot confuse the first line — the label is
    /// taken from the FIRST `[` to the FIRST `]` after it, and both sit left of
    /// the ` -> ` the path follows.
    fn parse(stdout: &str) -> SegmentReport {
        let bracketed = |line: &str| -> Option<String> {
            let open = line.find('[')?;
            let rest = &line[open + 1..];
            let close = rest.find(']')?;
            Some(rest[..close].to_string())
        };
        let mut out = SegmentReport { backend: String::new(), birefnet_deps: None };
        for line in stdout.split(['\n', '\r']) {
            let line = line.trim();
            let Some(body) = line.strip_prefix("segment.py:") else { continue };
            let body = body.trim();
            if body.contains(" mask [") {
                if let Some(label) = bracketed(body) {
                    out.backend = label;
                }
            } else if body.starts_with("subject backend deps") {
                out.birefnet_deps = match bracketed(body).as_deref() {
                    Some("ok") => Some(true),
                    Some("missing") => Some(false),
                    // A third word is a sidecar this build does not understand;
                    // "unknown" is the honest answer, not a coin flip.
                    _ => None,
                };
            }
        }
        out
    }
}

#[derive(serde::Serialize)]
struct GesturePromptPayload<'a> {
    version: &'static str,
    points: &'a [[f32; 2]],
}

/// A staged prompt is caller-owned IPC and must never survive the sidecar run.
/// Drop covers spawn, timeout, non-zero exit, output refusal and success paths.
#[derive(Debug)]
struct StagedPromptFile {
    path: PathBuf,
}

impl Drop for StagedPromptFile {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn parse_reference_point_text(text: &str) -> Option<[f32; 2]> {
    let mut fields = text.split_whitespace();
    let out: [f32; 2] = [fields.next()?.parse().ok()?, fields.next()?.parse().ok()?];
    (fields.next().is_none() && out[0].is_finite() && out[1].is_finite()).then_some(out)
}

fn prompt_staging_path(input: &Path) -> PathBuf {
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "autoshade-ai-prompt-{}-{}-{}.json",
        std::process::id(),
        TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        crate::pipeline::stem(input)
    ))
}

fn stage_prompt_file(opts: &SegmentOpts, input: &Path) -> Result<Option<StagedPromptFile>> {
    let Some(points) = opts.prompt_points.as_deref() else { return Ok(None) };
    if opts.target != "object" {
        bail!("{GESTURE_PROMPT_REFUSAL}: --prompt-file is valid only for target object");
    }
    if !(2..=MAX_GESTURE_PROMPT_POINTS).contains(&points.len()) {
        bail!(
            "{GESTURE_PROMPT_REFUSAL}: point count {} is outside 2..={MAX_GESTURE_PROMPT_POINTS}",
            points.len()
        );
    }
    if points.iter().flatten().any(|v| !v.is_finite()) {
        bail!("{GESTURE_PROMPT_REFUSAL}: prompt contains a non-finite coordinate");
    }
    let reference = opts
        .reference_point
        .as_deref()
        .and_then(parse_reference_point_text)
        .context("gesture point prompt refused: object prompt has no valid ReferencePoint")?;
    if points[0] != reference {
        bail!("{GESTURE_PROMPT_REFUSAL}: first point is not the argv ReferencePoint");
    }

    let path = prompt_staging_path(input);
    let mut file = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .with_context(|| format!("stage gesture point prompt {}", path.display()))?;
    // Only arm cleanup after `create_new` proves this process owns the file.
    let staged = StagedPromptFile { path };
    serde_json::to_writer(
        &mut file,
        &GesturePromptPayload { version: GESTURE_PROMPT_MAPPING_VERSION, points },
    )
    .with_context(|| format!("write gesture point prompt {}", staged.path.display()))?;
    file.flush()
        .with_context(|| format!("flush gesture point prompt {}", staged.path.display()))?;
    drop(file);
    Ok(Some(staged))
}

fn append_segment_args(
    cmd: &mut Command,
    opts: &SegmentOpts,
    input: &Path,
    output: &Path,
    prompt_file: Option<&Path>,
) {
    cmd.arg("-E")
        .arg(&opts.script)
        .arg("--input")
        .arg(input)
        .arg("--output")
        .arg(output)
        .arg("--target")
        .arg(&opts.target);
    if let Some(p) = &opts.reference_point {
        cmd.arg("--reference-point").arg(p);
    }
    if let Some(path) = prompt_file {
        cmd.arg("--prompt-file").arg(path);
    }
    // Where the model weights live. Inside a macOS `.app` the script's own
    // directory is signed and read-only, so the sidecar's script-relative
    // default cannot be the answer there (see `config::default_weights_dir`).
    cmd.args(crate::config::Config::load().weights_args());
}

/// Run the sidecar: `input` (any image file) → `output` (8-bit grayscale PNG).
/// The mask is in the INPUT's frame — feed it the original-frame preview so it
/// lands in the same space recipe masks live in.
pub fn segment_file(opts: &SegmentOpts, input: &Path, output: &Path) -> Result<SegmentReport> {
    if !opts.script.exists() {
        bail!(
            "segmentation sidecar not found at {} — run from the project dir or set \
             AUTOSHADE_SEGMENT_SCRIPT.",
            opts.script.display()
        );
    }
    crate::pipeline::ensure_parent(output)?;
    // BEFORE the spawn: both call sites (`store::claim_raster`,
    // `pipeline::unique_out`) reserve the name by CREATING a 0-byte file, so
    // "the mask exists" is true before the sidecar ever runs — the original
    // `exists()` guard here never fired for them.
    let before = crate::artifact_state(output);
    let prompt_file = stage_prompt_file(opts, input)?;
    // `crate::sidecar_command` carries the whole child preamble — the `.env`
    // ALLOWLIST, captured stdio, no console flash, a kill group — shared with
    // the denoise bridge and the backend probe below so none of them can drift.
    let mut cmd = crate::sidecar_command(&opts.python_bin);
    // `-E`: ignore PYTHON* environment variables — the second layer for that
    // family specifically (config.rs protects them too).
    append_segment_args(
        &mut cmd,
        opts,
        input,
        output,
        prompt_file.as_ref().map(|p| p.path.as_path()),
    );
    // Under the model slot: BiRefNet / OneFormer / SAM are VRAM-resident
    // models like every other, and `batch --jobs 3` is three of these at once.
    let run = crate::with_model_slot(|| {
        crate::run_sidecar_child(
            &mut cmd,
            "segmentation sidecar",
            &format!("{} {}", opts.python_bin, opts.script.display()),
            crate::denoise::sidecar_timeout(),
        )
    });
    let out = match run {
        Ok(out) => out,
        Err(error) => {
            crate::denoise::discard_failed_output(output, before);
            return Err(error);
        }
    };
    if !out.status.success() {
        crate::denoise::discard_failed_output(output, before);
        return Err(crate::sidecar_exit_error("segmentation sidecar", &out));
    }
    // Exit 0 alone is not success — THIS run must have produced a non-empty
    // mask (see `crate::sidecar_wrote` for the three refusals).
    if let Err(e) = crate::sidecar_wrote("segmentation sidecar", output, before) {
        crate::denoise::discard_failed_output(output, before);
        return Err(e);
    }
    // Byte-level acceptance is not MASK acceptance (L08-8): a sidecar that
    // wrote garbage (a truncated PNG, an HTML error body) used to be adopted
    // here and persist as the recipe's raster. The product must DECODE as a
    // raster before anything references it.
    if let Err(e) = crate::render::open_mask_bounded(output) {
        crate::denoise::discard_failed_output(output, before);
        return Err(e.context(format!(
            "segmentation sidecar produced an undecodable mask {}",
            output.display()
        )));
    }
    // WHAT THE SIDECAR SAID ON A RUN THAT SUCCEEDED, forwarded instead of
    // discarded (R29 B4). Until now `out.stderr` was read only on the failure
    // path, so a successful run could carry news no surface ever showed — and
    // R29 B4 created news worth showing: `--target subject` degrades to the
    // U²-Net fallback tier when BiRefNet cannot run, and the sidecar announces
    // that degradation here and nowhere else. "BiRefNet rendered this" and
    // "U²-Net rendered this because BiRefNet could not" are two different
    // sentences about the photographer's mask and must not collapse into one.
    // Also surfaces the first-run 444 MB download, which used to look like a
    // hang. Silent when the sidecar was silent.
    //
    // OUR OWN LINES ONLY, and the filter is a contract rather than a guess:
    // every line this sidecar family writes deliberately carries one of two
    // prefixes — `segment.py:` from this script's own `print`/`die`, and
    // `[denoise]` from the shared verified downloader it borrows. Everything
    // else on that stream belongs to a library. Measured, on the object
    // backend: `transformers` writes a 245 KB carriage-return progress bar per
    // mask, which forwarding wholesale would dump on the photographer's console
    // and bury the one line that matters. The failure path is unfiltered and
    // stays that way — a crash's message can come from anywhere.
    //
    // NOT through `sidecar_tail` either: that helper keeps the LAST 400
    // characters, which is right for a crash (the message ends the traceback)
    // and wrong here — a warning puts its headline first, and the 400-character
    // tail cut "WARNING - the BiRefNet subject backend did not run" off the
    // front of the very disclosure this block exists to carry.
    let said = String::from_utf8_lossy(&out.stderr);
    // `\r` as well as `\n`: progress bars rewrite one physical line, so
    // splitting on newlines alone can hand back a single 245 KB "line".
    for line in said.split(['\n', '\r']) {
        let line = line.trim();
        if line.starts_with("segment.py:") || line.starts_with("[denoise]") {
            eprintln!("[segment] {line}");
        }
    }
    // The mask must be durable BEFORE any recipe references it (L03). The
    // sidecar fsyncs before its os.replace now — this adopt is the belt for
    // an older script on disk, at the cost of one flush.
    crate::store::durable_adopt(output)
        .with_context(|| format!("sync segmentation output {}", output.display()))?;
    // STDOUT, which until now was read only to pad a failure message. It
    // carries the one fact the mask itself cannot: which of `--target
    // subject`'s two backends actually answered.
    Ok(SegmentReport::parse(&String::from_utf8_lossy(&out.stdout)))
}

/// Can THIS machine run the BiRefNet subject backend right now?
///
/// `Some(true)` / `Some(false)` from the sidecar's `--probe-backend`, `None`
/// when the question could not be asked at all (no sidecar, no Python, a
/// sidecar too old to know the flag) — and `None` must never be read as "no",
/// because acting on it would recompute alphas on evidence nobody has.
///
/// **Memoised for the process, and the memo is the point.** The probe costs one
/// interpreter start plus three imports: measured 4.3 s where torchvision IS
/// installed (it pulls torch in), ~0.1 s where it is not, against 0.06 s for a
/// bare `python -c pass`. A recipe with four AI masks must pay that at most
/// once, and [`resolve_ai_masks`] spends it at most once per develop — only
/// when a CACHED alpha's marker says it came from the fallback tier on a
/// machine that could not do better. On a machine that never fell back, or one
/// that still cannot, this is never called or returns in ~0.1 s.
///
/// The memo is keyed on nothing: a dependency appearing mid-session is a case
/// the NEXT session picks up, which is the same granularity `resolve_ai_masks`
/// already gives the cache.
fn birefnet_deps_available(opts: &SegmentOpts) -> Option<bool> {
    use std::sync::OnceLock;
    static PROBED: OnceLock<Option<bool>> = OnceLock::new();
    *PROBED.get_or_init(|| {
        if !opts.script.exists() {
            return None;
        }
        // The SAME child discipline as the segmentation spawn, because it is
        // literally the same helper: an allowlisted `.env`, no inherited
        // console, and a kill group so a wedged probe cannot outlive the
        // develop. `-E` is this argv's own second layer against PYTHON*.
        let mut cmd = crate::sidecar_command(&opts.python_bin);
        cmd.arg("-E")
            .arg(&opts.script)
            .arg("--target")
            .arg("subject")
            .arg("--probe-backend")
            .args(crate::config::Config::load().weights_args());
        let out = crate::run_sidecar_child(
            &mut cmd,
            "segmentation backend probe",
            &format!("{} {}", opts.python_bin, opts.script.display()),
            crate::denoise::sidecar_timeout(),
        )
        .ok()?;
        if !out.status.success() {
            return None;
        }
        SegmentReport::parse(&String::from_utf8_lossy(&out.stdout)).birefnet_deps
    })
}

/// The provenance marker written beside one cached AI alpha.
///
/// **Why a sidecar FILE and not a term in the cache key.** The key is spelled
/// by the Rust side BEFORE the sidecar chooses a backend, so it cannot contain
/// the answer — that is exactly the limitation R29 B4 registered and this
/// closes. The marker is written AFTER the run, by the side that has just been
/// told which backend answered.
///
/// It lives at `<alpha>.backend`, i.e. `ai-mask-<hash>.backend` beside
/// `ai-mask-<hash>.png` in the photo's own develop dir, so it is swept, cleared
/// and snapshotted with the alpha it describes and never outlives it in a way
/// that matters (a stray marker with no PNG is simply never read). It is NOT a
/// recipe reference, so nothing relativizes it and no schema grows a field.
///
/// Three lines, `key=value`, version-stamped — the same shape every other
/// small store record in this tree carries:
///
/// ```text
/// autoshade-ai-mask-backend v1
/// backend=U^2-Net (FALLBACK - BiRefNet did not run)
/// birefnet-deps=missing
/// ```
struct BackendMarker {
    backend: String,
    /// The dependency verdict AT THE TIME the alpha was written — not now.
    /// This is what makes the re-derivation rule terminate: see
    /// [`should_renew_cached_alpha`].
    birefnet_deps: Option<bool>,
}

const BACKEND_MARKER_HEADER: &str = "autoshade-ai-mask-backend v1";

/// A marker is three short lines; anything larger is not one. Bounded for the
/// reason every read in the store module is (R28 2a): the develop dir is not
/// always written by this app.
const MAX_BACKEND_MARKER: u64 = 4 * 1024;

fn backend_marker_path(alpha: &Path) -> PathBuf {
    alpha.with_extension("backend")
}

impl BackendMarker {
    fn render(&self) -> String {
        let deps = match self.birefnet_deps {
            Some(true) => "ok",
            Some(false) => "missing",
            None => "unknown",
        };
        format!("{BACKEND_MARKER_HEADER}\nbackend={}\nbirefnet-deps={deps}\n", self.backend)
    }

    fn parse(text: &str) -> Option<BackendMarker> {
        let mut lines = text.lines();
        // The header is a GATE, not decoration: without it this would happily
        // read provenance out of any file that happened to sit at the name.
        if lines.next()?.trim() != BACKEND_MARKER_HEADER {
            return None;
        }
        let mut m = BackendMarker { backend: String::new(), birefnet_deps: None };
        for line in lines {
            if let Some(v) = line.strip_prefix("backend=") {
                m.backend = v.trim_end().to_string();
            } else if let Some(v) = line.strip_prefix("birefnet-deps=") {
                m.birefnet_deps = match v.trim() {
                    "ok" => Some(true),
                    "missing" => Some(false),
                    _ => None,
                };
            }
        }
        Some(m)
    }

    fn read(alpha: &Path) -> Option<BackendMarker> {
        let p = backend_marker_path(alpha);
        let text = crate::store::read_text_capped(&p, MAX_BACKEND_MARKER).ok()?;
        BackendMarker::parse(&text)
    }

    /// Best effort, and deliberately so: the ALPHA is the artifact, the marker
    /// is provenance about it. A marker that could not be written costs one
    /// missed re-derivation opportunity; refusing the mask over it would cost
    /// the photographer their selection.
    fn write(alpha: &Path, report: &SegmentReport) {
        let m = BackendMarker {
            backend: report.backend.clone(),
            birefnet_deps: report.birefnet_deps,
        };
        let _ = crate::store::durable_write(&backend_marker_path(alpha), m.render().as_bytes());
    }
}

/// Should a cache HIT be thrown away and re-segmented?
///
/// Exactly one case says yes, and it is the one R29 B4 registered as missing:
/// **the alpha was produced by the fallback tier BECAUSE this machine could
/// not run BiRefNet, and it now can.** The photographer installed torchvision
/// since; serving them the softer U²-Net edges forever, under a cache key that
/// records neither model, is the cache lying about provenance.
///
/// **Why the recorded verdict and not just "is it a fallback".** A machine that
/// has the dependencies but fails for another reason — a digest mismatch, a
/// card that will not hold the weights — falls back too, and re-running it on
/// every develop would be an infinite retry loop that never improves. Comparing
/// the verdict RECORDED WITH THE ALPHA against today's makes the rule fire only
/// on an actual CHANGE in this machine's capability, so it fires once and then
/// stops: the re-run rewrites the marker with today's verdict either way.
///
/// **NO MARKER means gen-1, and gen-1 is renewed ONCE.** This arm used to
/// leave an unmarked alpha alone, on the premise that the population was empty:
/// "`AI_BACKEND_GENERATION` 2 has not shipped, so every alpha that survives
/// into v0.35.0 re-derives once anyway". That premise was false when it was
/// written — the constant has been 2 since v0.35.0 — so every alpha a v0.35.0
/// build cached BEFORE markers existed is on disk right now under today's
/// generation, with no provenance, and was being served forever with softer
/// edges nobody could see recorded. So the policy reads what is actually on
/// disk: a marker present is trusted; a marker ABSENT means the alpha predates
/// provenance, is treated as gen-1, and is re-derived once. The re-run writes a
/// marker, so it happens once per alpha and never again.
///
/// **And only where a replacement can actually be produced.** The renewal path
/// DELETES the cached alpha before re-running (see `resolve_ai_masks`, which
/// must, or `sidecar_wrote` reads the stale file as this run's output), so
/// renewing on a machine that cannot segment would trade a good mask for an
/// `unresolved`. Both arms therefore require today's probe to say `Some(true)`:
/// an unanswerable probe (no interpreter, no script, a probe that timed out)
/// leaves the cache exactly as it is.
fn should_renew_cached_alpha(alpha: &Path, opts: &SegmentOpts) -> bool {
    let marker = BackendMarker::read(alpha);
    // The MARKER half first, so the 4.3 s probe is never spent on an alpha
    // whose own record already says the answer cannot change.
    if let Some(m) = marker.as_ref()
        && (!backend_is_fallback(&m.backend) || m.birefnet_deps != Some(false))
    {
        return false;
    }
    renewal_verdict(marker.as_ref(), birefnet_deps_available(opts))
}

/// The decision of [`should_renew_cached_alpha`] with today's verdict handed
/// IN, so the rule can be exercised by a test instead of being reachable only
/// through a real interpreter and a process-wide memo — the same split
/// [`ai_cache_key_gen`] makes for the generation term.
///
/// Both branches end in `deps_now == Some(true)` and that is the termination
/// argument: a marked fallback is renewed only on a machine that has GAINED the
/// backend (the re-run rewrites the verdict, so it cannot fire twice), and an
/// unmarked alpha only where a replacement can be produced (the re-run writes
/// the marker, so it cannot fire twice either).
fn renewal_verdict(m: Option<&BackendMarker>, deps_now: Option<bool>) -> bool {
    match m {
        Some(m) => {
            backend_is_fallback(&m.backend)
                && m.birefnet_deps == Some(false)
                && deps_now == Some(true)
        }
        None => deps_now == Some(true),
    }
}

/// What one [`resolve_ai_masks`] pass did, for the disclosure channels.
///
/// Counts, never a boolean: "3 AI masks re-derived, 1 declined" is a different
/// sentence from "some AI masks did not render", and the photographer needs
/// the first one.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct AiMaskResolution {
    /// Masks that now have an alpha — cache hits included.
    pub resolved: usize,
    /// Masks reused from the cache without running the model.
    pub cached: usize,
    /// Masks whose CACHED alpha was discarded and re-segmented because this
    /// machine gained the primary backend since it was written
    /// ([`should_renew_cached_alpha`]). Counted separately from `resolved`'s
    /// plain re-derivations: "your machine can now do better, so this mask was
    /// redone" is a different sentence from "this mask had no alpha yet", and
    /// the photographer paying seconds of GPU is owed the first one.
    pub renewed: usize,
    /// Masks with no alpha, each with the reason, in recipe order. These
    /// render INERT and raise [`crate::xmp::MaskImportReason::AiMaskUnresolved`].
    pub unresolved: Vec<(String, String)>,
}

impl AiMaskResolution {
    /// One English line for the CLI, or `None` when there was nothing to say.
    ///
    /// It always names the RECOMPUTATION, never just the count: a
    /// photographer who sees "3 AI masks rendered" and does not see "by the
    /// local segmenter, not Adobe's" has been told the wrong thing.
    pub fn describe(&self) -> Option<String> {
        if self.resolved == 0 && self.unresolved.is_empty() {
            return None;
        }
        let mut s = String::new();
        if self.resolved > 0 {
            s.push_str(&format!(
                "{} AI mask(s) re-derived by the local segmenter (NOT Adobe's own raster — the \
                 sidecar carries no pixels, so the alpha is an approximation of the \
                 photographer's intent)",
                self.resolved
            ));
            if self.cached > 0 {
                s.push_str(&format!("; {} reused from the develop cache", self.cached));
            }
            if self.renewed > 0 {
                s.push_str(&format!(
                    "; {} re-segmented because this machine can now run the BiRefNet subject \
                     backend it previously fell back from",
                    self.renewed
                ));
            }
        }
        for (name, why) in &self.unresolved {
            if !s.is_empty() {
                s.push_str("; ");
            }
            s.push_str(&format!("AI mask \"{name}\" carried but NOT rendered ({why})"));
        }
        Some(s)
    }
}

/// The cache key for one AI mask's recomputed alpha: everything that decides
/// the pixels, and nothing that does not.
///
/// `(photo identity, subtype, reference point, FRAME, backend generation)` plus
/// a scoped `gp1` component only when subtype 0 sends gesture dabs. Thus all
/// unaffected generation-2 keys stay byte-identical, while a changed ordered
/// point list cannot reuse a ReferencePoint-only alpha. The NAME is deliberately
/// absent: it is a localised label the photographer may rename without changing
/// one pixel.
///
/// **The frame term is R28 Batch-3 (adjudication F1-A), and the sentence above
/// was FALSE without it.** The segmenter runs on the frame
/// [`stage_source_frame`] stages, and that frame is chosen by the recipe's
/// `quarter_turns` — so the turn decides the pixels as surely as the click
/// does. A rotate normally moves the click too
/// ([`crate::render::orient_recipe_coords`] turns `ref_x`/`ref_y` with the
/// frame), which hid the omission behind a key that changed anyway — except at
/// the ONE point every quarter turn leaves where it was, the frame centre
/// `(0.5, 0.5)`. A mask clicked there kept its key across a rotate and the
/// cache served the alpha segmented in the OLD frame, sideways, with every
/// surface reporting a normal cache hit.
///
/// **Behaviour disclosure: every alpha cached by v0.33 and earlier re-derives
/// once.** Adding a field re-spells every key, so the first develop after this
/// build runs the segmenter again per mask (seconds of GPU) and reports it
/// through [`AiMaskResolution::describe`] as the re-derivation it is. That is
/// cache invalidation doing its job — the alternative is serving alphas whose
/// frame nobody recorded. The superseded PNGs stay in the develop dir, the
/// same accepted residue as any superseded raster ([`crate::store::claim_raster`]).
///
/// `AI_BACKEND_GENERATION` is bumped whenever the segmenter's own output would
/// change — a model re-pin, a preprocessing change, the mask-size cap. Without
/// it the cache would happily serve an alpha produced by a model this build no
/// longer runs, which is the one way a cache can lie about provenance. It is
/// deliberately NOT bumped for the frame term: the model did not change, the
/// key's shape did, and conflating the two would leave the next reader thinking
/// this build's segmenter differs from v0.33's.
///
/// **Generation 2 (R29 B4, user ruling five of 2026-08-21): the SUBJECT backend
/// is now `ZhengPeng7/BiRefNet` and every subject alpha cached by v0.34 and
/// earlier re-derives once.** This is the case the constant was written for — a
/// model re-pin, measured on the photographer's own library and ruled on rather
/// than guessed. The sky and object backends did not move; they re-derive too,
/// because one constant covers all three, and the alternative (a generation per
/// backend) buys three fields to save one re-run of a model whose alpha is
/// cached beside the develop anyway.
///
/// **The limitation this constant CANNOT express, and where it is handled
/// instead (R29 C3/C4).** Generation 2 means "BiRefNet, or U²-Net if this
/// machine cannot run BiRefNet" — the Rust side spells the key before the
/// sidecar chooses the backend, so no term of this string can record which one
/// answered, and widening the generation would not help: it is one number for
/// every machine, and the fact in question differs per machine. A machine that
/// fell back and LATER installs torchvision used to keep being served its
/// U²-Net alphas forever, with "delete the develop's `ai-mask-*.png`" as the
/// only remedy — a manual step nobody could know they needed. That is now
/// automatic and lives one layer out, in the per-alpha [`BackendMarker`] that
/// [`should_renew_cached_alpha`] reads: provenance belongs beside the artifact,
/// not in its name.
const AI_BACKEND_GENERATION: u32 = 2;

fn ai_cache_key(raw: &Path, subtype: u32, ref_x: f32, ref_y: f32, quarter_turns: u8) -> String {
    ai_cache_key_gen(raw, subtype, ref_x, ref_y, quarter_turns, AI_BACKEND_GENERATION)
}

fn ai_cache_key_with_prompt(
    raw: &Path,
    subtype: u32,
    ref_x: f32,
    ref_y: f32,
    quarter_turns: u8,
    prompt_points: Option<&[[f32; 2]]>,
) -> String {
    let Some(prompt_points) = prompt_points else {
        return ai_cache_key(raw, subtype, ref_x, ref_y, quarter_turns);
    };
    ai_cache_key_gen_scoped(
        raw,
        subtype,
        ref_x,
        ref_y,
        quarter_turns,
        AI_BACKEND_GENERATION,
        Some((prompt_points, GESTURE_PROMPT_MAPPING_VERSION)),
    )
}

/// [`ai_cache_key`] with the generation handed IN, so the term can be exercised
/// by a test instead of being the one field in the identity string that only a
/// release could prove. Nothing but [`ai_cache_key`] and its tests calls it, and
/// nothing but [`AI_BACKEND_GENERATION`] is ever passed in production.
fn ai_cache_key_gen(
    raw: &Path,
    subtype: u32,
    ref_x: f32,
    ref_y: f32,
    quarter_turns: u8,
    generation: u32,
) -> String {
    ai_cache_key_gen_scoped(
        raw,
        subtype,
        ref_x,
        ref_y,
        quarter_turns,
        generation,
        None,
    )
}

fn fnv1a64(bytes: impl IntoIterator<Item = u8>) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn gesture_prompt_cache_component(points: &[[f32; 2]], version: &str) -> String {
    // The sent VALUES, not BrushStroke storage: two canonical IEEE-754 words
    // per ordered point. Duplicate points remain duplicate words; r/f/h and
    // value/radius/flow/hardness never enter this stream because Python never
    // receives them. Little-endian fixes the hash across host architectures.
    let bytes = points
        .iter()
        .flat_map(|point| point.iter())
        .flat_map(|value| value.to_bits().to_le_bytes());
    format!("|{version}:{:016x}", fnv1a64(bytes))
}

fn ai_cache_key_gen_scoped(
    raw: &Path,
    subtype: u32,
    ref_x: f32,
    ref_y: f32,
    quarter_turns: u8,
    generation: u32,
    prompt: Option<(&[[f32; 2]], &str)>,
) -> String {
    ai_cache_key_from_photo_key(
        &crate::store::photo_key(raw),
        subtype,
        ref_x,
        ref_y,
        quarter_turns,
        generation,
        prompt,
    )
}

fn ai_cache_key_from_photo_key(
    photo_key: &str,
    subtype: u32,
    ref_x: f32,
    ref_y: f32,
    quarter_turns: u8,
    generation: u32,
    prompt: Option<(&[[f32; 2]], &str)>,
) -> String {
    // FNV-1a over the identity string: small, stable across runs and platforms
    // (unlike `DefaultHasher`, whose output std explicitly does not guarantee
    // between releases — a cache name that moved between builds would silently
    // re-run the model on every upgrade).
    //
    // `% 4` because that is what the RENDER folds the field by
    // (`render::quarter_turn_orientation`): a hand-edited `quarter_turns: 5`
    // renders as one turn, and the key must not invent a second frame for it.
    let mut ident = format!(
        "{photo_key}|{subtype}|{ref_x:.6}|{ref_y:.6}|t{}|{generation}",
        quarter_turns % 4
    );
    if let Some((points, version)) = prompt {
        ident.push_str(&gesture_prompt_cache_component(points, version));
    }
    let h = fnv1a64(ident.bytes());
    format!("ai-mask-{h:016x}")
}

/// Give every [`crate::recipe::MaskGeometry::AiMask`] in `recipe` an alpha,
/// running the segmentation sidecar only for the ones that do not already have
/// a usable cached raster.
///
/// **This is a RECOMPUTATION, not an import, and the whole design turns on
/// that.** Lightroom's `Mask/Image` component carries no raster and no
/// raster geometry — only the intent (`MaskSubType` + `ReferencePoint`), an
/// optional region-hint gesture, and provenance digests. So the only way to
/// render one is to run a segmenter of our own, whose alpha differs from Adobe's
/// at every edge and can differ grossly on a hard scene. Every surface that
/// shows the result must say so;
/// [`AiMaskResolution::describe`] is this function's half of that, and
/// [`crate::xmp::MaskImportReason::AiMaskRecomputed`] is the parser's.
///
/// **Lazy and cached.** Importing a library must not spawn a model run per
/// photo, so nothing happens at parse time; this runs at DEVELOP time, and a
/// cache hit costs one `open_mask_bounded` instead of seconds of GPU. The
/// raster lands in the photo's own develop dir under a name derived from
/// [`ai_cache_key`], so it is swept, snapshotted and relativized with every
/// other mask raster (`LocalAdjustment::bitmap_paths_mut`).
///
/// **A cache hit is not unconditional.** Each alpha carries a
/// [`BackendMarker`] naming the backend that made it, and one written by the
/// fallback tier on a machine that could not run BiRefNet is thrown away once
/// that machine can — see [`should_renew_cached_alpha`] for why that rule
/// terminates instead of retrying forever.
///
/// **TWO paths, and they are not interchangeable (R28 Batch-3, adjudication
/// F1-C).** `raw` is the PHOTO — the identity every cache decision is made on:
/// [`ai_cache_key`] hashes it and [`crate::store::raster_target`] homes the
/// alpha in ITS develop dir, which is what makes the file relativizable,
/// snapshottable and sweepable. `src` is the PIXELS the render will use, and
/// that may be a baked retouch master (`store::render_source_checked`) living
/// somewhere else entirely. Both call sites used to hand the master in as the
/// only argument, so the key and the cache home were computed from the master's
/// path — a develop directory belonging to no photo — and the staging decoder
/// (RAW-only, then) simply failed on it.
///
/// **The frame is the RECIPE's, not the disk's (adjudication F1-B).** The
/// staged frame is decoded at `recipe.quarter_turns`, the same number
/// `render::render_to_image_in` and `render::render_baked_to_image` turn by, so
/// the alpha is segmented in the frame it will be sampled in. Reading the turn
/// back off the SAVED develop instead (`store::saved_quarter_turns`) made a
/// recipe that disagrees with the store — an external `apply`, a rotate not yet
/// saved — segment one frame and render another, and cached the result under a
/// key that recorded neither.
///
/// **A failure is never silent and never fatal.** A missing sidecar, a missing
/// dependency, a declined mask (the sidecar's exit 3) or an undecodable output
/// all leave `raster` at `None` — which the render treats as "this mask needs
/// pixels and has none" and SKIPS the whole adjustment rather than applying it
/// at weight 0 (a 0 under `inverted` would apply the edit to the entire
/// frame). The reason is carried out in `unresolved`.
pub fn resolve_ai_masks(
    cfg: &Config,
    raw: &Path,
    src: &Path,
    recipe: &mut crate::recipe::EditRecipe,
) -> AiMaskResolution {
    use crate::recipe::MaskGeometry;

    let mut out = AiMaskResolution::default();
    // Read the frame ONCE, before the mask walk borrows the recipe mutably —
    // and read it from the recipe being resolved, which is the whole point of
    // handing this function the authoritative recipe in the first place.
    let turns = recipe.quarter_turns % 4;
    // The source frame the sidecar segments in. Produced ONCE and only when
    // something actually needs it — a recipe with no unresolved AI mask must
    // not pay a RAW decode.
    let mut preview: Option<Option<PathBuf>> = None;

    for m in recipe.masks.iter_mut() {
        let mask_name = if m.name.is_empty() { "unnamed".to_string() } else { m.name.clone() };
        // A ZONE mask arrives with its alpha already in hand and this function
        // must not touch it (`crate::recipe::MaskRole::is_zone`). The zoned
        // reverse-fit renders that alpha itself, claims it beside the develop
        // and then measures the zone's exposure, colour gains and saturation
        // AGAINST IT; re-segmenting would swap the population every one of
        // those numbers was solved on, silently, for a differently-edged sky
        // from a model run that also costs seconds of GPU. It is not a cache
        // and there is nothing to renew.
        //
        // An UNRESOLVED zone still resolves here — that is a rotate having
        // dropped the alpha, and subtype 2 at the turned reference point is
        // exactly the sky this zone means.
        let zone_owns_its_alpha = m.role.is_zone();
        for g in std::iter::once(&mut m.mask).chain(m.components.iter_mut().map(|c| &mut c.geometry))
        {
            let MaskGeometry::AiMask { subtype, ref_x, ref_y, gesture, raster, .. } = g else {
                continue;
            };
            if zone_owns_its_alpha && raster.is_some() {
                // Deliberately uncounted: `resolved` / `cached` are the
                // segmenter's own tallies and `describe` renders them as "N AI
                // mask(s) re-derived by the local segmenter". This mask was not.
                continue;
            }
            let (subtype, rx, ry) = (*subtype, *ref_x, *ref_y);
            let prompt_points = match gesture_prompt_points(subtype, rx, ry, gesture) {
                Ok(points) => points,
                Err(e) => {
                    out.unresolved.push((mask_name.clone(), format!("{e:#}")));
                    continue;
                }
            };
            // KEYED AND HOMED ON THE PHOTO, never on the pixel source (F1-C).
            let target = crate::store::raster_target(
                raw,
                &frame_scoped_key(
                    ai_cache_key_with_prompt(raw, subtype, rx, ry, turns, prompt_points.as_deref()),
                    src,
                ),
            );
            // Built BEFORE the cache probe now, because the probe needs it —
            // `should_renew_cached_alpha` asks the sidecar whether this machine
            // has gained the primary backend. Still cheap (two clones and a
            // format), and a subtype with no backend keeps its old behaviour:
            // the cache still answers for it, and only a MISS reports it.
            let mut opts = SegmentOpts::for_ai_mask(cfg, subtype, rx, ry);
            if let Some(opts) = opts.as_mut() {
                opts.prompt_points = prompt_points;
            }
            // CACHE HIT, and it is checked by DECODING, not by `exists()`: a
            // half-written or truncated PNG on disk is exactly the file a
            // cheap existence test would serve forever.
            let mut renewing = false;
            if target.exists() && crate::render::open_mask_bounded(&target).is_ok() {
                renewing = opts.as_ref().is_some_and(|o| should_renew_cached_alpha(&target, o));
                if !renewing {
                    *raster = Some(target.to_string_lossy().into_owned());
                    out.resolved += 1;
                    out.cached += 1;
                    continue;
                }
                // Cleared BEFORE the re-run, both files. `sidecar_wrote`
                // refuses an output whose length AND mtime are unchanged, so a
                // stale alpha left in place could turn a real re-run into a
                // reported failure; and a marker left behind a re-run that
                // fails would keep claiming a provenance nothing produced.
                let _ = std::fs::remove_file(&target);
                let _ = std::fs::remove_file(backend_marker_path(&target));
            }
            let Some(opts) = opts else {
                out.unresolved.push((mask_name.clone(), format!("no backend for subtype {subtype}")));
                continue;
            };
            if !opts.script.exists() {
                out.unresolved.push((
                    mask_name.clone(),
                    format!("the segmentation sidecar is not at {}", opts.script.display()),
                ));
                continue;
            }
            // The preview, decoded at most once per call — from the RENDER
            // SOURCE's pixels, in the RECIPE's frame.
            let input = preview.get_or_insert_with(|| stage_source_frame(src, turns));
            let Some(input) = input.as_ref() else {
                out.unresolved
                    .push((mask_name.clone(), "the source frame could not be decoded".into()));
                continue;
            };
            match segment_file(&opts, input, &target) {
                Ok(report) => {
                    // The marker lands with the alpha, not before it: a
                    // provenance record for a mask that does not exist would be
                    // read by the next develop and believed.
                    BackendMarker::write(&target, &report);
                    *raster = Some(target.to_string_lossy().into_owned());
                    out.resolved += 1;
                    // Counted on the SUCCESS arm, so `renewed` means "was
                    // re-segmented", which is what the disclosure says. A
                    // renewal whose re-run failed is an `unresolved`, and
                    // claiming it as a re-derivation would be the counter
                    // reporting an intention rather than an outcome.
                    if renewing {
                        out.renewed += 1;
                    }
                }
                Err(e) => {
                    // `segment_file` already discarded whatever it wrote, so
                    // the cache is not poisoned with a failed run.
                    out.unresolved.push((mask_name.clone(), format!("{e:#}")));
                }
            }
        }
    }
    // The staged source frame is an intermediate — the MASKS are the artifacts.
    if let Some(Some(p)) = preview.as_ref() {
        let _ = std::fs::remove_file(p);
    }
    out
}

/// Stage the render source's preview as a PNG for the sidecar, in the frame
/// `quarter_turns` produces — or `None` if it cannot be decoded.
///
/// **The RECIPE's frame, and the same door the render uses.**
/// [`crate::decode::decode_any_turned`] dispatches RAW → `decode_raw_turned`
/// (EXIF composed with the turn) and baked → `decode_baked` + the turn, which
/// is exactly the pair `render::render_to_image_in` / `render::
/// render_baked_to_image` apply — so the frame the segmenter sees IS the frame
/// the alpha will be sampled in, for a camera RAW and for a baked retouch
/// master alike. Before R28 Batch-3 this called the RAW-only decoder with the
/// turn read off the saved develop: a baked master could not be decoded at all
/// (the mask was carried, disclosed, and never rendered), and a recipe whose
/// turn differed from the store's produced a correct-looking alpha in the wrong
/// frame (adjudication F1-B/C).
///
/// A `crs:ReferencePoint` is normalised to the frame the photographer clicked
/// in, and `orient_recipe_coords` turns it with every rotate, so the click and
/// this frame stay in the same space; the mask is sampled in that same
/// normalised space (`render::sample_gray_norm`), so both ends agree without
/// either one knowing the photo's pixel dimensions.
fn stage_source_frame(src: &Path, quarter_turns: u8) -> Option<PathBuf> {
    let frame = mask_source_frame(src, quarter_turns, SEGMENT_FRAME_EDGE)
        .map_err(|e| {
            eprintln!("⚠ AI mask: cannot decode {} for segmentation: {e:#}", src.display());
        })
        .ok()?;
    let out = staging_path(src);
    frame
        .to_rgb8()
        .save(&out)
        .map_err(|e| eprintln!("⚠ AI mask: cannot stage the source frame {}: {e}", out.display()))
        .ok()?;
    Some(out)
}

/// The longest edge a RAW's sensor develop is made at for the segmenter: the
/// alpha is sampled in normalised coordinates at any render size, and the
/// desktop app segments its own working preview, which is no larger.
const SEGMENT_FRAME_EDGE: u32 = 4096;

/// The frame a mask raster is CUT on for `src`, in the recipe's quarter turns,
/// no longer than `max_edge` — shared by the segmenter and the face finder.
///
/// **A RAW's frame is its SENSOR develop, never its embedded preview**
/// (2026-10-26; the v1.2.2 lesson, `fit::same_frame_plausible_dims`). The
/// preview is the camera's choice: a body set to an in-camera aspect (Sony at
/// 4:3) stores a centred 4:3 CROP of the 3:2 sensor, so a mask normalised on
/// it was stretched and shifted on the render, which samples the sensor frame.
/// The neutral develop (`render::render_to_image` of a default recipe in the
/// recipe's turn) IS that frame, and it is the frame the desktop app already
/// segments and finds faces on — so every surface now cuts the same mask for
/// the same photo. A baked source (a retouch master, a JPEG) is its own pixels.
pub(crate) fn mask_source_frame(src: &Path, quarter_turns: u8, max_edge: u32) -> anyhow::Result<image::DynamicImage> {
    mask_source_frame_with(src, quarter_turns, max_edge, |p, turns, edge| {
        crate::render::render_to_image(
            p,
            &crate::recipe::EditRecipe { quarter_turns: turns, ..Default::default() },
            None,
            Some(edge),
        )
    })
}

/// [`mask_source_frame`] with the sensor develop handed in, so the frame RULE
/// is testable without a RAW file.
pub(crate) fn mask_source_frame_with(
    src: &Path,
    quarter_turns: u8,
    max_edge: u32,
    develop: impl FnOnce(&Path, u8, u32) -> anyhow::Result<image::DynamicImage>,
) -> anyhow::Result<image::DynamicImage> {
    if crate::decode::is_raw(src) {
        return develop(src, quarter_turns, max_edge);
    }
    let p = crate::decode::decode_any_turned(src, quarter_turns)?.preview;
    // `thumbnail` also ENLARGES a smaller frame to the box; only shrink.
    Ok(if p.width().max(p.height()) > max_edge { crate::render::thumbnail(&p, max_edge, max_edge) } else { p })
}

/// The cache key of an alpha cut on `src`'s mask frame. A RAW's frame changed
/// on 2026-10-26 from the embedded preview to the sensor develop
/// ([`mask_source_frame`]), so its key carries that frame and an alpha cut on
/// the old frame is never served for the new one; a baked source's frame did
/// not change and its key stays byte-identical.
fn frame_scoped_key(key: String, src: &Path) -> String {
    if !crate::decode::is_raw(src) {
        return key;
    }
    let h = fnv1a64(format!("{key}|frame=sensor-develop").bytes());
    format!("ai-mask-{h:016x}")
}

/// A FRESH temp name for one staged source frame.
///
/// pid + seq + stem, the shape `style::stage_embed_frame` settled on after
/// the pid + tag pair proved insufficient there for exactly this reason.
/// `resolve_ai_masks` has two single-photo callers today (`apply`, `auto`)
/// and neither runs concurrently, so the collision is LATENT
/// rather than live — which is precisely why the sequence belongs here and not
/// in a future incident report: the day a pool calls this (the `--jobs`
/// sequencer already runs `process_one` three-wide) two workers staging the
/// same stem would overwrite each other's frame and each other's cleanup, and
/// the symptom would be one photo's mask computed on another photo's pixels.
/// R28 Batch-3, adjudication F1-D.
fn staging_path(src: &Path) -> PathBuf {
    static TMP_SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
    std::env::temp_dir().join(format!(
        "autoshade-ai-src-{}-{}-{}.png",
        std::process::id(),
        TMP_SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
        crate::pipeline::stem(src)
    ))
}
