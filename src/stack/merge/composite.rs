//! The three merges that never form a radiance.
//!
//! Exposure fusion and the focus stack are the SAME operator with different
//! weights — pick each frequency band from wherever it looks best, through
//! [`pyramid::blend_rgb`] — and they are written that way on purpose, because
//! what a photographer means by "blend where it is best" is one idea and only
//! the definition of "best" changes. The noise stack is not that idea at all:
//! it is an estimate of one unchanging scene from repeated readings, so it
//! averages in linear light and its only judgement is which readings to throw
//! out.

use super::{detail, saturation, well_exposed};
use crate::render::{gauss_blur_plane, linear_to_srgb, srgb_to_linear};
use crate::stack::pyramid;

/// The plane the local measures are taken on: the mean of the three ENCODED
/// channels.
///
/// NOT [`crate::stack::luma`], which is linear luminance and is the right
/// thing for measuring LIGHT. Everything in this file is a judgement about how
/// a picture looks, made in the space the picture is displayed in, and using a
/// linear luminance here would quietly weight a shadow's detail at a fraction
/// of an identical highlight's.
fn grey(f: &[[f32; 3]]) -> Vec<f32> {
    f.iter().map(|p| (p[0] + p[1] + p[2]) / 3.0).collect()
}

/// A floor under every weight, so a region that is flat, grey and dull in
/// every frame still has to come from somewhere rather than from a division by
/// zero. It is small enough to be invisible wherever any frame has an opinion.
const FLOOR: f32 = 1e-4;

/// Mertens' exposure fusion: each band of the picture taken from wherever it
/// is best exposed, with no radiance in between.
///
/// Three weights multiplied, which is Mertens' own recipe and each one covers
/// a case the others miss: DETAIL, so a blown-out region loses to one that
/// still has texture; SATURATION, so a washed-out region loses to one that
/// still has colour, which is what catches a highlight on its way to clipping
/// before the texture has gone; and WELL-EXPOSEDNESS, so what is left is
/// decided by which frame put this pixel nearest the middle.
pub(super) fn fuse(
    frames: &[Vec<[f32; 3]>],
    cover: &[Vec<bool>],
    w: usize,
    h: usize,
) -> Vec<[f32; 3]> {
    let weights: Vec<Vec<f32>> = frames
        .iter()
        .zip(cover)
        .map(|(f, c)| {
            let d = detail(&grey(f), w, h);
            (0..w * h)
                .map(|k| {
                    if !c[k] {
                        return 0.0;
                    }
                    let exposed: f32 = f[k].iter().copied().map(well_exposed).product();
                    (d[k] + FLOOR) * (saturation(&f[k]) + FLOOR) * exposed
                })
                .collect()
        })
        .collect();
    pyramid::blend_rgb(frames, &weights, w, h, pyramid::depth(w, h))
}

/// How far a sharpness measure is spread before it decides anything, as a
/// fraction of the frame's short side.
///
/// A Laplacian is large on an EDGE and zero on the smooth ground between
/// edges, so a weight map taken from it raw is a stencil of edges: the blend
/// would pick frame A along a contour and frame B a pixel to either side of
/// it, which is not what "this region is in focus" means. Blurring first turns
/// the measure into a statement about a neighbourhood. 0.6 % of the short side
/// is about 20 px on a 24 MP frame — wider than any edge, narrower than the
/// smallest thing a photographer focuses on deliberately.
const SHARPNESS_SPREAD: f32 = 0.006;

/// How hard the sharpest frame wins.
///
/// Cubing is a compromise with a reason on both sides. A linear weight blends
/// a sharp frame with a soft one wherever they are close, and a blend of sharp
/// and soft is soft — the whole failure a focus stack exists to avoid. A
/// winner-takes-all picks a single frame per pixel and shows every place it
/// changed its mind. The cube leaves a clear region entirely to the frame that
/// resolved it while still crossfading where two frames genuinely agree.
const SHARPNESS_DECISIVENESS: i32 = 3;

