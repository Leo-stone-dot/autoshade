//! Look-phrase bridge — Rust side of the sidecar (`python/describe.py`).
//!
//! Same shell-out pattern as [`crate::denoise`], [`crate::segment`] and
//! [`crate::embed`]: a local Python process runs Qwen3-VL-2B-Instruct over the
//! staged 512-px frames a reference-library build produces and writes, per
//! frame, the photograph's GRADE as a few short comma-separated phrases — the
//! way a photographer types a look request. The SigLIP text tower turns them
//! into the vector a typed direction is compared against
//! ([`crate::library`]). The weights auto-download to `python/weights` on
//! first run (~4.3 GB, digest pinned) — nothing is stored in this repo.
//!
//! **The phrases are UNTRUSTED text.** They are model output about the user's
//! own photograph, and they reach a proposer prompt. [`sanitize_desc`] is the
//! door: single line, no control characters, none of the invisible Cf
//! formatting characters `is_invisible` names, bounded to [`MAX_DESC_CHARS`],
//! non-empty or absent. The sidecar applies the same rule before writing — a
//! bound that protects a prompt has to hold even when the program on disk is
//! replaced.
//!
//! **One model at a time.** The run goes through [`crate::run_model_sidecar`]
//! under the process-wide [`crate::with_model_slot`], so Qwen (4.0 GiB in
//! bf16) is never resident beside SigLIP, SD 2.1, SCUNet or OneFormer.

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};

use crate::config::Config;

/// The checkpoint this build speaks to, spelled on the Rust side because it
/// goes into a library's model stamp: phrases from another checkpoint (or
/// another prompt) are not the same answer.
pub const MODEL_REPO: &str = "Qwen/Qwen3-VL-2B-Instruct";
pub const MODEL_REVISION: &str = "89644892e4d85e24eaac8bacfd4f463576704203";
/// `python/describe.py`'s `PROMPT_VERSION`, part of a library's model stamp
/// so an edited prompt re-describes. `the_prompt_version_is_the_sidecars_own`
/// pins the two together. v2 (2026-10-13): short look phrases, not a sentence.
pub const PROMPT_VERSION: u32 = 3;

/// The longest phrase list, in characters — the sidecar's own `MAX_DESC_CHARS`,
/// pinned to it by `the_phrase_bound_is_the_sidecars_own`.
pub const MAX_DESC_CHARS: usize = 200;

/// Everything one description run needs; built from [`Config`] like
/// [`crate::embed::EmbedOpts`] and [`crate::correspond::CorrespondOpts`].
#[derive(Clone, Debug)]
pub struct DescribeOpts {
    pub python_bin: String,
    pub script: PathBuf,
}

impl DescribeOpts {
    pub fn from_config(cfg: &Config) -> Self {
        DescribeOpts {
            python_bin: cfg.sidecar_python(),
            script: PathBuf::from(&cfg.describe_script),
        }
    }

    /// Is the sidecar even present? The index build asks BEFORE it stages a
    /// single frame, so a machine without it says so once instead of failing
    /// per photograph.
    pub fn available(&self) -> bool {
        crate::sidecar_script_present(&self.script)
    }
}

/// Load the model in BFLOAT16 unless the user asked otherwise.
///
/// ON by default, and measured rather than argued: on this machine's RTX
/// 4060 Ti (8 GB) a bf16 run peaks at **4.03 GiB allocated / 4.07 GiB
/// reserved**, so it fits with room for the rest of the desktop; fp32 would
/// need ~8.5 GiB of weights alone and would not. bf16 is also the precision
/// the checkpoint's own `config.json` declares (`"dtype": "bfloat16"`), i.e.
/// what it was trained and released in.
///
/// The escape hatch is real rather than decorative: `AUTOSHADE_DESCRIBE_FP32`
/// forces the fp32 load, which is the only way to describe on a machine whose
/// GPU disagrees with bf16 — and on CPU the sidecar ignores the flag by its
/// own construction (`describe.py`: `bf16 and device.startswith("cuda")`).
fn bf16_wanted() -> bool {
    !crate::config::live_env("AUTOSHADE_DESCRIBE_FP32")
        .map(|v| !matches!(v.trim(), "" | "0" | "false" | "off"))
        .unwrap_or(false)
}

