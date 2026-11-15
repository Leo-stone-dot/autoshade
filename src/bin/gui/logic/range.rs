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

#[cfg(test)]
mod tests {
    use super::*;
    use autoshade::recipe::LocalAdjustment;

    fn recipe_with(count: usize) -> EditRecipe {
        let mut r = EditRecipe::default();
        for _ in 0..count {
            r.masks.push(LocalAdjustment::default());
        }
        r
    }

    #[test]
    fn the_reference_is_the_prefix_before_the_mask() {
        let mut r = recipe_with(3);
        r.exposure_ev = 0.5;
        let pre = reference_recipe(&r, 2);
        assert_eq!(pre.masks.len(), 2, "masks before mask 2 only");
        assert_eq!(pre.exposure_ev, 0.5, "the develop itself rides");
        assert_eq!(reference_recipe(&r, 0).masks.len(), 0, "mask 0 is the whole image");
    }

    #[test]
    fn every_geometry_field_is_stripped() {
        let mut r = recipe_with(1);
        r.straighten_deg = 2.0;
        r.lens_distortion = 0.1;
        r.ca_r = 0.4;
        r.ca_b = -0.3;
        r.crop = Some(autoshade::recipe::Crop { left: 0.1, top: 0.1, right: 0.9, bottom: 0.9 });
        let pre = reference_recipe(&r, 1);
        assert_eq!((pre.straighten_deg, pre.lens_distortion), (0.0, 0.0));
        assert_eq!((pre.ca_r, pre.ca_b), (0.0, 0.0));
        assert!(pre.crop.is_none());
    }

    #[test]
    fn the_live_recipe_is_not_modified() {
        let mut r = recipe_with(2);
        r.straighten_deg = 3.0;
        let _ = reference_recipe(&r, 1);
        assert_eq!(r.masks.len(), 2, "the caller's recipe is untouched");
        assert_eq!(r.straighten_deg, 3.0);
    }
}
