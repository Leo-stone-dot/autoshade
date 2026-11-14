//! Lens Blur, headless: what each lens-blur action means, and the depth
//! estimate's own two halves — the worker body and the landing.
//!
//! The view (`panels/lens_blur.rs`, `canvas.rs`) never mutates a recipe: it
//! emits [`super::Action`]s, and the reach-through into the develop is here.

use super::{Action, Effect, UiText};
use autoshade::recipe::EditRecipe;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// `Some` while the card carries a blur that is switched on — the panel's
/// section title's "active" dot, and the only read the view needs.
pub(crate) fn active(recipe: &EditRecipe) -> bool {
    recipe.lens_blur.as_ref().is_some_and(|l| l.draws())
}

/// One lens-blur action's meaning. `start` carries the data the depth estimate
/// needs (the photo and its original-frame base) so this function can stay a
/// pure state change; `busy` is the one in-flight guard the view also greys.
pub(crate) fn apply(
    recipe: &mut EditRecipe,
    action: Action,
    start: Option<(PathBuf, Arc<image::DynamicImage>)>,
    busy: bool,
) -> Vec<Effect> {
    match action {
        Action::ToggleLensBlur(on) => with_blur(recipe, |lb| lb.enabled = on),
        Action::SetBlurAmount(value) => with_blur(recipe, |lb| lb.amount = value),
        Action::SetFocusRange(value) => with_blur(recipe, |lb| lb.range = value),
        Action::RemoveLensBlur => match recipe.lens_blur.take() {
            Some(_) => vec![Effect::ArmFocusPick(false), Effect::MarkDirty],
            None => Vec::new(),
        },
        Action::SetFocusPicking(on) => {
            if on {
                vec![Effect::ArmFocusPick(true), Effect::DisarmTools]
            } else {
                vec![Effect::ArmFocusPick(false)]
            }
        }
        Action::FocusPicked([x, y]) => {
            let mut effects = with_blur(recipe, |lb| {
                lb.focus = [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)];
            });
            if effects.is_empty() {
                return effects;
            }
            effects.push(Effect::ArmFocusPick(false));
            effects.push(Effect::Status(UiText::of(
                "Focus set — the blur follows the distance of what you clicked",
            )));
            effects
        }
        // The facade routes the look domain itself; this function is only
        // reached with a lens-blur action.
        Action::Look(_) | Action::References(_) => Vec::new(),
        Action::EstimateDepth => {
            if busy {
                return Vec::new();
            }
            let Some((photo, base)) = start else { return Vec::new() };
            vec![
                Effect::Status(UiText::of("AI estimating depth… (the first run downloads the model)")),
                Effect::EstimateDepth { photo, base },
            ]
        }
    }
}

/// The recipe change plus its dirty mark, or nothing when the card carries no
/// blur (a stale view frame cannot mint one).
fn with_blur(recipe: &mut EditRecipe, change: impl FnOnce(&mut autoshade::render::LensBlur)) -> Vec<Effect> {
    let Some(lb) = recipe.lens_blur.as_mut() else { return Vec::new() };
    change(lb);
    vec![Effect::MarkDirty]
}

/// The depth worker's body: stage the original-frame preview, claim a raster
/// name in the photo's develop folder, run the sidecar. A failed run releases
/// the (0-byte) claimed name. No UI, no app state.
pub(crate) fn estimate_depth(photo: &Path, base: &image::DynamicImage) -> anyhow::Result<PathBuf> {
    let input = crate::masks::stage_preview_png(base, "depth")?;
    let out = autoshade::store::claim_raster(photo, "depth")?;
    let run = autoshade::depth::estimate_depth(&autoshade::config::Config::load(), &input, &out);
    let _ = std::fs::remove_file(&input);
    run.inspect_err(|_| {
        let _ = std::fs::remove_file(&out);
    })?;
    Ok(out)
}

