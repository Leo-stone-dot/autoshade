//! On-image geometry editing of the selected mask (the LR gesture), headless:
//! what dragging a knob does to the mask's OWN GEOMETRY.
//!
//! The canvas owns the pointer (hit-testing the knobs, the drag anchor, the
//! view→original transform). This module applies a knob drag to the selected
//! mask's geometry in the ORIGINAL frame: linear/shaped end points move, radial
//! edges move by the pointer's delta projected onto the mask's own axes, the
//! rotation grip sets the angle from the pointer's bearing, and the centre
//! moves the whole radial. Geometry may start off-canvas (a generous band keeps
//! knobs recoverable), and a radial may never invert past its minimum size.

use super::{Effect};
use autoshade::recipe::{EditRecipe, MaskGeometry};

/// One knob drag on the selected mask. `mask`/`component` are the panel's
/// selection; `last`/`cur` are in the ORIGINAL frame (the canvas mapped them).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum MaskEditAction {
    Drag { mask: usize, component: Option<usize>, handle: u8, last: (f32, f32), cur: (f32, f32) },
}

/// Apply one drag. Masks are develop stages, so every drag frame asks for a
/// live preview — even a drag the band clamps to a no-op (the frame must not
/// go stale against an earlier move).
pub(crate) fn apply(recipe: &mut EditRecipe, action: MaskEditAction) -> Vec<Effect> {
    match action {
        MaskEditAction::Drag { mask, component, handle, last, cur } => {
            if mask >= recipe.masks.len() {
                return vec![Effect::MarkDirty];
            }
            let m = &mut recipe.masks[mask];
            let geom = match component {
                Some(c) if c < m.components.len() => &mut m.components[c].geometry,
                _ => &mut m.mask,
            };
            let (dx, dy) = (cur.0 - last.0, cur.1 - last.1);
            // LR allows geometry to start off-canvas; a generous band keeps
            // knobs recoverable instead of letting them fly to infinity.
            let cl = |v: f32| v.clamp(-0.5, 1.5);
            match geom {
                MaskGeometry::Linear { zero_x, zero_y, full_x, full_y }
                | MaskGeometry::Shaped { zero_x, zero_y, full_x, full_y, .. } => match handle {
                    1 => (*zero_x, *zero_y) = (cl(cur.0), cl(cur.1)),
                    2 => (*full_x, *full_y) = (cl(cur.0), cl(cur.1)),
                    _ => {
                        *zero_x = cl(*zero_x + dx);
                        *zero_y = cl(*zero_y + dy);
                        *full_x = cl(*full_x + dx);
                        *full_y = cl(*full_y + dy);
                    }
                },
                MaskGeometry::Radial { top, left, bottom, right, angle, .. } => {
                    const MIN_SIZE: f32 = 0.01;
                    // Edges move by the pointer's original-space DELTA, not to
                    // its absolute position: the displayed handles sit on the
                    // transformed BOUNDING BOX (≠ the true transformed ellipse
                    // under straighten/distortion), so an absolute assign made
                    // the edge jump to close that gap on the first drag frame.
                    // Under neutral geometry the two are identical.
                    //
                    // With a rotated mask the four knobs sit on the ROTATED
                    // axis endpoints, so the delta is projected onto the
                    // mask's own axes first — at angle 0 (du, dv) ≡ (dx, dy).
                    let (s_a, c_a) = angle.to_radians().sin_cos();
                    let du = dx * c_a + dy * s_a;
                    let dv = -dx * s_a + dy * c_a;
                    match handle {
                        1 => *left = cl(*left + du).min(*right - MIN_SIZE),
                        2 => *top = cl(*top + dv).min(*bottom - MIN_SIZE),
                        3 => *right = cl(*right + du).max(*left + MIN_SIZE),
                        4 => *bottom = cl(*bottom + dv).max(*top + MIN_SIZE),
                        5 => {
                            // Rotation grip: it is parked toward the TOP axis
                            // endpoint, so the ellipse angle is the pointer's
                            // bearing from the centre + 90° (same rotation
                            // convention as the engine — recipe.rs Radial).
                            let (cx, cy) = ((*left + *right) / 2.0, (*top + *bottom) / 2.0);
                            let th = (cur.1 - cy).atan2(cur.0 - cx).to_degrees() + 90.0;
                            *angle = th.rem_euclid(360.0);
                            if *angle > 180.0 {
                                *angle -= 360.0;
                            }
                        }
                        _ => {
                            // Clamp the SHIFT, not each edge — independent
                            // clamps at the band boundary squashed the ellipse
                            // toward zero size instead of parking it.
                            let (lx, hx) = (-0.5 - *left, 1.5 - *right);
                            let (ly, hy) = (-0.5 - *top, 1.5 - *bottom);
                            let dx = dx.clamp(lx.min(hx), lx.max(hx));
                            let dy = dy.clamp(ly.min(hy), ly.max(hy));
                            *left += dx;
                            *right += dx;
                            *top += dy;
                            *bottom += dy;
                        }
                    }
                }
                _ => {}
            }
            vec![Effect::MarkDirty]
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoshade::recipe::LocalAdjustment;

    fn radial(left: f32, top: f32, right: f32, bottom: f32, angle: f32) -> MaskGeometry {
        MaskGeometry::Radial {
            top,
            left,
            bottom,
            right,
            feather: 0.5,
            roundness: 0.0,
            flipped: false,
            angle,
            midpoint: 50.0,
            mask_version: 2,
        }
    }

    fn linear() -> MaskGeometry {
        MaskGeometry::Linear { zero_x: 0.2, zero_y: 0.3, full_x: 0.8, full_y: 0.9 }
    }

    fn with_mask(geom: MaskGeometry) -> EditRecipe {
        EditRecipe { masks: vec![LocalAdjustment { mask: geom, ..Default::default() }], ..Default::default() }
    }

    fn radial_of(recipe: &EditRecipe) -> (f32, f32, f32, f32, f32) {
        match &recipe.masks[0].mask {
            MaskGeometry::Radial { top, left, bottom, right, angle, .. } => {
                (*left, *top, *right, *bottom, *angle)
            }
            other => panic!("expected a radial, got {other:?}"),
        }
    }

    /// A linear drag moves both points by the delta; the two end knobs follow
    /// the pointer absolutely (clamped to the recoverable band).
    #[test]
    fn linear_knobs_move_or_follow_the_pointer() {
        let mut recipe = with_mask(linear());
        apply(
            &mut recipe,
            MaskEditAction::Drag { mask: 0, component: None, handle: 0, last: (0.5, 0.5), cur: (0.6, 0.4) },
        );
        match &recipe.masks[0].mask {
            MaskGeometry::Linear { zero_x, zero_y, full_x, full_y } => {
                assert!((zero_x - 0.3).abs() < 1e-5 && (zero_y - 0.2).abs() < 1e-5);
                assert!((full_x - 0.9).abs() < 1e-5 && (full_y - 0.8).abs() < 1e-5);
            }
            other => panic!("{other:?}"),
        }
        // The full point knob follows the pointer, clamped to the band.
        apply(
            &mut recipe,
            MaskEditAction::Drag { mask: 0, component: None, handle: 2, last: (0.0, 0.0), cur: (3.0, -2.0) },
        );
        match &recipe.masks[0].mask {
            MaskGeometry::Linear { full_x, full_y, .. } => {
                assert_eq!((*full_x, *full_y), (1.5, -0.5), "clamped to the recoverable band");
            }
            other => panic!("{other:?}"),
        }
    }

    /// A radial edge drag applies the delta (projected onto the mask's own
    /// rotated axes) and never inverts past the minimum size; the centre moves
    /// the whole ellipse, clamping the SHIFT so both edges stay in the band.
    #[test]
    fn radial_edges_and_move_respect_the_minimum_and_the_band() {
        let mut recipe = with_mask(radial(0.2, 0.2, 0.6, 0.6, 0.0));
        apply(
            &mut recipe,
            MaskEditAction::Drag { mask: 0, component: None, handle: 3, last: (0.0, 0.0), cur: (0.1, 0.0) },
        );
        assert!((radial_of(&recipe).2 - 0.7).abs() < 1e-5, "the right edge moved by dx");

        // Drag the right edge far past the left: it stops at left + MIN_SIZE.
        apply(
            &mut recipe,
            MaskEditAction::Drag { mask: 0, component: None, handle: 3, last: (0.0, 0.0), cur: (-9.0, 0.0) },
        );
        let (left, _, right, _, _) = radial_of(&recipe);
        assert!((right - (left + 0.01)).abs() < 1e-5, "never inverts: {right} vs {left}");

        // Angle 90°: the horizontal pointer delta becomes the vertical axis.
        let mut rotated = with_mask(radial(0.2, 0.2, 0.6, 0.6, 90.0));
        apply(
            &mut rotated,
            MaskEditAction::Drag { mask: 0, component: None, handle: 2, last: (0.0, 0.0), cur: (0.1, 0.0) },
        );
        let (_, top, _, _, _) = radial_of(&rotated);
        // At 90° the top knob's own axis is the screen's horizontal one: a
        // +0.1 horizontal pointer delta moves the top edge by −0.1.
        assert!((top - 0.1).abs() < 1e-5, "the delta is projected onto the rotated axis: {top}");

        // The centre knob parks the ellipse at the band instead of squashing it.
        let mut moved = with_mask(radial(0.2, 0.2, 0.6, 0.6, 0.0));
        apply(
            &mut moved,
            MaskEditAction::Drag { mask: 0, component: None, handle: 0, last: (0.0, 0.0), cur: (9.0, 9.0) },
        );
        let (left, top, right, bottom, _) = radial_of(&moved);
        assert!((right - left - 0.4).abs() < 1e-5 && (bottom - top - 0.4).abs() < 1e-5, "size kept");
        assert!(left <= 1.5 && top <= 1.5 && right >= -0.5 && bottom >= -0.5, "band respected");
    }

    /// The rotation grip reads the pointer's bearing from the centre (+90°,
    /// the engine's convention) and normalizes to (-180°, 180°].
    #[test]
    fn the_rotation_grip_sets_the_bearing() {
        let mut recipe = with_mask(radial(0.4, 0.4, 0.6, 0.6, 0.0));
        // Pointer straight above the centre → bearing −90° → angle 0°.
        apply(
            &mut recipe,
            MaskEditAction::Drag { mask: 0, component: None, handle: 5, last: (0.0, 0.0), cur: (0.5, 0.2) },
        );
        assert!(radial_of(&recipe).4.abs() < 1e-4, "parked toward the top axis");
        // Pointer straight right → bearing 0° → angle 90°.
        apply(
            &mut recipe,
            MaskEditAction::Drag { mask: 0, component: None, handle: 5, last: (0.0, 0.0), cur: (0.9, 0.5) },
        );
        assert!((radial_of(&recipe).4 - 90.0).abs() < 1e-4);
    }

    /// A component drag edits the component's geometry, not the mask's base;
    /// a stale selection (mask or component gone) is a no-op that still asks
    /// for the live frame.
    #[test]
    fn a_component_drag_edits_the_component_and_a_stale_selection_is_safe() {
        let mut recipe = with_mask(radial(0.2, 0.2, 0.6, 0.6, 0.0));
        recipe.masks[0].components.push(autoshade::recipe::MaskComponent {
            inverted: false,
            geometry: linear(),
            mode: autoshade::recipe::MaskCombine::Add,
        });
        apply(
            &mut recipe,
            MaskEditAction::Drag { mask: 0, component: Some(0), handle: 0, last: (0.5, 0.5), cur: (0.6, 0.6) },
        );
        match &recipe.masks[0].components[0].geometry {
            MaskGeometry::Linear { zero_x, zero_y, .. } => {
                assert!((zero_x - 0.3).abs() < 1e-5 && (zero_y - 0.4).abs() < 1e-5);
            }
            other => panic!("{other:?}"),
        }
        assert!(matches!(recipe.masks[0].mask, MaskGeometry::Radial { .. }), "the base mask is untouched");

        let stale = apply(
            &mut recipe,
            MaskEditAction::Drag { mask: 9, component: None, handle: 0, last: (0.0, 0.0), cur: (0.1, 0.1) },
        );
        assert!(matches!(stale.as_slice(), [Effect::MarkDirty]));
    }
}
