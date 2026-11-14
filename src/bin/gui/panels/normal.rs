//! Normal's sidebar (design-normal-mode.md §3, approved 2026-10-23): four
//! groups in the order a person asks for them — make it look good (a one-click
//! look), a little brighter or warmer (fine-tune), less noise, save it — and
//! nothing else. Drawn in the one workspace's controls column, between the
//! Portraits panel above and the shared panels below (`app.rs`
//! `controls_column`).
//!
//! Every verb here reuses a door the full workspace already has: a one-click
//! look is `produce_recipe` through `spawn_analyze` (a preset is a typed
//! direction, the reference folder is the library — Normal's own,
//! `library::NORMAL_NAME`), the four sliders write the recipe fields the
//! develop panel writes, the noise switch is the Retouch panel's AI denoise
//! (its ◈ card, shown on the canvas and saved as shown; off goes back to the
//! card it was made from), and the Save group is the single export and the
//! paste-then-render batch with a jpg's options, numbered rather than
//! replacing a jpg saved earlier. The words are plain (separate i18n keys,
//! design §6): none of the workspace's trade terms appear here.

use crate::*;
use crate::logic::look::{FineTune, OneClick};
#[cfg(test)]
use autoshade::pipeline::GradeRequest;

/// What the first AI denoise of a rendered image downloads: the SCUNet weight
/// plus its network file (`python/denoise.py` `WEIGHT_BYTES["color_real_psnr"]` +
/// `NETWORK_BYTES`, 71 994 286 bytes), in MB — pinned by a test against
/// that file.
pub(crate) const DENOISE_FIRST_DOWNLOAD_MB: u32 = 72;

/// The one line under the one-click group: what the last one-click did.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum NormalLine {
    #[default]
    Idle,
    Working,
    Done,
    /// No image key: the built-in look stood in (`advisor::HeuristicProposer`).
    BuiltIn,
    /// The look could not be made; the error's own text.
    Failed(String),
}

/// The JPEG quality steps of the Save group.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum SaveQuality {
    #[default]
    High,
    Medium,
    Small,
}

impl SaveQuality {
    pub(crate) const ALL: [SaveQuality; 3] = [SaveQuality::High, SaveQuality::Medium, SaveQuality::Small];

    pub(crate) fn label(self, lang: Lang) -> &'static str {
        match self {
            SaveQuality::High => tr(lang, "High"),
            SaveQuality::Medium => tr(lang, "Medium"),
            SaveQuality::Small => tr(lang, "Small file"),
        }
    }

    pub(crate) fn jpeg_quality(self) -> u8 {
        match self {
            SaveQuality::High => 95,
            SaveQuality::Medium => 85,
            SaveQuality::Small => 70,
        }
    }
}

/// The noise switch's two cards for one photo: the card it was turned on
/// from, and the ◈ card the AI denoise made from it (`None` while it runs).
/// Ids, not indices: the strip can change under them in the full workspace.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct DenoisePair {
    pub(crate) photo: PathBuf,
    pub(crate) plain: String,
    pub(crate) denoised: Option<String>,
}

