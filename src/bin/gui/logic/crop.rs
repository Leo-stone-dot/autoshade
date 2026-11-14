//! The crop box's geometry, headless: what a preset pick and a handle drag
//! mean.
//!
//! The canvas owns the pointer (hit-testing handles, the drag anchor, the
//! overlay). These are the pure mappings from a gesture to the box in the
//! post-rotation view frame: the preset's largest same-centre fit, the eight
//! resize/move handles with the pixel-aspect constraint, and the
//! rotate-straighten angle. No frame, no recipe: inputs in, box out.

use super::Effect;
use autoshade::recipe::{Crop, EditRecipe};

/// The smallest side a dragged box may take. The drag path refuses less; the
/// preset path grows to it, so a preset can never create a box the handles
/// could then never reproduce.
const MIN_SIDE: f32 = 0.05;

/// What the crop view asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum CropAction {
    /// A preset was picked: re-derive the box now (largest same-centre fit at
    /// the pixel ratio; "free" never reaches here — it keeps the box).
    Preset { current: Option<[f32; 4]>, aspect_px: Option<f32>, tex: (f32, f32) },
    /// A handle drag: `start`/`delta` are normalized VIEW coordinates;
    /// `aspect_px` is the preset's pixel w/h (None = free).
    Drag {
        handle: u8,
        start: [f32; 4],
        delta: (f32, f32),
        aspect_px: Option<f32>,
        tex: (f32, f32),
    },
    /// Outside-the-box drag: `raw_delta_deg` is the un-wrapped atan2 delta.
    Rotate { base_deg: f32, raw_delta_deg: f32 },
}

/// Apply one crop gesture. A gesture that would produce a degenerate box
/// changes nothing (the handles refuse it mid-drag, as they always have).
pub(crate) fn apply(recipe: &mut EditRecipe, action: CropAction) -> Vec<Effect> {
    match action {
        CropAction::Preset { current, aspect_px, tex } => {
            let Some(c) = preset_box(current, aspect_px, tex) else { return Vec::new() };
            let next = Some(Crop { left: c[0], top: c[1], right: c[2], bottom: c[3] });
            if recipe.crop == next {
                return Vec::new();
            }
            recipe.crop = next;
            vec![Effect::MarkDirty] // histogram/clipping follow the crop
        }
        CropAction::Drag { handle, start, delta, aspect_px, tex } => {
            let ratio_n = aspect_px.map(|r| r * tex.1.max(1.0) / tex.0.max(1.0));
            let Some(c) = drag_box(handle, start, delta, ratio_n) else { return Vec::new() };
            let next = Some(Crop { left: c[0], top: c[1], right: c[2], bottom: c[3] });
            if recipe.crop == next {
                return Vec::new();
            }
            recipe.crop = next;
            vec![Effect::MarkDirty]
        }
        CropAction::Rotate { base_deg, raw_delta_deg } => {
            let next = rotate_angle(base_deg, raw_delta_deg);
            if recipe.straighten_deg == next {
                return Vec::new();
            }
            recipe.straighten_deg = next;
            vec![Effect::MarkDirty]
        }
    }
}

/// Largest same-centre box at the requested pixel ratio, grown to the minimum
/// side and then re-clamped inside the frame (growing can shift it off-centre
/// at an edge).
fn preset_box(current: Option<[f32; 4]>, aspect_px: Option<f32>, tex: (f32, f32)) -> Option<[f32; 4]> {
    let r_px = aspect_px?;
    let rn = r_px * tex.1.max(1.0) / tex.0.max(1.0);
    let c0 = current.unwrap_or([0.0, 0.0, 1.0, 1.0]);
    let (cx, cy) = ((c0[0] + c0[2]) / 2.0, (c0[1] + c0[3]) / 2.0);
    let (mut w, mut h) = (c0[2] - c0[0], c0[3] - c0[1]);
    if w / h.max(1e-6) > rn {
        w = h * rn;
    } else {
        h = w / rn.max(1e-6);
    }
    let s = ((2.0 * cx.min(1.0 - cx)) / w.max(1e-6))
        .min((2.0 * cy.min(1.0 - cy)) / h.max(1e-6))
        .min(1.0);
    let (w, h) = (w * s, h * s);
    let grow = (MIN_SIDE / w.max(1e-6)).max(MIN_SIDE / h.max(1e-6)).max(1.0);
    let (w, h) = ((w * grow).min(1.0), (h * grow).min(1.0));
    let left = (cx - w / 2.0).clamp(0.0, 1.0 - w);
    let top = (cy - h / 2.0).clamp(0.0, 1.0 - h);
    Some([left, top, left + w, top + h])
}

