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
