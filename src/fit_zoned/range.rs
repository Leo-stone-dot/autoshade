use super::*;
use super::field::FieldBandProposal;

/// Four bands is the existing measured stability ceiling for value-range
/// evidence; finer partitions routinely fall below the evidence floor.
pub(super) const RANGE_MAX_BANDS: usize = 4;
/// Corrected rank-mean calibration keeps `0.03` between supported neutral
/// bins (01-07 and 12, at most `0.025`) and the coherent 08-11 run (starting
/// at `0.036`); the isolated supported bin 13 measures `0.223`.
const RANGE_RESIDUAL_TRIGGER: f32 = 0.03;
/// One 17-bin evidence interval is the minimum transition width; the retained
/// opposite-half-EV probe measured a 5/255 step when that protection vanished.
const RANGE_MIN_RAMP: f32 = 1.0 / 17.0;
/// Two evidence intervals provide measured transition headroom before the
/// shared boundary shrink has to reduce correction differentials.
///
/// # The re-cut probe, and what the widest ramp actually does
///
/// v1.2.3 could not measure this width. Its synthetic probe was 512 rows and
/// the mask-free instrument (`scripts/rim_overshoot.py`) needs 180 px of
/// margin on each side of the boundary it locates, so every column of the 2/17
/// rows was rejected and the table carried "unmeasurable", not "measured
/// clean". That probe also pinned the band at ONE luminance position.
///
/// The probe is re-cut in
/// `tests::the_recut_ramp_probe_measures_every_ramp_the_producer_emits`: a
/// 64 x 1020 vertical grey ramp, a 4/17-wide band, the instrument's own
/// 60/60/60 geometry ported unchanged, and the band's position swept across
/// five values. Every cell now returns n = 64 of 64 columns, including the
/// 2/17 rows nothing could read before.
///
/// TWO RULERS, AND THEY DISAGREE BY CONSTRUCTION. The mask-free overshoot
/// reads mean 0.0000 and max 0.0000 in ALL 90 cells — every ramp, every EV,
/// every position — because the DIFFERENCE it ranks stays monotone even where
/// the delivered luma does not. The tone-order reading
/// ([`range_transfer_reversal`], the depth of the delivered transfer's
/// non-monotone excursion) is the ruler that sees the rest, and its control
/// (the same frame at 0.00 EV) is exactly 0.000000 in all 15 of its cells.
/// Depths below, in 8-bit codes, columns = band centre luma:
///
/// ```text
/// ramp 1/17    0.36 0.43 0.50 0.57 0.64     ramp 1.5/17   0.36 0.43 0.50 0.57 0.64
///  -0.35 EV       0    0    0    1    2      -0.35 EV        0    0    0    0    0
///  -0.56 EV       0    1    4    7    8      -0.56 EV        0    0    1    2    3
///  -0.80 EV       3    6    9   13   17      -0.80 EV        0    1    5    7   10
///  -1.10 EV       7   12   16   21   26      -1.10 EV        2    7   11   15   19
///  -1.50 EV      13   19   25   31   38      -1.50 EV        8   13   18   25   31
///
/// ramp 2/17 (THIS CONSTANT)   0.36 0.43 0.50 0.57 0.64
///  -0.35 EV                      0    0    0    0    0
///  -0.56 EV                      0    0    0    0    0
///  -0.80 EV                      0    0    1    3    6
///  -1.10 EV                      0    2    5    9   14
///  -1.50 EV                      3    7   13   19   24
/// ```
///
/// So the widest ramp is the SAFEST of the three at every cell — which is the
/// case for this ceiling and not against it — but it is not safe: at -0.80 EV
/// and above it inverts too, and the position sweep is what shows it (the same
/// EV reads 0 codes low in the frame and 6 high in it). The viaduct's real 2/17
/// band at -0.80 EV sits at codes 0-30, the bottom of that row, which is why it
/// measures clean. Nothing here asks for a different width: a wider ramp is
/// monotonically better on this table and the ceiling is already the widest the
/// producer emits, while the inversions are closed by the sign test on the
/// delivered transfer ([`RANGE_TRANSFER_REVERSAL_MAX`]) rather than by a re-tune.
const RANGE_MAX_RAMP: f32 = 2.0 / 17.0;
/// Native range transitions reuse the calibrated zoned signed-rim budget
/// — MEASURED as defensible in that role on 2026-09-01, no longer carried
/// over on the shared number alone.
///
/// THIS RULER IS NOT `boundary_step`. It never differences the scene away:
/// `range_transition_rim` admits a neighbouring pair only where the
/// REFERENCE crossing is already smooth (`|dl| <= 2.5/255`) and then
/// reports the RENDERED gradient there, so the scene's own gradient is
/// INSIDE the reading and is spent before the correction adds anything.
/// That argument is not new — the comment inside `range_transition_rim`
/// has said since v1.2.2 that a graded context here is capped near 2.5/255
/// by construction and so has no dynamic range. What this block adds is
/// the MEASUREMENT behind it and a test that pins the scale.
///
/// CHARGING HERE WOULD OVER-TIGHTEN, NOT LOOSEN. v1.2.2's charge is
/// `raw * (MAX / budget)` with `budget` clamped at or below `MAX`
/// (`fit_zoned.rs`, `boundary_step`), so the multiplier is >= 1 by
/// construction and can never weaken a gate. For THIS ruler the context
/// term is the admitted crossing itself, capped at 2.5/255 = 0.0098 —
/// always under this 0.012 ceiling — so every crossing would be charged at
/// least 1.22x and the pass condition would collapse to
/// `scene + correction <= scene`: no steepening admitted anywhere, which
/// refuses nearly every band. The tile seam v1.2.2 fixed is the opposite
/// failure mode: the step ruler CANCELS the scene and then handed clean
/// sky the same flat budget as texture, which is why that ruler had to be
/// charged per crossing and this one must not be.
///
/// WHAT THE BUDGET BUYS IS A p90, NOT A PER-CROSSING CAP. The reading is
/// the 90th percentile of |signed bow| (`ZONE_BOUNDARY_PERCENTILE`), so a
/// tenth of the crossings sit above it, ungoverned. At the ranked crossing:
/// the widest crossing the window admitted on either real pair measures
/// 0.00978 luma (2.49 codes) against this budget's 3.06, so where the scene
/// already fills the window a correction adds 0.57 of a code and no more,
/// and where it is flat the ceiling is the whole 3.06 — 1.22x the steepest
/// crossing the window is willing to call smooth. The measured MAXIMA are
/// above both, as a p90 gate permits: calibration 0.00978 -> 0.01217
/// (1.24x, +0.61 code), viaduct 0.00978 -> 0.01407 (1.44x, +1.09 codes,
/// past 0.012 itself).
///
/// First-party on the real pairs, segmentation and correspondence made
/// unavailable so `match --zoned` takes this fallback. TWO RASTERS are in
/// play and every number below says which. The ENGINE gates on the 384-px
/// thumbnail it develops itself (its crossing counts appear in the
/// rationale); the readings are a transcription of this same statistic
/// over a full-resolution develop downsampled to a 384-px long edge. The
/// reference the engine passes is `&global_px` — the TWIN, global
/// correction with no range masks — so the transcription uses the twin as
/// its basis too.
///
/// * calibration pair, band luma [0.471, 0.765] at -0.56 EV: gate k=1.000
///   over 18651 engine crossings. Transcribed over 18297 crossings, twin
///   basis: uncorrected p90 0.00392 (1.00 code, max 0.00978), delivered
///   p90 0.00230 (0.59, max 0.01217).
/// * stone-viaduct pair, band luma [0.118, 0.882] at -0.80 EV and
///   saturation +23: gate k=1.000 over 1214 engine crossings. Transcribed
///   over 1224, twin basis: uncorrected p90 0.00874 (2.23 codes, max
///   0.00978), delivered p90 0.00857 (2.19, max 0.01407) — the band moved
///   the ranked reading DOWN. Read against the no-masks base instead of
///   the twin the same pair gives 0.00874 over 1272; that is not the frame
///   the gate sees.
/// * the Cornwall pair attaches no band at all: one is dropped by the
///   do-no-harm arm and two abstain, so it carries no reading either way.
///
/// The delivered transitions are RAMPS, measured and not assumed, with the
/// instrument's own noise beside them. Over the populated 1-code bins of
/// the delivered transfer (twin luma to delivered luma, 1200 px) the
/// calibration band shows 0 reversals in 30 bins, minimum slope +0.019 and
/// maximum +0.964 — COMPRESSIVE: 30 input codes into 9.9 delivered ones,
/// with 9 of the 30 bins under slope 0.1 — and the viaduct 0 reversals in
/// 19 bins with +0.855. The same estimator over the zero-weight control
/// region, where the true slope is exactly 1.000, reads +0.942 to +1.039:
/// a +/-0.05 noise floor, so +0.019 is one noise unit from a reversal.
/// Both pairs therefore pass the sign test v1.2.4 added
/// ([`RANGE_TRANSFER_REVERSAL_MAX`]) — driven end to end, the calibration
/// band reads 0.0001 luma against the 0.0020 allowed — and the gate carries
/// that reading beside this one.
/// The mask-free `scripts/rim_overshoot.py` reads mean 0.0006 /
/// p90 0.0018 / max 0.0082 at the calibration transition against its own
/// control of exactly 0.0000; it is NOT applicable at the viaduct's
/// contour and says so in its own numbers — its 60 px plateau windows must
/// bracket the transition, and there the band's spread of 40.4 codes
/// exceeds the plateau gap of 22.1 on 201 of 231 columns.
///
/// THE SEAM-STYLE READING IS BASIS-DEPENDENT HERE, WHICH IS WHY IT DOES
/// NOT DECIDE. Stepping across the calibration band's contours in 8-bit
/// codes at 1200 px, the shape v1.2.2 used on the tile, all four measured
/// rows: lo_outer +9.05 codes at z +10.08 on the twin basis but +2.83 at
/// z +1.94 on the neutral one; lo +7.18 at z +6.99 twin and +8.28 at
/// z +8.92 neutral (p90 |.| 22.84 codes, 26.8x the control's sd of 0.853).
/// A 5x swing at ONE contour between two honest bases is itself the
/// evidence. A range edge IS an iso-luminance contour of the photograph,
/// so a difference across it is the correction doing its job, and the
/// transfer above shows the ramp COMPRESSING (dT max +0.964 inside it):
/// those 9 codes are the scene's own gradient being flattened, not an
/// induced step. A tile boundary is an arbitrary rectangle over continuous
/// sky, where any step is an artefact — which is why the same statistic
/// decided v1.2.2 and cannot decide this.
///
/// WHAT THIS RULER DOES NOT SAY, AND WHAT NOW SAYS IT. This ruler ranks
/// MAGNITUDE, so it cannot tell a preserved gradient from an inverted one
/// of the same size, and neither can the mask-free ruler, because the
/// DIFFERENCE it ranks stays monotone even where the delivered luma does
/// not. v1.2.3 measured that gap on a synthetic ramp and left it open;
/// v1.2.4 closes it with a second reading rather than a re-tune of this
/// number. [`RANGE_TRANSFER_REVERSAL_MAX`] carries the sign test, the
/// re-cut probe behind it carries the table (see [`RANGE_MAX_RAMP`], where
/// every ramp the producer emits is now measured at five band positions,
/// and the mask-free ruler reads 0.0000 in all 90 cells while the tone
/// order inverts by up to 38 codes), and `enforce_range_boundary_gate`
/// applies both readings as one verdict: a band that inverts the tone order
/// is shrunk to the largest amount that does not, and both readings ride
/// the disclosure. This ceiling is unchanged and is not what was wrong.
const RANGE_BOUNDARY_RIM_MAX: f32 = 0.012;
/// Ceiling on the depth of a TONE REVERSAL the delivered transfer may carry
/// across a band's edges, in luma. `RANGE_BOUNDARY_RIM_MAX` above ranks
/// MAGNITUDE, so it cannot tell a preserved gradient from an inverted one of
/// the same size; this is the ruler that can, and the two together are what
/// "the delivered transition is a ramp" means.
///
/// MEASURED, on the re-cut synthetic probe
/// (`the_recut_ramp_probe_measures_every_ramp_the_producer_emits`) and on the
/// two real pairs that attach a band:
///
/// * The estimator's own floor is EXACTLY 0. On the probe's zero-strength
///   control — the same frame, the same band, 0.00 EV — the delivered transfer
///   is the identity and this reading is 0.000000 on all 15 (ramp, position)
///   cells. It is a running-maximum depth, not a slope, so per-bin noise has to
///   accumulate before it registers, and on an unmoved render there is none.
/// * The real pairs sit a long way under the line. Driven end to end on
///   2026-09-02 — `match --zoned` on the calibration pair with segmentation
///   pointed at nothing, so the band path is the one that attaches — the
///   accepted band [0.471, 0.765] at -0.56 EV reads 0.0001 luma of fall-back,
///   0.026 of an 8-bit code, one twentieth of this ceiling, beside a rim of
///   0.002 over 18651 crossings; the band attaches unchanged at k = 1.000.
///   v1.2.3's slope table said the same thing with a different estimator: 0
///   reversals over the calibration band's 30 populated 1-code bins and the
///   viaduct's 19.
/// * The probe's smallest REAL inversion is 1 code = 0.0039 luma (a 1/17 band
///   at -0.35 EV, v1.2.3's table), and this batch's own sweep reproduces it.
///
/// Half a code is therefore the line: it is above a floor of exactly zero and
/// below the smallest inversion any configuration has produced, and half an
/// 8-bit code is under the quantisation of the render this gate measures, so
/// nothing it admits can be drawn.
const RANGE_TRANSFER_REVERSAL_MAX: f32 = 0.5 / 255.0;
/// How close two neighbouring REFERENCE pixels have to be, in the coordinate
/// a band's mask ramps along, before the crossing between them counts as a
/// smooth one a correction may be charged for. Two-and-a-half 8-bit levels
/// preserves the retained smooth-gradient stress while excluding real subject
/// edges. The number is unchanged: it was spelled inline inside
/// [`range_transition_rim`] while luma was the only coordinate, and it is a
/// constant now because a colour band asks the same question in its own.
const RANGE_SMOOTH_CROSSING: f32 = 2.5 / 255.0;
/// Native bands reuse the global evidence model's measured 1.5% population
/// floor; a smaller interval is not a two-sided measurement.
pub(super) const RANGE_MIN_EVIDENCE_SHARE: f32 = 0.015;
/// Range bands must pay for themselves on the composed evidence-weighted
/// frame: the live regression measured `0.018 -> 0.024` after two bands.
/// Equality is acceptable, but any worse frame restores the running recipe.
const RANGE_FRAME_REGRESSION_TOL: f32 = 0.0;