/// A finished estimate: point the card's blur at the new map, keeping the
/// focus and dials already chosen; a FIRST estimate arms the focus pick (and
/// disarms every other canvas tool), while a re-estimate leaves the user's
/// chosen focus alone.
pub(crate) fn landed(recipe: &mut EditRecipe, res: anyhow::Result<PathBuf>) -> Vec<Effect> {
    let path = match res {
        Ok(path) => path,
        Err(e) => {
            return vec![Effect::ToastError(UiText::with(
                "Depth estimate failed: {e}",
                &[("e", &format!("{e:#}"))],
            ))];
        }
    };
    let first = recipe.lens_blur.is_none();
    let lb = recipe.lens_blur.get_or_insert_with(Default::default);
    lb.depth_map = path.to_string_lossy().into_owned();
    lb.enabled = true;
    let mut effects = vec![Effect::MarkDirty];
    if first {
        effects.push(Effect::DisarmTools);
        effects.push(Effect::ArmFocusPick(true));
        effects.push(Effect::Status(UiText::of(
            "Depth estimated — click the photo to choose what stays sharp",
        )));
    } else {
        effects.push(Effect::Status(UiText::of(
            "Depth re-estimated — the focus point and sliders are kept",
        )));
    }
    effects
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A path VALUE, never a file: the landing and the actions only store
    /// spellings, and a relative one keeps this test off the filesystem.
    fn depth(name: &str) -> PathBuf {
        PathBuf::from(name)
    }

    fn statuses(effects: &[Effect]) -> Vec<&'static str> {
        effects
            .iter()
            .filter_map(|e| match e {
                Effect::Status(t) => Some(t.key),
                _ => None,
            })
            .collect()
    }

    /// A first estimate lands the map on a fresh blur and arms the focus pick;
    /// a re-estimate swaps the map and keeps focus and dials; a failure says
    /// why and leaves the recipe byte-identical. No app, no frame.
    #[test]
    fn the_landing_arms_once_and_a_failure_changes_nothing() {
        let mut recipe = EditRecipe::default();
        let effects = landed(&mut recipe, Ok(depth("depth.png")));
        let lb = recipe.lens_blur.clone().expect("the first estimate lands a blur");
        assert_eq!(lb.depth_map, "depth.png");
        assert!(lb.enabled);
        assert!(effects.iter().any(|e| matches!(e, Effect::ArmFocusPick(true))));
        assert!(effects.iter().any(|e| matches!(e, Effect::DisarmTools)));
        assert_eq!(
            statuses(&effects),
            vec!["Depth estimated — click the photo to choose what stays sharp"]
        );

        let chosen = autoshade::render::LensBlur { focus: [0.2, 0.8], amount: 70.0, range: 30.0, ..lb };
        recipe.lens_blur = Some(chosen.clone());
        let effects = landed(&mut recipe, Ok(depth("depth-2.png")));
        let again = recipe.lens_blur.clone().unwrap();
        assert_eq!(again.depth_map, "depth-2.png");
        assert_eq!((again.focus, again.amount, again.range), (chosen.focus, chosen.amount, chosen.range));
        assert!(!effects.iter().any(|e| matches!(e, Effect::ArmFocusPick(true))), "a re-estimate does not re-arm the pick");

        let before = recipe.clone();
        let effects = landed(&mut recipe, Err(anyhow::anyhow!("no GPU")));
        assert_eq!(recipe, before, "a failed estimate changes nothing");
        assert!(matches!(effects.as_slice(), [Effect::ToastError(_)]));
    }

    /// The dials clamp, the toggle only touches an existing blur, and removing
    /// the blur also disarms the focus pick.
    #[test]
    fn the_edits_stay_inside_the_blur_and_mark_the_recipe_dirty() {
        let mut recipe = EditRecipe::default();
        assert!(apply(&mut recipe, Action::ToggleLensBlur(true), None, false).is_empty(), "nothing to toggle yet");
        assert!(recipe.lens_blur.is_none());

        landed(&mut recipe, Ok(depth("a.png")));
        let effects = apply(&mut recipe, Action::SetBlurAmount(250.0), None, false);
        assert!(matches!(effects.as_slice(), [Effect::MarkDirty]));
        // The reflection layer assigns; the RANGE is the library's rule, applied
        // by `EditRecipe::clamp` (the same call the develop panel makes after
        // an edit and every save makes).
        assert_eq!(recipe.lens_blur.as_ref().unwrap().amount, 250.0);
        recipe.clamp();
        assert_eq!(recipe.lens_blur.as_ref().unwrap().amount, 100.0, "the library clamps the dial");

        let effects = apply(&mut recipe, Action::SetFocusPicking(true), None, false);
        assert!(effects.iter().any(|e| matches!(e, Effect::ArmFocusPick(true))));
        assert!(effects.iter().any(|e| matches!(e, Effect::DisarmTools)));

        let effects = apply(&mut recipe, Action::RemoveLensBlur, None, false);
        assert!(recipe.lens_blur.is_none());
        assert!(effects.iter().any(|e| matches!(e, Effect::ArmFocusPick(false))));

        let effects = apply(&mut recipe, Action::RemoveLensBlur, None, false);
        assert!(effects.is_empty(), "removing an absent blur is a no-op");
    }

    /// A depth run starts only with somewhere to put it and nothing in flight.
    #[test]
    fn the_estimate_needs_a_photo_its_base_and_a_free_worker() {
        let photo = depth("photo.arw");
        let base = Arc::new(image::DynamicImage::new_rgb8(6, 4));
        assert!(apply(&mut EditRecipe::default(), Action::EstimateDepth, None, false).is_empty());
        assert!(apply(
            &mut EditRecipe::default(),
            Action::EstimateDepth,
            Some((photo.clone(), base.clone())),
            true,
        )
        .is_empty());
        let effects = apply(
            &mut EditRecipe::default(),
            Action::EstimateDepth,
            Some((photo.clone(), base)),
            false,
        );
        assert!(matches!(effects.as_slice(), [Effect::Status(_), Effect::EstimateDepth { .. }]));
    }

    /// The focus pick is a recipe edit with a confirmation line.
    #[test]
    fn a_picked_focus_lands_normalized_and_says_so() {
        let mut recipe = EditRecipe::default();
        landed(&mut recipe, Ok(depth("focus.png")));
        let effects = apply(&mut recipe, Action::FocusPicked([1.7, -0.4]), None, false);
        assert_eq!(recipe.lens_blur.as_ref().unwrap().focus, [1.0, 0.0]);
        assert!(effects.iter().any(|e| matches!(e, Effect::ArmFocusPick(false))));
        assert!(matches!(statuses(&effects)[..], ["Focus set — the blur follows the distance of what you clicked"]));
    }
}
