//! The AI region selection (box-select on the canvas), headless: what one
//! dragged rectangle means.
//!
//! The canvas maps ALL FOUR corners of the view rectangle into the ORIGINAL
//! frame (under rotation/distortion the two diagonals describe a different box
//! than the rectangle the user drew) and hands them here. This module takes
//! their bounding box, refuses a box too small to mean anything (a tiny drag
//! clears the selection, as it always has) and words the result.
use autoshade::text::generated::*;


use super::{Effect, UiText};

/// A region gesture, in the original frame (already mapped by the canvas).
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum RegionAction {
    /// The four corners of the dragged rectangle.
    Select([(f32, f32); 4]),
}

/// The smallest side a region must have to count; a smaller drag clears.
const MIN_SIDE: f32 = 0.02;

/// Apply one region gesture. `SetRegion(Some(box))` + the size line, or
/// `SetRegion(None)` when the box was too small to mean anything.
pub(crate) fn apply(action: RegionAction) -> Vec<Effect> {
    match action {
        RegionAction::Select(corners) => {
            let (mut l, mut t, mut r, mut b) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
            for (x, y) in corners {
                l = l.min(x);
                t = t.min(y);
                r = r.max(x);
                b = b.max(y);
            }
            if r - l > MIN_SIDE && b - t > MIN_SIDE {
                let w = ((r - l) * 100.0).round() as i32;
                let h = ((b - t) * 100.0).round() as i32;
                vec![
                    Effect::SetRegion(Some([l, t, r, b])),
                    Effect::Status(UiText::with(
                        K_LOGIC_REGION_TYPE_A_DIRECTION_THEN_AI_ANALYZE_CLICK_TO,
                        &[("w", &w.to_string()), ("h", &h.to_string())],
                    )),
                ]
            } else {
                // A tiny drag clears the selection.
                vec![Effect::SetRegion(None)]
            }
        }
    }
}
