//! The application state, its defaults, and the eframe hooks.

use super::*;

/// The selected library recipe, developed on the open photo for the recipe
/// window's preview pane. `key` is everything the image depends on — the
/// entry's name and file stamp, the photo, and the base pixels' allocation —
/// so a changed selection, a re-saved entry or a different photo rebuilds it.
pub(crate) struct RecipePreview {
    pub(crate) key: String,
    pub(crate) texture: egui::TextureHandle,
}

pub(crate) struct AutoShadeApp {
    pub(crate) src_path: Option<PathBuf>,
    // The ACTIVE variant's base pixels — shared with the preview worker by Arc,
    // so dispatching a 4096px develop is O(1), not a 50+ MB UI-thread deep copy.
    pub(crate) base_preview: Option<Arc<image::DynamicImage>>,
    // The pristine source neutral (RAW develop / loaded image), decoded once
    // per open. Source-based variants share this same allocation.
    pub(crate) source_preview: Option<Arc<image::DynamicImage>>,
    /// How many clockwise quarter turns are currently BAKED INTO the plates
    /// above (R27). The canvas has no rotation stage of its own — every
    /// screen↔frame mapping in `canvas.rs` (`view_norm_to_orig`, the crop
    /// handles, the paint canvas, the mask overlay) is defined against the
    /// plate, so a turn is applied to the PIXELS, exactly as the engine
    /// applies orientation before every other stage. This field is the one
    /// place that records which frame those pixels are in;
    /// `sync_base_turns` reconciles it against `recipe.quarter_turns` once per
    /// frame, so an undo, a variant switch or a version load needs no rotation
    /// code of its own.
    pub(crate) base_turns: u8,
    /// The active pixel source's full-resolution SHORT EDGE, cached against
    /// the path and its modification time it was read from
    /// (`film_short_edge`, v1.5.0) — the film the Detail panel's radii are
    /// stated in.
    pub(crate) film_edge: Option<(PathBuf, Option<std::time::SystemTime>, Option<u32>)>,
    pub(crate) before_tex: Option<egui::TextureHandle>,
    pub(crate) after_tex: Option<egui::TextureHandle>,
    /// The canvas cross-fade in flight: the frame being faded OUT (`None` =
    /// fade in from the empty canvas) and when the new one landed. Set when
    /// the first frame of a different photo or version lands
    /// (`after_shows` changes); cleared once [`MOTION_FADE_S`] has passed.
    pub(crate) canvas_fade: Option<(Option<egui::TextureHandle>, Instant)>,
    /// Which photo and version `after_tex` shows, so a landing frame can tell
    /// a slider re-render (no fade) from a swap (fade).
    pub(crate) after_shows: Option<(PathBuf, usize)>,
    pub(crate) recipe: EditRecipe,
    pub(crate) dirty: bool, // recipe changed → queue the latest preview state
    // --- asynchronous develop scheduler (single in-flight, latest wins) ---
    pub(crate) develop_inflight: bool,
    pub(crate) develop_count: u64, // accepted frames; regression counter (latest-wins)
    pub(crate) status: String,
    // Consumed by the startup greeting or the restored folder's status.
    /// The status chrome (toasts, histogram, title, clipping); see [`ChromeState`].
    pub(crate) chrome: ChromeState,
    pub(crate) busy: bool, // an analyze/export thread is running
    pub(crate) rx: Option<Receiver<Msg>>,
    pub(crate) tx: Sender<Msg>,
    /// Clone of the runtime egui context, installed at creation
    /// (`actions.rs::new`). Worker spawn and completion request repaints
    /// through it: a plain mpsc send does not wake the event loop, and the
    /// 100 ms pump's gate (`poll_workers`) runs BEFORE any panel can start a
    /// fetch in the same frame. Tests get the headless default.
    pub(crate) egui_ctx: egui::Context,
    // TYPED, not a baked string: the decision word + reasons are rendered
    // (and translated) at draw time, so a language switch re-renders them.
    pub(crate) verdict: Option<(autoshade::advisor::Decision, Vec<String>)>,
    /// Scripts already disclosed as undrawable this session (L12#3) — the
    /// tofu warning fires once per script, not once per folder or frame.
    pub(crate) disclosed_scripts: HashSet<&'static str>,
    pub(crate) rationale: String,
    /// The rationale's deterministic tail as typed notes (L12#2B). They
    /// render, in order, `rationale`'s SUFFIX — the panel strips it and
    /// draws the notes localized; on any mismatch (or when empty — a
    /// develop restored from disk carries none) the raw English rationale
    /// shows instead. Cleared wherever `rationale` is reloaded from disk.
    pub(crate) rationale_notes: Vec<autoshade::rationale::Note>,
    /// What guides AI Analyze: the reference library or the Direction text —
    /// either/or (user ruling 2026-10-14). The controls of the one not chosen
    /// are greyed out. Persisted.
    /// The persisted AI preferences; see [`AiPrefs`].
    pub(crate) aip: AiPrefs,
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
    pub(crate) ai: AiState,
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
    pub(crate) exp: ExportState,
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
    pub(crate) hist: HistoryState,
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
    pub(crate) open: OpenState,
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
    pub(crate) masks: MaskEditState,
    // Arrow-key navigation: gallery index to scroll into view next frame.
    // --- settings / denoise ---
    /// 「🤖 AI Denoise on export」's OWN strength (the Export fold's dial) and
    /// 「🤖 AI Denoise now」's OWN strength (the Detail fold's dial) — two
    /// timings of one denoiser, two dials, neither reaching the other (user
    /// decision 2026-09-12: a control sits in the fold whose verb reads it).
    /// RAW starts at the luminance-grain default; baked sources have
    /// separate saved choices, starting at SCUNet's unchanged 0.5.
    /// The reverse-fit dials and its dialog state; see [`FitState`].
    pub(crate) fit: FitState,
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
    pub(crate) ui: ShellState,
    pub(crate) lang: Lang,             // UI language: English skeleton / Chinese overlay (i18n.rs)
    // --- library / gallery ---
    /// The gallery's own state (scan, thumbnails, selection); see
    /// [`GalleryState`].
    pub(crate) gal: GalleryState,
    /// Decode attempts that FAILED, per gallery index. Bounds the retry: see
    /// `request_thumb`. Not a cache — it holds no pixels, only a count.
    // Decoded-base LRU (recent last): base pixels + the photo's base-look knots.
    // --- region box-select (local-edit target on the After image) ---
    /// The coverage overlay and the AI region; see [`OverlayState`].
    pub(crate) overlay: OverlayState,
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
    pub(crate) genai: GenerativeState,
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
    pub(crate) view: ViewState,
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
    pub(crate) crop: CropState,
    // (handle, drag start, crop at start, straighten° at start). Handles:
    // 0-3 corners, 4 move-inside, 5-8 edge midpoints (T/B/L/R), 9 = drag
    // OUTSIDE the box → rotate-straighten (the LR gesture).
    // --- manual local adjustments (masks) ---
    /// Which of the selected mask's COMPONENTS the canvas tools target:
    /// `None` = the base geometry. Reset whenever the mask selection moves.
    /// The combine mode the panel's "add shape" buttons will arm next.
    // --- tone-curve editor ---
    pub(crate) curve_channel: usize,                  // CURVE_CHANNELS index: 0=master 1=R 2=G 3=B
    // The control point being dragged, WITH the curve set it belongs to
    // (R25 P6: the global curves and a selected mask's can both be on screen
    // in one frame — see `CurveTarget`). Active channel is `curve_channel`.
    pub(crate) curve_drag: Option<(CurveTarget, usize)>,
    /// Layout/behaviour probes for the window tests; see [`TestSeams`].
    #[cfg(test)]
    pub(crate) seams: TestSeams,
    // --- batch recipe copy / paste ---
    // WHICH photo the clipboard came from: pasting back onto that photo may
    // keep its bitmap masks (the rasters are its own); every other target
    // gets them stripped.
    // --- recipe library (named develops, this file's siblings under <store>/recipes) ---
    /// The named-recipe library panel; see [`RecipeLibraryState`].
    pub(crate) rlib: RecipeLibraryState,
    /// The selected library recipe developed on the open photo (the window's
    /// preview pane); rebuilt when the selection, the photo or the file changes.
    // --- WB eyedropper ---
    /// The canvas tools and AI/generation flight flags; see [`ToolsState`].
    pub(crate) tools: ToolsState,
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
    pub(crate) variants: Vec<Variant>,
    /// The user answered "Discard & quit". The close guard must then stop
    /// re-testing unsaved state: discarding by definition leaves the state
    /// dirty, so without this the guard cancelled the close and re-raised
    /// the dialog every frame and the app could not be quit at all. (The
    /// SAVE path needs no such flag since v0.22 — Save-all persists each
    /// photo's whole strip to variants.json, so the way-out re-check finds
    /// the state genuinely clean.)
    pub(crate) active: usize,                         // index into `variants` (always valid once a photo is open)
    /// The card strip's dialog/one-shot state; see [`CardsState`].
    pub(crate) cards: CardsState,
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
    pub(crate) normal: crate::panels::normal::NormalState,
    /// The last line uv printed during the managed Python environment's
    /// install (`panels/settings.rs`).
    /// What the sidecar interpreter was found to be at startup
    /// (`python_env::inspect`); `None` until the first check.
    /// The Portraits panel's own state (`panels/portrait.rs`).
    pub(crate) portrait: crate::panels::portrait::PortraitState,
}