/// The sidecar's argv, as a pure function of the three things that decide it —
/// so the flags this build passes can be pinned by a test instead of being
/// visible only to a running Python.
fn sidecar_args(
    script: &Path,
    manifest: &Path,
    output: &Path,
    bf16: bool,
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
    if bf16 {
        v.push("--bf16".into());
    }
    v
}

/// One line of the sidecar's JSONL answer: the frame it describes, and either
/// a description or the sidecar's own reason for declining it.
#[derive(Debug, Clone)]
pub struct DescribeRecord {
    pub path: String,
    pub desc: Option<String>,
    pub error: Option<String>,
}

/// Run the sidecar over a JSONL manifest of frame paths and return one record
/// per line, IN THE SIDECAR'S ORDER.
///
/// The caller maps them back by PATH, never by position: `describe.py` reports
/// a malformed manifest line immediately and the rest in loop order, so a
/// positional zip would attach one photograph's description to another's
/// exemplar — the same failure `embed::parse_text_vectors` refuses a short
/// batch to avoid.
///
/// ONE process for a whole build. That is the entire point of the manifest
/// door: the expensive part of this sidecar is the 4.3 GB model LOAD, and a
/// per-photograph call would pay it 169 times for the user's own library.
pub fn describe_manifest(
    opts: &DescribeOpts,
    manifest: &Path,
    scratch: &Path,
) -> Result<Vec<DescribeRecord>> {
    if !opts.script.exists() {
        bail!(
            "look-description sidecar not found at {} — run from the project dir or set \
             AUTOSHADE_DESCRIBE_SCRIPT.",
            opts.script.display()
        );
    }
    let text = crate::run_model_sidecar(
        "look-description sidecar",
        &opts.python_bin,
        sidecar_args(&opts.script, manifest, scratch, bf16_wanted()),
        scratch,
    )?;
    let out = parse_records(&text).with_context(|| {
        format!("look-description sidecar wrote an unusable batch at {}", scratch.display())
    });
    // The scratch file is an INTERMEDIATE, not an artifact: the descriptions
    // are what the caller keeps, and leaving it behind would litter the store
    // with a JSONL nothing reads again. Removed on BOTH paths — a batch the
    // parser refused is no more an answer than one it accepted, and it used
    // to survive the refusal at the scratch name looking like one.
    let _ = std::fs::remove_file(scratch);
    out
}

/// The sidecar's JSONL → records, each description through [`sanitize_desc`].
///
/// A line that is not JSON is a FAILURE of the whole batch, not of one record:
/// the sidecar writes its output atomically and every line it writes is one it
/// composed with `json.dumps`, so unparseable text means the file is not the
/// answer it claims to be. A line that carries `error` IS a per-record
/// failure and is kept as one — that is the fail-soft contract.
pub fn parse_records(text: &str) -> Result<Vec<DescribeRecord>> {
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let rec: serde_json::Value = serde_json::from_str(line.trim())
            .with_context(|| format!("look-description line {} is not JSON", i + 1))?;
        let path = rec
            .get("path")
            .and_then(|v| v.as_str())
            .with_context(|| format!("look-description line {} names no path", i + 1))?
            .to_string();
        if let Some(e) = rec.get("error").and_then(|v| v.as_str()) {
            out.push(DescribeRecord { path, desc: None, error: Some(e.to_string()) });
            continue;
        }
        // The record must declare the checkpoint AND the prompt it came from,
        // and both must be the ones this build stamps its library with. A
        // sidecar that answered from another revision would store phrases the
        // stamp says are ours.
        let says = |key: &str| rec.get(key).and_then(|v| v.as_str()).unwrap_or_default();
        if says("model") != MODEL_REPO || says("revision") != MODEL_REVISION {
            bail!(
                "look-phrase sidecar answered from {}@{}, and this build expects \
                 {MODEL_REPO}@{MODEL_REVISION}",
                says("model"),
                says("revision")
            );
        }
        let version = rec.get("prompt_version").and_then(|v| v.as_u64());
        if version != Some(PROMPT_VERSION as u64) {
            bail!(
                "look-phrase sidecar answered prompt version {:?}, and this build expects \
                 v{PROMPT_VERSION}",
                version
            );
        }
        let desc = rec.get("desc").and_then(|v| v.as_str()).and_then(sanitize_desc);
        match desc {
            Some(d) => out.push(DescribeRecord { path, desc: Some(d), error: None }),
            None => out.push(DescribeRecord {
                path,
                desc: None,
                error: Some("the sidecar's description was empty after the door's bounds".into()),
            }),
        }
    }
    Ok(out)
}

