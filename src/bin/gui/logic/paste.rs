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

#[cfg(test)]
mod tests {
    use super::*;

    /// A clean paste names its counts; a failed projection and a regenerated
    /// sidecar each ride as their own suffix, in that order.
    #[test]
    fn a_clean_paste_names_its_counts_and_disclosures() {
        let (base, suffixes) = clean_words(12, 12, &[], &[]);
        assert_eq!(base.key, "Recipe pasted to {ok} photos ({xmp} XMP) → develop store");
        assert_eq!(base.args[0].1, super::super::Arg::Text("12".into()));
        assert!(suffixes.is_empty(), "nothing to disclose");

        let (_, both) = clean_words(
            3,
            2,
            &["a.xmp".into(), "b.xmp".into()],
            &["c.xmp".into()],
        );
        assert_eq!(both.len(), 2);
        assert!(both[0].key.starts_with(" — ⚠ "), "the projection failure interrupts the sentence");
        assert!(both[0].key.contains("{detail}"));
        assert_eq!(both[0].args[1].1, super::super::Arg::Text("a.xmp · b.xmp".into()));
        assert!(both[1].key.contains("regenerated rather than merged"));
    }

    /// A partial paste reports both counts and every error; a long list still
    /// says all of them (only the DISCLOSURE lists are abbreviated).
    #[test]
    fn a_partial_paste_names_the_failures() {
        let words = partial_words(4, &["a failed".into(), "b failed".into()]);
        assert_eq!(words.key, "{ok} succeeded, {fail} failed: {detail}");
        assert_eq!(words.args[0].1, super::super::Arg::Text("4".into()));
        assert_eq!(words.args[1].1, super::super::Arg::Text("2".into()));
        assert_eq!(words.args[2].1, super::super::Arg::Text("a failed · b failed".into()));
    }
}
