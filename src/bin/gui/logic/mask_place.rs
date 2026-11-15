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

#[cfg(test)]
mod tests {
    use super::*;

    fn linear() -> MaskGeometry {
        MaskGeometry::Linear { zero_x: 0.2, zero_y: 0.3, full_x: 0.8, full_y: 0.9 }
    }

    fn radial() -> MaskGeometry {
        MaskGeometry::Radial {
            top: 0.1,
            left: 0.1,
            bottom: 0.4,
            right: 0.5,
            feather: 0.8,
            roundness: 0.3,
            flipped: true,
            angle: 12.0,
            midpoint: 40.0,
            mask_version: 3,
        }
    }

    fn kinds(effects: &[Effect]) -> Vec<&'static str> {
        effects
            .iter()
            .map(|e| match e {
                Effect::MarkDirty => "dirty",
                Effect::Status(_) => "status",
                other => panic!("unexpected effect {other:?}"),
            })
            .collect()
    }

    /// A fresh drag pushes a mask, names it, selects it and asks for a
    /// redevelop with the new-mask line.
    #[test]
    fn a_new_drag_places_a_named_mask_and_selects_it() {
        let mut recipe = EditRecipe::default();
        let committed = commit(&mut recipe, Place::NewMask, linear(), "Manual 1".into());
        assert_eq!(recipe.masks.len(), 1);
        assert_eq!(recipe.masks[0].name, "Manual 1");
        assert_eq!(committed.selection, Selection::Mask(0));
        assert_eq!(kinds(&committed.effects), vec!["dirty", "status"]);
        match committed.effects[1] {
            Effect::Status(ref t) => assert!(t.key.starts_with("mask placed")),
            _ => unreachable!(),
        }
    }

    /// Redraw replaces the AREA and keeps the radial's tuned slider/handle
    /// state (and a linear's points); the selection stays put.
    #[test]
    fn a_redraw_keeps_the_tuned_radial_state() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(LocalAdjustment { mask: radial(), ..Default::default() });
        let committed = commit(&mut recipe, Place::Redraw(0), linear(), String::new());
        assert_eq!(committed.selection, Selection::Keep);
        assert!(matches!(recipe.masks[0].mask, MaskGeometry::Linear { .. }), "the area is the new drag");
        assert_eq!(kinds(&committed.effects), vec!["dirty", "status"]);

        let tuned = radial();
        recipe.masks[0].mask = tuned.clone();
        recipe.masks[0].components.push(MaskComponent {
            inverted: false,
            geometry: radial(),
            mode: MaskCombine::Add,
        });
        let committed = commit(&mut recipe, Place::Redraw(0), radial(), String::new());
        assert_eq!(committed.selection, Selection::Keep);
        match &recipe.masks[0].mask {
            MaskGeometry::Radial { feather, roundness, flipped, angle, midpoint, mask_version, .. } => {
                assert_eq!(*feather, 0.8, "feather survives a redraw");
                assert_eq!(*roundness, 0.3, "roundness survives a redraw");
                assert!(*flipped);
                assert_eq!(*angle, 12.0);
                assert_eq!(*midpoint, 40.0);
                assert_eq!(*mask_version, 3);
            }
            other => panic!("expected the kept radial, got {other:?}"),
        }
    }

    /// A component joins the mask it was drawn for and the selection moves to
    /// the new component; a stale owner index degrades to a fresh mask.
    #[test]
    fn a_component_joins_its_mask_or_degrades() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(LocalAdjustment::default());
        let committed = commit(&mut recipe, Place::Component(0, MaskCombine::Add), linear(), String::new());
        assert_eq!(recipe.masks[0].components.len(), 1);
        assert_eq!(committed.selection, Selection::Component(0, 0));
        assert_eq!(recipe.masks[0].components[0].mode, MaskCombine::Add);
        assert_eq!(kinds(&committed.effects), vec!["dirty", "status"]);

        let stale = commit(&mut recipe, Place::Component(9, MaskCombine::Add), linear(), "Manual 2".into());
        assert_eq!(recipe.masks.len(), 2, "the orphan component becomes its own mask");
        assert_eq!(stale.selection, Selection::Mask(1));
        assert_eq!(recipe.masks[1].name, "Manual 2");
    }

    /// A redraw whose mask was deleted mid-drag becomes a fresh mask rather
    /// than silently doing nothing.
    #[test]
    fn a_redraw_with_no_owner_places_a_fresh_mask() {
        let mut recipe = EditRecipe::default();
        let committed = commit(&mut recipe, Place::Redraw(4), linear(), "Manual 1".into());
        assert_eq!(recipe.masks.len(), 1);
        assert_eq!(committed.selection, Selection::Mask(0));
        assert!(matches!(recipe.masks[0].mask, MaskGeometry::Linear { .. }));
    }
}
