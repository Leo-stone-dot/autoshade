//! Image-and-text embedding bridge — Rust side of the sidecar (`python/embed.py`).
//!
//! Same shell-out pattern as [`crate::denoise`] and [`crate::segment`]: a local
//! Python process runs SigLIP 2 over one image (or a manifest of them, or a
//! manifest of texts) and writes 768-dim L2-normalised vectors. The reference
//! library ([`crate::library`]) keeps one image vector per finished photo and
//! one text vector per look-phrase list; a develop asks with its own image
//! vector and its direction's text vector. The weights auto-download to
//! `python/weights` on first run (~1.50 GB, digest pinned) — nothing is stored
//! in this repo.
//!
//! **Budgeted, since R28 Batch-4.** The model is the largest thing this tree
//! loads and it does not live in host RAM, so neither of the two budgets that
//! bound everything else (`decode::MAX_CONCURRENT_DECODES`, `jobs`' free-memory
//! division) ever saw it. Two rules close that: the sidecar is asked for
//! [`fp16_wanted`] half precision, and calls are SINGLE-FLIGHTED
//! ([`crate::with_model_slot`]) so at most one model is resident whatever the caller's
//! concurrency. Together, 4 concurrent × 1.50 GB becomes 1 × 0.75 GB.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::Config;

/// The vector width this build expects. Not a preference — [`parse_vector`]
/// refuses any other width, because a checkpoint that answered 1152 dims (the
/// so400m tier) would silently produce an index whose exemplars are not
/// comparable to each other.
pub const EMBED_DIM: usize = 768;
pub const MODEL_REPO: &str = "google/siglip2-base-patch16-384";
pub const MODEL_REVISION: &str = "f775b65a79762255128c981547af89addcfe0f88";
/// The ONE tokenizer class `python/embed.py` instantiates, and the class the
/// pinned `tokenizer_config.json` names.
///
/// Spelled on this side because it goes into a library's model stamp
/// ([`crate::library::Library::model`]): the checkpoint alone did not
/// distinguish two indices whose text vectors came from different doors, and
/// this batch's own root cause was exactly two doors on one checkpoint
/// answering vectors at cosine 0.72-0.78 of each other.
/// `the_embedding_sidecar_has_exactly_one_tokenizer_door` pins it against the
/// sidecar's own constant.
pub const TEXT_TOKENIZER_CLASS: &str = "GemmaTokenizer";

/// Everything one embedding run needs; built from [`Config`] like
/// [`crate::segment::SegmentOpts`].
#[derive(Clone, Debug)]
pub struct EmbedOpts {
    pub python_bin: String,
    pub script: PathBuf,
    pub text_file: Option<PathBuf>,
}

impl EmbedOpts {
    pub fn from_config(cfg: &Config) -> Self {
        EmbedOpts {
            python_bin: cfg.sidecar_python(),
            script: PathBuf::from(&cfg.embed_script),
            text_file: None,
        }
    }

    /// Is the sidecar even present? A library build asks BEFORE it decodes
    /// anything, so a build without the script says so once instead of
    /// failing per photo.
    pub fn available(&self) -> bool {
        crate::sidecar_script_present(&self.script)
    }
}

