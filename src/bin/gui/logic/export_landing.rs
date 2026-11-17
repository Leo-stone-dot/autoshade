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
