//! The Lens Blur section (2026-10-13, user: 「AI 景深虚化」 — the AI estimates
//! depth and blurs the background progressively; the user picks the focus
//! point).
//!
//! VIEW ONLY. This file draws the section and emits [`logic::Action`]s; what
//! each action means, the depth worker and its landing live in
//! `logic/lens_blur.rs`. It may not mutate the recipe, spawn a worker, touch
//! the filesystem or open a dialog — `tests/architecture.rs` pins that.

use crate::*;

impl AutoShadeApp {
    /// The Lens Blur section of the Develop panel.
    pub(crate) fn dev_lens_blur(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        // A verb section: its body returns the actions the worker start
        // needs, and the row's draw applies them (the facade does this for
        // every surface).
        let actions = self.lens_blur_section(ui);
        let touched = !actions.is_empty();
        if touched {
            self.apply_actions(actions);
        }
        touched
    }

    fn lens_blur_section(&mut self, ui: &mut egui::Ui) -> Vec<logic::Action> {
        let lang = self.lang;
        let active = logic::lens_blur::active(&self.editor.doc().recipe);
        let mut actions = Vec::new();
        ui.add_space(SPACE_MD);
        card_fold(
            ui,
            egui::CollapsingHeader::new(section_title(tr(lang, K_LENS_LENS_BLUR), active))
                .id_salt("sec_lens_blur")
                .default_open(false),
            |ui| actions = self.lens_blur_body(ui),
        );
        actions
    }

    fn lens_blur_body(&mut self, ui: &mut egui::Ui) -> Vec<logic::Action> {
        let lang = self.lang;
        let mut actions = Vec::new();
        let has_helper = depth_helper_available();
        let can_run = !self.workspace.busy() && self.editor.photo().base_preview.is_some() && self.editor.photo().src_path.is_some() && has_helper;
        let has_blur = self.editor.doc().recipe.lens_blur.is_some();
        let verb = if has_blur { tr(lang, K_LENS_RE_ESTIMATE_DEPTH) } else { tr(lang, K_LENS_ESTIMATE_DEPTH) };
        let tip = if has_helper {
            ai_xref(lang, tr(lang, K_LENS_MOGE_2_ESTIMATES_HOW_FAR_AWAY_EVERY_PART_OF_THE))
        } else {
            tr(lang, K_LENS_THIS_FEATURE_NEEDS_AUTOSHADE_S_PYTHON_HELPER_WHI).to_string()
        };
        if action_row(ui, can_run, verb).on_hover_text(tip).clicked() {
            actions.push(logic::Action::Lens(logic::lens_blur::LensAction::EstimateDepth));
        }
        // The card's current values, copied out so the draw below never holds a
        // borrow into the recipe the facade is about to change.
        let Some((enabled, amount, range)) = self.editor.doc().recipe.lens_blur.as_ref().map(|l| (l.enabled, l.amount, l.range)) else {
            let hint = tr(lang, K_LENS_ESTIMATE_THE_DEPTH_FIRST_THEN_CLICK_THE_PHOTO_TO);
            ui.label(egui::RichText::new(hint).weak().small());
            return actions;
        };
        let picking = self.workspace.tools().lens_focus_picking;
        ui.horizontal(|ui| {
            let mute = tr(lang, K_LENS_SHOW_MUTE_THE_LENS_BLUR_WITHOUT_LOSING_ITS_SETTI);
            if glyph_toggle(ui, enabled, "👁").on_hover_text(mute).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::ToggleLensBlur(!enabled)));
            }
            let cell = egui::vec2(ui.available_width() - row_h(ui) - ui.spacing().item_spacing.x, row_h(ui));
            let how = tr(lang, K_LENS_CLICK_THE_PHOTO_WHATEVER_IS_AT_THAT_DISTANCE_STA);
            if toggle_in(ui, cell, picking, tr(lang, K_LENS_PICK_FOCUS)).on_hover_text(how).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetFocusPicking(!picking)));
            }
            if glyph(ui, true, "🗑").on_hover_text(tr(lang, K_LENS_REMOVE_THE_LENS_BLUR)).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::RemoveLensBlur));
            }
        });
        // The two dials of `render::LensBlur`; the copy is what the view may
        // touch, the recipe change goes through the action.
        ui.add_enabled_ui(enabled, |ui| {
            let mut next = amount;
            if slider(ui, lang, tr(lang, K_LENS_BLUR_AMOUNT), &mut next, 0.0, 100.0, 50.0) {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetBlurAmount(next)));
            }
            let mut next = range;
            if slider(ui, lang, tr(lang, K_LENS_FOCUS_RANGE), &mut next, 0.0, 100.0, 10.0) {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetFocusRange(next)));
            }
        });
        let carried = tr(lang, K_LENS_RENDERED_HERE_AND_IN_EVERY_EXPORT_FROM_THIS_APP);
        ui.label(egui::RichText::new(carried).weak().small());
        actions
    }

    /// 🎯 armed: a click sets the focus point in the original frame, the one
    /// the develop and the depth map share.
    pub(crate) fn handle_lens_focus_pick(
        &self,
        ui: &egui::Ui,
        resp: &egui::Response,
        xf: ViewXform,
    ) -> Option<logic::Action> {
        let (nx, ny) = self.editor.clicked_orig_norm(ui, resp, xf)?;
        Some(logic::Action::Lens(logic::lens_blur::LensAction::FocusPicked([nx, ny])))
    }

}
