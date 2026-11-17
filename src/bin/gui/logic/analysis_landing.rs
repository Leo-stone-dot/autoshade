//! The analysis LANDING's decisions and words, headless.
//!
//! An analyzed result arrives as recipe + verdict + rationale notes. What
//! happens to that recipe (and what the user is told) is decided here; the
//! persistence itself (backup gate, one develop commit, XMP half) stays the
//! runtime's, which is why it reports back as a [`SaveReport`] instead of this
//! layer reaching for a lock or a file.
use autoshade::text::generated::*;


use super::{Arg, Tone, UiText, Words};
use autoshade::advisor::{Decision, Verdict};
use autoshade::recipe::EditRecipe;

/// Does this verdict let the app SAVE the result? Only `Accept` may auto-save;
/// a result the verifier judged not ready stays on the canvas as an unsaved
/// edit and the user decides (Ctrl+S / Ctrl+Z).
pub(crate) fn accepted(verdict: &Verdict) -> bool {
    verdict.decision == Decision::Accept
}

/// The landing's two recipes: `canvas` is what the frame shows (a GENERATED
/// card strips the camera look and lens profile — they are already IN its
/// pixels), `stamped` is what the develop store writes (the RAW's own develop,
/// which keeps its curve).
pub(crate) struct Shaped {
    pub(crate) canvas: EditRecipe,
    pub(crate) stamped: EditRecipe,
}

/// Shape a landed recipe for the canvas and for the store.
///
/// "Keep skin natural" holds where the landing is WRITTEN: the store write
/// happens before the frame's own guard reaches the canvas, so the clamp is
/// applied here to the recipe that is saved.
pub(crate) fn shape(recipe: EditRecipe, skin_held: bool, ai_pixels: bool) -> Shaped {
    let mut stamped = recipe;
    if skin_held {
        autoshade::face::keep_skin_natural_in(&mut stamped);
    }
    let mut canvas = stamped.clone();
    if ai_pixels {
        canvas.strip_baked_start();
        // Anchor follows the strip rule (baked WB).
        canvas.as_shot_k = None;
        canvas.as_shot_tint = None;
    }
    Shaped { canvas, stamped }
}

/// How the persistence half went, as data the wording below can consume.
pub(crate) enum SaveReport {
    /// The verifier did not accept: nothing is persisted.
    NotAccepted { decision: &'static str },
    Saved { backed_up: Option<u32> },
    BackupFailed { err: String },
    SidecarFailed { err: String },
    NoPhoto,
}

/// The words for one landing outcome. The decision word travels as a
/// catalogue KEY (`Arg::Key`), translated where the line is rendered.
pub(crate) fn words(report: &SaveReport) -> Words {
    match report {
        SaveReport::NotAccepted { decision } => {
            let text = UiText::with_args(
                K_LANDING_AI_EDIT_APPLIED_THE_AI_S_SELF_CHECK_SAID_NOT_SAV,
                &[("v", Arg::Key(decision.into()))],
            );
            Words { status: text.clone(), toast: Some((Tone::Success, text)) }
        }
        SaveReport::Saved { backed_up: Some(n) } => Words {
            status: UiText::with(
                K_LANDING_AI_DEVELOP_APPLIED_SAVED_PREVIOUS_SAVE_BACKED_UP,
                &[("n", &n.to_string())],
            ),
            toast: None,
        },
        SaveReport::Saved { backed_up: None } => Words {
            status: UiText::of(K_LANDING_AI_DEVELOP_APPLIED_SAVED_TO_RECIPE_JSON),
            toast: None,
        },
        SaveReport::BackupFailed { err } => {
            let text = UiText::with(
                K_LANDING_AI_DEVELOP_APPLIED_BUT_NOT_SAVED_BACKING_UP_YOUR,
                &[("err", err)],
            );
            Words { status: text.clone(), toast: Some((Tone::Error, text)) }
        }
        SaveReport::SidecarFailed { err } => {
            let text = UiText::with(
                K_LANDING_AI_DEVELOP_APPLIED_BUT_SAVING_THE_SIDECAR_FAILED,
                &[("err", err)],
            );
            Words { status: text.clone(), toast: Some((Tone::Error, text)) }
        }
        SaveReport::NoPhoto => Words { status: UiText::of(K_LANDING_AI_DEVELOP_APPLIED), toast: None },
    }
}
