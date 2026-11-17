//! The three on-canvas eyedroppers, headless: what ONE sample does.
//!
//! The canvas still does the sampling (it owns the preview, the overlay cache
//! and the frame transform); the DECISION — solve the white balance, re-key a
//! range mask's colour, add a Point Color swatch, keep or drop the tool — and
//! the words live here, so each arm is tested without a frame.
use autoshade::text::generated::*;


use super::{Effect, UiText};
use autoshade::recipe::{EditRecipe, MAX_POINT_COLORS, RangeMask};

/// What the user sampled. Coordinates are already normalized to the original
/// frame; a colour is the 5×5 mean the canvas took.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum PickAction {
    WhiteBalance([f32; 3]),
    /// The pending range mask's colour, plus the click position the mask
    /// stores with it.
    RangeColor { mask: usize, rgb: [f32; 3], px: f32, py: f32 },
    PointColor([f32; 3]),
}

/// Which eyedropper the canvas has armed: a landed sample puts it down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Picker {
    WhiteBalance,
    Range,
    PointColor,
}

/// Apply one sample. The effects are ordered: the state first, the line after.
pub(crate) fn apply(recipe: &mut EditRecipe, action: PickAction) -> Vec<Effect> {
    match action {
        PickAction::WhiteBalance(px) => {
            let (k, tint) = autoshade::render::solve_wb_for(px, recipe);
            recipe.temperature_k = Some(k);
            recipe.tint = tint;
            vec![
                Effect::DisarmPicker(Picker::WhiteBalance),
                Effect::MarkDirty,
                Effect::Status(UiText::with(
                    K_LOGIC_WB_EYEDROPPER_K_TINT_FINE_TUNE_IN_THE_TONE_SECTI,
                    &[("k", &format!("{k:.0}")), ("tint", &format!("{tint:+.0}"))],
                )),
            ]
        }
        PickAction::RangeColor { mask, rgb, px, py } => {
            // A stale index (mask deleted mid-pick) disarms and does nothing.
            if mask >= recipe.masks.len() {
                return vec![Effect::DisarmPicker(Picker::Range)];
            }
            // Keep the tolerance the user already dialled in; only re-key the
            // colour.
            let amount = match recipe.masks[mask].range {
                Some(RangeMask::Color { amount, .. }) => amount,
                _ => 0.5,
            };
            recipe.masks[mask].range =
                Some(RangeMask::Color { r: rgb[0], g: rgb[1], b: rgb[2], amount, px, py });
            vec![
                Effect::DisarmPicker(Picker::Range),
                Effect::MarkDirty,
                Effect::Status(UiText::of(
                    K_LOGIC_COLOR_RANGE_SAMPLED_THE_TOLERANCE_SLIDER_ADJUSTS,
                )),
            ]
        }
        PickAction::PointColor(px) => {
            if recipe.point_colors.len() >= MAX_POINT_COLORS {
                return vec![
                    Effect::DisarmPicker(Picker::PointColor),
                    Effect::Status(UiText::with(
                        K_LOGIC_POINT_COLOR_AT_MOST_SWATCHES,
                        &[("n", &MAX_POINT_COLORS.to_string())],
                    )),
                ];
            }
            // A near-grey spot gets no swatch (`point_color_at` gives it to
            // none): a swatch of it would be four sliders that move nothing,
            // so say why and stay armed for the retry.
            let Some(swatch) = autoshade::render::point_color_at(px) else {
                return vec![Effect::Status(UiText::of(
                    K_LOGIC_POINT_COLOR_THAT_SPOT_IS_ALMOST_GREY_SO_NO_SWATC,
                ))];
            };
            recipe.point_colors.push(swatch);
            let n = recipe.point_colors.len().to_string();
            vec![
                Effect::DisarmPicker(Picker::PointColor),
                Effect::MarkDirty,
                Effect::Status(UiText::with(
                    K_LOGIC_POINT_COLOR_SWATCH_ADDED_ITS_SLIDERS_ARE_IN_THE,
                    &[("n", &n)],
                )),
            ]
        }
    }
}
