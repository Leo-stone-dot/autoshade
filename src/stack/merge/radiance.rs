//! The HDR merge: a bracket into one measurement of light, and a record of how
//! much light that turned out to be.
//!
//! Two things come out of here and the second is the reason the first is
//! usable. The estimate is a RADIANCE in the reference frame's own exposure,
//! so 1.0 is still the reference's white and everything the darker frames
//! recovered sits above it — which is exactly the range a 16-bit master
//! cannot hold. So the estimate is folded back into [0, 1] by a shoulder, and
//! how far it was folded is returned as `headroom_ev` for the recipe to carry
//! in `hdr_max_ev`. The pixels stay a photograph every existing path can
//! open, and the stops are not lost, they are written down.

use super::trustworthy;
use crate::render::{linear_to_srgb, srgb_to_linear};

/// Merge a bracket, returning the frame and the stops it recovered.
pub(super) fn merge(
    frames: &[Vec<[f32; 3]>],
    cover: &[Vec<bool>],
    exposures: &[f32],
    w: usize,
    h: usize,
) -> (Vec<[f32; 3]>, f32) {
    let radiance = estimate(frames, cover, exposures, w * h);
    let headroom = headroom_of(&radiance);
    let curve = Shoulder::reaching(headroom);
    let pixels = radiance
        .iter()
        .map(|r| {
            let t = curve.apply(*r);
            let mut o = [0.0f32; 3];
            for (c, v) in o.iter_mut().enumerate() {
                *v = linear_to_srgb(t[c].clamp(0.0, 1.0));
            }
            o
        })
        .collect();
    (pixels, headroom)
}

/// The radiance every frame agrees on, in the REFERENCE frame's own exposure.
///
/// Each frame's samples are divided by its own exposure to put them on the
/// reference's scale, and weighted BY that same exposure because a frame that
/// collected twice the light carries twice the confidence per sample — which
/// is Debevec's weighting, and it is why a bracket is worth shooting rather
/// than just pushing one frame.
fn estimate(
    frames: &[Vec<[f32; 3]>],
    cover: &[Vec<bool>],
    exposures: &[f32],
    n: usize,
) -> Vec<[f32; 3]> {
    let mut num = vec![[0.0f32; 3]; n];
    let mut den = vec![0.0f32; n];
    for ((f, cov), ev) in frames.iter().zip(cover).zip(exposures) {
        let (scale, conf) = (2f32.powf(-ev), 2f32.powf(*ev));
        for k in 0..n {
            if !cov[k] {
                continue;
            }
            // ONE weight for the whole pixel, taken as the least trustworthy
            // of its three channels — never one weight per channel. A
            // per-channel weight withdraws a single channel of a pixel on its
            // own, so the pixel is rebuilt from a different mixture of frames
            // in red than in green, and its HUE moves. That is how a merged
            // sunset goes magenta right where it clips, and it is invisible in
            // any per-channel test.
            let t = f[k].iter().copied().map(trustworthy).fold(f32::INFINITY, f32::min);
            if t <= 0.0 {
                continue;
            }
            let wt = t * conf;
            den[k] += wt;
            for (c, acc) in num[k].iter_mut().enumerate() {
                *acc += wt * srgb_to_linear(f[k][c]) * scale;
            }
        }
    }
    (0..n)
        .map(|k| {
            if den[k] > 0.0 {
                let mut o = num[k];
                for v in o.iter_mut() {
                    *v /= den[k];
                }
                return o;
            }
            // No frame had anything to say here: the pixel is clipped in all
            // of them, or crushed in all of them. Falling back to an average
            // would be averaging two kinds of nothing, so take the ONE frame
            // that is least far from the middle — the darkest exposure for a
            // clipped pixel, the brightest for a crushed one — and take it
            // whole.
            let pick = (0..frames.len())
                .filter(|i| cover[*i][k])
                .min_by(|&i, &j| middling(&frames[i][k], exposures[i])
                    .total_cmp(&middling(&frames[j][k], exposures[j])))
                .unwrap_or(0);
            let s = 2f32.powf(-exposures[pick]);
            let mut o = [0.0f32; 3];
            for (c, v) in o.iter_mut().enumerate() {
                *v = srgb_to_linear(frames[pick][k][c]) * s;
            }
            o
        })
        .collect()
}

/// How far this sample sits from a mid-grey reading, once its own exposure is
/// divided out — the tie-break for a pixel no frame could measure.
fn middling(p: &[f32; 3], ev: f32) -> f32 {
    const MID: f32 = 0.18;
    (crate::stack::luma(p) * 2f32.powf(-ev) - MID).abs()
}

