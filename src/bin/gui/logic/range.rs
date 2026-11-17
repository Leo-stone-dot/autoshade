//! The range masks' reference develop, headless: the recipe a mask's colour
//! is judged against — everything BEFORE it (the engine's stacking rule), with
//! all geometry stripped so the sample is taken in the same frame the overlay
//! cache builds.
//!
//! ONE builder for the coverage overlay and the range eyedropper: they share a
//! one-slot cache compared by recipe equality, and a hand copy once forgot the
//! manual CA pair, missing the cache on every click under non-zero Red/cyan.

use autoshade::recipe::EditRecipe;

/// The prefix recipe mask `i` is judged against.
pub(crate) fn reference_recipe(recipe: &EditRecipe, i: usize) -> EditRecipe {
    let mut pre = recipe.clone();
    pre.masks.truncate(i);
    pre.straighten_deg = 0.0;
    pre.lens_distortion = 0.0;
    pre.ca_r = 0.0;
    pre.ca_b = 0.0;
    pre.crop = None;
    pre
}
