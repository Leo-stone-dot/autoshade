//! The open photo's edit as ONE aggregate: what a save persists, what a
//! render reads, what an undo step swaps — owned together so a feature has a
//! home that is not `AutoShadeApp`.
//!
//! Fields are `pub(crate)` for this slice (the mechanical move); the next slice
//! makes them private and turns every mutation into a method that maintains its
//! invariant. The point of the aggregate is ownership: these state objects die
//! together when another photo opens and are never splintered across clients.

use crate::*;

pub(crate) struct Editor {

    /// The open photo and its decoded plates (see [`PhotoPixels`]).
    pub(crate) photo: PhotoPixels,
    /// The canvas textures and their identity (see [`CanvasPlates`]).
    pub(crate) plates: CanvasPlates,
    /// The latest-wins develop scheduler's counters (see [`DevelopScheduler`]).
    pub(crate) scheduler: DevelopScheduler,
    /// The open document — recipe, cards, active index (see [`Document`]).
    pub(crate) doc: Document,
    /// The AI analysis session (see [`AnalysisSession`]).
    pub(crate) analysis: AnalysisSession,
    // WHERE an export lands (R22-7) — the setting that replaced the
    // Export / Download… button pair. See [`ExportDest`].
    // The folder the last export landed in; ExportDest::LastUsed delivers
    // there, and the ask-dialog reopens there.
    // The 「export .xmp beside the photo」 button is armed for a SECOND click:
    // a sidecar already sits beside this photo (Lightroom's own, or an earlier
    // copy) and the next click overwrites it. Session state, cleared on every
    // photo open and after the copy lands — a two-click confirm rather than a
    // modal, matching how the rest of this panel confirms in place.
    // --- undo / redo (a drag is one step, committed on release). Each step
    // carries the recipe AND the active variant's pixel identity (base Arc +
    // origin), so a baked pixel retouch (heal / clone / denoise) is
    // one undoable step instead of a point of no return. Arc clones share the
    // allocation — only a retouch introduces a new raster into history.
    /// The undo history and the ● baseline; see [`HistoryState`].
    /// The per-photo editing session (see [`EditorSession`]).
    pub(crate) session: EditorSession,
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            photo: PhotoPixels::default(),
            plates: Default::default(),
            scheduler: Default::default(),
            doc: Default::default(),
            analysis: Default::default(),
            session: Default::default(),
        }
    }
}
