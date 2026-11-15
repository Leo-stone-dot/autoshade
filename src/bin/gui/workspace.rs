//! The client-only workspace state: everything the renderer remembers that is
//! NOT the photo's edit and must never be persisted — folds, tabs, hovers,
//! windows, the gallery scan, tool arming, transient status.
//!
//! It lives in its own aggregate so the composition root is a wiring point
//! rather than a drawer of view fields. Its fields are private: reads go
//! through the value/`&` accessors, writes through `_mut` accessors, the scalar
//! and text doors (`set_status`, `set_busy`, …), and the two multi-field
//! methods a single draw path needs (`paint_upload_parts`, `brush_parts`).

use crate::*;

pub(crate) struct WorkspaceView {
    status: String,
    busy: bool, // an analyze/export thread is running
    // Consumed by the startup greeting or the restored folder's status.
    /// The status chrome (toasts, histogram, title, clipping); see [`ChromeState`].
    chrome: ChromeState,
    /// R23-3, feedback #5 ("the AI is too timid"): how COMMITTED the grade
    /// should be, 0..1. A SEPARATE axis from `style_strength` — that one asks
    /// how much like the user's own past edits, this one how hard to push — and
    /// keeping them apart is the point of the round. Persisted; stored as the
    /// raw fraction and handed to `GradeStrength::new` at the call site, which
    /// owns the clamp.
    /// The reverse-fit's HONESTY BUDGET, 0..1 — its OWN dial in the Reverse-fit
    /// fold (user decision 2026-09-12: a control belongs to the section whose
    /// function it serves, and a function two sections need gets two controls,
    /// not one shared reading). Through v1.3.1 the fit read the Analysis
    /// Strength two folds up, which nobody looking at the fit's own row could
    /// find. Persisted as the raw fraction; `GradeStrength::new` owns the clamp
    /// at the call site, exactly like `grade_strength`.
    /// R23-2, feedback #6: also SHOW the vision model the nearest past photo,
    /// not only its numbers. Persisted, and OFF by default — it puts a second
    /// image on every call of a paid analysis (the checkbox says so).
    /// R23-4, feedback #13 ("let it think"): DEEP THINKING for this photo's
    /// analysis — the structured working in the proposal, one step more
    /// reasoning effort, and a visual judge allowed to converge over more than
    /// one round. Persisted, OFF by default: it is the most expensive switch on
    /// the panel (the checkbox states the call counts), and every unattended
    /// surface (batch, eval) is structurally unable to turn it on.
    /// Step 14 / S2: also write a LOCAL prose description of each record's
    /// grade. Persisted, OFF by default — it downloads 4.3 GB once and makes
    /// every build slower, which is not a cost an upgrade may take unasked.
    /// The folder the reference library was last built FROM — prefilled into
    /// the picker so a rebuild does not mean finding it again. Persisted.
    /// The RAW folder a build looks in for each finished photo's sidecar
    /// settings (`library::settings`). Persisted; `None` = no settings check.
    /// The reference library's status as last READ (`library::summary`),
    /// cached: it parses a file that can reach tens of MB, so it must never
    /// happen per frame. `None` = not read yet.
    /// The AI section's run/library/tab state; see [`AiState`].
    ai: AiState,
    /// A status read is on a worker thread (keeps the first draw off the disk).
    /// A library BUILD is running. Its own flag, never the global `busy`:
    /// building measures every photo in a folder and takes minutes, and freezing
    /// the whole app for that would be worse than the missing entry point this
    /// closes. Only the build button gates on it.
    /// The running build's `(stage, completed, total)` — typed facts, worded
    /// at draw time (L12#4). The STAGE is named because the build is four
    /// phases over the whole folder: a bare pair would sweep 0..N more than
    /// once with nothing to say which sweep the user was looking at.
    // Delivery format + depth (阶段4) — replaces the old save_jpeg bool.
    /// The Export panel's dials and delivery facts; see [`ExportState`].
    exp: ExportState,
    // --- unsaved-edit protection ---
    // What recipe.json last held: the develop of the card that was active at
    // the last save. `active_baseline` joins it with saved_strip so every card
    // is compared to its own persisted state.
    // Mirror of the open photo's pixels.json (the saved baked-master path, if
    // any) so the per-frame ● indicator can compare pixel identity WITHOUT a
    // disk read per frame. Updated at open, save, analyze-save and clear; it is
    // the pixel half of the last-saved active card's baseline.
    /// Mirror of the photo's persisted `variants.json` (resolved form), the
    /// baseline the background-variant dirty test compares the live strip
    /// against. `None` = nothing persisted (the trivial single-card case).
    /// Updated at open and on every strip persist; comparing background
    /// variants against the ACTIVE canvas's `saved_recipe`/`pixels_on_disk`
    /// instead (the pre-v0.22 rule) made any two-origin strip permanently
    /// "unsaved" — the quit dialog then re-armed forever.
    /// The open/photo-source state; see [`OpenState`].
    open: OpenState,
    /// Master decodes in flight, keyed (photo, origin): repeat clicks into a
    /// still-decoding cold card must coalesce, not stack a fresh full-res
    /// decode thread per click.
    /// Decoded cold-variant masters, most-recent last. Keyed by the master's
    /// OWN identity (path + the edge it was shrunk to + its file stamp), the
    /// same rule as `base_cache` — a rewritten pixel master misses, and a px
    /// preference change re-decodes at the new edge instead of serving the
    /// old one.
    /// The open could not READ the saved develop (damaged file, or busy in
    /// another AutoShade process): the canvas baseline is not that save, so
    /// the next explicit save must version what it overwrites (save_xmp runs
    /// the backup gate while this stands).
    // Paste-to-selected wrote the open photo's store: (path, exact recipe
    // written) so Msg::Pasted can advance the ● baseline on full success.
    // The OPEN photo's fresh START (computed by the open worker, v1.17.0):
    // Lightroom's Adobe Color canvas where this machine can render it, else
    // the camera-matched base-look knots (empty for non-RAW / no preview).
    // Reset re-stamps it so "reset" always means the fresh-open look —
    // including on a photo whose legacy save carries no curve and therefore
    // opened dark.
    // The base part `source_preview` was developed under (v1.17.0,
    // `render::BasePart`): a source-based canvas whose recipe names another
    // is re-developed before it is shown (`ensure_source_part`).
    // The part a source re-develop is in flight for, so a held slider does
    // not queue a second one.
    // This photo's in-camera lens profile (open worker) — the twin of
    // photo_start: Reset re-stamps it, legacy toggles re-enable from it.
    // This photo's as-shot WB anchor (open worker) — the third calibration
    // half: Reset re-stamps it, the Temp slider and eyedropper anchor on it.
    // The start baked into `before_tex` (`EditRecipe::start_only`). The Before
    // pane mirrors the canvas recipe's start (Reset / paste / restore can
    // change it), so a mismatch triggers a cheap rebuild in `update`.
    // The title-bar ✕ was intercepted with unsaved edits: show the in-app
    // save-all / discard / cancel layer until the user decides.
    // Local buffer for the selected mask's name TextEdit:
    // (mask index, name at seed time, edited text). Committed on focus loss so
    // a rename is ONE undo step, not one per key; the seed-time name lets a
    // pending rename commit safely when the selection jumps rows (commit only
    // while masks[i].name still equals the seed — index shifts can't misfire).
    /// The painting tools and mask-edit state; see [`MaskEditState`].
    masks: MaskEditState,
    // Arrow-key navigation: gallery index to scroll into view next frame.
    // --- settings / denoise ---
    /// 「🤖 AI Denoise on export」's OWN strength (the Export fold's dial) and
    /// 「🤖 AI Denoise now」's OWN strength (the Detail fold's dial) — two
    /// timings of one denoiser, two dials, neither reaching the other (user
    /// decision 2026-09-12: a control sits in the fold whose verb reads it).
    /// RAW starts at the luminance-grain default; baked sources have
    /// separate saved choices, starting at SCUNet's unchanged 0.5.
    /// The reverse-fit dials and its dialog state; see [`FitState`].
    fit: FitState,
    // R23-6 D: 反推 runs the visual judge BEFORE the persist and buys bounded
    // guided rounds off it. Paid and opt-in, like `fit_ai_judge`, and a
    // REVISION of the R20 decision that the fit's review is informational
    // only — user decision 2026-08-17 ⑥, recorded at `start_fit`.
    // Step-6: opt-in fidelity retry for reimagine — a second billed
    // generation when the first diverges. Paid and opt-in, like `fit_deep`.
    // R23-6 B: an explicitly chosen reverse-fit reference (any finished
    // rendition of this frame — a Lightroom export, a camera JPEG, another
    // RAW's develop). SESSION state, not a preference: it names one file for
    // one photo, and remembering it across an app restart would silently fit
    // the next photo against the last photo's reference. Cleared on open.
    // 2026-09-30 (user decision): the card the reverse-fit solves FROM, by
    // id — `None` = automatic (`negative_index`). Session state, cleared on
    // open, for `fit_ref`'s reason: an id names a card of one photo.
    // v1.17.0: the question a whole-frame reimagine landing asks
    // (`fit_prompt_layer`) — None = not asking. Session state: it names a
    // card of the open photo.
    // Whether to ask it at all (a pref): the prompt's 「Don't ask again」 turns
    // it off, the reverse-fit fold's checkbox turns it back on.
    // Tab hides both side panels (the LR grammar) for an edge-to-edge canvas.
    // Session-only on purpose: relaunching with an invisible library reads as
    // a broken app, so the state never persists.
    // egui derives Tab's focus-traversal direction from raw input BEFORE
    // update() runs, so consuming the key can't stop the first widget from
    // taking focus this frame — which would also kill every focused-none
    // shortcut. Cleared on the NEXT frame instead.
    /// The window shell (settings form, theme, open windows, probes); see [`ShellState`].
    ui: ShellState,
    // --- library / gallery ---
    /// The gallery's own state (scan, thumbnails, selection); see
    /// [`GalleryState`].
    gal: GalleryState,
    /// Decode attempts that FAILED, per gallery index. Bounds the retry: see
    /// `request_thumb`. Not a cache — it holds no pixels, only a count.
    // Decoded-base LRU (recent last): base pixels + the photo's base-look knots.
    // --- region box-select (local-edit target on the After image) ---
    /// The coverage overlay and the AI region; see [`OverlayState`].
    overlay: OverlayState,
    // What a lone click just cleared, kept for one double-click window: the
    // second release of a Fit↔1:1 double-click restores it — the pair's
    // FIRST release already ran the plain-click clear (M18).
    // --- retouch: mask painting + generative fill + heal ---
    // Presence is independent of the texture upload flag: None means one
    // scan is due after a change; Some is an O(1) answer on every frame.
    /// Whose area the canvas holds (`BrushOwner`); the other tools' areas
    /// wait in `paint_store`, by `BrushOwner::slot` (None = blank).
    // (straighten, distortion, (profile distortion, profile CA), (ca_r, ca_b)) at build.
    // The manual CA pair is part of the KEY, not just of the composed profile
    // it feeds: the two bools say whether CA is on, never how far, so moving
    // ca_r from +5 to +50 compared equal and the overlay kept the old warp
    // (the L13-9 defect, one field over).
    // Union of the brush segments painted since the last texture upload
    // (canvas px, [x0,y0,x1,y1] half-open). With no geometry active, only
    // this sub-rectangle is uploaded (set_partial) — brushing used to clone
    // + re-upload the WHOLE canvas on every pointer move.
    /// A LOCAL-ADJUSTMENT brush session rides the same paint canvas as
    /// fill/heal: `(target, erase)` — target `None` = the strokes become a
    /// NEW Bitmap mask on Apply; `Some(i)` = they edit mask `i`'s raster
    /// (seeded below). While `Some`, `paint_mode` is also true so every
    /// existing paint affordance (dispatch, cursor, Esc, `[`/`]`) works.
    /// The brush session's WEIGHT buffer — greyscale source of truth the
    /// Apply bakes to a raster. The RGBA canvas is only its display: seeding
    /// an existing raster through the canvas' 8-bit alpha and back would
    /// drift soft edges by a rounding step per edit cycle.
    /// The generative tools and the recipe clipboard; see [`GenerativeState`].
    genai: GenerativeState,
    /// What to change on the SELECTED card's develop (2026-10-21): the request
    /// 「AI adjust the selected variant」 sends with the card's render and
    /// recipe. Its own field — the Direction box guides a fresh analysis.
    // --- production niceties ---
    // --- diagnostic view layers (UX batch) ---
    // Cached masks-cleared develop the coverage's range weights are judged
    // on — reused while the global (non-mask) recipe is unchanged, so a
    // mask-slider drag rebuilds only the coverage map, not a second develop.
    // First-frame OS titlebar sync (see update()'s head): viewport commands
    // sent from the creation closure are dropped before the loop runs.
    // --- zoom / pan (per-photo, reset on open) ---
    /// The canvas view state; see [`ViewState`].
    view: ViewState,
    // The zoom that puts one texel on one PHYSICAL pixel — computed where
    // vis_px/disp/ppp exist (canvas after_view, each frame) so the `1` key
    // can act at frame start, before layout. 1.0 until a first frame lands.
    // Where the zoom is HEADING: discrete jumps (Fit/1:1/keys/double-click)
    // set only this and `zoom` glides there over ~120 ms (apply_zoom_glide,
    // one glide_step per frame from update());
    // continuous inputs (scroll) and photo opens write both, staying
    // instant. Pan is never animated — view_uv's per-frame clamp re-solves
    // it against the gliding zoom, which eases it for free.
    // Last frame's controls-panel rect: ↑/↓ walk the library EXCEPT over
    // this panel, where the same keys nudge a hovered slider.
    // --- crop tool ---
    /// The crop tool's state; see [`CropState`].
    crop: CropState,
    // (handle, drag start, crop at start, straighten° at start). Handles:
    // 0-3 corners, 4 move-inside, 5-8 edge midpoints (T/B/L/R), 9 = drag
    // OUTSIDE the box → rotate-straighten (the LR gesture).
    // --- manual local adjustments (masks) ---
    /// Which of the selected mask's COMPONENTS the canvas tools target:
    /// `None` = the base geometry. Reset whenever the mask selection moves.
    /// The combine mode the panel's "add shape" buttons will arm next.
    // --- tone-curve editor ---
    curve_channel: usize,                  // CURVE_CHANNELS index: 0=master 1=R 2=G 3=B
    // The control point being dragged, WITH the curve set it belongs to
    // (R25 P6: the global curves and a selected mask's can both be on screen
    // in one frame — see `CurveTarget`). Active channel is `curve_channel`.
    curve_drag: Option<(CurveTarget, usize)>,
    // --- batch recipe copy / paste ---
    // WHICH photo the clipboard came from: pasting back onto that photo may
    // keep its bitmap masks (the rasters are its own); every other target
    // gets them stripped.
    // --- recipe library (named develops, this file's siblings under <store>/recipes) ---
    /// The named-recipe library panel; see [`RecipeLibraryState`].
    rlib: RecipeLibraryState,
    /// The selected library recipe developed on the open photo (the window's
    /// preview pane); rebuilt when the selection, the photo or the file changes.
    // --- WB eyedropper ---
    /// The canvas tools and AI/generation flight flags; see [`ToolsState`].
    tools: ToolsState,
    // --- colour-range sample (Range Mask) ---
    // --- Point Color eyedropper (v1.5.0) ---
    // --- AI lens blur (2026-10-13) ---
    // --- clone stamp ---
    // --- stacking (v1.5.0 Track S) ---
    // The Stack fold's own two dials. Deliberately NOT in `Prefs`: which merge
    // a set of frames wants is a property of that shoot, not a taste — a
    // bracket and a focus sweep are different jobs, and remembering last
    // week's answer would run the wrong one on the first click.
    // --- cancellable retouch/generative workers ---
    // Cancel bumps `gen_epoch`; a worker's Retouched message carries the epoch
    // it was started under and is discarded on mismatch (the canvas is never
    // mutated by an abandoned task). `gen_cancel` is Some while such a worker
    // is in flight — it arms the status-bar ✕ and, for streaming generative
    // calls, is polled between events to stop the download itself.
    /// An `Analyze` worker is still on the wire, cancelled or not.
    ///
    /// Cancelling an analyze is ABANDON-only — `produce_recipe` takes no
    /// cancel flag, so nothing stops the HTTP call — and ✕ clears `busy`,
    /// whose only other job was to gate `logic::look::plan`. Without this the ✕
    /// became a "start another one" button: each press spawned a fresh
    /// high-detail vision request that ran to its stall deadline and was
    /// BILLED, N deep. The flag is cleared when the worker's message lands,
    /// whatever its epoch.
    /// The one AI look in flight, with the epoch/keep facts the landing needs
    /// (`logic::look`).
    // --- variants (版本/变体): parallel renditions of the open photo ---
    // Original + any AI-generated / reverse-fitted versions. The active one
    // drives the sliders, histogram and canvas; switching is lossless (each
    // remembers its own base + recipe), so an AI develop no longer "reverts"
    // when you touch a slider — you're editing that variant's own base.
    /// The user answered "Discard & quit". The close guard must then stop
    /// re-testing unsaved state: discarding by definition leaves the state
    /// dirty, so without this the guard cancelled the close and re-raised
    /// the dialog every frame and the app could not be quit at all. (The
    /// SAVE path needs no such flag since v0.22 — Save-all persists each
    /// photo's whole strip to variants.json, so the way-out re-check finds
    /// the state genuinely clean.)
    /// The card strip's dialog/one-shot state; see [`CardsState`].
    cards: CardsState,
    // --- export pipeline (gap batch F + D2) ---
    // --- preview resolution (gap batch E) ---
    // --- recipe versions (gap batch G): ./out/<stem>.v<N>.recipe.json ---
    /// Advisory names + provenance for `versions`, keyed by number (R24-2).
    /// Refreshed with the list; a number with no entry is simply unnamed —
    /// `.version-meta.json` is not required to exist.
    /// A version rename in flight: (the photo it belongs to, the number, the
    /// name as seeded, the edit buffer). Keyed by the NUMBER — versions are
    /// never re-numbered and a deleted number is never re-issued, so unlike
    /// the mask buffer's row index this key cannot cross-commit onto some
    /// other row. The photo rides along so a commit at a boundary that has
    /// already re-pointed `src_path` still writes to the right develop.
    /// Versions list filter: show only snapshots taken from the ACTIVE
    /// variant (R24-2). Session state, deliberately not persisted.
    /// A VARIANT rename in flight (R24-3): (the photo, the card's id, the
    /// name as seeded, the edit buffer). Keyed by the card's own opaque id,
    /// not its strip index — a background push or delete renumbers indices
    /// while a box is open, and the version buffer's number key showed the
    /// way. Unlike the version half this commits into memory only: a card's
    /// name reaches disk with the strip record, at the next save.
    /// The strip's ✕ is ARMED, not fired (R24-4): the index whose delete the
    /// next click confirms. A variant delete is immediate and irreversible —
    /// the undo stack is per-variant by contract (`reset_history`) and no
    /// tombstone can bring a card back — so it asks first, the same
    /// arm-then-act shape as `xmp_beside_confirm`.
    /// Normal's own sidebar state (`panels/normal.rs`).
    normal: crate::panels::normal::NormalState,
    /// The last line uv printed during the managed Python environment's
    /// install (`panels/settings.rs`).
    /// What the sidecar interpreter was found to be at startup
    /// (`python_env::inspect`); `None` until the first check.
    /// The Portraits panel's own state (`panels/portrait.rs`).
    portrait: crate::panels::portrait::PortraitState,
}