/// Normal's own state. Nothing here is persisted: the remembered preferences
/// (the look strength among them) are the workspace's own.
#[derive(Default)]
pub(crate) struct NormalState {
    pub(crate) line: NormalLine,
    /// The reference folder picked this session.
    pub(crate) ref_dir: Option<PathBuf>,
    /// The folder is being read into Normal's library.
    pub(crate) building: bool,
    /// The photo the folder was picked for: the look the build hands over
    /// to is made for this photo only (`on_one_click_built`).
    pub(crate) build_photo: Option<PathBuf>,
    pub(crate) build_progress: Option<(usize, usize)>,
    /// The photo a one-click last landed on, and its four fine-tune values.
    pub(crate) result: Option<(PathBuf, FineTune)>,
    /// The noise switch's cards (`set_normal_denoise`); the switch itself
    /// reads the card on screen (`normal_denoise_on`).
    pub(crate) denoise_pair: Option<DenoisePair>,
    /// Whether the rendered-image denoiser's weight is on disk, read on the
    /// first frame the switch is drawn (`None` = not read yet).
    pub(crate) denoise_ready: Option<bool>,
    pub(crate) quality: SaveQuality,
    /// Is an image key set? Read on the first Normal frame and after a
    /// Settings save; `None` = not read yet.
    pub(crate) key_present: Option<bool>,
    /// 「Apply to every photo」 is between its paste and its render: the
    /// folder the jpgs go to, resolved before the paste.
    pub(crate) apply_all: Option<PathBuf>,
    /// A Save-group delivery is running (its landing is worded here).
    pub(crate) delivering: bool,
    /// Test seam: what the last analysis asked for (request, direction,
    /// library name).
    #[cfg(test)]
    pub(crate) last_request: Option<(GradeRequest, Option<String>, &'static str)>,
    /// Test seam: stands in for the paid chain inside the analyze worker.
    #[cfg(test)]
    pub(crate) runner: Option<AnalyzeRunner>,
    /// Test seam: the batch-folder dialog's answer (`Some(None)` = cancelled).
    #[cfg(test)]
    pub(crate) folder_pick: Option<Option<PathBuf>>,
    /// Test seam: the save window's answer (`Some(None)` = cancelled).
    #[cfg(test)]
    pub(crate) file_pick: Option<Option<PathBuf>>,
}

/// A stand-in for `produce_recipe` (tests only).
#[cfg(test)]
pub(crate) type AnalyzeRunner = fn(
    &std::path::Path,
    &GradeRequest,
) -> anyhow::Result<(EditRecipe, autoshade::advisor::Verdict, Vec<autoshade::rationale::Note>)>;

impl AutoShadeApp {
    /// Put a plain sentence in place of the status a shared landing just
    /// wrote — and in its toast, when it raised one.
    pub(crate) fn reword_landing(&mut self, plain: String) {
        let old = std::mem::replace(&mut self.status, plain.clone());
        if let Some(t) = self.toasts.iter_mut().rev().find(|t| t.text == old) {
            t.text = plain;
        }
    }

    /// The one-click group's line, in the session language.
    pub(crate) fn normal_line_text(&self, lang: Lang) -> String {
        if self.normal.building {
            return match self.normal.build_progress {
                Some((done, total)) => trf(
                    lang,
                    "Reading your reference photos… {done} / {total}",
                    &[("done", &done.to_string()), ("total", &total.to_string())],
                ),
                None => tr(lang, "Reading your reference photos…").into(),
            };
        }
        match &self.normal.line {
            // A cancelled one-click leaves no worker behind to say so.
            NormalLine::Working if self.analyze_inflight => tr(lang, "Making the look…").into(),
            NormalLine::Idle | NormalLine::Working => tr(lang, "Pick a look, or match your own reference photos.").into(),
            NormalLine::Done => tr(lang, "Done — Ctrl+Z goes back").into(),
            NormalLine::BuiltIn => tr(lang, "No AI key set — built-in look used").into(),
            NormalLine::Failed(why) => why.clone(),
        }
    }

    /// The sidebar: the four groups, behind the same mid-open gate as the
    /// shared panels (L15-2 — these controls write the open photo's recipe).
    pub(crate) fn normal_panel(&mut self, ui: &mut egui::Ui) {
        if self.normal.key_present.is_none() {
            self.normal.key_present = Some(autoshade::config::Config::load().openai_api_key.is_some());
        }
        let editable = !self.open_in_flight;
        ui.add_enabled_ui(editable, |ui| {
            self.normal_one_click(ui);
            self.normal_fine_tune(ui);
            self.normal_noise(ui);
            self.normal_save(ui);
        });
    }

