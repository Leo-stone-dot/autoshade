//! App actions: open/variants/versions/history/quit flow.

use super::*;

use autoshade::advisor::{hint_action, FitAction};

/// The persisted develop state belonging to the card that is on the canvas.
///
/// `recipe.json` / `pixels.json` describe whichever card was active at the
/// last save; a different persisted card gets its baseline from the matching
/// `variants.json.others[]` entry. Keeping this pair behind one owner prevents
/// a card switch from being mistaken for an edit by one of the four unsaved
/// consumers.
pub(crate) struct ActiveBaseline<'a> {
    pub(crate) recipe: &'a EditRecipe,
    pub(crate) origin: Option<&'a std::path::Path>,
}

struct PersistedCard<'a> {
    position: usize,
    kind: VariantKind,
    id: Option<&'a str>,
    name: Option<&'a str>,
    recipe: &'a EditRecipe,
    origin: Option<&'a std::path::Path>,
}

impl AutoShadeApp {
    /// The Analysis fold's Strength dial as the typed axis the develop request
    /// reads (R23-3). Nothing else reads it.
    pub(crate) fn analysis_strength(&self) -> autoshade::recipe::GradeStrength {
        autoshade::recipe::GradeStrength::new(self.aip.grade_strength)
    }

    /// The Reverse-fit fold's OWN Strength dial as the fit's honesty budget
    /// (F1). Separate from [`Self::analysis_strength`] on purpose — user
    /// decision 2026-09-12: the two functions live in two folds, so each has
    /// its own control and neither reading reaches the other. Through v1.3.1
    /// both read one `panel_strength()`, and the fit's budget was a dial two
    /// folds above the fit's own row.
    pub(crate) fn fit_strength(&self) -> autoshade::recipe::GradeStrength {
        autoshade::recipe::GradeStrength::new(self.aip.fit_strength)
    }

