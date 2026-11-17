//! Heuristic proposer — a no-AI baseline used when `OPENAI_API_KEY` is unset.
//!
//! It is NOT a placeholder stub: it derives a sane recipe from the histogram
//! (exposure toward a midtone target, highlight/shadow recovery on clipping)
//! so the full propose → verify chain runs and produces something reviewable
//! today, before the GPT vision advisor is wired with a key. It ignores the
//! image content (no vision) and the revision hint.

use crate::decode::{Histogram, Meta};
use crate::recipe::EditRecipe;

use super::{Advisor, AdvisorError, Preview, ProposeContext};

#[derive(Default)]
pub struct HeuristicProposer {
    /// Why the heuristic is standing in for the vision proposer, when the cause
    /// is NOT simply "no key configured". Quoted verbatim in the recipe's
    /// rationale because that string is the only fallback explanation the user
    /// ever sees: the windowed GUI has no console for the caller's stderr
    /// warning, so a hard-coded "OPENAI_API_KEY unset" here would send a user
    /// whose key is fine (quota, network, HTTP error) hunting for the wrong bug.
    pub fallback_reason: Option<String>,
}

impl Advisor for HeuristicProposer {
    fn name(&self) -> &'static str {
        "heuristic"
    }

    fn propose(
        &self,
        _img: &Preview,
        _meta: &Meta,
        hist: &Histogram,
        ctx: &ProposeContext,
    ) -> Result<EditRecipe, AdvisorError> {
        self.propose_noted(hist, ctx.strength).map(|(r, _)| r)
    }
}

impl HeuristicProposer {
    /// The midtone the EV-offset estimator aims the frame's mean luma at, on
    /// the histogram's 0..255 scale (≈ 46 %, a touch above where 18 % grey
    /// encodes in sRGB — a camera-rendered preview's mean sits high).
    pub(crate) const MIDTONE_TARGET: f32 = 118.0;

    /// Mean luma of a histogram, 0..255, floored at 1 so the log in
    /// [`Self::ev_offset_estimate`] stays finite on an all-black frame.
    pub(crate) fn mean_luma(hist: &Histogram) -> f32 {
        let total: u64 = hist.luma.iter().map(|&v| v as u64).sum::<u64>().max(1);
        let weighted: u64 = hist
            .luma
            .iter()
            .enumerate()
            .map(|(i, &v)| i as u64 * v as u64)
            .sum();
        (weighted as f32 / total as f32).max(1.0)
    }

    /// **The EV-offset estimator** (M1_PLAN §8 #12), RAW: how many stops the
    /// frame sits from the midtone target — before the ±1.5 clamp, the
    /// 0.15-stop deadband and the 0.1-stop rounding [`Self::propose_noted`]
    /// applies on top of it.
    ///
    /// Split out as an ASSOCIATED function (the type is already re-exported,
    /// so this needs no new module visibility) purely so the estimator can be
    /// MEASURED against real photographs instead of only being observed
    /// through the recipe it feeds — `eval`'s `AUTOSHADE_ESTIMATOR_PROBE`.
    /// `propose_noted` is its only production caller and gets exactly the
    /// number it used to compute inline.
    pub(crate) fn ev_offset_estimate(hist: &Histogram) -> f32 {
        (Self::MIDTONE_TARGET / Self::mean_luma(hist)).log2()
    }

    /// The concrete entry `produce_recipe` calls (it holds this type, not a
    /// `dyn Advisor`): the recipe PLUS its rationale as a typed note, so the
    /// GUI can render the baseline explanation in the session language. The
    /// recipe's `rationale` string is `render_one(&note)` byte-for-byte —
    /// the L12#2B suffix contract with an empty prose prefix.
    pub fn propose_noted(
        &self,
        hist: &Histogram,
        strength: crate::recipe::GradeStrength,
    ) -> Result<(EditRecipe, crate::rationale::Note), AdvisorError> {
        let mean = Self::mean_luma(hist); // 0..255

        let mut r = EditRecipe::default();

        // Nudge exposure toward a midtone target of ~118/255, capped to ±1.5 EV.
        // Deadband: leave exposure untouched for sub-0.15-stop corrections — a
        // near-neutral frame doesn't need a trivial (and visually pointless)
        // nudge whose sign looks arbitrary.
        let ev_raw = Self::ev_offset_estimate(hist).clamp(-1.5, 1.5);
        r.exposure_ev = if ev_raw.abs() < 0.15 {
            0.0
        } else {
            (ev_raw * 10.0).round() / 10.0
        };

        // Recover blown highlights / lifted-but-clipped blacks proportionally.
        if hist.clip_white_pct > 0.5 {
            r.highlights = -(hist.clip_white_pct * 8.0).min(70.0);
            r.whites = -(hist.clip_white_pct * 4.0).min(40.0);
        }
        if hist.clip_black_pct > 1.0 {
            r.shadows = (hist.clip_black_pct * 6.0).min(60.0);
        }

        // Mild, conservative default "presence".
        r.contrast = 8.0;
        r.vibrance = 8.0;
        r.clarity = 4.0;

        r.confidence = 0.4;
        r.clamp();
        // GATE 6 of the strength axis (R23-3): the same guardrail as the AI path,
        // now on the same dial as the AI path. The baseline's own presence values
        // (contrast 8 / vibrance 8 / clarity 4) sit far below every soft-cap knee,
        // so the dial reaches this recipe only through the histogram-driven
        // recovery above — a heavily clipped frame drives Highlights to −70 and
        // Shadows to +60, where the knee decides the final number. Threading it
        // anyway is the point: a fallback that tastes different from the AI path
        // at the same dial setting is its own reported bug.
        r.temper(strength);

        // Rationale formatted AFTER clamp+temper: both can move the very
        // numbers it quotes (temper soft-caps recovery strength), and a
        // rationale that contradicts the recipe's own values reads as a bug —
        // a real verifier run flagged exactly that mismatch.
        use crate::rationale::{keys, render_one, Note};
        let mut args = vec![
            ("mean", format!("{mean:.0}")),
            ("clip_b", format!("{:.1}", hist.clip_black_pct)),
            ("clip_w", format!("{:.1}", hist.clip_white_pct)),
            ("ev", format!("{:+.1}", r.exposure_ev)),
            ("hl", format!("{:.0}", r.highlights)),
            ("sh", format!("{:.0}", r.shadows)),
        ];
        let note = match &self.fallback_reason {
            Some(e) => {
                args.push(("e", crate::rationale::error_line(&anyhow::anyhow!("{}", e))));
                Note::new(keys::HEURISTIC_UNAVAILABLE, args)
            }
            None => Note::new(keys::HEURISTIC_NO_KEY, args),
        };
        r.rationale = render_one(&note);
        Ok((r, note))
    }
}