/// Load the model in HALF precision unless the user asked otherwise.
///
/// ON by default (R28 Batch-4 4b, adjudication F3): `python/embed.py` has
/// implemented `--fp16` since R27 Batch-5 and nothing ever passed it, so every
/// sidecar call has been loading 375.5 M parameters at 4 B each — 1.50 GB per
/// process — when 0.75 GB would do. It is a no-op on CPU by the sidecar's own
/// construction (`embed.py`: `if fp16 and device.startswith("cuda")`), so a
/// machine without CUDA is unaffected in every direction.
///
/// **Why the consumers tolerate it.** The sidecar casts the pooled output back
/// to fp32 and normalises THERE (`v = v.float()` before the norm), so the
/// record it writes is still float32 text declaring `"norm":"l2"` — no
/// invariant `parse_vector` checks moves, including the ±1e-3 norm gate. The
/// only consumer of the elements is a ranking — dot products of unit vectors
/// ([`crate::library`]'s search) — and fp16 carries ~4.9e-4 of relative
/// precision per element, which an order COMPARISON does not feel.
///
/// **MEASURED** (on the previous index, 169 photos, same binary and GPU, the
/// two arms differing only in `AUTOSHADE_EMBED_FP32`): `1 - cos(fp16, fp32)`
/// of the image vectors is at most 2.235e-5 and 2.951e-6 on average, both
/// arms land within 1.2e-7 of unit norm, and an image-only ranking's top-5
/// was identical for 168 of 169 leave-one-out queries (the exception swapped
/// ranks 1 and 2 of the same five). `AUTOSHADE_EMBED_FP32` stays a real escape
/// hatch: a library built in one precision is queried in the arithmetic it
/// was built with by setting the variable.
fn fp16_wanted() -> bool {
    !crate::config::live_env("AUTOSHADE_EMBED_FP32")
        .map(|v| !matches!(v.trim(), "" | "0" | "false" | "off"))
        .unwrap_or(false)
}

/// The BATCH TEXT door's argv — N strings in one process, no image.
///
/// It exists because the two index builders each need one text vector per
/// record, and the alternative was one sidecar PROCESS per record: the look
/// build used to re-invoke `embed.py` — a 1.5 GB model load — once per
/// photograph purely to embed that photograph's own tag string, and the RAW
/// build did not compute the vector at all. One manifest, one load, N vectors,
/// in order.
fn text_sidecar_args(
    script: &Path,
    manifest: &Path,
    output: &Path,
    fp16: bool,
) -> Vec<std::ffi::OsString> {
    let mut v: Vec<std::ffi::OsString> = vec![
        "-E".into(),
        script.into(),
        "--text-manifest".into(),
        manifest.into(),
        "--output".into(),
        output.into(),
    ];
    if fp16 {
        v.push("--fp16".into());
    }
    v
}

/// Run the sidecar over a JSONL text manifest and return the vectors IN ORDER.
///
/// `expect` is the number of lines the caller wrote: a short or long answer is
/// refused rather than zipped, because a text vector that landed on the wrong
/// record is a silently wrong ranking and nothing else.
pub fn embed_text_batch(
    opts: &EmbedOpts,
    manifest: &Path,
    scratch: &Path,
    expect: usize,
) -> Result<Vec<Vec<f32>>> {
    if !opts.script.exists() {
        bail!(
            "embedding sidecar not found at {} — run from the project dir or set \
             AUTOSHADE_EMBED_SCRIPT.",
            opts.script.display()
        );
    }
    let text = crate::run_model_sidecar(
        "embedding sidecar",
        &opts.python_bin,
        text_sidecar_args(&opts.script, manifest, scratch, fp16_wanted()),
        scratch,
    )?;
    let out = parse_text_vectors(&text, expect).with_context(|| {
        format!("embedding sidecar wrote an unusable text batch at {}", scratch.display())
    });
    // Removed on BOTH paths: a batch the parser refused is no more an answer
    // than one it accepted (see [`embed_file_record`]).
    let _ = std::fs::remove_file(scratch);
    out
}

/// One `{"text_vectors": [...]}` record → the vectors, each through the same
/// width / finiteness / unit-norm gate a single vector goes through.
pub fn parse_text_vectors(text: &str, expect: usize) -> Result<Vec<Vec<f32>>> {
    let rec: serde_json::Value =
        serde_json::from_str(text.trim()).context("embedding text output is not JSON")?;
    if let Some(e) = rec.get("error").and_then(|v| v.as_str()) {
        bail!("embedding sidecar declined this text batch: {e}");
    }
    let rows = rec
        .get("text_vectors")
        .and_then(|v| v.as_array())
        .context("embedding text output has no `text_vectors` array")?;
    if rows.len() != expect {
        bail!(
            "embedding sidecar answered {} text vectors for {expect} texts — a batch that \
             does not line up would attach a vector to the wrong record",
            rows.len()
        );
    }
    rows.iter()
        .map(|row| {
            let arr = row.as_array().context("`text_vectors` holds a non-array")?;
            parse_unit_vector(arr, "text_vectors")
        })
        .collect()
}

