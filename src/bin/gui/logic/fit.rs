//! The reverse-fit landing's facts and words, headless. The fit itself, the
//! variant it creates and the persist are the runtime's; this module owns what
//! a landed fit says.
use autoshade::text::generated::*;


use super::UiText;

/// One reverse-fit landing fact (L12#4): the worker records WHAT happened
/// on the persist path; `render_fit_note` (workers.rs) translates it when
/// the result lands. Owned args only — stringified errors, paths, counts.
pub(crate) enum FitNote {
    /// The finished fit includes a semantic sky/land correction, possibly bands.
    IncludesSkyZone,
    /// The fit contains at least one native range mask.
    IncludesRangeMasks,
    /// This many spatial tiles are present, as native geometry or refined rasters.
    IncludesSpatialTiles(usize),
    /// Complete guided-refinement outcomes across semantic/tile masks.
    MaskRefinement { kept: usize, abstained: usize },
    /// `commit_develop` failed — the fit stays on the canvas unsaved.
    NotPersistedCommit(String),
    /// The Lightroom XMP sidecar landed at this path.
    XmpWritten(std::path::PathBuf),
    /// write_xmp's regenerated-not-merged disclosure (library English).
    XmpMergeNote(String),
    /// recipe.json persisted but the XMP write failed.
    XmpFailed(String),
    /// The previous explicit save was snapshotted as v{n} first.
    BackedUpAs(u32),
    /// The backup gate refused — nothing was written.
    NotPersistedBackup(String),
    /// The develop store lock could not be taken.
    NotPersistedLock(String),
    /// R20 opt-in AI review: the vision judge scored how faithfully the
    /// fitted render matches the target look. Critique is model English
    /// (the rationale contract — args ride verbatim, like error text).
    /// The judgement's accept/revise decision is deliberately NOT carried:
    /// the prompt pins its thresholds to the score (accept = 85+), so the
    /// score already says it — a second word would just need translating.
    ///
    /// `hint` is the reviewer's ONE actionable instruction. R20 dropped it
    /// on the floor — the ROADMAP entry gave a reason for dropping
    /// `decision` and none at all for this — so the user paid for a vision
    /// call and was shown two thirds of what it said. It is displayed, never
    /// executed, on the default path; the deep path (R23-6 D) additionally
    /// uses it to pick an ACTION, never to write a value.
    AiReview { score: f32, critique: String, hint: Option<String> },
    /// The AI review call failed — the fit itself already landed; this is
    /// the informational layer degrading, never the fit erroring.
    AiReviewFailed(String),
    /// R33 §I: which solver ran. The mode decides whether white balance,
    /// channel curves and the paired estimators exist at all, and until now
    /// the status line said nothing about it — two fits of the same pair from
    /// two entry points landed in different modes and read identical.
    SolverMode { atmosphere: bool },
    /// R33 §I: the two structural readings and the pairing scale they bought.
    /// Strings, not floats: they are already formatted (and may read
    /// "unmeasured"), and a landing-time format would be a second rounding.
    SolverPairing { pixel: bool, fine: String, coarse: String },
    /// R23-6 A-3: the terminal do-no-harm fired and the whole recipe was
    /// reset to the calibration base. A rationale line was never enough for
    /// this one — it is the "the reverse-fit did nothing" outcome, and it
    /// belongs in the status line in those words.
    FitReset,
    /// R23-6 B-7: the chosen reference does not look like this frame.
    ReferenceNotSameFrame,
    /// R23-6 D: the deep path ran and this is what it did. `action` names the
    /// move the review's hint selected; [`DeepFitOutcome`] says what became of
    /// it.
    DeepFit { action: &'static str, outcome: DeepFitOutcome },
}

/// What the deep reverse-fit's ONE guided retry came to.
///
/// Four states, not the `{rounds, adopted}` pair this used to be (R23 review
/// LOW-4). `rounds == 0` had THREE producers — the hint named no move the app
/// has, the zoned re-solve failed, or the saturation step clamped to the value
/// the recipe already carried — and all three rendered as "the review found
/// nothing this app can act on", which is only true of the first. The other two
/// are the app failing to carry out a move it DID select, and telling the user
/// the reviewer had no opinion hides that.
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) enum DeepFitOutcome {
    /// The hint named no move this app can make (`FitAction::None`).
    NothingActionable,
    /// A move WAS selected and could not be carried out: the zoned re-solve
    /// errored (no raster home, segmentation unavailable), or the saturation
    /// step landed on the value the recipe already had after clamping. No
    /// second review was bought either way.
    ActionDidNotRun,
    /// The retry was bought and KEPT — it re-scored at least as high.
    Adopted,
    /// The retry was bought and discarded — it re-scored lower.
    Discarded,
}

/// The main sentence of a finished fit.
pub(crate) fn done_words(err_before: f32, err_after: f32) -> UiText {
    UiText::with(
        K_LOGIC_REVERSE_FIT_DONE_DIFFERENCE_FROM_TARGET_LOWER_CL,
        &[
            ("before", &format!("{err_before:.3}")),
            ("after", &format!("{err_after:.3}")),
        ],
    )
}

