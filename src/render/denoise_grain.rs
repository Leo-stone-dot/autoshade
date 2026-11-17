//! Return the frame's own luminance residual after demosaic, in linear light.
//! One scalar goes along RGB grey; channel differences cannot acquire noise.
use anyhow::Result;
use rayon::prelude::*;

use super::{ExportColorSpace, linear_to_srgb, srgb_to_linear};

pub(super) fn weights(space: ExportColorSpace) -> [f32; 3] {
    let row = super::rgb_to_xyz(super::space_primaries(space), super::D65_XY)[1];
    let sum = row.iter().sum::<f32>();
    row.map(|v| v / sum)
}

fn luminance(rgb: [f32; 3], weights: [f32; 3]) -> f32 {
    rgb[0] * weights[0] + rgb[1] * weights[1] + rgb[2] * weights[2]
}

/// The closure owns the expensive develop. Endpoints and fallback never call
/// it, and its RGB allocation dies here: only one f32 Y plane survives.
pub(super) fn capture_original(
    strength: f32,
    applicable: bool,
    weights: [f32; 3],
    develop: impl FnOnce() -> Result<Vec<[f32; 3]>>,
) -> Result<Option<Vec<f32>>> {
    if !(strength > 0.0 && strength < 1.0 && applicable) {
        return Ok(None);
    }
    let original = develop()?;
    Ok(Some(original.par_iter().map(|p| luminance(p.map(srgb_to_linear), weights)).collect()))
}

/// Buffers in every working space use the sRGB transfer. Do not clip here:
/// negative grain is real; the final output container owns its gamut boundary.
pub(super) fn return_luminance(
    clean: &mut [[f32; 3]],
    original_y: &[f32],
    share: f32,
    weights: [f32; 3],
) {
    if share == 0.0 {
        return;
    }
    assert_eq!(clean.len(), original_y.len(), "grain return must use the identical developed frame");
    clean.par_iter_mut().zip(original_y.par_iter()).for_each(|(px, original)| {
        let linear = px.map(srgb_to_linear);
        let delta = share * (original - luminance(linear, weights));
        *px = linear.map(|c| linear_to_srgb(c + delta));
    });
}
