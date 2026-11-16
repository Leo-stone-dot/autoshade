//! The recipe library window: named, portable develops kept outside any one
//! photo (`autoshade::recipe_library`), applied through the same paths the
//! clipboard paste uses.

use crate::*;

impl AutoShadeApp {
    /// Open the window (the Library panel's 「Recipe library…」 button).
    pub(crate) fn open_recipe_library(&mut self) {
        self.rlib.show_recipes = true;
        self.apply_recipe(logic::recipes::RecipeAction::Refresh);
    }

    /// One `recipe_library` window frame.
    pub(crate) fn recipe_library_window(&mut self, ctx: &egui::Context) {
        if !self.rlib.show_recipes {
            return;
        }
        if !self.rlib.recipes_loaded {
            self.refresh_recipes();
        }
        let lang = self.lang;
        let mut open = true;
        egui::Window::new(tr(lang, "Recipe library"))
            // Fixed id: the title varies with the language and egui keys
            // window state (position, size) off the id.
            .id(egui::Id::new("recipe_library_window"))
            .collapsible(false)
            .default_width(520.0)
            .open(&mut open)
            .show(ctx, |ui| self.recipe_library_ui(ui));
        if !open {
            self.rlib.show_recipes = false;
        }
    }

    fn recipe_library_ui(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let colors = self.ui.theme.colors();
        let n = self.rlib.recipes.len();
        ui.set_min_width(540.0);

        // --- new recipe ------------------------------------------------------
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(tr(lang, "New recipe")).weak().small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, true, tr(lang, "Import file…"))
                    .on_hover_text(tr(lang, "Add a recipe.json or a Lightroom / Camera Raw .xmp sidecar to the library"))
                    .clicked()
                {
                    self.apply_recipe(logic::recipes::RecipeAction::ImportFile);
                }
            });
        });
        ui.horizontal(|ui| {
            let ready = self.photo.src_path.is_some() && !self.busy;
            let button_w = 92.0;
            let field_w = (ui.available_width() - button_w - ui.spacing().item_spacing.x).max(180.0);
            let resp = ui.add_sized(
                [field_w, row_h(ui)],
                egui::TextEdit::singleline(&mut self.rlib.recipe_name_buf).hint_text(tr(lang, "Recipe name")),
            );
            // Enter in the name field saves, like every other name box in the
            // app (the U10 boundary rule): a typed name plus Enter is the
            // shortest path from "this look" to "a named look".
            let entered = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            let clicked = primary_in(ui, egui::vec2(button_w, row_h(ui)), ready, tr(lang, "＋ Save"))
                .on_hover_text(tr(
                    lang,
                    "Saves this photo's look under the name in the field (blank = the photo's name). Crop, and the parts whose pixels belong to this photo, are left out.",
                ))
                .on_disabled_hover_text(tr(lang, "Open a photo first — this saves the open photo's look"))
                .clicked();
            if ready && (clicked || entered) {
                self.apply_recipe(logic::recipes::RecipeAction::SaveCurrent);
            }
        });
        ui.separator();

        // --- filter ----------------------------------------------------------
        let filter_empty = self.rlib.recipes_filter.trim().is_empty();
        ui.horizontal(|ui| {
            let clear_w = if filter_empty { 0.0 } else { row_h(ui) + ui.spacing().item_spacing.x };
            let count_w = 80.0;
            let w = (ui.available_width() - clear_w - count_w - ui.spacing().item_spacing.x).max(140.0);
            ui.add_sized(
                [w, row_h(ui)],
                egui::TextEdit::singleline(&mut self.rlib.recipes_filter).hint_text(tr(lang, "Filter")),
            );
            if !filter_empty
                && glyph(ui, true, "✕").on_hover_text(tr(lang, "Clear filter")).clicked()
            {
                self.rlib.recipes_filter.clear();
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    egui::RichText::new(trf(lang, "{n} saved", &[("n", &n.to_string())])).weak().small(),
                );
            });
        });

        // --- list ------------------------------------------------------------
        let filter = self.rlib.recipes_filter.trim().to_lowercase();
        let rows: Vec<(String, Option<String>, usize, Option<String>)> = self
            .rlib.recipes
            .iter()
            .filter(|r| filter.is_empty() || r.name.to_lowercase().contains(&filter))
            .map(|r| {
                (
                    r.name.clone(),
                    r.modified.and_then(|t| age_text(lang, t)),
                    r.masks,
                    r.error.clone(),
                )
            })
            .collect();
        let mut picked: Option<String> = None;
        let mut commit_rename: Option<(String, String)> = None;
        ui.add_space(SPACE_XS);
        egui::ScrollArea::vertical().id_salt("recipe_list").max_height(260.0).show(ui, |ui| {
            if rows.is_empty() {
                ui.add_space(SPACE_LG);
                let (headline, hint) = if self.rlib.recipes.is_empty() {
                    (
                        tr(lang, "No recipes yet"),
                        tr(lang, "Save this photo's look, or import a recipe.json / .xmp file."),
                    )
                } else {
                    (
                        tr(lang, "No recipe matches this filter."),
                        tr(lang, "Clear the filter to see the whole library."),
                    )
                };
                ui.vertical_centered(|ui| {
                    ui.label(egui::RichText::new("🖼").size(22.0).weak());
                    ui.add_space(SPACE_XS);
                    ui.label(egui::RichText::new(headline).weak());
                    ui.label(egui::RichText::new(hint).weak().small());
                });
                ui.add_space(SPACE_LG);
            }
            for (name, age, masks, error) in &rows {
                let selected = self.rlib.recipe_sel.as_deref() == Some(name.as_str());
                let editing = self.rlib.recipe_rename.as_ref().is_some_and(|(n, _)| n == name);
                if editing {
                    let mut buf = self.rlib.recipe_rename.as_ref().map(|(_, b)| b.clone()).unwrap_or_default();
                    let resp = ui.add_sized(
                        [ui.available_width(), RECIPE_ROW_H],
                        egui::TextEdit::singleline(&mut buf)
                            .id(egui::Id::new(("recipe_rename", name))),
                    );
                    if let Some((_, b)) = self.rlib.recipe_rename.as_mut() {
                        *b = buf;
                    }
                    if resp.lost_focus() {
                        commit_rename = self.rlib.recipe_rename.take();
                    } else if !resp.has_focus() {
                        // The frame the box first appears in: put the caret
                        // here or the user's next keystroke goes nowhere
                        // (the version-rename rule).
                        resp.request_focus();
                    }
                    continue;
                }
                // The meta sits at the right edge; a file that does not parse
                // says so there in the error colour instead of hiding.
                let meta = match error {
                    Some(_) => tr(lang, "unreadable").to_string(),
                    None => {
                        let mut meta = age.clone().unwrap_or_default();
                        if *masks == 1 {
                            meta = format!("{} · {}", meta, tr(lang, "1 mask"));
                        } else if *masks > 1 {
                            meta = format!(
                                "{} · {}",
                                meta,
                                trf(lang, "{n} masks", &[("n", &masks.to_string())])
                            );
                        }
                        meta
                    }
                };
                let (resp, truncated) = paint_recipe_row(ui, colors, name, &meta, selected, error.is_some());
                // The full name only when the row had to cut it; the error
                // only when the file did not parse. A tooltip on every row
                // was a sentence following the pointer across the list.
                let resp = match (error, truncated) {
                    (Some(err), _) => resp.on_hover_text(trf(lang, "unreadable: {err}", &[("err", err)])),
                    (None, true) => resp.on_hover_text(name.as_str()),
                    (None, false) => resp,
                };
                if resp.clicked() {
                    picked = Some(name.clone());
                }
            }
        });
        if let Some((from, to)) = commit_rename {
            self.apply_recipe(logic::recipes::RecipeAction::Rename {
                from: from.clone(),
                to: to.clone(),
            });
        }
        if let Some(name) = picked {
            self.rlib.recipe_sel = Some(name);
            self.rlib.recipe_delete_armed = None;
        }
        ui.separator();
        // The selected entry: what it looks like on this photo, what it
        // changes, and the verbs that act on IT (the library-wide verbs —
        // paste onto the selection, open the folder — stay below).
        if let Some(name) = self.rlib.recipe_sel.clone() {
            self.recipe_details_pane(ui, &name);
            ui.separator();
        }

        // --- the library-wide verbs ------------------------------------------
        let sel = self.rlib.recipe_sel.clone();
        let targets = self.gal.multi_sel.len();
        let busy = self.busy;
        ui.horizontal_wrapped(|ui| {
            if action(
                ui,
                sel.is_some() && targets > 0 && !busy,
                trf(lang, "Apply to selected ({n})", &[("n", &targets.to_string())]),
            )
            .on_hover_text(tr(
                lang,
                "Writes each photo's develop into your develop store (recipe JSON; RAW also gets a Lightroom XMP). Leaves library files untouched, renders nothing.",
            ))
            .on_disabled_hover_text(if targets == 0 {
                tr(lang, "Ctrl/⌘+click photos in the Library first")
            } else {
                tr(lang, "A task is running — wait for it to finish")
            })
            .clicked()
                && let Some(name) = &sel
            {
                self.apply_recipe(logic::recipes::RecipeAction::ApplyToSelection(name.to_owned()));
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if action(ui, true, tr(lang, "🗂 Show library folder"))
                    .on_hover_text(tr(lang, "Opens the folder that holds these recipe files"))
                    .clicked()
                    && let Err(e) = reveal_folder(&autoshade::recipe_library::library_dir())
                {
                    self.recipe_status(
                        false,
                        trf(lang, "The library folder could not be opened: {err}", &[("err", &e.to_string())]),
                    );
                }
            });
        });
    }

    /// The selected entry: its look developed on the open photo, the settings
    /// it actually changes, and the verbs that act on it.
    fn recipe_details_pane(&mut self, ui: &mut egui::Ui, name: &str) {
        let lang = self.lang;
        let entry = autoshade::recipe_library::read(name);
        let colors = self.ui.theme.colors();
        let photo_stem = self.photo.src_path.as_deref().map(|p| autoshade::pipeline::stem(p).to_string());
        ui.horizontal_top(|ui| {
            const PREVIEW_W: f32 = 200.0;
            const PREVIEW_H: f32 = 140.0;
            let tex = if entry.is_ok() { self.recipe_preview_tex(ui.ctx(), name) } else { None };
            ui.vertical(|ui| {
                let (rect, _) =
                    ui.allocate_exact_size(egui::vec2(PREVIEW_W, PREVIEW_H), egui::Sense::hover());
                match tex {
                    Some((id, size)) => {
                        let scale = (PREVIEW_W / size.x).min(PREVIEW_H / size.y).min(1.0);
                        let shown = size * scale;
                        let at = egui::Rect::from_center_size(rect.center(), shown);
                        egui::Image::new(egui::load::SizedTexture::new(id, shown)).paint_at(ui, at);
                        ui.painter().rect_stroke(
                            at.expand(1.0),
                            RADIUS_SM,
                            egui::Stroke::new(1.0, colors.hairline),
                            egui::StrokeKind::Middle,
                        );
                        if let Some(stem) = &photo_stem {
                            ui.label(
                                egui::RichText::new(trf(lang, "On {photo}", &[("photo", stem)]))
                                    .weak()
                                    .small(),
                            );
                        }
                    }
                    None => {
                        ui.painter().rect_filled(rect, RADIUS_MD, colors.canvas_bed);
                        ui.painter().text(
                            rect.center(),
                            egui::Align2::CENTER_CENTER,
                            tr(lang, "Open a photo to see this recipe on it."),
                            egui::TextStyle::Small.resolve(ui.style()),
                            ui.visuals().weak_text_color(),
                        );
                    }
                }
            });
            ui.vertical(|ui| {
                ui.label(egui::RichText::new(name).strong());
                match &entry {
                    Ok(r) => {
                        let facts = self.recipe_facts(r);
                        if facts.is_empty() {
                            ui.label(
                                egui::RichText::new(tr(lang, "No edits — this recipe is neutral.")).weak().small(),
                            );
                        } else {
                            egui::Grid::new("recipe_facts")
                                .num_columns(2)
                                .spacing([16.0, 3.0])
                                .show(ui, |ui| {
                                    for (label, value) in facts {
                                        ui.label(egui::RichText::new(label).weak().small());
                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(egui::RichText::new(value).small());
                                            },
                                        );
                                        ui.end_row();
                                    }
                                });
                        }
                    }
                    Err(e) => {
                        ui.label(
                            egui::RichText::new(trf(lang, "unreadable: {err}", &[("err", &e.to_string())]))
                                .color(colors.toast_err_fg)
                                .small(),
                        );
                    }
                }
            });
        });
        let ok = entry.is_ok();
        let has_photo = self.photo.src_path.is_some();
        let busy = self.busy;
        ui.add_space(SPACE_XS);
        let running = tr(lang, "A task is running — wait for it to finish");
        let needs_photo = || {
            if has_photo {
                running
            } else {
                tr(lang, "Open a photo first")
            }
        };
        ui.horizontal_wrapped(|ui| {
            if primary(ui, ok && has_photo && !busy, tr(lang, "✓ Apply to this photo"))
                .on_hover_text(tr(lang, "Replaces what is on the canvas; Ctrl+Z undoes it. Nothing is saved until Ctrl+S."))
                .on_disabled_hover_text(needs_photo())
                .clicked()
            {
                self.apply_recipe(logic::recipes::RecipeAction::ApplyToOpen(name.to_owned()));
            }
            if action(ui, ok && has_photo && !busy, tr(lang, "Write back to this recipe"))
                .on_hover_text(tr(lang, "Saves what is on the canvas into this entry, keeping its name — the way an edit on the photo becomes part of the library"))
                .on_disabled_hover_text(needs_photo())
                .clicked()
            {
                self.apply_recipe(logic::recipes::RecipeAction::UpdateFromOpen(name.to_owned()));
            }
            if action(ui, ok && !busy, tr(lang, "✎ Rename"))
                .on_hover_text(tr(lang, "Rename this recipe (the file moves with it)"))
                .clicked()
            {
                self.rlib.recipe_rename = Some((name.to_string(), name.to_string()));
            }
            let armed = self.rlib.recipe_delete_armed.as_deref() == Some(name);
            let del = if armed { tr(lang, "⚠ Confirm delete") } else { tr(lang, "🗑 Delete") };
            if action(ui, ok && !busy, del)
                .on_hover_text(tr(
                    lang,
                    "Deletes this recipe file from the library. Photos it was applied to keep their own saved develops.",
                ))
                .clicked()
            {
                if armed {
                    self.apply_recipe(logic::recipes::RecipeAction::Delete(name.to_owned()));
                } else {
                    self.rlib.recipe_delete_armed = Some(name.to_string());
                }
            }
        });
    }

    /// The selected entry developed on the open photo, as a texture + its
    /// pixel size. Rebuilt only when its key changes — the entry's file stamp,
    /// the photo, or the base pixels' allocation — so clicking through the
    /// list pays one downscale plus one small develop per entry, never a
    /// full-frame render.
    fn recipe_preview_tex(
        &mut self,
        ctx: &egui::Context,
        name: &str,
    ) -> Option<(egui::TextureId, egui::Vec2)> {
        let src = self.photo.src_path.clone()?;
        let base = self.photo.base_preview.clone()?;
        let entry = autoshade::recipe_library::read(name).ok()?;
        let stamp = std::fs::metadata(autoshade::recipe_library::entry_path(name))
            .and_then(|m| m.modified())
            .ok();
        let key = format!(
            "{name}|{}|{stamp:?}|{:p}",
            src.display(),
            std::sync::Arc::as_ptr(&base)
        );
        if let Some(p) = &self.rlib.recipe_preview
            && p.key == key
        {
            return Some((p.texture.id(), p.texture.size_vec2()));
        }
        let plate = autoshade::render::Plate::thumbnail_of(&base, 420);
        // What APPLYING would show: the target's own calibration, the
        // library entry's edit — the same shaping the paste path runs.
        let mut r = crate::persist::paste_recipe_for(&src, &entry);
        r.clamp();
        let img = autoshade::render::develop_preview(plate.shared().as_ref(), &r);
        let rgba = img.to_rgba8();
        let size = [rgba.width() as usize, rgba.height() as usize];
        let color = egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
        let texture = ctx.load_texture(
            format!("recipe-preview:{key}"),
            color,
            egui::TextureOptions::LINEAR,
        );
        let shown = (texture.id(), texture.size_vec2());
        self.rlib.recipe_preview = Some(RecipePreview { key, texture });
        Some(shown)
    }

    /// What the entry CHANGES, as label/value rows: only the controls that
    /// differ from a neutral develop, because "what is in this recipe" is the
    /// question the list could not answer. The numbers are the engine's own.
    fn recipe_facts(&self, r: &EditRecipe) -> Vec<(String, String)> {
        let lang = self.lang;
        let d = EditRecipe::default();
        let mut out: Vec<(String, String)> = Vec::new();
        if (r.exposure_ev - d.exposure_ev).abs() > 1e-4 {
            out.push((tr(lang, "Exposure").to_string(), format!("{:+.2}", r.exposure_ev)));
        }
        for (label, v, neutral) in [
            ("Contrast", r.contrast, d.contrast),
            ("Highlights", r.highlights, d.highlights),
            ("Shadows", r.shadows, d.shadows),
            ("Whites", r.whites, d.whites),
            ("Blacks", r.blacks, d.blacks),
            ("Tint", r.tint, d.tint),
            ("Vibrance", r.vibrance, d.vibrance),
            ("Saturation", r.saturation, d.saturation),
            ("Texture", r.texture, d.texture),
            ("Clarity", r.clarity, d.clarity),
            ("Dehaze", r.dehaze, d.dehaze),
            ("Sharpening", r.sharpening, d.sharpening),
            ("Noise Reduction", r.noise_reduction, d.noise_reduction),
        ] {
            if (v - neutral).abs() > 0.05 {
                out.push((tr(lang, label).to_string(), format!("{v:+.0}")));
            }
        }
        if let Some(k) = r.temperature_k.filter(|k| *k > 0.0) {
            out.push((tr(lang, "Temperature").to_string(), format!("{k:.0} K")));
        }
        let points = r.tone_curve.len() + r.red_curve.len() + r.green_curve.len() + r.blue_curve.len();
        if points > 0 {
            out.push((
                tr(lang, "Curve").to_string(),
                trf(lang, "{n} points", &[("n", &points.to_string())]),
            ));
        }
        if r.convert_to_grayscale {
            out.push((tr(lang, "Black & white").to_string(), "✓".to_string()));
        }
        if r.hsl != d.hsl {
            out.push((tr(lang, "Color Mixer (HSL)").to_string(), "✓".to_string()));
        }
        if r.color_grade != d.color_grade {
            out.push((tr(lang, "Color Grading").to_string(), "✓".to_string()));
        }
        if !r.point_colors.is_empty() {
            out.push((tr(lang, "Point Color").to_string(), r.point_colors.len().to_string()));
        }
        if !r.masks.is_empty() {
            out.push((tr(lang, "Masks").to_string(), r.masks.len().to_string()));
        }
        out
    }
}

