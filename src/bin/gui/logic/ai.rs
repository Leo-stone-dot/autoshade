//! The AI panel's folder dialogs, as verbs the view can ask for. Building and
//! reading the reference library is core; only the pick is a dialog.

/// What the AI panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiAction {
    /// Pick the optional RAW folder the reference settings are read from.
    PickRawFolder,
    /// Pick the reference-photo folder and start its build.
    PickReferenceFolder,
}
