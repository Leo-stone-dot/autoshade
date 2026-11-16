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
    /// The colour-field row (the mask's own "one luminance colour" row; not a
    /// mask, which is why it is not a list entry).
    pub(crate) fn dev_mask_colour_field(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        let mut changed = false;
        // R33 §G. The colour field, in the Masks section because that is
        // where a photographer looks for "what else is local about this
        // photo" — one row, never a list: a recipe holds at most one.
        //
        // Its own row rather than a mask entry, because it is not a mask:
        // it has no geometry to select, no coverage to preview and no
        // stack position to drag. Three controls, which are the three
        // questions a user actually has about it — is it on, how much of
        // it, and get rid of it.
        if self.editor.doc().recipe.colour_field.is_some() {
            ui.add_space(SPACE_XS);
            let mut remove_field = false;
            ui.horizontal(|ui| {
                let field = self.editor.doc_mut().recipe.colour_field.as_mut().expect("just checked");
                if glyph_toggle(ui, field.enabled, "👁")
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_SHOW_MUTE_THE_COLOUR_FIELD_WITHOUT_LOSING_ITS_AM,
                    ))
                    .clicked()
                {
                    field.enabled = !field.enabled;
                    changed = true;
                }
                let label = format!(
                    "▦ {} · {}",
                    tr(lang, K_DEVELOP_COLOUR_FIELD),
                    tr(lang, K_DEVELOP_ENGINE_ONLY),
                );
                ui.label(if field.enabled {
                    egui::RichText::new(label)
                } else {
                    egui::RichText::new(label).weak()
                })
                .on_hover_text(tr(
                    lang,
                    K_DEVELOP_A_SMOOTH_LOCAL_COLOUR_TONE_FIELD_THE_REVERSE_FIT,
                ));
                if glyph(ui, true, "🗑")
                    .on_hover_text(tr(lang, K_DEVELOP_REMOVE_THE_COLOUR_FIELD))
                    .clicked()
                {
                    remove_field = true;
                }
            });
            let field = self.editor.doc_mut().recipe.colour_field.as_mut().expect("just checked");
            // Through the shared slider (its look, reset to the solved
            // field's full amount, ↑/↓) — it was the one bare egui slider.
            let enabled = field.enabled;
            if ui
                .add_enabled_ui(enabled, |ui| {
                    slider_pct(ui, lang, tr(lang, K_DEVELOP_AMOUNT), &mut field.amount, 1.0, 1.0)
                })
                .inner
            {
                changed = true;
            }
            if remove_field {
                self.editor.doc_mut().recipe.colour_field = None;
                changed = true;
            }
        }
        changed
    }

    /// The selected mask's adjustment half: the four captioned groups and the
    /// mask's own curve editor. Tone edits are not coverage, so they are
    /// deliberately outside the overlay invalidation above.
    pub(crate) fn dev_mask_adjustments(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        let m = &mut self.editor.doc_mut().recipe.masks[i];
        // Lightroom's own three groups for a mask's adjustments (user
        // decision 8 of the 17-feedback triage): Tone / Detail /
        // Color, weak captions in the Color-Grading "All regions"
        // style. Twelve sliders in one unbroken column is what the
        // grouping replaces — the same reason the global panel is
        // sectioned. `Tone` is our own caption for LR's "Light" group:
        // the key "Light" already means the light THEME.
        let group = |ui: &mut egui::Ui, title: &str| sub_caption(ui, title);
        group(ui, tr(lang, K_DEVELOP_TONE));
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_EXPOSURE), &mut m.exposure_ev, -5.0, 5.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_CONTRAST), &mut m.contrast, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_HIGHLIGHTS), &mut m.highlights, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_SHADOWS), &mut m.shadows, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_WHITES), &mut m.whites, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_BLACKS), &mut m.blacks, -100.0, 100.0, 0.0);
        // Texture / Clarity / Dehaze come out of the old
        // 「More (XMP/Lightroom only)」 fold: R22-3 put all three ON the
        // engine (render::apply_masks), so that title became a lie and
        // the fold merely hid three working sliders one level deep.
        group(ui, tr(lang, K_DEVELOP_DETAIL));
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_TEXTURE), &mut m.texture, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_CLARITY), &mut m.clarity, -100.0, 100.0, 0.0);
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_DEHAZE), &mut m.dehaze, -100.0, 100.0, 0.0);
        // R23-1b. SIGNED, unlike the global 0..150 Sharpening: this is
        // ACR's local band, and its negative half (soften) is what
        // throws a background back — the tooltip says so, because the
        // two controls share a name and not a range.
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_SHARPNESS),
            &mut m.sharpness,
            -100.0,
            100.0,
            0.0,
            tr(lang, K_DEVELOP_SHARPENS_INSIDE_THE_MASK_WHEN_POSITIVE_AND_SOFTE),
        );
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_NOISE_REDUCTION), &mut m.noise_reduction, 0.0, 100.0, 0.0);
        group(ui, tr(lang, K_DEVELOP_COLOR));
        changed |= slider(ui, lang, tr(lang, K_DEVELOP_SATURATION), &mut m.saturation, -100.0, 100.0, 0.0);
        // R23-1b: a hue ROTATION of everything the mask covers — the
        // one colour move the global 8-band mixer cannot make, because
        // that one acts on a band everywhere in the frame.
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_HUE_SHIFT),
            &mut m.hue,
            -100.0,
            100.0,
            0.0,
            tr(lang, K_DEVELOP_ROTATES_EVERY_COLOUR_INSIDE_THE_MASK_100_30_UNLI),
        );
        // Engine-rendered since batch #2-B (render.rs apply_masks
        // mirrors the global WB model inside the mask) — live in the
        // preview like the tone sliders above. The ±100 pair is a
        // RELATIVE gel around a fixed anchor, a different axis from
        // the global 「Temp (K)」 absolute Kelvin — two controls named
        // "temp" that answer different questions, so the tooltip
        // carries the engine's OWN conversion (render::
        // local_temp_to_kelvin, not a number retyped here) for the
        // value currently on the slider.
        let temp_k = format!(
            "{:.0}",
            autoshade::render::local_temp_to_kelvin(m.temperature)
        );
        let temp_hint = trf(
            lang,
            K_DEVELOP_A_RELATIVE_WARM_COOL_SHIFT_100_AROUND_A_FIXED_55,
            &[("k", &temp_k)],
        );
        changed |= slider_hinted(ui, lang, tr(lang, K_DEVELOP_TEMP_SHIFT), &mut m.temperature, -100.0, 100.0, 0.0, &temp_hint);
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_TINT_SHIFT),
            &mut m.tint,
            -100.0,
            100.0,
            0.0,
            tr(lang, K_DEVELOP_A_RELATIVE_GREEN_MAGENTA_SHIFT_100_INSIDE_THE_MA),
        );
        // Lightroom's mask "Color" swatch (`crs:LocalToningSaturation`
        // / `LocalToningHue`), as its two numbers: how strongly, then
        // toward which hue. Last in the group because the render
        // applies it last (`render::apply_masks`); engine-rendered and
        // carried by the sidecar, and the hue does nothing at 0.
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_COLOR_TINT),
            &mut m.toning_amount,
            0.0,
            100.0,
            0.0,
            tr(lang, K_DEVELOP_LIGHTROOM_S_MASK_COLOR_SWATCH_SHIFTS_EVERY_COLOU),
        );
        changed |= slider_hinted(
            ui,
            lang,
            tr(lang, K_DEVELOP_COLOR_TINT_HUE),
            &mut m.toning_hue,
            0.0,
            360.0,
            0.0,
            tr(lang, K_DEVELOP_THE_HUE_0_360_COLOR_TINT_PUSHES_TOWARD_ON_LIGHTR),
        );
        // Reverse-fit recolour gains: per-channel linear gains no
        // classic-ACR key can express (recipe.rs `color_gains`); the
        // sidecar carries them as the mask's Temp/Tint through Camera
        // Raw's mask white law (`render::gains_as_white`). Not a
        // slider — a disclosure plus the one action that makes sense
        // on a value the user never typed.
        if m.color_gains.is_some() {
            ui.horizontal_wrapped(|ui| {
                // One cell of the grid, measured at the row's start;
                // it wraps under the note when the note is long (2026-09-27).
                let cell = columns(ui, 2);
                ui.label(
                    egui::RichText::new(tr(lang, K_DEVELOP_CARRIES_REVERSE_FIT_RECOLOUR_WRITTEN_TO_XMP_AS_T))
                        .weak()
                        .small(),
                );
                if action_in(ui, cell, true, tr(lang, K_DEVELOP_CLEAR))
                    .on_hover_text(tr(lang, K_DEVELOP_DROP_THIS_MASK_S_PER_CHANNEL_RECOLOUR_GAINS_ONE))
                    .clicked()
                {
                    m.color_gains = None;
                    changed = true;
                }
            });
        }
        // R25 P6 — the FOURTH group under Lightroom's three: this
        // mask's own point curves (`crs:MainCurve` / `RedCurve` /
        // `GreenCurve` / `BlueCurve`). The very editor the global
        // Curves section uses, pointed at this mask, so the plot, the
        // click/drag/drag-out gestures and the engine-faithful line
        // stay ONE implementation — a second copy is where the
        // preview line drifts from `render::curve_lut`.
        //
        // Last in the section on purpose: it is the only control here
        // that is not a slider, and it is the tallest.
        group(ui, tr(lang, K_DEVELOP_CURVE));
        changed |= self.curve_editor_for(ui, CurveTarget::Mask(i));
        changed
    }
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