impl AutoShadeApp {
    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    // pub(crate): the zoom-keys test drives a real headless frame through it.
    pub(crate) fn upd_shortcuts(&mut self, ctx: &egui::Context) {
        // Tab's egui focus traversal fired last frame despite the consume (see
        // `defocus_next`): drop that focus now so the panel toggle doesn't
        // leave a surprise-focused widget eating every later shortcut.
        if std::mem::take(&mut self.ui.defocus_next)
            && let Some(id) = ctx.memory(|m| m.focused())
        {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
        // Global shortcuts, in THREE tiers (all skipped while a modal layer
        // is up — the quit confirm, where no key must mutate the very state
        // the user is deciding whether to save, or the fit prompt, whose
        // answer is about the card under it; the Settings / ⌨ windows are
        // keyboard-modal too, D13):
        //  * Esc while a transient window is up dismisses it.
        //  * Ctrl+O/E/S have no text-editing meaning, so they work even while
        //    a widget holds focus — focusing a prompt used to silently kill
        //    Save/Open/Export until the user clicked elsewhere.
        //  * Everything else (plain letters, arrows, Ctrl+Z/Y) stays gated on
        //    "no widget focused": typing must type, and a text field's own
        //    undo owns Ctrl+Z there.
        // Ctrl+Z/Y = undo/redo, Ctrl+O = open, Ctrl(+Shift)+E = export,
        // Ctrl+S = save XMP, ←/→ = walk the gallery (crop armed: nudge the
        // box), R = crop, Enter = commit crop, W/Q/K/M = LR tool keys,
        // \ / Y = compare, [ ] = brush size, Tab = hide the side panels —
        // the keyboard grammar of every desktop photo editor.
        let transient_open = self.ui.show_settings || self.ui.show_shortcuts;
        if !self.modal_up() && transient_open {
            let sheet_open = self.ui.show_shortcuts;
            let (esc, sheet_key) = ctx.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::Escape),
                    // F1 / ? stay live while the sheet is open — they are its
                    // advertised toggle, and the focused-none tier below is
                    // skipped entirely while a transient is up (L19).
                    sheet_open
                        && (i.consume_key(egui::Modifiers::NONE, egui::Key::F1)
                            || i.consume_key(egui::Modifiers::NONE, egui::Key::Questionmark)
                            || i.consume_key(egui::Modifiers::SHIFT, egui::Key::Questionmark)),
                )
            });
            if esc || sheet_key {
                // Cheat-sheet first — it is the topmost transient.
                if self.ui.show_shortcuts {
                    self.ui.show_shortcuts = false;
                } else {
                    self.ui.show_settings = false;
                }
            }
        }
        if !self.modal_up() && !transient_open {
            let (mut do_open, mut do_export, mut do_xmp) = (false, false, false);
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::O) { do_open = true; }
                // LR's export key is Ctrl+Shift+E — consumed FIRST (the
                // exact-match consume would leave it dead behind plain
                // Ctrl+E, which stays as the historical alias, D13).
                if i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::E) { do_export = true; }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::E) { do_export = true; }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::S) { do_xmp = true; }
            });
            if do_open && !self.busy {
                self.gal.selected = None;
                self.apply_actions(vec![logic::Action::Gallery(
                    logic::gallery::GalleryAction::OpenPhotoFile,
                )]);
            }
            if do_export && self.src_path.is_some() && !self.busy {
                self.start_export();
            }
            if do_xmp && self.src_path.is_some() && !self.busy {
                // The pending-rename flush lives at save_xmp's head now (it
                // fires while a TextEdit holds focus either way) — the Save
                // develop BUTTON takes the same entry point (U10).
                self.save_xmp();
            }
        }
        if !self.modal_up() && !transient_open && ctx.memory(|m| m.focused()).is_none() {
            let (mut do_undo, mut do_redo) = (false, false);
            let (mut do_escape, mut do_overlay, mut do_clip) = (false, false, false);
            let (mut do_cheatsheet, mut do_crop, mut do_panels) = (false, false, false);
            let (mut do_crop_commit, mut do_crop_grid) = (false, false);
            let (mut do_wb, mut do_heal, mut do_brush) = (false, false, false);
            let (mut do_linear, mut do_radial) = (false, false);
            let (mut do_before, mut do_compare) = (false, false);
            let (mut do_fit, mut do_one) = (false, false);
            let (mut do_copy, mut do_paste) = (false, false);
            let mut zoom_key: f32 = 0.0;
            let mut crop_nudge = (0.0f32, 0.0f32);
            let mut nav: i32 = 0;
            let mut brush_delta: f32 = 0.0;
            // [ / ] belong to the active brush tool; when none is armed the
            // keys stay unconsumed (free for egui / future bindings).
            let brush_tool = self.masks.paint_mode || self.tools.clone_mode;
            let crop_tool = self.crop.mode;
            // ↑/↓ walk the library ONLY while the pointer is off the controls
            // panel: over it, the same keys nudge a hovered slider — and this
            // handler runs FIRST each frame, so consuming here would eat the
            // nudge before the slider ever saw the key. Last frame's rect
            // (panels draw after the shortcuts) — one frame stale, harmless.
            let over_controls = ctx
                .input(|i| i.pointer.latest_pos())
                .zip(self.view.controls_rect)
                .is_some_and(|(p, r)| r.contains(p));
            ctx.input_mut(|i| {
                if i.consume_key(egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z) { do_redo = true; }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Y) { do_redo = true; }
                if i.consume_key(egui::Modifiers::COMMAND, egui::Key::Z) { do_undo = true; }
                if crop_tool {
                    // Crop owns the arrows while armed (keyboard-only
                    // geometry, D13); gallery walking resumes on exit.
                    let s = if i.modifiers.shift { 10.0 } else { 1.0 };
                    for (key, dx, dy) in [
                        (egui::Key::ArrowRight, 1.0, 0.0),
                        (egui::Key::ArrowLeft, -1.0, 0.0),
                        (egui::Key::ArrowDown, 0.0, 1.0),
                        (egui::Key::ArrowUp, 0.0, -1.0),
                    ] {
                        if i.consume_key(egui::Modifiers::NONE, key)
                            || i.consume_key(egui::Modifiers::SHIFT, key)
                        {
                            crop_nudge.0 += dx * s;
                            crop_nudge.1 += dy * s;
                        }
                    }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) { do_crop_commit = true; }
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight) { nav = 1; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft) { nav = -1; }
                // ↑/↓: the vertical twins (a vertical list wants vertical
                // keys) — gated on `over_controls` above.
                if !over_controls {
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown) { nav = 1; }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp) { nav = -1; }
                }
                // Zoom keys — the canvas Fit / 1:1 buttons' twins. `=` is
                // unshifted `+` on ANSI layouts; Shift++ covers the rest.
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Plus)
                    || i.consume_key(egui::Modifiers::SHIFT, egui::Key::Plus)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Equals)
                {
                    zoom_key = 1.0;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Minus) { zoom_key = -1.0; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Num0) { do_fit = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Num1) { do_one = true; }
                // Ctrl+Shift+C/V (LR's Copy/Paste Develop Settings):
                // keyboard twins of ⎘ Copy recipe / ⇩ Paste to selected.
                // EVENT-level, not consume_key: egui-winit swallows every
                // Ctrl(+Shift)+C into Event::Copy and emits NO Key event
                // (egui-winit lib.rs is_copy_command — the Codex 阶段5
                // review caught the Key-based version as unreachable on
                // real input). Shift is the discriminator: a plain Ctrl+C
                // keeps egui's own selected-text copy, the Shift chord is
                // ours and the event is removed so nothing double-fires.
                if i.modifiers.command && i.modifiers.shift {
                    let n = i.events.len();
                    i.events.retain(|e| !matches!(e, egui::Event::Copy));
                    do_copy |= i.events.len() < n;
                    let n = i.events.len();
                    i.events.retain(|e| !matches!(e, egui::Event::Paste(_)));
                    do_paste |= i.events.len() < n;
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) { do_escape = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::O) {
                    if crop_tool { do_crop_grid = true; } else { do_overlay = true; }
                }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::J) { do_clip = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::R) { do_crop = true; }
                // LR tool keys (D13): W / Q / K and M / Shift+M (the Shift
                // variant consumed first — exact match leaves it dead after).
                if i.consume_key(egui::Modifiers::NONE, egui::Key::W) { do_wb = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Q) { do_heal = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::K) { do_brush = true; }
                if i.consume_key(egui::Modifiers::SHIFT, egui::Key::M) { do_radial = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::M) { do_linear = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Backslash) { do_before = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Y) { do_compare = true; }
                if i.consume_key(egui::Modifiers::NONE, egui::Key::Tab) { do_panels = true; }
                if brush_tool {
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::OpenBracket) { brush_delta = -4.0; }
                    if i.consume_key(egui::Modifiers::NONE, egui::Key::CloseBracket) { brush_delta = 4.0; }
                }
                // F1 / ? — the cheat-sheet (Shift+/ produces ? on most layouts).
                if i.consume_key(egui::Modifiers::NONE, egui::Key::F1)
                    || i.consume_key(egui::Modifiers::NONE, egui::Key::Questionmark)
                    || i.consume_key(egui::Modifiers::SHIFT, egui::Key::Questionmark)
                {
                    do_cheatsheet = true;
                }
            });
            // Mirror the ↶/↷ BUTTONS exactly — `add_enabled(ready &&
            // !stack.is_empty())` with `ready = src_path.is_some() && !busy`.
            // Un-gated, Ctrl+Z during a retouch swapped the base the landing
            // result is applied over and, since every pixel undo now writes a
            // status, wiped the only liveness the bar was showing for a
            // worker that sends no progress. But the BUSY half alone made the
            // refusal lie: Ctrl+Z during a folder scan with no photo open
            // drew "busy — …unlock when the current task finishes" for an
            // unlock that would never come (no photo, no history). Only a
            // press that WOULD have acted earns a refusal; the rest stay the
            // silent no-ops the disabled buttons are.
            let armed_undo = do_undo && self.src_path.is_some() && !self.hist.undo_stack.is_empty();
            let armed_redo = do_redo && self.src_path.is_some() && !self.hist.redo_stack.is_empty();
            if self.busy {
                if armed_undo || armed_redo {
                    let t = tr(
                        self.lang,
                        "busy — undo and redo unlock when the current task finishes",
                    );
                    self.toast(ToastKind::Error, t);
                }
            } else {
                if armed_undo { self.undo(ctx); }
                if armed_redo { self.redo(ctx); }
            }
            // R mirrors the crop button exactly (incl. the one-tool-at-a-time
            // disarms); no photo → nothing to crop.
            if do_crop && self.src_path.is_some() {
                let on = !self.crop.mode;
                self.disarm_tools();
                self.set_crop_mode(on);
            }
            // Enter commits the crop (LR, D13): the box is already live in
            // recipe.crop — committing is dropping the grab and disarming.
            if do_crop_commit {
                self.crop.drag = None;
                self.set_crop_mode(false);
            }
            if crop_nudge != (0.0, 0.0)
                && let Some(c) = self.recipe.crop
            {
                // Keyboard twin of the move drag (D13): 0.5% of the frame per
                // press (Shift ×10 via the multiplier above), clamped
                // in-frame; a full-frame box has no room and stays put — and
                // NO box at all is a no-op: materialising an identity crop
                // from None lit a false ● and stamped HasCrop into the next
                // XMP (M19).
                let c0 = [c.left, c.top, c.right, c.bottom];
                let (w, h) = (c0[2] - c0[0], c0[3] - c0[1]);
                let nl = (c0[0] + crop_nudge.0 * 0.005).clamp(0.0, 1.0 - w);
                let nt = (c0[1] + crop_nudge.1 * 0.005).clamp(0.0, 1.0 - h);
                let next = Some(autoshade::recipe::Crop {
                    left: nl,
                    top: nt,
                    right: nl + w,
                    bottom: nt + h,
                });
                if self.recipe.crop != next {
                    self.recipe.crop = next;
                    self.dirty = true; // histogram/clipping follow the crop
                }
            }
            if do_crop_grid {
                self.crop.grid = (self.crop.grid + 1) % 3;
            }
            // LR tool keys: each mirrors its own button exactly
            // (disarm-then-arm; a second press exits the tool).
            if do_wb && self.src_path.is_some() {
                let on = !self.tools.wb_picking;
                self.disarm_tools();
                self.tools.wb_picking = on;
                if on {
                    self.status = tr(self.lang, "WB eyedropper: click a spot that should be neutral grey/white").into();
                }
            }
            if do_heal && self.src_path.is_some() {
                let on = !self.tools.clone_mode;
                self.disarm_tools();
                self.tools.clone_mode = on;
            }
            if do_brush && self.src_path.is_some() {
                self.toggle_brush_key();
            }
            if do_linear && self.src_path.is_some() {
                let armed = matches!(self.masks.placing_mask, Some((MaskKind::Linear, PlaceTarget::NewMask)));
                self.disarm_tools();
                if !armed {
                    self.masks.placing_mask = Some((MaskKind::Linear, PlaceTarget::NewMask));
                    self.status = tr(self.lang, "Drag on the image to draw a linear gradient (start fully applied → end unaffected; Shift = axis lock)").into();
                }
            }
            if do_radial && self.src_path.is_some() {
                let armed = matches!(self.masks.placing_mask, Some((MaskKind::Radial, PlaceTarget::NewMask)));
                self.disarm_tools();
                if !armed {
                    self.masks.placing_mask = Some((MaskKind::Radial, PlaceTarget::NewMask));
                    self.status = tr(self.lang, "Drag on the image to draw a radial (elliptical) area").into();
                }
            }
            // \ latches the Before view, Y flips the comparison layout (LR).
            if do_before {
                self.view.before = !self.view.before;
            }
            if do_compare {
                self.view.mode = if self.view.mode == ViewMode::SideBySide {
                    ViewMode::AfterOnly
                } else {
                    ViewMode::SideBySide
                };
            }
            if do_panels {
                self.ui.panels_hidden = !self.ui.panels_hidden;
                self.ui.defocus_next = true; // undo egui's Tab focus traversal next frame
            }
            if brush_delta != 0.0 {
                // Proportional, as Lightroom's [ / ]: a fixed 4 px step was
                // fifty presses across the range.
                let now = ctx.input(|i| i.time);
                let factor = if brush_delta > 0.0 { 1.15 } else { 1.0 / 1.15 };
                self.set_brush((self.masks.brush * factor).round(), now);
            }
            if do_cheatsheet {
                self.ui.show_shortcuts = !self.ui.show_shortcuts;
            }
            // Esc leaves whatever on-image tool is active (the universal
            // editor exit) — the transient windows own their Esc in the
            // pre-tier above; painted canvases/samples stay for resuming.
            if do_escape
                && (self.tool_armed() || self.overlay.region_drag.is_some() || self.view.before)
            {
                // disarm_tools also kills the transient gesture anchors — a
                // grab or stroke surviving Esc used to hijack the next drag.
                // The \-latched Before view exits here too: it locks every
                // tool out, and Esc is the universal way back (M21).
                self.disarm_tools();
                self.overlay.region_drag = None;
                self.view.before = false;
                self.status = tr(self.lang, "Exited the current tool (Esc)").into();
            }
            if do_overlay {
                self.overlay.show_mask_overlay = !self.overlay.show_mask_overlay;
                self.overlay.overlay_stale = true;
            }
            if do_clip {
                // Instant: rebuilt from the retained frame, no redevelop (J).
                self.toggle_clipping(ctx);
            }
            if nav != 0 && !self.busy && !self.gal.gallery.is_empty() {
                // Nothing selected yet: either arrow ENTERS the gallery at its
                // first photo (the old Right-arrow default skipped index 0).
                let next = match self.gal.selected {
                    Some(i) => (i as i32 + nav).clamp(0, self.gal.gallery.len() as i32 - 1),
                    None => 0,
                };
                if Some(next as usize) != self.gal.selected {
                    // Keyboard walking must keep the highlight on screen — the
                    // gallery scrolls to it next frame (clicks don't set this:
                    // a clicked row is visible by definition).
                    self.gal.gallery_scroll_to = Some(next as usize);
                    self.open_gallery_index(next as usize);
                }
            }
            // Zoom keys act on the open photo's canvas. Fit/1:1 mirror their
            // buttons exactly (1:1 via the per-frame zoom_one_to_one twin);
            // +/− step about the view centre — pan holds, view_uv reclamps.
            // All three set the TARGET only: the glide in update() carries
            // `zoom` there, and +/− compound on the target so a key roll
            // accumulates instead of fighting the animation.
            if self.src_path.is_some() {
                if do_fit {
                    self.view.zoom_target = 1.0;
                    self.view.pan = egui::vec2(0.5, 0.5);
                }
                if do_one {
                    self.view.zoom_target = self.view.one_to_one.clamp(1.0, 12.0);
                }
                if zoom_key != 0.0 {
                    self.view.zoom_target =
                        (self.view.zoom_target * 1.25f32.powi(zoom_key as i32)).clamp(1.0, 12.0);
                }
            }
            // Ctrl+C mirrors ⎘ Copy recipe (same guards + the pending-rename
            // flush); Ctrl+V mirrors ⇩ Paste to selected — and like the
            // disabled buttons, a press that would not act stays silent.
            if do_copy && self.src_path.is_some() && !self.busy {
                // Every name box at once (the 10+1 boundary rule): a name
                // typed into a mask row, a version row or a variant card and
                // never blurred still lands before anything else in the app
                // acts.
                self.commit_pending_names();
                self.genai.copied = Some(self.recipe.clone());
                self.genai.copied_from = self.src_path.clone();
                self.status = tr(
                    self.lang,
                    "Recipe copied — Ctrl/⌘+click to pick several, then “Paste to selected”",
                )
                .to_string();
            }
            if do_paste && !self.busy && self.genai.copied.is_some() && !self.gal.multi_sel.is_empty() {
                self.start_paste();
            }
        }

        // Drag & drop: dropping a photo opens it, a folder opens the library.
        // Ignored entirely while the quit-confirm layer is up — a drop must
        // not mutate the very state the user is deciding whether to save
        // (the shortcut block is gated on the same condition).
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    fn upd_dropped_and_title(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = if self.modal_up() {
            Vec::new()
        } else {
            ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect())
        };
        let n_dropped = dropped.len();
        if let Some(p) = dropped.into_iter().next() {
            if self.busy {
                self.toast(
                    ToastKind::Error,
                    tr(self.lang, "busy — wait for the current task to finish before opening"),
                );
            } else if p.is_dir() {
                self.open_folder(p);
                if n_dropped > 1 {
                    // EVERY branch discloses the leftovers — silently eating
                    // them read as "it handled them all" (the photo branch
                    // alone used to say so).
                    self.toast(
                        ToastKind::Error,
                        trf(
                            self.lang,
                            "opened the first folder — {n} more dropped item(s) ignored",
                            &[("n", &(n_dropped - 1).to_string())],
                        ),
                    );
                }
            } else if is_photo_path(&p) {
                self.gal.selected = None;
                self.open_path(p);
                if n_dropped > 1 {
                    // Silently eating the rest read as "it opened them all".
                    self.toast(
                        ToastKind::Error,
                        trf(
                            self.lang,
                            "opened the first photo — {n} more ignored (drop their folder to browse them all)",
                            &[("n", &(n_dropped - 1).to_string())],
                        ),
                    );
                }
            } else {
                let msg = if n_dropped > 1 {
                    trf(
                        self.lang,
                        "unsupported file type: {path} — {n} more dropped item(s) ignored",
                        &[
                            ("path", &p.display().to_string()),
                            ("n", &(n_dropped - 1).to_string()),
                        ],
                    )
                } else {
                    trf(self.lang, "unsupported file type: {path}", &[("path", &p.display().to_string())])
                };
                self.toast(ToastKind::Error, msg);
            }
        }

        // Window title mirrors the open photo (send only on change).
        let title = match &self.src_path {
            Some(p) => format!(
                "{} — AutoShade",
                p.file_name().and_then(|s| s.to_str()).unwrap_or("photo")
            ),
            None => "AutoShade".to_string(),
        };
        if title != self.chrome.last_title {
            ctx.send_viewport_cmd(egui::ViewportCommand::Title(title.clone()));
            self.chrome.last_title = title;
        }
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_top_bar(&mut self, ctx: &egui::Context) {
        // egui's side_top_panel frame keeps only 2px of vertical margin —
        // at that spacing the action row visually touches the strip edges.
        window_chrome::edge_resize(ctx);
        window_chrome::window_outline(ctx);
        let frame = egui::Frame::side_top_panel(&ctx.style())
            .inner_margin(egui::Margin::symmetric(8, SPACE_MD.round() as i8))
            .fill(self.ui.theme.colors().bar_fill);
        egui::TopBottomPanel::top("top").frame(frame).show(ctx, |ui| {
            let lang = self.lang;
            // ONE wrapped row of ACTIONS only (UX batch): settings that used
            // to live here moved to where they are used — export options into
            // the Develop panel's Export section, the AI direction/refine/
            // style trio into the AI section. The row wraps, never clips (the
            // "shrink the window and lose the buttons" bug); wrapping only
            // works between ATOMIC widget allocations, so the enabled gating
            // stays per-widget (ui.add_enabled), and groups are fenced with
            // add_space (a separator that wraps to a line start orphans).
            // AI Analyze moved INTO the AI section, onto the same row as
            // the Direction prompt it consumes (user feedback: a trigger
            // stranded in the toolbar, far from its input, reads as
            // unrelated — every prompt entry now carries its own button).
            let ready = self.src_path.is_some() && !self.busy;
            // R38: the row wraps only BETWEEN its groups — each group is one
            // measured allocation (`buttons::group`, sized from its labels),
            // so a narrow window moves a whole group to the next line instead
            // of splitting Undo from Redo; a plain `ui.horizontal` would be
            // laid at the cursor at whatever width was left and never wrap.
            // The group fence sits INSIDE each group's end, where a wrapped
            // line start cannot orphan it. Export is the row's one primary
            // verb; Settings and the shortcut sheet are glyph squares.
            // On Windows the row is also the title bar (`window_chrome`).
            window_chrome::toolbar_row(ui, lang, |ui| {
                group(ui, &[tr(lang, "Open photo…"), tr(lang, "Reset")], 0, |ui| {
                    if action(ui, !self.busy, tr(lang, "Open photo…"))
                        .on_hover_text(tr(lang, "Ctrl+O · or drag a file into the window"))
                        .clicked()
                    {
                        self.gal.selected = None; // a one-off file isn't a gallery selection
                        self.apply_actions(vec![logic::Action::Gallery(
                            logic::gallery::GalleryAction::OpenPhotoFile,
                        )]);
                    }
                    if action(ui, ready, tr(lang, "Reset"))
                        .on_hover_text(tr(lang, "Back to this photo's fresh-open look: sliders neutral on the camera's own default colour and tone, like the in-camera JPEG (one undo brings it back)"))
                        .clicked()
                    {
                        // Reset = the fresh-open look: sliders neutral on this
                        // photo's camera-matched base. RE-STAMP the open knots
                        // rather than keeping the canvas curve — a legacy save
                        // deliberately carries none, and preserving that emptiness
                        // made Reset stay dark on every previously-edited photo.
                        // Only a GENERATED variant keeps no curve (its pixels
                        // carry the look); an InPlace retouch master is a NEUTRAL
                        // develop that still needs the calibration on top —
                        // stripping there turned Reset into "go dark".
                        let generated = self.active_on_ai_pixels();
                        // The fresh START (v1.17.0): the canvas profile and
                        // Look, or the camera curve where they cannot render.
                        let start =
                            if generated { Default::default() } else { self.open.photo_start.clone() };
                        // The lens profile is the same kind of calibration: Reset
                        // re-stamps the photo's own (a Generated master's pixels
                        // already carry the corrections — none re-applied).
                        let lens_profile =
                            if generated { Default::default() } else { self.open.photo_lens.clone() };
                        // The as-shot anchor follows the SAME generated strip:
                        // baked pixels carry their WB, so an absolute-Kelvin
                        // claim over them would be false.
                        let (as_shot_k, as_shot_tint) = match self.open.photo_as_shot {
                            Some((k, t)) if !generated => (Some(k), Some(t)),
                            _ => (None, None),
                        };
                        let mut fresh = EditRecipe {
                            lens_profile,
                            as_shot_k,
                            as_shot_tint,
                            ..EditRecipe::default()
                        };
                        let start: autoshade::pipeline::CanvasStart = start;
                        start.stamp(&mut fresh);
                        self.recipe = fresh;
                        self.overlay.region = None;
                        self.dirty = true;
                        // The old AI rationale/verdict describe the recipe Reset
                        // just discarded — showing them under neutral sliders lies.
                        self.resync_recipe_display();
                    }
                });
                group(ui, &[tr(lang, "↶ Undo"), tr(lang, "↷ Redo")], 0, |ui| {
                    if action(ui, ready && !self.hist.undo_stack.is_empty(), tr(lang, "↶ Undo"))
                        .on_hover_text(tr(lang, "Ctrl+Z · undo the last edit"))
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.undo(&ctx);
                    }
                    if action(ui, ready && !self.hist.redo_stack.is_empty(), tr(lang, "↷ Redo"))
                        .on_hover_text(tr(lang, "Ctrl+Y · redo the undone edit"))
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.redo(&ctx);
                    }
                });
                group(ui, &[tr(lang, "⭯ Turn left"), tr(lang, "⭮ Turn right")], 0, |ui| {
                    // Rotate (R27). NOT ↶/↷ — those two glyphs are Undo/Redo one
                    // button to the left, and ↺/↻ are already 「reset all」/
                    // 「Clear」/「↻ Redraw」 in the panels. ⭯/⭮ (U+2B6F/U+2B6E) are
                    // unused anywhere in the tree and both ship in
                    // NotoSansSymbols2-autoshade.ttf, so no subset regeneration.
                    let can_rotate = self.can_rotate();
                    // Disabled buttons state their reason (the pixel-state rule):
                    // a control that greys out in silence reads as a broken app.
                    // Spelled at each site rather than through a helper — the
                    // i18n audit reads `tr()` LEXICALLY, and a key that reaches it
                    // through a closure parameter is a key it cannot check.
                    let blocked = tr(
                        lang,
                        "rotation is off while this photo carries rendered pixels (retouch / AI rendition) or a tool is armed — its master raster is a file in the frame it was rendered in; turn first, retouch after",
                    );
                    let left_hover = if can_rotate {
                        tr(lang, "A quarter turn anticlockwise — the crop and every mask turn with it (one undo)")
                    } else {
                        blocked
                    };
                    if action(ui, can_rotate, tr(lang, "⭯ Turn left"))
                        .on_hover_text(left_hover)
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.rotate_photo(3, &ctx); // anticlockwise = three clockwise
                    }
                    let right_hover = if can_rotate {
                        tr(lang, "A quarter turn clockwise — the crop and every mask turn with it (one undo)")
                    } else {
                        blocked
                    };
                    if action(ui, can_rotate, tr(lang, "⭮ Turn right"))
                        .on_hover_text(right_hover)
                        .clicked()
                    {
                        let ctx = ui.ctx().clone();
                        self.rotate_photo(1, &ctx);
                    }
                });
                group(ui, &[tr(lang, "◫ Compare"), tr(lang, "⬛ Single")], 0, |ui| {
                    // View mode: side-by-side vs a full-width edit (hold B =
                    // compare). ◫ lives in egui's bundled fonts — the old ⿲
                    // (U+2FF2) only rendered when the OPTIONAL CJK fallback font
                    // loaded, tofu otherwise.
                    ui.selectable_value(&mut self.view.mode, ViewMode::SideBySide, tr(lang, "◫ Compare"))
                        .on_hover_text(tr(lang, "Before/After side by side"));
                    ui.selectable_value(&mut self.view.mode, ViewMode::AfterOnly, tr(lang, "⬛ Single"))
                        .on_hover_text(tr(lang, "The edit fills the canvas; hold B to quickly compare the original"));
                });
                group(ui, &[tr(lang, "Export"), "▾", tr(lang, "Save develop")], 0, |ui| {
                    // Delivery ACTIONS (their settings live in the Develop panel's
                    // Export section; the hover echoes the current delivery state
                    // so it stays glanceable without a toolbar row of combos).
                    //
                    // ONE split button (R22-7). 「Export」 and 「Download…」 were the
                    // same code path — start_render_to — differing only in where
                    // the target came from, which made "where do my files go" a
                    // property of WHICH BUTTON you pressed instead of a setting.
                    // The main half now follows the Destination setting; the ▾ half
                    // is the one-off escape hatch that leaves the setting alone.
                    // Zero item_spacing joins the two halves visually into one
                    // control (egui has no split-button widget).
                    let summary = self.export_summary(lang);
                    ui.scope(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        if primary(ui, ready, tr(lang, "Export"))
                            .on_hover_text(format!(
                                "{}\n{summary}",
                                tr(lang, "Ctrl+Shift+E (or Ctrl+E) · full-resolution render to the Destination below (follows the current variant's pixels); Destination + settings live in the Export section")
                            ))
                            .clicked()
                        {
                            self.start_export();
                        }
                        // `add_enabled_ui`, not a disabled Button: menu_button
                        // returns an InnerResponse and takes no enabled flag.
                        ui.add_enabled_ui(ready, |ui| {
                            ui.menu_button("▾", |ui| {
                                if ui
                                    .button(tr(lang, "Export to…"))
                                    .on_hover_text(tr(lang, "Pick a path for THIS export only — the Destination setting is left as it is"))
                                    .clicked()
                                {
                                    ui.close();
                                    self.export_to_chosen_path();
                                }
                            })
                            .response
                            .on_hover_text(tr(lang, "Export to a one-off path…"));
                        });
                    });
                    if action(ui, ready, tr(lang, "Save develop"))
                        .on_hover_text(tr(lang, "Ctrl+S · save this photo's develop (recipe + a Lightroom/ACR XMP for RAW; a baked retouch master is linked so reopening restores it) to your develop store"))
                        .clicked()
                    {
                        self.save_xmp();
                    }
                });
                group(ui, &[], 2, |ui| {
                    if glyph(ui, true, "⚙").on_hover_text(tr(lang, "Settings · AI provider / model / API key")).clicked() {
                        // Reload the form only on the closed→open edge — reloading
                        // while open wiped everything already typed (incl. keys).
                        // Toggle semantics, matching ⌨ next door.
                        if !self.ui.show_settings {
                            self.load_settings_form();
                        }
                        self.ui.show_settings = !self.ui.show_settings;
                    }
                    if glyph(ui, true, "⌨").on_hover_text(tr(lang, "Keyboard shortcuts (F1 / ?)")).clicked() {
                        self.ui.show_shortcuts = !self.ui.show_shortcuts;
                    }
                });
            });
        });
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_status_bar(&mut self, ctx: &egui::Context) {
        let frame = egui::Frame::side_top_panel(&ctx.style()).fill(self.ui.theme.colors().bar_fill);
        egui::TopBottomPanel::bottom("status").frame(frame).show(ctx, |ui| {
            ui.horizontal(|ui| {
                if self.busy {
                    ui.spinner();
                    // Armed by every cancellable long-run: retouch/generative
                    // workers (which poll the flag and really stop), local
                    // compute, and AI analyze — the last two are ABANDON-only,
                    // which the hover text says rather than promising a
                    // checkpoint they do not have.
                    if self.tools.gen_cancel.is_some()
                        && action(ui, true, tr(self.lang, "✕ Cancel"))
                            .on_hover_text(tr(
                                self.lang,
                                "Stop waiting: the app unblocks now and the late result is discarded. A generative call halts at its next checkpoint; an AI analyze keeps running (and billing) until it finishes or times out",
                            ))
                            .clicked()
                    {
                        self.cancel_generative();
                    }
                }
                // The unsaved marker: canvas differs from the saved develop
                // (base_curve excluded — calibration is not an edit, dirty_vs)
                // OR its pixels differ from that same card's recorded master.
                // `active_baseline` resolves both halves after card switches;
                // an unsaved heal is work even under an untouched recipe. It sits FIRST
                // — before the batch bar and the status text — so neither can
                // ever push it out of view at narrow widths.
                if self.unsaved_marker_dirty() {
                    ui.label(
                        egui::RichText::new(tr(self.lang, "● unsaved"))
                            .color(ui.visuals().warn_fg_color),
                    )
                    .on_hover_text(tr(
                        self.lang,
                        "This card differs from its saved develop — Ctrl+S saves every card; switching photos keeps unsaved work for this session only",
                    ));
                }
                // Live batch-render progress, beside the spinner (its old home
                // at the START of the toolbar shifted every control right by
                // ~160px exactly while the user was waiting).
                if let Some((done, total)) = self.exp.batch_progress {
                    ui.add(
                        egui::ProgressBar::new(done as f32 / total.max(1) as f32)
                            .desired_width(150.0)
                            .text(trf(self.lang, "Batch {done}/{total}",
                                &[("done", &done.to_string()), ("total", &total.to_string())])),
                    );
                }
                // Long messages (paths, batch reports) must clip, not blow the
                // panel wide; the full text is one hover away.
                ui.add(egui::Label::new(&self.status).truncate())
                    .on_hover_text(&self.status);
            });
        });
    }

    /// Variant-strip panel height. Derivation: an 84 px card column (52 px
    /// thumb, 6 px item_spacing, and a 26 px label row — egui rows are at
    /// least interact_size.y tall), 8 px of air above and below the column
    /// (variant_strip top-pads it into center), and the panel frame's
    /// 2+2 px vertical margins. One authority — the strip geometry test
    /// lays the real panel out at this same constant and asserts the two
    /// airs are EQUAL.
    pub(crate) const VARIANT_STRIP_H: f32 = 104.0;

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    /// The controls panel's one scroll column: the AI, Develop and Retouch
    /// panels with a photo open, the empty state without.
    ///
    /// The column keeps a gutter on its right as wide as the scrollbar and a
    /// breath more (user report 2026-10-12: the Generative Fill hint ran under
    /// the bar). egui 0.29's bar FLOATS over the content and reserves no width
    /// of its own, so every row used to fill the panel to its last pixel. The
    /// gutter is a margin, not the scroll style's `floating_allocated_width`:
    /// that one reserves only while the bar shows, and a side panel sizes
    /// itself to its content, so the panel shrank a few pixels on every frame
    /// the bar faded in.
    pub(crate) fn controls_column(&mut self, ui: &mut egui::Ui) {
        let gutter = controls_gutter(ui.style());
        egui::ScrollArea::vertical().show(ui, |ui| {
            egui::Frame::NONE
                .inner_margin(egui::Margin { right: gutter.round() as i8, ..Default::default() })
                .show(ui, |ui| {
                    if self.src_path.is_some() {
                        // The one sidebar, top to bottom: the Portraits panel
                        // (people, skin, eyes, background, `panels/portrait.rs`),
                        // then Normal's four groups (`panels/normal.rs` — the
                        // one-click look, fine-tune, noise, save), then the
                        // shared AI / Develop / Retouch panels.
                        self.portrait_panel(ui);
                        self.normal_panel(ui);
                        // The AI area is FIRST among the shared panels (R22 #4):
                        // one first-level section holding every AI develop verb
                        // — analysis, whole-image generation, reverse-fit —
                        // above the sliders it writes into. It used to be three
                        // fragments: a section inside Develop, a section in
                        // the middle of Retouch, and a switch in Settings.
                        self.ai_panel(ui);
                        self.develop_panel(ui);
                        self.retouch_panel(ui);
                    } else {
                        // Empty state with the same voice as the canvas hero:
                        // centered, quiet, and it says what to do next — a
                        // bare left-aligned label read as a stuck error.
                        ui.add_space(24.0);
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new("🖼").size(26.0).weak());
                            ui.add_space(SPACE_SM);
                            ui.label(egui::RichText::new(tr(self.lang, "No photo open.")).weak());
                            ui.label(
                                egui::RichText::new(tr(
                                    self.lang,
                                    "Click a thumbnail in the Library, or press Ctrl+O.",
                                ))
                                .weak()
                                .small(),
                            );
                            ui.add_space(SPACE_MD);
                            // Round-13 easter egg — quiet enough to miss,
                            // which is the point of an easter egg.
                            ui.label(
                                egui::RichText::new(tr(
                                    self.lang,
                                    "Fun fact: this empty state finished loading while Photoshop's splash screen would still be painting its clouds.",
                                ))
                                .weak()
                                .small()
                                .italics(),
                            );
                        });
                    }
                });
        });
    }

    pub(crate) fn upd_strips_and_side_panels(&mut self, ctx: &egui::Context) {
        // Variant strip — sits directly above the status bar (registered after
        // it so it stacks on top), only when a photo is open. The selector for
        // 原片 / AI 生成 / 反推 renditions.
        if self.src_path.is_some() {
            egui::TopBottomPanel::bottom("variants")
                // At the old 96 the cards' slack all pooled below the labels
                // — height and its derivation live on VARIANT_STRIP_H.
                .exact_height(Self::VARIANT_STRIP_H)
                .show(ctx, |ui| {
                    self.variant_strip(ui);
                });
        }

        // Left-most: the library gallery (folder browse + thumbnails), then
        // the develop controls. Tab collapses both for an edge-to-edge canvas
        // (LR's panel-hiding grammar); all state lives on, only layout skips.
        if !self.ui.panels_hidden {
            egui::SidePanel::left("gallery").default_width(240.0).show(ctx, |ui| {
                self.gallery_panel(ui);
            });

            let controls = controls_side_panel(ctx, |ui| self.controls_column(ui));
            // ↑/↓ routing (see upd_shortcuts): over this panel the keys
            // belong to a hovered slider; anywhere else they walk the
            // library. One frame stale — panels draw after the shortcuts.
            self.view.controls_rect = Some(controls.response.rect);
        } else {
            self.view.controls_rect = None;
        }

        // Re-develop AFTER the controls are read (so this frame reflects edits).
        // The preview build runs on a SINGLE background worker (latest wins), so
        // egui's update loop never blocks on it — the old synchronous path froze
        // the UI for the whole develop (100-300 ms at 2560/4096, and 0.6-1.2 s
        // once a v0.8 zoned colour mask was present). While a frame is in flight,
        // further edits only set `dirty`; the completion handler discards a stale
        // frame and this block re-dispatches the newest recipe next tick, so
        // fast drags coalesce to the worker's throughput without a render storm.
        // The Before pane mirrors the canvas recipe's base_curve (calibration,
        // not an edit — the ● logic ignores it, but the compare must not):
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_central(&mut self, ctx: &egui::Context) {
        // The canvas sits on a bed one step below the panel fill (recessed
        // stage / raised chrome) — keep the stock margins, swap the fill.
        let bed = egui::Frame::central_panel(&ctx.style())
            .fill(self.ui.theme.colors().canvas_bed);
        egui::CentralPanel::default().frame(bed).show(ctx, |ui| {
            // Empty state: a real landing surface instead of a blank canvas.
            if self.src_path.is_none() {
                ui.vertical_centered(|ui| {
                    ui.add_space(ui.available_height() * 0.32);
                    ui.heading("AutoShade");
                    ui.label(egui::RichText::new(tr(self.lang, "AI auto-edit · RAW processing")).weak());
                    ui.add_space(SPACE_LG);
                    ui.horizontal(|ui| {
                        // Center the button pair by padding half the leftover width.
                        let w = 300.0;
                        ui.add_space((ui.available_width() - w).max(0.0) * 0.5);
                        if primary(ui, true, tr(self.lang, "Open photo…  (Ctrl+O)")).clicked() {
                            self.apply_actions(vec![logic::Action::Gallery(
                                logic::gallery::GalleryAction::OpenPhotoFile,
                            )]);
                        }
                        if action(ui, true, tr(self.lang, "🗂 Open folder…")).clicked() {
                            self.apply_actions(vec![logic::Action::Gallery(
                                logic::gallery::GalleryAction::OpenFolder,
                            )]);
                        }
                    });
                    ui.add_space(8.0);
                    ui.label(
                        egui::RichText::new(tr(self.lang, "or drag a RAW / image straight into the window · drag & drop anywhere"))
                            .weak()
                            .small(),
                    );
                });
                return;
            }

            // Fit BOTH dimensions (max_width alone lets a portrait overflow the
            // panel). The displayed rect is what paint/box-select map against,
            // so sizing here never changes their coordinate math.
            let avail = ui.available_size() - egui::vec2(0.0, 22.0); // room for the caption row
            // Hold B to flash the source in place — the Lightroom compare
            // gesture (\ latches it until pressed again, D13). Focus-gated
            // like every other shortcut: typing a "b" into Direction must
            // not flash the Before mid-word.
            let comparing = self.view.before
                || (ctx.memory(|m| m.focused()).is_none()
                    && ctx.input(|i| i.key_down(egui::Key::B)));

            match self.view.mode {
                ViewMode::SideBySide => {
                    let half = (avail.x - 16.0) * 0.5;
                    let uv = self.view_uv(); // same window for both panes (synced zoom)
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                egui::RichText::new(tr(self.lang, "Before (source)"))
                                    .weak()
                                    .small(),
                            );
                            if let Some(t) = &self.before_tex {
                                let size = t.size_vec2();
                                let vis = egui::vec2(uv.width() * size.x, uv.height() * size.y);
                                let disp = fit_in(vis, half, avail.y);
                                let (rect, _) =
                                    ui.allocate_exact_size(disp, egui::Sense::hover());
                                ui.painter_at(rect).image(t.id(), rect, uv, egui::Color32::WHITE);
                            }
                        });
                        ui.separator();
                        ui.vertical(|ui| self.after_view(ui, half, avail.y, comparing));
                    });
                }
                ViewMode::AfterOnly => {
                    ui.vertical(|ui| self.after_view(ui, avail.x, avail.y, comparing));
                }
            }
        });
    }

    /// One update() phase — body extracted verbatim from the eframe
    /// update loop (round-12 decomposition).
    pub(crate) fn upd_sheets(&mut self, ctx: &egui::Context) {
        self.analysis_full_window(ctx);
        self.recipe_library_window(ctx);
        // Settings window (provider / model / API keys). A local `open` avoids a
        // double &mut self borrow (Window::open vs the closure that reads self).
        if self.ui.show_settings {
            let mut open = true;
            // Scroll inside the window, capped below the screen height: the
            // provider sections outgrow a small display, and without a scroll
            // area the 保存 button ends up unreachable off-screen.
            let max_h = ctx.content_rect().height() * 0.85;
            egui::Window::new(tr(self.lang, "Settings"))
                // Fixed id: the TITLE now varies with the language, and egui
                // keys window state (position, size) off the id.
                .id(egui::Id::new("settings_window"))
                .collapsible(false)
                .resizable(false)
                .default_width(480.0)
                .open(&mut open)
                .show(ctx, |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(max_h)
                        .show(ui, |ui| self.settings_ui(ui));
                });
            if !open {
                self.ui.show_settings = false;
            }
        }

        // Keyboard cheat-sheet (F1 / ? / the ⌨ toolbar button) — the full
        // shortcut + gesture map lived only in tooltips and a code comment;
        // O (mask overlay) had no visible control at all.
        if self.ui.show_shortcuts {
            let mut open = true;
            let lang = self.lang;
            let accent = self.ui.theme.colors().accent_text;
            egui::Window::new(tr(lang, "⌨ Shortcuts"))
                // Fixed id, as for Settings: the title varies with the language
                // and egui keys window state (position) off the id.
                .id(egui::Id::new("shortcuts_window"))
                .collapsible(false)
                .resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    // Runtime table (not `const`): the ZH column is resolved by
                    // `tr` at draw time. ASCII key combos carry no
                    // natural-language words, so they stay literal.
                    let rows: [(&str, &str); 33] = [
                        ("Ctrl/⌘+O", tr(lang, "Open photo")),
                        ("Ctrl/⌘+Shift+E / Ctrl/⌘+E", tr(lang, "Export to the Destination (Destination + settings in the Export section)")),
                        ("Ctrl/⌘+S", tr(lang, "Save develop (recipe + XMP for RAW)")),
                        ("Ctrl/⌘+Z / +Shift+Z / +Y", tr(lang, "Undo / Redo")),
                        ("Ctrl/⌘+Shift+C / +Shift+V", tr(lang, "Copy recipe / paste to selected")),
                        ("← / →", tr(lang, "Step through the library")),
                        ("↑ / ↓", tr(lang, "Step through the library (outside the controls panel)")),
                        ("+ / − / 0 / 1", tr(lang, "Zoom in / out / fit / 1:1")),
                        ("R", tr(lang, "Enter / exit crop")),
                        ("Enter", tr(lang, "Commit the crop (exit the tool)")),
                        ("W", tr(lang, "WB eyedropper")),
                        ("Q", tr(lang, "Retouch stamp")),
                        ("K", tr(lang, "Paint mask brush")),
                        ("M / Shift+M", tr(lang, "Linear / radial gradient")),
                        (tr(lang, "Shift (while drawing a gradient)"), tr(lang, "Lock to horizontal / vertical")),
                        ("[ / ]", tr(lang, "Brush size (paint / clone armed)")),
                        ("Tab", tr(lang, "Hide / show the side panels")),
                        (tr(lang, "Hover a slider + ↑/↓"), tr(lang, "Nudge its value (Shift ×10)")),
                        (tr(lang, "B (hold)"), tr(lang, "Compare original")),
                        ("\\", tr(lang, "Before / after (toggle)")),
                        ("Y", tr(lang, "Side-by-side ↔ single view")),
                        ("O", tr(lang, "Toggle mask overlay (crop: cycle grid)")),
                        ("J", tr(lang, "Toggle clipping warning")),
                        ("Esc", tr(lang, "Exit tool / close this window")),
                        ("F1 / ?", tr(lang, "This cheat-sheet")),
                        (tr(lang, "Scroll"), tr(lang, "Zoom (toward cursor)")),
                        (tr(lang, "Double-click canvas"), tr(lang, "Fit ↔ 1:1")),
                        (tr(lang, "Space+drag / middle-drag"), tr(lang, "Pan")),
                        (tr(lang, "Drag when zoomed"), tr(lang, "Pan (Ctrl+drag = box-select)")),
                        (tr(lang, "Alt+click"), tr(lang, "Sample clone source")),
                        (tr(lang, "Slider double-click / right-click"), tr(lang, "Reset to its default")),
                        (tr(lang, "Curve: click / drag / drag-out"), tr(lang, "Add / move / delete point")),
                        (tr(lang, "Drag a mask handle"), tr(lang, "Reshape / move the selected mask")),
                    ];
                    // Bounded + scrollable: the 30-row grid clipped its lower
                    // entries off short displays (the window is non-resizable
                    // and had no height limit, unlike the Settings window).
                    let max_h = (ctx.content_rect().height() * 0.7).max(200.0);
                    egui::ScrollArea::vertical().max_height(max_h).show(ui, |ui| {
                        egui::Grid::new("shortcut_grid").num_columns(2).striped(true).show(
                            ui,
                            |ui| {
                                for (keys, what) in rows {
                                    ui.label(egui::RichText::new(keys).monospace().color(accent));
                                    ui.label(what);
                                    ui.end_row();
                                }
                            },
                        );
                    });
                    ui.add_space(SPACE_SM);
                    // Round-13 easter egg: the pricing model, disclosed.
                    ui.label(
                        egui::RichText::new(tr(
                            lang,
                            "Every shortcut above ships free — no Creative tier, no Cloud, no monthly ransom.",
                        ))
                        .weak()
                        .small(),
                    );
                });
            if !open {
                self.ui.show_shortcuts = false;
            }
        }
    }
}

