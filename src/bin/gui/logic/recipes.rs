//! The recipe library's dialog verbs. The library itself is core
//! (`autoshade::recipe_library`); this is only the view's ask — the runtime
//! opens the dialog and runs the import/save side of it.

/// What the recipe-library window asks for. A closed per-domain enum.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RecipeAction {
    /// Pick a recipe file and add it to the library.
    ImportFile,
    /// Re-read the library listing.
    Refresh,
    /// Save the open card's develop as a new library entry.
    SaveCurrent,
    /// Put this entry on the canvas (nothing written until Ctrl+S).
    ApplyToOpen(String),
    /// Paste this entry onto the batch selection.
    ApplyToSelection(String),
    /// Overwrite this entry with the open card's develop.
    UpdateFromOpen(String),
    Rename { from: String, to: String },
    Delete(String),
}
