//! The open photo's edit as ONE aggregate: what a save persists, what a
//! render reads, what an undo step swaps — owned together so a feature has a
//! home that is not `AutoShadeApp`.
//!
//! Its six state objects are PRIVATE. The only ways in are the read accessors
//! (`doc()`, `photo()`, …), the mutation accessors, and the methods that own a
//! multi-step invariant: `active_variant_mut`, `sync_active_recipe`,
//! `flush_active_variant`, `insert_variant_after_active` and `history_mut`.
//! The point of the aggregate is ownership: these state objects die together
//! when another photo opens and are never splintered across clients.

use crate::*;

pub(crate) struct Editor {

    /// The open photo and its decoded plates (see [`PhotoPixels`]).
    photo: PhotoPixels,
    /// The canvas textures and their identity (see [`CanvasPlates`]).
    plates: CanvasPlates,
    /// The latest-wins develop scheduler's counters (see [`DevelopScheduler`]).
    scheduler: DevelopScheduler,
    /// The open document — recipe, cards, active index (see [`Document`]).
    doc: Document,
    /// The AI analysis session (see [`AnalysisSession`]).
    analysis: AnalysisSession,
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
    session: EditorSession,
}

impl Editor {
    pub(crate) fn photo(&self) -> &PhotoPixels {
        &self.photo
    }

    pub(crate) fn photo_mut(&mut self) -> &mut PhotoPixels {
        &mut self.photo
    }

    pub(crate) fn plates(&self) -> &CanvasPlates {
        &self.plates
    }

    pub(crate) fn plates_mut(&mut self) -> &mut CanvasPlates {
        &mut self.plates
    }

    pub(crate) fn scheduler(&self) -> &DevelopScheduler {
        &self.scheduler
    }

    pub(crate) fn scheduler_mut(&mut self) -> &mut DevelopScheduler {
        &mut self.scheduler
    }

    pub(crate) fn doc(&self) -> &Document {
        &self.doc
    }

    pub(crate) fn doc_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    pub(crate) fn analysis(&self) -> &AnalysisSession {
        &self.analysis
    }

    pub(crate) fn analysis_mut(&mut self) -> &mut AnalysisSession {
        &mut self.analysis
    }

    pub(crate) fn session(&self) -> &EditorSession {
        &self.session
    }

    pub(crate) fn session_mut(&mut self) -> &mut EditorSession {
        &mut self.session
    }

    /// The variant the document shows.
    pub(crate) fn active(&self) -> usize {
        self.doc.active
    }

    /// The active variant, mutably — the canvas→card sync's one borrow.
    pub(crate) fn active_variant_mut(&mut self) -> Option<&mut Variant> {
        let active = self.doc.active;
        self.doc.variants.get_mut(active)
    }

    /// Copy the live recipe into the ACTIVE variant (the edit→card flush).
    pub(crate) fn sync_active_recipe(&mut self) {
        let recipe = self.doc.recipe.clone();
        if let Some(cur) = self.doc.variants.get_mut(self.doc.active) {
            cur.recipe = recipe;
        }
    }

    /// Sync the active variant and, when no frame landed for the snapshot,
    /// drop its stale thumb (the switch/push card rule, L06#6).
    pub(crate) fn flush_active_variant(&mut self, drop_thumb: bool) {
        let recipe = self.doc.recipe.clone();
        if let Some(cur) = self.doc.variants.get_mut(self.doc.active) {
            cur.recipe = recipe;
            if drop_thumb {
                cur.thumb = None;
            }
        }
    }

    /// Insert a variant after the active one and make it active.
    pub(crate) fn insert_variant_after_active(&mut self, variant: Variant) {
        let at = (self.doc.active + 1).min(self.doc.variants.len());
        self.doc.variants.insert(at, variant);
        self.doc.active = at;
    }

    /// The undo/redo history — one mutable borrow for a multi-stack move.
    pub(crate) fn history_mut(&mut self) -> &mut HistoryState {
        &mut self.session.hist
    }
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
