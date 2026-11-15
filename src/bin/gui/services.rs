//! The app's infrastructure services: the worker channel and the egui context a
//! worker wakes. They are NOT photo state — replacing the executor or the event
//! loop should not touch the document, the panels or the history.

use std::sync::mpsc::{Receiver, Sender};

use crate::model::Msg;

/// What the app hands to background work: the completion channel and the
/// event-loop context a completion repaints through (a plain mpsc send does not
/// wake egui).
pub(crate) struct Services {
    pub(crate) rx: Option<Receiver<Msg>>,
    pub(crate) tx: Sender<Msg>,
    pub(crate) egui_ctx: egui::Context,
}

impl Default for Services {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self { rx: Some(rx), tx, egui_ctx: egui::Context::default() }
    }
}
