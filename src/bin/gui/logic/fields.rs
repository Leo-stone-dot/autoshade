//! Small pure field rules the panels share: the export target's container, the
//! analysis model's provider flip, the model pick-list and the credential
//! fingerprint. No files, no widgets.

/// The FORMAT dropdown owns the export container; a typed save path only picks
/// the name. Spellings of the SAME container (jpeg/jfif, tiff) are kept as
/// typed; a different container is replaced by the selected one.
pub(crate) fn normalize_export_target(mut p: std::path::PathBuf, ext: &str) -> std::path::PathBuf {
    let typed = p.extension().map(|e| e.to_string_lossy().to_ascii_lowercase());
    let same = matches!(
        (ext, typed.as_deref()),
        ("jpg", Some("jpg" | "jpeg" | "jfif")) | ("tif", Some("tif" | "tiff")) | ("png", Some("png"))
    );
    if !same {
        p.set_extension(ext);
    }
    p
}

/// What the analysis model field should become when the provider radio flips
/// (`None` = keep the user's value). API-ward only CLI aliases are rewritten (a
/// full `claude-*` id is legitimate on a bridge); OAuth-ward anything that is
/// neither an alias nor a `claude-*` id becomes `opus`.
pub(crate) fn analysis_model_on_flip(current: &str, to_api: bool) -> Option<&'static str> {
    let m = current.trim();
    let claude_alias = crate::model::CLAUDE_ALIASES.contains(&m);
    let claude_id = claude_alias || m.to_ascii_lowercase().starts_with("claude-");
    if to_api && claude_alias {
        Some("gpt-5.5")
    } else if !to_api && !claude_id {
        Some("opus")
    } else {
        None
    }
}

/// The model ComboBox's option list: fetched ids when present, else the
/// grounded fallback; the current value is always included (a custom id is
/// never dropped from its own menu).
pub(crate) fn model_opts(fetched: &[String], fallback: &[&str], current: &str) -> Vec<String> {
    let mut v: Vec<String> = if fetched.is_empty() {
        fallback.iter().map(|s| s.to_string()).collect()
    } else {
        fetched.to_vec()
    };
    if !current.trim().is_empty() && !v.iter().any(|x| x == current) {
        v.insert(0, current.to_string());
    }
    v
}

/// In-memory stamp for "which credential fetched this pick-list". Session-local
/// only, never persisted: a collision merely keeps a stale list one fetch
/// longer.
pub(crate) fn key_fingerprint(key: &str) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut h);
    h.finish()
}