impl WorkspaceView {
    pub(crate) fn status(&self) -> String {
        self.status.clone()
    }

    pub(crate) fn status_mut(&mut self) -> &mut String {
        &mut self.status
    }

    pub(crate) fn busy(&self) -> bool {
        self.busy
    }

    pub(crate) fn chrome(&self) -> &ChromeState {
        &self.chrome
    }

    pub(crate) fn chrome_mut(&mut self) -> &mut ChromeState {
        &mut self.chrome
    }

    pub(crate) fn ai(&self) -> &AiState {
        &self.ai
    }

    pub(crate) fn ai_mut(&mut self) -> &mut AiState {
        &mut self.ai
    }

    #[cfg(test)]
    pub(crate) fn busy_mut(&mut self) -> &mut bool {
        &mut self.busy
    }

    #[cfg(test)]
    pub(crate) fn curve_channel_mut(&mut self) -> &mut usize {
        &mut self.curve_channel
    }

    pub(crate) fn exp(&self) -> &ExportState {
        &self.exp
    }

    pub(crate) fn exp_mut(&mut self) -> &mut ExportState {
        &mut self.exp
    }

    pub(crate) fn open(&self) -> &OpenState {
        &self.open
    }

    pub(crate) fn open_mut(&mut self) -> &mut OpenState {
        &mut self.open
    }

