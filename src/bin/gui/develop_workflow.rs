//! The develop/redevelop workflow: when to dispatch a preview frame, and
//! whether a landed frame still describes the live base + recipe.
//!
//! The worker and the texture upload stay host responsibilities; this module
//! owns the two decisions that used to sit inside `start_redevelop` and
//! `finish_redevelop`.

use std::sync::Arc;

use crate::*;

/// What the frame gate asks the host to do.
pub(crate) enum DevelopDispatch {
    /// Nothing to do: a frame is already in flight, the base part is not
    /// ready, or there are no pixels to develop.
    Skip,
    /// The source frame cannot develop this recipe; the host words it.
    Fail(String),
    /// Start one worker with this snapshot.
    Start {
        base: Arc<image::DynamicImage>,
        recipe: Box<EditRecipe>,
        show_clipping: bool,
        film: Option<u32>,
        source: autoshade::render::SourceFrame,
    },
}

/// Decide whether to start a preview worker. `source_part_ready` is the
/// host's `ensure_source_part()` answer; `source` is the host's checked
/// source frame, already formatted for the status line.
pub(crate) fn plan_dispatch(
    inflight: bool,
    source_part_ready: bool,
    base: Option<Arc<image::DynamicImage>>,
    recipe: &EditRecipe,
    show_clipping: bool,
    film: Option<u32>,
    source: Result<autoshade::render::SourceFrame, String>,
) -> DevelopDispatch {
    if inflight || !source_part_ready {
        return DevelopDispatch::Skip;
    }
    let Some(base) = base else {
        return DevelopDispatch::Skip;
    };
    let source = match source {
        Ok(source) => source,
        Err(e) => return DevelopDispatch::Fail(e),
    };
    DevelopDispatch::Start { base, recipe: Box::new(recipe.clone()), show_clipping, film, source }
}

/// Is a landed frame stale? It is current only when it was solved for the
/// live base allocation AND the live recipe.
pub(crate) fn is_stale(
    frame: &PreviewDone,
    current_base: Option<&Arc<image::DynamicImage>>,
    current_recipe: &EditRecipe,
) -> bool {
    !(current_base.is_some_and(|base| Arc::ptr_eq(base, &frame.base)) && current_recipe == &frame.recipe)
}
