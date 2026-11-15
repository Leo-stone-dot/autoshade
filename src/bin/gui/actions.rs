//! App actions: open/variants/versions/history/quit flow.

use super::*;


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

/// The persisted develop of one saved card: only the halves the baseline
/// guard compares. Identity matching happens in `logic::strip`.
struct PersistedCard<'a> {
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
        let mut app = Self {
            services: Services { egui_ctx: cc.egui_ctx.clone(), ..Services::default() },
            ..Self::default()
        };
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
            app.workspace.chrome.startup_note = Some(note);
        }
        if !app.workspace.busy {
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
        app.aip.style_strength = crate::logic::prefs::clamp01(prefs.style_strength);
        app.aip.grade_strength = crate::logic::prefs::clamp01(prefs.grade_strength);
        app.aip.fit_strength = crate::logic::prefs::clamp01(prefs.fit_strength);
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
        app.workspace.exp.format = ExportFormat::from_pref(prefs.exp_format, prefs.save_jpeg);
        app.workspace.exp.dest = ExportDest::from_pref(prefs.exp_dest);
        app.workspace.exp.last_dir = prefs.last_export_dir.clone();
        app.workspace.exp.save_denoise = prefs.save_denoise;
        app.workspace.exp.save_denoise_strength = crate::logic::prefs::clamp01(prefs.save_denoise_strength);
        app.workspace.exp.denoise_strength = crate::logic::prefs::clamp01(prefs.denoise_strength);
        app.workspace.exp.baked_denoise_strength = crate::logic::prefs::clamp01(prefs.baked_denoise_strength);
        app.workspace.exp.baked_save_denoise_strength = crate::logic::prefs::clamp01(prefs.baked_save_denoise_strength);
        app.workspace.fit.zoned_fit = prefs.zoned_fit;
        app.workspace.fit.zoned_four_regions = prefs.zoned_four_regions;
        app.workspace.fit.fit_ai_judge = prefs.fit_ai_judge;
        app.workspace.fit.fit_deep = prefs.fit_deep;
        app.workspace.fit.reimagine_retry = prefs.reimagine_retry;
        app.workspace.fit.ask_fit_after_generate = prefs.ask_fit_after_generate;
        app.workspace.genai.adjust_quality = crate::logic::prefs::choice(prefs.adjust_quality);
        app.workspace.view.mode = prefs.view_mode;
        app.workspace.exp.long_edge = prefs.exp_long_edge;
        app.workspace.exp.sharpen = crate::logic::prefs::clamp_sharpen(prefs.exp_sharpen);
        app.workspace.exp.quality = crate::logic::prefs::clamp_quality(prefs.exp_quality);
        app.workspace.chrome.show_clipping = prefs.show_clipping;
        // Restore the UI language (an older save without this key decoded to
        // `Lang::En` via `#[serde(default)]`, so this is always valid).
        app.lang = prefs.lang;
        // Restore the theme and re-apply it — main() installed the Dark
        // default before prefs were readable (same shape as the greeting).
        app.workspace.ui.theme = prefs.theme;
        app.workspace.ui.accent = prefs.accent;
        install_look(&app.services.egui_ctx, app.workspace.ui.theme, app.workspace.ui.accent);
        // The greeting was set by default() BEFORE the language was known —
        // re-issue it, or a Chinese install opens with one English line.
        app.workspace.status =
            tr(app.lang, K_ACTIONS_OPEN_A_PHOTO_OR_OPEN_A_FOLDER_TO_BROWSE_YOUR_LIB).into();
        // Each saved file advances to the current dial law; a current/future
        // file keeps the choices made under its own law. The wording of the
        // reset note uses the values the file actually carried.
        for era in prefs.prefs_era..PREFS_ERA {
            let migrated = crate::logic::prefs::migrate_era(era, prefs.denoise_strength, prefs.save_denoise_strength);
            app.workspace.exp.denoise_strength = migrated.raw_detail;
            app.workspace.exp.save_denoise_strength = migrated.raw_export;
            if let Some(v) = migrated.baked_detail {
                app.workspace.exp.baked_denoise_strength = v;
            }
            if let Some(v) = migrated.baked_export {
                app.workspace.exp.baked_save_denoise_strength = v;
            }
            if let Some((a, b)) = migrated.reset_note {
                let default = autoshade::denoise::DEFAULT_STRENGTH_RAW;
                app.workspace.chrome.startup_note = Some(trf(
                    app.lang,
                    K_ACTIONS_AI_DENOISE_STRENGTH_FOR_RAW_FILES_WAS_RESET_TO_W,
                    &[
                        ("new", &format!("{:.0}", default * 100.0)),
                        ("a", &format!("{:.0}", a * 100.0)),
                        ("b", &format!("{:.0}", b * 100.0)),
                    ],
                ));
            }
        }
        // Only known color spaces and working-preview steps: a corrupt pref
        // must not produce an unrenderable space or a 1-px / 100-MP preview.
        app.workspace.exp.space = crate::logic::prefs::known_space(prefs.exp_space);
        app.workspace.genai.preview_edge = crate::logic::prefs::known_preview_edge(prefs.preview_edge);
        if let Some(dir) = prefs.gallery_dir.filter(|d| d.is_dir()) {
            app.open_folder(dir);
        }
        // A restored folder's asynchronous status must not erase the note
        // before the user sees it. Its completion uses the same status line.
        if !app.workspace.busy {
            app.show_startup_note();
        }
    }

    /// The active card's actual pixels choose the path, including generated
    /// masters under a RAW original. An unopened app displays the RAW dials.
    pub(crate) fn denoise_uses_raw(&self) -> bool {
        crate::logic::noise::uses_raw(self.active_source_path().as_deref())
    }

    pub(crate) fn selected_denoise_strength(&self, on_export: bool) -> f32 {
        crate::logic::noise::selected_strength(
            self.denoise_uses_raw(),
            on_export,
            self.workspace.exp.denoise_strength,
            self.workspace.exp.save_denoise_strength,
            self.workspace.exp.baked_denoise_strength,
            self.workspace.exp.baked_save_denoise_strength,
        )
    }

    pub(crate) fn show_startup_note(&mut self) {
        if let Some(note) = self.workspace.chrome.startup_note.take() {
            self.workspace.status.push_str(" — ");
            self.workspace.status.push_str(&note);
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
        crate::quit::QuitState::from_app(self.workspace.busy, self.workspace.open.discard_requested, || {
            self.workspace.masks.mask_name_buf.is_some()
                || self.workspace.cards.version_name_buf.is_some()
                || self.workspace.cards.variant_name_buf.is_some()
                || !self.workspace.open.nav_stash.is_empty()
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
        let stems: Vec<String> = self.workspace.gal.gallery
            .iter()
            .map(|p| autoshade::pipeline::stem(p).to_string())
            .collect();
        for stem in &stems {
            for (script, sample) in undrawable_scripts(stem, installed) {
                if self.editor.analysis_mut().disclosed_scripts.insert(script) {
                    let t = trf(
                        self.lang,
                        K_ACTIONS_SOME_FILE_NAMES_USE_CHARACTERS_NO_INSTALLED_FONT,
                        &[("sample", &sample.to_string())],
                    );
                    self.toast(ToastKind::Error, t.clone());
                    self.workspace.status = t;
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
        if let Some(i) = self.workspace.chrome.toasts.iter().position(|t| t.text == text && t.kind == kind) {
            self.workspace.chrome.toasts.remove(i);
        }
        self.workspace.chrome.toasts.push(Toast { text, kind, born: Instant::now() });
        if self.workspace.chrome.toasts.len() > 5 {
            self.workspace.chrome.toasts.remove(0); // keep the stack readable
        }
    }

    /// A worker finished successfully: status line + a success toast, unbusy.
    pub(crate) fn done(&mut self, text: impl Into<String>) {
        let text = text.into();
        self.workspace.status = text.clone();
        self.toast(ToastKind::Success, text);
        self.workspace.busy = false;
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
        self.workspace.status = text.clone();
        self.toast(ToastKind::Error, text);
        self.workspace.busy = false;
    }

    /// Decoded-base LRU lookup (most-recent entry kept last). Missing metadata
    /// (mtime `None`) matches itself, so the cache still works where mtime is
    /// unavailable — it just loses the staleness guard there.
    pub(crate) fn cached_base(&mut self, path: &std::path::Path, edge: u32) -> Option<OpenedBase> {
        let mtime = file_stamp(path);
        let pixels = pixel_identity(path);
        crate::logic::lru::get(&mut self.workspace.open.base_cache, |(p, e, t, px)| {
            p == path && *e == edge && *t == mtime && *px == pixels
        })
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
        crate::logic::lru::insert(
            &mut self.workspace.open.base_cache,
            (path.to_path_buf(), edge, mtime, pixels),
            opened.clone(),
            |(p, e, _, _)| p == path && *e == edge,
            BASE_CACHE_CAP,
        );
    }

    /// Drop the open photo's decoded-base cache entries. Called whenever its
    /// BAKED pixel identity changes (a retouch lands, a save writes/clears
    /// pixels.json) — the source mtime doesn't move then, so the mtime guard
    /// alone would happily serve a hit with stale baked pixels.
    pub(crate) fn forget_open_base(&mut self) {
        if let Some(p) = self.editor.photo().src_path.clone() {
            crate::logic::lru::retain(&mut self.workspace.open.base_cache, |(q, _, _, _)| q != &p);
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
        crate::logic::strip::identity_matches(
            &live.id,
            live.kind,
            live_position,
            saved_kind,
            saved_position,
            saved_id,
        )
    }

    fn persisted_card_for(&self, live_position: usize) -> Option<PersistedCard<'_>> {
        let live = self.editor.doc().variants.get(live_position)?;
        let rec = match &self.editor.session().hist.saved_strip {
            Some(rec) => rec,
            None => {
                let trivial = self.editor.doc().variants.len() == 1
                    && live_position == 0
                    && crate::model::strip_is_trivial(
                        live.kind,
                        &live.id,
                        live.name.as_deref(),
                        0,
                    );
                return trivial.then_some(PersistedCard {
                    recipe: &self.editor.session().hist.saved_recipe,
                    origin: self.editor.session().hist.pixels_on_disk.as_deref(),
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
                recipe: &self.editor.session().hist.saved_recipe,
                origin: self.editor.session().hist.pixels_on_disk.as_deref(),
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
        let saved = self.persisted_card_for(self.editor.doc().active)?;
        Some(ActiveBaseline { recipe: saved.recipe, origin: saved.origin })
    }

    fn active_canvas_dirty(&self) -> bool {
        if self.editor.photo().src_path.is_none() || self.active_variant().is_none() {
            return false;
        }
        self.active_baseline().is_none_or(|saved| {
            dirty_vs(&self.editor.doc().recipe, saved.recipe)
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
        let saved = self.editor.session().hist.saved_strip.as_ref().map(|rec| {
            use crate::logic::strip::SavedCard;
            let mut cards: Vec<SavedCard<'_>> = Vec::with_capacity(rec.others.len() + 1);
            if let Some(kind) = VariantKind::from_store_str(&rec.active_kind) {
                cards.push(SavedCard {
                    kind,
                    position: rec.active_pos,
                    id: rec.active_id.as_deref(),
                    name: rec.active_name.as_deref(),
                    recipe: &self.editor.session().hist.saved_recipe,
                    origin: self.editor.session().hist.pixels_on_disk.as_deref(),
                });
            }
            cards.extend(rec.others.iter().enumerate().filter_map(|(i, entry)| {
                let kind = VariantKind::from_store_str(&entry.kind)?;
                Some(SavedCard {
                    kind,
                    position: if i < rec.active_pos { i } else { i + 1 },
                    id: entry.id.as_deref(),
                    name: entry.name.as_deref(),
                    recipe: &entry.recipe,
                    origin: entry.origin.as_deref(),
                })
            }));
            cards
        });
        crate::logic::strip::open_dirty_count(&self.editor.doc().variants, self.editor.doc().active, saved.as_deref())
    }

    pub(crate) fn inactive_dirty_variants(&self) -> usize {
        // Photos NAVIGATED AWAY FROM count too. Since batch 47 a photo is
        // stashed when only a BACKGROUND variant is dirty, so the quit dialog
        // lists it. Save-all persists each stashed photo's strip record along
        // with its active canvas (v0.22), so these are savable — the count
        // deliberately stays conservative (every stashed background variant,
        // not only those differing from their photo's on-disk record: that
        // comparison would cost a disk read per stash entry per check).
        let stashed: usize = self.workspace.open.nav_stash.values().map(|st| st.others.len()).sum();
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
            self.editor.doc().variants.len().saturating_sub(1),
        ) {
            return None;
        }
        Some(autoshade::store::VariantsRecord {
            extra: Default::default(),
            v: 1,
            active_kind: ak.store_str().to_string(),
            active_pos: self.editor.doc().active,
            others: self.editor.doc().variants
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != self.editor.doc().active)
                .map(|(_, v)| autoshade::store::VariantEntry {
                    extra: Default::default(),
                    kind: v.kind.store_str().to_string(),
                    recipe: v.recipe.clone(),
                    origin: v.origin.clone(),
                    // Hop 1 of 6 (R24-2): the live strip's identities + names
                    // to disk. An EMPTY id is written as absent — the record
                    // must not claim an identity the card does not have.
                    id: crate::model::variant_id_field(&v.id),
                    name: v.name.clone(),
                })
                .collect(),
            active_id: self.active_variant().and_then(|v| crate::model::variant_id_field(&v.id)),
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
                self.editor.session_mut().hist.saved_strip = Some(rec);
            }
            None => {
                autoshade::store::clear_variants(path)?;
                self.editor.session_mut().hist.saved_strip = None;
            }
        }
        Ok(())
    }

    /// The active variant, if a photo is open.
    pub(crate) fn active_variant(&self) -> Option<&Variant> {
        self.editor.doc().variants.get(self.editor.doc().active)
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
            Some(v) => v.origin.clone().or_else(|| self.editor.photo().src_path.clone()),
            None => self.editor.photo().src_path.clone(),
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
            trf(self.lang, K_ACTIONS_NOT_SAVED_A_DEVELOP_STORE_WRITE_FAILED, &a)
        };
        self.workspace.status = t.clone();
        self.toast(ToastKind::Error, t);
    }

    /// Every crop_mode flip goes through here (L13-1): `pan` is stored in
    /// the CURRENT window's coordinates — the committed crop normally, the
    /// full frame while the crop tool is open (`view_uv`) — so the VALUE
    /// must be rebased when the window changes. Left alone, entering the
    /// tool reinterpreted a crop-relative pan as full-frame and the
    /// viewport landed outside the crop box at higher zoom.
    pub(crate) fn set_crop_mode(&mut self, on: bool) {
        if self.workspace.crop.mode == on {
            return;
        }
        if let Some(c) = &self.editor.doc().recipe.crop {
            let (l, t) = (c.left.min(c.right), c.top.min(c.bottom));
            let (w, h) =
                ((c.right - c.left).abs().max(1e-6), (c.bottom - c.top).abs().max(1e-6));
            self.workspace.view.pan = if on {
                // crop-window coords → full-frame coords
                egui::vec2(l + self.workspace.view.pan.x * w, t + self.workspace.view.pan.y * h)
            } else {
                // full-frame coords → crop-window coords
                egui::vec2((self.workspace.view.pan.x - l) / w, (self.workspace.view.pan.y - t) / h)
            };
            // view_uv re-clamps against zoom each frame; keep it sane here.
            self.workspace.view.pan = egui::vec2(self.workspace.view.pan.x.clamp(0.0, 1.0), self.workspace.view.pan.y.clamp(0.0, 1.0));
        }
        self.workspace.crop.mode = on;
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
        self.workspace.masks.paint_mode = false;
        self.workspace.tools.clone_mode = false;
        self.workspace.tools.wb_picking = false;
        self.workspace.tools.range_picking = None;
        self.workspace.tools.point_color_picking = false;
        self.workspace.tools.lens_focus_picking = false;
        self.workspace.masks.placing_mask = None;
        self.workspace.masks.place_start = None;
        self.workspace.crop.drag = None;
        self.workspace.masks.mask_drag = None;
        self.workspace.masks.paint_last = None;
        self.end_mask_brush();
        // A preset change armed for the crop tool must die with it — the
        // flag survived Esc / Done / a hold-B detour and rewrote the box as
        // a surprise on the next crop entry (CX5-4).
        self.workspace.crop.aspect_pending = false;
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
        if self.workspace.masks.mask_brush.take().is_some() {
            self.workspace.masks.mask_brush_gray = None;
            self.workspace.masks.paint_mode = false;
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
            self.workspace.masks.placing_mask,
            Some((_, PlaceTarget::Redraw(j) | PlaceTarget::Component(j, _))) if Some(j) != keep
        ) {
            self.workspace.masks.placing_mask = None;
            self.workspace.masks.place_start = None;
        }
        // The colour sampler drops unconditionally — its 💧 label lives on
        // the row that was just left (the first fix of this class).
        self.workspace.tools.range_picking = None;
        if matches!(self.workspace.masks.mask_brush, Some((Some(j), _)) if Some(j) != keep) {
            return self.end_mask_brush();
        }
        false
    }

    /// Any canvas tool armed? (the once-hand-written OR list)
    pub(crate) fn tool_armed(&self) -> bool {
        self.workspace.crop.mode
            || self.workspace.masks.paint_mode
            || self.workspace.tools.clone_mode
            || self.workspace.tools.wb_picking
            || self.workspace.tools.range_picking.is_some()
            || self.workspace.tools.point_color_picking
            || self.workspace.masks.placing_mask.is_some()
    }

    /// Commit a pending mask-rename buffer to its own mask (the same
    /// seed-guard the panel's row-switch commit uses: only while that mask's
    /// name still equals the seed-time snapshot). Focus-independent Ctrl+S
    /// runs while the TextEdit still holds focus — without this it saved the
    /// old name and the advanced baseline dropped the rename.
    pub(crate) fn commit_mask_name_buf(&mut self) {
        if let Some((j, orig, buf)) = self.workspace.masks.mask_name_buf.clone()
            && let Some(prev) = self.editor.doc_mut().recipe.masks.get_mut(j)
            && prev.name == orig
            && buf != orig
        {
            prev.name = buf.clone();
            // Re-seed so the panel's own lost-focus commit stays a no-op.
            self.workspace.masks.mask_name_buf = Some((j, buf.clone(), buf));
        }
    }

    /// A modal layer is up — the quit confirm or the fit prompt — and owns
    /// the keyboard and the pointer (D13).
    pub(crate) fn modal_up(&self) -> bool {
        self.workspace.ui.confirm_quit || self.workspace.fit.fit_prompt.is_some()
    }

    /// Add a recipe file the user picked (our own recipe.json, or a Lightroom
    /// .xmp sidecar) to the library.
    pub(crate) fn import_recipe(&mut self) {
        // The panel only emits the verb; the dialog and the library writes are
        // the app's side effects.
        let Some(path) = rfd::FileDialog::new()
            .add_filter(tr(self.lang, K_ACTIONS_RECIPE_FILE), &["json", "xmp"])
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
            let text = trf(self.lang, K_ACTIONS_A_RECIPE_NAMED_ALREADY_EXISTS, &[("name", &name)]);
            self.recipe_status(false, text);
            return;
        }
        let recipe = match autoshade::recipe_library::read_file(&path) {
            Ok(r) => r,
            Err(e) => {
                let text = trf(self.lang, K_ACTIONS_IS_NOT_A_RECIPE, &[("file", &path.display().to_string()), ("err", &e.to_string())]);
                self.recipe_status(false, text);
                return;
            }
        };
        let (entry, losses) = autoshade::recipe_library::portable(&recipe, false);
        match autoshade::recipe_library::create(&name, &entry) {
            Ok(()) => {
                self.refresh_recipes();
                self.workspace.rlib.recipe_sel = Some(name.clone());
                let base = trf(self.lang, K_ACTIONS_IMPORTED, &[("name", &name)]);
                let text = if losses.is_empty() {
                    base
                } else {
                    format!(
                        "{} — {} {}",
                        base,
                        tr(self.lang, K_ACTIONS_LEFT_OUT_WHAT_BELONGS_TO_THE_SOURCE_PHOTO),
                        self.recipe_losses(losses)
                    )
                };
                self.recipe_status(true, text);
            }
            Err(e) => {
                let text = trf(self.lang, K_ACTIONS_RECIPE_COULD_NOT_BE_SAVED, &[("name", &name), ("err", &e.to_string())]);
                self.recipe_status(false, text);
            }
        }
    }

    /// The jpg the Save group delivers.
    pub(crate) fn normal_export_opts(&self) -> autoshade::render::ExportOpts {
        autoshade::render::ExportOpts { jpeg_quality: self.workspace.normal.quality.jpeg_quality(), ..Default::default() }
    }

    /// 「Save as jpg」: the single export of the card on screen (the ◈ card
    /// when the noise switch is on), as a jpg at the quality step, to the
    /// export destination (asked when the destination is "ask"). A name the
    /// app made is numbered (`-2`, `-3`, …) when a jpg of that name is
    /// already there; a file picked in the save window is replaced, as that
    /// window confirmed.
    pub(crate) fn start_normal_save(&mut self) {
        if self.active_source_path().is_none() || self.workspace.busy {
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
            self.workspace.normal.delivering = true;
            if picked.is_some() {
                self.workspace.exp.last_dir = picked;
            }
        }
    }

    /// The save window's answer (`None` = cancelled).
    fn pick_save_file(&mut self, name: String) -> Option<PathBuf> {
        #[cfg(test)]
        if let Some(pick) = self.workspace.normal.file_pick.take() {
            return pick;
        }
        let mut dlg = rfd::FileDialog::new().add_filter("jpg", &["jpg"]).set_file_name(name);
        if let Some(d) = self.workspace.exp.last_dir.clone().filter(|d| d.is_dir()) {
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
        if self.workspace.busy || self.workspace.gal.gallery.is_empty() || self.editor.photo().src_path.is_none() {
            return;
        }
        let Some(dest) = self.apply_all_dest() else {
            // Cancelled: nothing was changed, and the line says nothing.
            self.workspace.status.clear();
            return;
        };
        self.commit_pending_names();
        let targets = self.workspace.gal.gallery.clone();
        self.workspace.normal.apply_all = Some(dest);
        self.start_paste_to(self.editor.doc().recipe.clone(), self.editor.photo().src_path.clone(), targets, false);
        if !self.workspace.busy {
            self.workspace.normal.apply_all = None;
        }
    }

    /// The destination setting's folder, or the folder dialog's answer when
    /// it asks (`None` = cancelled).
    fn apply_all_dest(&mut self) -> Option<PathBuf> {
        if let Some(d) = self.export_dest_dir() {
            return Some(d);
        }
        #[cfg(test)]
        if let Some(pick) = self.workspace.normal.folder_pick.take() {
            return pick;
        }
        self.pick_batch_folder()
    }

    /// Move one mask a row up/down. Mask ORDER is render semantics (masks
    /// stack sequentially; a later mask's range sees earlier masks' output), so
    /// the list order is editable — and the armed-tool indices remap with it.
    pub(crate) fn move_mask(&mut self, i: usize, up: bool) {
        let j = if up {
            if i == 0 { return }
            i - 1
        } else {
            if i + 1 >= self.editor.doc().recipe.masks.len() { return }
            i + 1
        };
        crate::logic::command::apply(
            &mut self.editor.doc_mut().recipe,
            crate::logic::command::Command::Mask(if up {
                crate::logic::command::MaskCommand::MoveUp { index: i }
            } else {
                crate::logic::command::MaskCommand::MoveDown { index: i }
            }),
        );
        self.remap_mask_indices(|s| Some(if s == i { j } else if s == j { i } else { s }));
        self.workspace.overlay.overlay_stale = true;
        self.editor.session_mut().dirty = true;
    }

    /// Duplicate a mask: its tuned sliders come along, its rasters are DETACHED
    /// copies (the version-load rule), so the twins never share a mutable file.
    pub(crate) fn duplicate_mask(&mut self, lang: Lang, i: usize) {
        if self.editor.doc().recipe.masks.len() >= 64 {
            let t = tr(lang, K_ACTIONS_MASK_LIMIT_REACHED_64_DELETE_ONE_FIRST);
            self.toast(ToastKind::Error, t.to_string());
            return;
        }
        let Some(mut clone) = self.editor.doc().recipe.masks.get(i).cloned() else { return };
        if let Some(p) = self.editor.photo().src_path.as_deref() {
            let mut tmp = EditRecipe { masks: vec![clone], ..Default::default() };
            autoshade::store::detach_rasters(p, &mut tmp, "mask-dup");
            clone = tmp.masks.remove(0);
        }
        if !clone.name.is_empty() {
            clone.name = format!("{} 2", clone.name);
        }
        crate::logic::command::apply(
            &mut self.editor.doc_mut().recipe,
            crate::logic::command::Command::InsertMask { after: i, mask: Box::new(clone) },
        );
        self.remap_mask_indices(|s| Some(if s > i { s + 1 } else { s }));
        self.workspace.masks.sel_mask = Some(i + 1);
        self.workspace.masks.sel_component = None;
        self.workspace.overlay.overlay_stale = true;
        // The remap maps an armed Redraw(i)/brush(i) to ITSELF (s > i is false
        // at s == i) while the selection jumps to the copy — armed on the
        // original, pointed at the duplicate. Selection moved ⇒ same rule.
        if self.disarm_selection_bound_tools(self.workspace.masks.sel_mask) {
            let t = tr(
                self.lang,
                K_ACTIONS_THE_RASTER_EDIT_BRUSH_SESSION_ENDED_YOU_SELECTED,
            );
            self.toast(ToastKind::Error, t.to_string());
        }
        self.editor.session_mut().dirty = true;
    }

}
