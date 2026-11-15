//! The canvas view's animation math, headless: one glide step toward a zoom
//! target. The frame applies it and asks for the repaint.

/// One exponential glide step toward `target`, with the frame delay capped so a
/// stalled frame cannot jump the zoom. Snaps when within 1e-3 of the target so
/// the animation terminates exactly.
pub(crate) fn glide_step(cur: f32, target: f32, dt: f32) -> f32 {
    let k = 1.0 - (-dt.clamp(0.0, 0.05) / 0.045).exp();
    let next = cur + (target - cur) * k;
    if (target - next).abs() < 1e-3 { target } else { next }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_step_moves_toward_the_target_without_passing_it() {
        let next = glide_step(1.0, 4.0, 1.0 / 60.0);
        assert!(next > 1.0 && next < 4.0, "one step: {next}");
        let back = glide_step(4.0, 1.0, 1.0 / 60.0);
        assert!(back < 4.0 && back > 1.0, "downward too: {back}");
    }

    #[test]
    fn the_step_snaps_when_it_would_only_nudge() {
        assert_eq!(glide_step(1.0004, 1.0, 1.0 / 60.0), 1.0, "no sub-milli drift: the animation ends");
        assert_eq!(glide_step(4.0, 4.0, 1.0 / 60.0), 4.0);
    }

    #[test]
    fn a_long_or_negative_frame_is_clamped() {
        let stalled = glide_step(1.0, 4.0, 10.0);
        let capped = glide_step(1.0, 4.0, 0.05);
        assert_eq!(stalled, capped, "a stalled frame uses the 50 ms cap");
        assert_eq!(glide_step(1.0, 4.0, -1.0), 1.0, "a negative dt does nothing");
    }
}
