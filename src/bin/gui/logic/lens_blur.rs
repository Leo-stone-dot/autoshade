//! Lens Blur, headless: what each lens-blur action means, and the depth
//! estimate's own two halves — the worker body and the landing.
//!
//! The view (`panels/lens_blur.rs`, `canvas.rs`) never mutates a recipe: it
//! emits [`super::Action`]s, and the reach-through into the develop is here.
use autoshade::text::generated::*;


use super::{Effect, UiText};
use autoshade::recipe::EditRecipe;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// What the Lens Blur section asks for. A closed per-domain enum, so this
/// domain never has to match the other verbs' actions.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum LensAction {
    ToggleLensBlur(bool),
    SetBlurAmount(f32),
    SetFocusRange(f32),
    RemoveLensBlur,
    /// Arm / disarm the click-on-the-photo focus pick.
    SetFocusPicking(bool),
    /// Ask for the depth estimate (the worker runs the sidecar).
    EstimateDepth,
    /// The armed focus pick landed on this point, in normalized original-frame
    /// coordinates.
    FocusPicked([f32; 2]),
}

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
    action: LensAction,
    start: Option<(PathBuf, Arc<image::DynamicImage>)>,
    busy: bool,
) -> Vec<Effect> {
    match action {
        LensAction::ToggleLensBlur(on) => with_blur(recipe, |lb| lb.enabled = on),
        LensAction::SetBlurAmount(value) => with_blur(recipe, |lb| lb.amount = value),
        LensAction::SetFocusRange(value) => with_blur(recipe, |lb| lb.range = value),
        LensAction::RemoveLensBlur => match recipe.lens_blur.take() {
            Some(_) => vec![Effect::ArmFocusPick(false), Effect::MarkDirty],
            None => Vec::new(),
        },
        LensAction::SetFocusPicking(on) => {
            if on {
                vec![Effect::ArmFocusPick(true), Effect::DisarmTools]
            } else {
                vec![Effect::ArmFocusPick(false)]
            }
        }
        LensAction::FocusPicked([x, y]) => {
            let mut effects = with_blur(recipe, |lb| {
                lb.focus = [x.clamp(0.0, 1.0), y.clamp(0.0, 1.0)];
            });
            if effects.is_empty() {
                return effects;
            }
            effects.push(Effect::ArmFocusPick(false));
            effects.push(Effect::Status(UiText::of(
                K_LOGIC_FOCUS_SET_THE_BLUR_FOLLOWS_THE_DISTANCE_OF_WHAT,
            )));
            effects
        }
        // The facade routes the look domain itself; this function is only
        // reached with a lens-blur action.
        LensAction::EstimateDepth => {
            if busy {
                return Vec::new();
            }
            let Some((photo, base)) = start else { return Vec::new() };
            vec![
                Effect::Status(UiText::of(K_LOGIC_AI_ESTIMATING_DEPTH_THE_FIRST_RUN_DOWNLOADS_THE)),
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
                K_LOGIC_DEPTH_ESTIMATE_FAILED,
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
            K_LOGIC_DEPTH_ESTIMATED_CLICK_THE_PHOTO_TO_CHOOSE_WHAT_S,
        )));
    } else {
        effects.push(Effect::Status(UiText::of(
            K_LOGIC_DEPTH_RE_ESTIMATED_THE_FOCUS_POINT_AND_SLIDERS_A,
        )));
    }
    effects
}
