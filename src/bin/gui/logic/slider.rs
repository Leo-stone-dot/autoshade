//! The develop sliders' pure nudge/range rules: arrow-key stepping with the
//! class's shown decimals, and the room a parametric split keeps between its
//! neighbours.

/// One arrow-key nudge: `step` up or down, rounded to `decimals` shown digits
/// (a direct assignment bypasses the widget's own rounding, so an inherited
/// 13.485 must nudge to 14, never to a hidden 14.485), then clamped to the
/// slider's range.
pub(crate) fn nudged(value: f32, up: bool, step: f32, decimals: usize, min: f32, max: f32) -> f32 {
    let f = 10f32.powi(decimals as i32);
    let next = ((value + if up { step } else { -step }) * f).round() / f;
    next.clamp(min, max)
}

/// The open range of split `i` of three (shadow/midtone/highlight): each split
/// keeps `gap` from its neighbours, and the outer sides keep `gap` from the
/// ends. The returned reset value is clamped into the same room.
pub(crate) fn split_bounds(i: usize, splits: [f32; 3], gap: f32, defaults: [f32; 3]) -> (f32, f32, f32) {
    let lo = if i == 0 { gap } else { splits[i - 1] + gap };
    let hi = if i == 2 { 100.0 - gap } else { splits[i + 1] - gap };
    (lo, hi, defaults[i].clamp(lo, hi))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_nudge_rounds_to_the_shown_decimals_then_clamps() {
        assert_eq!(nudged(13.485, true, 1.0, 0, -100.0, 100.0), 14.0, "one shown digit: 14, not 14.485");
        assert_eq!(nudged(13.4, true, 1.0, 1, -100.0, 100.0), 14.4);
        assert_eq!(nudged(99.9, true, 1.0, 0, -100.0, 100.0), 100.0, "clamped at the top");
        assert_eq!(nudged(-99.9, false, 1.0, 0, -100.0, 100.0), -100.0, "clamped at the bottom");
    }

    #[test]
    fn the_shift_nudge_steps_ten_times() {
        assert_eq!(nudged(0.0, true, 10.0, 0, -100.0, 100.0), 10.0);
        assert_eq!(nudged(0.0, false, 10.0, 0, -100.0, 100.0), -10.0);
    }

    #[test]
    fn each_split_keeps_the_gap_from_its_neighbours_and_the_ends() {
        let splits = [30.0, 50.0, 70.0];
        let gap = 5.0;
        let defaults = [25.0, 50.0, 75.0];
        let (lo, hi, reset) = split_bounds(0, splits, gap, defaults);
        assert_eq!((lo, hi), (5.0, 45.0), "the shadow split stops a gap before the midtone");
        assert_eq!(reset, 25.0, "the default lands inside the room");
        let (lo, hi, _) = split_bounds(2, splits, gap, defaults);
        assert_eq!((lo, hi), (55.0, 95.0), "the highlight split keeps its gap on both sides");
    }

    #[test]
    fn a_reset_outside_the_room_is_clamped_into_it() {
        let splits = [10.0, 20.0, 30.0];
        let defaults = [25.0, 50.0, 75.0];
        let (_, _, reset) = split_bounds(0, splits, 5.0, defaults);
        assert_eq!(reset, 15.0, "the shadow default is clamped to hi = midtone - gap");
    }
}
