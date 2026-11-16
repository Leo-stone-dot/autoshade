//! What a panel may touch: a projection of the aggregates, never the app.
//!
//! A panel draws with the edit (the widgets that own a recipe vector — the
//! curve editor — mutate it), the view state, the language and the test seams.
//! It cannot reach the workers, the services or another panel's methods: the
//! type simply does not carry them.

use crate::*;

pub(crate) struct PanelView<'a, 'b> {
    pub(crate) editor: &'a mut Editor,
    pub(crate) workspace: &'a mut WorkspaceView,
    pub(crate) lang: Lang,
    /// The frame's action sink: a panel reports intents, the app runs them.
    pub(crate) actions: &'b mut Vec<logic::Action>,
    #[cfg(test)]
    pub(crate) seams: &'a mut TestSeams,
}

impl PanelView<'_, '_> {
    /// One capability table through the registry (the panel's only write
    /// surface for sliders).
    pub(crate) fn capability_rows(
        &mut self,
        ui: &mut egui::Ui,
        table: &[&dyn crate::controls::ControlWidget],
    ) -> bool {
        crate::panels::develop_forms::render_and_apply(ui, self.lang, &mut self.editor.doc_mut().recipe, table)
    }

    /// Report a batch of intents.
    pub(crate) fn act_all(&mut self, actions: impl IntoIterator<Item = logic::Action>) {
        self.actions.extend(actions);
    }

    /// Toggle the clipping overlay (the J overlay and the histogram's corner
    /// triangles share it). At rest it drops the layer; once on it rebuilds
    /// from the retained RGB frame, so the toggle is instant — no redevelop.
    pub(crate) fn toggle_clipping(&mut self, ctx: &egui::Context) {
        self.workspace.chrome_mut().show_clipping = !self.workspace.chrome().show_clipping;
        if !self.workspace.chrome().show_clipping {
            self.workspace.chrome_mut().clip_tex = None; // OFF is just dropping the layer
            return;
        }
        match &self.workspace.chrome().last_rgb {
            Some(rgb) => {
                let clip = clipping_overlay_for(rgb, self.editor.doc().recipe.crop);
                if let Some(tex) = &mut self.workspace.chrome_mut().clip_tex {
                    tex.set(clip, egui::TextureOptions::NEAREST);
                } else {
                    self.workspace.chrome_mut().clip_tex =
                        Some(ctx.load_texture("clip", clip, egui::TextureOptions::NEAREST));
                }
            }
            None => self.editor.session_mut().dirty = true, // nothing retained (fresh open) — worker builds it
        }
    }
}