    /// Restore persisted prefs (last folder, view mode, export options) and
    /// re-open the library the user was browsing. Window geometry itself is
    /// restored by eframe's own persistence layer.
    pub(crate) fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // The REAL event-loop context replaces default()'s headless one:
        // workers wake the loop through this clone (see `spawn_worker`).
        let mut app = Self { egui_ctx: cc.egui_ctx.clone(), ..Self::default() };
        if let Some(prefs) =
            cc.storage.and_then(|s| eframe::get_value::<Prefs>(s, eframe::APP_KEY))
        {
            app.restore_prefs(prefs);
        }
        // The launch check of the interpreter the AI sidecars would run: a fact
        // about this machine, measured once before the first frame. It spawns
        // the interpreter only when the managed environment is not the answer,
        // so a prepared install costs one file check. After the prefs restore
        // (which decides the language) and outside it, so headless tests that
        // restore prefs run no interpreter.
        if let Some(note) = app.refresh_python_check() {
            app.chrome.startup_note = Some(note);
        }
        if !app.busy {
            app.show_startup_note();
        }
        // The quit guard's two process-wide hooks (see `quit.rs`): the repaint
        // handle a vetoed quit wakes the UI with, and — on macOS — the
        // delegate method that does the vetoing. Both need a live event loop,
        // and this creation closure is the first moment there is one.
        crate::quit::set_ctx(&cc.egui_ctx);
        #[cfg(target_os = "macos")]
        if let Err(why) = crate::macos::install_quit_guard() {
            // Not fatal, and not silent: without that method ⌘Q behaves the
            // way it did before this batch, and the user is owed the fact.
            eprintln!(
                "warning: ⌘Q and Dock Quit will NOT ask about unsaved work — {why}. \
                 Close the window with its red button instead, which does ask."
            );
        }
        app
    }

    /// Restore the persisted settings through the same path at startup and
    /// in headless tests; no access to the user's actual prefs is needed.
    pub(crate) fn restore_prefs(&mut self, prefs: Prefs) {
        let app = self;
        app.aip.guide = prefs.guide;
        app.aip.style_strength = prefs.style_strength.clamp(0.0, 1.0);
        app.aip.grade_strength = prefs.grade_strength.clamp(0.0, 1.0);
        app.aip.fit_strength = prefs.fit_strength.clamp(0.0, 1.0);
        app.aip.send_style_ref_image = prefs.send_style_ref_image;
        app.aip.deep_think = prefs.deep_think;
        app.aip.style_embed = prefs.style_embed;
        app.aip.style_describe = prefs.style_describe;
        // Only a folder that still EXISTS is prefilled: a picker opened at
        // a deleted path lands wherever the OS decides (the same rule the
        // gallery restore above follows).
        app.aip.style_src_dir = prefs.style_src_dir.clone().filter(|d| d.is_dir());
        app.aip.style_raw_dir = prefs.style_raw_dir.clone().filter(|d| d.is_dir());
        app.aip.direction_adherence = autoshade::recipe::DirectionAdherence::new(prefs.direction_adherence).get();
        app.exp.format = ExportFormat::from_pref(prefs.exp_format, prefs.save_jpeg);
        app.exp.dest = ExportDest::from_pref(prefs.exp_dest);
        app.exp.last_dir = prefs.last_export_dir.clone();
        app.exp.save_denoise = prefs.save_denoise;
        app.exp.save_denoise_strength = prefs.save_denoise_strength.clamp(0.0, 1.0);
        app.exp.denoise_strength = prefs.denoise_strength.clamp(0.0, 1.0);
        app.exp.baked_denoise_strength = prefs.baked_denoise_strength.clamp(0.0, 1.0);
        app.exp.baked_save_denoise_strength = prefs.baked_save_denoise_strength.clamp(0.0, 1.0);
        app.fit.zoned_fit = prefs.zoned_fit;
        app.fit.zoned_four_regions = prefs.zoned_four_regions;
        app.fit.fit_ai_judge = prefs.fit_ai_judge;
        app.fit.fit_deep = prefs.fit_deep;
        app.fit.reimagine_retry = prefs.reimagine_retry;
        app.fit.ask_fit_after_generate = prefs.ask_fit_after_generate;
        app.genai.adjust_quality = prefs.adjust_quality.min(2);
        app.view.mode = prefs.view_mode;
        app.exp.long_edge = prefs.exp_long_edge;
        app.exp.sharpen = prefs.exp_sharpen.clamp(0.0, 100.0);
        app.exp.quality = prefs.exp_quality.clamp(1.0, 100.0);
        app.chrome.show_clipping = prefs.show_clipping;
        // Restore the UI language (an older save without this key decoded to
        // `Lang::En` via `#[serde(default)]`, so this is always valid).
        app.lang = prefs.lang;
        // Restore the theme and re-apply it — main() installed the Dark
        // default before prefs were readable (same shape as the greeting).
        app.ui.theme = prefs.theme;
        app.ui.accent = prefs.accent;
        install_look(&app.egui_ctx, app.ui.theme, app.ui.accent);
        // The greeting was set by default() BEFORE the language was known —
        // re-issue it, or a Chinese install opens with one English line.
        app.status =
            tr(app.lang, "Open a photo, or open a folder to browse your library.").into();
        // Each arm advances one persisted dial law; current/future files
        // keep the choices made under their own law.
        for era in prefs.prefs_era..PREFS_ERA {
            match era {
                0 => {
                    // Era 1's historical RAW law. The final step below emits
                    // one note using the original saved values, not this hop.
                    app.exp.denoise_strength = 1.0;
                    app.exp.save_denoise_strength = 1.0;
                }
                1 => {
                    // The old file cannot tell which source used a shared
                    // dial. Preserve its choice for SCUNet, reset only RAW.
                    app.exp.baked_denoise_strength = prefs.denoise_strength.clamp(0.0, 1.0);
                    app.exp.baked_save_denoise_strength = prefs.save_denoise_strength.clamp(0.0, 1.0);
                    let default = autoshade::denoise::DEFAULT_STRENGTH_RAW;
                    if prefs.denoise_strength != default || prefs.save_denoise_strength != default {
                        app.chrome.startup_note = Some(trf(
                            app.lang,
                            "AI denoise strength for RAW files was reset to {new}% (was {a}% in Detail, {b}% in Export). The new default looks like Lightroom's Denoise at 50; higher removes more luminance grain, any positive strength keeps the colour clean, and 0% leaves the input untouched",
                            &[
                                ("new", &format!("{:.0}", default * 100.0)),
                                ("a", &format!("{:.0}", prefs.denoise_strength * 100.0)),
                                ("b", &format!("{:.0}", prefs.save_denoise_strength * 100.0)),
                            ],
                        ));
                    }
                    app.exp.denoise_strength = default;
                    app.exp.save_denoise_strength = default;
                }
                _ => unreachable!("missing preference migration for era {era}"),
            }
        }
        // Only known color spaces — an out-of-range pref falls back to sRGB.
        if prefs.exp_space <= 2 {
            app.exp.space = prefs.exp_space;
        }
        // Only the known steps — a corrupt pref must not produce a 1-px
        // or 100-MP working preview.
        if [1280, 2560, 4096].contains(&prefs.preview_edge) {
            app.genai.preview_edge = prefs.preview_edge;
        }
        if let Some(dir) = prefs.gallery_dir.filter(|d| d.is_dir()) {
            app.open_folder(dir);
        }
        // A restored folder's asynchronous status must not erase the note
        // before the user sees it. Its completion uses the same status line.
        if !app.busy {
            app.show_startup_note();
        }
    }

    /// The active card's actual pixels choose the path, including generated
    /// masters under a RAW original. An unopened app displays the RAW dials.
    pub(crate) fn denoise_uses_raw(&self) -> bool {
        self.active_source_path().is_none_or(|p| autoshade::decode::is_raw(&p))
    }

    pub(crate) fn selected_denoise_strength(&self, on_export: bool) -> f32 {
        match (self.denoise_uses_raw(), on_export) {
            (true, false) => self.exp.denoise_strength,
            (true, true) => self.exp.save_denoise_strength,
            (false, false) => self.exp.baked_denoise_strength,
            (false, true) => self.exp.baked_save_denoise_strength,
        }
    }

    pub(crate) fn show_startup_note(&mut self) {
        if let Some(note) = self.chrome.startup_note.take() {
            self.status.push_str(" — ");
            self.status.push_str(&note);
        }
    }

    /// What quitting RIGHT NOW would cost, in the terms `quit.rs` publishes.
    ///
    /// Built from the same predicates as the close guard in `app.rs`, plus the
    /// one thing that guard reaches by MUTATING: a typed but uncommitted mask,
    /// card or version name. The guard may call `commit_pending_names` because
    /// it runs on the way out; a per-frame reading may not write, so it counts
    /// an uncommitted name as unsaved work directly. Leaving that out would be
    /// the U10 loss over again — an otherwise clean recipe whose pending
    /// rename dies with the process.
    ///
    /// The `||` chain is ordered by cost: `inactive_dirty_variants` walks the
    /// strip deep-comparing recipes, so it is asked last.
    pub(crate) fn quit_state(&self) -> crate::quit::QuitState {
        crate::quit::QuitState::from_app(self.busy, self.open.discard_requested, || {
            self.masks.mask_name_buf.is_some()
                || self.cards.version_name_buf.is_some()
                || self.cards.variant_name_buf.is_some()
                || !self.open.nav_stash.is_empty()
                || self.quit_guard_open_dirty()
                || self.inactive_dirty_variants() > 0
        })
    }

    /// L12#3: disclose gallery names whose script no installed font can
    /// draw — once per script per session. Runs over the CURRENT gallery
    /// (folder-open granularity); pure classification, no file I/O.
    pub(crate) fn disclose_undrawable_names(&mut self) {
        self.disclose_undrawable_names_given(installed_scripts());
    }

    /// [`Self::disclose_undrawable_names`] against an explicit set of
    /// installed scripts. The session's set is process-wide (filled once by
    /// `install_fonts`), so a test that reads it sees whatever another test
    /// in the same process installed; tests pass their own.
    pub(crate) fn disclose_undrawable_names_given(&mut self, installed: &[&'static str]) {
        let stems: Vec<String> = self
            .gal.gallery
            .iter()
            .map(|p| autoshade::pipeline::stem(p).to_string())
            .collect();
        for stem in &stems {
            for (script, sample) in undrawable_scripts(stem, installed) {
                if self.disclosed_scripts.insert(script) {
                    let t = trf(
                        self.lang,
                        "some file names use characters no installed font can draw ({sample}) — they show as boxes",
                        &[("sample", &sample.to_string())],
                    );
                    self.toast(ToastKind::Error, t.clone());
                    self.status = t;
                }
            }
        }
    }

    pub(crate) fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        let text = text.into();
        // An identical LIVE toast refreshes instead of duplicating: undoing
        // through a washed history region fires the same repair disclosure
        // once per healed step, and five copies evicted still-live ERROR
        // toasts from the ring. The refresh MOVES it to the back — it is
        // the newest content, and refreshing in place left it first in
        // line for eviction, the exact loss the dedup exists to prevent.
        if let Some(i) = self.chrome.toasts.iter().position(|t| t.text == text && t.kind == kind) {
            self.chrome.toasts.remove(i);
        }
        self.chrome.toasts.push(Toast { text, kind, born: Instant::now() });
        if self.chrome.toasts.len() > 5 {
            self.chrome.toasts.remove(0); // keep the stack readable
        }
    }

    /// A worker finished successfully: status line + a success toast, unbusy.
    pub(crate) fn done(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.status = text.clone();
        self.toast(ToastKind::Success, text);
        self.busy = false;
    }

    /// A worker failed: status line + a lingering error toast, unbusy. A single
    /// status line is too easy to miss for a failed export or API call.
    pub(crate) fn fail(&mut self, what: &str, e: impl std::fmt::Display) {
        // `{e:#}`, not `{e}`: anyhow's alternate Display prints the WHOLE
        // chain ("mask refine failed: open image X: The image format could not
        // be determined" instead of just the outermost line). Every `.context`
        // this app attaches — the file, the stage, the cause — was being
        // dropped exactly where the user needed it, in the toast.
        let text = format!("{what}: {e:#}");
        self.status = text.clone();
        self.toast(ToastKind::Error, text);
        self.busy = false;
    }

    /// Arm the cancellable-worker state for a retouch/generative task: a fresh
    /// cancel flag (this shows the status-bar ✕) and the epoch its result must
    /// present to be applied. Streaming generative workers also hand the flag
    /// to the lib so the download itself stops at the next event; local
    /// compute is abandon-only (its late result is discarded by epoch).
    pub(crate) fn arm_cancel(&mut self) -> (u64, Arc<std::sync::atomic::AtomicBool>) {
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        self.tools.gen_cancel = Some(flag.clone());
        (self.tools.gen_epoch, flag)
    }

    /// The status-bar ✕: stop the running retouch/generative task. The UI
    /// unblocks NOW; the worker sees the flag at its next checkpoint and its
    /// late result arrives under a stale epoch — discarded, never applied.
    pub(crate) fn cancel_generative(&mut self) {
        // The cancelled analysis' late result has no claim to win.
        logic::look::cancel(&mut self.tools.look);
        if let Some(flag) = self.tools.gen_cancel.take() {
            flag.store(true, std::sync::atomic::Ordering::Relaxed);
            self.tools.gen_epoch = self.tools.gen_epoch.wrapping_add(1);
            self.busy = false;
            // Honest about BOTH shapes. A generative worker polls the flag
            // between stream events and really does stop; `produce_recipe`
            // takes no flag at all, so an Analyze keeps running — and keeps
            // billing — until it answers or hits its deadline. Claiming a
            // checkpoint that does not exist for the path the user is most
            // likely cancelling is the kind of false status this project
            // treats as a defect.
            self.status = tr(
                self.lang,
                "cancelled — the app is free again and the late result is discarded; a generative call stops at its next checkpoint, while an AI analyze keeps running (and billing) until it finishes or times out",
            )
            .into();
        }
    }

    pub(crate) fn open_path(&mut self, path: PathBuf) {
        if self.busy {
            // Defence-in-depth: since the px combo refuses BEFORE arming, no
            // live caller reaches this guard with keep_recipe set — but a
            // stale request here once misclassified a genuine cross-photo
            // open as a same-path keep-flight, and any future caller that
            // arms-then-calls gets the same protection instead of relying on
            // its own busy check. Said so, instead of claiming a live
            // scenario.
            self.cards.keep_recipe = false;
            // Refusal must be visible (the switch_variant rule: a silent
            // no-op reads as a dead UI) — the EMPTY-STATE picker button has
            // no busy gate, so this arm IS user-reachable (L11-4).
            let t = tr(self.lang, "busy — the photo opens when the current task finishes");
            self.toast(ToastKind::Error, t);
            return;
        }
        // Flush every typed-but-uncommitted name before the stash below
        // snapshots the strip — a thumbnail click / Ctrl+O / arrow-key nav
        // with a name box focused silently stashed the OLD name (U10). Each
        // buffer carries the photo it was seeded on, so the commits land on
        // the OUTGOING photo even though `src_path` moves below.
        self.commit_pending_names();
        // …and an edit the frame hook has not moved yet lands on its ✎ card
        // before the strip is snapshotted (the immutability rule's boundary
        // discipline — every persist entry point forks first).
        self.fork_edited_card();
        // …and the buffers themselves die with the photo (M15): carried
        // across, a same-index mask / same-number version / same-card rename
        // box on the NEXT photo skipped the reseed and greeted it pre-filled
        // with the previous photo's text.
        self.masks.mask_name_buf = None;
        self.cards.version_name_buf = None;
        self.cards.variant_name_buf = None;
        // An armed strip ✕ dies with the photo too — the index it named
        // belongs to a strip that is about to be replaced (R24-4).
        self.cards.variant_delete_confirm = None;
        // The mid-open window marker (see confirm_quit_layer): src_path is
        // re-pointed below while recipe/saved_recipe still describe the old
        // photo; cleared in BOTH Msg::Opened arms. `open_same_path` is the
        // FACT the repair gates key on — keep_recipe is only a request, and
        // a stale request must never classify a flight.
        self.open.open_in_flight = true;
        self.open.open_same_path = self.src_path.as_ref() == Some(&path);
        // Leaving a photo with unsaved edits: stash the canvas for this
        // session so navigation can never silently destroy work (arrow keys /
        // thumbnail clicks used to drop the whole develop). Ctrl+S still owns
        // disk; returning to the photo restores the stash over the sidecar.
        // "Unsaved" covers PIXELS too: a baked retouch whose master isn't the
        // one recorded in the store's pixels.json would otherwise vanish from
        // the canvas on the way back even though its raster is on disk.
        // SAME-path reopens stash too (no `old != path` gate): clicking the
        // current thumbnail / re-picking the current file takes the fresh
        // branch below, which resets per-photo state and restores stash-then-
        // disk — without the entry, a dirty canvas was silently replaced by
        // the disk state. The stash entry is consumed by that same restore,
        // so a same-path reopen becomes "reload, preserving unsaved work".
        if let Some(old) = self.src_path.clone() {
            let origin = self.active_variant().and_then(|v| v.origin.clone());
            // Background variants count too (H4): their unsaved work has no
            // sidecar to survive in, and the strip used to collapse to the
            // active canvas alone on navigation. THIS photo's strip only:
            // the cross-photo sum (inactive_dirty_variants) belongs to the
            // quit dialog, and using it here chain-stashed clean photos.
            if self.nav_stash_gate_dirty() {
                let others: Vec<StashedVariant> = self
                    .variants
                    .iter()
                    .enumerate()
                    .filter(|(i, _)| *i != self.active)
                    .map(|(_, v)| StashedVariant {
                        kind: v.kind,
                        // Hop 3 of 6 (R24-2): identity + name ride the stash
                        // out; the workers restore is hop 4 back.
                        id: v.id.clone(),
                        name: v.name.clone(),
                        recipe: v.recipe.clone(),
                        base: v.base.clone(),
                        origin: v.origin.clone(),
                    })
                    .collect();
                let active_pos = self.active.min(others.len());
                self.open.nav_stash.insert(
                    old,
                    StashEntry {
                        recipe: self.recipe.clone(),
                        base: self.active_variant().and_then(|v| v.base.clone()),
                        origin,
                        kind: self.active_variant().map_or(VariantKind::Original, |v| v.kind),
                        // The ACTIVE card's identity + name travel with the
                        // rest of its state (R24-2): a nav round-trip that
                        // re-minted the id would orphan every version taken
                        // from this card.
                        id: self
                            .active_variant()
                            .map_or_else(|| ORIGINAL_VARIANT_ID.to_string(), |v| v.id.clone()),
                        name: self.active_variant().and_then(|v| v.name.clone()),
                        others,
                        active_pos,
                    },
                );
            }
        }
        let lang = self.lang;
        self.busy = true;
        // Every open starts resolved-until-proven-otherwise; the Opened
        // handler's Unreadable arm re-arms it for THIS photo when the read
        // fails.
        self.open.open_unresolved = false;
        self.src_path = Some(path.clone());
        // The version list describes the photo `src_path` NAMES, so it dies
        // with the old value of that field — not later, when the decode lands.
        // `refresh_versions` only runs from the completed `Msg::Opened`, and a
        // RAW decode is seconds; for that whole window the Versions panel
        // showed photo A's v1/v2/v3 with live buttons while every action
        // resolved `self.src_path`, which already pointed at photo B. Clicking
        // 🗑 on the stale list called `store::delete_version(B, n)` and
        // PERMANENTLY removed a snapshot of B — recipe and frozen mask
        // rasters, no backup, no undo — then refreshed the list from B so the
        // rows visibly changed and the status line confirmed a delete, hiding
        // what had happened. "＋ Save as version" was the mirror image: it
        // wrote A's canvas into B's develop as B's next version.
        self.cards.versions.clear();
        // …and so does the armed "overwrite the .xmp beside the photo" confirm
        // (R22-8): it is a claim about THIS photo's neighbouring sidecar, and
        // carrying it across would let one click overwrite the NEXT photo's
        // Lightroom sidecar with no warning at all. Same reasoning as the
        // version list above.
        self.exp.xmp_beside_confirm = false;
        self.status = trf(lang, "decoding {path} …", &[("path", &path.display().to_string())]);
        // Working-preview size is a user choice now (gap batch E): 1280 keeps
        // sliders fluid; 2560/4096 trade tick latency for real 1:1 detail when
        // checking focus / noise.
        let edge = self.genai.preview_edge.clamp(640, 8192);
        // Decoded-base LRU: gallery culling flips between neighbours constantly,
        // and a hit skips the multi-second full demosaic entirely. Delivered
        // through the ordinary Msg::Opened channel so the handler stays the one
        // place that resets per-photo state.
        if let Some(hit) = self.cached_base(&path, edge) {
            let _ = self.tx.send(Msg::Opened(Box::new(Ok(hit))));
            return;
        }
        self.spawn_worker(
            move || {
                // Full-frame commit budget (budget.rs): the demosaic transient
                // pays the corpus peak whatever `edge` caps afterwards.
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path)));
                // Build a CLEAN preview base by developing the RAW sensor data
                // (downscaled), NOT the camera's already-baked 8-bit JPEG preview:
                // re-developing that double-processes it and amplifies its grain when
                // you push tone/clarity. Baked images (PNG/TIFF/JPEG) are their own
                // source. Demosaic is slow, so this runs off the UI thread.
                let res = (|| -> anyhow::Result<OpenedBase> {
                    // The LRU key, captured BEFORE any read (the curve-memo
                    // rule): filing the decode under completion-time state
                    // handed a mid-open replacement the previous content's
                    // pixels and knots under its own key, and a popup-driven
                    // px change mid-flight mislabelled the edge.
                    let src_ident = (edge, file_stamp(&path), pixel_identity(&path));
                    let (thumb, start, lens, as_shot) = if autoshade::decode::is_raw(&path) {
                        // Identity BEFORE the read: the primed answer below
                        // is filed under the content it was computed FROM —
                        // stat'ing after the multi-second develop filed a
                        // mid-open replacement's NEW identity with the OLD
                        // content's answer.
                        let ident = autoshade::pipeline::curve_ident(&path);
                        // The BASE part this photo's develop starts from
                        // (v1.17.0): its saved develop's camera profile and
                        // Look, else the fresh canvas's. Developed under it so
                        // the common open pays ONE develop; a canvas recipe
                        // that settles on another part is re-developed by the
                        // canvas itself (`ensure_source_part`).
                        let part = autoshade::pipeline::photo_base_part(&path);
                        // Developed AT the working edge (the cap runs before
                        // tone/geometry): opening a 61 MP RAW no longer keeps
                        // a full-resolution develop resident just to be
                        // thumbnailed — the knots estimate below is CDF
                        // statistics and reads the same at working res.
                        let full = autoshade::render::base_develop(&path, &part, Some(edge))?;
                        // In-camera lens profile (Sony 0x70xx): one cheap TIFF
                        // parse, stamped all-available-on (the user default).
                        let lens = autoshade::pipeline::fresh_lens_profile(&path);
                        // As-shot WB anchor: one metadata-only decode (no
                        // demosaic) — the Temp slider then speaks absolute
                        // Kelvin for this photo.
                        let as_shot = autoshade::render::as_shot_wb(&path);
                        // The fresh START: the camera-matched base look:
                        // CDF-match the neutral
                        // develop against the camera's own rendition, paired
                        // like with like by the one entry all three open
                        // paths share — the frame the rendition shows, and
                        // the profile's corner lift only when the rendition
                        // shows it (render::camera_base_look).
                        // A RAW with no embedded preview (or a decode hiccup)
                        // just opens without a base look — never a failure.
                        // The decision is the library's one
                        // (`pipeline::fresh_canvas_start_with`); the estimate
                        // below runs only when it is asked for.
                        let estimate = || -> Vec<[f32; 2]> {
                            // The estimate needs the NEUTRAL develop. `full` is
                            // it unless a saved develop names a profile; that
                            // case pays its own neutral develop, and only here.
                            let neutral = if part.is_neutral() {
                                None
                            } else {
                                match autoshade::render::render_to_image(
                                    &path,
                                    &EditRecipe::default(),
                                    None,
                                    Some(edge),
                                ) {
                                    Ok(n) => Some(n),
                                    Err(_) => return Vec::new(),
                                }
                            };
                            match autoshade::decode::embedded_preview(&path) {
                                Ok(Some(cam)) => {
                                    match autoshade::render::camera_base_look(neutral.as_ref().unwrap_or(&full), &lens, &cam) {
                                        // The ANSWER primes the repair memo: the
                                        // open already paid this develop, and
                                        // discarding the result made every later
                                        // repair site (Ctrl+Z, variant switch,
                                        // Ctrl+S) pay a second full decode on
                                        // the UI thread. Answers only — an
                                        // inability stays uncached so the next
                                        // reader retries.
                                        Some(k) => {
                                            autoshade::pipeline::prime_curve_memo(
                                                &path,
                                                ident,
                                                k.clone(),
                                            );
                                            k
                                        }
                                        // None = could not judge. For an OPEN
                                        // that is the same as no base look — the
                                        // repair path is where the distinction
                                        // decides whether a SAVED curve may be
                                        // replaced.
                                        None => Vec::new(),
                                    }
                                }
                                _ => Vec::new(),
                            }
                        };
                        let start = autoshade::pipeline::fresh_canvas_start_with(
                            &path,
                            &autoshade::diag::photo(&path),
                            estimate,
                        );
                        // render_to_image already developed AT the working edge
                        // — `full` IS the thumb. The unconditional
                        // `full.thumbnail(edge, edge)` here duplicated a
                        // ~45 MP frame at the 8192 edge just to copy it; the
                        // resize only runs in the defensive off-spec case.
                        let thumb = if full.width().max(full.height()) > edge {
                            autoshade::render::thumbnail(&full, edge, edge)
                        } else {
                            full
                        };
                        (thumb, (start, part), lens, as_shot)
                    } else {
                        (
                            // baked-by-construction: the !is_raw arm of the open (branch above).
                            autoshade::render::thumbnail(&autoshade::decode::load_image(&path)?, edge, edge),
                            Default::default(),
                            Default::default(),
                            None,
                        )
                    };
                    // The saved develop's baked pixel master (in-place heal /
                    // clone / fill / denoise), when the store records one and
                    // it still resolves — decoded HERE so the UI thread never
                    // loads a 61 MP PNG. A missing/unreadable master degrades
                    // to the plain source open, never a failure.
                    let mut baked_note = None;
                    let baked = autoshade::store::read_pixel_source(&path).and_then(
                        |(origin, generated)| {
                            // baked-by-construction: the store's baked pixel master (a PNG we wrote).
                            let img = match autoshade::decode::load_image(&origin) {
                                Ok(i) => i,
                                Err(e) => {
                                    // Disclosed at LANDING too, not only in a
                                    // log the windowed build cannot show
                                    // (L11-3). Typed detail; on_opened
                                    // localizes.
                                    eprintln!(
                                        "⚠ baked master {} failed to decode ({e}) — opening the un-retouched source",
                                        origin.display()
                                    );
                                    baked_note =
                                        Some(format!("{}: {e:#}", origin.display()));
                                    return None;
                                }
                            };
                            Some((Arc::new(autoshade::render::thumbnail(&img, edge, edge)), origin, generated))
                        },
                    );
                    // Arc once here so every downstream sharer (variants, the
                    // preview worker) is an O(1) refcount bump, not a deep copy.
                    Ok((Arc::new(thumb), start, lens, as_shot, baked, src_ident, baked_note))
                })();
                Msg::Opened(Box::new(res))
            },
            |e| Msg::Opened(Box::new(Err(e))),
        );
    }

    /// Decoded-base LRU lookup (most-recent entry kept last). Missing metadata
    /// (mtime `None`) matches itself, so the cache still works where mtime is
    /// unavailable — it just loses the staleness guard there.
    pub(crate) fn cached_base(&mut self, path: &std::path::Path, edge: u32) -> Option<OpenedBase> {
        let mtime = file_stamp(path);
        let pixels = pixel_identity(path);
        let pos = self.open.base_cache.iter().position(|((p, e, t, px), _)| {
            p == path && *e == edge && *t == mtime && *px == pixels
        })?;
        let entry = self.open.base_cache.remove(pos);
        let hit = entry.1.clone();
        self.open.base_cache.push(entry);
        Some(hit)
    }

    /// Remember a freshly decoded base. The KEY comes from the worker's
    /// pre-read capture (`opened.5`), never from a stat or `preview_edge`
    /// here: the decode takes seconds, an egui dropdown popup outlives
    /// add_enabled_ui(!busy) (so the px combo CAN change mid-flight), and
    /// filing the pixels under completion-time state handed a mid-open
    /// replacement the previous content's pixels AND knots under its own
    /// key — the curve memo's identity-before-read rule, applied to its
    /// missed twin.
    pub(crate) fn remember_base(&mut self, path: &std::path::Path, opened: &OpenedBase) {
        let (edge, mtime, pixels) = opened.5.clone();
        self.open.base_cache.retain(|((p, e, _, _), _)| !(p == path && *e == edge));
        self.open.base_cache.push(((path.to_path_buf(), edge, mtime, pixels), opened.clone()));
        if self.open.base_cache.len() > BASE_CACHE_CAP {
            self.open.base_cache.remove(0); // least-recent first
        }
    }

    /// Drop the open photo's decoded-base cache entries. Called whenever its
    /// BAKED pixel identity changes (a retouch lands, a save writes/clears
    /// pixels.json) — the source mtime doesn't move then, so the mtime guard
    /// alone would happily serve a hit with stale baked pixels.
    pub(crate) fn forget_open_base(&mut self) {
        if let Some(p) = self.src_path.clone() {
            self.open.base_cache.retain(|((q, _, _, _), _)| q != &p);
        }
    }

    /// Match a live card to a persisted card. Stable IDs are authoritative
    /// when both sides carry one; an id-less legacy side falls back to kind +
    /// strip position, mirroring [`Self::version_is_from`]'s ID-first rule.
    fn persisted_identity_matches(
        live: &Variant,
        live_position: usize,
        saved_kind: VariantKind,
        saved_position: usize,
        saved_id: Option<&str>,
    ) -> bool {
        if !live.id.is_empty()
            && let Some(id) = saved_id.filter(|id| !id.is_empty())
        {
            return live.id == id;
        }
        live.kind == saved_kind && live_position == saved_position
    }

    fn persisted_card_for(&self, live_position: usize) -> Option<PersistedCard<'_>> {
        let live = self.variants.get(live_position)?;
        let rec = match &self.hist.saved_strip {
            Some(rec) => rec,
            None => {
                let trivial = self.variants.len() == 1
                    && live_position == 0
                    && crate::model::strip_is_trivial(
                        live.kind,
                        &live.id,
                        live.name.as_deref(),
                        0,
                    );
                return trivial.then_some(PersistedCard {
                    position: 0,
                    kind: VariantKind::Original,
                    id: Some(ORIGINAL_VARIANT_ID),
                    name: None,
                    recipe: &self.hist.saved_recipe,
                    origin: self.hist.pixels_on_disk.as_deref(),
                });
            }
        };

        if let Some(kind) = VariantKind::from_store_str(&rec.active_kind)
            && Self::persisted_identity_matches(
                live,
                live_position,
                kind,
                rec.active_pos,
                rec.active_id.as_deref(),
            )
        {
            return Some(PersistedCard {
                position: rec.active_pos,
                kind,
                id: rec.active_id.as_deref(),
                name: rec.active_name.as_deref(),
                recipe: &self.hist.saved_recipe,
                origin: self.hist.pixels_on_disk.as_deref(),
            });
        }

        rec.others.iter().enumerate().find_map(|(i, entry)| {
            let position = if i < rec.active_pos { i } else { i + 1 };
            let kind = VariantKind::from_store_str(&entry.kind)?;
            Self::persisted_identity_matches(
                live,
                live_position,
                kind,
                position,
                entry.id.as_deref(),
            )
            .then_some(PersistedCard {
                position,
                kind,
                id: entry.id.as_deref(),
                name: entry.name.as_deref(),
                recipe: &entry.recipe,
                origin: entry.origin.as_deref(),
            })
        })
    }

    /// The ONE owner for the persisted `(recipe, origin)` of the active card.
    /// A card created after the last save has no baseline and is therefore
    /// unsaved. With no strip record, only the trivial lone Original maps to
    /// `recipe.json` / `pixels.json`.
    pub(crate) fn active_baseline(&self) -> Option<ActiveBaseline<'_>> {
        let saved = self.persisted_card_for(self.active)?;
        Some(ActiveBaseline { recipe: saved.recipe, origin: saved.origin })
    }

    fn active_canvas_dirty(&self) -> bool {
        if self.src_path.is_none() || self.active_variant().is_none() {
            return false;
        }
        self.active_baseline().is_none_or(|saved| {
            dirty_vs(&self.recipe, saved.recipe)
                || !same_master_opt(
                    self.active_variant().and_then(|v| v.origin.as_deref()),
                    saved.origin,
                )
        })
    }

    /// Per-frame status-bar consumer of [`Self::active_baseline`].
    pub(crate) fn unsaved_marker_dirty(&self) -> bool {
        self.active_canvas_dirty()
    }

    /// Window-close consumer of [`Self::active_baseline`].
    pub(crate) fn quit_guard_open_dirty(&self) -> bool {
        self.active_canvas_dirty()
    }

    /// Navigation-stash consumer: active canvas plus this photo's strip work.
    pub(crate) fn nav_stash_gate_dirty(&self) -> bool {
        self.active_canvas_dirty() || self.open_dirty_variants() > 0
    }

    /// Quit-dialog `PendingSave` consumer: the same open-photo state as the
    /// navigation stash, without counting other photos' stash entries.
    pub(crate) fn pending_save_gate_dirty(&self) -> bool {
        self.active_canvas_dirty() || self.open_dirty_variants() > 0
    }

    /// How many persisted-card mismatches hold work that quitting would discard.
    ///
    /// Dirty means: a live card cannot be matched to the union of the saved
    /// active card (`recipe.json` / `pixels.json`) and `variants.json.others`,
    /// or a matched card differs in kind/name/develop, or a persisted card was
    /// deleted. IDs match first; id-less records fall back to kind + position.
    /// Which card is selected is navigation, not an edit. The OPEN photo's
    /// strip only — the per-photo half of
    /// [`Self::inactive_dirty_variants`]; the stash decision in `open_path`
    /// keys on THIS (summing other photos' stashed variants into the gate
    /// chain-stashed every clean photo the user merely visited).
    ///
    /// The pre-v0.22 rule compared each background variant against the
    /// ACTIVE canvas's `saved_recipe`/`pixels_on_disk`. Those describe a
    /// DIFFERENT variant, so any strip whose cards disagree in origin (every
    /// generate→fit flow) counted dirty forever, no save could clear it, and
    /// the quit dialog re-armed until 「Discard」.
    pub(crate) fn open_dirty_variants(&self) -> usize {
        if self.variants.is_empty() {
            return 0;
        }
        let Some(rec) = &self.hist.saved_strip else {
            let active = self.active_variant();
            let trivial = self.variants.len() == 1
                && self.active == 0
                && active.is_some_and(|v| {
                    crate::model::strip_is_trivial(
                        v.kind,
                        &v.id,
                        v.name.as_deref(),
                        0,
                    )
                });
            let background = self.variants.len().saturating_sub(1);
            return if background > 0 {
                background
            } else if trivial {
                0
            } else {
                1
            };
        };

        let mut persisted = Vec::with_capacity(rec.others.len() + 1);
        if let Some(kind) = VariantKind::from_store_str(&rec.active_kind) {
            persisted.push(PersistedCard {
                position: rec.active_pos,
                kind,
                id: rec.active_id.as_deref(),
                name: rec.active_name.as_deref(),
                recipe: &self.hist.saved_recipe,
                origin: self.hist.pixels_on_disk.as_deref(),
            });
        }
        persisted.extend(rec.others.iter().enumerate().filter_map(|(i, entry)| {
            let kind = VariantKind::from_store_str(&entry.kind)?;
            Some(PersistedCard {
                position: if i < rec.active_pos { i } else { i + 1 },
                kind,
                id: entry.id.as_deref(),
                name: entry.name.as_deref(),
                recipe: &entry.recipe,
                origin: entry.origin.as_deref(),
            })
        }));

        let mut dirty = 0usize;
        for (position, live) in self.variants.iter().enumerate() {
            let Some(saved) = persisted.iter().find(|saved| {
                Self::persisted_identity_matches(
                    live,
                    position,
                    saved.kind,
                    saved.position,
                    saved.id,
                )
            }) else {
                dirty += 1;
                continue;
            };
            let develop_dirty = position != self.active
                && (dirty_vs(&live.recipe, saved.recipe)
                    || !same_master_opt(live.origin.as_deref(), saved.origin));
            if live.kind != saved.kind || live.name.as_deref() != saved.name || develop_dirty {
                dirty += 1;
            }
        }

        // Persisted cards with no live identity are deletions. Selection drift
        // never enters this comparison, so merely viewing another card stays
        // clean and reopening still returns to the last SAVED active card.
        dirty
            + persisted
                .iter()
                .filter(|saved| {
                    !self.variants.iter().enumerate().any(|(position, live)| {
                        Self::persisted_identity_matches(
                            live,
                            position,
                            saved.kind,
                            saved.position,
                            saved.id,
                        )
                    })
                })
                .count()
    }

    pub(crate) fn inactive_dirty_variants(&self) -> usize {
        // Photos NAVIGATED AWAY FROM count too. Since batch 47 a photo is
        // stashed when only a BACKGROUND variant is dirty, so the quit dialog
        // lists it. Save-all persists each stashed photo's strip record along
        // with its active canvas (v0.22), so these are savable — the count
        // deliberately stays conservative (every stashed background variant,
        // not only those differing from their photo's on-disk record: that
        // comparison would cost a disk read per stash entry per check).
        let stashed: usize = self.open.nav_stash.values().map(|st| st.others.len()).sum();
        self.open_dirty_variants() + stashed
    }

    /// The live strip as a persistable [`store::VariantsRecord`]: `None` when
    /// the strip is trivial ([`crate::model::strip_is_trivial`] — recipe.json
    /// plus pixels.json already say everything, and a record would be pure
    /// sidecar noise). Since R24-3 a lone Original card that carries a NAME or
    /// a minted identity is no longer trivial: the record is the only home
    /// either of those has.
    pub(crate) fn current_strip_record(&self) -> Option<autoshade::store::VariantsRecord> {
        let ak = self.active_variant().map_or(VariantKind::Original, |v| v.kind);
        if crate::model::strip_is_trivial(
            ak,
            self.active_variant().map_or("", |v| v.id.as_str()),
            self.active_variant().and_then(|v| v.name.as_deref()),
            self.variants.len().saturating_sub(1),
        ) {
            return None;
        }
        Some(autoshade::store::VariantsRecord {
            extra: Default::default(),
            v: 1,
            active_kind: ak.store_str().to_string(),
            active_pos: self.active,
            others: self
                .variants
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != self.active)
                .map(|(_, v)| autoshade::store::VariantEntry {
                    extra: Default::default(),
                    kind: v.kind.store_str().to_string(),
                    recipe: v.recipe.clone(),
                    origin: v.origin.clone(),
                    // Hop 1 of 6 (R24-2): the live strip's identities + names
                    // to disk. An EMPTY id is written as absent — the record
                    // must not claim an identity the card does not have.
                    id: variant_id_field(&v.id),
                    name: v.name.clone(),
                })
                .collect(),
            active_id: self.active_variant().and_then(|v| variant_id_field(&v.id)),
            active_name: self.active_variant().and_then(|v| v.name.clone()),
        })
    }

    /// Persist the open photo's strip beside recipe.json/pixels.json and
    /// advance the mirror. Trivial strip ⇒ clear (no noise file). An error
    /// leaves the mirror untouched, so the unsaved protection stays armed.
    /// Test-only since the single-generation commit (L03): production saves
    /// stage the strip INTO [`autoshade::store::commit_develop`] and advance
    /// the mirror on the commit's success — this remains the strip-half
    /// primitive the guard tests drive in isolation.
    #[cfg(test)]
    pub(crate) fn persist_strip(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        match self.current_strip_record() {
            Some(rec) => {
                autoshade::store::write_variants(path, &rec)?;
                self.hist.saved_strip = Some(rec);
            }
            None => {
                autoshade::store::clear_variants(path)?;
                self.hist.saved_strip = None;
            }
        }
        Ok(())
    }

    /// The active variant, if a photo is open.
    pub(crate) fn active_variant(&self) -> Option<&Variant> {
        self.variants.get(self.active)
    }

    /// The on-disk PIXEL SOURCE the active variant renders / retouches /
    /// exports FROM. Any variant whose pixels are baked into a ./out raster — a
    /// reimagine or fill (Generated), OR an in-place heal/clone/denoise on ANY variant —
    /// carries that full-resolution artifact in `origin` and renders from it;
    /// a pristine source-based variant (原片 / 反推 with no pixel retouch) has
    /// `origin = None` and renders from `src_path` (the RAW / loaded image)
    /// developed by the recipe. Retouch and export key off THIS — not raw
    /// `src_path` — so what exports matches what's on screen (WYSIWYG), never
    /// the untouched negative underneath a generated / retouched rendition.
    pub(crate) fn active_source_path(&self) -> Option<PathBuf> {
        match self.active_variant() {
            Some(v) => v.origin.clone().or_else(|| self.src_path.clone()),
            None => self.src_path.clone(),
        }
    }

    /// Does the active card develop an AI-generated raster — the pristine
    /// 「✨ AI generated」 card or a 「✎ Edited AI image」 over the same
    /// pixels ([`VariantKind::on_ai_pixels`])? Such a canvas carries no
    /// calibration (the raster already has the camera look, the lens
    /// geometry and a baked white balance), projects to no Lightroom XMP (no
    /// sidecar reproduces AI pixels), and is what the `pixels.json` flag
    /// records. The one place the two kinds part — the ✨ card is immutable,
    /// the ✎ card is where its edits live — asks `== Generated` directly.
    pub(crate) fn active_on_ai_pixels(&self) -> bool {
        self.active_variant().is_some_and(|v| v.kind.on_ai_pixels())
    }

    /// Uniform failure disclosure for a foreground persist compound — the
    /// single mapper the NoWait develop-lock wrappers report through
    /// (arch item b: five hand-rolled copies had already drifted into
    /// wording bugs). TYPED: `WouldBlock` really is "another AutoShade
    /// process owns this develop right now", so it gets the caller's busy
    /// wording with its retry hint; any other error is real I/O and must
    /// not wear the busy costume — a full disk used to read as "another
    /// process is working on this photo".
    pub(crate) fn persist_postponed(
        &mut self,
        e: &std::io::Error,
        busy_key: &'static str,
        args: &[(&str, &str)],
    ) {
        let es = e.to_string();
        let mut a: Vec<(&str, &str)> = args.to_vec();
        a.push(("err", &es));
        let t = if e.kind() == std::io::ErrorKind::WouldBlock {
            trf(self.lang, busy_key, &a)
        } else {
            trf(self.lang, "not saved — a develop-store write failed: {err}", &a)
        };
        self.status = t.clone();
        self.toast(ToastKind::Error, t);
    }

    /// Every crop_mode flip goes through here (L13-1): `pan` is stored in
    /// the CURRENT window's coordinates — the committed crop normally, the
    /// full frame while the crop tool is open (`view_uv`) — so the VALUE
    /// must be rebased when the window changes. Left alone, entering the
    /// tool reinterpreted a crop-relative pan as full-frame and the
    /// viewport landed outside the crop box at higher zoom.
    pub(crate) fn set_crop_mode(&mut self, on: bool) {
        if self.crop.mode == on {
            return;
        }
        if let Some(c) = &self.recipe.crop {
            let (l, t) = (c.left.min(c.right), c.top.min(c.bottom));
            let (w, h) =
                ((c.right - c.left).abs().max(1e-6), (c.bottom - c.top).abs().max(1e-6));
            self.view.pan = if on {
                // crop-window coords → full-frame coords
                egui::vec2(l + self.view.pan.x * w, t + self.view.pan.y * h)
            } else {
                // full-frame coords → crop-window coords
                egui::vec2((self.view.pan.x - l) / w, (self.view.pan.y - t) / h)
            };
            // view_uv re-clamps against zoom each frame; keep it sane here.
            self.view.pan = egui::vec2(self.view.pan.x.clamp(0.0, 1.0), self.view.pan.y.clamp(0.0, 1.0));
        }
        self.crop.mode = on;
    }

    /// Recipe snapshot path for version `n` — `v<n>.recipe.json` in the photo's
    /// central develop dir (gap batch G, ≈ Lightroom virtual copies: cheap
    /// parametric versions, never touching the library or the working
    /// `recipe.json`).
    pub(crate) fn version_path(src: &std::path::Path, n: u32) -> PathBuf {
        autoshade::store::version_target(src, n)
    }

    /// Re-point every stored mask index (selection, colour-range sampler,
    /// redraw target) after the mask list changed shape — delete, drag-drop
    /// reorder, ⬆/⬇ swap. One place, so an index-carrying tool cannot be
    /// forgotten: a dangling index used to make the range sampler write into
    /// whatever mask slid under it.
    pub(crate) fn remap_mask_indices(&mut self, f: impl Fn(usize) -> Option<usize>) {
        let sel_before = self.masks.sel_mask;
        self.masks.sel_mask = self.masks.sel_mask.and_then(&f);
        // Component selection is meaningful only while the SAME mask stays
        // selected under the same index-space; a moved/vanished mask drops it
        // (its component list travelled with the mask, but every canvas tool
        // re-derives from sel_mask — a stale pair here was the same class of
        // bug as the dangling range sampler).
        if self.masks.sel_mask != sel_before || self.masks.sel_mask.is_none() {
            self.masks.sel_component = None;
        }
        self.tools.range_picking = self.tools.range_picking.and_then(&f);
        self.masks.placing_mask = self.masks.placing_mask.take().and_then(|(k, t)| match t {
            PlaceTarget::NewMask => Some((k, PlaceTarget::NewMask)),
            // A vanished redraw target degrades to a NEW mask placement (the
            // pre-v0.22 `Option<usize>` behaviour, kept); a vanished
            // component target must NOT — appending a component to nothing
            // has no sensible fallback, so the arm disarms.
            PlaceTarget::Redraw(i) => {
                Some((k, f(i).map_or(PlaceTarget::NewMask, PlaceTarget::Redraw)))
            }
            PlaceTarget::Component(i, mode) => {
                f(i).map(|j| (k, PlaceTarget::Component(j, mode)))
            }
        });
        // The pending-rename buffer is index-carrying too: after a delete /
        // reorder its seed-guard (name-at-seed must still match) correctly
        // refused to cross-commit — but the SAME mask at its new index then
        // failed the match as well, and the typed rename silently died.
        self.masks.mask_name_buf =
            self.masks.mask_name_buf.take().and_then(|(j, o, b)| Some((f(j)?, o, b)));
        // The mask-brush session carries its TARGET index: 「Apply」 bakes the
        // stroke into `mask_brush.0`, so after a delete/reorder it painted
        // whatever mask slid under the stale slot — the dangling-range-sampler
        // class again, writing pixel weights instead of range bounds. A moved
        // target follows its mask; a vanished one ends the session exactly
        // like Esc (gray buffer AND paint canvas — see disarm_tools: fill/heal
        // would inherit the strokes as a phantom retouch selection). Not a
        // new-mask fallback: that would resurrect a just-deleted mask.
        if let Some((target, erase)) = self.masks.mask_brush {
            match target.map(&f) {
                None => self.masks.mask_brush = Some((None, erase)),
                Some(Some(j)) => self.masks.mask_brush = Some((Some(j), erase)),
                Some(None) => {
                    self.end_mask_brush();
                }
            }
        }
    }

    /// Rescan the photo's develop dir for version snapshots (cached in
    /// `self.cards.versions`; called on photo open and after saving a version — NOT
    /// every frame). The advisory name/provenance sidecar is read in the same
    /// pass and RESTRICTED to numbers that are actually listed: a crash
    /// between a delete's sweep and its metadata drop can leave a record for
    /// a number nothing lists any more, and that record must never surface as
    /// a phantom row (it can never be re-attached either — the number is
    /// burned in the deleted-version registry).
    pub(crate) fn refresh_versions(&mut self) {
        self.cards.versions.clear();
        self.cards.version_meta.clear();
        let Some(src) = self.src_path.as_deref() else { return };
        // ONE store query for the photo's whole edit-state list (R24-4
        // `store::list_edits`), which joins the version family with its
        // advisory metadata — including the listed-numbers-only restriction
        // this used to spell for itself. The VARIANT half is deliberately
        // dropped here: it describes the LAST SAVED strip, while
        // `self.variants` is the live one (pushes, deletes and renames that
        // have not been saved yet), and an editor must never renumber its own
        // cards from a stale record. For every non-GUI surface that half IS
        // the list.
        for e in autoshade::store::list_edits(src) {
            if let autoshade::store::EditStateKind::Version { n, from_kind, from_id, source } =
                e.state
            {
                self.cards.versions.push(n);
                self.cards.version_meta.insert(
                    n,
                    autoshade::store::VersionMetaEntry {
                        n,
                        name: e.name,
                        from_kind,
                        from_id,
                        origin: source,
                    },
                );
            }
        }
    }

    /// Does version metadata `m` name the given variant as its source
    /// (R24-2)? The ID is authoritative when both sides have one — a card
    /// renamed or re-kinded since the snapshot still matches. KIND is the
    /// fallback for a record written before ids existed on this photo (or
    /// for a card whose own id was never recorded). A record with neither —
    /// or no record at all — is UNATTRIBUTED and matches nothing: the filter
    /// is "only this variant", and quietly including everything unknown
    /// would make it a checkbox that does not filter.
    pub(crate) fn version_is_from(
        m: Option<&autoshade::store::VersionMetaEntry>,
        active_id: &str,
        active_kind: Option<VariantKind>,
    ) -> bool {
        let Some(m) = m else { return false };
        if let Some(id) = m.from_id.as_deref()
            && !active_id.is_empty()
        {
            return id == active_id;
        }
        match (m.from_kind.as_deref(), active_kind) {
            (Some(k), Some(ak)) => k == ak.store_str(),
            _ => false,
        }
    }

    /// Commit a pending version rename to ITS OWN photo and number — the
    /// version half of the `commit_mask_name_buf` rule (U10): a name still
    /// sitting in its TextEdit when the user hits Ctrl+S, clicks another
    /// photo or quits must reach the disk, not die with the buffer. Keyed by
    /// (photo, number), both captured at seed time, so a boundary that has
    /// already re-pointed `src_path` cannot rename the incoming photo's v1.
    /// The write is advisory: a failure is disclosed and the buffer is left
    /// alone so the next boundary retries.
    pub(crate) fn commit_version_name_buf(&mut self) {
        let Some((photo, n, seed, buf)) = self.cards.version_name_buf.clone() else { return };
        if buf == seed {
            return;
        }
        // NoWait wrapper around the store's own Wait lock (the delete_version
        // rule): a develop held by another process must report here, never
        // freeze the UI thread on a name.
        match autoshade::store::with_develop_lock(
            &photo,
            autoshade::store::DevelopLockMode::NoWait,
            || autoshade::store::set_version_name(&photo, n, Some(buf.as_str())),
        ) {
            Ok(()) => {
                let named = Some(buf.trim().to_string()).filter(|s| !s.is_empty());
                // Re-seed rather than clear: the row is very likely still on
                // screen, and a SECOND rename in the same row has to compare
                // against what we just wrote, not the original.
                self.cards.version_name_buf = Some((photo.clone(), n, buf.clone(), buf));
                if self.src_path.as_deref() == Some(photo.as_path()) {
                    self.cards.version_meta
                        .entry(n)
                        .or_insert_with(|| autoshade::store::VersionMetaEntry {
                            n,
                            ..Default::default()
                        })
                        .name = named;
                }
            }
            Err(e) => {
                self.persist_postponed(&e, "Renaming v{n} failed: {err}", &[("n", &n.to_string())]);
            }
        }
    }

    /// Save the CURRENT develop as the next numbered version snapshot.
    pub(crate) fn save_version(&mut self) {
        // A typed-but-uncommitted name belongs in the snapshot — every
        // persistence entry point flushes all of them (the U10 rule; L27).
        self.commit_pending_names();
        // The immutability rule's boundary discipline: an edit the frame
        // hook has not moved yet forks NOW, so the snapshot is attributed
        // to the ✎ card that holds it.
        self.fork_edited_card();
        let lang = self.lang;
        // The pristine ✨ card has no develop to snapshot — its recipe is
        // neutral by the immutability rule, so "＋ Save as version" would
        // store an empty recipe that restores to nothing (R24-2). An ✎ card
        // does: its develop over the AI pixels is what the snapshot holds
        // (stripped of calibration like every snapshot taken on AI pixels,
        // and re-stamped if it ever lands on the negative — R24-3).
        if self.active_variant().is_some_and(|v| v.kind == VariantKind::Generated) {
            let t = tr(
                lang,
                "A pristine ✨ AI generated card has no develop to snapshot — move a slider to start an ✎ Edited AI image card, or run 「Reverse-fit」",
            );
            self.status = t.into();
            self.toast(ToastKind::Error, t);
            return;
        }
        let Some(src) = self.src_path.clone() else { return };
        // Attribution for the record below, read before the lock closure
        // borrows `self`: which card this snapshot is a picture OF (R24-2).
        let from_kind = self.active_variant().map(|v| v.kind.store_str().to_string());
        let from_id = self.active_variant().and_then(|v| variant_id_field(&v.id));
        // ONE NoWait lock across claim → raster freeze → recipe write:
        // another process's delete_version sweeping the just-claimed slot
        // mid-snapshot is a real interleave — and this is a foreground
        // button, so busy must report, not hang the UI.
        let locked = autoshade::store::with_develop_lock(
            &src,
            autoshade::store::DevelopLockMode::NoWait,
            || -> std::io::Result<()> {
        // ATOMICALLY reserved number (store::claim_version): disk-list+1
        // raced the auto-backup (and the web/CLI) to the same N — the later
        // publish silently replaced the earlier snapshot.
        let (n, vpath) = match autoshade::store::claim_version(&src) {
            Ok(v) => v,
            Err(e) => {
                let t = trf(lang, "Save version failed: {err}", &[("err", &e.to_string())]);
                self.status = t.clone();
                self.toast(ToastKind::Error, t);
                return Ok(());
            }
        };
        // Freeze referenced rasters under THIS version (the backup gate's
        // pair): a version whose recipe pointed at another version's frozen
        // raster (load v3 → save as v4) broke the moment v3 was deleted.
        let dev = autoshade::store::develop_dir(&src);
        let mut snap = self.recipe.clone();
        if let Err(e) = autoshade::store::snapshot_rasters(&mut snap, &dev, n) {
            let _ = std::fs::remove_file(&vpath); // release the claim
            let t = trf(lang, "Save version failed: {err}", &[("err", &e.to_string())]);
            self.status = t.clone();
            self.toast(ToastKind::Error, t);
            return Ok(());
        }
        let res = autoshade::pipeline::write_recipe(&src, &snap, Some(vpath.clone()), autoshade::diag::stderr());
        if res.is_err() {
            // Release the claimed slot AND this call's frozen rasters — an
            // empty version must not pollute the list after a failed write.
            autoshade::store::rollback_frozen_rasters(&dev, n);
            let _ = std::fs::remove_file(&vpath);
        }
        match res {
            Ok(p) => {
                // Provenance BEFORE the rescan, so the fresh row already
                // carries「· 来自 …」. Best-effort by contract (R24-2): the
                // snapshot is on disk and complete — an advisory record that
                // could not be written must not turn a successful save into a
                // failure, it only leaves the version unattributed. The
                // develop lock is already held (reentrant).
                let _ = autoshade::store::record_version_meta(
                    &src,
                    &autoshade::store::VersionMetaEntry {
                        n,
                        name: None,
                        from_kind: from_kind.clone(),
                        from_id: from_id.clone(),
                        origin: Some(autoshade::store::VERSION_ORIGIN_USER.to_string()),
                    },
                );
                self.refresh_versions();
                self.status = trf(
                    lang,
                    "Version v{n} saved → {path}",
                    &[("n", &n.to_string()), ("path", &p.display().to_string())],
                );
            }
            Err(e) => {
                let t = trf(lang, "Save version failed: {err}", &[("err", &e.to_string())]);
                self.status = t.clone();
                self.toast(ToastKind::Error, t);
            }
        }
                Ok(())
            },
        );
        if let Err(e) = locked {
            self.persist_postponed(&e, "Save version failed: {err}", &[]);
        }
    }

    /// Load version `n` as the working recipe (one undo step, like AI Analyze).
    pub(crate) fn load_version(&mut self, n: u32) {
        let lang = self.lang;
        let Some(src) = self.src_path.clone() else { return };
        let p = Self::version_path(&src, n);
        // ONE NoWait lock across the whole snapshot read: the version file,
        // the mask re-anchor probes and the raster detach copies are separate
        // file touches, and delete_version sweeps v<N>.* rasters FIRST and
        // removes the recipe LAST — an unlocked load could parse a recipe
        // whose rasters were already swept and the next Ctrl+S then persists
        // dangling paths. Foreground button ⇒ NoWait (the save_version rule);
        // a busy develop reports and leaves the canvas untouched. The clamp
        // toast and the pre-era repair (a seconds-long RAW decode) run
        // OUTSIDE the lock — the read-under-lock / decide-outside split
        // read_saved_develop already draws.
        let locked = autoshade::store::with_develop_lock(
            &src,
            autoshade::store::DevelopLockMode::NoWait,
            || -> std::io::Result<anyhow::Result<(EditRecipe, autoshade::recipe::ClampSummary)>> {
                Ok((|| {
                    // A stale UI row can still name a half-deleted version:
                    // complete pending recovery (killed deletes included)
                    // under OUR lock before reading — reentrant, so this
                    // costs no second kernel acquisition (L03).
                    let _ = autoshade::store::recover_orphan_baks(&src);
                    let s =
                        autoshade::store::read_text_capped(&p, autoshade::store::MAX_STORE_JSON)?;
                    let mut r = serde_json::from_str::<EditRecipe>(&s)?;
                    let dropped = r.clamp();
                    // Snapshots name their rasters by bare file name, like
                    // the working recipe — re-anchor them to the develop dir.
                    if let Some(base) = p.parent() {
                        autoshade::store::resolve_mask_paths(&mut r, base);
                    }
                    // Then DETACH from the snapshot: the frozen v<N>.*.png
                    // files belong to that version and `delete_version`
                    // sweeps them, so a canvas pointing at them lost its
                    // masks the moment the user deleted the version it was
                    // loaded from — and the next save persisted the dangling
                    // path. The loaded state gets its own claimed copies.
                    autoshade::store::detach_rasters(&src, &mut r, "mask-restored");
                    Ok((r, dropped))
                })())
            },
        );
        let outcome = match locked {
            Ok(inner) => inner,
            Err(e) => {
                // The uniform busy/IO mapper every foreground compound uses.
                return self.persist_postponed(
                    &e,
                    "Load v{n} failed: {err}",
                    &[("n", &n.to_string())],
                );
            }
        };
        match outcome {
            Ok((mut r, dropped)) => {
                if !dropped.is_empty() {
                    // Same W20 disclosure as the open restore: a snapshot
                    // past the caps loads minus edits, never silently —
                    // and all four loss kinds, like the open toast.
                    let t = trf(
                        lang,
                        "recipe limits discarded {n} mask(s), {m} component(s), {c} curve point(s) and {s} string byte(s) on restore — the saved file exceeds the app's caps",
                        &[
                            ("n", &dropped.dropped_masks.to_string()),
                            ("m", &dropped.dropped_components.to_string()),
                            ("c", &dropped.truncated_curve_points.to_string()),
                            ("s", &dropped.truncated_string_bytes.to_string()),
                        ],
                    );
                    self.toast(ToastKind::Error, t);
                }
                // A Generated variant's pixels already carry the camera look
                // AND the lens corrections — a source-based snapshot's
                // calibration would cook both twice (same strip rule as the
                // open-restore and Analyze paths).
                // BOTH directions of the calibration rule, through their one
                // owner (R24-3 `reconcile_snapshot_calibration`): stripped
                // onto a pixel-state card, and — the direction nothing
                // covered — re-stamped when a snapshot taken OFF one lands on
                // the negative with no camera base look at all. The photo's
                // estimate is a RAW decode, so it is paid lazily, inside the
                // branch that needs it.
                let restamped = reconcile_snapshot_calibration(
                    &mut r,
                    self.active_on_ai_pixels(),
                    || {
                        let (k, t) = autoshade::pipeline::fresh_as_shot_wb(&src);
                        (
                            autoshade::pipeline::fresh_canvas_start(&src, &autoshade::diag::photo(&src)),
                            autoshade::pipeline::fresh_lens_profile(&src),
                            k.zip(t),
                        )
                    },
                );
                // A version snapshot is a saved recipe like any other, and
                // it predates the era stamp by definition — loading one put
                // an unrepaired washed curve straight onto the canvas and
                // into everything exported from it. AFTER the generated
                // strip: a generated canvas keeps no base curve, so there is
                // nothing to repair (paying the estimate just to discard it,
                // and disclosing a re-estimate the strip then deleted, were
                // both wrong). The note rides the SAME status write as the
                // load announcement — a separate earlier write was replaced
                // one statement later, so the promised disclosure never
                // survived to a single rendered frame.
                let relook =
                    autoshade::pipeline::repair_pre_era_base_curve(&src, &mut r).is_some();
                // …and the coordinate frame, for the same reason and from the
                // same file: a `v<N>.recipe.json` written before v0.30.0 holds
                // its crop and masks in the SENSOR frame of a rotated RAW.
                // Ungated by the generated strip above (a pixel-state card
                // still has geometry) and toasted rather than folded into the
                // status line, so the raster caveat has room.
                let reframe = autoshade::pipeline::migrate_recipe_coord_frame(&src, &mut r);
                self.recipe = r;
                // Read from disk: "Keep skin natural" is decided afresh for
                // this card (`panels/portrait.rs`) — opening never changes a
                // value.
                self.decide_keep_skin_for_card();
                self.resync_recipe_display();
                self.dirty = true;
                // A snapshot loaded onto the pristine ✨ card is an edit like
                // any other: it lands on a new ✎ card (the immutability rule),
                // and the load announcement below names that card's canvas.
                self.fork_edited_card();
                let loaded = trf(lang, "Loaded version v{n} — Ctrl+Z returns to before the load", &[("n", &n.to_string())]);
                if restamped {
                    // Said out loud, in the same channel the other
                    // calibration correction uses: the snapshot on disk holds
                    // no camera base look, and the canvas now does.
                    self.toast(
                        ToastKind::Success,
                        tr(
                            lang,
                            "this snapshot was taken on a generated variant, whose look lives in its pixels — the photo's own camera base look was applied so it renders on the negative",
                        )
                        .to_string(),
                    );
                }
                match reframe {
                    Ok(Some(c)) => self.toast(ToastKind::Success, coord_migration_sentence(lang, c)),
                    // Said as an error: the snapshot loaded, but its geometry
                    // is off the picture until the raster can be re-written.
                    Err(e) => self.toast(
                        ToastKind::Error,
                        coord_migration_failure_sentence(lang, &e.to_string()),
                    ),
                    Ok(None) => {}
                }
                self.status = if relook {
                    // The GUI's OWN sentence, localized: the engine note is
                    // English prose meant for the CLI/HTTP surfaces, and
                    // embedding it verbatim put raw English into a localized
                    // status line.
                    format!(
                        "{loaded} — {}",
                        tr(
                            lang,
                            "camera base look re-estimated — this photo was saved by an earlier version, whose estimate of the camera's tone this version replaces",
                        )
                    )
                } else {
                    loaded
                };
            }
            Err(e) => {
                let t = trf(
                    lang,
                    "Load v{n} failed: {err}",
                    &[("n", &n.to_string()), ("err", &e.to_string())],
                );
                self.status = t.clone();
                self.toast(ToastKind::Error, t);
            }
        }
    }

    /// Open one of the gallery photos by index (keeps the thumbnail highlighted).
    pub(crate) fn open_gallery_index(&mut self, idx: usize) {
        if self.busy {
            return;
        }
        let Some(path) = self.gal.gallery.get(idx).cloned() else { return };
        self.gal.selected = Some(idx);
        self.open_path(path);
    }

    /// Scan `dir` (recursively) for sources off the UI thread and replace the
    /// gallery — folders can hold thousands of RAWs, so this never blocks paint.
    pub(crate) fn open_folder(&mut self, dir: PathBuf) {
        if self.busy {
            return;
        }
        let lang = self.lang;
        self.busy = true;
        self.status = trf(lang, "scanning {path} …", &[("path", &dir.display().to_string())]);
        self.spawn_worker(
            move || {
                let res = autoshade::pipeline::find_sources_counted(&dir)
                    .map(|(list, skipped)| (dir, list, skipped));
                Msg::Folder(Box::new(res))
            },
            |e| Msg::Folder(Box::new(Err(e))),
        );
    }

    /// The live working state as an [`UndoStep`]: current recipe + the active
    /// variant's pixel identity. Arc/path clones only — never pixel copies.
    pub(crate) fn current_step(&self) -> UndoStep {
        UndoStep {
            recipe: self.recipe.clone(),
            base: self.active_variant().and_then(|v| v.base.clone()),
            origin: self.active_variant().and_then(|v| v.origin.clone()),
            keep_skin: self.skin_switch(),
        }
    }

    /// Reset undo history — call when a brand-new photo opens, when the active
    /// variant changes (you can't undo across either), or when a photo GOES
    /// AWAY: a failed fresh open leaves no canvas for its steps to apply to,
    /// and an undo there restored one that no longer existed. `committed`
    /// becomes the current head — with no photo open that is the empty step,
    /// which is exactly what stops the next settled frame pushing a phantom.
    pub(crate) fn reset_history(&mut self) {
        self.hist.committed = self.current_step();
        self.hist.undo_stack.clear();
        self.hist.redo_stack.clear();
    }

    /// Commit the current state as one undo step RIGHT NOW (when it differs
    /// from the head). Used directly by programmatic pixel swaps (the retouch
    /// bake-in): waiting for `commit_if_settled` left a one-frame window where
    /// a same-frame Ctrl+Z saw only the older history and undid a recipe step
    /// while keeping the freshly retouched pixels installed.
    pub(crate) fn commit_now(&mut self) {
        let cur = self.current_step();
        if cur.recipe != self.hist.committed.recipe || !cur.same_pixels(&self.hist.committed) {
            self.hist.undo_stack.push(std::mem::replace(&mut self.hist.committed, cur));
            if self.hist.undo_stack.len() > 100 {
                self.hist.undo_stack.remove(0); // cap history memory
            }
            self.hist.redo_stack.clear();
        } else {
            // A flip of the skin switch that changed no value belongs to the
            // step on screen, so a later undo/redo hands it back as set.
            self.hist.committed.keep_skin = cur.keep_skin;
        }
    }

    /// Commit the current state as ONE undo step once the edit gesture settles
    /// (pointer released) — dragging a slider is one step, not one per frame.
    /// Programmatic edits (Analyze, Reset) land here on the next frame.
    pub(crate) fn commit_if_settled(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_down()) {
            self.commit_now();
        }
    }

    /// Recipe replaced wholesale (undo/redo/Reset/version load/AI apply): the
    /// derived display state must follow, or the panel keeps showing an AI
    /// rationale / verdict that describes a recipe no longer on screen — and
    /// an ARMED index-carrying tool (range sampler, ↻ Redraw) would fire into
    /// whatever mask now happens to sit at its remembered index.
    /// One tool at a time on the canvas — the SINGLE owner of the disarm
    /// list. Ten arming sites used to hand-copy these assignments, and the
    /// comments at two of them record real bugs born from copies drifting;
    /// every arming site now calls this before setting its own flag.
    /// Transient gesture anchors die with the tools: a stale place_start or
    /// crop_drag used to hijack the next drag. clone_src (the sampled source
    /// pin) deliberately survives — samples stay for resuming, like paint.
    pub(crate) fn disarm_tools(&mut self) {
        self.set_crop_mode(false);
        self.masks.paint_mode = false;
        self.tools.clone_mode = false;
        self.tools.wb_picking = false;
        self.tools.range_picking = None;
        self.tools.point_color_picking = false;
        self.tools.lens_focus_picking = false;
        self.masks.placing_mask = None;
        self.masks.place_start = None;
        self.crop.drag = None;
        self.masks.mask_drag = None;
        self.masks.paint_last = None;
        self.end_mask_brush();
        // A preset change armed for the crop tool must die with it — the
        // flag survived Esc / Done / a hold-B detour and rewrote the box as
        // a surprise on the next crop entry (CX5-4).
        self.crop.aspect_pending = false;
    }

    /// End a live mask-brush session (the Esc teardown, extracted from
    /// `disarm_tools` so the three former hand-copies stay one owner):
    /// strokes live in the canvas + gray buffer only until 「Apply」 bakes
    /// them into a claimed raster, so dropping both IS the cancel. The
    /// canvas is cleared too — fill/heal would otherwise inherit the
    /// brush-mask strokes as a phantom retouch selection. Returns whether a
    /// session was actually live, so a caller whose teardown is NOT
    /// user-initiated can disclose the discarded strokes.
    pub(crate) fn end_mask_brush(&mut self) -> bool {
        if self.masks.mask_brush.take().is_some() {
            self.masks.mask_brush_gray = None;
            self.masks.paint_mode = false;
            self.clear_mask();
            // The owner's area the session displaced comes back (2026-09-27).
            self.unstash_owner_area();
            true
        } else {
            false
        }
    }

    /// Selection moved: every INDEX-ARMED tool whose target is no longer
    /// the selected mask dies with it — `remap_mask_indices`' twin (that
    /// one covers list SHAPE changes; this one covers which row is live).
    /// ↻ Redraw / add-component are the silent pair: their arming buttons
    /// live inside the selected-mask block and the canvas hint discards the
    /// PlaceTarget, so an armed Redraw(j) reads exactly like a fresh
    /// gradient and the next drag rewrites mask j while the user is looking
    /// at another row. A `NewMask` placement and the non-mask tools (crop,
    /// WB picker, clone) are deliberately spared — they are not bound to a
    /// selection. Returns whether a BRUSH session died (its strokes are
    /// user paint), for the caller's disclosure.
    pub(crate) fn disarm_selection_bound_tools(&mut self, keep: Option<usize>) -> bool {
        if matches!(
            self.masks.placing_mask,
            Some((_, PlaceTarget::Redraw(j) | PlaceTarget::Component(j, _))) if Some(j) != keep
        ) {
            self.masks.placing_mask = None;
            self.masks.place_start = None;
        }
        // The colour sampler drops unconditionally — its 💧 label lives on
        // the row that was just left (the first fix of this class).
        self.tools.range_picking = None;
        if matches!(self.masks.mask_brush, Some((Some(j), _)) if Some(j) != keep) {
            return self.end_mask_brush();
        }
        false
    }

    /// Any canvas tool armed? (the once-hand-written OR list)
    pub(crate) fn tool_armed(&self) -> bool {
        self.crop.mode
            || self.masks.paint_mode
            || self.tools.clone_mode
            || self.tools.wb_picking
            || self.tools.range_picking.is_some()
            || self.tools.point_color_picking
            || self.masks.placing_mask.is_some()
    }

    /// Commit a pending mask-rename buffer to its own mask (the same
    /// seed-guard the panel's row-switch commit uses: only while that mask's
    /// name still equals the seed-time snapshot). Focus-independent Ctrl+S
    /// runs while the TextEdit still holds focus — without this it saved the
    /// old name and the advanced baseline dropped the rename.
    pub(crate) fn commit_mask_name_buf(&mut self) {
        if let Some((j, orig, buf)) = self.masks.mask_name_buf.clone()
            && let Some(prev) = self.recipe.masks.get_mut(j)
            && prev.name == orig
            && buf != orig
        {
            prev.name = buf.clone();
            // Re-seed so the panel's own lost-focus commit stays a no-op.
            self.masks.mask_name_buf = Some((j, buf.clone(), buf));
        }
    }

    pub(crate) fn resync_recipe_display(&mut self) {
        self.rationale = self.recipe.rationale.clone();
        // Typed notes described the PREVIOUS rationale — a swap without a
        // clear would render a stale zh text over a different develop. The
        // landings that produce fresh notes install them AFTER this call.
        self.rationale_notes.clear();
        self.verdict = None;
        // A wholesale recipe swap (undo/redo/Reset/version load/AI apply)
        // disarms EVERY canvas tool — an armed index-carrying tool would fire
        // into whatever now sits at its remembered index, and a live
        // crop/rotate drag would re-apply its stale start angle over the
        // freshly restored recipe on the very next drag frame.
        self.disarm_tools();
        self.masks.mask_name_buf = None; // stale (index, text) must not cross-commit
        // A curve-point drag in flight when the recipe is swapped (Ctrl+Z
        // mid-drag) kept mutating point i of the RESTORED curve on the next
        // drag frame (M17).
        self.curve_drag = None;
        // A wholesale recipe swap can shrink the mask list — an out-of-range
        // selection must not linger (every consumer bounds-checks, but the
        // panel would silently deselect anyway; do it deterministically).
        // Same-index-different-mask after undo is a known cosmetic residue —
        // mask identity isn't tracked across history steps.
        self.masks.sel_mask = self.masks.sel_mask.filter(|&i| i < self.recipe.masks.len());
        // The component selection rides on the mask's OWN list, which the
        // swap can also reshape — same deterministic bound.
        self.masks.sel_component = self.masks.sel_component.filter(|&c| {
            self.masks.sel_mask
                .and_then(|i| self.recipe.masks.get(i))
                .is_some_and(|m| c < m.components.len())
        });
    }

    /// Apply a history step: recipe always; the active variant's pixels only
    /// when the step's identity differs (so plain slider undos stay pixel-free
    /// and O(1)). A restored `base: None` reverts a retouched source variant
    /// to the shared source neutral.
    pub(crate) fn apply_step(&mut self, step: UndoStep, ctx: &egui::Context) {
        let pixels_differ = !step.same_pixels(&self.hist.committed);
        let keep_skin = step.keep_skin;
        self.hist.committed = step.clone();
        self.recipe = step.recipe;
        // The history is a READER too: a step recorded while the canvas was
        // washed reinstalls the pre-era pair on undo/redo — and the actor
        // that later promoted the canvas (Reset, Analyze, load_version,
        // Ctrl+S) cannot know which stack entries still hold it (the
        // save-time restamp covers only the pair IT replaced). Repairing on
        // the way back keeps every door to the canvas behind one rule.
        // Cost honesty: when the OPEN succeeded, the worker primed the
        // memo and this is a lookup. But washed steps enter history
        // precisely when the open-time estimate was an INABILITY — uncached
        // by design — so the first traversal here IS the retry point and
        // pays one estimate (the accepted class shared with load_active and
        // the open gate); its answer memoizes and the rest of the washed
        // history heals by lookup. Era-2 installs short-circuit before any
        // I/O; the Generated guard matches every other ordering site; and a
        // CROSS-PHOTO in-flight open is refused — src_path already points
        // at the INCOMING photo while this history belongs to the outgoing
        // one (the live-canvas override records the same hazard), so
        // repairing would estimate the WRONG photo onto this canvas. A
        // same-path preview-resolution re-decode (request AND fact: the
        // keep_recipe request verified against the recorded open_same_path)
        // keeps the pair coherent AND its keep arm preserves history —
        // refusing there made a washed install DURABLE, so it is admitted.
        // A same-path FRESH reopen is NOT: its rebuild discards this repair,
        // so admitting it bought a concurrent decode and a false success
        // toast. `committed` follows the heal, or the next commit_now would
        // push the washed head straight back onto the stack.
        //
        // DEFENCE IN DEPTH since the keyboard undo took the buttons' busy
        // gate: `open_in_flight` implies `busy` (open_path early-returns while
        // busy, sets both, and clears both in one handler), and undo/redo are
        // this fn's only callers — so no live caller reaches here mid-flight
        // any more. The gate stays because ungating the keyboard would
        // resurrect every scenario above, and the test drives it directly.
        if (!self.open.open_in_flight || (self.open.open_same_path && self.cards.keep_recipe))
            && self.variants.get(self.active).is_none_or(|v| v.kind.is_source_based())
            && let Some(p) = self.src_path.clone()
            && autoshade::pipeline::repair_pre_era_base_curve(&p, &mut self.recipe).is_some()
        {
            self.hist.committed.recipe = self.recipe.clone();
            if let Some(v) = self.variants.get_mut(self.active) {
                v.recipe = self.recipe.clone();
            }
            let lang = self.lang;
            self.toast(
                ToastKind::Success,
                tr(
                    lang,
                    "camera base look re-estimated — this photo was saved by an earlier version, whose estimate of the camera's tone this version replaces",
                )
                .to_string(),
            );
        }
        if pixels_differ {
            if let Some(v) = self.variants.get_mut(self.active) {
                v.base = step.base;
                v.origin = step.origin;
            }
            self.refresh_active_pixels(ctx);
            // The quietest door: undoing to a SUPERSEDED master installs a
            // raster the preference cannot re-decode (the resolution switch
            // repoints only the Arc the canvas held), and undo/redo wrote no
            // status at all — so the combo was left to imply the canvas had
            // followed it. BOTH directions: the redo back to a reachable
            // canvas has to retract the claim, or it stands there false.
            self.set_canvas_status("restored the canvas pixels");
        }
        self.dirty = true;
        self.resync_recipe_display();
        // The skin switch comes back as the step had it; a step past the
        // skin limits is then put back as it was (Portraits).
        self.restore_skin_switch(keep_skin);
        self.release_skin_for_history();
    }

    /// Rebuild the pixel-derived per-variant state after the active variant's
    /// base changed under the same variant (retouch undo/redo): before pane,
    /// paint canvas, overlay reference and the retained frame all described
    /// the old raster. History is deliberately NOT touched here.
    pub(crate) fn refresh_active_pixels(&mut self, ctx: &egui::Context) {
        let Some(v) = self.variants.get(self.active) else { return };
        let base = v.base.clone().or_else(|| self.source_preview.clone());
        if let Some(base) = base {
            let (mw, mh) = base.dimensions();
            // Same rule as load_active: the canvas recipe's own curve serves
            // every variant (a Generated recipe's curve is empty by
            // construction; an InPlace master is neutral and needs it).
            let start = self.recipe.start_only();
            self.set_before(ctx, &base, &start);
            self.base_preview = Some(base);
            self.rebind_paint_canvas(mw, mh);
        }
        self.overlay.overlay_ref = None;
        self.overlay.overlay_stale = true;
        self.chrome.last_rgb = None;
    }

    /// The base plate under the canvas was REPLACED (a preview-resolution
    /// re-decode, a cold master landing, a retouch bake, a fresh open): the
    /// paint canvas is resized to it — and any live mask-brush session dies
    /// with the old plate, DISCLOSED. The session's weight buffer is
    /// dimension-locked to the plate it was armed on (`start_mask_brush`
    /// sizes both together), so a stroke stamped after the swap lands at
    /// the wrong coordinates in it — and the always-visible 「✓ Apply」 row
    /// would bake those misplaced weights into a durably-adopted raster. A
    /// variant switch already gets the teardown via `load_active`'s
    /// `disarm_tools`; the async landings never called it (L06#5). The
    /// toast lives HERE so no future plate-replacement door can forget it;
    /// a second rebind in the same landing finds no session and stays
    /// silent.
    pub(crate) fn rebind_paint_canvas(&mut self, w: u32, h: u32) {
        if self.end_mask_brush() {
            let t = tr(
                self.lang,
                "the mask-brush session ended — the canvas pixels underneath were replaced, so its strokes no longer line up",
            );
            self.toast(ToastKind::Error, t.to_string());
        }
        self.masks.mask_paint = Some(image::RgbaImage::new(w, h));
        self.paint_mask_changed(Some(false));
        // The other tools' areas were in the old frame too.
        self.masks.paint_store = Default::default();
        self.overlay.mask_tex = None;
        self.overlay.mask_dirty = false;
        self.masks.paint_last = None;
    }

    /// May the photographer turn this photo right now? (Drives both the
    /// toolbar buttons' enabled state and the plate reconciler's own gate, so
    /// the two can never disagree about which photos rotate.)
    ///
    /// The one exclusion is BAKED PIXELS. A retouched/generated master is a
    /// raster on disk in the frame it was baked in, and this build has no way
    /// to record that a file was written before a turn — so turning the canvas
    /// under it would put the master back sideways on the next open, which is
    /// the silent-corruption shape the whole orientation chain exists to
    /// prevent. Rotate FIRST, bake after: the master is then written in the
    /// turned frame and everything agrees. The whole strip counts, not just
    /// the active card — an inactive variant's master is on disk exactly the
    /// same way. (Registered limitation, R27 A6.)
    ///
    /// An ARMED tool is excluded for a smaller reason: brush and retouch
    /// gestures bind screen coordinates to plate pixels mid-drag, and the
    /// plate is about to move under them.
    pub(crate) fn can_rotate(&self) -> bool {
        self.src_path.is_some()
            && self.base_preview.is_some()
            && !self.busy
            && !self.open.open_in_flight
            && !self.masks.paint_mode
            && self.overlay.region.is_none()
            && self.variants.iter().all(|v| v.base.is_none() && v.origin.is_none())
            // A fill layer is pixels in the frame it was made in, like a
            // baked master (2026-10-01): the live card and every stored one.
            && self.recipe.pixel_layers.is_empty()
            && self.variants.iter().all(|v| v.recipe.pixel_layers.is_empty())
            // …and so is a lens blur's depth map (2026-10-13): estimated in
            // the frame the photo had, its texels would not follow a turn.
            && self.recipe.lens_blur.is_none()
            && self.variants.iter().all(|v| v.recipe.lens_blur.is_none())
    }

    /// Turn the open photo by `delta` clockwise quarter turns — ONE undo step,
    /// the whole develop moving together ([`autoshade::pipeline::rotate_recipe`]
    /// owns geometry + raster masks + the turn count).
    ///
    /// The plates are NOT turned here: `sync_base_turns` sees the changed
    /// `recipe.quarter_turns` on the next frame and moves them, which is the
    /// same path an UNDO of this action takes. One mover, so the two
    /// directions cannot drift.
    pub(crate) fn rotate_photo(&mut self, delta: u8, ctx: &egui::Context) {
        if !self.can_rotate() {
            return;
        }
        let Some(src) = self.src_path.clone() else { return };
        let lang = self.lang;
        // The step BEFORE the turn, so one Ctrl+Z puts it back (7.7).
        self.commit_now();
        let mut next = self.recipe.clone();
        match autoshade::pipeline::rotate_recipe(&mut next, &src, delta) {
            Ok(_) => {
                self.recipe = next;
                self.dirty = true;
                self.sync_base_turns(ctx);
                self.resync_recipe_display();
                // …and SEAL it, rather than waiting for `commit_if_settled`
                // on the next frame: a click is already a settled gesture, and
                // the one-frame window let a same-frame Ctrl+Z undo the step
                // BEFORE this one (the hazard `commit_now`'s own doc records).
                self.commit_now();
            }
            Err(e) => {
                // All-or-nothing (see `rotate_recipe`): the recipe is the
                // untouched clone, so nothing moved and nothing needs undoing.
                // The sentence below covers the DISK too since R28 Batch-3 3b —
                // a failed turn used to leave one orphan PNG per attempt in the
                // develop dir while this toast said nothing was changed.
                let t = trf(
                    lang,
                    "could not turn this photo: {err} — nothing was changed",
                    &[("err", &e.to_string())],
                );
                self.toast(ToastKind::Error, t);
            }
        }
    }

    /// Bring the base plates into the frame `recipe.quarter_turns` asks for.
    ///
    /// Called once per frame (`app.rs` update) rather than from each of the
    /// eight places that can change the turn — undo, redo, a variant switch, a
    /// version load, a paste, the rotate buttons. `image`'s rotate is
    /// LOSSLESS and the plate is preview-sized, and the common case is a `u8`
    /// comparison that returns immediately.
    ///
    /// Turning the PIXELS rather than teaching the canvas a rotation is
    /// deliberate, and it is the same decision `render_to_image_in` makes:
    /// every screen↔frame mapping downstream (`view_norm_to_orig`, the crop
    /// handles, the paint canvas, the coverage overlay, the retouch region)
    /// is defined against the plate, so one turn here moves all of them and
    /// none of them needs to know.
    pub(crate) fn sync_base_turns(&mut self, ctx: &egui::Context) {
        let want = self.recipe.quarter_turns % 4;
        let delta = (want + 4 - self.base_turns % 4) % 4;
        if delta == 0 {
            return;
        }
        // A baked master is on disk in its own frame; `can_rotate` refuses to
        // create a delta over one, and this is the same gate seen from the
        // other side. Every master is written in the EXIF frame (the pixel
        // doors refuse a turned photo — `refuse_pixel_work_on_a_turned_photo`),
        // so what this branch protects is the per-variant `v.base` plate,
        // whose frame nothing tracks: turning the previews under it would
        // put the canvas and the cards in two frames.
        if self.variants.iter().any(|v| v.base.is_some() || v.origin.is_some()) {
            self.base_turns = want;
            return;
        }
        let turn = |p: &Arc<image::DynamicImage>| {
            Arc::new(autoshade::render::turn_image((**p).clone(), delta))
        };
        self.source_preview = self.source_preview.as_ref().map(&turn);
        self.base_preview = self.base_preview.as_ref().map(&turn);
        self.base_turns = want;
        // Everything derived from the plate's frame. The Before texture is
        // rebuilt here rather than invalidated: `set_before` is the only
        // writer, and leaving a stale one on screen for a frame is the
        // sideways-preview bug in miniature. The paint canvas is REBOUND
        // (`refresh_active_pixels`'s rule for a replaced plate): its buffer
        // is dimension-locked to the plate and an odd turn transposes the
        // plate, so a canvas left W×H under an H×W plate put every later
        // brush stroke — and the fill/heal mask exported from it — at the
        // wrong coordinates. A live brush session dies with the old frame,
        // disclosed, exactly as at every other replacement door.
        if let Some(b) = self.base_preview.clone() {
            let start = self.recipe.start_only();
            self.set_before(ctx, &b, &start);
            let (mw, mh) = b.dimensions();
            self.rebind_paint_canvas(mw, mh);
        }
        self.overlay.overlay_ref = None;
        self.overlay.overlay_stale = true;
        self.chrome.last_rgb = None;
        self.overlay.mask_tex = None;
        self.overlay.mask_overlay_tex = None;
        self.overlay.overlay_key = None;
        self.dirty = true;
    }

    pub(crate) fn undo(&mut self, ctx: &egui::Context) {
        if let Some(prev) = self.hist.undo_stack.pop() {
            self.hist.redo_stack.push(self.hist.committed.clone());
            self.apply_step(prev, ctx);
        }
    }

    pub(crate) fn redo(&mut self, ctx: &egui::Context) {
        if let Some(next) = self.hist.redo_stack.pop() {
            self.hist.undo_stack.push(self.hist.committed.clone());
            self.apply_step(next, ctx);
        }
    }

    /// A modal layer is up — the quit confirm or the fit prompt — and owns
    /// the keyboard and the pointer (D13).
    pub(crate) fn modal_up(&self) -> bool {
        self.ui.confirm_quit || self.fit.fit_prompt.is_some()
    }

    /// Raise the fit prompt after a whole-frame reimagine landed and the strip
    /// switched to its ✨ card. Not over a running job (the answer could not
    /// start), not once the user said not to ask, and not without the
    /// negative's preview [`Self::start_fit`] reads.
    pub(crate) fn offer_fit_prompt(&mut self, ctx: &egui::Context, divergence: f32) {
        if self.busy
            || !self.fit.ask_fit_after_generate
            || self.source_preview.is_none()
        {
            return;
        }
        let Some(target) = self
            .active_variant()
            .filter(|v| v.kind == VariantKind::Generated && v.origin.is_some())
            .map(|v| v.id.clone())
        else {
            return;
        };
        // One question at a time: a newer generation takes the open one over
        // — the strip already stands on its card — and keeps what the user
        // picked in it.
        if let Some(p) = &mut self.fit.fit_prompt {
            p.target = target;
            p.divergence = divergence;
            return;
        }
        // The ▣ original first (the user's wording: 默认原图), not Automatic,
        // which solves from a ◈ / ▦ master whenever the strip holds one.
        let from = self.original_index().map(|i| self.variants[i].id.clone()).filter(|id| !id.is_empty());
        self.fit.fit_prompt = Some(FitPrompt { target, divergence, from, dont_ask: false });
        // The layer's Enter waits for "no widget focused" — surrendered now,
        // as the quit layer does when it opens.
        if let Some(id) = ctx.memory(|m| m.focused()) {
            ctx.memory_mut(|m| m.surrender_focus(id));
        }
    }

    /// The fit prompt (v1.17.0, user directive): after「AI generate」, ask
    /// whether to reverse-fit the new ✨ card, and from which negative. It
    /// only hands over to the fold's own verb — 「Reverse-fit」 sets the
    /// fold's source and calls [`Self::start_fit`] — so the fit it starts IS
    /// the panel button's; the strength and the switches stay the fold's
    /// controls (the AI review is paid, and its cost is named there), and the
    /// layer only states them.
    pub(crate) fn fit_prompt_layer(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        let Some(mut prompt) = self.fit.fit_prompt.take() else {
            return;
        };
        let (automatic, sources) = self.fit_source_menu();
        let to = self
            .variants
            .iter()
            .position(|v| v.id == prompt.target)
            .map(|i| trf(lang, "To: {card}", &[("card", &self.numbered_card_label(i))]));
        let fidelity = crate::i18n::text(lang, &logic::retouch::reimagine_fidelity(prompt.divergence));
        let on_off = |on: bool| if on { tr(lang, "on") } else { tr(lang, "off") };
        let in_force = trf(
            lang,
            "Reverse-fit strength {pct}% · local areas {zoned} · AI review {review} — set in the AI panel's reverse-fit fold",
            &[
                ("pct", &format!("{:.0}", self.aip.fit_strength * 100.0)),
                ("zoned", on_off(self.fit.zoned_fit)),
                ("review", on_off(self.fit.fit_ai_judge)),
            ],
        );
        let mut open = true;
        let mut fit = false;
        let mut later = false;
        // The quit layer's keyboard grammar: Esc = the safe escape (nothing
        // changes), Enter = the primary — unless a control holds focus, which
        // then owns its own Enter.
        let widget_focused = ctx.memory(|m| m.focused()).is_some();
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                later = true;
            }
            if !widget_focused && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                fit = true;
            }
        });
        // And the pointer (D13): the strip and the sliders behind it must not
        // change the card the answer is about.
        egui::Area::new(egui::Id::new("fit_prompt_scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                let r = ctx.content_rect();
                ui.allocate_response(r.size(), egui::Sense::click_and_drag());
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(96));
            });
        egui::Window::new(tr(lang, "Reverse-fit this AI result?"))
            .id(egui::Id::new("fit_prompt"))
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(Self::fit_explainer(lang));
                // The landing's own reading, without its ` · ` joint.
                ui.label(egui::RichText::new(fidelity.trim_start_matches(" · ")).small().weak());
                if let Some(to) = &to {
                    ui.label(egui::RichText::new(to).small().weak());
                }
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "From:"));
                    let shown = match prompt.from.as_deref() {
                        Some(id) => sources.iter().find(|(s, _)| s == id).map(|(_, l)| l.clone()),
                        None => None,
                    }
                    .unwrap_or_else(|| automatic.clone());
                    // The fold's menu, the ▣ original first: Automatic stays
                    // offered, after the cards it chooses among.
                    egui::ComboBox::from_id_salt("fit_prompt_from").selected_text(shown).show_ui(ui, |ui| {
                        for (id, label) in &sources {
                            let on = prompt.from.as_deref() == Some(id.as_str());
                            if ui.selectable_label(on, label).clicked() {
                                prompt.from = Some(id.clone());
                            }
                        }
                        if ui.selectable_label(prompt.from.is_none(), &automatic).clicked() {
                            prompt.from = None;
                        }
                    });
                });
                ui.label(egui::RichText::new(&in_force).small().weak());
                ui.checkbox(&mut prompt.dont_ask, tr(lang, "Don't ask again after generating"))
                    .on_hover_text(tr(lang, "Turn it back on in the AI panel's reverse-fit fold: 「Ask after each AI generation」"));
                ui.add_space(SPACE_MD);
                // The safe answer first, the primary tinted and last — the
                // quit layer's order.
                ui.horizontal(|ui| {
                    later |= action(ui, true, tr(lang, "Not now")).on_hover_text("Esc").clicked();
                    fit |= primary(ui, true, tr(lang, "Reverse-fit")).on_hover_text("Enter").clicked();
                });
            });
        if !open {
            later = true;
        }
        if !(fit || later) {
            self.fit.fit_prompt = Some(prompt);
            return;
        }
        if prompt.dont_ask {
            self.fit.ask_fit_after_generate = false;
        }
        // The answer is about the card it asked about; the strip stands on it
        // (the scrim kept every other click away) unless a landing moved it.
        if fit && self.active_variant().is_some_and(|v| v.id == prompt.target) {
            self.fit.fit_from = prompt.from;
            // A reference file picked in the fold earlier wins `fit_target`
            // over the active card — the fit would solve toward it, not
            // toward the result the question named.
            self.fit.fit_ref = None;
            self.start_fit();
        }
    }

    /// The quit-confirm layer (in-app egui window). Lists what quitting would
    /// lose; 「Save all & quit」 writes each pending develop exactly like
    /// Ctrl+S (explicit user save → no backup gate), 「Discard & quit」 makes
    /// the state clean so the close guard lets the next close through.
    pub(crate) fn confirm_quit_layer(&mut self, ctx: &egui::Context) {
        let lang = self.lang;
        // Everything quitting would lose: the stash + the open photo's canvas
        // (the live canvas outranks its own stale stash entry). Each entry
        // carries its pixel identity so 「Save all」 persists a baked retouch's
        // master link exactly like Ctrl+S would. (The bool is the
        // `pixels.json` flag — `VariantKind::on_ai_pixels`, the axis the
        // flag records: a ✨ card and its ✎ cards sit on the same raster.)
        let mut pending: Vec<PendingSave> = self
            .open.nav_stash
            .iter()
            .map(|(p, st)| PendingSave {
                photo: p.clone(),
                recipe: st.recipe.clone(),
                pixels: st.origin.clone().map(|o| (o, st.kind.on_ai_pixels())),
                strip: stash_strip_record(st),
            })
            .collect();
        // Live-canvas override — skipped while a photo OPEN is in flight:
        // src_path already points at the new photo but recipe/saved_recipe
        // still describe the old one, so acting on the pair would either drop
        // the new photo's stash entry or save the old recipe under the new
        // path. The stash (written by open_path just before the flight) is
        // the correct authority for that window. ONLY the open transition —
        // gating on plain `busy` let any background worker (Export…) empty
        // `pending`, and the empty-pending branch below then CLOSED the app,
        // losing a sole live unsaved canvas (background work still lands
        // under the dialog — the scrim below blocks only pointer input).
        if !self.open.open_in_flight && let Some(p) = self.src_path.clone() {
            let origin = self.active_variant().and_then(|v| v.origin.clone());
            // The live canvas outranks its own stash entry WHOLESALE: displace
            // it even when the canvas is clean (the user reverted) — the loop
            // below would otherwise write the stale stashed edits to disk.
            pending.retain(|e| e.photo != p);
            // The strip counts as unsaved work of the OPEN photo too — a
            // clean canvas over an unpersisted strip must still be listed,
            // or Save-all had nothing to write and the guard's re-check
            // bounced the close forever (the v0.21 dead-button livelock).
            if self.pending_save_gate_dirty() {
                let pix = origin.map(|o| (o, self.active_on_ai_pixels()));
                pending.push(PendingSave {
                    photo: p,
                    recipe: self.recipe.clone(),
                    pixels: pix,
                    strip: self.current_strip_record(),
                });
            }
        }
        // Background variants ride IN `pending` since v0.22: each entry
        // carries its photo's strip record, and Save-all persists it to
        // variants.json beside the develop. The count still gates the
        // empty-pending close below (defence-in-depth — a strip-dirty photo
        // is also pushed into `pending` above).
        let orphan_variants = self.inactive_dirty_variants();
        if pending.is_empty() && orphan_variants == 0 {
            // Saved (or discarded) since the guard fired — nothing to protect.
            self.ui.confirm_quit = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            return;
        }
        let mut open = true;
        let mut save_quit = false;
        let mut discard_quit = false;
        let mut cancel = false;
        // The global shortcut block is gated off while this layer is up, so
        // the promised keyboard grammar is honoured HERE: Esc = the safe
        // escape (always), Enter = the safe default (save everything) — but
        // only while NO button holds keyboard focus, or Enter on a focused
        // Cancel/Discard would fire Save instead of the focused control.
        let widget_focused = ctx.memory(|m| m.focused()).is_some();
        ctx.input_mut(|i| {
            if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
                cancel = true;
            }
            if !widget_focused && i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
                save_quit = true;
            }
        });
        // A modal owns the pointer as well as the keyboard (D13): the scrim
        // swallows every click behind the dialog — sliders were still live
        // during the save-or-discard decision — and dims the stakes into
        // view. Background workers are unaffected (see open_in_flight above).
        egui::Area::new(egui::Id::new("confirm_quit_scrim"))
            .order(egui::Order::Middle)
            .fixed_pos(egui::Pos2::ZERO)
            .show(ctx, |ui| {
                let r = ctx.content_rect();
                ui.allocate_response(r.size(), egui::Sense::click_and_drag());
                ui.painter().rect_filled(r, 0.0, egui::Color32::from_black_alpha(96));
            });
        egui::Window::new(tr(lang, "● Unsaved edits"))
            .id(egui::Id::new("confirm_quit"))
            .order(egui::Order::Foreground)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .open(&mut open)
            .show(ctx, |ui| {
                if !pending.is_empty() {
                    ui.label(trf(
                        lang,
                        "{n} photo(s) have edits that were never saved:",
                        &[("n", &pending.len().to_string())],
                    ));
                }
                // Parent folder + stem: two DSC_0431 from two shoots must be
                // distinguishable in the one dialog where the stakes are real.
                egui::ScrollArea::vertical().max_height(140.0).show(ui, |ui| {
                    for PendingSave { photo: p, .. } in &pending {
                        let folder = p
                            .parent()
                            .and_then(|d| d.file_name())
                            .and_then(|s| s.to_str())
                            .unwrap_or("");
                        ui.label(
                            egui::RichText::new(format!(
                                "{folder}/{}",
                                autoshade::pipeline::stem(p)
                            ))
                            .small()
                            .weak(),
                        );
                    }
                });
                if orphan_variants > 0 {
                    // Informational since v0.22: Save-all persists each
                    // photo's whole variant strip (variants.json), so these
                    // are rescued WITH their photo, not lost.
                    ui.add_space(SPACE_MD);
                    ui.label(
                        egui::RichText::new(trf(
                            lang,
                            "{n} other variant(s) hold edits — 「Save all」 saves each photo's whole variant strip along with its develop.",
                            &[("n", &orphan_variants.to_string())],
                        ))
                        .small()
                        .weak(),
                    );
                }
                ui.add_space(SPACE_MD);
                // Safe escape far left, the destructive action fenced off by
                // space, the primary (save) tinted and last — a 4px slip from
                // Save must not land on an unrecoverable Discard.
                ui.horizontal(|ui| {
                    cancel |= action(ui, true, tr(lang, "Cancel")).on_hover_text("Esc").clicked();
                    ui.add_space(18.0);
                    let warn = ui.visuals().warn_fg_color;
                    discard_quit = action(ui, true, egui::RichText::new(tr(lang, "Discard & quit")).color(warn))
                        .on_hover_text(tr(lang, "Quit WITHOUT saving — these edits are gone for good"))
                        .clicked();
                    save_quit |= primary(ui, true, tr(lang, "Save all & quit"))
                        .on_hover_text(tr(lang, "Enter · save every listed develop, then quit"))
                        .clicked();
                });
            });
        if save_quit {
            let mut failed: Option<String> = None;
            // Collected, not fatal — reported on the way out (see below).
            let mut xmp_warns: Vec<String> = Vec::new();
            let mut clear_warns: Vec<String> = Vec::new();
            for PendingSave { photo: p, recipe: r, pixels: pix, strip } in &pending {
                // ONE NoWait lock per photo across its whole persist compound
                // (the Ctrl+S pairing). A develop busy in another process
                // fails THIS photo — the quit bounces and reports below —
                // instead of freezing the dialog behind an unbounded Wait.
                let locked: std::io::Result<()> = autoshade::store::with_develop_lock(
                    p,
                    autoshade::store::DevelopLockMode::NoWait,
                    || {
                // Neutral + no pixel identity + trivial strip = Ctrl+S's
                // "clear my edits": WRITING neutral files here pinned the
                // existence-keyed ● badge that a direct Ctrl+S removes. A
                // non-trivial strip takes the save path below instead —
                // clear_develop would destroy the OTHER cards' work.
                if r.is_noop() && pix.is_none() && strip.is_none() {
                    // The store's ONE clear primitive (both surfaces): it takes
                    // the retired pixels.json.bak with it, which a bare unlink
                    // left behind for the next open to republish.
                    match autoshade::store::clear_develop(p) {
                        Ok(o) => {
                            // clear_develop took variants.json with it — the
                            // open photo's mirror must follow, or the guard's
                            // way-out re-check compares live-trivial against
                            // a stale record and bounces the close.
                            if self.src_path.as_deref() == Some(p.as_path()) {
                                self.hist.saved_strip = None;
                            }
                            // Cleared, but not MARKED — same channel as the XMP
                            // half: quitting silently would let a projection
                            // copied beside the RAW undo this clear unannounced.
                            if let Some(w) = o.marker_warning {
                                clear_warns.push(format!("{}: {w}", autoshade::pipeline::stem(p)));
                            }
                        }
                        Err(e) => {
                            failed = Some(format!("{}: {e}", autoshade::pipeline::stem(p)));
                        }
                    }
                    return Ok(());
                }
                // A GENERATED entry's recipe is the STRIPPED canvas form —
                // persisting it verbatim erased the RAW's calibration from
                // disk (the Analyze saver writes the calibrated form; a
                // master that later fails to decode then falls back to a
                // dark, uncorrected develop). Re-stamp from the same
                // saved-first source produce_recipe uses.
                let mut disk = r.clone();
                if pix.as_ref().is_some_and(|(_, g)| *g) {
                    // ONE snapshot for every half — independent reads could
                    // pair an OLD curve with a NEW profile if another surface
                    // published between them. The era stamp rides WITH the
                    // curve (the paste rule): this WRITES recipe.json, and a
                    // canvas-era stamp over a saved era-1 curve would launder
                    // it past every repair.
                    // Every calibration half, the start (v1.17.0) included,
                    // through the one stamper.
                    let cal = autoshade::pipeline::photo_calibration(p);
                    autoshade::pipeline::stamp_fit_calibration(&mut disk, cal);
                }
                // The WRITER's rule (Ctrl+S, api_xmp): a stashed canvas whose
                // open-time estimate failed still carries the washed pre-era
                // curve — repaired before it is persisted. Memo-cheap when
                // already repaired, and it also covers the generated arm
                // above, whose calibration snapshot may itself have adopted
                // an unrepaired curve when the store read hit an inability.
                // SAID on the loop's existing quitting-time channel — gated
                // on the RECIPE write, not the compound result below: the
                // pixel-link half can fail AFTER recipe.json already
                // published the re-estimated curve, and dropping the note
                // then hid a mutation that IS on disk (the Ctrl+S rule: the
                // disclosure travels with the mutation).
                let relook_note = autoshade::pipeline::repair_pre_era_base_curve(p, &mut disk);
                let generated = pix.as_ref().is_some_and(|(_, g)| *g);
                // The projection is a member of the same generation
                // (2026-09-13): written for a source develop, CLEARED for a
                // generated entry. Skipping the write alone (the rule below)
                // left a sidecar written for an earlier card standing — it
                // outlived the card and the recipe it projected, and the
                // open path restored it over the pristine generated pixels.
                let (xmp_member, xmp_outcome) = match autoshade::pipeline::xmp_projection_member(
                    p,
                    &disk,
                    !generated,
                    autoshade::diag::stderr(),
                ) {
                    Ok((m, note, _)) => (m, Ok(note)),
                    Err(e) => (autoshade::store::CommitMember::Keep, Err(e)),
                };
                // ONE single-generation commit per photo (the Ctrl+S rule,
                // L03): recipe + baked-pixels link + strip record land whole
                // or not at all — the background variants this dialog just
                // listed cannot outlive a save that tore between members.
                let res: anyhow::Result<()> = (|| {
                    let recipe_bytes = autoshade::pipeline::recipe_store_bytes(p, &disk, autoshade::diag::stderr())?;
                    let pixels = match pix {
                        Some((o, g)) => autoshade::store::CommitMember::Write(
                            autoshade::store::pixel_source_record_bytes(p, o, *g)?,
                        ),
                        None => autoshade::store::CommitMember::Clear,
                    };
                    let variants = match strip {
                        Some(rec) => autoshade::store::CommitMember::Write(
                            autoshade::store::variants_record_bytes(p, rec)?,
                        ),
                        None => autoshade::store::CommitMember::Clear,
                    };
                    autoshade::store::commit_develop(
                        p,
                        autoshade::store::DevelopCommit {
                            recipe: Some(recipe_bytes),
                            pixels,
                            variants,
                            xmp: xmp_member,
                        },
                    )?;
                    Ok(())
                })();
                match res {
                    Ok(()) => {
                        // The disclosure travels with the mutation (the
                        // Ctrl+S rule): the commit landed whole, so the
                        // re-estimated curve IS on disk.
                        if let Some(note) = relook_note.as_deref() {
                            eprintln!("⚠ {}: {note}", autoshade::pipeline::stem(p));
                        }
                        if self.src_path.as_deref() == Some(p.as_path()) {
                            self.hist.saved_strip = strip.clone();
                            // The FOURTH mirror of the single-save set
                            // (export.rs): pixels.json for the ACTIVE photo
                            // just committed, and without this the ● badge
                            // and the exit re-check compared against a stale
                            // master link — a just-saved retouch was
                            // re-reported as unsaved (L11-6).
                            self.hist.pixels_on_disk = pix.as_ref().map(|(o, _)| o.clone());
                        }
                    }
                    Err(e) => {
                        failed = Some(format!("{}: {e}", autoshade::pipeline::stem(p)));
                        return Ok(());
                    }
                }
                // NO XMP for a generated entry: the look lives in baked
                // pixels no parametric sidecar can reproduce, and writing one
                // here overwrote a real Lightroom sidecar with a lie — the
                // member above CLEARS the projection for such an entry in the
                // same generation. The recipe + pixels pair alone restores
                // faithfully.
                //
                // OUTSIDE the fatal result: the recipe write alone decides the
                // saved state (cross-surface rule), so a failed XMP projection
                // must not abort the remaining photos or refuse to quit over a
                // develop that IS durably saved.
                if autoshade::decode::is_raw(p) && !generated {
                    // The M6a mask-loss list is dropped HERE for the same
                    // reason as in the batch paste: `xmp_warns` renders under
                    // one fixed "projection(s) failed" sentence, which a
                    // lossy-but-successful projection is not. stderr carries
                    // the per-photo line from the builder.
                    match xmp_outcome {
                        Ok(None) => {}
                        // A regenerated (unmerged) sidecar loses LR-only
                        // properties — Save-all's warning list is exactly
                        // where that belongs (round-12 disclosure threading).
                        Ok(Some(n)) => {
                            xmp_warns.push(format!("{}: {n}", autoshade::pipeline::stem(p)));
                        }
                        Err(e) => {
                            xmp_warns.push(format!("{}: {e}", autoshade::pipeline::stem(p)));
                        }
                    }
                }
                        Ok(())
                    },
                );
                if let Err(e) = locked {
                    failed = Some(format!(
                        "{}: the photo is being changed by another AutoShade process ({e})",
                        autoshade::pipeline::stem(p)
                    ));
                }
                if failed.is_some() {
                    break;
                }
            }
            // ALL said regardless of a later photo's failure (the
            // travels-with-the-mutation rule the repair note above follows),
            // COUNTED (a single-slot accumulator reported only the last of
            // several same-kind failures), and worded per-photo so the
            // sentence stays true on the abort path — the old "develops
            // saved" claimed a completed batch even after a break.
            // stderr keeps the full list for logs; the USER-facing copy is
            // below — an eprintln alone vanished with the closing window,
            // which is exactly the silence W3 exists to end.
            if !xmp_warns.is_empty() {
                eprintln!(
                    "⚠ {} Lightroom XMP projection(s) failed for develop(s) that ARE saved: {}",
                    xmp_warns.len(),
                    xmp_warns.join("; ")
                );
            }
            if !clear_warns.is_empty() {
                eprintln!(
                    "⚠ {} clear(s) succeeded but could not be marked: {} — a sidecar \
                     beside the RAW may restore those edits on the next open",
                    clear_warns.len(),
                    clear_warns.join("; ")
                );
            }
            let mut quit_warns: Vec<String> = Vec::new();
            if !xmp_warns.is_empty() {
                quit_warns.push(trf(
                    lang,
                    "{n} Lightroom XMP projection(s) failed (those develops ARE saved): {detail}",
                    &[("n", &xmp_warns.len().to_string()), ("detail", &logic::brief_list(&xmp_warns))],
                ));
            }
            if !clear_warns.is_empty() {
                quit_warns.push(trf(
                    lang,
                    "{n} clear(s) could not be marked: {detail} — a sidecar beside the RAW may restore those edits on the next open",
                    &[("n", &clear_warns.len().to_string()), ("detail", &logic::brief_list(&clear_warns))],
                ));
            }
            match failed {
                None => {
                    self.open.nav_stash.clear();
                    self.hist.saved_recipe = self.recipe.clone();
                    self.ui.confirm_quit = false;
                    // The close guard re-checks on the way out. Everything
                    // above just persisted, so the re-check passes — but if
                    // anything is STILL dirty (logic drift, a future writer
                    // this loop misses), the bounce must be SAID: a silently
                    // re-armed dialog is the v0.21 dead-button livelock.
                    let residue = self.inactive_dirty_variants();
                    if residue > 0 {
                        let t = trf(
                            lang,
                            "saved, but {n} variant(s) still count as unsaved — the window stays open; please report this",
                            &[("n", &residue.to_string())],
                        );
                        self.toast(ToastKind::Error, t);
                    } else if !quit_warns.is_empty() {
                        // Everything IS saved (recipe-write-decides), so
                        // refusing to quit would be wrong — but a toast on a
                        // closing window dies unread. Bounce ONCE with the
                        // warnings visible; nothing is dirty any more, so the
                        // next quit passes straight through without this
                        // dialog. One extra click, only in the failure case.
                        let t = trf(
                            lang,
                            "saved with warnings — the window stays open so they can be read; quit again to close: {detail}",
                            &[("detail", &quit_warns.join(" · "))],
                        );
                        self.status = t.clone();
                        self.toast(ToastKind::Error, t);
                    } else {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                    }
                }
                Some(e) => {
                    // Stay open: a failed save on quit must never quietly quit.
                    let t = trf(lang, "save failed: {err}", &[("err", &e)]);
                    self.status = t.clone();
                    self.toast(ToastKind::Error, t);
                    self.ui.confirm_quit = false;
                }
            }
        } else if discard_quit {
            self.open.nav_stash.clear();
            self.hist.saved_recipe = self.recipe.clone();
            // The answer to the guard's question, recorded: these two lines
            // clean the ACTIVE canvas, but a background variant's edits have
            // nowhere to be saved, so nothing can make them "clean" — only
            // the user's decision can end the question.
            self.open.discard_requested = true;
            self.ui.confirm_quit = false;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        } else if cancel || !open {
            self.ui.confirm_quit = false;
        }
    }

    /// Add a recipe file the user picked (our own recipe.json, or a Lightroom
    /// .xmp sidecar) to the library.
    pub(crate) fn import_recipe(&mut self) {
        // The panel only emits the verb; the dialog and the library writes are
        // the app's side effects.
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr(self.lang, "Recipe file"), &["json", "xmp"])
            .pick_file()
        else {
            return;
        };
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or_default().to_string();
        let name = match autoshade::recipe_library::check_name(&stem) {
            Ok(name) => name,
            Err(e) => {
                let text = self.recipe_name_error(e);
                self.recipe_status(false, text);
                return;
            }
        };
        if autoshade::recipe_library::exists(&name) {
            let text = trf(self.lang, "A recipe named 「{name}」 already exists", &[("name", &name)]);
            self.recipe_status(false, text);
            return;
        }
        let recipe = match autoshade::recipe_library::read_file(&path) {
            Ok(r) => r,
            Err(e) => {
                let text = trf(self.lang, "{file} is not a recipe: {err}", &[("file", &path.display().to_string()), ("err", &e.to_string())]);
                self.recipe_status(false, text);
                return;
            }
        };
        let (entry, losses) = autoshade::recipe_library::portable(&recipe, false);
        match autoshade::recipe_library::create(&name, &entry) {
            Ok(()) => {
                self.refresh_recipes();
                self.rlib.recipe_sel = Some(name.clone());
                let base = trf(self.lang, "Imported 「{name}」", &[("name", &name)]);
                let text = if losses.is_empty() {
                    base
                } else {
                    format!(
                        "{} — {} {}",
                        base,
                        tr(self.lang, "left out what belongs to the source photo:"),
                        self.recipe_losses(losses)
                    )
                };
                self.recipe_status(true, text);
            }
            Err(e) => {
                let text = trf(self.lang, "Recipe 「{name}」 could not be saved: {err}", &[("name", &name), ("err", &e.to_string())]);
                self.recipe_status(false, text);
            }
        }
    }

    /// The jpg the Save group delivers.
    pub(crate) fn normal_export_opts(&self) -> autoshade::render::ExportOpts {
        autoshade::render::ExportOpts { jpeg_quality: self.normal.quality.jpeg_quality(), ..Default::default() }
    }

    /// 「Save as jpg」: the single export of the card on screen (the ◈ card
    /// when the noise switch is on), as a jpg at the quality step, to the
    /// export destination (asked when the destination is "ask"). A name the
    /// app made is numbered (`-2`, `-3`, …) when a jpg of that name is
    /// already there; a file picked in the save window is replaced, as that
    /// window confirmed.
    pub(crate) fn start_normal_save(&mut self) {
        if self.active_source_path().is_none() || self.busy {
            return;
        }
        let name = self.export_file_name_as("jpg");
        let (out, how) = match self.export_dest_dir() {
            Some(dir) => (dir.join(name), OutName::Numbered),
            None => {
                let Some(p) = self.pick_save_file(name) else { return };
                (normalize_export_target(p, "jpg"), OutName::Chosen)
            }
        };
        let export = self.normal_export_opts();
        // The folder picked in the save window is remembered when the render
        // starts — the workspace Export's rule (`export_to_chosen_path`), so
        // 「the folder you last saved to」 has somewhere to point.
        let picked = (how == OutName::Chosen).then(|| out.parent().map(|d| d.to_path_buf())).flatten();
        if self.start_render_with(out, export, None, false, how) {
            self.normal.delivering = true;
            if picked.is_some() {
                self.exp.last_dir = picked;
            }
        }
    }

    /// The save window's answer (`None` = cancelled).
    fn pick_save_file(&mut self, name: String) -> Option<PathBuf> {
        #[cfg(test)]
        if let Some(pick) = self.normal.file_pick.take() {
            return pick;
        }
        let mut dlg = rfd::FileDialog::new().add_filter("jpg", &["jpg"]).set_file_name(name);
        if let Some(d) = self.exp.last_dir.clone().filter(|d| d.is_dir()) {
            dlg = dlg.set_directory(d);
        }
        dlg.save_file()
    }

    /// 「Apply this look to every photo in the folder」: the open photo's look
    /// pasted onto every photo of the library folder (crop left out), then —
    /// once the paste has landed whole (`after_apply_all_paste`) — every one
    /// rendered as a jpg. The library panel's own two verbs, as one. The
    /// jpgs' folder is settled FIRST: a folder dialog cancelled after the
    /// paste would have left every photo's look changed and nothing saved.
    pub(crate) fn start_apply_to_folder(&mut self) {
        if self.busy || self.gal.gallery.is_empty() || self.src_path.is_none() {
            return;
        }
        let Some(dest) = self.apply_all_dest() else {
            // Cancelled: nothing was changed, and the line says nothing.
            self.status.clear();
            return;
        };
        self.commit_pending_names();
        let targets = self.gal.gallery.clone();
        self.normal.apply_all = Some(dest);
        self.start_paste_to(self.recipe.clone(), self.src_path.clone(), targets, false);
        if !self.busy {
            self.normal.apply_all = None;
        }
    }

    /// The destination setting's folder, or the folder dialog's answer when
    /// it asks (`None` = cancelled).
    fn apply_all_dest(&mut self) -> Option<PathBuf> {
        if let Some(d) = self.export_dest_dir() {
            return Some(d);
        }
        #[cfg(test)]
        if let Some(pick) = self.normal.folder_pick.take() {
            return pick;
        }
        self.pick_batch_folder()
    }

    /// Populate the Settings form from the resolved config (keys are shown only as
    /// "present", never revealed). Called when the window opens.
    pub(crate) fn load_settings_form(&mut self) {
        let cfg = autoshade::config::Config::load();
        // Keep any model lists already fetched this session so reopening Settings
        // doesn't force a re-fetch — and keep an in-flight fetch's flag alive:
        // zeroing it mid-fetch stopped the repaint pump AND re-armed the fetch
        // button (duplicate requests, stalled status). The catalogues also
        // carry the per-role auto-fetch guards and generation stamps.
        let image_models = std::mem::take(&mut self.ui.settings.image_models);
        let analysis_models = std::mem::take(&mut self.ui.settings.analysis_models);
        // ONE read of the settings file for the two fields below that show the
        // file's own spelling rather than the resolved value.
        let local = autoshade::config::load_local_settings();
        let comfy = autoshade::comfy::Settings::load();
        self.ui.settings = SettingsForm {
            analysis_provider_api: cfg.analysis_is_api(),
            image_provider_oauth: cfg.image_is_oauth(),
            analysis_model: cfg.analysis_model.clone(),
            analysis_base_url: cfg.analysis_base_url.clone(),
            analysis_api_key: String::new(),
            analysis_key_present: cfg.analysis_api_key.is_some(),
            analysis_effort: cfg.analysis_effort.clone().unwrap_or_default(),
            image_model: cfg.openai_model.clone(),
            image_base_url: cfg.openai_base_url.clone(),
            image_gen_model: cfg.openai_image_model.clone(),
            image_api_key: String::new(),
            image_key_present: cfg.openai_api_key.is_some(),
            image_effort: cfg.image_effort.clone().unwrap_or_default(),
            image_api_family: cfg.image_api_family.clone(),
            // The FILE's own spelling, not the resolved root: this is the
            // field the user edits, and showing a resolved absolute path in it
            // would silently turn "use the default" into a pinned folder on
            // the next save. `load_local_settings` (not the raw file) so an
            // ambient working-directory copy cannot pre-fill it either.
            out_dir: local.out_dir.unwrap_or_default(),
            // Same rule for the interpreter (M1-3): showing the RESOLVED
            // program here would turn "use the default" into a pinned path on
            // the next save, and `load_local_settings` is what keeps an
            // ambient working-directory copy from pre-filling either field.
            python_bin: local.python_bin.unwrap_or_default(),
            comfy_cli: comfy.cli,
            comfy_workspace: comfy.workspace,
            comfy_base_dir: comfy.base_dir,
            comfy_reimagine_workflow: comfy.reimagine_workflow,
            comfy_fill_workflow: comfy.fill_workflow,
            status: String::new(),
            image_models,
            analysis_models,
        };
        // Availability is credential-dependent: another surface (the web
        // Settings) may have replaced a saved key since these lists were
        // fetched, and the URL-keyed self-invalidation in `settings_ui`
        // cannot see that. Compare each kept catalogue's credential stamp
        // against today's key and drop a stale list here, at the one moment
        // the resolved config is already in hand.
        for role in [ModelRole::Image, ModelRole::Analysis] {
            let fp = key_fingerprint(match role {
                ModelRole::Image => cfg.openai_api_key.as_deref().unwrap_or(""),
                ModelRole::Analysis => cfg.analysis_api_key.as_deref().unwrap_or(""),
            });
            let cat = self.catalogue_mut(role);
            if !cat.is_empty() && cat.from_key != fp {
                cat.clear();
            }
        }
        self.autofetch_models_once(&cfg);
    }

    /// First time Settings opens in a session, fill the pick-lists without
    /// making the user find the button — "which models can I use here" is the
    /// question the panel exists to answer, and a blank dropdown next to a
    /// configured key reads as "none available".
    ///
    /// Once per session PER ROLE, and only for a role that already has a key:
    /// a probe with no credential can only fail, and firing on every open
    /// would put a network call behind "I came here to switch the language".
    ///
    /// The guard is consumed at DISPATCH (`fetch_models` marks the role), not
    /// by the visit itself: one global open-consumed boolean meant a Settings
    /// visit before any key existed spent the whole session's opportunity, so
    /// the role that became eligible five minutes later — key saved, provider
    /// flipped to `api` — was never probed on any later open.
    pub(crate) fn autofetch_models_once(&mut self, cfg: &autoshade::config::Config) {
        if cfg.openai_api_key.is_some() && !self.ui.settings.image_models.autofetched {
            self.fetch_models(ModelRole::Image);
        }
        // The analysis endpoint only has a catalogue to fetch in `api` mode —
        // the OAuth verifier is the `claude` CLI, which serves no /models.
        if cfg.analysis_is_api()
            && cfg.analysis_api_key.is_some()
            && !self.ui.settings.analysis_models.autofetched
        {
            self.fetch_models(ModelRole::Analysis);
        }
    }

    /// Persist the Settings form to autoshade.local.json (gitignored). A blank key
    /// keeps the stored one. The next Analyze/Export reloads Config, so it applies.
    pub(crate) fn save_settings_form(&mut self) {
        // The whole load-merge-save runs under the cross-process settings
        // lock (`config::update_local_settings`): this panel and the serve
        // process merge onto the same file, and the old unlocked cycle here
        // let a save that landed between its load and its rename be erased.
        let form = &self.ui.settings;
        let saved = autoshade::config::update_local_settings(|cur| {
            cur.analysis_provider =
                Some(if form.analysis_provider_api { "api" } else { "oauth" }.to_string());
            cur.image_provider =
                Some(if form.image_provider_oauth { "oauth" } else { "api" }.to_string());
            cur.analysis_model = Some(form.analysis_model.trim().to_string());
            cur.analysis_base_url = Some(form.analysis_base_url.trim().to_string());
            cur.image_model = Some(form.image_model.trim().to_string());
            cur.image_base_url = Some(form.image_base_url.trim().to_string());
            cur.image_gen_model = Some(form.image_gen_model.trim().to_string());
            // Effort: an empty field is a real choice — "let the provider
            // decide" — so it is stored as empty rather than skipped, or
            // clearing the field could never take effect.
            cur.analysis_effort = Some(form.analysis_effort.trim().to_string());
            cur.image_effort = Some(form.image_effort.trim().to_string());
            cur.image_api_family = Some(form.image_api_family.trim().to_string());
            // Stored blank for OpenAI's call, so the choice can undo a template.
            // The delivery root (M8), same explicit-blank rule as the two
            // efforts: an emptied field is a real choice ("the default
            // ./out"), so it is STORED empty rather than skipped, or clearing
            // it could never take effect. `update_local_settings` drops the
            // memo, so the next export claim reads this value.
            cur.out_dir = Some(form.out_dir.trim().to_string());
            // The interpreter (M1-3), same explicit-blank rule: an emptied
            // field is the real choice "use the platform default", and only a
            // STORED blank can undo a path saved earlier.
            cur.python_bin = Some(form.python_bin.trim().to_string());
            // Secrets: only overwrite when a non-empty value was actually
            // typed — and a typed key is FOR the endpoint on screen beside
            // it, so record that home (`config::file_key_for` enforces it at
            // load: a later provider flip or base edit cannot re-route it).
            let ak = form.analysis_api_key.trim().to_string();
            let ik = form.image_api_key.trim().to_string();
            if !ak.is_empty() {
                cur.analysis_api_key = Some(ak.clone());
                cur.analysis_api_key_base =
                    cur.analysis_base_url.clone().filter(|s| !s.trim().is_empty());
            }
            if !ik.is_empty() {
                cur.image_api_key = Some(ik.clone());
                cur.image_api_key_base =
                    cur.image_base_url.clone().filter(|s| !s.trim().is_empty());
            }
            // ONE DeepSeek key for both roles. With the DeepSeek preset both
            // endpoints are api.deepseek.com, so a key typed in either API Key
            // field fills the other role when that role has no key of its own
            // (a key saved for another endpoint is refused at load by its
            // recorded home, so it does not block this).
            if let Some((key, base)) = autoshade::config::fill_deepseek_key(
                cur.analysis_base_url.as_deref(),
                cur.image_base_url.as_deref(),
                &ik,
                cur.analysis_api_key.as_deref().is_some_and(|s| !s.is_empty()),
            ) {
                cur.analysis_api_key = Some(key);
                cur.analysis_api_key_base = base;
            }
            if let Some((key, base)) = autoshade::config::fill_deepseek_key(
                cur.image_base_url.as_deref(),
                cur.analysis_base_url.as_deref(),
                &ak,
                cur.image_api_key.as_deref().is_some_and(|s| !s.is_empty()),
            ) {
                cur.image_api_key = Some(key);
                cur.image_api_key_base = base;
            }
        });
        // ComfyUI settings live in their own file (comfy.local.json): the
        // generative backend has its own surface, so a failed/absent Comfy
        // install must not block saving the AI settings above.
        {
            let mut comfy = autoshade::comfy::Settings::load();
            comfy.cli = self.ui.settings.comfy_cli.trim().to_string();
            comfy.workspace = self.ui.settings.comfy_workspace.trim().to_string();
            comfy.base_dir = self.ui.settings.comfy_base_dir.trim().to_string();
            comfy.reimagine_workflow = self.ui.settings.comfy_reimagine_workflow.trim().to_string();
            comfy.fill_workflow = self.ui.settings.comfy_fill_workflow.trim().to_string();
            if let Err(e) = comfy.save() {
                self.ui.settings.status = trf(
                    self.lang,
                    "settings saved, but the ComfyUI paths failed to save: {err}",
                    &[("err", &e.to_string())],
                );
            }
        }
        // Did the user type a key on this save? Asked BEFORE the fields are
        // cleared, so the refusal check below can tell "typed and rejected"
        // from "left blank on purpose".
        let typed_image = !self.ui.settings.image_api_key.trim().is_empty();
        let typed_analysis = !self.ui.settings.analysis_api_key.trim().is_empty();
        match saved {
            Ok(p) => {
                self.ui.settings.analysis_api_key.clear();
                self.ui.settings.image_api_key.clear();
                // Presence reflects the RESOLVED config (file merged with env) —
                // deriving it from the file alone told a user whose key lives in
                // OPENAI_API_KEY that no key was set right after saving.
                let cfg = autoshade::config::Config::load();
                self.ui.settings.analysis_key_present = cfg.analysis_api_key.is_some();
                self.ui.settings.image_key_present = cfg.openai_api_key.is_some();
                // Normal's sidebar says whether its looks use the AI.
                self.normal.key_present = Some(cfg.openai_api_key.is_some());
                // A key that cannot appear in an HTTP header is REFUSED by
                // Config::load (`header_safe_key` — a newline or space from a
                // paste is the usual cause). The refusal only prints to
                // stderr, which the windowed GUI does not show, so "saved"
                // beside a still-empty key state was the whole story the user
                // got. Say it here instead.
                let refused = (typed_image && !self.ui.settings.image_key_present)
                    || (typed_analysis && !self.ui.settings.analysis_key_present);
                // The file's HOME decides what `Config::load` reads back:
                // under the shared temp root (no per-user data directory on
                // this machine) it is not trusted with a key or an endpoint,
                // and only the models survive the merge. Said HERE — the
                // loader's warning goes to stderr, which the windowed GUI
                // never shows, so "saved" was the whole story until 2026-09-24.
                let shared = autoshade::config::load_local_settings_from().1
                    == autoshade::config::SettingsOrigin::SharedRoot;
                self.ui.settings.status = if refused {
                    tr(self.lang, "saved, but the key was not accepted — it contains characters that cannot appear in an HTTP header (a stray space or newline from a copy/paste?). Re-copy it and save again.").into()
                } else if shared {
                    trf(self.lang, "saved → {path} — but this file sits in the shared temp folder, which is not trusted with keys: the API key and base URL fields will be ignored (only the models are read). Set AUTOSHADE_DATA_DIR to a folder only you can write and save again.", &[("path", &p.display().to_string())])
                } else {
                    trf(self.lang, "saved → {path}", &[("path", &p.display().to_string())])
                };
                self.status = tr(self.lang, "settings saved — applies to the next AI call (Analyze / Fill / Generate image)").into();
                // The interpreter field may have just changed: measure it now
                // and let the note ride the same line (a wrong one would fail
                // every sidecar later with a different error).
                if let Some(note) = self.refresh_python_check() {
                    self.status.push_str(" — ");
                    self.status.push_str(&note);
                }
            }
            Err(e) => {
                self.ui.settings.status =
                    trf(self.lang, "save failed: {err}", &[("err", &e.to_string())])
            }
        }
    }

    /// Fetch one role's model ids (`GET /models`) on a worker thread and fill that
    /// role's pick-lists. Uses the key/base typed in the form if present, else the
    /// saved config — so it works whether or not the user has saved a key yet.
    ///
    /// The base and key resolve HERE, on the UI thread, so the catalogue's
    /// `from_base`/`from_key` stamps describe exactly what the worker sends
    /// (the worker used to re-load Config on its own thread, so the stamp and
    /// the request could disagree about which credential was used).
    pub(crate) fn fetch_models(&mut self, role: ModelRole) {
        let (form_key, form_base) = match role {
            ModelRole::Image => (
                self.ui.settings.image_api_key.trim().to_string(),
                self.ui.settings.image_base_url.trim().to_string(),
            ),
            ModelRole::Analysis => (
                self.ui.settings.analysis_api_key.trim().to_string(),
                self.ui.settings.analysis_base_url.trim().to_string(),
            ),
        };
        let cfg = autoshade::config::Config::load();
        let (cfg_base, cfg_key) = match role {
            ModelRole::Image => (cfg.openai_base_url, cfg.openai_api_key),
            ModelRole::Analysis => (cfg.analysis_base_url, cfg.analysis_api_key),
        };
        let base = if form_base.is_empty() { cfg_base.clone() } else { form_base };
        // The SAVED key is only ever sent to the endpoint it is saved beside —
        // `config::file_key_for` is this same rule at load time. Probing a
        // freshly TYPED endpoint takes a typed (or first saved) credential;
        // silently reusing the old endpoint's key against the new URL is the
        // provider-flip misroute in miniature. With no usable key the probe
        // fails immediately with "no API key — set one in Settings", which is
        // the accurate instruction.
        let key = if !form_key.is_empty() {
            form_key
        } else if autoshade::config::same_endpoint(&base, &cfg_base) {
            cfg_key.unwrap_or_default()
        } else {
            String::new()
        };
        let cat = self.catalogue_mut(role);
        if cat.fetching {
            return;
        }
        cat.fetching = true;
        // Any dispatch — auto or manual — spends this role's convenience probe.
        cat.autofetched = true;
        // Drop the previous endpoint's lists NOW (bumping `gen`, so an older
        // in-flight completion can no longer land) and stamp what this fetch
        // targets: a failed fetch then leaves empty lists (grounded fallbacks)
        // rather than ids from the old server under the new URL's name.
        cat.clear();
        cat.from_base = base.clone();
        cat.from_key = key_fingerprint(&key);
        let generation = cat.generation;
        self.ui.settings.status = tr(self.lang, "fetching models…").into();
        // spawn_worker's catch_unwind guarantees the UI's `fetching` flag
        // always clears — a panic still delivers Msg::Models(role, gen, Err)
        // (this site used to hand-roll a Drop guard for exactly that; the
        // helper now covers every worker uniformly).
        self.spawn_worker(
            move || Msg::Models(role, generation, autoshade::openai_models::list_models(&base, &key)),
            move |e| Msg::Models(role, generation, Err(e)),
        );
    }

    pub(crate) fn catalogue_mut(&mut self, role: ModelRole) -> &mut ModelCatalogue {
        match role {
            ModelRole::Image => &mut self.ui.settings.image_models,
            ModelRole::Analysis => &mut self.ui.settings.analysis_models,
        }
    }

    /// 「准备 Python 环境」: create the managed environment with `uv` and
    /// install the sidecar requirements into it. Explicit, because the wheel
    /// set is a multi-gigabyte download; the app must not start one on its own.
    pub(crate) fn start_python_env_install(&mut self) {
        if self.busy {
            return;
        }
        let lang = self.lang;
        let plan = match autoshade::python_env::Plan::resolve() {
            Ok(p) => p,
            Err(e) => {
                let t = format!("{e:#}");
                self.status = t.clone();
                self.toast(ToastKind::Error, t);
                return;
            }
        };
        self.busy = true;
        self.ui.python_env_line = None;
        self.status = tr(
            lang,
            "Preparing the Python environment (uv)… the download is a few gigabytes",
        )
        .into();
        let tx = self.tx.clone();
        self.spawn_worker(
            move || {
                // uv writes pip's progress to stderr; each line lands in the
                // Settings row, so a long download says what it is doing.
                let mut send = |line: &str| {
                    let _ = tx.send(Msg::PythonEnvLine(line.to_string()));
                };
                Msg::PythonEnvReady(plan.install(&mut send).map(|p| p.display().to_string()))
            },
            |e| Msg::PythonEnvReady(Err(e)),
        );
    }

    /// The install's landing: the interpreter it produced, or the failure.
    pub(crate) fn on_python_env_ready(&mut self, lang: Lang, res: anyhow::Result<String>) {
        self.busy = false;
        match res {
            Ok(path) => {
                self.ui.python_env_line = Some(path.clone());
                self.status = trf(lang, "Python environment ready: {path}", &[("path", &path)]);
                // The environment is ready now: re-measure so the Settings row
                // and any later note describe the new interpreter.
                self.refresh_python_check();
            }
            Err(e) => {
                let t = format!("{e:#}");
                self.ui.python_env_line = Some(t.clone());
                self.status = t.clone();
                self.toast(ToastKind::Error, t);
            }
        }
    }

    /// Measure what the AI sidecars would run (`python_env::inspect`) and keep
    /// the fact on the app; returns the line a launch has to say about it, when
    /// it has to say one.
    pub(crate) fn refresh_python_check(&mut self) -> Option<String> {
        let check = autoshade::python_env::inspect(&autoshade::config::Config::load());
        let words = crate::panels::settings::python_startup_words(self.lang, &check);
        self.ui.python_check = Some(check);
        words
    }

    /// Move one mask a row up/down. Mask ORDER is render semantics (masks
    /// stack sequentially; a later mask's range sees earlier masks' output), so
    /// the list order is editable — and the armed-tool indices remap with it.
    pub(crate) fn move_mask(&mut self, i: usize, up: bool) {
        let j = if up {
            if i == 0 { return }
            i - 1
        } else {
            if i + 1 >= self.recipe.masks.len() { return }
            i + 1
        };
        self.recipe.masks.swap(i, j);
        self.remap_mask_indices(|s| Some(if s == i { j } else if s == j { i } else { s }));
        self.overlay.overlay_stale = true;
        self.dirty = true;
    }

    /// Duplicate a mask: its tuned sliders come along, its rasters are DETACHED
    /// copies (the version-load rule), so the twins never share a mutable file.
    pub(crate) fn duplicate_mask(&mut self, lang: Lang, i: usize) {
        if self.recipe.masks.len() >= 64 {
            let t = tr(lang, "mask limit reached (64) — delete one first");
            self.toast(ToastKind::Error, t.to_string());
            return;
        }
        let Some(mut clone) = self.recipe.masks.get(i).cloned() else { return };
        if let Some(p) = self.src_path.as_deref() {
            let mut tmp = EditRecipe { masks: vec![clone], ..Default::default() };
            autoshade::store::detach_rasters(p, &mut tmp, "mask-dup");
            clone = tmp.masks.remove(0);
        }
        if !clone.name.is_empty() {
            clone.name = format!("{} 2", clone.name);
        }
        self.recipe.masks.insert(i + 1, clone);
        self.remap_mask_indices(|s| Some(if s > i { s + 1 } else { s }));
        self.masks.sel_mask = Some(i + 1);
        self.masks.sel_component = None;
        self.overlay.overlay_stale = true;
        // The remap maps an armed Redraw(i)/brush(i) to ITSELF (s > i is false
        // at s == i) while the selection jumps to the copy — armed on the
        // original, pointed at the duplicate. Selection moved ⇒ same rule.
        if self.disarm_selection_bound_tools(self.masks.sel_mask) {
            let t = tr(
                self.lang,
                "the raster-edit brush session ended — you selected another mask; its unbaked strokes were discarded",
            );
            self.toast(ToastKind::Error, t.to_string());
        }
        self.dirty = true;
    }

    /// Delete one version snapshot. The develop store is locked NO-WAIT: a
    /// foreground 🗑 click must fail into the error arm when another process
    /// holds the develop, not hang.
    pub(crate) fn delete_version_clicked(&mut self, lang: Lang, src: PathBuf, n: u32) {
        match autoshade::store::with_develop_lock(
            &src,
            autoshade::store::DevelopLockMode::NoWait,
            || autoshade::store::delete_version(&src, n),
        ) {
            Ok(()) => {
                self.refresh_versions();
                self.status = trf(lang, "Version v{n} deleted", &[("n", &n.to_string())]);
            }
            Err(e) => {
                self.persist_postponed(&e, "Delete v{n} failed: {err}", &[("n", &n.to_string())]);
            }
        }
    }

    /// Migrate every gallery photo's sidecars from a user-picked pre-store
    /// ./out folder (Settings button). Worker-side: one read_dir per photo
    /// over a possibly huge folder is I/O the UI thread must not pay.
    pub(crate) fn start_import_legacy(&mut self, dir: PathBuf) {
        if self.busy {
            return;
        }
        let lang = self.lang;
        let photos = self.gal.gallery.clone();
        if photos.is_empty() {
            self.status = tr(
                lang,
                "Open a folder first — import migrates the photos currently in the gallery",
            )
            .into();
            return;
        }
        self.busy = true;
        self.status = trf(
            lang,
            "Importing develops from {path} …",
            &[("path", &dir.display().to_string())],
        );
        self.spawn_worker(
            move || {
                // One directory scan for the whole gallery — the per-photo
                // migrate_legacy_from re-scanned the (possibly huge) legacy
                // folder once per photo, making import O(photos × entries).
                // FACTS out (L12#4): the landing renders the summary.
                let n = autoshade::store::migrate_legacy_from_many(&dir, &photos);
                Msg::LegacyImported(Ok((n, dir)))
            },
            |e| Msg::LegacyImported(Err(e)),
        );
    }

    /// Read the reference library's status onto a worker thread (R23-2).
    ///
    /// Off the UI thread because the file it parses can reach tens of MB, and
    /// CACHED in `style_info` because the panel that shows it draws every
    /// frame. Re-run after a build; nothing else invalidates it (another
    /// process rebuilding the shared library mid-session is the one staleness
    /// this accepts — the alternative is a stat per frame forever).
    pub(crate) fn start_style_info(&mut self) {
        if self.ai.style_info_loading {
            return;
        }
        self.ai.style_info_loading = true;
        self.spawn_worker(
            || Msg::StyleInfo(Box::new(autoshade::library::summary())),
            // A read that PANICS must still clear the flag, or the status line
            // says "reading…" for the rest of the session.
            |e| Msg::StyleInfo(Box::new(autoshade::library::Summary::Unusable(e.to_string()))),
        );
    }

    /// Build (or rebuild) the reference library from a folder of finished
    /// photos (R23-2, feedback #6: the desktop app's own entry point).
    ///
    /// Background, with progress, and NOT `busy`-gated — freezing the whole
    /// app for minutes would be its own defect — so only this button gates on
    /// `style_build_inflight`. No cancel: the build has no cancellation
    /// checkpoints, and closing the window mid-build is safe because the
    /// library is published atomically (`Library::save`), so a killed process
    /// leaves the previous library intact.
    pub(crate) fn start_style_build(&mut self, dir: PathBuf) {
        if self.ai.style_build_inflight {
            return;
        }
        self.ai.style_build_inflight = true;
        self.ai.style_build_progress = None;
        self.status = trf(
            self.lang,
            "Building the reference library from {path} … the first build measures every photo, so a big folder takes minutes",
            &[("path", &abs_display(&dir))],
        );
        // The progress channel is the app's own sender: `spawn_worker` delivers
        // ONE terminal message, and this build has to report while it runs.
        let tx = self.tx.clone();
        let ctx = self.egui_ctx.clone();
        // Resolved HERE, on the UI thread: a preference flipped mid-build must
        // not change what this build is doing.
        let describe = autoshade::library::DescribeSwitch::resolve(None, self.aip.style_describe);
        let raw_root = self.aip.style_raw_dir.clone().filter(|d| d.is_dir());
        self.spawn_worker(
            move || {
                let progress = |p: autoshade::library::BuildProgress| {
                    let _ = tx.send(Msg::StyleBuildProgress { stage: p.stage, done: p.done, total: p.total });
                    // An mpsc send does not wake egui (see `spawn_worker`).
                    ctx.request_repaint();
                };
                let sidecars = autoshade::library::Sidecars::from_config(&autoshade::config::Config::load());
                match autoshade::library::rebuild(&dir, &sidecars, describe, raw_root.as_deref(), &progress) {
                    Ok(lib) => Msg::StyleBuilt(Box::new(StyleBuildOutcome::Saved {
                        total: lib.entries.len(),
                        phrased: lib.entries.iter().filter(|e| e.phrases.is_some()).count(),
                        dir,
                    })),
                    Err(e) => Msg::StyleBuilt(Box::new(StyleBuildOutcome::Failed { err: format!("{e:#}") })),
                }
            },
            |e| Msg::StyleBuilt(Box::new(StyleBuildOutcome::Failed { err: e.to_string() })),
        );
    }

    /// The analyze worker every analysis starter shares — 「AI Analyze」,
    /// 「AI adjust」 and Normal's one-click looks (`panels/normal.rs`): arms
    /// the cancel, marks the wire busy and runs `produce_recipe` on a worker
    /// thread inside `library`'s scope (`library::with_library`), landing as
    /// `Msg::Analyzed` under the returned epoch. The caller has already set
    /// `busy` and its status line.
    pub(crate) fn spawn_analyze(
        &mut self,
        path: PathBuf,
        pixels: Option<PathBuf>,
        guidance: Option<String>,
        base: Option<EditRecipe>,
        req: autoshade::pipeline::GradeRequest,
        library: &'static str,
    ) -> u64 {
        #[cfg(test)]
        {
            self.normal.last_request = Some((req, guidance.clone(), library));
        }
        #[cfg(test)]
        let runner = self.normal.runner;
        // Every other network starter arms this; Analyze — the one a stalled
        // endpoint holds the longest — did not, so a hung stream left no ✕
        // and only killing the process recovered. Abandon semantics (like the
        // local-compute starters): the advisor has no cancel checkpoints, so
        // ✕ frees the UI immediately, the call dies at its stall/progress
        // deadline, and the stale-epoch result is discarded on arrival.
        let (epoch, _flag) = self.arm_cancel();
        self.tools.analyze_inflight = true;
        self.spawn_worker(
            move || {
                // Test seam: a headless test stands in for the paid chain.
                #[cfg(test)]
                if let Some(run) = runner {
                    return Msg::Analyzed(epoch, Box::new(run(&path, &req)));
                }
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&path))); // full-frame commit budget (budget.rs)
                // Config is reloaded in-thread (cheap) so we don't need it to be Clone.
                let cfg = autoshade::config::Config::load();
                let res = autoshade::library::with_library(library, || {
                    autoshade::pipeline::produce_recipe(
                        &path,
                        pixels.as_deref(),
                        &cfg,
                        false,
                        guidance.as_deref(),
                        base.as_ref(),
                        req,
                        // Interactive single-photo analyze: the visual closed
                        // loop IS the R20 strengthening (cost disclosed in the
                        // button's hover text; batch surfaces pass false).
                        true,
                        autoshade::diag::stderr(),
                    )
                });
                Msg::Analyzed(epoch, Box::new(res))
            },
            move |e| Msg::Analyzed(epoch, Box::new(Err(e))),
        );
        epoch
    }

    /// Reverse-fit ("match"): statistically solve the develop parameters that
    /// map the SOURCE neutral onto a finished rendition of this frame — the
    /// active「AI 生成」variant, or (R23-6) any reference file the user chose.
    /// The result lands as a new「反推」variant (base = source neutral, look in
    /// the recipe), and for a RAW the XMP sidecar is written immediately. The
    /// fit itself is deterministic and local; only the optional review calls
    /// out.
    ///
    /// The base is `source_preview`, NOT `base_preview`: after a reimagine the
    /// active variant's base IS the generated raster, and fitting a rendition
    /// onto itself would recover ~neutral. Fit must map the negative → the look.
    pub(crate) fn start_fit(&mut self) {
        let (Some(base), Some(tgt)) = (self.source_preview.clone(), self.fit_target())
        else {
            return;
        };
        if self.busy {
            return;
        }
        let src_path = self.src_path.clone();
        // The source the fold names (`fit_source_index`: the user's pick, else
        // the automatic negative): the source frame below, the ◭ card's origin
        // and the persisted pixel link all read this ONE capture, so the three
        // cannot disagree.
        let negative = self.fit_negative();
        let fit_strength = self.fit_strength();
        let zoned = self.fit.zoned_fit;
        let zoned_regions = if self.fit.zoned_four_regions {
            autoshade::fit_zoned::semantic::MAX_SEMANTIC_REGIONS
        } else {
            autoshade::fit_zoned::semantic::DEFAULT_SEMANTIC_REGIONS
        };
        let ai_judge = self.fit.fit_ai_judge;
        // The deep path IS the review, iterated — it cannot run without it,
        // and the checkbox is disabled accordingly, but the worker must not
        // depend on a UI gate for a spending decision.
        let deep = self.fit.fit_deep && ai_judge;
        let lang = self.lang;
        self.busy = true;
        let mut running = if zoned {
            tr(lang, "Reverse-fitting… (global + semantic/ranges + spatial tiles)").to_string()
        } else {
            tr(lang, "Reverse-fitting… (statistical fit, local compute)").to_string()
        };
        if deep {
            running.push_str(tr(lang, " · retry: AI review BEFORE saving, up to one guided retry"));
        } else if ai_judge {
            // The review runs AFTER the local fit and is a network call — the
            // busy line must own that extra wait up front.
            running.push_str(tr(lang, " · then AI review (vision call)"));
        }
        self.status = running;
        self.spawn_worker(
            move || {
                // Full-frame commit budget (budget.rs); the reference's own
                // header raises the floor when it is the bigger file.
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&tgt)));
                let res = (|| -> anyhow::Result<FitOutcome> {
                    // THE raw-vs-baked dispatch (R22-1): the reference may now
                    // be any file the user picked, including a RAW, which
                    // `decode::load_image` refuses by name. `source_pixels`
                    // develops a RAW neutrally and loads a baked file — the
                    // one branch, never hand-copied. The cap is comfortably
                    // above both consumers (the fit analyses at 384, the judge
                    // at 1024) and preserves aspect, which the same-frame
                    // check reads.
                    const FIT_REF_EDGE: u32 = 2048;
                    let target = autoshade::render::source_pixels(&tgt, Some(FIT_REF_EDGE))?;
                    // The gate runs at PERSIST time (below), not up front:
                    // a pre-fit snapshot left the whole multi-minute
                    // segmentation as a race window — an explicit save
                    // landing during the fit was then overwritten
                    // unversioned. (Claimed unique rasters already made the
                    // fit itself harmless to the saved develop.)
                    // Automatic local pass only when enabled AND the photo
                    // has a real path (the semantic raster needs a home).
                    // Segmentation success produces semantic sky/land; an
                    // unavailable sidecar falls through to native luminance
                    // ranges inside fit_recipe_zoned_with_regions. The raster
                    // gets a FRESH unique name per fit (mask-zone-sky.png,
                    // -2, -3, …, create_new-claimed like every master name):
                    // the old fixed name was rewritten IN PLACE before the
                    // recipe landed, so a crash or a failed recipe write left
                    // the still-live saved recipe pointing at the NEW bytes —
                    // the same shared-file corruption class that once made
                    // the fit's cleanup delete 「AI select sky」's raster.
                    // With unique names the saved develop's raster is never
                    // touched, so the pass no longer needs the backup gate;
                    // superseded rasters stay on disk (tiny greyscale PNGs —
                    // v<N> snapshots freeze their own copies regardless).
                    // R15/R16: the photo's calibration composes INTO the
                    // solve — the recipe starts from the calibration-only
                    // base, every closed-loop candidate render is the
                    // canvas's own one-pass user(base(x)), and the
                    // deliverable carries the calibration by construction
                    // (no separate stamp, no two-pass seed render). Fitting
                    // from the raw neutral burned the bounded model on
                    // re-deriving the 0.6–1.4 EV camera look (real pair:
                    // saturation pegged, cast curves vetoed, a helpful sky
                    // zone dropped); the v0.24.0 pre-rendered seed then
                    // clipped saturated channels at the pass boundary —
                    // both retired by pipeline::calibration_recipe +
                    // fit_recipe_from.
                    // …and since R33 §A the SOURCE FRAME comes from the same
                    // one function the CLI uses: a 2048-px neutral develop of
                    // the sensor frame. The panel's 1280-px `source_preview`
                    // is a different image of the same capture — different
                    // resampling, different sharpening — and the structural
                    // reading that picks Full vs Atmosphere is measured on
                    // whichever image the entry chose. A non-RAW source has no
                    // sensor frame, so it keeps the preview it already has.
                    // …and when the ▣ card hangs off an in-place master
                    // (2026-09-13), THAT is the source frame: a neutral
                    // develop already, loaded as-is through the same dispatch
                    // (a full-res master is capped to the fit's working edge
                    // by the same area average the RAW arm downscales with),
                    // with the photo's calibration composing on top exactly
                    // as the ▣ card renders it — the master carries none.
                    // The fit source is a `render::Plate` held while this fit
                    // runs (`plate`): a RAW's keeps its camera frame, so the
                    // candidates that move the white are rendered with it in
                    // the profile stage, and every plate says which frame its
                    // pixels are in (a master's marker, v1.17.0).
                    let (mut plate, fit_base, raw_source) = match (src_path.as_deref(), negative.as_deref()) {
                        (Some(p), Some(master)) => {
                            let cal = autoshade::pipeline::calibration_recipe(
                                autoshade::pipeline::fit_calibration(p),
                            );
                            let pixels = autoshade::render::source_pixels(
                                master,
                                Some(autoshade::pipeline::FIT_SOURCE_EDGE),
                            )?;
                            (autoshade::pipeline::fit_source_baked(master, pixels)?, cal, None)
                        }
                        (Some(p), None) if autoshade::decode::is_raw(p) => {
                            let (frame, cal) = autoshade::pipeline::fit_source(p)?;
                            (frame, cal, Some(p))
                        }
                        (Some(p), _) => {
                            let cal = autoshade::pipeline::calibration_recipe(
                                autoshade::pipeline::fit_calibration(p),
                            );
                            (autoshade::pipeline::fit_source_baked(p, (*base).clone())?, cal, None)
                        }
                        (None, _) => (autoshade::render::Plate::baked((*base).clone()), Default::default(), None),
                    };
                    // The source in the target's frame — the CLI's rule
                    // (`pipeline::fit_target`). The target is paired and
                    // judged as it was exported.
                    let mut fit_base = fit_base;
                    let fitted_target =
                        autoshade::pipeline::fit_target(&mut plate, raw_source, &target, &mut fit_base)?;
                    let base = plate.shared();
                    let fit_target = &fitted_target.image;
                    // ONE judge call site for both orderings (R23-6): the
                    // default path reviews AFTER the persist, the deep path
                    // reviews BEFORE it and again after its retry, and a
                    // second hand-written copy of "encode both frames at
                    // detail:high tile size and ask FitMatch" is exactly how
                    // the two would drift into judging different pixels.
                    // Domain: `rep.recipe` renders from the source NEUTRAL
                    // (`base` — the room the composed fit solved in), so that
                    // render against the target is the pair the fit's own
                    // residual measures.
                    const JUDGE_EDGE: u32 = 1024; // detail:high tiles at 512 px — 4 tiles read a grade
                    let judge_of = |recipe: &autoshade::recipe::EditRecipe|
                     -> anyhow::Result<autoshade::advisor::Judgement> {
                        let enc = |img: &image::DynamicImage| -> anyhow::Result<Vec<u8>> {
                            let mut j = Vec::new();
                            img.write_to(
                                &mut std::io::Cursor::new(&mut j),
                                image::ImageFormat::Jpeg,
                            )?;
                            Ok(j)
                        };
                        let fitted = autoshade::render::develop_preview(
                            &autoshade::render::Plate::thumbnail_of(&base, JUDGE_EDGE),
                            recipe,
                        );
                        let t_jpeg = enc(&autoshade::render::thumbnail(&target, JUDGE_EDGE, JUDGE_EDGE))?;
                        let f_jpeg = enc(&fitted)?;
                        Ok(autoshade::advisor::judge_pair(
                            autoshade::advisor::JudgeImages {
                                reference: &t_jpeg,
                                candidate: &f_jpeg,
                            },
                            autoshade::advisor::JudgeTask::FitMatch,
                            None,
                            // FitMatch scores a MATCH between two renders —
                            // the strength axis has no bearing on it (R23-3).
                            None,
                        )?)
                    };
                    // Returns the raster this run CLAIMED alongside the report:
                    // `claim_raster` hands out a fresh `mask-zone-sky[-N].png`
                    // and only the recipe that keeps the solve ever references
                    // it, so a discarded candidate would leave a zero-reference
                    // file behind (R23 review LOW-5). The claimed PATH, never a
                    // rebuilt name — the claim may be the `-2` / `-3` suffix.
                    // 完全自动 (user ruling 2026-08-26): the fit's D gate may
                    // consult the local DIFT sidecar on content-divergent
                    // pairs; failures degrade into the rationale.
                    let corr = autoshade::correspond::fit_provider(
                        autoshade::correspond::CorrespondOpts::from_config(
                            &autoshade::config::Config::load(),
                        ),
                    );
                    let run_zoned = |seg_on: bool|
                     -> anyhow::Result<(autoshade::fit::FitReport, Option<std::path::PathBuf>)> {
                        Ok(match (seg_on, &src_path) {
                            (true, Some(p)) => {
                                let cfg = autoshade::config::Config::load();
                                let seg =
                                    autoshade::segment::SegmentOpts::from_config(&cfg, "sky");
                                // Drawn in the source's frame, and marked so.
                                let mask = autoshade::store::OwnedRaster::claim(p, "mask-zone-sky")?
                                    .with_frame(autoshade::render::fit_raster_frame(&base, &fit_base));
                                let rep = autoshade::fit_zoned::fit_recipe_zoned_with_regions(
                                    &base,
                                    fit_target,
                                    &seg,
                                    &mask,
                                    &fit_base,
                                    autoshade::fit::FitOptions {
                                        strength: fit_strength,
                                        provider: Some(&corr),
                                    },
                                    zoned_regions,
                                );
                                (rep, Some(mask.into_path()))
                            }
                            _ => (
                                autoshade::fit::fit_recipe_from_with(
                                    &base,
                                    fit_target,
                                    &fit_base,
                                    autoshade::fit::FitOptions {
                                        strength: fit_strength,
                                        provider: Some(&corr),
                                    },
                                ),
                                None,
                            ),
                        })
                    };
                    // The FIRST solve's claim is deliberately not swept: this
                    // report is the one that ships unless a candidate beats it,
                    // and a candidate can only be the zoned pass when this solve
                    // was NOT zoned (`hint_action`'s `zoned_used` fact), i.e.
                    // when it claimed nothing at all.
                    let (mut rep, _first_claim) = run_zoned(zoned)?;
                    autoshade::pipeline::note_target_geometry(&mut rep, fitted_target.geometry);
                    // FACTS, not prose (L12#4): the landing renders these
                    // with the language live when the result LANDS — the old
                    // trf calls here used the language captured at spawn,
                    // minutes stale after a segmentation run.
                    let mut status: Vec<FitNote> = Vec::new();
                    // FIRST, before every other landing fact: which solver ran
                    // and at which pairing scale. Everything else on this line
                    // is a consequence of those two.
                    let reading = |r: Option<autoshade::fit::Divergence>| {
                        r.map_or_else(|| "unmeasured".to_string(), |r| format!("{:.3}", r.d))
                    };
                    status.push(FitNote::SolverMode {
                        atmosphere: rep.mode == autoshade::fit::FitMode::Atmosphere,
                    });
                    status.push(FitNote::SolverPairing {
                        pixel: rep.pairing == autoshade::fit::PairingScale::Pixel,
                        fine: reading(rep.divergence),
                        coarse: reading(rep.divergence_coarse),
                    });
                    // ── the DEEP path (R23-6 D). Everything here happens
                    // BEFORE the persist below, which is the whole point and
                    // the deliberate revision of R20's ordering — recorded at
                    // the persist site.
                    //
                    // Discipline, copied wholesale from `visual_review_round`
                    // (pipeline.rs) because it is the shape that already
                    // survived review: the retry must re-score AT LEAST as
                    // high to be kept, an action that changes nothing
                    // short-circuits before spending a second call, and every
                    // failure keeps the plain solve and degrades to a note.
                    //
                    // `judged` is THREE-state, the same shape the CLI's
                    // `--deep` path uses: `None` = the deep path never ran,
                    // `Some(Ok(_))` = a verdict describing what ships,
                    // `Some(Err(_))` = the deep review ran and FAILED. The last
                    // one used to be indistinguishable from "never ran", so the
                    // informational block below bought a THIRD network attempt
                    // after two had already failed — past the two-paid-calls
                    // ceiling its own tooltip states (R23 review LOW-6).
                    let mut judged: Option<anyhow::Result<autoshade::advisor::Judgement>> = None;
                    if deep {
                        match judge_of(&rep.recipe) {
                            Ok(first) => {
                                let action = hint_action(
                                    first.hint.as_deref().unwrap_or(""),
                                    !rep.recipe.masks.is_empty(),
                                    !zoned && src_path.is_some(),
                                );
                                let candidate = match action {
                                    // The one action that needs the solver
                                    // again. A claim/segmentation failure is
                                    // not an error here: the plain solve in
                                    // hand is already a valid result.
                                    FitAction::Zoned => run_zoned(true).ok().map(|(mut r, claim)| {
                                        autoshade::pipeline::note_target_geometry(&mut r, fitted_target.geometry);
                                        (r, claim)
                                    }),
                                    FitAction::Saturation(d) => {
                                        let mut r = rep.recipe.clone();
                                        r.saturation += d;
                                        r.clamp();
                                        // Identical settings ⇒ nothing to
                                        // judge, and no second call bought.
                                        (r.saturation != rep.recipe.saturation).then(|| {
                                            // RE-DERIVED, never cloned off the
                                            // solve (R23 review MED-3): every
                                            // outcome note — the residual pair,
                                            // the joint numbers, the
                                            // unrepresented-controls diagnosis,
                                            // the terminal-reset verdict the
                                            // FitReset check below reads — is a
                                            // statement about the recipe, and
                                            // this recipe is not the one the
                                            // solver reported on.
                                            (
                                                autoshade::fit::rescore_report(
                                                    &base,
                                                    fit_target,
                                                    &r,
                                                    &fit_base,
                                                    rep.err_before,
                                                    &rep.notes,
                                                ),
                                                None,
                                            )
                                        })
                                    }
                                    FitAction::None => None,
                                };
                                let outcome = match candidate {
                                    Some((mut cand, claimed)) => {
                                        let verdict = judge_of(&cand.recipe);
                                        let keep = matches!(
                                            &verdict,
                                            Ok(second) if second.score >= first.score
                                        );
                                        if !keep {
                                            // The candidate is gone, so its
                                            // claimed raster is now referenced
                                            // by nothing (R23 review LOW-5).
                                            // ONLY this run's claim is touched,
                                            // by the path `claim_raster`
                                            // returned. Best effort by design:
                                            // a failure leaves an inert
                                            // greyscale PNG the user can delete,
                                            // and reporting it would be noise on
                                            // a fit that succeeded.
                                            if let Some(p) = &claimed {
                                                let _ = std::fs::remove_file(p);
                                            }
                                        }
                                        match verdict {
                                            Ok(second) if keep => {
                                                autoshade::rationale::push_note(
                                                    &mut cand.recipe.rationale,
                                                    &mut cand.notes,
                                                    autoshade::rationale::Note::new(
                                                        autoshade::rationale::keys::FIT_NOTE_DEEP_ADOPTED,
                                                        vec![
                                                            ("score1", format!("{:.0}", first.score)),
                                                            ("score2", format!("{:.0}", second.score)),
                                                            ("action", action.tag().to_string()),
                                                        ],
                                                    ),
                                                );
                                                rep = cand;
                                                judged = Some(Ok(second));
                                                DeepFitOutcome::Adopted
                                            }
                                            // Lower, or un-judgeable: the
                                            // plain solve stands, and the
                                            // FIRST verdict is the one that
                                            // describes what ships.
                                            Ok(_) => {
                                                judged = Some(Ok(first));
                                                DeepFitOutcome::Discarded
                                            }
                                            Err(e) => {
                                                status.push(FitNote::AiReviewFailed(e.to_string()));
                                                judged = Some(Ok(first));
                                                DeepFitOutcome::Discarded
                                            }
                                        }
                                    }
                                    // No candidate, and WHY differs (R23
                                    // review LOW-4): `FitAction::None` is the
                                    // reviewer having named nothing this app
                                    // can do, while a selected action that
                                    // produced no candidate is the app failing
                                    // to carry out a move it did choose — the
                                    // zoned re-solve errored, or the saturation
                                    // step clamped back to the value in hand.
                                    None => {
                                        judged = Some(Ok(first));
                                        match action {
                                            FitAction::None => DeepFitOutcome::NothingActionable,
                                            _ => DeepFitOutcome::ActionDidNotRun,
                                        }
                                    }
                                };
                                status.push(FitNote::DeepFit { action: action.tag(), outcome });
                            }
                            Err(e) => {
                                status.push(FitNote::AiReviewFailed(e.to_string()));
                                judged = Some(Err(e));
                            }
                        }
                    }
                    status.extend(fit_mask_notes(&rep.recipe, &rep.notes));
                    // R23-6 A-3: the terminal do-no-harm reset is "the
                    // reverse-fit did nothing", and a line inside a rationale
                    // block is not where a user finds that out.
                    if rep.notes.iter().any(|n| n.key == autoshade::rationale::keys::FIT_NOTE_REGRESSED)
                    {
                        status.push(FitNote::FitReset);
                    }
                    if rep
                        .notes
                        .iter()
                        .any(|n| n.key == autoshade::rationale::keys::FIT_NOTE_NOT_SAME_FRAME)
                    {
                        status.push(FitNote::ReferenceNotSameFrame);
                    }
                    let mut persisted = false;
                    if let Some(p) = &src_path {
                        // Worker thread ⇒ Wait, ONE lock across gate →
                        // recipe → pixel-link clear → XMP: still the
                        // narrowest gate window, now with no other process
                        // interleaving between the halves. The
                        // identical-content skip keeps the gate from
                        // spamming versions on repeated fits.
                        let persist = autoshade::store::with_develop_lock(
                            p,
                            autoshade::store::DevelopLockMode::Wait,
                            || -> std::io::Result<()> {
                        let backed = autoshade::store::backup_saved_develop(p, Some(&rep.recipe));
                        // Persist the fit losslessly: recipe.json carries the
                        // bitmap zone masks + exact recolour gains the XMP cannot,
                        // so reopening the photo restores the whole fit. The
                        // snapshot above is the gate — if it FAILED, skip the
                        // persist entirely (the fit stays on the canvas with
                        // ● unsaved; Ctrl+S overwrites explicitly).
                        match &backed {
                            Ok(backed) => {
                              // ONE single-generation commit (the Ctrl+S
                              // rule, L03): the fit's recipe and the
                              // pixel link land together. The fit is a
                              // SOURCE-based develop by construction (it maps
                              // the source neutral onto the rendition), and a
                              // stale pixels.json link surviving the recipe
                              // write made a reopen render the fit on baked
                              // pixels it was never computed against — so the
                              // link is CLEARED, unless the negative itself
                              // hangs off an in-place master (2026-09-13):
                              // then the fit WAS computed against those
                              // pixels, the ◭ card lands on them, and the
                              // link records them as the ▣ card's own Ctrl+S
                              // would (`inplace`, never `generated`).
                              // The projection rides the same generation
                              // (2026-09-13): a fit is a source develop, so
                              // it always projects; a projection this build
                              // cannot produce degrades to Keep + the
                              // XmpFailed note, the recipe still landing.
                              let (xmp_member, xmp_outcome) =
                                  match autoshade::pipeline::xmp_projection_member(
                                      p,
                                      &rep.recipe,
                                      true,
                                      autoshade::diag::stderr(),
                                  ) {
                                      Ok((m, note, _)) => (m, Ok(note)),
                                      Err(e) => (autoshade::store::CommitMember::Keep, Err(e)),
                                  };
                              let commit_res: anyhow::Result<()> = (|| {
                                  autoshade::store::commit_develop(
                                      p,
                                      autoshade::store::DevelopCommit {
                                          recipe: Some(autoshade::pipeline::recipe_store_bytes(
                                              p,
                                              &rep.recipe,
                                              autoshade::diag::stderr(),
                                          )?),
                                          pixels: match negative.as_deref() {
                                              Some(m) => autoshade::store::CommitMember::Write(
                                                  autoshade::store::pixel_source_record_bytes(
                                                      p, m, false,
                                                  )?,
                                              ),
                                              None => autoshade::store::CommitMember::Clear,
                                          },
                                          // R24-4: this worker publishes a
                                          // REVERSE-FIT into the active slot,
                                          // and the strip record's active
                                          // half has to say so — left `Keep`,
                                          // a photo whose record read
                                          // 「original」 reopened as 「▣ 原片」
                                          // holding the fit. Worker thread:
                                          // no live strip to hand over, so
                                          // the shared primitive restates the
                                          // one fact this writer owns (the
                                          // card's id/name/position stand).
                                          variants: autoshade::store::variants_member(
                                              p,
                                              autoshade::store::ActiveWrite::Kind("fitted"),
                                          )?,
                                          xmp: xmp_member,
                                      },
                                  )?;
                                  Ok(())
                              })();
                              match commit_res {
                              Err(e) => {
                                // A failed store write must NOT discard the
                                // minutes of segmentation + fitting that
                                // already succeeded — same degrade shape as
                                // the backup-gate branch below: the fit lands
                                // on the canvas unsaved.
                                status.push(FitNote::NotPersistedCommit(e.to_string()));
                              }
                              Ok(()) => {
                                persisted = true;
                                if autoshade::decode::is_raw(p) {
                                    // The recipe write ALONE decides the saved
                                    // state (same rule as Ctrl+S / Analyze):
                                    // an XMP failure must not collapse an
                                    // already-persisted fit into an error —
                                    // reopening WOULD restore it, so the UI
                                    // must agree that it saved.
                                    // The M6a list is dropped HERE: the zoned
                                    // fit's own rationale already states that
                                    // its corrections are bitmap masks the
                                    // Lightroom sidecar cannot carry (the
                                    // ZONE_ATTACHED note), so a second copy
                                    // would say it twice for the same masks.
                                    match xmp_outcome {
                                        Ok(merge_note) => {
                                            status.push(FitNote::XmpWritten(
                                                autoshade::pipeline::xmp_target(p),
                                            ));
                                            // Regenerated-not-merged: same
                                            // disclosure as Ctrl+S.
                                            if let Some(m) = merge_note {
                                                status.push(FitNote::XmpMergeNote(m));
                                            }
                                        }
                                        Err(e) => {
                                            status.push(FitNote::XmpFailed(e.to_string()));
                                        }
                                    }
                                }
                                if let Some(n) = backed {
                                    status.push(FitNote::BackedUpAs(*n));
                                }
                              }
                              }
                            }
                            Err(e) => {
                                status.push(FitNote::NotPersistedBackup(e.to_string()));
                            }
                        }
                                Ok(())
                            },
                        );
                        if let Err(e) = persist {
                            status.push(FitNote::NotPersistedLock(e.to_string()));
                        }
                    }
                    // Opt-in AI review (LLM-as-a-judge): the vision model
                    // scores how faithfully the fitted render matches the
                    // target look.
                    //
                    // ORDERING, and its history. R20 decided this review runs
                    // AFTER the persist on purpose — informational, never
                    // gating, delaying or failing the fit's landing, every
                    // failure degrading to a status note (docs/ROADMAP.md's
                    // R20 entry; Opus S5 定案). That is still the DEFAULT
                    // path below, unchanged to the byte.
                    //
                    // R23-6 revises it for one explicitly-chosen case: with
                    // 「deep」 ticked the user has asked the reviewer to act,
                    // which it cannot do after the recipe is on disk. The deep
                    // block above therefore reviews BEFORE the persist and may
                    // buy ONE bounded guided retry (user decision 2026-08-17
                    // ⑥). The R20 rule is not repealed — it is now the
                    // behaviour of the unticked box, which is also the
                    // default, and the historical entry stays as written.
                    //
                    // Domain: rep.recipe renders from the source NEUTRAL
                    // (source_preview — the room the composed fit solved in),
                    // so that render vs the target is exactly the pair the
                    // fit's own residual measures.
                    if ai_judge {
                        // The two-paid-calls ceiling, decided by a pure
                        // function so it is a pinned property and not a shape
                        // one has to re-read this closure to check
                        // (R23 review LOW-6).
                        let verdict = match FitReviewPlan::of(&judged) {
                            FitReviewPlan::Reuse => judged,
                            FitReviewPlan::Skip => None,
                            FitReviewPlan::Call => Some(judge_of(&rep.recipe)),
                        };
                        match verdict {
                            Some(Ok(j)) => status.push(FitNote::AiReview {
                                score: j.score,
                                critique: j.critique,
                                // R23-6: SHOWN, never executed on this path.
                                // R20 dropped it silently and the user paid
                                // for it.
                                hint: j.hint,
                            }),
                            Some(Err(e)) => status.push(FitNote::AiReviewFailed(e.to_string())),
                            None => {}
                        }
                    }
                    Ok(FitOutcome {
                        err_before: rep.err_before,
                        err_after: rep.err_after,
                        rationale_notes: rep.notes,
                        recipe: rep.recipe,
                        status,
                        persisted,
                        negative,
                    })
                })();
                Msg::Fitted(Box::new(res))
            },
            |e| Msg::Fitted(Box::new(Err(e))),
        );
    }
}
