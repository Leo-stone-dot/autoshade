//! The reverse-fit landing's facts and words, headless. The fit itself, the
//! variant it creates and the persist are the runtime's; this module owns what
//! a landed fit says.

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
        "Reverse-fit done: difference from target {before}→{after} (lower = closer) · created a「Reverse-fit」variant (editable / XMP / full-res)",
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
            vec![UiText::of(" · full solve (all develop controls)")]
        }
        FitNote::SolverMode { atmosphere: true } => vec![UiText::of(
            " · mood-only match (the two images differ in content, so only overall tone and colour were matched)",
        )],
        FitNote::SolverPairing { pixel: true, fine, coarse } => vec![UiText::with(
            " · matched pixel to pixel (difference {fine} at pixel scale, {coarse} at layout scale)",
            &[("fine", fine), ("coarse", coarse)],
        )],
        FitNote::SolverPairing { pixel: false, fine, coarse } => vec![UiText::with(
            " · matched by cell statistics only (difference {fine} at pixel scale, {coarse} at layout scale)",
            &[("fine", fine), ("coarse", coarse)],
        )],
        FitNote::IncludesSkyZone => vec![UiText::of(
            " · includes sky-zone correction (editable in the mask panel; native masks export to XMP and remaining losses are named)",
        )],
        FitNote::IncludesRangeMasks => vec![UiText::of(
            " · includes native range correction (adjustable in the mask panel and written to XMP)",
        )],
        FitNote::IncludesSpatialTiles(n) => vec![UiText::with(
            " · includes {n} spatial tile correction(s) (editable in the mask panel; native tiles export to XMP, retained rasters have a named loss)",
            &[("n", &n.to_string())],
        )],
        FitNote::MaskRefinement { kept, abstained } => vec![UiText::with(
            " · guided mask refinement: {kept} kept, {abstained} abstained",
            &[("kept", &kept.to_string()), ("abstained", &abstained.to_string())],
        )],
        FitNote::NotPersistedCommit(e) => vec![UiText::with(
            " · NOT persisted: saving the develop failed ({err}) — Ctrl+S to save explicitly",
            &[("err", e)],
        )],
        FitNote::XmpWritten(p) => {
            vec![UiText::with(" · XMP → {path}", &[("path", &p.display().to_string())])]
        }
        FitNote::XmpMergeNote(m) => vec![UiText::with(" · ⚠ {note}", &[("note", m)])],
        FitNote::XmpFailed(e) => vec![UiText::with(
            "recipe saved — but the Lightroom XMP failed: {err}",
            &[("err", e)],
        )],
        FitNote::BackedUpAs(n) => {
            vec![UiText::with(" · previous save backed up as v{n}", &[("n", &n.to_string())])]
        }
        FitNote::NotPersistedBackup(e) => vec![UiText::with(
            " · NOT persisted: backing up your existing save failed ({err}) — Ctrl+S to save explicitly",
            &[("err", e)],
        )],
        FitNote::NotPersistedLock(e) => vec![UiText::with(
            " · NOT persisted: the develop store could not be locked ({err}) — Ctrl+S to save explicitly",
            &[("err", e)],
        )],
        FitNote::AiReview { score, critique, hint } => {
            let mut pieces = vec![UiText::with(
                " · AI review: match {score}/100 — {critique}",
                &[("score", &format!("{score:.0}")), ("critique", critique)],
            )];
            // The third of the three things the paid call returned, labelled as
            // a SUGGESTION because on this path nothing acted on it.
            if let Some(h) = hint.as_deref().map(str::trim).filter(|h| !h.is_empty()) {
                pieces.push(UiText::with(
                    " · it suggests: {hint} (nothing was changed — tick 「Retry once」 to let it try)",
                    &[("hint", h)],
                ));
            }
            pieces
        }
        FitNote::AiReviewFailed(e) => vec![UiText::with(
            " · AI review unavailable ({err}) — the fit itself already landed",
            &[("err", e)],
        )],
        FitNote::FitReset => vec![UiText::of(
            " · ⚠ THE REVERSE-FIT WAS DISCARDED: every version of it rendered farther from the target than your untouched photo, so the recipe was reset to neutral — this is the same as not having reverse-fitted at all",
        )],
        FitNote::ReferenceNotSameFrame => vec![UiText::of(
            " · ⚠ the reference's proportions do not match this photo — it was cropped, or it is not the same frame; either way the two distributions are not comparable",
        )],
        FitNote::DeepFit { action, outcome } => vec![match outcome {
            DeepFitOutcome::NothingActionable => UiText::of(
                " · retry: the review found nothing this app can act on — the plain fit stands",
            ),
            DeepFitOutcome::ActionDidNotRun => UiText::with(
                " · retry: the review asked for {action}, which could not be carried out — the plain fit stands",
                &[("action", action)],
            ),
            DeepFitOutcome::Adopted => UiText::with(
                " · retry: tried {action} on the review's suggestion and kept it (it re-scored at least as high)",
                &[("action", action)],
            ),
            DeepFitOutcome::Discarded => UiText::with(
                " · retry: tried {action} on the review's suggestion and discarded it (it re-scored lower)",
                &[("action", action)],
            ),
        }],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The done sentence carries both error readings.
    #[test]
    fn the_done_line_names_both_errors() {
        let words = done_words(0.42, 0.11);
        assert!(words.key.starts_with("Reverse-fit done"));
        assert_eq!(words.args[0].1, super::super::Arg::Text("0.420".into()));
        assert_eq!(words.args[1].1, super::super::Arg::Text("0.110".into()));
    }

    /// A landing fact can contribute more than one piece (the AI review and its
    /// hint); most contribute exactly one.
    #[test]
    fn note_pieces_are_one_or_more() {
        assert_eq!(note_words(&FitNote::IncludesSkyZone).len(), 1);
        let review = note_words(&FitNote::AiReview {
            score: 88.0,
            critique: "close".into(),
            hint: Some("raise contrast".into()),
        });
        assert_eq!(review.len(), 2);
        assert!(review[1].key.contains("it suggests"));
        let no_hint = note_words(&FitNote::AiReview {
            score: 70.0,
            critique: "soft".into(),
            hint: Some("   ".into()),
        });
        assert_eq!(no_hint.len(), 1, "a blank hint says nothing");
    }

    /// The XMP-failure sentence is the bare save-landing key (its separator is
    /// the caller's), while the other pieces carry their own " · ".
    #[test]
    fn the_xmp_failure_key_is_the_shared_bare_sentence() {
        assert_eq!(
            note_words(&FitNote::XmpFailed("denied".into()))[0].key,
            "recipe saved — but the Lightroom XMP failed: {err}"
        );
        assert!(note_words(&FitNote::BackedUpAs(3))[0].key.starts_with(" · "));
        assert!(note_words(&FitNote::XmpMergeNote("regenerated".into()))[0].key.starts_with(" · ⚠ "));
    }

    /// The deep path's four outcomes are four different sentences.
    #[test]
    fn deep_retry_outcomes_are_distinct() {
        let keys: Vec<&str> = [
            DeepFitOutcome::NothingActionable,
            DeepFitOutcome::ActionDidNotRun,
            DeepFitOutcome::Adopted,
            DeepFitOutcome::Discarded,
        ]
        .into_iter()
        .map(|outcome| note_words(&FitNote::DeepFit { action: "exposure", outcome })[0].key)
        .collect();
        let unique: std::collections::HashSet<&str> = keys.iter().copied().collect();
        assert_eq!(unique.len(), 4, "each outcome says its own thing: {keys:?}");
    }
}
