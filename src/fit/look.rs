//! Look error, with and without evidence, and the calibration corpus and recipe.

use super::*;



/// Evidence-weighted look error used by every fit decision.  The structural
/// term makes the objective sensitive to spatially wrong pixels; the weighted
/// distribution terms ignore ranges that are one-sided or structurally
/// divergent instead of silently treating them as a match.
pub(crate) fn look_err_with_evidence(
    a: &[[f32; 3]],
    b: &[[f32; 3]],
    evidence: &EvidenceModel,
) -> f32 {
    if evidence.identifiability <= 1e-5 {
        return 1.0;
    }
    let tonal = tonal_err_with_evidence(a, b, evidence);
    let colour = (0..3)
        .map(|ch| {
            (weighted_mean(a, &evidence.source_weights, ch).unwrap_or(0.0)
                - weighted_mean(b, &evidence.target_weights, ch).unwrap_or(0.0))
                .abs()
        })
        .sum::<f32>()
        / 3.0;
    let base = 0.55 * tonal + 0.20 * colour;
    // Per-band centroid hue disagreement — the WORST qualifying band, not a
    // weighted mean: one region with wrecked hue ruins a photo no matter how
    // small its area share (a lavender sky over perfect rocks), and an
    // area-weighted mean lets exactly that hide (measured: the violet-sky
    // curves slipped through the cast-acceptance gate on the mean variant).
    // |Δ| saturates at 60° so a fully-wrecked band reads 1.
    let (sa, ta) = band_stats_weighted(a, &evidence.source_hue_weights);
    let (sb, tb) = band_stats_weighted(b, &evidence.target_hue_weights);
    let mut hue = 0.0f32;
    let mut hue_weight = 0.0f32;
    if ta >= 1.0 && tb >= 1.0 {
        for i in 0..8 {
            let (x, y) = (&sa[i], &sb[i]);
            let range_weight = evidence.hue.get(i).map(|r| r.weight).unwrap_or(0.0);
            if range_weight <= 0.0 || x.w / ta < EVIDENCE_MIN_SHARE as f64 || y.w / tb < EVIDENCE_MIN_SHARE as f64 {
                continue;
            }
            let mut d = y.sin.atan2(y.cos).to_degrees() - x.sin.atan2(x.cos).to_degrees();
            while d > 180.0 {
                d -= 360.0;
            }
            while d < -180.0 {
                d += 360.0;
            }
            hue = hue.max((d.abs().min(60.0) / 60.0) as f32);
            hue_weight = hue_weight.max(range_weight);
        }
    }
    let (w, h) = (evidence.width, evidence.height);
    // A PENALTY term: with no reading there is nothing to charge. That is what
    // the abstention says here — not that the structure survived.
    let structural = if w > 0 && h > 0 && evidence.source_weights.len() == a.len() {
        structure_divergence(a, b, w, h, &evidence.source_weights).map_or(0.0, |r| r.d)
    } else {
        0.0
    };
    base + 0.15 * hue * hue_weight.min(1.0) + 0.10 * structural.min(1.0)
}

/// The TONAL term of [`look_err_with_evidence`] on its own: the mean absolute
/// gap between the two frames' evidence-weighted luma CDFs at 21 quantiles.
/// It is the term a tone stage controls, and so the ruler a tone stage is
/// held to (2026-10-21): the sliders and the residual curve each ship only
/// where they leave this reading no worse than they found it. Judging them on
/// the whole objective charged a correct luma move for the colour-mean drift
/// that brightening a muted frame toward a vivid target necessarily brings —
/// drift the stages after it exist to correct — while the lantern street's
/// wrong +1.65 EV raised this term 0.0468 → 0.0748 on its own and needs no
/// other term to be refused.
pub(crate) fn tonal_err_with_evidence(
    a: &[[f32; 3]],
    b: &[[f32; 3]],
    evidence: &EvidenceModel,
) -> f32 {
    let (ca, cb) = (
        weighted_cdf(a, &evidence.source_weights, luma601),
        weighted_cdf(b, &evidence.target_weights, luma601),
    );
    let mut tonal = 0.0f32;
    let mut n = 0.0f32;
    for i in 0..=20 {
        let p = (i as f32 / 20.0).clamp(P_CLIP, 1.0 - P_CLIP);
        tonal += (quantile(&ca, p) - quantile(&cb, p)).abs();
        n += 1.0;
    }
    tonal / n
}


pub(super) fn round1(v: f32) -> f32 {
    (v * 10.0).round() / 10.0
}
pub(super) fn round2(v: f32) -> f32 {
    (v * 100.0).round() / 100.0
}



// --------------------------------------------------------------------------
