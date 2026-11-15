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
    pub(crate) fn dev_lens_blur(&mut self, ui: &mut egui::Ui) -> Vec<logic::Action> {
        let lang = self.lang;
        let active = logic::lens_blur::active(&self.recipe);
        let mut actions = Vec::new();
        ui.add_space(SPACE_MD);
        card_fold(
            ui,
            egui::CollapsingHeader::new(section_title(tr(lang, "Lens Blur"), active))
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
        let can_run = !self.busy && self.photo.base_preview.is_some() && self.photo.src_path.is_some() && has_helper;
        let has_blur = self.recipe.lens_blur.is_some();
        let verb = if has_blur { tr(lang, "🤖 Re-estimate depth") } else { tr(lang, "🤖 Estimate depth") };
        let tip = if has_helper {
            ai_xref(lang, tr(lang, "MoGe-2 estimates how far away every part of the photo is (Python helper; the first run downloads a ~1.3 GB model). Then click the photo to choose what stays sharp."))
        } else {
            tr(lang, "This feature needs AutoShade's Python helper, which this download does not include. Install the full package, or (advanced) set AUTOSHADE_DEPTH_SCRIPT to the path of depth.py").to_string()
        };
        if action_row(ui, can_run, verb).on_hover_text(tip).clicked() {
            actions.push(logic::Action::Lens(logic::lens_blur::LensAction::EstimateDepth));
        }
        // The card's current values, copied out so the draw below never holds a
        // borrow into the recipe the facade is about to change.
        let Some((enabled, amount, range)) = self.recipe.lens_blur.as_ref().map(|l| (l.enabled, l.amount, l.range)) else {
            let hint = tr(lang, "Estimate the depth first, then click the photo to choose what stays sharp.");
            ui.label(egui::RichText::new(hint).weak().small());
            return actions;
        };
        let picking = self.tools.lens_focus_picking;
        ui.horizontal(|ui| {
            let mute = tr(lang, "Show/mute the lens blur without losing its settings");
            if glyph_toggle(ui, enabled, "👁").on_hover_text(mute).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::ToggleLensBlur(!enabled)));
            }
            let cell = egui::vec2(ui.available_width() - row_h(ui) - ui.spacing().item_spacing.x, row_h(ui));
            let how = tr(lang, "Click the photo: whatever is at that distance stays sharp; nearer and farther blur by how far they are from it.");
            if toggle_in(ui, cell, picking, tr(lang, "🎯 Pick focus")).on_hover_text(how).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetFocusPicking(!picking)));
            }
            if glyph(ui, true, "🗑").on_hover_text(tr(lang, "Remove the lens blur")).clicked() {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::RemoveLensBlur));
            }
        });
        // The two dials of `render::LensBlur`; the copy is what the view may
        // touch, the recipe change goes through the action.
        ui.add_enabled_ui(enabled, |ui| {
            let mut next = amount;
            if Self::slider(ui, lang, tr(lang, "Blur amount"), &mut next, 0.0, 100.0, 50.0) {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetBlurAmount(next)));
            }
            let mut next = range;
            if Self::slider(ui, lang, tr(lang, "Focus range"), &mut next, 0.0, 100.0, 10.0) {
                actions.push(logic::Action::Lens(logic::lens_blur::LensAction::SetFocusRange(next)));
            }
        });
        let carried = tr(lang, "Rendered here and in every export from this app; Lightroom's sidecar cannot carry it.");
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
        let (nx, ny) = self.clicked_orig_norm(ui, resp, xf)?;
        Some(logic::Action::Lens(logic::lens_blur::LensAction::FocusPicked([nx, ny])))
    }

    /// The focus point as a small ring while the pick is armed, so the
    /// photographer sees where the current focus sits before moving it.
    pub(crate) fn draw_lens_focus(&self, ui: &egui::Ui, xf: ViewXform) {
        let Some(lb) = self.recipe.lens_blur.as_ref() else { return };
        let ((bw, bh), deg, dist) = self.geom_ctx();
        let (vx, vy) = orig_norm_to_view(lb.focus[0], lb.focus[1], (bw, bh), deg, &dist);
        let c = xf.to_screen(vx, vy);
        let p = ui.painter();
        p.circle_stroke(c, 9.0, egui::Stroke::new(3.0, egui::Color32::from_black_alpha(150)));
        p.circle_stroke(c, 9.0, egui::Stroke::new(1.5, egui::Color32::from_white_alpha(235)));
        p.circle_filled(c, 2.0, egui::Color32::WHITE);
    }
}
