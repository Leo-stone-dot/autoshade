//! The batch-paste landing, headless: what a paste wrote and what to say about
//! it. The stash/-baseline bookkeeping and the worker call are the runtime's;
//! the sentences and their composition are not.
use autoshade::text::generated::*;


use super::{UiText, brief_list};

/// The line for a FULLY successful paste: the base, plus the sidecar
/// disclosures that ride it (a failed projection does not fail the paste; a
/// regenerated sidecar dropped Lightroom-only properties).
pub(crate) fn clean_words(
    ok: usize,
    xmp: usize,
    xmp_fails: &[String],
    xmp_notes: &[String],
) -> (UiText, Vec<UiText>) {
    let base = UiText::with(
        K_LOGIC_RECIPE_PASTED_TO_PHOTOS_XMP_DEVELOP_STORE,
        &[("ok", &ok.to_string()), ("xmp", &xmp.to_string())],
    );
    let mut suffixes = Vec::new();
    if !xmp_fails.is_empty() {
        suffixes.push(UiText::with(
            K_LOGIC_XMP_PROJECTION_S_FAILED_THOSE_PASTES_ARE_SAVED,
            &[("n", &xmp_fails.len().to_string()), ("detail", &brief_list(xmp_fails))],
        ));
    }
    if !xmp_notes.is_empty() {
        suffixes.push(UiText::with(
            K_LOGIC_SIDECAR_S_REGENERATED_RATHER_THAN_MERGED_LIGHTRO,
            &[("n", &xmp_notes.len().to_string()), ("detail", &brief_list(xmp_notes))],
        ));
    }
    (base, suffixes)
}

/// The failure line for a PARTIAL paste: it must never read as a clean
/// success, so both counts and every error are in it.
pub(crate) fn partial_words(ok: usize, errs: &[String]) -> UiText {
    UiText::with(
        K_LOGIC_SUCCEEDED_FAILED,
        &[("ok", &ok.to_string()), ("fail", &errs.len().to_string()), ("detail", &errs.join(" · "))],
    )
}