/// One library row: 34 px, the name at the left, its meta at the right edge,
/// a hover wash under the pointer and an accent bar while selected. Painted
/// rather than composed so every row is the same line whatever the name's
/// length (a long stem truncates; the tooltip carries it whole).
fn paint_recipe_row(
    ui: &mut egui::Ui,
    colors: &crate::theme::ThemeColors,
    name: &str,
    meta: &str,
    selected: bool,
    broken: bool,
) -> (egui::Response, bool) {
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(ui.available_width(), RECIPE_ROW_H), egui::Sense::click());
    if !ui.is_rect_visible(rect) {
        return (resp, false);
    }
    if resp.hovered() {
        // A row is a click target, like a gallery thumbnail (the same cursor
        // language everywhere a click does something).
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), RADIUS_SM, colors.sel_bg);
        let bar = egui::Rect::from_min_size(
            rect.left_center() - egui::vec2(0.0, 10.0),
            egui::vec2(2.0, 20.0),
        );
        painter.rect_filled(bar, 1.0, colors.accent_fill);
    } else if resp.hovered() {
        // One quiet step off the panel: the selection is the strong state,
        // the pointer's wash must not out-shout it.
        painter.rect_filled(rect.shrink2(egui::vec2(0.0, 1.0)), RADIUS_SM, colors.card_fill);
    }
    let font = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let weak = if broken { colors.toast_err_fg } else { ui.visuals().weak_text_color() };
    let meta_galley = painter.layout_no_wrap(meta.to_owned(), small, weak);
    let meta_w = meta_galley.size().x;
    painter.galley(
        rect.right_center() - egui::vec2(12.0 + meta_w, meta_galley.size().y / 2.0),
        meta_galley,
        weak,
    );
    let name_w = (rect.width() - 24.0 - meta_w - 16.0).max(40.0);
    let name_color = if selected {
        colors.accent_text
    } else if broken {
        weak
    } else {
        ui.visuals().text_color()
    };
    let shown = truncate_to(ui, name, font.clone(), name_w);
    let truncated = shown != name;
    painter.text(
        rect.left_center() + egui::vec2(14.0, 0.0),
        egui::Align2::LEFT_CENTER,
        shown,
        font,
        name_color,
    );
    (resp, truncated)
}