const RANGE_HOST: MaskGeometry = MaskGeometry::Linear {
    zero_x: 0.5,
    zero_y: -0.8,
    full_x: 0.5,
    full_y: -0.4,
};

/// The coordinate ONE range mask's weight ramps along.
///
/// A boundary reading only means something in that coordinate. A luminance
/// band ramps over an interval of luma, so a step it induces is a luma step.
/// A colour band ramps over a shell of chromaticity distance around its own
/// reference colour, so a step it induces is a chromaticity step, and the luma
/// ruler reads 0.000 across an edge that is plainly visible: a red pixel beside
/// a green one of the same luma differs by nothing the luma ruler measures.
///
/// So the family keeps ONE rule and reads it in each band's own units. Two
/// neighbours are admitted when the REFERENCE separates them by at most
/// [`RANGE_SMOOTH_CROSSING`] here; they belong to a transition when their
/// reference midpoint falls inside that transition's ramp; and the reading is
/// how far apart the RENDER puts the same two pixels. Instantiated on
/// [`Self::Luma`] this is, arithmetically, the reading the rim gate has taken
/// since v1.2.2 -- the p90 ranks magnitudes, so the luma arm's sign never
/// reached the verdict.
#[derive(Clone, Copy, Debug, PartialEq)]
enum RangeSelector {
    /// Rec.601 display luma: the coordinate `RangeMask::Luminance` ramps in.
    Luma,
    /// Brightness-independent distance from a colour band's reference colour:
    /// the exact quantity `render::range_weight` ramps a `RangeMask::Color`
    /// over ([`render::chromaticity_distance`]).
    Chromaticity([f32; 3]),
}

impl RangeSelector {
    /// Where one pixel sits along this coordinate. Non-finite when the pixel
    /// carries no reading here, which for chromaticity is a pixel too dark to
    /// have one; callers skip those samples rather than inventing a position.
    fn position(self, px: &[f32; 3]) -> f32 {
        match self {
            Self::Luma => display_luma(px),
            Self::Chromaticity(reference) => render::chromaticity_distance(&reference, px),
        }
    }

    /// How far apart two pixels are along this coordinate. The luma arm keeps
    /// the signed difference the rim gate has always taken; the chromaticity
    /// arm is the same distance function applied BETWEEN the two pixels
    /// instead of between one pixel and the band's colour, and is never
    /// negative. Both are ranked by magnitude below.
    fn separation(self, a: &[f32; 3], b: &[f32; 3]) -> f32 {
        match self {
            Self::Luma => display_luma(b) - display_luma(a),
            Self::Chromaticity(_) => render::chromaticity_distance(a, b),
        }
    }
}

/// One ramp of one mask: the coordinate it ramps in, and the interval over
/// which its weight changes.
#[derive(Clone, Copy, Debug)]
struct RangeTransition {
    selector: RangeSelector,
    lo: f32,
    hi: f32,
}

/// Every ramp a range mask carries. A luminance trapezoid has two, and a hard
/// edge contributes none exactly as before; a colour ball has one, from the
/// distance where its weight starts falling to the one where it reaches zero.
fn range_transitions(range: &RangeMask) -> Vec<RangeTransition> {
    match *range {
        RangeMask::Luminance { lo_outer, lo, hi, hi_outer } => [(lo_outer, lo), (hi, hi_outer)]
            .into_iter()
            .filter(|(a, b)| a.is_finite() && b.is_finite() && b > a)
            .map(|(lo, hi)| RangeTransition { selector: RangeSelector::Luma, lo, hi })
            .collect(),
        RangeMask::Color { r, g, b, amount, .. } => {
            let tolerance = render::colour_range_tolerance(amount);
            [(0.5 * tolerance, tolerance)]
                .into_iter()
                .filter(|(a, b)| a.is_finite() && b.is_finite() && b > a)
                .map(|(lo, hi)| RangeTransition {
                    selector: RangeSelector::Chromaticity([r, g, b]),
                    lo,
                    hi,
                })
                .collect()
        }
    }
}