/// Map one handle drag to the box. `handle`: 0=TL 1=TR 2=BL 3=BR, 4=move,
/// 5=top 6=bottom 7=left 8=right (the view's handle ids). `ratio_n` is the
/// fixed aspect in normalized view units.
fn drag_box(handle: u8, orig: [f32; 4], delta: (f32, f32), ratio_n: Option<f32>) -> Option<[f32; 4]> {
    let (dx, dy) = delta;
    let c: [f32; 4] = if handle == 4 {
        // Move: shift, clamped so the rect stays inside the frame.
        let (w, hg) = (orig[2] - orig[0], orig[3] - orig[1]);
        let nl = (orig[0] + dx).clamp(0.0, 1.0 - w);
        let nt = (orig[1] + dy).clamp(0.0, 1.0 - hg);
        [nl, nt, nl + w, nt + hg]
    } else if handle == 5 || handle == 6 {
        // Top/bottom edge: anchored at the opposite edge. Free aspect moves
        // only this edge; a fixed aspect rederives the width centred on the
        // box's vertical axis (the LR edge behaviour), shrinking to stay in
        // frame.
        let ay = if handle == 5 { orig[3] } else { orig[1] };
        let y = (if handle == 5 { orig[1] } else { orig[3] } + dy).clamp(0.0, 1.0);
        let mut h_n = (y - ay).abs();
        let (mut l, mut r) = (orig[0], orig[2]);
        if let Some(rn) = ratio_n {
            let cx = (orig[0] + orig[2]) / 2.0;
            let mut w_n = h_n * rn;
            let room = 2.0 * cx.min(1.0 - cx);
            if w_n > room {
                w_n = room;
                h_n = w_n / rn;
            }
            l = cx - w_n / 2.0;
            r = cx + w_n / 2.0;
        }
        let y = if handle == 5 { ay - h_n } else { ay + h_n };
        [l, y.min(ay), r, y.max(ay)]
    } else if handle == 7 || handle == 8 {
        // Left/right edge — the transpose of the above.
        let ax = if handle == 7 { orig[2] } else { orig[0] };
        let x = (if handle == 7 { orig[0] } else { orig[2] } + dx).clamp(0.0, 1.0);
        let mut w_n = (x - ax).abs();
        let (mut t, mut b) = (orig[1], orig[3]);
        if let Some(rn) = ratio_n {
            let cy = (orig[1] + orig[3]) / 2.0;
            let mut h_n = w_n / rn;
            let room = 2.0 * cy.min(1.0 - cy);
            if h_n > room {
                h_n = room;
                w_n = h_n * rn;
            }
            t = cy - h_n / 2.0;
            b = cy + h_n / 2.0;
        }
        let x = if handle == 7 { ax - w_n } else { ax + w_n };
        [x.min(ax), t, x.max(ax), b]
    } else {
        // Corner: drag it, anchored at the opposite corner.
        let (ax, ay) = match handle {
            0 => (orig[2], orig[3]),
            1 => (orig[0], orig[3]),
            2 => (orig[2], orig[1]),
            _ => (orig[0], orig[1]),
        };
        let mut x = (match handle {
            0 | 2 => orig[0] + dx,
            _ => orig[2] + dx,
        })
        .clamp(0.0, 1.0);
        let mut y = (match handle {
            0 | 1 => orig[1] + dy,
            _ => orig[3] + dy,
        })
        .clamp(0.0, 1.0);
        if let Some(rn) = ratio_n {
            // The DOMINANT drag axis drives and the other follows the ratio.
            // Dominance comes from the DRAG DELTAS (in width units), not
            // max(extents): max() can only grow past the stale axis, so an
            // inward pull dominated by one axis left the corner pinned at the
            // old size. The Y side follows the POINTER relative to the anchor,
            // exactly like x (pinning it to the handle's original identity
            // meant a TL corner dragged past BR rebuilt the box on the
            // anchor's far side, away from the pointer).
            let above = y < ay;
            let mut w_n = if dx.abs() >= dy.abs() * rn {
                (x - ax).abs()
            } else {
                (y - ay).abs() * rn
            };
            let mut h_n = w_n / rn;
            let room = if above { ay } else { 1.0 - ay };
            if h_n > room {
                h_n = room;
                w_n = h_n * rn;
            }
            // Horizontal room too: a dominantly VERTICAL pull can derive a
            // width wider than the frame leaves on the driven side — x would
            // land outside 0..1 and the final min/max cannot repair that.
            let x_room = if x >= ax { 1.0 - ax } else { ax };
            if w_n > x_room {
                w_n = x_room;
                h_n = w_n / rn;
            }
            x = if x >= ax { ax + w_n } else { ax - w_n };
            y = if above { ay - h_n } else { ay + h_n };
        }
        [x.min(ax), y.min(ay), x.max(ax), y.max(ay)]
    };
    ((c[2] - c[0]) >= MIN_SIDE && (c[3] - c[1]) >= MIN_SIDE).then_some(c)
}

