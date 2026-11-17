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