/// The footprint the ORDER reading walks, in the coordinate that has an
/// order. `None` for a colour band, and that is a measured decision rather
/// than an omission.
///
/// [`range_transfer_reversal`] asks whether the delivered values stayed in
/// the order the reference put them in. Luma is a signed axis and the move a
/// luminance band makes — lift or lower a span — preserves that order when it
/// is doing its job, so a reversal there is always a defect. Chromaticity
/// distance from a band's own colour is RADIAL, and the move a colour band
/// exists to make — carry the band's population toward the target's colour —
/// changes every member's radius: the member that sat exactly on the band's
/// colour ends up a shift away from it while the member that sat a shift away
/// on the far side ends up on it. The radial order inverts by construction,
/// so the test would read the correction's success as its defect. Measured on
/// the synthetic scattered-hue pair: a modest desaturation of the blue band
/// was refused on this reading with a rim of 0.000, shrunk to k=0.104, and
/// then dropped by the composed-frame ceiling for want of any gain left.
///
/// A colour band's boundary is therefore judged by the rim, in ITS coordinate
/// — which is the reading that answers the question a colour ramp raises: did
/// two neighbours the photograph gave the same colour come out different.
fn range_footprint(range: &RangeMask) -> Option<(RangeSelector, (f32, f32))> {
    match *range {
        RangeMask::Luminance { lo_outer, hi_outer, .. } => {
            Some((RangeSelector::Luma, (lo_outer, hi_outer)))
        }
        RangeMask::Color { .. } => None,
    }
}

#[derive(Clone, Debug)]
struct ResidualRun {
    first: usize,
    last: usize,
    target_first: usize,
    target_last: usize,
    residual: f32,
    score: f32,
}

#[derive(Clone, Debug, PartialEq)]
struct RangeBand {
    attachment: ZoneAttachment,
    source: RangeMask,
    target: RangeMask,
    divergence: fit::Divergence,
    /// The ACR hue band a COLOUR band was measured on; `None` for a luminance
    /// band. It names the population in the disclosure and decides which
    /// abstention sentence a refusal is written into.
    hue_band: Option<usize>,
}

#[derive(Clone, Debug, PartialEq)]
struct RangeAbstention {
    lo: f32,
    hi: f32,
    reason: String,
}

#[derive(Clone, Debug, PartialEq)]
struct RangeMerge {
    lo: f32,
    hi: f32,
    into_lo: f32,
    into_hi: f32,
    sign: &'static str,
    why: &'static str,
}

#[derive(Debug, Default, PartialEq)]
struct RangeDerivation {
    bands: Vec<RangeBand>,
    abstentions: Vec<RangeAbstention>,
    merges: Vec<RangeMerge>,
}


fn display_luma(p: &[f32; 3]) -> f32 {
    0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]
}

fn range_weights_for_pixels(range: &RangeMask, pixels: &[[f32; 3]]) -> Vec<f32> {
    pixels.iter().map(|p| render::range_weight(range, p)).collect()
}

/// Overlapping ramps of ONE partition may not count a pixel twice in one
/// estimator. The two families never meet here: each is a stage of its own
/// ([`attach_band_stage`]) and every band re-derives its population from a
/// fresh render of the stack so far, so a pixel a luminance band already
/// moved is measured as the luminance band left it.
fn normalize_partition_weights(weights: &mut [Vec<f32>]) {
    let n = weights.iter().map(Vec::len).min().unwrap_or(0);
    for i in 0..n {
        let total = weights.iter().map(|band| band[i]).sum::<f32>();
        if total > 1.0 {
            for band in &mut *weights {
                band[i] /= total;
            }
        }
    }
}

fn evidence_refusal(e: &fit::EvidenceRange) -> Option<String> {
    if !e.source_populated || e.source_share < RANGE_MIN_EVIDENCE_SHARE {
        Some(format!(
            "source population {:.1}% is below the {:.1}% evidence floor",
            e.source_share * 100.0,
            RANGE_MIN_EVIDENCE_SHARE * 100.0,
        ))
    } else if !e.target_populated || e.target_share < RANGE_MIN_EVIDENCE_SHARE {
        Some(format!(
            "target population {:.1}% is below the {:.1}% evidence floor",
            e.target_share * 100.0,
            RANGE_MIN_EVIDENCE_SHARE * 100.0,
        ))
    } else if e.two_sided_share <= 0.0 {
        Some("the paired population has zero two-sided evidence".to_string())
    } else if e.weight <= 0.0 {
        Some("the paired population has zero structural evidence".to_string())
    } else {
        None
    }
}

fn target_range_for_ranks(
    target_luma: &[f32],
    first: usize,
    last: usize,
) -> RangeMask {
    let n = target_luma.len();
    let first = first.min(n.saturating_sub(1));
    let last = last.max(first + 1).min(n);
    let lo = target_luma.get(first).copied().unwrap_or(0.0);
    let hi = target_luma.get(last - 1).copied().unwrap_or(lo);
    RangeMask::Luminance {
        lo_outer: (lo - RANGE_MAX_RAMP).max(0.0),
        lo,
        hi,
        hi_outer: (hi + RANGE_MAX_RAMP).min(1.0),
    }
}

fn source_ranges_for_runs(runs: &[ResidualRun]) -> Vec<RangeMask> {
    runs.iter()
        .enumerate()
        .map(|(i, run)| {
            let lo = run.first as f32 / 17.0;
            let hi = (run.last + 1) as f32 / 17.0;
            let left_gap = i
                .checked_sub(1)
                .map(|j| lo - (runs[j].last + 1) as f32 / 17.0)
                .unwrap_or(RANGE_MAX_RAMP);
            let right_gap = runs
                .get(i + 1)
                .map(|next| next.first as f32 / 17.0 - hi)
                .unwrap_or(RANGE_MAX_RAMP);
            let ramp = |gap: f32| gap.clamp(RANGE_MIN_RAMP, RANGE_MAX_RAMP);
            RangeMask::Luminance {
                lo_outer: (lo - ramp(left_gap)).max(0.0),
                lo,
                hi,
                hi_outer: (hi + ramp(right_gap)).min(1.0),
            }
        })
        .collect()
}

fn residual_run(
    first: usize,
    last: usize,
    bin_residual: &[f32; 17],
    target_bounds: &[(usize, usize); 17],
    evidence: &fit::EvidenceModel,
) -> ResidualRun {
    let verdict = fit::luma_evidence_for_bins(evidence, first, last);
    let residual_weight = verdict.two_sided_share.max(1e-6);
    let residual = (first..=last)
        .map(|bin| bin_residual[bin] * evidence.luma[bin].two_sided_share.max(1e-6))
        .sum::<f32>()
        / (first..=last)
            .map(|bin| evidence.luma[bin].two_sided_share.max(1e-6))
            .sum::<f32>();
    ResidualRun {
        first,
        last,
        target_first: target_bounds[first].0,
        target_last: target_bounds[last].1,
        residual,
        score: residual.abs() * residual_weight,
    }
}

fn bands_from_runs(
    source_px: &[[f32; 3]],
    target_px: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
    target_luma: &[f32],
    runs: Vec<ResidualRun>,
) -> Vec<RangeBand> {
    let source_ranges = source_ranges_for_runs(&runs);
    let target_ranges = runs
        .iter()
        .map(|run| target_range_for_ranks(target_luma, run.target_first, run.target_last))
        .collect::<Vec<_>>();
    let mut source_coverage = source_ranges
        .iter()
        .map(|range| range_weights_for_pixels(range, source_px))
        .collect::<Vec<_>>();
    let mut source_weights = source_coverage.clone();
    normalize_partition_weights(&mut source_weights);
    let mut bands = Vec::with_capacity(runs.len());
    for (i, run) in runs.into_iter().enumerate() {
        let verdict = fit::luma_evidence_for_bins(evidence, run.first, run.last);
        let d = if verdict.divergence.is_finite() { verdict.divergence } else { 1.0 };
        let source = source_ranges[i];
        let target = target_ranges[i];
        let name = format!("Luminance range {:02}", i + 1);
        bands.push(RangeBand {
            attachment: ZoneAttachment {
                source_weights: std::mem::take(&mut source_weights[i]),
                target_weights: Vec::new(),
                coverage: Some(ZoneCoverage {
                    source: std::mem::take(&mut source_coverage[i]),
                    target: range_weights_for_pixels(&target, target_px),
                }),
                mask: RANGE_HOST,
                components: Vec::new(),
                range: Some(source),
                name: name.clone(),
                role: MaskRole::Custom,
                inverted: false,
                label: name,
                min_share: MIN_ZONE_SHARE,
                frame_regression_tol: RANGE_FRAME_REGRESSION_TOL,
            },
            source,
            target,
            divergence: fit::Divergence {
                correlation: (1.0 - d).clamp(-1.0, 1.0),
                energy_error: 0.0,
                d,
            },
            hue_band: None,
        });
    }
    bands
}

/// Maps a field proposal (a span of CURRENT-render luma) into the evidence-bin
/// domain (ORIGINAL source luma) through the pixels that occupy the span: the
/// weighted 10th..90th percentile of their original luma. The band then names
/// the population the field actually measured, not the same numbers read in a
/// domain a global tone move already shifted.
fn evidence_bins_for_span(
    proposal: &FieldBandProposal,
    source_px: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
) -> Option<(usize, usize)> {
    let inside = |luma: f32| {
        luma >= proposal.lo && (luma < proposal.hi || (proposal.hi >= 1.0 && luma <= 1.0))
    };
    let mut members = source_px
        .iter()
        .zip(&evidence.source_pixels)
        .zip(&evidence.source_weights)
        .filter(|((current, _), _)| inside(display_luma(current)))
        .map(|((_, base), weight)| (display_luma(base), weight.max(0.0)))
        .filter(|(_, weight)| *weight > 0.0)
        .collect::<Vec<_>>();
    members.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mass = members.iter().map(|(_, weight)| *weight as f64).sum::<f64>();
    if mass <= 0.0 {
        return None;
    }
    let quantile = |q: f64| {
        let mut acc = 0.0f64;
        members
            .iter()
            .find(|(_, weight)| {
                acc += *weight as f64;
                acc >= q * mass
            })
            .map(|(luma, _)| *luma)
    };
    let (lo, hi) = (quantile(0.10)?, quantile(0.90)?);
    Some((fit::evidence_luma_bin(lo), fit::evidence_luma_bin(hi)))
}

