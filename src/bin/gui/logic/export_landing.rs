//! The export landing's facts and words, headless. The export itself (render,
//! guard, claim, write) is the worker's; remembering where a batch DELIVERED,
//! the camera-curve repair and the ● baseline are the runtime's. This module
//! owns what an outcome says.
use autoshade::text::generated::*;


use super::{UiText, brief_list};
use std::path::PathBuf;

/// What one export run DELIVERED. Facts, not prose; the landing words them.
pub(crate) enum ExportOutcome {
    Single {
        out: PathBuf,
        /// The export repaired its own copy of a washed base curve (saved by an
        /// earlier version): the canvas is repaired too, and the line says so.
        relooked: bool,
        /// Masks that could not be drawn — the file is written without them.
        warn: Option<String>,
    },
    Batch {
        ok: usize,
        errs: Vec<String>,
        /// Same-name photos kept apart: "photo → name" pairs.
        renamed: Vec<String>,
        /// Photos whose base look the export had to re-estimate.
        relooked: usize,
        /// The develop warnings every photo's own export shares.
        warns: Vec<String>,
        dest: PathBuf,
    },
}

/// One single-export line, the camera-curve repair that rides it, and the mask
/// warning that rides it.
pub(crate) struct SingleWords {
    pub(crate) line: UiText,
    pub(crate) relook: Option<UiText>,
    pub(crate) warn: Option<UiText>,
}

/// Word one single export. `path` is the already-displayed target.
pub(crate) fn single_words(path: &str, relooked: bool, warn: Option<&str>) -> SingleWords {
    let line = UiText::with(K_LANDING_EXPORTED, &[("path", path)]);
    let relook = relooked.then(|| {
        UiText::of(
            K_EXPORT_CAMERA_BASE_LOOK_RE_ESTIMATED_THIS_PHOTO_WAS_SAV,
        )
    });
    let warn = warn.map(|why| {
        UiText::with(
            K_LANDING_SOME_MASKS_COULD_NOT_BE_DRAWN_SO_THEY_ARE_NOT_AP,
            &[("why", why)],
        )
    });
    SingleWords { line, relook, warn }
}

/// A batch landing's line: the base (done or partial), plus the disclosures
/// that ride it in a fixed order.
pub(crate) struct BatchWords {
    pub(crate) base: UiText,
    pub(crate) renamed: Option<UiText>,
    pub(crate) relooked: Option<UiText>,
    pub(crate) develops: Option<UiText>,
}

