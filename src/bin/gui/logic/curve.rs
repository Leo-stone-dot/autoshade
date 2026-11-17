//! Tone-curve editing, headless: insert a control point and drag one, keeping
//! the inputs sorted and unique (the engine's piecewise-linear interpolation
//! needs distinct inputs).

use autoshade::recipe::CurvePoint;

/// Insert a point keeping inputs sorted and unique — a second point at the same
/// input overwrites instead of duplicating. Returns the point's index.
pub(crate) fn insert_point(pts: &mut Vec<CurvePoint>, input: u8, output: u8) -> usize {
    match pts.binary_search_by_key(&input, |p| p.input) {
        Ok(i) => {
            pts[i].output = output;
            i
        }
        Err(i) => {
            pts.insert(i, CurvePoint { input, output });
            i
        }
    }
}

/// Move point `i` to (input, output), clamping the input STRICTLY between its
/// neighbours so the points stay sorted with unique inputs.
pub(crate) fn drag_point(pts: &mut [CurvePoint], i: usize, input: u8, output: u8) {
    let lo = if i > 0 { pts[i - 1].input.saturating_add(1) } else { 0 };
    let hi = if i + 1 < pts.len() { pts[i + 1].input.saturating_sub(1) } else { 255 };
    if lo <= hi {
        pts[i].input = input.clamp(lo, hi);
    }
    pts[i].output = output;
}