/// Derive coherent residual runs from the current rendered state. Target
/// populations are monotone rank partners, matching the fixed evidence model;
/// no spatial alignment is assumed.
fn derive_luminance_bands(
    source_px: &[[f32; 3]],
    target_px: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
    proposals: &[FieldBandProposal],
) -> RangeDerivation {

    if source_px.is_empty() || target_px.is_empty() || evidence.luma.len() != 17 {
        return RangeDerivation::default();
    }
    let mut target_luma = target_px.iter().map(display_luma).collect::<Vec<_>>();
    target_luma.sort_by(f32::total_cmp);

    let mut bin_residual = [0.0f32; 17];
    let mut bin_count = [0usize; 17];
    for p in &evidence.source_pixels {
        bin_count[fit::evidence_luma_bin(display_luma(p))] += 1;
    }
    let mut target_cursor = 0usize;
    let mut target_bounds = [(0usize, 0usize); 17];
    for bin in 0..17 {
        let current_values = evidence
            .source_pixels
            .iter()
            .zip(source_px)
            .filter(|(base, _)| fit::evidence_luma_bin(display_luma(base)) == bin)
            .map(|(_, current)| display_luma(current))
            .collect::<Vec<_>>();
        let last = (target_cursor + bin_count[bin]).min(target_luma.len());
        target_bounds[bin] = (target_cursor.min(target_luma.len()), last);
        let target_values = &target_luma[target_cursor.min(target_luma.len())..last];
        if !current_values.is_empty() && !target_values.is_empty() {
            let current_mean = current_values.iter().sum::<f32>() / current_values.len() as f32;
            let target_mean = target_values.iter().sum::<f32>() / target_values.len() as f32;
            bin_residual[bin] = target_mean - current_mean;
        }
        target_cursor = last;
    }

    let mut out = RangeDerivation::default();
    let mut candidate = [false; 17];
    for bin in 0..17 {
        if bin_count[bin] == 0 || bin_residual[bin].abs() < RANGE_RESIDUAL_TRIGGER {
            continue;
        }
        if let Some(reason) = evidence_refusal(&evidence.luma[bin]) {
            out.abstentions.push(RangeAbstention {
                lo: bin as f32 / 17.0,
                hi: (bin + 1) as f32 / 17.0,
                reason,
            });
        } else {
            candidate[bin] = true;
        }
    }

    let mut runs = Vec::<ResidualRun>::new();
    let mut bin = 0usize;
    while bin < 17 {
        if !candidate[bin] {
            bin += 1;
            continue;
        }
        let first = bin;
        let sign = bin_residual[bin].is_sign_positive();
        let mut last = bin;
        while last + 1 < 17
            && candidate[last + 1]
            && bin_residual[last + 1].is_sign_positive() == sign
        {
            let weighted = (first..=last + 1)
                .map(|b| {
                    let w = evidence.luma[b].two_sided_share.max(1e-6);
                    (bin_residual[b] * w, w)
                })
                .fold((0.0f32, 0.0f32), |(rs, ws), (r, w)| (rs + r, ws + w));
            if weighted.1 <= 0.0 || weighted.0.is_sign_positive() != sign {
                break;
            }
            last += 1;
        }
        let qualifies = last > first
            || (first..=last).any(|b| bin_residual[b].abs() >= 2.0 * RANGE_RESIDUAL_TRIGGER);
        if qualifies {
            runs.push(residual_run(
                first, last, &bin_residual, &target_bounds, evidence,
            ));
        }
        bin = last + 1;
    }

    let mut valid = Vec::new();
    for run in runs {
        let verdict = fit::luma_evidence_for_bins(evidence, run.first, run.last);
        if let Some(reason) = evidence_refusal(&verdict) {
            out.abstentions.push(RangeAbstention {
                lo: run.first as f32 / 17.0,
                hi: (run.last + 1) as f32 / 17.0,
                reason,
            });
        } else {
            valid.push(run);
        }
    }

    for proposal in proposals {
        let Some((first, last)) = evidence_bins_for_span(proposal, source_px, evidence) else {
            out.abstentions.push(RangeAbstention {
                lo: proposal.lo,
                hi: proposal.hi,
                reason: "field proposal covers no evidence population".to_string(),
            });
            continue;
        };
        let verdict = fit::luma_evidence_for_bins(evidence, first, last);
        if let Some(reason) = evidence_refusal(&verdict) {
            out.abstentions.push(RangeAbstention {
                lo: first as f32 / 17.0,
                hi: (last + 1) as f32 / 17.0,
                reason: format!("field proposal: {reason}"),
            });
            continue;
        }
        let proposed = residual_run(first, last, &bin_residual, &target_bounds, evidence);
        let overlaps = |run: &ResidualRun| run.first <= last && first <= run.last;
        if valid.iter().any(|run| overlaps(run)
            && run.residual.is_sign_positive() != proposal.sign.is_sign_positive())
        {
            out.abstentions.push(RangeAbstention {
                lo: first as f32 / 17.0,
                hi: (last + 1) as f32 / 17.0,
                reason: "field proposal conflicts with the rank-paired band".to_string(),
            });
            continue;
        }
        // The proposal's own rank-paired residual must agree with the field's
        // sign: a band the two estimators read in opposite directions is a
        // disagreement to disclose, not a correction to widen.
        if proposed.residual.is_sign_positive() != proposal.sign.is_sign_positive() {
            out.abstentions.push(RangeAbstention {
                lo: first as f32 / 17.0,
                hi: (last + 1) as f32 / 17.0,
                reason: "field proposal sign disagrees with its rank-paired residual".to_string(),
            });
            continue;
        }
        let touches = |run: &ResidualRun| {
            run.first <= last.saturating_add(1) && first <= run.last.saturating_add(1)
                && run.residual.is_sign_positive() == proposal.sign.is_sign_positive()
        };
        let merged = valid.iter().enumerate()
            .filter_map(|(i, run)| touches(run).then_some(i)).collect::<Vec<_>>();
        if let Some(&into) = merged.first() {
            let old = proposed.clone();
            valid[into].first = valid[into].first.min(proposed.first);
            valid[into].last = valid[into].last.max(proposed.last);
            valid[into].target_first = valid[into].target_first.min(proposed.target_first);
            valid[into].target_last = valid[into].target_last.max(proposed.target_last);
            for &index in merged.iter().skip(1).rev() {
                let joined = valid.remove(index);
                valid[into].first = valid[into].first.min(joined.first);
                valid[into].last = valid[into].last.max(joined.last);
                valid[into].target_first = valid[into].target_first.min(joined.target_first);
                valid[into].target_last = valid[into].target_last.max(joined.target_last);
            }
            out.merges.push(RangeMerge {
                lo: old.first as f32 / 17.0,
                hi: (old.last + 1) as f32 / 17.0,
                into_lo: valid[into].first as f32 / 17.0,
                into_hi: (valid[into].last + 1) as f32 / 17.0,
                sign: if proposal.sign.is_sign_positive() { "positive" } else { "negative" },
                why: "absorbed by the overlapping rank-paired run before the cap",
            });
        } else {
            valid.push(proposed);
        }
    }
    valid.sort_by_key(|run| run.first);
    debug_assert!(valid.windows(2).all(|pair| pair[0].last < pair[1].first));

    if valid.len() > RANGE_MAX_BANDS {
        let mut ranked = (0..valid.len()).collect::<Vec<_>>();
        ranked.sort_by(|&a, &b| {
            valid[b]
                .score
                .total_cmp(&valid[a].score)
                .then_with(|| valid[a].first.cmp(&valid[b].first))
        });
        let mut keep = ranked[..RANGE_MAX_BANDS].to_vec();
        for positive in [false, true] {
            if valid.iter().any(|r| r.residual.is_sign_positive() == positive)
                && !keep
                    .iter()
                    .any(|&i| valid[i].residual.is_sign_positive() == positive)
            {
                let replacement = ranked
                    .iter()
                    .copied()
                    .find(|&i| valid[i].residual.is_sign_positive() == positive)
                    .expect("sign was observed");
                keep.pop();
                keep.push(replacement);
            }
        }
        keep.sort_unstable();
        keep.dedup();
        let dropped = (0..valid.len()).filter(|i| !keep.contains(i)).collect::<Vec<_>>();
        for drop in dropped {
            let nearest = keep
                .iter()
                .copied()
                .filter(|&i| {
                    valid[i].residual.is_sign_positive()
                        == valid[drop].residual.is_sign_positive()
                })
                .filter(|&into| {
                    let lo = valid[into].first.min(valid[drop].first);
                    let hi = valid[into].last.max(valid[drop].last);
                    keep.iter().copied().filter(|&other| other != into).all(|other| {
                        valid[other].last < lo || valid[other].first > hi
                    })
                })
                .min_by_key(|&i| valid[i].first.abs_diff(valid[drop].first));
            let old = valid[drop].clone();
            if let Some(nearest) = nearest {
                valid[nearest].first = valid[nearest].first.min(old.first);
                valid[nearest].last = valid[nearest].last.max(old.last);
                valid[nearest].target_first = valid[nearest].target_first.min(old.target_first);
                valid[nearest].target_last = valid[nearest].target_last.max(old.target_last);
                out.merges.push(RangeMerge {
                    lo: old.first as f32 / 17.0,
                    hi: (old.last + 1) as f32 / 17.0,
                    into_lo: valid[nearest].first as f32 / 17.0,
                    into_hi: (valid[nearest].last + 1) as f32 / 17.0,
                    sign: if old.residual.is_sign_positive() { "positive" } else { "negative" },
                    why: "after the four-band evidence cap",
                });
            } else {
                out.abstentions.push(RangeAbstention {
                    lo: old.first as f32 / 17.0,
                    hi: (old.last + 1) as f32 / 17.0,
                    reason: "the four-band cap found no adjacent same-sign merge that preserved every retained core"
                        .to_string(),
                });
            }
        }
        valid = keep.into_iter().map(|i| valid[i].clone()).collect();
    }
    valid.sort_by_key(|run| run.first);
    debug_assert!(valid.windows(2).all(|pair| pair[0].last < pair[1].first));
    out.bands = bands_from_runs(source_px, target_px, evidence, &target_luma, valid);
    out
}

