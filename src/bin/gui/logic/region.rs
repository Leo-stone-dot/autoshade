//! The AI region selection (box-select on the canvas), headless: what one
//! dragged rectangle means.
//!
//! The canvas maps ALL FOUR corners of the view rectangle into the ORIGINAL
//! frame (under rotation/distortion the two diagonals describe a different box
//! than the rectangle the user drew) and hands them here. This module takes
//! their bounding box, refuses a box too small to mean anything (a tiny drag
//! clears the selection, as it always has) and returns the canvas-owned result.

use super::UiText;

/// The result of one region gesture: the box the canvas owns, plus the optional
/// status line the host chrome owns.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RegionOutcome {
    pub(crate) region: Option<[f32; 4]>,
    pub(crate) status: Option<UiText>,
}

/// The smallest side a region must have to count; a smaller drag clears.
const MIN_SIDE: f32 = 0.02;

/// Select the bounding box of four original-frame corners.
pub(crate) fn select(corners: [(f32, f32); 4]) -> RegionOutcome {
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
        RegionOutcome {
            region: Some([l, t, r, b]),
            status: Some(UiText::with(
                "region {w}×{h}% — type a direction, then AI Analyze (click to clear)",
                &[("w", &w.to_string()), ("h", &h.to_string())],
            )),
        }
    } else {
        // A tiny drag clears the selection.
        RegionOutcome { region: None, status: None }
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
        let outcome = select([(0.1, 0.2), (0.6, 0.5), (0.2, 0.4), (0.5, 0.3)]);
        let [l, t, r, b] = outcome.region.expect("the selected region");
        assert!((l - 0.1).abs() < 1e-6 && (t - 0.2).abs() < 1e-6);
        assert!((r - 0.6).abs() < 1e-6 && (b - 0.5).abs() < 1e-6);
        let text = outcome.status.expect("the size line");
        assert_eq!(text.key, "region {w}×{h}% — type a direction, then AI Analyze (click to clear)");
        assert_eq!(text.args[0].1, super::super::Arg::Text("50".into()), "50% wide");
        assert_eq!(text.args[1].1, super::super::Arg::Text("30".into()), "30% tall");
    }

    /// A drag under the 2% minimum on either side clears the region instead of
    /// storing an unusable sliver.
    #[test]
    fn a_tiny_drag_clears_the_region() {
        for corners in [
            [(0.1, 0.1), (0.11, 0.5), (0.1, 0.5), (0.11, 0.1)],
            [(0.1, 0.1), (0.5, 0.11), (0.1, 0.11), (0.5, 0.1)],
        ] {
            let outcome = select(corners);
            assert_eq!(outcome, RegionOutcome { region: None, status: None });
        }
    }
}
