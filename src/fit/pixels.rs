//! Pixel statistics: pixels and luma, CDFs, the neutral-gate misprediction, quantiles, luma variance and mean chroma.

use super::*;

// --------------------------------------------------------------------------
// statistics primitives
// --------------------------------------------------------------------------

pub fn pixels_of(img: &DynamicImage) -> Vec<[f32; 3]> {
    img.to_rgb8()
        .pixels()
        .map(|p| [p[0] as f32 / 255.0, p[1] as f32 / 255.0, p[2] as f32 / 255.0])
        .collect()
}

pub fn luma601(p: &[f32; 3]) -> f32 {
    0.299 * p[0] + 0.587 * p[1] + 0.114 * p[2]
}



/// Near-neutral gate for the TONE evidence (the cast catch-all fits on
/// ungated per-channel CDFs — see `residual_channel_curve`). Gated on HSV
/// saturation ((max−min)/max), which is INVARIANT under pure luminance
/// scaling — so the same pixels qualify in the source and in its tone-mapped
/// target (an absolute-chroma gate is not: dark colours slip under it in the
/// source and leave it once brightened, skewing the two CDFs against each
/// other). Near-black counts as neutral.
pub(super) fn is_neutralish(p: &[f32; 3]) -> bool {
    let mx = p[0].max(p[1]).max(p[2]);
    let mn = p[0].min(p[1]).min(p[2]);
    mx < 0.04 || (mx - mn) / mx < 0.15
}


pub(super) fn tone_cdf_pair_weighted(
    sp: &[[f32; 3]],
    tp: &[[f32; 3]],
    evidence: &EvidenceModel,
) -> (Vec<f32>, Vec<f32>) {
    let s_n: Vec<(f32, f32)> = sp
        .iter()
        .enumerate()
        .filter(|(_, p)| is_neutralish(p))
        .filter_map(|(i, p)| {
            let w = evidence.source_weights.get(i).copied().unwrap_or(0.0);
            (w > 0.0).then_some((luma601(p), w))
        })
        .collect();
    let t_n: Vec<(f32, f32)> = tp
        .iter()
        .enumerate()
        .filter(|(_, p)| is_neutralish(p))
        .filter_map(|(i, p)| {
            let w = evidence.target_weights.get(i).copied().unwrap_or(0.0);
            (w > 0.0).then_some((luma601(p), w))
        })
        .collect();
    let weighted = |v: &[(f32, f32)]| {
        let mut hist = vec![0.0f32; HIST_BINS];
        let total = v.iter().map(|(_, w)| *w).sum::<f32>();
        if total <= 1e-8 { return hist; }
        for &(x, w) in v {
            hist[(x.clamp(0.0, 1.0) * (HIST_BINS - 1) as f32).round() as usize] += w;
        }
        let mut acc = 0.0;
        for h in &mut hist { acc += *h; *h = acc / total; }
        hist
    };
    // The SAME identification gates the unweighted twin (`tone_cdf_pair`)
    // documents: the neutral gate only carries clean evidence when the same
    // population is neutral on BOTH sides. This arm shipped without them, and
    // the p36 calibration pair showed the cost live: the source's neutral
    // class is its dark rock, the target's is its bright sky, and the gated
    // quantile map sent 0.05 → 0.77 — a map the slider solve then pegged
    // itself against. Population parity is judged on the pixels that carry
    // evidence weight, floor and ratio exactly as the twin's contract states.
    let s_total = evidence.source_weights.iter().filter(|&&w| w > 0.0).count();
    let t_total = evidence.target_weights.iter().filter(|&&w| w > 0.0).count();
    let share_s = s_n.len() as f32 / s_total.max(1) as f32;
    let share_t = t_n.len() as f32 / t_total.max(1) as f32;
    let gated = enough_evidence(s_n.len(), s_total)
        && enough_evidence(t_n.len(), t_total)
        && share_s.max(share_t) <= 1.75 * share_s.min(share_t);
    let s = if gated { weighted(&s_n) } else { Vec::new() };
    let t = if gated { weighted(&t_n) } else { Vec::new() };
    if s.iter().any(|&v| v > 0.0) && t.iter().any(|&v| v > 0.0) {
        (s, t)
    } else {
        (weighted_cdf(sp, &evidence.source_weights, luma601), weighted_cdf(tp, &evidence.target_weights, luma601))
    }
}

/// The tone-evidence sample floor: at least 5% of the frame and never fewer
/// than 512 px. Shared between the per-side gate and the shared-class floor
/// inside [`neutral_gate_misprediction`], so "enough to trust" means one
/// thing.
fn enough_evidence(n: usize, total: usize) -> bool {
    n >= (total / 20).max(NEUTRAL_SHARED_MIN)
}


/// F(x): fraction of pixels ≤ x (linear interp between bins).
pub(crate) fn cdf_at(cdf: &[f32], x: f32) -> f32 {
    let pos = x.clamp(0.0, 1.0) * (cdf.len() - 1) as f32;
    let i = pos.floor() as usize;
    if i >= cdf.len() - 1 {
        return cdf[cdf.len() - 1];
    }
    let t = pos - i as f32;
    cdf[i] * (1.0 - t) + cdf[i + 1] * t
}

/// Q(p): the value at quantile `p` (inverse CDF, linear interp within the bin).
pub(crate) fn quantile(cdf: &[f32], p: f32) -> f32 {
    let n = cdf.len();
    let mut lo = 0usize;
    let mut hi = n - 1;
    while lo < hi {
        let mid = (lo + hi) / 2;
        if cdf[mid] < p {
            lo = mid + 1;
        } else {
            hi = mid;
        }
    }
    // Interpolate within the step from the previous bin for a smooth inverse.
    if lo == 0 {
        return 0.0;
    }
    let (c0, c1) = (cdf[lo - 1], cdf[lo]);
    let t = if c1 > c0 { ((p - c0) / (c1 - c0)).clamp(0.0, 1.0) } else { 1.0 };
    ((lo - 1) as f32 + t) / (n - 1) as f32
}

/// Variance of the per-pixel channel mean — the degenerate-input probe
/// (see the refusal at the top of [`fit_recipe`]). Zero for an empty slice.
pub(super) const DEGENERATE_LUMA_VAR: f32 = 1e-6;
pub(super) fn luma_variance(px: &[[f32; 3]]) -> f32 {
    if px.is_empty() {
        return 0.0;
    }
    let n = px.len() as f32;
    let lum = |p: &[f32; 3]| (p[0] + p[1] + p[2]) / 3.0;
    let mean: f32 = px.iter().map(lum).sum::<f32>() / n;
    px.iter().map(|p| (lum(p) - mean).powi(2)).sum::<f32>() / n
}

pub(super) fn mean_chroma(px: &[[f32; 3]]) -> f32 {
    if px.is_empty() {
        return 0.0;
    }
    let sum: f32 =
        px.iter().map(|p| p[0].max(p[1]).max(p[2]) - p[0].min(p[1]).min(p[2])).sum();
    sum / px.len() as f32
}