/// One JSON array → a checked unit vector of [`EMBED_DIM`] elements. The ONE
/// place the three vector-bearing keys (`vector`, `text_vector`,
/// `text_vectors`) agree about width, finiteness and norm.
fn parse_unit_vector(arr: &[serde_json::Value], key: &str) -> Result<Vec<f32>> {
    if arr.len() != EMBED_DIM {
        bail!("embedding `{key}` has wrong width {}", arr.len());
    }
    let mut out = Vec::with_capacity(arr.len());
    for x in arr {
        let f = x.as_f64().context("embedding vector holds a non-number")? as f32;
        if !f.is_finite() {
            bail!("embedding `{key}` holds a non-finite element");
        }
        out.push(f);
    }
    let n = out.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>().sqrt();
    if (n - 1.0).abs() > 1e-3 {
        bail!("embedding `{key}` is not L2-normalised (norm {n:.6})");
    }
    Ok(out)
}

/// One line of `embed.py --manifest-jsonl`'s JSONL answer: the frame, and
/// either its record or the sidecar's own reason for declining it.
#[derive(Debug, Clone)]
pub struct EmbedBatchRecord {
    pub path: String,
    pub record: Option<EmbedRecord>,
    pub error: Option<String>,
}

/// The BATCH IMAGE door's argv — N frames in one process, one model load.
///
/// It exists for the same reason the batch TEXT door does, one step later:
/// until S2 the old style-index build called this sidecar once PER PHOTOGRAPH on the
/// image side too, so a 169-photo library re-loaded 1.50 GB of weights 169
/// times — 5,618 s measured on the photographer's own corpus. `embed.py` has
/// implemented `--manifest-jsonl` since R27 Batch-5; nothing was wired to it.
fn image_batch_args(
    script: &Path,
    manifest: &Path,
    output: &Path,
    fp16: bool,
) -> Vec<std::ffi::OsString> {
    let mut v: Vec<std::ffi::OsString> = vec![
        // `-E`: the second layer against a PYTHON* import hijack (the env
        // allowlist in `dotenv_child_env` is the first).
        "-E".into(),
        script.into(),
        "--manifest-jsonl".into(),
        manifest.into(),
        "--output".into(),
        output.into(),
    ];
    if fp16 {
        v.push("--fp16".into());
    }
    v
}

/// Run the sidecar over a JSONL manifest of IMAGE paths and return one record
/// per line, in the sidecar's own order.
///
/// The caller maps them back by PATH, never by position: `embed.py` reports a
/// malformed manifest line as soon as it reads it and the rest in batch order,
/// so a positional zip would attach one photograph's vector to another's
/// exemplar — the failure [`parse_text_vectors`] refuses a short batch to
/// avoid.
pub fn embed_image_batch(
    opts: &EmbedOpts,
    manifest: &Path,
    scratch: &Path,
) -> Result<Vec<EmbedBatchRecord>> {
    if !opts.script.exists() {
        bail!(
            "embedding sidecar not found at {} — run from the project dir or set \
             AUTOSHADE_EMBED_SCRIPT.",
            opts.script.display()
        );
    }
    let text = crate::run_model_sidecar(
        "embedding sidecar",
        &opts.python_bin,
        image_batch_args(
            &opts.script,
            manifest,
            scratch,
            fp16_wanted(),
        ),
        scratch,
    )?;
    let out = parse_batch_records(&text).with_context(|| {
        format!("embedding sidecar wrote an unusable image batch at {}", scratch.display())
    });
    // Removed on BOTH paths: a batch the parser refused is no more an answer
    // than one it accepted (see [`embed_file_record`]).
    let _ = std::fs::remove_file(scratch);
    out
}

