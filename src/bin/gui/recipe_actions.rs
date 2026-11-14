//! The recipe library's business: listing, saving, applying, renaming and
//! deleting library entries, and the words those outcomes say.
//!
//! The window (`panels/recipes.rs`) draws and emits verbs; the import dialog
//! lives in `actions.rs` beside this file.

use crate::*;

impl AutoShadeApp {

    /// Re-read the library from disk. Synchronous and cheap: the folder holds
    /// a handful of small JSON files.
    pub(crate) fn refresh_recipes(&mut self) {
        self.recipes = autoshade::recipe_library::list();
        if let Some(sel) = self.recipe_sel.clone()
            && !self.recipes.iter().any(|r| r.name == sel)
        {
            self.recipe_sel = None;
            self.recipe_rename = None;
            self.recipe_delete_armed = None;
        }
    }

    /// The name a blank field saves under: the open photo's stem.
    fn recipe_default_name(&self) -> String {
        match self.src_path.as_deref() {
            Some(p) => autoshade::pipeline::stem(p).to_string(),
            None => tr(self.lang, "Recipe").to_string(),
        }
    }

    /// Localized wording for a refused name.
    pub(crate) fn recipe_name_error(&self, e: autoshade::recipe_library::NameError) -> String {
        use autoshade::recipe_library::NameError as N;
        match e {
            N::Empty => tr(self.lang, "A recipe needs a name").to_string(),
            N::TooLong => trf(
                self.lang,
                "The name is longer than {n} characters",
                &[("n", &autoshade::store::MAX_STORE_NAME.to_string())],
            ),
            N::Illegal(c) => trf(self.lang, "The name cannot contain {char}", &[("char", &c.to_string())]),
        }
    }

    /// What a portable entry left behind, as one list of user-facing fragments.
    pub(crate) fn recipe_losses(&self, losses: autoshade::recipe_library::Losses) -> String {
        let lang = self.lang;
        let mut parts: Vec<String> = Vec::new();
        if losses.masks > 0 {
            parts.push(trf(lang, "{n} bitmap mask(s)", &[("n", &losses.masks.to_string())]));
        }
        if losses.colour_field {
            parts.push(tr(lang, "the colour field").to_string());
        }
        if losses.layers > 0 {
            parts.push(trf(lang, "{n} fill layer(s)", &[("n", &losses.layers.to_string())]));
        }
        if losses.lens_blur {
            parts.push(tr(lang, "the lens blur").to_string());
        }
        if losses.geometry {
            parts.push(tr(lang, "crop / straighten").to_string());
        }
        parts.join(", ")
    }

    /// Report an outcome on both channels (the status line for the record, a
    /// toast for the failure the user must not miss).
    pub(crate) fn recipe_status(&mut self, ok: bool, text: String) {
        self.status = text.clone();
        if !ok {
            self.toast(ToastKind::Error, text);
        }
    }