fn range_transition_rim(
    reference: &[[f32; 3]],
    rendered: &[[f32; 3]],
    ranges: &[RangeMask],
    width: u32,
    height: u32,
) -> BoundaryReading {
    let (w, h) = (width as usize, height as usize);
    let transitions = ranges.iter().flat_map(range_transitions).collect::<Vec<_>>();
    let mut rims = vec![Vec::new(); transitions.len()];
    let mut sample_pair = |a: usize, b: usize| {
        let Some((ra, rb, pa, pb)) = reference
            .get(a)
            .zip(reference.get(b))
            .zip(rendered.get(a).zip(rendered.get(b)))
            .map(|((ra, rb), (pa, pb))| (ra, rb, pa, pb))
        else {
            return;
        };
        for (index, transition) in transitions.iter().enumerate() {
            let selector = transition.selector;
            // A range rim is a bow in a locally smooth crossing, not a
            // pre-existing subject edge -- and SMOOTH is a claim about the
            // coordinate this band selects on, which is why the test moved
            // inside the loop. A colour band's ramp lives exactly where the
            // luma ruler would have called every crossing smooth.
            let separation = selector.separation(ra, rb);
            if !separation.is_finite() || separation.abs() > RANGE_SMOOTH_CROSSING {
                continue;
            }
            let (sa, sb) = (selector.position(ra), selector.position(rb));
            if !sa.is_finite() || !sb.is_finite() {
                continue;
            }
            let middle = (sa + sb) * 0.5;
            if !(transition.lo..=transition.hi).contains(&middle) {
                continue;
            }
            let bow = selector.separation(pa, pb);
            if bow.is_finite() {
                rims[index].push(bow);
            }
        }
    };
    for y in 0..h {
        for x in 1..w {
            sample_pair(y * w + x - 1, y * w + x);
        }
    }
    for y in 1..h {
        for x in 0..w {
            sample_pair((y - 1) * w + x, y * w + x);
        }
    }
    let transition_count = rims.iter().map(Vec::len).sum();
    if transition_count == 0 {
        return BoundaryReading::uncharged(0.0, 0);
    }
    let rim = rims
        .into_iter()
        .filter(|values| !values.is_empty())
        .map(|mut values| {
            // Budget the signed samples by magnitude so bright and dark bows
            // cannot cancel or hide on opposite sides of a range transition.
            values.sort_by(|a, b| a.abs().total_cmp(&b.abs()));
            let rank = ((values.len() as f32 * ZONE_BOUNDARY_PERCENTILE).ceil() as usize)
                .saturating_sub(1)
                .min(values.len() - 1);
            values[rank].abs()
        })
        .fold(0.0f32, f32::max);
    // This family declines to charge, and it is the ONLY one that still
    // does: its admission rule above (a bow in a locally smooth crossing,
    // never a pre-existing subject edge) is already a binary form of the
    // contextual test, and a graded context here is capped at
    // [`RANGE_SMOOTH_CROSSING`] by construction — no dynamic range. It also
    // does not run the per-channel ruler, because it already reads whichever
    // coordinate its band is made of. See [`super::BoundaryReading::uncharged`].
    BoundaryReading::uncharged(rim, transition_count)
}

/// The delivered transfer's worst ORDER REVERSAL across the bands' edges, in
/// each band's own coordinate.
///
/// The reading is taken in the same two frames the rim gate uses: the
/// REFERENCE (the globals-only twin the engine passes in) supplies the input
/// order, and the RENDERED frame supplies the delivered one. Pixels are binned
/// by their reference POSITION along the coordinate, into the render's own
/// 8-bit codes, and each populated bin holds the mean delivered position of its
/// members; walking the bins upward, the reading is the deepest fall below the
/// highest delivered position already reached. A monotone transfer of any
/// steepness — including one that compresses 30 input codes into 9.9, which is
/// what the calibration band measures — reads exactly 0. An inversion reads
/// its own depth.
///
/// ONLY BANDS WHOSE COORDINATE HAS AN ORDER are walked, which today means the
/// luminance family: [`range_footprint`] hands back nothing for a colour band,
/// and the measurement behind that is written there. Luma is a signed axis and
/// this is the tone order the gate has ranked since v1.2.4.
///
/// Two coordinates would never share a histogram — 0.30 of luma and 0.30 of
/// chromaticity are different facts about different pixels — so each footprint
/// is grouped by its own selector, given its own bins, and the reading is the
/// worst of them. A colour band contributes no group and is judged by the rim.
///
/// Only the bands' own footprints are read: outside them the correction has no
/// weight and its transfer is the identity, so including them would dilute the
/// bins that carry the question.
fn range_transfer_reversal(
    reference: &[[f32; 3]],
    rendered: &[[f32; 3]],
    ranges: &[RangeMask],
) -> f32 {
    const BINS: usize = 256;
    let mut groups: Vec<(RangeSelector, Vec<(f32, f32)>)> = Vec::new();
    for range in ranges {
        let Some((selector, span)) = range_footprint(range) else { continue };
        match groups.iter_mut().find(|(known, _)| *known == selector) {
            Some((_, spans)) => spans.push(span),
            None => groups.push((selector, vec![span])),
        }
    }
    let mut worst = 0.0f32;
    for (selector, spans) in groups {
        let mut sum = [0.0f64; BINS];
        let mut count = [0u32; BINS];
        for (reference, delivered) in reference.iter().zip(rendered) {
            let l = selector.position(reference);
            if !l.is_finite() || !spans.iter().any(|(lo, hi)| l >= *lo && l <= *hi) {
                continue;
            }
            let after = selector.position(delivered);
            if !after.is_finite() {
                continue;
            }
            let bin = (l.clamp(0.0, 1.0) * (BINS - 1) as f32).round() as usize;
            sum[bin] += after as f64;
            count[bin] += 1;
        }
        let mut highest = f32::NEG_INFINITY;
        let mut depth = 0.0f32;
        for bin in 0..BINS {
            if count[bin] == 0 {
                continue;
            }
            let delivered = (sum[bin] / count[bin] as f64) as f32;
            if highest > f32::NEG_INFINITY {
                depth = depth.max(highest - delivered);
            }
            highest = highest.max(delivered);
        }
        worst = worst.max(depth);
    }
    worst
}

fn range_boundary_note_args(
    n: usize,
    k: f32,
    before: BoundaryReading,
    after: BoundaryReading,
    reversal: f32,
) -> Vec<(&'static str, String)> {
    vec![
        ("n", n.to_string()),
        ("k", format!("{k:.3}")),
        ("before", format!("{:.3}", before.rim)),
        ("after", format!("{:.3}", after.rim)),
        ("max", format!("{RANGE_BOUNDARY_RIM_MAX:.3}")),
        ("transitions", after.transitions.to_string()),
        ("reversal", format!("{reversal:.4}")),
        ("rev_max", format!("{RANGE_TRANSFER_REVERSAL_MAX:.4}")),
    ]
}

