//! The Portraits panel (design-portraits-mode.md §3, the user's ruling of
//! 2026-10-23: the full workspace, and one "Portraits" panel at the top of
//! the sidebar). Drawn first in the one workspace's controls column
//! (`app.rs` `controls_column`), above Normal's groups and the shared panels.
//!
//! Four groups, in the order a portrait is worked: find the people and the
//! faces (and an AI look made for portraits), the skin, the eyes, the
//! background. The finding is two sidecars on the original-frame preview —
//! the subject matte (`segment.py --target subject`) and the face finder
//! (`face.py`, MediaPipe) — and lands four ordinary bitmap masks carrying
//! their `MaskRole` (`autoshade::face`): Face, Eyes, Skin (the face narrowed
//! by a colour range seeded from the face) and Background (the subject,
//! inverted). Every slider here writes a field of the mask with that role,
//! or of the lens blur, so the recipe format does not change and every one
//! of them is also in the mask list below, editable there.
//!
//! "Keep skin natural" (§4) is a switch of this panel, on by default, not a
//! recipe field. While it is on, every write of the recipe — a slider here or
//! in the mask list, a paste, an AI landing — meets the same limits at one
//! place (`guard_portrait_skin`, run once per frame after the panels drew and
//! the workers landed). A write that goes to DISK in the same step — an AI
//! landing's save, a paste onto other photos — meets them where it is written
//! (`skin_held` / `skin_held_for`), before the file.

use crate::*;
use autoshade::face::{
    PORTRAIT_ROLES, crisp_of, darken_of, face_raster, fade_of, role_mask, set_crisp, set_darken,
    set_fade, set_soften, skin_target, soften_of, subject_raster,
};
use autoshade::recipe::{LocalAdjustment, MaskRole};

/// The eyes' "Brighten" slider's reach, in stops.
const EYES_BRIGHTEN_MAX_EV: f32 = 1.5;

pub(crate) use crate::logic::portrait::PortraitFind;




/// The one line under the first group.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum PortraitLine {
    #[default]
    Idle,
    Finding,
    /// The finding landed: faces found, and what failed, in the error's
    /// own words (the subject matte or the face finder).
    Found { faces: usize, failed: Option<String> },
    /// Nothing could be found at all.
    Failed(String),
    Looking,
    Looked,
}

/// The Portraits panel's own state. Nothing here is persisted (the
/// preferences file is unchanged); the masks it makes live in the recipe.
pub(crate) struct PortraitState {
    pub(crate) line: PortraitLine,
    /// The photo the last find step ran on, and how many faces it found —
    /// which words the greyed groups use ("no face found" vs "find them
    /// first").
    pub(crate) found: Option<(PathBuf, usize)>,
    /// "Keep skin natural" (design §4), per photo and card: (photo, card
    /// id) → on; a card with no entry is on. Opening never changes a value:
    /// when a photo or one of its saved versions is read from disk, every
    /// card whose saved edit already goes past the limits is entered OFF
    /// (`decide_keep_skin_for_photo` / `_for_card`), and nothing is clamped
    /// there until the user turns it on (`set_keep_skin`). A card made in
    /// this session (an AI look, a fit, a fork's default) starts on, so the
    /// app's own writes meet the limits; a fork inherits its parent's switch
    /// (`inherit_keep_skin`).
    pub(crate) keep_skin: std::collections::HashMap<(PathBuf, String), bool>,
    /// A depth estimate was started from the Background group: its landing
    /// aims the focus at the face and keeps the person sharp.
    pub(crate) blur_from_panel: bool,
    /// Test seam: what the last heal was started with (painted mask?, the
    /// limit raster) — `start_heal`, shared by the Retouch panel's heal and
    /// this panel's 🤖 Remove blemishes.
    #[cfg(test)]
    pub(crate) heal_asked: Option<(bool, Option<PathBuf>)>,
    /// Test seam: stands in for `retouch::heal` inside the heal worker.
    #[cfg(test)]
    pub(crate) heal_runner: Option<HealRunner>,
}