    /// Save the open photo's develop into the library under a name.
    pub(crate) fn save_current_recipe(&mut self) {
        let Some(src) = self.src_path.clone() else {
            return;
        };
        if self.busy {
            return;
        }
        // Every other persistence-shaped verb commits open name boxes first.
        self.commit_pending_names();
        let typed = self.recipe_name_buf.trim();
        let name = if typed.is_empty() { self.recipe_default_name() } else { typed.to_string() };
        let name = match autoshade::recipe_library::check_name(&name) {
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
        let (entry, losses) = autoshade::recipe_library::portable(&self.recipe, false);
        match autoshade::recipe_library::create(&name, &entry) {
            Ok(()) => {
                let _ = src;
                self.recipe_name_buf.clear();
                self.refresh_recipes();
                self.recipe_sel = Some(name.clone());
                let base = trf(self.lang, "Saved recipe 「{name}」", &[("name", &name)]);
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

    /// Put a library entry on the canvas. Nothing is written until Ctrl+S —
    /// the same stance as loading a version.
    pub(crate) fn apply_recipe_to_open(&mut self, name: &str) {
        if self.busy {
            return;
        }
        let Some(src) = self.src_path.clone() else {
            return;
        };
        self.commit_pending_names();
        let entry = match autoshade::recipe_library::read(name) {
            Ok(r) => r,
            Err(e) => {
                let text = trf(self.lang, "Recipe 「{name}」 could not be read: {err}", &[("name", name), ("err", &e.to_string())]);
                self.recipe_status(false, text);
                return;
            }
        };
        // The target's own calibration and baked-source rules, through the
        // same shaper the clipboard paste uses.
        let mut live = crate::persist::paste_recipe_for(&src, &entry);
        if self.skin_held_for(&src) {
            autoshade::face::keep_skin_natural_in(&mut live);
        }
        if self.active_on_ai_pixels() {
            live.strip_baked_start();
            live.as_shot_k = None;
            live.as_shot_tint = None;
        }
        let dropped = live.clamp();
        if !dropped.is_empty() {
            let t = trf(
                self.lang,
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
        if let Err(e) = autoshade::pipeline::migrate_recipe_coord_frame(&src, &mut live) {
            self.toast(
                ToastKind::Error,
                coord_migration_failure_sentence(self.lang, &e.to_string()),
            );
        }
        self.recipe = live;
        self.dirty = true;
        // A recipe placed on the pristine ✨ card is an edit like any other:
        // it lands on a new ✎ card (the immutability rule).
        self.fork_edited_card();
        self.resync_recipe_display();
        let text = trf(self.lang, "Applied recipe 「{name}」 — Ctrl+Z returns to before", &[("name", name)]);
        self.recipe_status(true, text);
    }

    /// Write a library entry into the selected photos' develops (the paste
    /// path: recipe.json per photo, XMP for RAWs, nothing rendered).
    pub(crate) fn apply_recipe_to_selection(&mut self, name: &str) {
        if self.busy {
            return;
        }
        let targets: Vec<PathBuf> = {
            let mut idx: Vec<usize> = self.multi_sel.iter().copied().collect();
            idx.sort_unstable();
            idx.into_iter().filter_map(|i| self.gallery.get(i).cloned()).collect()
        };
        if targets.is_empty() {
            return;
        }
        let entry = match autoshade::recipe_library::read(name) {
            Ok(r) => r,
            Err(e) => {
                let text = trf(self.lang, "Recipe 「{name}」 could not be read: {err}", &[("name", name), ("err", &e.to_string())]);
                self.recipe_status(false, text);
                return;
            }
        };
        self.commit_pending_names();
        self.start_paste_to(entry, None, targets, false);
    }

    /// Replace a library entry's content with the open photo's current look.
    pub(crate) fn update_recipe_from_open(&mut self, name: &str) {
        if self.busy || self.src_path.is_none() {
            return;
        }
        self.commit_pending_names();
        let (entry, losses) = autoshade::recipe_library::portable(&self.recipe, false);
        match autoshade::recipe_library::replace(name, &entry) {
            Ok(()) => {
                self.refresh_recipes();
                let base = trf(self.lang, "Updated recipe 「{name}」", &[("name", name)]);
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
                let text = trf(self.lang, "Recipe 「{name}」 could not be saved: {err}", &[("name", name), ("err", &e.to_string())]);
                self.recipe_status(false, text);
            }
        }
    }

    /// Rename an entry (`from` -> `to`; the file moves with it).
    pub(crate) fn rename_recipe(&mut self, from: &str, to: &str) {
        let to = match autoshade::recipe_library::check_name(to) {
            Ok(name) => name,
            Err(e) => {
                let text = self.recipe_name_error(e);
                self.recipe_status(false, text);
                return;
            }
        };
        if to == from {
            return;
        }
        if autoshade::recipe_library::exists(&to) {
            let text = trf(self.lang, "A recipe named 「{name}」 already exists", &[("name", &to)]);
            self.recipe_status(false, text);
            return;
        }
        match autoshade::recipe_library::rename(from, &to) {
            Ok(()) => {
                self.refresh_recipes();
                self.recipe_sel = Some(to.clone());
                let text = trf(self.lang, "Renamed to 「{name}」", &[("name", &to)]);
                self.recipe_status(true, text);
            }
            Err(e) => {
                let text = trf(self.lang, "Recipe 「{name}」 could not be renamed: {err}", &[("name", from), ("err", &e.to_string())]);
                self.recipe_status(false, text);
            }
        }
    }

    /// Delete an entry's file. The photos it was applied to keep their own
    /// saved develops — this removes the library copy only.
    pub(crate) fn delete_recipe(&mut self, name: &str) {
        match autoshade::recipe_library::delete(name) {
            Ok(()) => {
                self.refresh_recipes();
                self.recipe_sel = None;
                self.recipe_delete_armed = None;
                let text = trf(self.lang, "Deleted recipe 「{name}」", &[("name", name)]);
                self.recipe_status(true, text);
            }
            Err(e) => {
                let text = trf(self.lang, "Recipe 「{name}」 could not be deleted: {err}", &[("name", name), ("err", &e.to_string())]);
                self.recipe_status(false, text);
            }
        }
    }
}