/// The sidecar's JSONL → records, each vector through the same width /
/// finiteness / unit-norm gate a single record goes through.
///
/// A line that is not JSON fails the WHOLE batch: every line the sidecar
/// writes it composed itself, and text it did not compose means the file is
/// not the answer it claims to be. A line carrying `error` is one
/// photograph's failure and is kept as one — the fail-soft contract.
pub fn parse_batch_records(text: &str) -> Result<Vec<EmbedBatchRecord>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let value: serde_json::Value = serde_json::from_str(line.trim())
            .with_context(|| format!("embedding batch line {} is not JSON", i + 1))?;
        let path = value
            .get("path")
            .and_then(|v| v.as_str())
            .with_context(|| format!("embedding batch line {} names no path", i + 1))?
            .to_string();
        if let Some(e) = value.get("error").and_then(|v| v.as_str()) {
            out.push(EmbedBatchRecord { path, record: None, error: Some(e.to_string()) });
            continue;
        }
        match parse_record(line.trim()) {
            Ok(record) => out.push(EmbedBatchRecord { path, record: Some(record), error: None }),
            // A record that fails the DOOR (wrong width, unnormalised, a
            // non-finite element) is this photograph's failure, not the
            // batch's: the index is allowed to be a mixed one, and refusing
            // 168 good vectors over one bad line would be the opposite of the
            // contract the per-line `error` arm keeps.
            Err(e) => out.push(EmbedBatchRecord {
                path,
                record: None,
                error: Some(format!("{e:#}")),
            }),
        }
    }
    Ok(out)
}

/// The sidecar's argv, as a pure function of the four things that decide it —
/// so the flags this build passes can be pinned by a test instead of being
/// visible only to a running Python.
fn sidecar_args(script: &Path, input: &Path, output: &Path, fp16: bool, text_file: Option<&Path>) -> Vec<std::ffi::OsString> {
    let mut v: Vec<std::ffi::OsString> = vec![
        // `-E`: the second layer against a PYTHON* import hijack (the env
        // allowlist in `dotenv_child_env` is the first).
        "-E".into(),
        script.into(),
        "--input".into(),
        input.into(),
        "--output".into(),
        output.into(),
    ];
    if fp16 {
        v.push("--fp16".into());
    }
    if let Some(p) = text_file { v.extend(["--text-file".into(), p.into()]); }
    v
}

#[derive(Debug, Clone, Default)]
pub struct EmbedRecord {
    pub vector: Vec<f32>,
    pub text_vector: Option<Vec<f32>>,
}