/// The pieces one landing fact contributes to the status line. Each carries its
/// own leading " · " — except the XMP-failure sentence, whose catalogue key is
/// the bare sentence the save landing shares (the runtime adds its separator).
pub(crate) fn note_words(n: &FitNote) -> Vec<UiText> {
    match n {
        FitNote::SolverMode { atmosphere: false } => {
            vec![UiText::of(K_LOGIC_FULL_SOLVE_ALL_DEVELOP_CONTROLS)]
        }
        FitNote::SolverMode { atmosphere: true } => vec![UiText::of(
            K_LOGIC_MOOD_ONLY_MATCH_THE_TWO_IMAGES_DIFFER_IN_CONTENT,
        )],
        FitNote::SolverPairing { pixel: true, fine, coarse } => vec![UiText::with(
            K_LOGIC_MATCHED_PIXEL_TO_PIXEL_DIFFERENCE_AT_PIXEL_SCALE,
            &[("fine", fine), ("coarse", coarse)],
        )],
        FitNote::SolverPairing { pixel: false, fine, coarse } => vec![UiText::with(
            K_LOGIC_MATCHED_BY_CELL_STATISTICS_ONLY_DIFFERENCE_AT_PI,
            &[("fine", fine), ("coarse", coarse)],
        )],
        FitNote::IncludesSkyZone => vec![UiText::of(
            K_LOGIC_INCLUDES_SKY_ZONE_CORRECTION_EDITABLE_IN_THE_MAS,
        )],
        FitNote::IncludesRangeMasks => vec![UiText::of(
            K_LOGIC_INCLUDES_NATIVE_RANGE_CORRECTION_ADJUSTABLE_IN_T,
        )],
        FitNote::IncludesSpatialTiles(n) => vec![UiText::with(
            K_LOGIC_INCLUDES_SPATIAL_TILE_CORRECTION_S_EDITABLE_IN_T,
            &[("n", &n.to_string())],
        )],
        FitNote::MaskRefinement { kept, abstained } => vec![UiText::with(
            K_LOGIC_GUIDED_MASK_REFINEMENT_KEPT_ABSTAINED,
            &[("kept", &kept.to_string()), ("abstained", &abstained.to_string())],
        )],
        FitNote::NotPersistedCommit(e) => vec![UiText::with(
            K_LOGIC_NOT_PERSISTED_SAVING_THE_DEVELOP_FAILED_CTRL_S_T,
            &[("err", e)],
        )],
        FitNote::XmpWritten(p) => {
            vec![UiText::with(K_LOGIC_XMP, &[("path", &p.display().to_string())])]
        }
        FitNote::XmpMergeNote(m) => vec![UiText::with(K_LOGIC_TEXT, &[("note", m)])],
        FitNote::XmpFailed(e) => vec![UiText::with(
            K_EXPORT_RECIPE_SAVED_BUT_THE_LIGHTROOM_XMP_FAILED,
            &[("err", e)],
        )],
        FitNote::BackedUpAs(n) => {
            vec![UiText::with(K_LOGIC_PREVIOUS_SAVE_BACKED_UP_AS_V, &[("n", &n.to_string())])]
        }
        FitNote::NotPersistedBackup(e) => vec![UiText::with(
            K_LOGIC_NOT_PERSISTED_BACKING_UP_YOUR_EXISTING_SAVE_FAIL,
            &[("err", e)],
        )],
        FitNote::NotPersistedLock(e) => vec![UiText::with(
            K_LOGIC_NOT_PERSISTED_THE_DEVELOP_STORE_COULD_NOT_BE_LOC,
            &[("err", e)],
        )],
        FitNote::AiReview { score, critique, hint } => {
            let mut pieces = vec![UiText::with(
                K_LOGIC_AI_REVIEW_MATCH_100,
                &[("score", &format!("{score:.0}")), ("critique", critique)],
            )];
            // The third of the three things the paid call returned, labelled as
            // a SUGGESTION because on this path nothing acted on it.
            if let Some(h) = hint.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
                pieces.push(UiText::with(
                    K_LOGIC_IT_SUGGESTS_NOTHING_WAS_CHANGED_TICK_RETRY_ONCE,
                    &[("hint", h)],
                ));
            }
            pieces
        }
        FitNote::AiReviewFailed(e) => vec![UiText::with(
            K_LOGIC_AI_REVIEW_UNAVAILABLE_THE_FIT_ITSELF_ALREADY_LAN,
            &[("err", e)],
        )],
        FitNote::FitReset => vec![UiText::of(
            K_LOGIC_THE_REVERSE_FIT_WAS_DISCARDED_EVERY_VERSION_OF_I,
        )],
        FitNote::ReferenceNotSameFrame => vec![UiText::of(
            K_LOGIC_THE_REFERENCE_S_PROPORTIONS_DO_NOT_MATCH_THIS_PH,
        )],
        FitNote::DeepFit { action, outcome } => vec![match outcome {
            DeepFitOutcome::NothingActionable => UiText::of(
                K_LOGIC_RETRY_THE_REVIEW_FOUND_NOTHING_THIS_APP_CAN_ACT,
            ),
            DeepFitOutcome::ActionDidNotRun => UiText::with(
                K_LOGIC_RETRY_THE_REVIEW_ASKED_FOR_WHICH_COULD_NOT_BE_CA,
                &[("action", action)],
            ),
            DeepFitOutcome::Adopted => UiText::with(
                K_LOGIC_RETRY_TRIED_ON_THE_REVIEW_S_SUGGESTION_AND_KEPT,
                &[("action", action)],
            ),
            DeepFitOutcome::Discarded => UiText::with(
                K_LOGIC_RETRY_TRIED_ON_THE_REVIEW_S_SUGGESTION_AND_DISCA,
                &[("action", action)],
            ),
        }],
    }
}