/// THE DOOR: one model answer → the single bounded line an index may carry.
///
/// Four rules, each a real failure mode rather than tidiness:
/// * control characters (`\n` included) become spaces — a description is
///   appended to a proposer block as one field of one line, and a newline in
///   it would forge a new line of that block;
/// * the invisible Cf block (soft hyphen, zero-width joiners, the bidi
///   overrides, the interlinear annotations, the BOM) is stripped — those are
///   exactly what a payload hides behind, and they survive a "printable"
///   filter;
/// * runs of whitespace collapse, so the bound below counts content;
/// * the result is cut to [`MAX_DESC_CHARS`] CHARACTERS (never
///   bytes — a byte slice would split a multi-byte codepoint) and an empty
///   result is `None`, because an empty description is an absent one.
pub fn sanitize_desc(text: &str) -> Option<String> {
    let mut out = String::with_capacity(text.len().min(MAX_DESC_CHARS * 4));
    // Characters in `out`, counted as they land: the bound below is checked
    // once per kept character, so a re-count per push was quadratic as well.
    let mut used = 0usize;
    let mut pending_space = false;
    for ch in text.chars() {
        let drop_it = is_invisible(ch);
        let space = ch.is_whitespace() || (ch as u32) < 0x20 || ch == '\u{7f}';
        if drop_it || space {
            // Only mark a gap once we have content: leading whitespace is
            // trimmed by construction rather than by a second pass.
            pending_space = !out.is_empty();
            continue;
        }
        // The gap and the character behind it land together or not at all,
        // and the bound is checked over BOTH before either is pushed. Pushing
        // the space first and checking after it let a description that was
        // exactly at the cap gain one more character — a trailing space — and
        // the library's door then refused the whole entry for being over the
        // bound this function exists to hold.
        if used + usize::from(pending_space) >= MAX_DESC_CHARS {
            break;
        }
        if pending_space {
            out.push(' ');
            used += 1;
            pending_space = false;
        }
        out.push(ch);
        used += 1;
    }
    (!out.is_empty()).then_some(out)
}

/// The Cf (format) characters [`sanitize_desc`] removes, by CODE POINT — the
/// characters themselves are invisible in an editor, so a literal set could
/// not be reviewed. `describe.py`'s `_INVISIBLE` is kept to the same set for
/// what it writes; this door is the one that has to hold, so a code point is
/// never stripped THERE alone.
fn is_invisible(ch: char) -> bool {
    matches!(
        ch,
        '\u{00ad}'
            | '\u{200b}'..='\u{200f}'
            | '\u{202a}'..='\u{202e}'
            | '\u{2060}'..='\u{2064}'
            | '\u{2066}'..='\u{2069}'
            | '\u{feff}'
            // The interlinear annotation anchor, separator and terminator:
            // named by the rule above since it was written, in neither set
            // until the 2026-09 audit.
            | '\u{fff9}'..='\u{fffb}'
            // The Arabic letter mark and the Mongolian vowel separator (Cf),
            // and the TAG block — LANGUAGE TAG plus the invisible copy of
            // ASCII 0x20–0x7E — which is the canonical carrier for text a
            // reader never sees and a model reads as instructions. Mirrored
            // in `python/describe.py`'s `_INVISIBLE`.
            | '\u{061c}'
            | '\u{180e}'
            | '\u{e0001}'
            | '\u{e0020}'..='\u{e007f}'
    )
}

/// The CONTENT key of one staged frame: the SHA-256 of its bytes.
///
/// Content, not path: a reference-library entry is identified by its pixels,
/// so a moved or renamed photo keeps what was measured from it
/// ([`crate::library::Entry::id`]).
pub fn frame_digest(path: &Path) -> Result<String> {
    use std::io::Read as _;
    let mut file = std::fs::File::open(path)
        .with_context(|| format!("read staged frame {}", path.display()))?;
    let mut s = crate::sha256::Sha256::new();
    let mut buf = vec![0u8; 1 << 16];
    loop {
        let n = file
            .read(&mut buf)
            .with_context(|| format!("read staged frame {}", path.display()))?;
        if n == 0 {
            break;
        }
        s.update(&buf[..n]);
    }
    Ok(s.finish())
}