/// How many stops of highlight the merge recovered above the reference frame's
/// own white.
///
/// The 99.9th percentile rather than the maximum. One hot pixel, or one
/// specular glint off a chrome bumper, is a legitimate radiance and a terrible
/// headroom: taking the maximum would let it push the entire picture down the
/// shoulder to make room for a highlight nobody is going to look into. The
/// clamp at 8 EV is the same one `EditRecipe::hdr_max_ev` applies, so the
/// number handed over is one the recipe will keep.
fn headroom_of(radiance: &[[f32; 3]]) -> f32 {
    if radiance.is_empty() {
        return 0.0;
    }
    let mut v: Vec<f32> = radiance.iter().map(|p| p[0].max(p[1]).max(p[2])).collect();
    let at = ((v.len() - 1) as f32 * 0.999) as usize;
    let (_, top, _) = v.select_nth_unstable_by(at, f32::total_cmp);
    top.max(1.0).log2().clamp(0.0, 8.0)
}

/// The curve that folds recovered highlights back into a frame which still has
/// to fit in [0, 1].
///
/// **In STOPS, not in linear light, and applied to the pixel's brightest
/// channel rather than to each channel on its own.** Both of those were got
/// wrong on the first attempt and both are visible in the photograph, so both
/// are worth keeping written down:
///
/// * a hyperbola in LINEAR light with its knee at 0.8 leaves 0.2 of the output
///   range for everything above white — which is 0.09 of the sRGB range. A
///   2.6 EV recovery came back spanning sRGB 0.990 to 1.000: the window the
///   merge had just recovered was still, to an eye and to a measurement, flat
///   white. Vision is logarithmic and a rolloff has to be too;
/// * applied per channel, the curve compresses a clipping highlight's red more
///   than its blue, because red sits further up the curve, and the pixel's
///   ratios move. Measured on this module's own warm highlight: linear
///   1 : 0.55 : 0.25 came back 1 : 0.975 : 0.750, which is to say white. So
///   the curve reads the BRIGHTEST channel and scales all three by the same
///   factor, which is the one way to change a pixel's brightness without
///   touching its colour.
///
/// Identity below the knee, a hyperbola above it, joined so the slope does not
/// jump, and scaled so the top of the recovered range lands exactly on white.
struct Shoulder {
    /// Stops below white at which the rolloff starts.
    knee: f32,
    /// The hyperbola's scale; `0.0` means there was no headroom and the curve
    /// is the identity.
    a: f32,
}

impl Shoulder {
    /// Two stops below white, which on an sRGB output is 0.54 — so a picture's
    /// midtones and all of its shadows pass through exactly as the reference
    /// frame had them, and the top two stops are what absorb the recovery.
    /// That is where a film shoulder puts it and where an eye goes looking for
    /// it; a knee much lower would produce the flat grey an HDR merge is
    /// infamous for, and would do it to pixels that never needed the help.
    const KNEE_EV: f32 = 2.0;

    /// The shoulder that maps `headroom_ev` stops above white onto white.
    ///
    /// With no headroom there is nothing to fold and the curve is the identity
    /// — a bracket of one exposure must come out of an HDR merge looking like
    /// the frame it went in as.
    fn reaching(headroom_ev: f32) -> Shoulder {
        let k = Self::KNEE_EV;
        // Solving `−k + a·(H+k)/(a + H + k) = 0` for `a`, with the slope at
        // the knee being `a²/a² = 1` so the join is smooth.
        let a = if headroom_ev > 1e-4 { k * (headroom_ev + k) / headroom_ev } else { 0.0 };
        Shoulder { knee: k, a }
    }

    /// Where a brightness in stops-below-white comes out, in the same units.
    fn stops(&self, x: f32) -> f32 {
        if self.a <= 0.0 || x <= -self.knee {
            return x;
        }
        let u = x + self.knee;
        -self.knee + self.a * u / (self.a + u)
    }

    /// The whole pixel, scaled by what the rolloff does to its brightest
    /// channel.
    fn apply(&self, p: [f32; 3]) -> [f32; 3] {
        let m = p[0].max(p[1]).max(p[2]);
        if self.a <= 0.0 || m <= 0.0 {
            return p;
        }
        let x = m.log2();
        if x <= -self.knee {
            return p;
        }
        let s = 2f32.powf(self.stops(x)) / m;
        [p[0] * s, p[1] * s, p[2] * s]
    }
}