    /// Group 1 — the one-click look: six fixed looks, or the reference folder.
    fn normal_one_click(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, "One-click look"), false, |_| {});
        let ready = self.src_path.is_some() && !self.busy && !self.analyze_inflight && !self.normal.building;
        let names: [&str; 6] = [
            tr(lang, "Natural"),
            tr(lang, "Bright & clean"),
            tr(lang, "Warm"),
            tr(lang, "Cool night"),
            tr(lang, "Film"),
            tr(lang, "Black & white"),
        ];
        let presets = &autoshade::advisor::presets::PRESETS;
        let mut asked: Option<OneClick> = None;
        // Two to a row: at three, "Bright & clean" and "Black & white" are cut short.
        for row in 0..3 {
            ui.horizontal(|ui| {
                let cell = columns(ui, 2);
                for col in 0..2 {
                    let i = row * 2 + col;
                    if action_in(ui, cell, ready, names[i])
                        .on_hover_text(tr(lang, "The AI looks at this photo and gives it this look — Ctrl+Z goes back"))
                        .clicked()
                    {
                        asked = Some(OneClick::Preset(presets[i].id));
                    }
                }
            });
        }
        ui.vertical_centered(|ui| ui.label(egui::RichText::new(tr(lang, "or")).weak().small()));
        let pick = action_row(ui, ready, tr(lang, "Match my reference photos…"))
            .on_hover_text(tr(
                lang,
                "Pick a folder of photos whose look you like. They are read on this computer (the first time, a 1.5 GB model is downloaded for it); then the AI looks at this photo and up to 3 of them, each billed as one more picture.",
            ))
            .clicked();
        if let Some(dir) = &self.normal.ref_dir {
            let name = dir.file_name().map_or_else(|| abs_display(dir), |n| n.to_string_lossy().into_owned());
            ui.label(egui::RichText::new(trf(lang, "Chosen: {name}", &[("name", &name)])).weak().small());
        }
        Self::slider_pct_hinted(
            ui,
            lang,
            tr(lang, "Look strength"),
            &mut self.grade_strength,
            1.0,
            GRADE_STRENGTH_DEFAULT,
            tr(lang, "How strong the look is. Double-click to reset."),
        );
        let line = self.normal_line_text(lang);
        ui.horizontal_wrapped(|ui| {
            if self.normal.building || (self.normal.line == NormalLine::Working && self.analyze_inflight) {
                ui.spinner();
            }
            ui.label(egui::RichText::new(line).small());
        });
        if self.normal.key_present == Some(false) {
            ui.label(
                egui::RichText::new(tr(lang, "To make looks with AI, add a service key in Settings (⚙)."))
                    .weak()
                    .small(),
            );
        }
        if let Some(ask) = asked {
            self.apply_actions(vec![logic::Action::Look(logic::look::LookIntent::of(
                logic::look::LookKind::OneClick(ask),
            ))]);
        }
        // The dialog after the group is laid out: it is modal and blocks, so
        // the runtime opens it and starts the build/look.
        if pick {
            self.apply_actions(vec![logic::Action::Normal(
                logic::normal::NormalAction::PickReferencesFolder,
            )]);
        }
    }

    /// Group 2 — fine-tune: four of the develop panel's fields under plain
    /// names, on top of the one-click result.
    fn normal_fine_tune(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, "Fine-tune"), false, |_| {});
        let mut changed = false;
        let r = &mut self.recipe;
        changed |= Self::slider(ui, lang, tr(lang, "Brightness"), &mut r.exposure_ev, -5.0, 5.0, 0.0);
        changed |= Self::slider(ui, lang, tr(lang, "Contrast"), &mut r.contrast, -100.0, 100.0, 0.0);
        let mut warmth = r.warmth();
        if Self::slider_hinted(
            ui,
            lang,
            tr(lang, "Warmth"),
            &mut warmth,
            -100.0,
            100.0,
            0.0,
            tr(lang, "Cooler to the left, warmer to the right; 0 is the camera's own colour."),
        ) {
            let kelvin = r.kelvin_at_warmth(warmth);
            r.temperature_k = Some(kelvin);
            changed = true;
        }
        changed |= Self::slider(ui, lang, tr(lang, "Vividness"), &mut r.saturation, -100.0, 100.0, 0.0);
        let back = self
            .normal
            .result
            .as_ref()
            .filter(|(p, f)| self.src_path.as_ref() == Some(p) && *f != FineTune::of(&self.recipe))
            .map(|(_, f)| *f);
        if action_row(ui, back.is_some(), tr(lang, "Back to the one-click result")).clicked()
            && let Some(f) = back
        {
            f.apply(&mut self.recipe);
            changed = true;
        }
        if changed {
            self.recipe.clamp();
            self.dirty = true;
        }
    }

    /// Group 3 — noise: one switch, the Retouch panel's AI denoise
    /// (`set_normal_denoise`).
    fn normal_noise(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, "Noise"), false, |_| {});
        // A run that ended without its card (failed, cancelled) leaves no
        // pair behind.
        if !self.busy && self.normal.denoise_pair.as_ref().is_some_and(|d| d.denoised.is_none()) {
            self.normal.denoise_pair = None;
        }
        let helper = denoise_helper_available();
        let mut on = self.normal_denoise_on();
        let toggled = ui
            .add_enabled(helper && !self.busy, egui::Checkbox::new(&mut on, tr(lang, "AI denoise")))
            .on_hover_text(tr(lang, "Cleans the grain out of the photo, and the preview shows the result — it helps night and dark phone photos most; it uses the graphics card when there is one. Turn it off to see the photo without it."))
            .changed();
        if helper && self.normal.denoise_ready.is_none() {
            let cfg = autoshade::config::Config::load();
            let weight = std::path::Path::new(&cfg.weights_dir).join(format!("scunet_{}.pth", cfg.denoise_model));
            self.normal.denoise_ready = Some(weight.is_file());
        }
        if toggled {
            self.apply_actions(vec![logic::Action::Normal(
                logic::normal::NormalAction::SetDenoise(on),
            )]);
        }
        for line in self.noise_lines(lang, helper) {
            ui.label(egui::RichText::new(line).weak().small());
        }
    }

    /// The lines under the noise switch, given whether the denoise helper is
    /// installed (a process-wide probe, so it is passed in).
    pub(crate) fn noise_lines(&self, lang: Lang, helper: bool) -> Vec<String> {
        if !helper {
            return vec![tr(lang, "This download does not include the noise tool.").into()];
        }
        // The switch is the Retouch panel's denoise: the preview shows it.
        let mut lines = vec![tr(lang, "It takes a moment; the preview then shows the result, and that is what gets saved.").to_string()];
        if self.normal.denoise_ready != Some(true) && !self.denoise_uses_raw() {
            lines.push(trf(
                lang,
                "The first time, the noise model is downloaded (about {mb} MB).",
                &[("mb", &DENOISE_FIRST_DOWNLOAD_MB.to_string())],
            ));
        }
        lines
    }

    /// Group 4 — save: a jpg of this photo, or the look on every photo of the
    /// folder.
    fn normal_save(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, "Save"), false, |_| {});
        let ready = self.src_path.is_some() && !self.busy;
        let mut save = false;
        ui.horizontal(|ui| {
            let cell = columns(ui, 2);
            save = primary_in(ui, cell, ready, tr(lang, "Save as jpg"))
                .on_hover_text(tr(lang, "Saves the photo with this look as a jpg; the original file is never changed."))
                .clicked();
            egui::ComboBox::from_id_salt("normal_quality")
                .selected_text(format!("{} · {}", tr(lang, "Quality"), self.normal.quality.label(lang)))
                .width(cell.x)
                .show_ui(ui, |ui| {
                    for q in SaveQuality::ALL {
                        ui.selectable_value(&mut self.normal.quality, q, q.label(lang));
                    }
                });
        });
        // The app's output folder from Settings, or the folder last saved to,
        // or a dialog — the one destination setting the workspace's Export
        // reads. (A folder inside the photo library is a valid destination; a
        // save only refuses to REPLACE an existing photo.)
        let place = match (self.exp_dest, self.export_dest_dir()) {
            (_, None) => tr(lang, "Asks where to save each time").to_string(),
            (ExportDest::LastUsed, Some(d)) => {
                trf(lang, "Saves to the folder you last saved to: {folder}", &[("folder", &crate::normal_actions::folder_name(&d))])
            }
            (_, Some(d)) => trf(
                lang,
                "Saves to the app's output folder {folder} (change it in Settings ⚙)",
                &[("folder", &crate::normal_actions::folder_name(&d))],
            ),
        };
        let full = self.export_dest_dir().map(|d| abs_display(&d)).unwrap_or_default();
        ui.label(egui::RichText::new(place).weak().small()).on_hover_text(full);
        let all = action_row(ui, ready && !self.gallery.is_empty(), tr(lang, "Apply this look to every photo in the folder"))
            .on_hover_text(tr(lang, "Gives every photo in the open folder this look and saves each one as a jpg."))
            .on_disabled_hover_text(tr(lang, "Open a folder first — the library on the left."))
            .clicked();
        if save {
            self.apply_actions(vec![logic::Action::Normal(logic::normal::NormalAction::SaveJpg)]);
        }
        if all {
            self.apply_actions(vec![logic::Action::Normal(
                logic::normal::NormalAction::ApplyToFolder,
            )]);
        }
    }
}