/// Word one batch export. `dest` is the already-displayed delivery folder.
pub(crate) fn batch_words(
    ok: usize,
    errs: &[String],
    renamed: &[String],
    relooked: usize,
    warns: &[String],
    dest: &str,
) -> BatchWords {
    let base = if errs.is_empty() {
        UiText::with(K_LANDING_BATCH_DONE, &[("n", &ok.to_string()), ("path", dest)])
    } else {
        UiText::with(
            K_LANDING_BATCH_SUCCEEDED_FAILED,
            &[
                ("ok", &ok.to_string()),
                ("fail", &errs.len().to_string()),
                ("detail", &errs.join("; ")),
            ],
        )
    };
    // Same-stem photos were kept apart — disclose WHICH photo took WHICH name,
    // or the user hunts for an export that "vanished".
    let renamed = (!renamed.is_empty()).then(|| {
        UiText::with(K_LANDING_SAME_NAME_PHOTOS_KEPT_APART, &[("list", &renamed.join(", "))])
    });
    let relooked = (relooked > 0).then(|| {
        UiText::with(
            K_LANDING_BASE_LOOK_S_RE_ESTIMATED_SAVED_BY_AN_EARLIER_VER,
            &[("n", &relooked.to_string())],
        )
    });
    // The open path's develop warnings, on the batch outcome — both reply
    // arms, so a partial failure keeps its disclosures too.
    let develops = (!warns.is_empty()).then(|| {
        UiText::with(
            K_LANDING_DEVELOP_WARNING_S,
            &[("n", &warns.len().to_string()), ("detail", &brief_list(warns))],
        )
    });
    BatchWords { base, renamed, relooked, develops }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain export names its file; a repaired base curve — and a mask
    /// warning — ride the same line.
    #[test]
    fn a_single_export_states_the_repair_and_the_mask_warning() {
        let plain = single_words("out/a.jpg", false, None);
        assert_eq!(plain.line.key, "exported → {path}");
        assert_eq!(plain.line.args[0].1, super::super::Arg::Text("out/a.jpg".into()));
        assert!(plain.warn.is_none() && plain.relook.is_none());

        let repaired = single_words("out/a.jpg", true, Some("mask.png"));
        assert!(repaired.relook.expect("the repair is disclosed").key.contains("camera base look re-estimated"));
        let warn = repaired.warn.expect("the skipped masks are disclosed");
        assert_eq!(warn.key, "Some masks could not be drawn, so they are not applied: {why}");
        assert_eq!(warn.args[0].1, super::super::Arg::Text("mask.png".into()));
    }

    /// A batch's base is "done" only when nothing failed; the three
    /// disclosures appear only when present, in their fixed order.
    #[test]
    fn a_batch_names_its_destination_and_each_disclosure_that_applies() {
        let done = batch_words(12, &[], &[], 0, &[], "out");
        assert_eq!(done.base.key, "batch {n} done → {path}");
        assert_eq!(done.base.args[1].1, super::super::Arg::Text("out".into()));
        assert!(done.renamed.is_none() && done.relooked.is_none() && done.develops.is_none());

        let loud = batch_words(
            8,
            &[],
            &["a.jpg → b.jpg".into()],
            2,
            &["w1".into(), "w2".into()],
            "out",
        );
        assert!(loud.renamed.is_some() && loud.relooked.is_some() && loud.develops.is_some());
        assert_eq!(
            loud.develops.expect("develop warnings").args[1].1,
            super::super::Arg::Text("w1 · w2".into())
        );
    }

    /// A partial batch is a failure sentence with both counts and every error.
    #[test]
    fn a_partial_batch_reads_as_a_failure() {
        let partial = batch_words(3, &["a failed".into(), "b failed".into()], &[], 0, &[], "out");
        assert_eq!(partial.base.key, "Batch: {ok} succeeded, {fail} failed: {detail}");
        assert_eq!(partial.base.args[0].1, super::super::Arg::Text("3".into()));
        assert_eq!(partial.base.args[1].1, super::super::Arg::Text("2".into()));
        assert_eq!(partial.base.args[2].1, super::super::Arg::Text("a failed; b failed".into()));
    }
}

#[cfg(test)]
mod disclosure_order_tests {
    use super::*;

    #[test]
    fn a_partial_batch_keeps_every_disclosure_in_its_fixed_order() {
        let words = batch_words(
            2,
            &["a failed".into()],
            &["photo → kept-2".into()],
            1,
            &["warn one".into(), "warn two".into()],
            "out",
        );
        assert_eq!(words.base.key, "Batch: {ok} succeeded, {fail} failed: {detail}");
        assert!(words.renamed.expect("renames disclosed").key.contains("kept apart"));
        assert!(words.relooked.expect("relook disclosed").key.contains("re-estimated"));
        let develops = words.develops.expect("develop warnings disclosed");
        assert_eq!(develops.key, " · {n} develop warning(s): {detail}");
        assert_eq!(develops.args[1].1, super::super::Arg::Text("warn one · warn two".into()));
    }

    #[test]
    fn a_clean_batch_with_warnings_still_reads_as_done() {
        let words = batch_words(4, &[], &[], 0, &["w1".into()], "out");
        assert_eq!(words.base.key, "batch {n} done → {path}");
        assert!(words.renamed.is_none() && words.relooked.is_none());
        assert!(words.develops.is_some(), "a warning is never dropped because the batch succeeded");
    }

    #[test]
    fn a_long_warning_list_is_abbreviated_but_counted() {
        let warns: Vec<String> = (0..5).map(|i| format!("w{i}")).collect();
        let words = batch_words(1, &[], &[], 0, &warns, "out");
        let develops = words.develops.unwrap();
        assert_eq!(develops.args[0].1, super::super::Arg::Text("5".into()), "the count names all five");
        assert_eq!(develops.args[1].1, super::super::Arg::Text("w0 · w1 · w2 …".into()), "the list is abbreviated");
    }
}