fn enforce_range_boundary_gate(
    s_img: &DynamicImage,
    report: &mut FitReport,
    reference: &[[f32; 3]],
    ranges: &[RangeMask],
    correction_shares: &[f32],
    first_range: usize,
    initial_px: Vec<[f32; 3]>,
) -> BoundaryGateResult {
    let initial = range_transition_rim(
        reference,
        &initial_px,
        ranges,
        s_img.width(),
        s_img.height(),
    );
    let initial_reversal = range_transfer_reversal(reference, &initial_px, ranges);
    // TWO readings, one verdict. The rim ranks the SIZE of the bow a transition
    // carries and the reversal ranks its SIGN, and a band can fail either
    // alone: v1.2.3 measured a 1.5/17 band at -0.56 EV inverting the tone order
    // by 2 codes at the far corner while the rim ruler read 0.0000 over its
    // whole n, because the DIFFERENCE that ruler ranks stays monotone even
    // where the delivered luma does not.
    let passes = |reading: &BoundaryReading, reversal: f32| {
        reading.rim <= RANGE_BOUNDARY_RIM_MAX && reversal <= RANGE_TRANSFER_REVERSAL_MAX
    };
    let range_count = report.recipe.masks.len().saturating_sub(first_range);
    // A correction that survives this gate must MOVE something, and the test
    // is BYTE IDENTITY of the render against the reference rather than a
    // threshold on `k` — the zoned gate's own invariant since step 9 (see
    // [`crate::rationale::keys::ZONE_BOUNDARY_INERT`]), which this family
    // never had. The colour producer is what made it fire: on the p39
    // calibration pair the bisection returned k=0.000 and the stage kept a
    // mask whose every dial was zero and whose colour gains had been dropped —
    // a correction the sidecar would carry, the GUI would list, and nothing
    // would render. An inert attachment is strictly worse than a refusal.
    let refuse_inert = |report: &mut FitReport,
                        reading: BoundaryReading,
                        reversal: f32,
                        k: f32| {
        report.recipe.masks.truncate(first_range);
        crate::rationale::push_note(
            &mut report.recipe.rationale,
            &mut report.notes,
            crate::rationale::Note::new(
                crate::rationale::keys::RANGE_BOUNDARY_INERT,
                range_boundary_note_args(range_count, k, initial, reading, reversal),
            ),
        );
    };
    if initial_px.as_slice() == reference {
        refuse_inert(report, initial, initial_reversal, 1.0);
        return BoundaryGateResult::Dropped;
    }
    if passes(&initial, initial_reversal) {
        crate::rationale::push_note(
            &mut report.recipe.rationale,
            &mut report.notes,
            crate::rationale::Note::new(
                crate::rationale::keys::RANGE_BOUNDARY_PASSED,
                range_boundary_note_args(range_count, 1.0, initial, initial, initial_reversal),
            ),
        );
        return BoundaryGateResult::Kept {
            k: 1.0,
            before: initial,
            after: initial,
            pixels: initial_px,
        };
    }

    let originals = report.recipe.masks[first_range..].to_vec();
    debug_assert_eq!(originals.len(), correction_shares.len());
    let render_at = |report: &mut FitReport, k: f32| -> (BoundaryReading, f32, Vec<[f32; 3]>) {
        shrink_zone_corrections(
            &mut report.recipe.masks[first_range..],
            &originals,
            correction_shares,
            k,
        );
        let pixels = fit::pixels_of(&render::develop_preview(s_img, &report.recipe));
        let reading = range_transition_rim(
            reference,
            &pixels,
            ranges,
            s_img.width(),
            s_img.height(),
        );
        let reversal = range_transfer_reversal(reference, &pixels, ranges);
        (reading, reversal, pixels)
    };
    // `k = 0` is no local correction at all, so its delivered transfer IS the
    // reference's own and the reversal reading is 0 by construction; only the
    // rim can refuse here, and it does so for the reason it always did.
    let (zero, zero_reversal, zero_px) = render_at(report, 0.0);
    if !passes(&zero, zero_reversal) {
        report.recipe.masks.truncate(first_range);
        crate::rationale::push_note(
            &mut report.recipe.rationale,
            &mut report.notes,
            crate::rationale::Note::new(
                crate::rationale::keys::RANGE_BOUNDARY_REFUSED,
                range_boundary_note_args(range_count, 0.0, initial, zero, zero_reversal),
            ),
        );
        return BoundaryGateResult::Dropped;
    }
    let (mut lo, mut hi) = (0.0f32, 1.0f32);
    let mut best = (zero, zero_reversal, zero_px);
    for _ in 0..12 {
        let mid = (lo + hi) * 0.5;
        let measured = render_at(report, mid);
        if passes(&measured.0, measured.1) {
            lo = mid;
            best = measured;
        } else {
            hi = mid;
        }
    }
    if best.2.as_slice() == reference {
        refuse_inert(report, best.0, best.1, lo);
        return BoundaryGateResult::Dropped;
    }
    shrink_zone_corrections(
        &mut report.recipe.masks[first_range..],
        &originals,
        correction_shares,
        lo,
    );
    crate::rationale::push_note(
        &mut report.recipe.rationale,
        &mut report.notes,
        crate::rationale::Note::new(
            crate::rationale::keys::RANGE_BOUNDARY_PASSED,
            range_boundary_note_args(range_count, lo, initial, best.0, best.1),
        ),
    );
    BoundaryGateResult::Kept { k: lo, before: initial, after: best.0, pixels: best.2 }
}

fn push_range_abstention(report: &mut FitReport, abstention: &RangeAbstention) {
    crate::rationale::push_note(
        &mut report.recipe.rationale,
        &mut report.notes,
        crate::rationale::Note::new(
            crate::rationale::keys::RANGE_ABSTAINED,
            vec![
                ("lo", format!("{:.3}", abstention.lo)),
                ("hi", format!("{:.3}", abstention.hi)),
                ("reason", abstention.reason.clone()),
            ],
        ),
    );
}

fn range_weights_from_current_render(
    s_img: &DynamicImage,
    recipe: &crate::recipe::EditRecipe,
    ranges: &[RangeMask],
) -> (Vec<Vec<f32>>, Vec<Vec<f32>>) {

    let current = fit::pixels_of(&render::develop_preview(s_img, recipe));
    let coverage = ranges
        .iter()
        .map(|range| range_weights_for_pixels(range, &current))
        .collect::<Vec<_>>();
    let mut weights = coverage.clone();
    normalize_partition_weights(&mut weights);
    (weights, coverage)
}

fn final_range_frame_err(
    pixels: &[[f32; 3]],
    target: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
) -> f32 {
    fit::look_err_with_evidence(pixels, target, evidence)
}

/// A hue band this producer declined, and the reading that declined it.
/// Colour bands are named by the ACR band they were measured on rather than
/// by an interval, because that band is the population the frozen evidence
/// model already carries a verdict for.
#[derive(Clone, Debug, PartialEq)]
struct ColourAbstention {
    band: usize,
    reason: String,
}

/// A colour band that cleared every proposal gate, carrying the numbers its
/// propose disclosure prints.
#[derive(Clone, Debug, PartialEq)]
struct ColourProposal {
    band: RangeBand,
    hue_band: usize,
    /// Weighted share of the analysis frame the emitted mask selects.
    share: f32,
    /// The chromaticity tolerance the emitted `crs:ColorAmount` really buys,
    /// after the schema's 0..=1 clamp — not the radius that was measured.
    tolerance: f32,
    residual: f32,
    score: f32,
}

#[derive(Debug, Default, PartialEq)]
struct ColourDerivation {
    proposals: Vec<ColourProposal>,
    abstentions: Vec<ColourAbstention>,
}

/// Hard membership of one ACR hue band on whichever frame is passed in.
/// [`fit::evidence_hue_band`] is the SAME classifier the evidence model bins
/// with (a 0.06 chroma floor, then the nearest of the eight HSL centres), so
/// the population this producer measures and the population the frozen
/// verdicts are about are one population and not two that resemble each other.
fn hue_band_members(px: &[[f32; 3]], band: usize) -> Vec<f32> {
    px.iter()
        .map(|p| if fit::evidence_hue_band(p) == Some(band) { 1.0 } else { 0.0 })
        .collect()
}

/// One hue band's own colour and the chromaticity radius that holds nine of
/// its members in ten, plus the member that sits closest to that colour.
///
/// The radius is the band's [`ZONE_BOUNDARY_PERCENTILE`] distance from its own
/// weighted mean: DERIVED from the population, not tuned, and taken at the
/// same percentile the boundary readings are budgeted at, so the mask and the
/// gate agree on which members are the body of the band and which are its
/// edge.
///
/// `None` when the band has no members, or when its mean colour is too dark to
/// carry a chromaticity at all — the same rule `render::range_weight` applies
/// to a near-black reference, asked before a mask is built rather than after.
fn colour_band_centre(px: &[[f32; 3]], members: &[f32]) -> Option<([f32; 3], f32, usize)> {
    let mut mass = 0.0f64;
    let mut mean = [0.0f64; 3];
    for (p, &w) in px.iter().zip(members) {
        if w <= 0.0 {
            continue;
        }
        mass += w as f64;
        for (channel, value) in mean.iter_mut().zip(p) {
            *channel += w as f64 * *value as f64;
        }
    }
    if mass <= 0.0 {
        return None;
    }
    let reference = [
        (mean[0] / mass) as f32,
        (mean[1] / mass) as f32,
        (mean[2] / mass) as f32,
    ];
    let mut distances = Vec::new();
    let mut nearest: Option<(f32, usize)> = None;
    for (i, (p, &w)) in px.iter().zip(members).enumerate() {
        if w <= 0.0 {
            continue;
        }
        let d = render::chromaticity_distance(&reference, p);
        if !d.is_finite() {
            continue;
        }
        distances.push(d);
        if nearest.is_none_or(|(best, _)| d < best) {
            nearest = Some((d, i));
        }
    }
    let (_, sample) = nearest?;
    distances.sort_by(f32::total_cmp);
    let rank = ((distances.len() as f32 * ZONE_BOUNDARY_PERCENTILE).ceil() as usize)
        .saturating_sub(1)
        .min(distances.len() - 1);
    Some((reference, distances[rank], sample))
}

/// The colour Range Mask keyed to one band's colour at that band's radius.
///
/// The tolerance IS the radius. Measured on the synthetic scattered-hue pair:
/// at the radius, every unit of the ball's weight lands on a member of the
/// band it was derived from (purity 1.000 on both of that frame's bands); at
/// twice the radius — which would put the p90 member in the full-weight core
/// rather than on the ramp — purity falls to 0.55 and 0.78, because the ball
/// starts selecting the very pixels the band excluded.
///
/// AND IT STOPS BEFORE NEUTRAL. A ball wide enough to reach the achromatic
/// axis is not a colour range: it would select grey, which belongs to no hue
/// band at all, and a correction through it is a global move that walked past
/// the global stage's gates. The reference colour's own distance from neutral
/// is that limit, measured rather than chosen, and at it grey lands exactly on
/// the tolerance where the weight reaches zero.
///
/// Over 1.05 the sidecar's grammar cannot go, so a very wide band gets a mask
/// narrower than its measurement asked for and the proposal disclosure prints
/// the tolerance that was actually bought.
///
/// `(px, py)` is the member closest to the reference colour. Lightroom treats
/// it as a cosmetic sample marker, and this way it is a real sample of the
/// band instead of an invented point.
fn colour_mask(
    reference: [f32; 3],
    radius: f32,
    sample: usize,
    width: u32,
    height: u32,
) -> RangeMask {
    let (w, h) = (width.max(1) as usize, height.max(1) as usize);
    let neutral = render::chromaticity_distance(&reference, &[0.5; 3]);
    let tolerance = if neutral.is_finite() { radius.min(neutral) } else { radius };
    RangeMask::Color {
        r: reference[0],
        g: reference[1],
        b: reference[2],
        amount: render::colour_range_amount(tolerance),
        px: ((sample % w) as f32 + 0.5) / w as f32,
        py: ((sample / w).min(h - 1) as f32 + 0.5) / h as f32,
    }
}

