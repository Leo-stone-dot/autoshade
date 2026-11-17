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