/// `text` cut to `max_w` with an ellipsis (egui 0.29 has no overflow
/// character on a laid-out galley, so this walks the tail off and re-measures;
/// the galleys are cached, and a row name is a few dozen characters).
fn truncate_to(ui: &egui::Ui, text: &str, font: egui::FontId, max_w: f32) -> String {
    let width = |s: &str| {
        ui.fonts_mut(|f| {
            f.layout_no_wrap(s.to_owned(), font.clone(), egui::Color32::PLACEHOLDER).size().x
        })
    };
    if width(text) <= max_w {
        return text.to_owned();
    }
    let mut cut = text.to_owned();
    while !cut.is_empty() {
        cut.pop();
        let candidate = format!("{}…", cut.trim_end());
        if width(&candidate) <= max_w {
            return candidate;
        }
    }
    "…".to_owned()
}

/// The one row height of the library list.
const RECIPE_ROW_H: f32 = 34.0;


/// How old a library file is, in words: the file stamp measured against now
/// (no date crate in the tree, and an elapsed span needs no time zone).
fn age_text(lang: Lang, t: std::time::SystemTime) -> Option<String> {
    let days = t.elapsed().ok()?.as_secs() / 86_400;
    Some(if days == 0 {
        tr(lang, "today").to_string()
    } else if days == 1 {
        tr(lang, "yesterday").to_string()
    } else if days < 30 {
        trf(lang, "{n} days ago", &[("n", &days.to_string())])
    } else if days < 365 {
        trf(lang, "{n} months ago", &[("n", &(days / 30).to_string())])
    } else {
        trf(lang, "{n} years ago", &[("n", &(days / 365).to_string())])
    })
}

#[cfg(test)]
mod age_text_tests {
    use super::age_text;
    use crate::Lang;
    use std::time::{Duration, SystemTime};

    #[test]
    fn a_file_stamp_reads_as_an_elapsed_span() {
        let now = SystemTime::now();
        assert_eq!(age_text(Lang::En, now - Duration::from_secs(60)).as_deref(), Some("today"));
        assert_eq!(
            age_text(Lang::En, now - Duration::from_secs(3 * 86_400)).as_deref(),
            Some("3 days ago")
        );
        assert_eq!(
            age_text(Lang::En, now - Duration::from_secs(60 * 86_400)).as_deref(),
            Some("2 months ago")
        );
        assert_eq!(age_text(Lang::En, now + Duration::from_secs(60)), None, "a future stamp says nothing");
    }
}
