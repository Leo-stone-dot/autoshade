//! Settings' folder dialogs, as verbs the view can ask for. The settings form
//! itself is view state; the dialogs and the migration run outside it.

/// What the settings panel asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SettingsAction {
    /// Pick an old pre-store ./out folder and migrate its develops.
    ImportLegacyFolder,
    /// Pick the delivery folder into the form field.
    PickDeliveryFolder,
}