    pub(crate) fn masks(&self) -> &MaskEditState {
        &self.masks
    }

    pub(crate) fn masks_mut(&mut self) -> &mut MaskEditState {
        &mut self.masks
    }

    pub(crate) fn fit(&self) -> &FitState {
        &self.fit
    }

    pub(crate) fn fit_mut(&mut self) -> &mut FitState {
        &mut self.fit
    }

    pub(crate) fn ui(&self) -> &ShellState {
        &self.ui
    }

    pub(crate) fn ui_mut(&mut self) -> &mut ShellState {
        &mut self.ui
    }

    pub(crate) fn gal(&self) -> &GalleryState {
        &self.gal
    }

    pub(crate) fn gal_mut(&mut self) -> &mut GalleryState {
        &mut self.gal
    }

    pub(crate) fn overlay(&self) -> &OverlayState {
        &self.overlay
    }

    pub(crate) fn overlay_mut(&mut self) -> &mut OverlayState {
        &mut self.overlay
    }

    pub(crate) fn genai(&self) -> &GenerativeState {
        &self.genai
    }

    pub(crate) fn genai_mut(&mut self) -> &mut GenerativeState {
        &mut self.genai
    }

    pub(crate) fn view(&self) -> &ViewState {
        &self.view
    }

    pub(crate) fn view_mut(&mut self) -> &mut ViewState {
        &mut self.view
    }

