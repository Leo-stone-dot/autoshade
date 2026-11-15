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

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(effects: &[Effect]) -> Vec<&'static str> {
        effects
            .iter()
            .map(|e| match e {
                Effect::DisarmPicker(_) => "disarm",
                Effect::MarkDirty => "dirty",
                Effect::Status(_) => "status",
                other => panic!("unexpected effect {other:?}"),
            })
            .collect()
    }

    /// The WB sample solves the (Kelvin, tint) pair, writes it, drops the tool
    /// and asks for a redevelop.
    #[test]
    fn a_white_balance_sample_writes_the_solved_pair() {
        let mut recipe = EditRecipe::default();
        let effects = apply(&mut recipe, PickAction::WhiteBalance([0.9, 0.5, 0.4]));
        assert!(recipe.temperature_k.is_some(), "the solve wrote a Kelvin target");
        assert_ne!(recipe.tint, 0.0, "and a tint");
        assert_eq!(kinds(&effects), vec!["disarm", "dirty", "status"]);
    }

    /// A range sample keeps the tolerance already dialled in, keys only the
    /// colour, disarms and dirties. A stale mask index disarms silently.
    #[test]
    fn a_range_sample_rekeys_the_colour_and_keeps_the_tolerance() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(Default::default());
        recipe.masks[0].range = Some(RangeMask::Color { r: 0.0, g: 0.0, b: 0.0, amount: 0.7, px: 0.0, py: 0.0 });
        let effects =
            apply(&mut recipe, PickAction::RangeColor { mask: 0, rgb: [0.2, 0.4, 0.6], px: 0.3, py: 0.7 });
        match recipe.masks[0].range {
            Some(RangeMask::Color { r, g, b, amount, px, py }) => {
                assert_eq!([r, g, b], [0.2, 0.4, 0.6]);
                assert_eq!(amount, 0.7, "the user's tolerance survives the click");
                assert_eq!((px, py), (0.3, 0.7), "the click position is stored with it");
            }
            other => panic!("expected a color range, got {other:?}"),
        }
        assert_eq!(kinds(&effects), vec!["disarm", "dirty", "status"]);

        let stale = apply(&mut recipe, PickAction::RangeColor { mask: 9, rgb: [1.0, 0.0, 0.0], px: 0.0, py: 0.0 });
        assert_eq!(kinds(&stale), vec!["disarm"], "a deleted mask just puts the tool down");
        assert_eq!(recipe.masks.len(), 1);
    }

    /// A colourful spot becomes a swatch (keyed to the colour clicked), the
    /// tool drops and the line names its slot; a grey spot adds nothing and
    /// stays armed for the retry; a full list refuses and drops.
    #[test]
    fn the_point_color_eyedropper_adds_or_explains() {
        let mut recipe = EditRecipe::default();
        let grey = apply(&mut recipe, PickAction::PointColor([0.5, 0.5, 0.5]));
        assert!(recipe.point_colors.is_empty(), "a grey spot is no swatch");
        assert_eq!(kinds(&grey), vec!["status"], "…and the tool stays armed");

        let added = apply(&mut recipe, PickAction::PointColor([0.8, 0.3, 0.2]));
        assert_eq!(recipe.point_colors.len(), 1, "the swatch is added");
        let chip = autoshade::render::point_color_rgb(&recipe.point_colors[0]);
        for (c, want) in chip.iter().zip([0.8, 0.3, 0.2]) {
            assert!((c - want).abs() < 2e-3, "the swatch is keyed to the colour clicked: {chip:?}");
        }
        assert_eq!(kinds(&added), vec!["disarm", "dirty", "status"]);

        recipe.point_colors = vec![recipe.point_colors[0].clone(); MAX_POINT_COLORS];
        let full = apply(&mut recipe, PickAction::PointColor([0.2, 0.3, 0.8]));
        assert_eq!(recipe.point_colors.len(), MAX_POINT_COLORS, "a full list takes no more");
        assert_eq!(kinds(&full), vec!["disarm", "status"]);
        match &full[1] {
            Effect::Status(text) => {
                assert_eq!(text.args[0].0, "n");
                assert_eq!(text.args[0].1, super::super::Arg::Text(MAX_POINT_COLORS.to_string()));
            }
            other => panic!("expected the refusal line, got {other:?}"),
        }
    }
}
