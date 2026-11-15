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

#[cfg(test)]
mod tests {
    use super::*;

    fn pts() -> Vec<CurvePoint> {
        vec![CurvePoint { input: 0, output: 0 }, CurvePoint { input: 255, output: 255 }]
    }

    #[test]
    fn insert_keeps_the_inputs_sorted_and_unique() {
        let mut p = pts();
        let idx = insert_point(&mut p, 128, 200);
        assert_eq!(idx, 1);
        assert_eq!(p.iter().map(|c| c.input).collect::<Vec<_>>(), [0, 128, 255]);
        let same = insert_point(&mut p, 128, 40);
        assert_eq!(same, 1);
        assert_eq!(p.len(), 3, "the same input overwrites, never duplicates");
        assert_eq!(p[1].output, 40);
    }

    #[test]
    fn a_drag_stays_strictly_between_the_neighbours() {
        let mut p = pts();
        insert_point(&mut p, 128, 128);
        drag_point(&mut p, 1, 0, 90);
        assert_eq!(p[1].input, 1, "clamped above the previous point");
        drag_point(&mut p, 1, 255, 90);
        assert_eq!(p[1].input, 254, "clamped below the next point");
        assert_eq!(p[1].output, 90, "the output is never clamped");
    }

    #[test]
    fn the_end_points_clamp_to_the_full_input_range() {
        let mut first = pts();
        drag_point(&mut first, 0, 255, 10);
        assert_eq!(first[0].input, 254, "the first point stops one short of the next");
        let mut last = pts();
        drag_point(&mut last, 1, 0, 10);
        assert_eq!(last[1].input, 1, "the last point stops one past the previous");
    }
}