impl Default for AutoShadeApp {
    fn default() -> Self {
        let (tx, rx) = std::sync::mpsc::channel();
        Self {
            src_path: None,
            base_preview: None,
            source_preview: None,
            base_turns: 0,
            film_edge: None,
            before_tex: None,
            after_tex: None,
            canvas_fade: None,
            after_shows: None,
            recipe: EditRecipe::default(),
            dirty: false,
            develop_inflight: false,
            develop_count: 0,
            status: "Open a photo, or open a folder to browse your library.".into(),
            chrome: ChromeState::default(),
            busy: false,
            rx: Some(rx),
            tx,
            egui_ctx: egui::Context::default(),
            verdict: None,
            disclosed_scripts: HashSet::new(),
            rationale: String::new(),
            rationale_notes: Vec::new(),
            // ONE definition, shared with Prefs::default, the slider's reset
            // target and the AI section's ● baseline (R22 #16) — see
            // model::STYLE_STRENGTH_DEFAULT for why the literal cannot live in
            // two Default impls.
            aip: AiPrefs::default(),
            // 0.65 — braver than the 0.5 calibration point, by user decision
            // (2026-08-17 ⑦). Same one-definition rule as above: the constant is
            // the lib's own `GradeStrength::DEFAULT`.
            // OFF: an extra image on a paid call is the user's opt-in, never a
            // default (the same rule `fit_ai_judge` follows).
            // OFF, same rule and a bigger bill (up to 17 calls per analyze).
            ai: AiState::default(),
            exp: ExportState::default(),
            // ./out — the CLI's and the batch renderer's own root; mirror
            // Prefs::default so a missing pref key means exactly this.
            hist: HistoryState::default(),
            fit: FitState::default(),
            // Paid opt-in (a vision call per fit) — mirror Prefs::default.
            // Paid opt-in, and up to three vision calls — mirror Prefs::default.
            // Paid opt-in (a second image per diverged reimagine) — mirror
            // Prefs::default.
            // Session state, never persisted (see the field's comment).
            // Mirror Prefs::default.
            ui: ShellState::default(),
            // English is the default / skeleton language; a persisted pref
            // (restored in `new`) overrides this on launch.
            lang: Lang::En,
            gal: GalleryState::default(),
            open: OpenState::default(),
            masks: MaskEditState::default(),
            overlay: OverlayState::default(),
            genai: GenerativeState::default(),
            view: ViewState::default(),
            crop: CropState::default(),
            curve_channel: 0,
            curve_drag: None,
            #[cfg(test)]
            #[cfg(test)]
            seams: TestSeams::default(),
            rlib: RecipeLibraryState::default(),
            tools: ToolsState::default(),
            variants: Vec::new(),
            active: 0,
            cards: CardsState::default(),
            normal: Default::default(),
            portrait: Default::default(),
        }
    }
}

