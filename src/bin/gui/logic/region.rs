//! The AI region selection (box-select on the canvas), headless: what one
//! dragged rectangle means.
//!
//! The canvas maps ALL FOUR corners of the view rectangle into the ORIGINAL
//! frame (under rotation/distortion the two diagonals describe a different box
//! than the rectangle the user drew) and hands them here. This module takes
//! their bounding box, refuses a box too small to mean anything (a tiny drag
//! clears the selection, as it always has) and words the result.

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
                        "region {w}×{h}% — type a direction, then AI Analyze (click to clear)",
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

#[cfg(test)]
mod tests {
    use super::*;

    /// The box is the bounding box of ALL FOUR mapped corners (not just the
    /// diagonals), and the line names its rounded size.
    #[test]
    fn a_drag_becomes_the_four_corner_bounding_box() {
        // Two diagonal corners that would describe a different box under the
        // view transform; the bounding box must cover all four.
        let effects = apply(RegionAction::Select([(0.1, 0.2), (0.6, 0.5), (0.2, 0.4), (0.5, 0.3)]));
        match &effects[0] {
            Effect::SetRegion(Some([l, t, r, b])) => {
                assert!((*l - 0.1).abs() < 1e-6 && (*t - 0.2).abs() < 1e-6);
                assert!((*r - 0.6).abs() < 1e-6 && (*b - 0.5).abs() < 1e-6);
            }
            other => panic!("expected the region, got {other:?}"),
        }
        match &effects[1] {
            Effect::Status(text) => {
                assert_eq!(text.key, "region {w}×{h}% — type a direction, then AI Analyze (click to clear)");
                assert_eq!(text.args[0].1, super::super::Arg::Text("50".into()), "50% wide");
                assert_eq!(text.args[1].1, super::super::Arg::Text("30".into()), "30% tall");
            }
            other => panic!("expected the size line, got {other:?}"),
        }
    }

    /// A drag under the 2% minimum on either side clears the region instead of
    /// storing an unusable sliver.
    #[test]
    fn a_tiny_drag_clears_the_region() {
        for corners in [
            [(0.1, 0.1), (0.11, 0.5), (0.1, 0.5), (0.11, 0.1)],
            [(0.1, 0.1), (0.5, 0.11), (0.1, 0.11), (0.5, 0.1)],
        ] {
            let effects = apply(RegionAction::Select(corners));
            assert!(matches!(effects.as_slice(), [Effect::SetRegion(None)]), "{effects:?}");
        }
    }
}
