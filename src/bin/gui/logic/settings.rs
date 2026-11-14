//! Settings' folder dialogs, as verbs the view can ask for. The settings form
//! itself is view state; the dialogs and the migration run outside it.

/// Which role's model catalogue a fetch fills.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ModelSide {
    Analysis,
    Image,
}

/// What the settings panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsAction {
    /// Pick an old pre-store ./out folder and migrate its develops.
    ImportLegacyFolder,
    /// Pick the delivery folder into the form field.
    PickDeliveryFolder,
    /// Merge the form into the saved settings (the cross-process locked save).
    SaveForm,
    /// Fetch one role's model ids and fill its pick-lists.
    FetchModels(ModelSide),
    /// Install the pinned Python environment for the sidecars.
    InstallPython,
}
