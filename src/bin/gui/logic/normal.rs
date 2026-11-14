//! Normal's Save group verbs, as actions the view can ask for. The export
//! itself is the app's (`start_normal_save` / `start_apply_to_folder`); the
//! view only says which one was pressed.

/// What the Normal panel's Save group asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum NormalAction {
    /// Save the card on screen as a jpg.
    SaveJpg,
    /// Paste the open look onto every photo, then render each as a jpg.
    ApplyToFolder,
    /// Pick the reference-photo folder, then read it and start the look.
    PickReferencesFolder,
}