/// The rotate-straighten angle: wrap the raw atan2 delta into (-180°, 180°]
/// (crossing the branch cut otherwise reads a small clockwise move near the
/// leftward ray as −358° and slams the slider to the clamp), then clamp to the
/// engine's ±45°.
fn rotate_angle(base_deg: f32, raw_delta_deg: f32) -> f32 {
    let delta = (raw_delta_deg + 540.0) % 360.0 - 180.0;
    (base_deg + delta).clamp(-45.0, 45.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn box_of(c: &[f32; 4]) -> Crop {
        Crop { left: c[0], top: c[1], right: c[2], bottom: c[3] }
    }

    fn ratio_of(c: &Crop) -> f32 {
        (c.right - c.left) / (c.bottom - c.top)
    }

    /// The preset re-derives the largest same-centre box at the pixel ratio,
    /// inside the frame, at least the minimum side.
    #[test]
    fn a_preset_fits_the_largest_centre_box_at_its_ratio() {
        let tex = (3000.0, 2000.0); // 3:2
        let square = preset_box(None, Some(1.0), tex).expect("a pixel ratio was asked");
        let rn = 1.0 * tex.1 / tex.0; // normalized ratio of a 1:1 pixel box
        assert!((ratio_of(&box_of(&square)) - rn).abs() < 1e-5);
        let cx = (square[0] + square[2]) / 2.0;
        let cy = (square[1] + square[3]) / 2.0;
        assert!((cx - 0.5).abs() < 1e-5 && (cy - 0.5).abs() < 1e-5, "same centre");
        assert!(square[0] >= 0.0 && square[3] <= 1.0);

        assert_eq!(preset_box(None, None, tex), None, "free keeps the box: no re-derive");

        let tiny = Some([0.4, 0.4, 0.42, 0.42]);
        let grown = preset_box(tiny, Some(1.0), tex).expect("grown, not refused");
        assert!(grown[2] - grown[0] >= MIN_SIDE && grown[3] - grown[1] >= MIN_SIDE);
    }

    /// A move keeps the box size and stays inside the frame.
    #[test]
    fn a_move_shifts_and_clamps_inside_the_frame() {
        let start = [0.2, 0.2, 0.6, 0.6];
        let moved = drag_box(4, start, (0.1, 0.1), None).expect("moved");
        for (got, want) in moved.iter().zip([0.3, 0.3, 0.7, 0.7]) {
            assert!((got - want).abs() < 1e-5, "{moved:?}");
        }
        let clamped = drag_box(4, start, (-5.0, 5.0), None).expect("clamped");
        for (got, want) in clamped.iter().zip([0.0, 0.6, 0.4, 1.0]) {
            assert!((got - want).abs() < 1e-5, "{clamped:?}");
        }
    }

    /// A right-edge drag without a ratio moves only that edge; with a ratio it
    /// keeps the ratio and shrinks to fit the frame; a box under the minimum
    /// side is refused (the handle does nothing).
    #[test]
    fn an_edge_drag_keeps_the_ratio_when_one_is_set() {
        let start = [0.2, 0.2, 0.6, 0.6];
        let edge = drag_box(8, start, (0.1, 0.0), None).expect("edge moved");
        for (got, want) in edge.iter().zip([0.2, 0.2, 0.7, 0.6]) {
            assert!((got - want).abs() < 1e-5, "{edge:?}");
        }

        let tex = (3000.0, 2000.0);
        let rn = 1.0 * tex.1 / tex.0;
        let c = drag_box(8, start, (0.1, 0.0), Some(rn)).expect("ratio box");
        assert!(((c[2] - c[0]) / (c[3] - c[1]) - rn).abs() < 1e-5, "the ratio holds");
        assert!((c[1] + c[3]) / 2.0 - 0.4 < 1e-5, "centred on the box's own axis");

        // Right edge pulled within 0.05 of the anchored left edge: refused.
        let narrow = [0.2, 0.2, 0.24, 0.6];
        assert_eq!(drag_box(8, narrow, (-0.08, 0.0), None), None, "under the minimum side");
    }

    /// A corner drag with a ratio takes its dominant axis from the drag deltas
    /// and stays inside on both driven sides.
    #[test]
    fn a_corner_drag_follows_its_dominant_axis() {
        let start = [0.2, 0.2, 0.6, 0.6];
        let tex = (3000.0, 2000.0);
        let rn = 1.0 * tex.1 / tex.0;
        let c = drag_box(3, start, (0.2, 0.01), Some(rn)).expect("a width-driven box");
        assert!(((c[2] - c[0]) / (c[3] - c[1]) - rn).abs() < 1e-5);
        assert!(c[3] <= 1.0, "the bottom stays inside");

        let wide = drag_box(3, start, (0.9, 0.9), Some(rn)).expect("clamped, not refused");
        assert!(wide[2] <= 1.0 && wide[3] <= 1.0);
    }

    /// The rotate gesture wraps at atan2's branch cut and clamps to ±45°.
    #[test]
    fn a_rotate_wraps_and_clamps() {
        assert!((rotate_angle(0.0, 10.0) - 10.0).abs() < 1e-4);
        assert!((rotate_angle(0.0, 350.0) - (-10.0)).abs() < 1e-4, "the branch cut wraps");
        assert_eq!(rotate_angle(40.0, 20.0), 45.0);
        assert_eq!(rotate_angle(-40.0, -20.0), -45.0);
    }
}