/// Run the sidecar on ONE image and return its record: the L2-normalised
/// vector, plus the text vector when the caller asked for one.
///
/// `scratch` is the JSON file the sidecar writes; the caller names it
/// (`library::frames` hands over a pid + seq name so parallel workers never
/// share one). The run itself is [`crate::run_model_sidecar`] — the
/// shared spawn/bound/exit-0-is-not-success executor.
///
/// SERIALISED against every other model sidecar in this process — see
/// [`crate::with_model_slot`]. The fan-out that gate closes (adjudication F3):
/// the old style-index build used to reach this door from up to
/// `decode::MAX_CONCURRENT_DECODES` = 4 workers, each spawning its own
/// sidecar, so four SigLIP loads could be live at once — 6.0 GB of fp32
/// weights, 3.0 GB even with `--fp16`, against a consumer GPU that commonly
/// has 4. Nothing else in the tree serialised them, because every other
/// budget is shaped like host RAM (`MAX_CONCURRENT_DECODES` counts 181 MB
/// decodes, `jobs` divides `GlobalMemoryStatusEx`) and the model does not live
/// in host RAM at all.
///
/// A GATE, not a batcher. The index build has since moved onto the manifest
/// doors ([`embed_image_batch`], [`embed_text_batch`] — one load per build),
/// but a manifest helps only a build: this single-image door is the
/// develop-time query, and `pipeline::produce_recipe` under `batch --jobs 3`
/// is three concurrent single-image calls that no manifest can merge. So the
/// gate stays, and it needs no new record format, no per-line failure mapping
/// and no second staging lifetime. The staging the caller did (writing the
/// PNG) stays OUTSIDE the gate: it is disk work, it costs no model, and
/// holding the slot across it would serialise the cheap half too.
pub fn embed_file_record(opts: &EmbedOpts, input: &Path, scratch: &Path) -> Result<EmbedRecord> {
    if !opts.script.exists() {
        bail!(
            "embedding sidecar not found at {} — run from the project dir or set \
             AUTOSHADE_EMBED_SCRIPT.",
            opts.script.display()
        );
    }
    let text = crate::run_model_sidecar(
        "embedding sidecar",
        &opts.python_bin,
        sidecar_args(&opts.script, input, scratch, fp16_wanted(), opts.text_file.as_deref()),
        scratch,
    )?;
    let v = parse_record(&text).with_context(|| {
        format!("embedding sidecar wrote an unusable record at {}", scratch.display())
    });
    // The scratch file is an INTERMEDIATE, not an artifact: the vector is what
    // the caller keeps. Leaving it behind would litter the develop store with
    // 10 KB JSON files nothing ever reads again — and a record the parser
    // refused is no more an answer than one it accepted, so the removal is on
    // BOTH paths rather than letting the refusal leave it at the scratch name.
    let _ = std::fs::remove_file(scratch);
    v
}

/// One sidecar JSON record → the vector, with every invariant the index's own
/// door check would otherwise have to re-derive.
///
/// Refuses, rather than repairs, four things: a record that carries an
/// `error`, a width other than [`EMBED_DIM`], a non-finite element, and a
/// vector whose L2 norm is not ~1. The last one is the load-bearing check —
/// the retrieval treats these as unit vectors and computes a cosine as a plain
/// dot product, so an unnormalised vector would not be *wrong by a little*, it
/// would silently outweigh every other exemplar.
pub fn parse_vector(text: &str) -> Result<Vec<f32>> {
    Ok(parse_record(text)?.vector)
}

pub fn parse_record(text: &str) -> Result<EmbedRecord> {
    let rec: serde_json::Value = serde_json::from_str(text.trim())
        .context("embedding output is not JSON")?;
    if let Some(e) = rec.get("error").and_then(|v| v.as_str()) {
        bail!("embedding sidecar declined this image: {e}");
    }
    let arr = rec
        .get("vector")
        .and_then(|v| v.as_array())
        .context("embedding output has no `vector` array")?;
    if arr.len() != EMBED_DIM {
        bail!(
            "embedding sidecar returned a {}-dim vector; this build indexes {EMBED_DIM}-dim \
             vectors and mixing widths in one index makes its exemplars incomparable",
            arr.len()
        );
    }
    let mut v = Vec::with_capacity(EMBED_DIM);
    for x in arr {
        let f = x.as_f64().context("embedding vector holds a non-number")? as f32;
        if !f.is_finite() {
            bail!("embedding vector holds a non-finite element");
        }
        v.push(f);
    }
    let norm = v.iter().map(|&x| (x as f64) * (x as f64)).sum::<f64>().sqrt();
    // 1e-3 is generous against the f32 round-trip of 768 shortest-decimal
    // elements (~1e-7 each) and tight enough that an unnormalised vector — the
    // only failure this is guarding — never passes.
    if (norm - 1.0).abs() > 1e-3 {
        bail!("embedding vector is not L2-normalised (norm {norm:.6})");
    }
    let parse_vec = |key: &str| -> Result<Option<Vec<f32>>> {
        let Some(arr) = rec.get(key).and_then(|v| v.as_array()) else { return Ok(None) };
        parse_unit_vector(arr, key).map(Some)
    };
    Ok(EmbedRecord { vector: v, text_vector: parse_vec("text_vector")? })
}
