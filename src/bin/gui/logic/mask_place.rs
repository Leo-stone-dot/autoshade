//! Committing a dragged mask shape into the recipe: what the box drag MEANS.
//!
//! The canvas owns the drag (view-space start/end, the transform out of the
//! view, the live outline). This module owns the commit: redraw keeps a
//! radial's tuned slider state and only replaces its area; a component joins
//! the mask it was drawn for; a fresh mask is created (with the name the view
//! rendered) when no owner survives. Selection and the line come back as data.
use autoshade::text::generated::*;


use super::{Effect, UiText};
use autoshade::recipe::{EditRecipe, LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry};

/// What the drag was armed for — the view's `PlaceTarget` with no view types.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Place {
    NewMask,
    Redraw(usize),
    Component(usize, MaskCombine),
}

/// Where the selection lands after the commit; `Keep` is a redraw, which
/// leaves the panel's selection exactly where it was.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Selection {
    Keep,
    Mask(usize),
    Component(usize, usize),
}

/// What the frame must now apply: the selection move and the ordered effects.
pub(crate) struct Committed {
    pub(crate) selection: Selection,
    pub(crate) effects: Vec<Effect>,
}

/// Commit one dragged shape. `new_mask_name` is the already-rendered default
/// name a fresh mask gets (the view renders it — recipes store display names).
pub(crate) fn commit(
    recipe: &mut EditRecipe,
    place: Place,
    geom: MaskGeometry,
    new_mask_name: String,
) -> Committed {
    let mut effects = vec![Effect::MarkDirty];
    match place {
        Place::Redraw(i) if i < recipe.masks.len() => {
            let mut geom = geom;
            // Redraw replaces the AREA only: a radial's tuned feather /
            // roundness / flipped / angle are slider+handle state, and the two
            // carried attributes ride the same rule — nothing in this app can
            // even show them, so resetting an imported mask's own Midpoint to
            // 50 would be a loss with no surface to notice it on.
            if let (
                MaskGeometry::Radial {
                    feather, roundness, flipped, angle, midpoint, mask_version, ..
                },
                MaskGeometry::Radial {
                    feather: kept_f,
                    roundness: kept_r,
                    flipped: kept_fl,
                    angle: kept_a,
                    midpoint: kept_m,
                    mask_version: kept_v,
                    ..
                },
            ) = (&mut geom, &recipe.masks[i].mask)
            {
                *feather = *kept_f;
                *roundness = *kept_r;
                *flipped = *kept_fl;
                *angle = *kept_a;
                *midpoint = *kept_m;
                *mask_version = *kept_v;
            }
            recipe.masks[i].mask = geom;
            effects.push(Effect::Status(UiText::of(
                K_LOGIC_MASK_AREA_REDRAWN_ITS_EXISTING_ADJUSTMENTS_NOW_A,
            )));
            Committed { selection: Selection::Keep, effects }
        }
        Place::Component(i, mode) if i < recipe.masks.len() => {
            let m = &mut recipe.masks[i];
            m.components.push(MaskComponent { inverted: false, geometry: geom, mode });
            effects.push(Effect::Status(UiText::of(
                K_LOGIC_SHAPE_ADDED_TO_THIS_MASK_DRAG_ITS_KNOBS_TO_ADJUS,
            )));
            Committed { selection: Selection::Component(i, m.components.len() - 1), effects }
        }
        // NewMask — and the two index-carrying arms whose mask vanished
        // mid-drag (an async recipe replace): a redraw degrades to a fresh mask
        // (the pre-v0.22 behaviour); a component with no owner joins nothing
        // and would vanish silently, so it becomes one too.
        _ => {
            let n = recipe.masks.len();
            recipe.masks.push(LocalAdjustment { mask: geom, name: new_mask_name, ..Default::default() });
            effects.push(Effect::Status(UiText::of(
                K_LOGIC_MASK_PLACED_PULL_ITS_SLIDERS_IN_LOCAL_MASKS_AT_L,
            )));
            Committed { selection: Selection::Mask(n), effects }
        }
    }
}