/// Derive colour bands from the current rendered stack.
///
/// THE CASE THIS PRODUCER EXISTS FOR is a look that differs by COLOUR over a
/// population no rectangle and no silhouette can hold: the same hue wherever it
/// appears in the frame. The luminance producer above cannot see it, because a
/// hue move need not move luma at all; the spatial producers cannot hold it,
/// because the population is scattered. Until this existed such a look was
/// fitted globally or not at all.
///
/// THE EVIDENCE IS TWO-SIDED OR THERE IS NO BAND, and that is not a
/// precaution: a hue-shifting edit DEPOPULATES its own band on the target
/// side, so a band only the source populates is exactly the shape a circular
/// argument takes here — "the target has no blues, therefore correct the
/// blues" is the edit talking about itself. Two independent arms answer it.
/// The FROZEN original-pair verdict `evidence.hue[band]` is asked through the
/// same [`evidence_refusal`] the luminance producer uses, so both sides must
/// clear the 1.5% population floor and the band must carry structural weight.
/// Then the DELIVERED populations are asked the same question again, on the
/// two frames the mask will actually be applied to, because the global stage
/// may have moved hues since the evidence was frozen. A band that fails either
/// arm abstains with the reading that refused it. A third arm sits downstream
/// and is not optional either: [`attach_one_zone`] re-scopes the evidence over
/// the mask's OWN population and withholds the colour class outright when the
/// hue bands that population moves are unsupported.
///
/// A GLOBAL COLOUR MOVE PRODUCES NO BAND HERE, for the same reason a global
/// tone move produces no luminance band: the residual is read on the CURRENT
/// render, so whatever the global stage already matched is gone before this
/// function sees it, and the bands read under [`ZONE_MATCHED_ERR`] — the
/// observed matched domain, which is the line a band has to clear to be
/// proposed at all. A global move the global stage was REFUSED is caught by
/// the two-sidedness arms instead, because refusing it does not put the
/// target's hues back into the source's bands.
fn derive_colour_bands(
    source_px: &[[f32; 3]],
    target_px: &[[f32; 3]],
    evidence: &fit::EvidenceModel,
    width: u32,
    height: u32,
) -> ColourDerivation {
    let mut out = ColourDerivation::default();
    let n = source_px.len().min(target_px.len());
    if n == 0 {
        return out;
    }
    let (source_px, target_px) = (&source_px[..n], &target_px[..n]);
    for band in 0..crate::recipe::HSL_BANDS.len().min(evidence.hue.len()) {
        let source_members = hue_band_members(source_px, band);
        let target_members = hue_band_members(target_px, band);
        let residual = zone_err(
            &zone_moments(source_px, &source_members),
            &zone_moments(target_px, &target_members),
        );
        // Silent under the observed matched domain, exactly as a luminance bin
        // under the residual trigger is silent. A quiet band is not a refusal
        // anyone needs explained, and eight of them per fit would bury the
        // bands that were refused for a reason.
        if residual < ZONE_MATCHED_ERR {
            continue;
        }
        if let Some(reason) = evidence_refusal(&evidence.hue[band]) {
            out.abstentions.push(ColourAbstention { band, reason });
            continue;
        }
        let share_of = |members: &[f32]| members.iter().sum::<f32>() / n as f32;
        let (source_share, target_share) =
            (share_of(&source_members), share_of(&target_members));
        if source_share < RANGE_MIN_EVIDENCE_SHARE || target_share < RANGE_MIN_EVIDENCE_SHARE {
            out.abstentions.push(ColourAbstention {
                band,
                reason: format!(
                    "the rendered band holds {:.1}% and the target band {:.1}% \
                     of the frame, against the {:.1}% two-sided floor",
                    source_share * 100.0,
                    target_share * 100.0,
                    RANGE_MIN_EVIDENCE_SHARE * 100.0,
                ),
            });
            continue;
        }
        let Some((reference, radius, sample)) = colour_band_centre(source_px, &source_members)
        else {
            out.abstentions.push(ColourAbstention {
                band,
                reason: "the band's mean colour is too dark to carry a chromaticity"
                    .to_string(),
            });
            continue;
        };
        // ONE MASK, READ ON BOTH FRAMES. The target population has to be
        // defined the same way as the source one or the share gate reads a
        // sizing artefact as a composition mismatch: on the synthetic pair the
        // target band's OWN ball covered 4.7% of the frame against the source
        // ball's 18.2%, for populations that are 26.9% and 26.8% of it. The
        // emitted mask is the ruler for both sides, so what the share gate
        // compares is one question asked twice — which is why `target` here is
        // the same mask and not a second one keyed to the target's colour.
        //
        // That also makes the gate a circularity guard in its own right, and
        // the last of four: a colour move large enough to carry the target's
        // pixels out of the mask leaves the target share far under the source's
        // and the shared 2:1 composition gate refuses the band, rather than
        // fitting a correction to a population that is no longer there.
        let source = colour_mask(reference, radius, sample, width, height);
        let target = source;
        let coverage = ZoneCoverage {
            source: range_weights_for_pixels(&source, source_px),
            target: range_weights_for_pixels(&target, target_px),
        };
        let share = coverage.source.iter().sum::<f32>() / n as f32;
        let verdict = &evidence.hue[band];
        let d = if verdict.divergence.is_finite() { verdict.divergence } else { 1.0 };
        out.proposals.push(ColourProposal {
            band: RangeBand {
                attachment: ZoneAttachment {
                    // Both weight vectors are placeholders the entry point
                    // replaces: the source population is re-derived from the
                    // current rendered stack before every attempt, and the
                    // target population is normalised against every OTHER band
                    // in the same fit. They start as this band's own raw ramp so
                    // the structure is meaningful when a test builds one alone.
                    source_weights: coverage.source.clone(),
                    target_weights: coverage.target.clone(),
                    coverage: Some(coverage),
                    mask: RANGE_HOST,
                    components: Vec::new(),
                    range: Some(source),
                    name: String::new(),
                    role: MaskRole::Custom,
                    inverted: false,
                    label: String::new(),
                    min_share: MIN_ZONE_SHARE,
                    frame_regression_tol: RANGE_FRAME_REGRESSION_TOL,
                },
                source,
                target,
                divergence: fit::Divergence {
                    correlation: (1.0 - d).clamp(-1.0, 1.0),
                    energy_error: 0.0,
                    d,
                },
                hue_band: Some(band),
            },
            hue_band: band,
            share,
            tolerance: match source {
                RangeMask::Color { amount, .. } => render::colour_range_tolerance(amount),
                RangeMask::Luminance { .. } => 0.0,
            },
            residual,
            score: residual * verdict.two_sided_share.max(1e-6),
        });
    }
    // The same cap the luminance family answers to, applied to this family's
    // own set: four bands is the measured stability ceiling for value-range
    // evidence and a hue band is no better identified than a luma one. The
    // ranking is the luminance family's too — residual weighted by the band's
    // two-sided share — so a large move in a band barely anyone populates does
    // not displace a real one. Ties go to the lower band index, so the outcome
    // does not depend on iteration order.
    out.proposals.sort_by(|a, b| {
        b.score.total_cmp(&a.score).then_with(|| a.hue_band.cmp(&b.hue_band))
    });
    for dropped in out.proposals.split_off(out.proposals.len().min(RANGE_MAX_BANDS)) {
        out.abstentions.push(ColourAbstention {
            band: dropped.hue_band,
            reason: format!(
                "band residual {:.3} ranked below the {} bands the evidence cap keeps",
                dropped.residual, RANGE_MAX_BANDS,
            ),
        });
    }
    // Attach in band order, not in score order: the rationale then reads red to
    // magenta the way the HSL mixer does, and the mask names follow it.
    out.proposals.sort_by_key(|proposal| proposal.hue_band);
    out.abstentions.sort_by_key(|abstention| abstention.band);
    for (index, proposal) in out.proposals.iter_mut().enumerate() {
        proposal.band.attachment.name = format!("Colour range {:02}", index + 1);
        proposal.band.attachment.label = format!(
            "Colour range {:02} ({})",
            index + 1,
            crate::recipe::HSL_BANDS[proposal.hue_band],
        );
    }
    out
}

fn push_colour_proposal(report: &mut FitReport, proposal: &ColourProposal) {
    crate::rationale::push_note(
        &mut report.recipe.rationale,
        &mut report.notes,
        crate::rationale::Note::new(
            crate::rationale::keys::COLOUR_RANGE_PROPOSED,
            vec![
                ("label", proposal.band.attachment.label.clone()),
                ("band", crate::recipe::HSL_BANDS[proposal.hue_band].to_string()),
                ("share", format!("{:.1}", proposal.share * 100.0)),
                ("tol", format!("{:.3}", proposal.tolerance)),
                ("residual", format!("{:.3}", proposal.residual)),
            ],
        ),
    );
}

fn push_colour_abstention(report: &mut FitReport, abstention: &ColourAbstention) {
    crate::rationale::push_note(
        &mut report.recipe.rationale,
        &mut report.notes,
        crate::rationale::Note::new(
            crate::rationale::keys::COLOUR_RANGE_ABSTAINED,
            vec![
                ("band", crate::recipe::HSL_BANDS[abstention.band].to_string()),
                ("reason", abstention.reason.clone()),
            ],
        ),
    );
}