/// What the heal worker runs: `retouch::heal`, or a test's stand-in for it
/// — (the source, the limit raster, the output).
pub(crate) type HealRunner =
    fn(&std::path::Path, Option<&std::path::Path>, &std::path::Path) -> anyhow::Result<autoshade::retouch::HealReport>;

impl Default for PortraitState {
    fn default() -> Self {
        PortraitState {
            line: PortraitLine::Idle,
            found: None,
            keep_skin: Default::default(),
            blur_from_panel: false,
            #[cfg(test)]
            heal_asked: None,
            #[cfg(test)]
            heal_runner: None,
        }
    }
}

/// The find step itself, off the UI thread: the subject matte and the faces
/// on the original-frame preview (`original`), every raster under a freshly
/// claimed name beside the photo's develop. The skin seed is sampled from the
/// canvas's `base` as `recipe` develops it up to the skin mask
/// ([`panel_skin_seed`]); `lens` is the composed profile when that base is
/// lens-corrected (the two are then different frames). `film` / `raw_source`
/// are the canvas's (`film_short_edge`, `source_is_raw`).
#[allow(clippy::too_many_arguments)] // the two pictures, the base's lens, the photo, and the canvas's recipe, film edge and source kind
impl AutoShadeApp {
    /// 🤖 Find people and faces: both sidecars in one worker.
        /// The find landing: the four masks into the recipe — a mask
    /// that already has its role is re-pointed at the new raster and keeps
    /// its sliders — and the line says what was found.
        /// ✨ AI portrait look: the portrait preset through the one analysis door,
    /// a fresh look at the look strength. The masks and blur the photo has now
    /// are remembered by the look protocol and put back on the landed recipe.
    pub(crate) fn start_portrait_look(&mut self) {
        let kept: Vec<LocalAdjustment> =
            self.recipe.masks.iter().filter(|m| PORTRAIT_ROLES.contains(&m.role)).cloned().collect();
        let mut intent = logic::look::LookIntent::of(logic::look::LookKind::Portrait);
        intent.kept_masks = kept;
        intent.kept_blur = self.recipe.lens_blur.clone();
        self.apply_actions(vec![logic::Action::Look(intent)]);
    }

    /// 🤖 Remove blemishes: the Retouch panel's AI heal, held to the face
    /// mask — only the spots whose centre is on the face are healed.
        /// The frame's one place for "Keep skin natural": run after the panels
    /// drew and the workers landed, so a slider, a paste and an AI landing
    /// all meet it before the develop or the undo commit sees the recipe.
    /// Only while the switch is on for this card (`keep_skin_on`).
        /// Does "Keep skin natural" hold the open card's writes: the switch on
    /// for this card.
        /// Does it hold a write landing on `photo` (a paste): the open card's
    /// switch for the open photo, a stashed photo's card's own, and on for a
    /// photo not opened this session — the write replaces the edit opening
    /// would decide it on.
        /// Undo or redo put back an edit past the skin limits (made before the
    /// switch was on): it is put back as it was, so the switch goes off for
    /// the card — the next frame must not pull it back and record a new step
    /// over the redo history.
        /// The open card's switch as a history step records it (None: no photo).
        /// Undo or redo puts back the switch the step was recorded with. The
    /// values are not touched: the step's recipe already is what the switch
    /// left it as.
        /// The open card's key for the per-card switch.
        /// Is "Keep skin natural" on for the open card? On unless the card was
    /// read from disk past the limits or the user turned it off. No photo:
    /// on.
        /// The user's own flip of the switch. Turning it on is the explicit act
    /// that may bring values back within the limits, and the status line
    /// says how many it did.
        /// The photo was just read from disk (its strip settled): every card is
    /// decided afresh against the edit that was read — off where it already
    /// goes past the limits, on (no entry) elsewhere.
        /// Every card of the open photo whose edit goes past the limits: off.
        /// A saved version was just read onto the open card: that card alone is
    /// decided afresh, the same way.
        /// A card forked from the open one takes over its switch — a fork is an
    /// edit, not an opening.
        /// A depth estimate started from the Background group landed: aim the
    /// focus at the face (the skin colour range's sample point is the faces'
    /// centre) and keep the person sharp — instead of the canvas pick the
    /// Lens Blur section arms.
        /// "Keep the person sharp": the blur protects the subject matte (on), or
    /// nothing (off). `false` when there is no blur, or nothing to protect
    /// with.
        /// The first group's line, in the session language.
    pub(crate) fn portrait_line_text(&self, lang: Lang) -> String {
        match &self.portrait.line {
            PortraitLine::Idle => tr(lang, "Find the people first: the skin, eyes and background tools work on what is found.").into(),
            PortraitLine::Finding => tr(lang, "Finding the people and faces…").into(),
            PortraitLine::Found { faces: 0, .. } => tr(lang, "No face found in this photo").into(),
            PortraitLine::Found { faces: 1, .. } => tr(lang, "Found 1 face").into(),
            PortraitLine::Found { faces, .. } => trf(lang, "Found {n} faces", &[("n", &faces.to_string())]),
            PortraitLine::Failed(why) => why.clone(),
            PortraitLine::Looking if self.tools.analyze_inflight => tr(lang, "Making the portrait look…").into(),
            PortraitLine::Looking => tr(lang, "Find the people first: the skin, eyes and background tools work on what is found.").into(),
            PortraitLine::Looked => tr(lang, "Done — Ctrl+Z goes back").into(),
        }
    }