    pub(crate) fn crop(&self) -> &CropState {
        &self.crop
    }

    pub(crate) fn crop_mut(&mut self) -> &mut CropState {
        &mut self.crop
    }

    pub(crate) fn curve_channel(&self) -> usize {
        self.curve_channel
    }

    pub(crate) fn curve_drag(&self) -> Option<(CurveTarget, usize)> {
        self.curve_drag
    }

    pub(crate) fn rlib(&self) -> &RecipeLibraryState {
        &self.rlib
    }

    pub(crate) fn rlib_mut(&mut self) -> &mut RecipeLibraryState {
        &mut self.rlib
    }

    pub(crate) fn tools(&self) -> &ToolsState {
        &self.tools
    }

    pub(crate) fn tools_mut(&mut self) -> &mut ToolsState {
        &mut self.tools
    }

    pub(crate) fn cards(&self) -> &CardsState {
        &self.cards
    }

    pub(crate) fn cards_mut(&mut self) -> &mut CardsState {
        &mut self.cards
    }

    pub(crate) fn normal(&self) -> &crate::panels::normal::NormalState {
        &self.normal
    }

    pub(crate) fn normal_mut(&mut self) -> &mut crate::panels::normal::NormalState {
        &mut self.normal
    }

    pub(crate) fn portrait(&self) -> &crate::panels::portrait::PortraitState {
        &self.portrait
    }