impl AutoShadeApp {
    /// One frame of the app: the whole of `update` but the prefs flush, so a
    /// headless test can drive it with a bare `egui::Context`.
    pub(crate) fn run_frame(&mut self, ctx: &egui::Context) {
        // First frame: re-issue the OS-titlebar theme. install_theme sends it
        // from the creation closure, but viewport commands sent before the
        // event loop runs are dropped — a FIRST-ever launch (no prefs, so no
        // second install) kept the OS-default titlebar over dark chrome.
        if !std::mem::replace(&mut self.ui.os_theme_synced, true) {
            ctx.send_viewport_cmd(egui::ViewportCommand::SetTheme(self.ui.theme.system()));
            // …and say which system faces the font chain turned away.
            // `install_fonts` runs before this app exists and can only reach
            // stderr, which a windowed build does not have — so the first
            // frame is the earliest moment a user can be told at all. A
            // refusal is a decision this build made about a font that IS
            // installed, which is a different fact from the tofu warning the
            // gallery raises about names it cannot draw.
            for font in refused_system_fonts() {
                let t = trf(
                    self.lang,
                    "a system font was found but not loaded — {font}. File names in the scripts it covers may show as boxes.",
                    &[("font", font)],
                );
                self.toast(ToastKind::Error, t);
            }
        }
        self.poll_workers(ctx);
        // What quitting would cost, published for the one caller that cannot
        // reach `self`: macOS's `applicationShouldTerminate:`, which AppKit
        // invokes from outside this loop for ⌘Q, the App menu's Quit, the
        // Dock's Quit and log-out (see `quit.rs`). On every other platform
        // this is a cheap store nothing reads — the state machine is compiled
        // and tested everywhere so the Windows battery gates it.
        crate::quit::publish(self.quit_state());
        // A vetoed quit becomes an ordinary close request, so ⌘Q lands in the
        // SAME guard below as the title-bar ✕ rather than a parallel dialog
        // that would have to be kept in step with it.
        if crate::quit::take_close_request() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
        // Window-close guard: the unsaved-edit protection (● + nav_stash)
        // used to stop at photo switching — the title-bar ✕ dropped the open
        // photo's uncommitted develop AND every stashed one with no prompt.
        // An in-app confirm layer (egui Window, never an OS dialog) offers
        // save-all / discard / cancel; the guard re-checks on the way out, so
        // both quit buttons work by making the state genuinely clean.
        if ctx.input(|i| i.viewport().close_requested()) {
            // A typed-but-uncommitted mask or CARD rename must COUNT as
            // unsaved work: commit them before the dirty check below — an
            // otherwise clean recipe used to close without a prompt and the
            // rename died with the window (U10). A version rename is not
            // unsaved work in the same sense (it writes straight to its own
            // advisory sidecar), but it must not die with the window either,
            // so the last boundary before the quit decision flushes the lot.
            self.commit_pending_names();
            self.fork_edited_card();
            let unsaved_open = self.quit_guard_open_dirty();
            if self.busy {
                // A running export / retouch / paid AI generation dies with
                // the process — the ✕ used to bypass every guard mid-flight.
                // Block and say why; close again once the worker lands.
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                let t = tr(self.lang, "An operation is still running — wait for it to finish, then close").to_string();
                self.toast(ToastKind::Error, t);
            } else if !self.open.discard_requested
                && (unsaved_open
                    || !self.open.nav_stash.is_empty()
                    || self.inactive_dirty_variants() > 0)
            {
                ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
                self.ui.confirm_quit = true;
                // Surrender any widget focus NOW: the layer's Enter-saves
                // default is gated on "no widget focused", and a text field
                // that kept focus from before the close request silently
                // disabled the promised keyboard default. (Tab inside the
                // dialog can still focus its buttons — the gate then
                // correctly routes Enter to the focused control.)
                if let Some(id) = ctx.memory(|m| m.focused()) {
                    ctx.memory_mut(|m| m.surrender_focus(id));
                }
            }
        }
        if self.ui.confirm_quit {
            self.confirm_quit_layer(ctx);
        }
        // Hover-to-preview is frame-scoped: take last frame's target; the mask
        // list re-sets it below if the cursor is still on a row. The diff is
        // checked right before the overlay refresh at the end of update().
        let hover_prev = self.overlay.hover_mask.take();
        // The question a generation landing asks: BEFORE the shortcut tiers,
        // so its Esc / Enter are its own; never under the quit layer, which
        // outranks it.
        if !self.ui.confirm_quit {
            self.fit_prompt_layer(ctx);
        }

        self.upd_shortcuts(ctx);
        self.apply_zoom_glide(ctx);
        self.upd_dropped_and_title(ctx);

        self.upd_top_bar(ctx);

        self.upd_status_bar(ctx);

        self.upd_strips_and_side_panels(ctx);
        // "Keep skin natural" (Portraits, `panels/portrait.rs`): whatever this
        // frame wrote into the recipe — a slider, a paste, an AI landing —
        // meets the skin limits here, before the fork, the develop and the
        // undo commit below read it.
        self.guard_portrait_skin();
        // The immutability rule (2026-09-13): whatever this frame wrote into
        // the live recipe — a slider, a shortcut, a landing — continues on
        // an ✎ card when the active card is the pristine ✨ image, BEFORE the
        // turn sync, the Before rebuild, the develop dispatch and the undo
        // commit below see it.
        self.fork_edited_card();
        // The plates follow `recipe.quarter_turns` (R27) BEFORE the Before
        // rebuild and the redevelop dispatch below, so a turn — however it
        // arrived: the toolbar, an undo, a variant switch, a version load —
        // lands in the same frame it was asked for. Returns on a `u8` compare
        // when nothing turned, which is every frame but the one.
        self.sync_base_turns(ctx);
        // Reset re-stamping a legacy photo, a paste, or a restore would
        // otherwise leave the compare against a stale starting point. One
        // cheap LUT-only develop, and only when the curve actually changed.
        if self.src_path.is_some()
            // NOT base-present: an InPlace master legitimately renders Before
            // under the canvas curve, and gating on v.base froze its Before
            // against Reset / undo / paste forever. A Generated canvas
            // recipe's curve is empty by construction, so this compare is
            // simply quiet there.
            && !self.active_on_ai_pixels()
            && self.recipe.start_only() != self.open.before_start
            && let Some(b) = self.base_preview.clone()
        {
            let start = self.recipe.start_only();
            self.set_before(ctx, &b, &start);
        }
        // A held-still pointer still needs a repaint to receive the result.
        if self.dirty && !self.develop_inflight {
            self.start_redevelop();
        }
        if self.develop_inflight {
            ctx.request_repaint();
        }
        // The mask coverage overlay follows develop / selection / toggle /
        // hover (a changed hover target includes "left the list entirely").
        if self.overlay.hover_mask != hover_prev {
            self.overlay.overlay_stale = true;
        }
        if std::mem::take(&mut self.overlay.overlay_stale) {
            self.refresh_mask_overlay(ctx);
        }

        self.upd_central(ctx);

        self.upd_sheets(ctx);
        // Gated like drop PROCESSING is: while a modal layer is up, drops are
        // deliberately discarded — promising "Drop to open" there was a lie.
        if !self.modal_up() && ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_overlay"),
            ));
            let rect = ctx.content_rect();
            // Chrome-side veil → the chrome accent's family (see the colour
            // rule at ACCENT), dimmed to 0.29 so the white line reads on it
            // in every theme.
            let a = self.ui.theme.colors().accent_fill;
            let dim = |v: u8| (f32::from(v) * 0.29).round() as u8;
            painter.rect_filled(rect, 0.0, egui::Color32::from_rgba_unmultiplied(dim(a.r()), dim(a.g()), dim(a.b()), 150));
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                tr(self.lang, "Drop to open"),
                egui::FontId::proportional(28.0),
                egui::Color32::WHITE,
            );
        }

        // Transient toasts (bottom-right). Errors linger longer than
        // successes, long texts longer than short ones (Toast::ttl), a click
        // dismisses early, and the last 300 ms fade instead of blinking out.
        self.chrome.toasts.retain(|t| t.born.elapsed() < t.ttl());
        if !self.chrome.toasts.is_empty() {
            let mut dismiss: Option<usize> = None;
            let mut fading = false;
            egui::Area::new(egui::Id::new("toasts"))
                .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-12.0 + MOTION_TOAST_SLIDE, -40.0))
                .constrain(false)
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    let c = self.ui.theme.colors();
                    // Right-aligned rows: a toast slides in from the right
                    // edge by a shrinking right-hand gap.
                    ui.with_layout(egui::Layout::top_down(egui::Align::Max), |ui| {
                    for (idx, t) in self.chrome.toasts.iter().enumerate() {
                        let (bg, fg) = match t.kind {
                            ToastKind::Success => (c.toast_ok_bg, c.toast_ok_fg),
                            ToastKind::Error => (c.toast_err_bg, c.toast_err_fg),
                        };
                        let left = t.ttl().saturating_sub(t.born.elapsed());
                        let arrive = ease_out(t.born.elapsed().as_secs_f32() / MOTION_TOAST_IN_S);
                        let fade = (left.as_millis() as f32 / 300.0).min(1.0) * arrive;
                        fading |= fade < 1.0;
                        // Elevated pill: the theme's popup shadow + a faint
                        // text-tinted hairline lift it off the photo — a flat
                        // fill over arbitrary canvas content had no edge at
                        // all where the photo matched the toast colour.
                        let mut shadow = ui.visuals().popup_shadow;
                        shadow.color = shadow.color.gamma_multiply(fade);
                        let r = ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        // The area's right edge sits MOTION_TOAST_SLIDE past
                        // the old anchor (unconstrained, so it may leave the
                        // window); a settled toast keeps that gap, an
                        // arriving one starts with none — off to the right.
                        ui.add_space(arrive * MOTION_TOAST_SLIDE);
                        egui::Frame::NONE
                            .fill(bg.gamma_multiply(fade))
                            .stroke(egui::Stroke::new(1.0, fg.gamma_multiply(0.35 * fade)))
                            .shadow(shadow)
                            .corner_radius(egui::CornerRadius::same(RADIUS_MD.round() as u8))
                            .inner_margin(egui::Margin::symmetric(10, 8))
                            .show(ui, |ui| {
                                // Named token (#14a): this 420 is a WRAPPED
                                // paragraph's measure, not the prompt-field
                                // ceiling that happens to share the number —
                                // see theme.rs on why they are two constants.
                                ui.set_max_width(TOAST_W_MAX);
                                // `.wrap()` explicitly: this ui is a child of
                                // the right_to_left row above, and a label in
                                // a horizontal layout EXTENDS by default
                                // (egui 0.29 `Ui::wrap_mode`), so a long error
                                // text ignored the measure and ran past the
                                // window's edge as one line.
                                ui.add(
                                    egui::Label::new(egui::RichText::new(&t.text).color(fg.gamma_multiply(fade)))
                                        .wrap(),
                                );
                            })
                            .response
                            .interact(egui::Sense::click())
                        })
                        .inner;
                        if r.hovered() {
                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                        }
                        if r.clicked() {
                            dismiss = Some(idx);
                        }
                        ui.add_space(SPACE_MD);
                    }
                    });
                });
            if let Some(i) = dismiss {
                self.chrome.toasts.remove(i);
            }
            // Keep repainting so expiry doesn't wait for the next input
            // event — per frame while a fade is running, else a coarse poll.
            if fading {
                ctx.request_repaint();
            } else {
                ctx.request_repaint_after(Duration::from_millis(200));
            }
        }

        // Land a finished edit gesture (slider release, AI Analyze, Reset) into
        // the undo history — once per gesture, after all controls are read.
        self.commit_if_settled(ctx);
        // LAST, after every panel had its chance to spawn (L12-3): evaluated
        // at the top of update (inside poll_workers) the gate judged a frame
        // whose spawns had not happened yet, so the FIRST batch of workers
        // each frame started with no completion pump armed and their results
        // sat in the channel until the next input event.
        self.pump_repaint_gate(ctx);
    }
}