    /// Why the skin and eyes groups are greyed, or `None` when they work.
    pub(crate) fn portrait_face_gate(&self, lang: Lang) -> Option<&'static str> {
        if self.recipe.masks.iter().any(|m| matches!(m.role, MaskRole::Face | MaskRole::Skin)) {
            return None;
        }
        let searched = self.portrait.found.as_ref().is_some_and(|(p, n)| self.src_path.as_ref() == Some(p) && *n == 0);
        Some(if searched { tr(lang, "No face found in this photo") } else { tr(lang, "Find the people and faces first (the first group).") })
    }

    /// The panel: a first-level heading and the four groups, behind the same
    /// mid-open gate as the shared panels.
    pub(crate) fn portrait_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let editable = !self.open.open_in_flight;
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, "Portraits"), false, |_| {});
        let mut changed = false;
        ui.add_enabled_ui(editable, |ui| {
            self.portrait_people(ui);
            changed |= self.portrait_skin(ui);
            changed |= self.portrait_eyes(ui);
            changed |= self.portrait_background(ui);
        });
        if changed {
            self.recipe.clamp();
            self.dirty = true;
        }
        ui.add_space(SPACE_MD);
        ui.separator();
    }

    /// A group's sub-heading.
    fn portrait_group(ui: &mut egui::Ui, title: &str) {
        ui.add_space(SPACE_MD);
        ui.label(egui::RichText::new(title).strong());
    }

    /// Group 1 — the people and the background, and the AI portrait look.
    fn portrait_people(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        Self::portrait_group(ui, tr(lang, "People and background"));
        let ready = self.src_path.is_some() && self.base_preview.is_some() && !self.busy && !self.tools.analyze_inflight;
        let mut find = false;
        let mut look = false;
        ui.horizontal(|ui| {
            let cell = columns(ui, 2);
            find = action_in(ui, cell, ready, tr(lang, "🤖 Find people and faces"))
                .on_hover_text(tr(lang, "Finds the person and every face on this computer (MediaPipe finds the faces and eyes — install it with pip install mediapipe). The first run downloads the models."))
                .clicked();
            look = action_in(ui, cell, ready, tr(lang, "✨ AI portrait look"))
                .on_hover_text(tr(lang, "The AI looks at this photo and gives it a portrait look: softer skin, brighter eyes, a calmer background — Ctrl+Z goes back. Needs a service key in Settings (⚙)."))
                .clicked();
        });
        Self::slider_pct_hinted(
            ui,
            lang,
            tr(lang, "Look strength"),
            &mut self.aip.grade_strength,
            1.0,
            GRADE_STRENGTH_DEFAULT,
            tr(lang, "How strong the look is. Double-click to reset."),
        );
        let line = self.portrait_line_text(lang);
        ui.horizontal_wrapped(|ui| {
            if self.portrait.line == PortraitLine::Finding || (self.portrait.line == PortraitLine::Looking && self.tools.analyze_inflight) {
                ui.spinner();
            }
            ui.label(egui::RichText::new(line).small());
        });
        if find {
            self.apply_actions(vec![logic::Action::Portrait(
                logic::portrait::PortraitAction::FindPeople,
            )]);
        }
        if look {
            self.start_portrait_look();
        }
    }

    /// Group 2 — the skin: soften, warmth, colour, the guardrail switch.
    fn portrait_skin(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        Self::portrait_group(ui, tr(lang, "Skin"));
        let gate = self.portrait_face_gate(lang);
        let guarded = self.keep_skin_on();
        let mut changed = false;
        ui.add_enabled_ui(gate.is_none(), |ui| {
            let mut probe = LocalAdjustment::default();
            let m = skin_target(&mut self.recipe).unwrap_or(&mut probe);
            let mut soften = soften_of(m);
            let soft_max = logic::portrait::reach(guarded, -autoshade::face::SKIN_TEXTURE_MIN, 100.0);
            if Self::slider_hinted(ui, lang, tr(lang, "Soften skin"), &mut soften, 0.0, soft_max, 0.0, tr(lang, "Smooths the skin's fine texture; the face's shape and the eyes stay as they are.")) {
                set_soften(m, soften);
                changed = true;
            }
            let warm = logic::portrait::reach(guarded, autoshade::face::SKIN_TEMPERATURE_MAX, 100.0);
            changed |= Self::slider_hinted(ui, lang, tr(lang, "Skin warmth"), &mut m.temperature, -warm, warm, 0.0, tr(lang, "Cooler to the left, warmer to the right — on the skin only."));
            let rich = logic::portrait::reach(guarded, autoshade::face::SKIN_SATURATION_MAX, 100.0);
            changed |= Self::slider_hinted(ui, lang, tr(lang, "Skin colour intensity"), &mut m.saturation, -100.0, rich, 0.0, tr(lang, "Paler to the left, richer to the right — on the skin only."));
        });
        let mut on = guarded;
        let flipped = ui
            .checkbox(&mut on, tr(lang, "Keep skin natural"))
            .on_hover_text(tr(
                lang,
                "On: skin colour can be made only a little richer, warmer, cooler or shifted in tone, softening stops before the skin looks fake, and skin is never sharpened. This holds for your own changes and for the AI's.",
            ))
            .changed();
        if flipped {
            self.apply_actions(vec![logic::Action::Portrait(
                logic::portrait::PortraitAction::SetKeepSkin(on),
            )]);
        }
        let can_heal = face_raster(&self.recipe).is_some() && !self.busy && self.src_path.is_some();
        if action_row(ui, can_heal, tr(lang, "🤖 Remove blemishes"))
            .on_hover_text(tr(
                lang,
                "The AI finds small spots and blemishes on the face, and each is healed from the skin around it; only spots on the face are healed, and the heal of a spot at the face's edge reaches a little past it — Ctrl+Z goes back. Needs a service key in Settings (⚙).",
            ))
            .clicked()
        {
            self.apply_actions(vec![logic::Action::Portrait(
                logic::portrait::PortraitAction::RemoveBlemishes,
            )]);
        }
        if let Some(why) = gate {
            ui.label(egui::RichText::new(why).weak().small());
        }
        changed
    }

    /// Group 3 — the eyes: brighten, crisp.
    fn portrait_eyes(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        Self::portrait_group(ui, tr(lang, "Eyes"));
        let gate = self.portrait_face_gate(lang).or_else(|| {
            (!self.recipe.masks.iter().any(|m| m.role == MaskRole::Eyes)).then(|| tr(lang, "Find the people and faces first (the first group)."))
        });
        let mut changed = false;
        ui.add_enabled_ui(gate.is_none(), |ui| {
            let mut probe = LocalAdjustment::default();
            let m = role_mask(&mut self.recipe, MaskRole::Eyes).unwrap_or(&mut probe);
            changed |= Self::slider_hinted(ui, lang, tr(lang, "Brighten eyes"), &mut m.exposure_ev, 0.0, EYES_BRIGHTEN_MAX_EV, 0.0, tr(lang, "Lifts the eyes a little."));
            let mut crisp = crisp_of(m);
            if Self::slider_hinted(ui, lang, tr(lang, "Crisp eyes"), &mut crisp, 0.0, 100.0, 0.0, tr(lang, "Brings out the detail in the eyes.")) {
                set_crisp(m, crisp);
                changed = true;
            }
        });
        if let Some(why) = gate {
            ui.label(egui::RichText::new(why).weak().small());
        }
        changed
    }

    /// Group 4 — the background: the blur (with the person kept sharp), and
    /// darken / fade on the background mask.
    fn portrait_background(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        Self::portrait_group(ui, tr(lang, "Background"));
        let mut changed = false;
        let subject = subject_raster(&self.recipe);
        if self.recipe.lens_blur.is_none() {
            let can = !self.busy && self.base_preview.is_some() && self.src_path.is_some() && depth_helper_available();
            if action_row(ui, can, tr(lang, "🤖 Prepare the background blur"))
                .on_hover_text(tr(lang, "Measures how far away every part of the photo is, on this computer (the first run downloads a 1.3 GB model); then the blur sliders appear."))
                .clicked()
            {
                self.portrait.blur_from_panel = true;
                self.apply_actions(vec![logic::Action::Lens(logic::lens_blur::LensAction::EstimateDepth)]);
            }
        } else if let Some(lb) = self.recipe.lens_blur.as_mut() {
            changed |= Self::slider(ui, lang, tr(lang, "Blur amount"), &mut lb.amount, 0.0, 100.0, 50.0);
            changed |= Self::slider_hinted(ui, lang, tr(lang, "Sharp range"), &mut lb.range, 0.0, 100.0, 10.0, tr(lang, "How deep the sharp part of the picture is, around the face."));
            let mut keep = lb.protect.is_some();
            let toggled = ui
                .add_enabled(subject.is_some() || keep, egui::Checkbox::new(&mut keep, tr(lang, "Keep the person sharp")))
                .on_hover_text(tr(lang, "The whole person stays sharp, however far the blur reaches."))
                .on_disabled_hover_text(tr(lang, "Find the people and faces first (the first group)."))
                .changed();
            if toggled {
                self.apply_actions(vec![logic::Action::Portrait(
                    logic::portrait::PortraitAction::KeepPersonSharp(keep),
                )]);
            }
        }
        let has_bg = self.recipe.masks.iter().any(|m| m.role == MaskRole::Background);
        ui.add_enabled_ui(has_bg, |ui| {
            let mut probe = LocalAdjustment::default();
            let m = role_mask(&mut self.recipe, MaskRole::Background).unwrap_or(&mut probe);
            let mut darken = darken_of(m);
            if Self::slider_hinted(ui, lang, tr(lang, "Darken background"), &mut darken, 0.0, 100.0, 0.0, tr(lang, "Everything but the person, a little darker.")) {
                set_darken(m, darken);
                changed = true;
            }
            let mut fade = fade_of(m);
            if Self::slider_hinted(ui, lang, tr(lang, "Fade background colour"), &mut fade, 0.0, 100.0, 0.0, tr(lang, "Everything but the person, a little greyer.")) {
                set_fade(m, fade);
                changed = true;
            }
        });
        if !has_bg {
            ui.label(egui::RichText::new(tr(lang, "Find the people and faces first (the first group).")).weak().small());
        }
        changed
    }
}