/// A focus stack: each band taken from the frame that resolved it.
pub(super) fn focus(
    frames: &[Vec<[f32; 3]>],
    cover: &[Vec<bool>],
    w: usize,
    h: usize,
) -> Vec<[f32; 3]> {
    let sigma = (SHARPNESS_SPREAD * w.min(h) as f32).max(2.0);
    let weights: Vec<Vec<f32>> = frames
        .iter()
        .zip(cover)
        .map(|(f, c)| {
            let d = gauss_blur_plane(&detail(&grey(f), w, h), w, h, sigma);
            (0..w * h)
                .map(|k| {
                    if c[k] {
                        (d[k] + FLOOR).powi(SHARPNESS_DECISIVENESS)
                    } else {
                        0.0
                    }
                })
                .collect()
        })
        .collect();
    pyramid::blend_rgb(frames, &weights, w, h, pyramid::depth(w, h))
}

/// How many robust spreads away a reading has to be before it stops counting.
///
/// Three, so ordinary noise is barely touched — a Gaussian puts 99.7 % of its
/// readings inside three sigma — while something that is not noise at all,
/// like a person who walked through one frame of the stack, is gone.
const GHOST_SPREADS: f32 = 3.0;

/// The noise stack: repeated readings of one scene, averaged in LINEAR light,
/// with the readings that disagree withdrawn.
///
/// Linear light because that is where a photon count adds. Averaging the
/// ENCODED values would average through a curve and darken every edge between
/// two brightnesses — the same mistake as resizing in gamma space, and just as
/// invisible until it is pointed out.
///
/// The withdrawal is per pixel and per channel, against the MEDIAN of that
/// pixel's readings rather than their mean, because the mean is exactly what a
/// ghost moves. The spread it is measured in is that pixel's own median
/// absolute deviation, floored at a frame-wide estimate: a per-pixel spread
/// from three or four frames is a crude number, and the floor stops a pixel
/// whose readings happen to agree perfectly from rejecting its neighbours over
/// a rounding difference.
pub(super) fn average(
    frames: &[Vec<[f32; 3]>],
    cover: &[Vec<bool>],
    w: usize,
    h: usize,
) -> Vec<[f32; 3]> {
    let n = w * h;
    let floor = frame_spread(frames, cover, n);
    (0..n)
        .map(|k| {
            let mut out = [0.0f32; 3];
            for (c, o) in out.iter_mut().enumerate() {
                let mut v: Vec<f32> = frames
                    .iter()
                    .zip(cover)
                    .filter(|(_, cv)| cv[k])
                    .map(|(f, _)| srgb_to_linear(f[k][c]))
                    .collect();
                if v.is_empty() {
                    *o = frames[0][k][c];
                    continue;
                }
                let med = median(&mut v);
                let spread = (mad(&v, med) * GHOST_SPREADS).max(floor);
                let (mut num, mut den) = (0.0f32, 0.0f32);
                for x in &v {
                    let z = (x - med) / spread;
                    let wt = (-z * z).exp();
                    num += wt * x;
                    den += wt;
                }
                *o = linear_to_srgb(if den > 0.0 { num / den } else { med });
            }
            out
        })
        .collect()
}

/// A frame-wide floor for the rejection spread: the median, over a sample of
/// pixels, of how far that pixel's readings sit from their own median.
///
/// Sampled rather than exhaustive because it is a scale, not a measurement —
/// every 97th pixel of a 24 MP frame is still a quarter of a million readings,
/// and 97 is prime so the sample cannot land on a column or a Bayer phase.
fn frame_spread(frames: &[Vec<[f32; 3]>], cover: &[Vec<bool>], n: usize) -> f32 {
    let mut s: Vec<f32> = (0..n)
        .step_by(97)
        .filter_map(|k| {
            let mut v: Vec<f32> = frames
                .iter()
                .zip(cover)
                .filter(|(_, cv)| cv[k])
                .map(|(f, _)| srgb_to_linear(f[k][1]))
                .collect();
            (v.len() >= 2).then(|| {
                let m = median(&mut v);
                mad(&v, m)
            })
        })
        .collect();
    if s.is_empty() {
        return 1e-4;
    }
    (median(&mut s) * GHOST_SPREADS).max(1e-5)
}

fn median(v: &mut [f32]) -> f32 {
    let mid = v.len() / 2;
    let (_, m, _) = v.select_nth_unstable_by(mid, f32::total_cmp);
    *m
}

fn mad(v: &[f32], med: f32) -> f32 {
    let mut d: Vec<f32> = v.iter().map(|x| (x - med).abs()).collect();
    median(&mut d)
}