impl eframe::App for AutoShadeApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.run_frame(ctx);
    }

    /// Persist the prefs (last folder, view mode, export options) — restored by
    /// [`AutoShadeApp::new`]. Window geometry is saved by eframe itself.
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(
            storage,
            eframe::APP_KEY,
            &Prefs {
                prefs_era: PREFS_ERA,
                gallery_dir: self.gal.gallery_dir.clone(),
                guide: self.aip.guide,
                style_strength: self.aip.style_strength,
                grade_strength: self.aip.grade_strength,
                fit_strength: self.aip.fit_strength,
                send_style_ref_image: self.aip.send_style_ref_image,
                deep_think: self.aip.deep_think,
                style_embed: self.aip.style_embed,
                style_describe: self.aip.style_describe,
                style_src_dir: self.aip.style_src_dir.clone(),
                style_raw_dir: self.aip.style_raw_dir.clone(),
                direction_adherence: self.aip.direction_adherence,
                // Both written: exp_format is the truth, save_jpeg keeps a
                // pre-阶段4 build reading this prefs file sane.
                save_jpeg: self.exp.format == ExportFormat::Jpeg,
                exp_format: self.exp.format.pref_code(),
                exp_dest: self.exp.dest.pref_code(),
                last_export_dir: self.exp.last_dir.clone(),
                save_denoise: self.exp.save_denoise,
                save_denoise_strength: self.exp.save_denoise_strength,
                denoise_strength: self.exp.denoise_strength,
                baked_denoise_strength: self.exp.baked_denoise_strength,
                baked_save_denoise_strength: self.exp.baked_save_denoise_strength,
                zoned_fit: self.fit.zoned_fit,
                zoned_four_regions: self.fit.zoned_four_regions,
                fit_ai_judge: self.fit.fit_ai_judge,
                // R23-6 D: the deep reverse-fit toggle persists like every
                // other paid opt-in. `fit_ref` deliberately does NOT — it
                // names one file for one photo (see the field).
                fit_deep: self.fit.fit_deep,
                reimagine_retry: self.fit.reimagine_retry,
                ask_fit_after_generate: self.fit.ask_fit_after_generate,
                adjust_quality: self.genai.adjust_quality,
                view_mode: self.view.mode,
                exp_long_edge: self.exp.long_edge,
                exp_sharpen: self.exp.sharpen,
                exp_quality: self.exp.quality,
                exp_space: self.exp.space,
                preview_edge: self.genai.preview_edge,
                show_clipping: self.chrome.show_clipping,
                lang: self.lang,
                theme: self.ui.theme,
                accent: self.ui.accent,
            },
        );
    }
}