/// One family's bands, attached, gated and shrunk as a stage of its own.
///
/// TWO FAMILIES, TWO STAGES, and that is not tidiness. The boundary gate
/// shrinks every correction it holds by ONE shared `k`, so a colour band whose
/// edge is over budget would crush the luminance bands beside it: measured on
/// the p37 corpus pair, one gate over both families took `k` to 0.003 and the
/// frame from 0.15915 — where the luminance bands alone left it — to 0.16474.
/// The composed-frame ceiling could not see that, because it compares against
/// the residual the fallback was handed and not against the stage before it.
/// Staged, each family carries its own shrink and its own ceiling, and the
/// colour family is measured against the frame the luminance family actually
/// left behind. It is the same layering the producers above already use.
struct RangeStage {
    accepted: usize,
    worst: f32,
    /// The render this stage leaves behind; `None` when it attached nothing
    /// and the frame the caller already had still stands.
    pixels: Option<Vec<[f32; 3]>>,
}

fn attach_band_stage(
    s_img: &DynamicImage,
    tgt_px: &[[f32; 3]],
    report: &mut FitReport,
    mut bands: Vec<RangeBand>,
    reference_px: &[[f32; 3]],
) -> RangeStage {
    let nothing = RangeStage { accepted: 0, worst: 0.0, pixels: None };
    if bands.is_empty() {
        return nothing;
    }
    // The frame metric this stage was HANDED is its ceiling; every band it
    // accepts must leave the composed frame no worse than it found it.
    let entry_frame_err = report.err_after;
    let all_ranges = bands.iter().map(|band| band.source).collect::<Vec<_>>();
    let target_coverage = bands
        .iter()
        .map(|band| range_weights_for_pixels(&band.target, tgt_px))
        .collect::<Vec<_>>();
    let mut target_weights = target_coverage.clone();
    normalize_partition_weights(&mut target_weights);
    for (band, weights) in bands.iter_mut().zip(target_weights) {
        band.attachment.target_weights = weights;
    }
    let first_range = report.recipe.masks.len();
    let mut frame_err = report.err_after;
    let corr = report.correspondence.take();
    let mut accepted = Vec::new();
    for i in 0..bands.len() {
        let (mut current_weights, mut current_coverage) =
            range_weights_from_current_render(s_img, &report.recipe, &all_ranges);
        bands[i].attachment.source_weights = std::mem::take(&mut current_weights[i]);
        bands[i].attachment.coverage = Some(ZoneCoverage {
            source: std::mem::take(&mut current_coverage[i]),
            target: target_coverage[i].clone(),
        });
        let accepted_band = attach_one_zone(
            s_img,
            tgt_px,
            report,
            &mut frame_err,
            &bands[i].attachment,
            // A band's divergence is DERIVED from its own evidence range
            // rather than read off the structural instrument, so it is always
            // present: there is nothing here for the instrument to abstain
            // about.
            Some(bands[i].divergence),
            corr.as_ref(),
        );
        let refused = "the shared estimator or do-no-harm gates refused the correction";
        match (accepted_band, bands[i].hue_band) {
            (Some(zone), _) => accepted.push(zone),
            (None, Some(band)) => push_colour_abstention(
                report,
                &ColourAbstention { band, reason: refused.to_string() },
            ),
            (None, None) => push_range_abstention(
                report,
                &RangeAbstention {
                    lo: match bands[i].source {
                        RangeMask::Luminance { lo, .. } => lo,
                        RangeMask::Color { .. } => 0.0,
                    },
                    hi: match bands[i].source {
                        RangeMask::Luminance { hi, .. } => hi,
                        RangeMask::Color { .. } => 1.0,
                    },
                    reason: refused.to_string(),
                },
            ),
        }
    }
    report.correspondence = corr;
    if accepted.is_empty() {
        return nothing;
    }
    // R39. The k=1 controls of every accepted band, so a band the SHRUNK set
    // no longer justifies (`refuse_shrunk_zones`) is dropped and the rest are
    // gated again from full strength — see `attach_zones`.
    let mut originals = report.recipe.masks[first_range..].to_vec();
    let mut initial_px = accepted.last().expect("accepted range exists").rendered.clone();
    let (final_px, final_frame_err) = loop {
        let accepted_ranges = accepted
            .iter()
            .filter_map(|zone| zone.range)
            .collect::<Vec<_>>();
        let shares = accepted
            .iter()
            .map(|zone| {
                zone.source_weights.iter().sum::<f32>()
                    / zone.source_weights.len().max(1) as f32
            })
            .collect::<Vec<_>>();
        let (k, pixels) = match enforce_range_boundary_gate(
            s_img,
            report,
            reference_px,
            &accepted_ranges,
            &shares,
            first_range,
            initial_px,
        ) {
            BoundaryGateResult::Kept { k, pixels, .. } => (k, pixels),
            BoundaryGateResult::Dropped => return nothing,
        };
        let final_frame_err = final_range_frame_err(&pixels, tgt_px, &report.evidence);
        if final_frame_err > entry_frame_err + RANGE_FRAME_REGRESSION_TOL {
            report.recipe.masks.truncate(first_range);
            crate::rationale::push_note(
                &mut report.recipe.rationale,
                &mut report.notes,
                crate::rationale::Note::new(
                    crate::rationale::keys::RANGE_FRAME_REFUSED,
                    vec![
                        ("n", accepted.len().to_string()),
                        ("global", format!("{entry_frame_err:.3}")),
                        ("after", format!("{final_frame_err:.3}")),
                        ("tol", format!("{RANGE_FRAME_REGRESSION_TOL:+.3}")),
                    ],
                ),
            );
            report.err_after = entry_frame_err;
            return nothing;
        }
        if !refuse_shrunk_zones(
            s_img, report, &mut accepted, &mut originals, first_range, &pixels, tgt_px, k,
            entry_frame_err, final_frame_err,
        ) {
            break (pixels, final_frame_err);
        }
        report.recipe.masks.truncate(first_range);
        if accepted.is_empty() {
            report.err_after = entry_frame_err;
            return nothing;
        }
        report.recipe.masks.extend(originals.iter().cloned());
        for (j, zone) in accepted.iter_mut().enumerate() {
            zone.mask_index = first_range + j;
        }
        initial_px = fit::pixels_of(&render::develop_preview(s_img, &report.recipe));
    };
    for zone in &mut accepted {
        let after = zone_moments(&final_px, &zone.source_weights);
        let target = zone_moments(tgt_px, &zone.target_weights);
        zone.after = zone_err(&after, &target);
        push_zone_attached_note(report, zone);
    }
    report.err_after = final_frame_err;
    RangeStage {
        accepted: accepted.len(),
        worst: accepted.iter().map(|zone| zone.after).fold(0.0f32, f32::max),
        pixels: Some(final_px),
    }
}

/// Automatic pure-Rust fallback after the global fit, and the one entry point
/// both range families share.
///
/// Luminance bands run first, in ascending luma order, then colour bands in
/// ACR band order on the frame the luminance stage left behind. Each family is
/// a stage of its own — see [`attach_band_stage`] for the measurement that
/// made that necessary — and every attempt inside a stage derives its source
/// population from the current rendered stack rather than the untouched
/// source. The confidence the two stages earn is reported once, at the end,
/// on the worst band either of them accepted.
pub(super) fn attach_ranges(
    src: &DynamicImage,
    target: &DynamicImage,
    report: &mut FitReport,
    proposals: &[FieldBandProposal],
) {
    let (s_img, t_img) = fit::analysis_pair(src, target);
    let tgt_px = fit::pixels_of(&t_img);
    let mut current_px = fit::pixels_of(&render::develop_preview(&s_img, &report.recipe));
    let derived = derive_luminance_bands(&current_px, &tgt_px, &report.evidence, proposals);
    for merged in &derived.merges {
        crate::rationale::push_note(
            &mut report.recipe.rationale,
            &mut report.notes,
            crate::rationale::Note::new(
                crate::rationale::keys::RANGE_MERGED,
                vec![
                    ("lo", format!("{:.3}", merged.lo)),
                    ("hi", format!("{:.3}", merged.hi)),
                    ("into_lo", format!("{:.3}", merged.into_lo)),
                    ("into_hi", format!("{:.3}", merged.into_hi)),
                    ("sign", merged.sign.to_string()),
                    ("why", merged.why.to_string()),
                ],
            ),
        );
    }
    for abstention in &derived.abstentions {
        push_range_abstention(report, abstention);
    }
    let luminance = attach_band_stage(&s_img, &tgt_px, report, derived.bands, &current_px);
    if let Some(pixels) = luminance.pixels {
        current_px = pixels;
    }
    let colour = derive_colour_bands(
        &current_px,
        &tgt_px,
        &report.evidence,
        s_img.width(),
        s_img.height(),
    );
    for proposal in &colour.proposals {
        push_colour_proposal(report, proposal);
    }
    for abstention in &colour.abstentions {
        push_colour_abstention(report, abstention);
    }
    let bands = colour.proposals.into_iter().map(|proposal| proposal.band).collect();
    let colour = attach_band_stage(&s_img, &tgt_px, report, bands, &current_px);
    if let Some(pixels) = colour.pixels {
        current_px = pixels;
    }
    let accepted = luminance.accepted + colour.accepted;
    if accepted > 0 {
        let worst = luminance.worst.max(colour.worst);
        report.recipe.confidence = report
            .recipe
            .confidence
            .min(fit::clamp_confidence(1.0 - worst * ZONE_CONFIDENCE_SLOPE));
        crate::rationale::push_note(
            &mut report.recipe.rationale,
            &mut report.notes,
            crate::rationale::Note::new(
                crate::rationale::keys::RANGE_CONFIDENCE,
                vec![
                    ("n", accepted.to_string()),
                    ("worst", format!("{worst:.3}")),
                    ("frame", format!("{:.3}", report.err_after)),
                ],
            ),
        );
    }
    fit::append_finished_disclosure(report, &current_px, &tgt_px);
}