    pub(crate) fn portrait_mut(&mut self) -> &mut crate::panels::portrait::PortraitState {
        &mut self.portrait
    }


    /// Write the status bar's text (the two doors below are the only writers)
    pub(crate) fn set_status(&mut self, status: String) {
        self.status = status;
    }

    pub(crate) fn set_busy(&mut self, busy: bool) {
        self.busy = busy;
    }

    pub(crate) fn set_curve_channel(&mut self, channel: usize) {
        self.curve_channel = channel;
    }

    pub(crate) fn set_curve_drag(&mut self, drag: Option<(CurveTarget, usize)>) {
        self.curve_drag = drag;
    }

    /// The paint canvas and the overlay texture it uploads into: one borrow
    /// because a partial upload READS the pixels and WRITES the texture.
    pub(crate) fn paint_upload_parts(
        &self,
    ) -> (Option<&image::RgbaImage>, Option<&egui::TextureHandle>, Option<[u32; 4]>) {
        (self.masks.mask_paint.as_ref(), self.overlay.mask_tex.as_ref(), self.overlay.mask_dirty_rect)
    }

    /// The brush path's two owners in one borrow: the mask canvas and its
    /// fields, and the overlay's pending dirty rect, are written together.
    pub(crate) fn brush_parts(&mut self) -> (&mut MaskEditState, &mut OverlayState) {
        (&mut self.masks, &mut self.overlay)
    }
}

impl Default for WorkspaceView {
    fn default() -> Self {
        Self {
            status: "Open a photo, or open a folder to browse your library.".into(),
            busy: Default::default(),
            chrome: Default::default(),
            ai: Default::default(),
            exp: Default::default(),
            open: Default::default(),
            masks: Default::default(),
            fit: Default::default(),
            ui: Default::default(),
            gal: Default::default(),
            overlay: Default::default(),
            genai: Default::default(),
            view: Default::default(),
            crop: Default::default(),
            curve_channel: Default::default(),
            curve_drag: Default::default(),
            rlib: Default::default(),
            tools: Default::default(),
            cards: Default::default(),
            normal: Default::default(),
            portrait: Default::default(),
        }
    }
}
