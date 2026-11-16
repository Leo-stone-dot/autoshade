//! The Lens Blur section (2026-10-13, user: 「AI 景深虚化」 — the AI estimates
//! depth and blurs the background progressively; the user picks the focus
//! point).
//!
//! VIEW ONLY. This file draws the section and emits [`logic::Action`]s; what
//! each action means, the depth worker and its landing live in
//! `logic/lens_blur.rs`. It may not mutate the recipe, spawn a worker, touch
//! the filesystem or open a dialog — `tests/architecture.rs` pins that.

use crate::*;

impl AutoShadeApp {




}
