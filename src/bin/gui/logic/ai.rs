//! The AI panel's folder dialogs, as verbs the view can ask for. Building and
//! reading the reference library is core; only the pick is a dialog.

/// What the AI panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AiAction {
    /// Pick the optional RAW folder the reference settings are read from.
    PickRawFolder,
    /// Pick the reference-photo folder and start its build.
    PickReferenceFolder,
    /// Read the reference library's summary once (the panel's lazy status).
    RefreshLibraryStatus,
    /// Build / rebuild the library from the remembered folder.
    BuildReferenceLibrary,
    /// Whole-frame「AI generate」.
    Reimagine,
    /// Adjust the selected generated card.
    AdjustGenerated,
    /// Reverse-fit the reference onto this photo.
    ReverseFit,
    /// Extract the look as a style prompt file.
    StylePrompt,
}
