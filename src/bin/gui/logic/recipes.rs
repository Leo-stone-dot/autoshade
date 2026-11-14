//! The recipe library's dialog verbs. The library itself is core
//! (`autoshade::recipe_library`); this is only the view's ask — the runtime
//! opens the dialog and runs the import/save side of it.

/// What the recipe-library window asks for. A closed per-domain enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum RecipeAction {
    /// Pick a recipe file and add it to the library.
    ImportFile,
}
