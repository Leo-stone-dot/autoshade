//! The develop panel's custom widgets: the tone-curve editor, the parametric
//! curve, the colour mixer (HSL) grid with its Point Color rows, and the colour
//! grading wheels. They are real widget code, not form rows — a capability table
//! cannot draw them — so they live in their own module instead of the panel.

use crate::*;
use crate::panels::grade_wheel::{grade_wheel, GRADE_VIEWS};

impl AutoShadeApp {
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_curves(&mut self, ui: &mut egui::Ui, curves_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;

        ui.add_space(SPACE_MD);
        // --- 曲线: master + RGB tone curves (engine + XMP already apply them,
        // this is purely the editing surface — Lightroom's panel order) --------
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Curves"), curves_active))
            .id_salt("sec_curves")
            .default_open(false)
            , |ui| {
                changed |= self.curve_editor(ui);
                changed |= Self::parametric_curve(ui, lang, &mut self.recipe);
            });
        changed
    }

    /// Lightroom's parametric tone curve (v1.5.0), under the point curve in
    /// the order the engine composes them (`render::build_tone_lut`: the point
    /// curve bends what these regions made). The four regions in the panel's
    /// top-down order, then the three splits — each bounded by its neighbours,
    /// the way Lightroom's own split handles stop short of each other, so a
    /// drag can never cross two splits and the sliders always show the layout
    /// the curve renders (`EditRecipe::parametric_splits`).
    fn parametric_curve(ui: &mut egui::Ui, lang: Lang, r: &mut EditRecipe) -> bool {
        use autoshade::recipe::{PARAMETRIC_SPLITS, PARAMETRIC_SPLIT_GAP};
        sub_caption(ui, tr(lang, "Parametric curve"));
        let mut changed = false;
        changed |= Self::slider(ui, lang, tr(lang, "Highlights"), &mut r.param_highlights, -100.0, 100.0, 0.0);
        changed |= Self::slider(ui, lang, tr(lang, "Lights"), &mut r.param_lights, -100.0, 100.0, 0.0);
        changed |= Self::slider(ui, lang, tr(lang, "Darks"), &mut r.param_darks, -100.0, 100.0, 0.0);
        changed |= Self::slider(ui, lang, tr(lang, "Shadows"), &mut r.param_shadows, -100.0, 100.0, 0.0);
        let gap = PARAMETRIC_SPLIT_GAP;
        let mut splits = r.parametric_splits();
        let mut moved = false;
        let labels = [tr(lang, "Shadow split"), tr(lang, "Midtone split"), tr(lang, "Highlight split")];
        for (i, label) in labels.into_iter().enumerate() {
            // The reset lands inside the room the neighbours leave: a reset
            // must not cross them either (`slider_impl` resets unclamped).
            let (lo, hi, reset) =
                crate::logic::slider::split_bounds(i, splits, gap, PARAMETRIC_SPLITS);
            // Whole numbers whatever the room: the sidecar writes integers,
            // and a narrow range would otherwise pick the fractional feel.
            moved |= Self::slider_impl(ui, lang, label, &mut splits[i], lo, hi, reset, SliderFeel::Int, "");
        }
        if moved {
            r.param_shadow_split = splits[0];
            r.param_midtone_split = splits[1];
            r.param_highlight_split = splits[2];
        }
        changed || moved
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    ///
    /// v1.5.0: the section is Lightroom's WHOLE Color Mixer. The Black & White
    /// switch swaps the colour bands for the grey mix, because the render runs
    /// one or the other (`render::apply_gray_mix` in the colour mixer's place,
    /// as in Lightroom) and showing both would be eight sliders that move no
    /// pixel; the point colours follow the mixer ([`Self::point_color_rows`]).
    pub(crate) fn dev_hsl(&mut self, ui: &mut egui::Ui, hsl_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Color Mixer (HSL)"), hsl_active))
            .id_salt("sec_hsl")
            .default_open(false)
            , |ui| {
                changed |= ui
                    .checkbox(&mut self.recipe.convert_to_grayscale, tr(lang, "Black & White"))
                    .changed();
                // What the CANVAS does, not what the checkbox says (v1.5.0 F7):
                // a monochrome creative profile develops the photograph to grey
                // with the photographer's own switch still off, and a panel
                // offering the colour mixer over a grey canvas is a row of
                // sliders that move numbers and no pixels. `render::apply_develop`
                // asks the same question the same way.
                let mono = self.recipe.renders_grayscale();
                // LR's mixer layout: pick ONE property (Hue / Saturation /
                // Luminance) and see all 8 bands at once. The old band picker
                // showed one band at a time — every other band's state was
                // invisible, the opposite of what a mixer is for. The B&W
                // mixer has ONE property (how light each colour turns), so it
                // has no tabs — its caption stands where they do.
                ui.horizontal(|ui| {
                    if mono {
                        ui.label(egui::RichText::new(tr(lang, "B&W mix")).weak().small());
                    } else {
                        for (i, name) in ["Hue", "Saturation", "Luminance"].iter().enumerate() {
                            if ui.selectable_label(self.ai.hsl_tab == i, tr(lang, name)).clicked() {
                                self.ai.hsl_tab = i;
                            }
                        }
                    }
                    // R38: a glyph square — the same reset in every mixer row;
                    // as a text button it ran the HSL tab row to 318 px at the
                    // 320 px default and widened the auto-fitting panel.
                    if glyph(ui, true, "↺").on_hover_text(tr(lang, "Reset all")).clicked() {
                        if mono {
                            for b in 0..HSL_BANDS.len() {
                                if let Some(v) = self.recipe.gray_mixer_mut(b) {
                                    *v = 0.0;
                                }
                            }
                        } else {
                            self.recipe.hsl = Hsl::default();
                        }
                        changed = true;
                    }
                });
                let tab = self.ai.hsl_tab;
                for (b, name) in HSL_BANDS.iter().enumerate() {
                    // ONE row shape for both mixers: same band, same swatch,
                    // same ±100 track — only the number behind it changes.
                    let v = match (mono, tab) {
                        (true, _) => match self.recipe.gray_mixer_mut(b) {
                            Some(v) => v,
                            None => continue,
                        },
                        (false, 0) => &mut self.recipe.hsl.hue[b],
                        (false, 1) => &mut self.recipe.hsl.saturation[b],
                        (false, _) => &mut self.recipe.hsl.luminance[b],
                    };
                    ui.horizontal(|ui| {
                        // Band swatch so rows scan like LR's mixer.
                        let (rect, _) = ui
                            .allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, HSL_SWATCH[b]);
                        changed |= Self::slider(ui, lang, tr(lang, name), v, -100.0, 100.0, 0.0);
                    });
                }
                changed |= self.point_color_rows(ui);
            });
        changed
    }

    /// Lightroom's Point Color (v1.5.0), under the mixer as in Lightroom: the
    /// 💧 that arms the canvas eyedropper (`canvas::handle_point_color_pick`
    /// makes the swatch), then one block per swatch — the colour it was
    /// sampled from, its three shifts and its Range.
    ///
    /// Disabled under Black & White, where `apply_gray_mix` runs in the
    /// mixer's place and a swatch has no colour left to move.
    ///
    /// The Range slider goes through [`autoshade::recipe::PointColor::set_range`],
    /// which REBUILDS the swatch's three windows: the engine matches pixels
    /// against the stored windows and never reads `range_amount`, so a Range
    /// that only wrote its own number would be a slider that moves no pixel.
    fn point_color_rows(&mut self, ui: &mut egui::Ui) -> bool {
        use autoshade::recipe::MAX_POINT_COLORS;
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_SM);
        // Point Color has no colour to act on in black and white, whichever of
        // the two switches produced it (v1.5.0 F7 — see the mixer above).
        ui.add_enabled_ui(!self.recipe.renders_grayscale(), |ui| {
            ui.horizontal_wrapped(|ui| {
                // One cell of the grid beside the caption (2026-09-27).
                let cell = columns(ui, 2);
                ui.label(egui::RichText::new(tr(lang, "Point Color")).weak().small());
                let label = if self.tools.point_color_picking {
                    tr(lang, "💧 Click in image…")
                } else {
                    tr(lang, "💧 Pick a color")
                };
                let room = self.recipe.point_colors.len() < MAX_POINT_COLORS;
                if action_in(ui, cell, room, label)
                    .on_hover_text(tr(lang,
                        "Click a colour in the image to add a swatch; its sliders then move that colour alone. Click again to cancel.",
                    ))
                    // The limit says itself where the button that hit it is —
                    // the canvas handler repeats it for the click that arrives
                    // after a paste filled the list behind an armed tool.
                    .on_disabled_hover_text(trf(
                        lang,
                        "Point Color: at most {n} swatches",
                        &[("n", &MAX_POINT_COLORS.to_string())],
                    ))
                    .clicked()
                {
                    let on = !self.tools.point_color_picking;
                    self.disarm_tools();
                    self.tools.point_color_picking = on;
                    if on {
                        self.status =
                            tr(lang, "Point Color eyedropper: click the colour to adjust").into();
                    }
                }
            });
            // Deferred like every other list row's ✕ (R24-5): removing inside
            // the loop would invalidate the iterator it is drawn from.
            let mut remove = None;
            for (i, p) in self.recipe.point_colors.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    // The colour the swatch was sampled from, as a chip: the
                    // sliders below say what happens to it, and nothing else
                    // on the row says WHICH colour it is.
                    let rgb = autoshade::render::point_color_rgb(p)
                        .map(|c| (c.clamp(0.0, 1.0) * 255.0).round() as u8);
                    let (rect, _) =
                        ui.allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                    ui.painter().rect_filled(
                        rect,
                        2.0,
                        egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2]),
                    );
                    ui.label(trf(lang, "Swatch {n}", &[("n", &(i + 1).to_string())]));
                    // 🗑, the vocabulary's DELETE-a-list-row glyph (masks,
                    // versions) — ✕ is "clear / close" and says the wrong thing
                    // about a swatch that is gone for good.
                    if glyph(ui, true, "🗑").on_hover_text(tr(lang, "Remove swatch")).clicked() {
                        remove = Some(i);
                    }
                });
                // Lightroom's ±100 tracks over the stored -1..=1 shifts.
                for (name, shift) in [
                    ("Hue", &mut p.hue_shift),
                    ("Saturation", &mut p.sat_scale),
                    ("Luminance", &mut p.lum_scale),
                ] {
                    let mut shown = *shift * 100.0;
                    if Self::slider(ui, lang, tr(lang, name), &mut shown, -100.0, 100.0, 0.0) {
                        *shift = shown / 100.0;
                        changed = true;
                    }
                }
                let mut range = p.range_amount;
                if Self::slider_pct(ui, lang, tr(lang, "Range"), &mut range, 1.0, 0.5) {
                    p.set_range(range);
                    changed = true;
                }
            }
            if let Some(i) = remove {
                self.recipe.point_colors.remove(i);
                changed = true;
            }
        });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_grading(&mut self, ui: &mut egui::Ui, grade_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;

        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Color Grading"), grade_active))
            .id_salt("sec_grade")
            .default_open(false)
            , |ui| {
                // Lightroom's grading panel (2026-10-13): the 3-way view shows
                // the shadow, midtone and highlight wheels together, each with
                // its luminance under it; Global is one wheel for the whole
                // picture (`panels/grade_wheel.rs`).
                ui.horizontal(|ui| {
                    let gap = ui.spacing().item_spacing.x;
                    let w = ((ui.available_width().min(FIELD_W_MAX) - row_h(ui) - 2.0 * gap) / 2.0).floor();
                    for (i, name) in GRADE_VIEWS.iter().enumerate() {
                        if toggle_in(ui, egui::vec2(w, row_h(ui)), self.ai.grade_region == i, tr(lang, name)).clicked() {
                            self.ai.grade_region = i;
                        }
                    }
                    // R38: a glyph square — the same reset in every mixer row.
                    if glyph(ui, true, "↺").on_hover_text(tr(lang, "Reset all")).clicked() {
                        self.recipe.color_grade = ColorGrade::default();
                        changed = true;
                    }
                });
                ui.add_space(SPACE_SM);
                let cg = &mut self.recipe.color_grade;
                // One wheel with its luminance slider in a column `width` wide.
                let wheel = |ui: &mut egui::Ui, title: &str, width: f32, h: &mut f32, s: &mut f32, l: &mut f32| {
                    let mut c = false;
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        c |= grade_wheel(ui, lang, title, (width - 8.0).min(170.0), h, s);
                        c |= Self::slider(ui, lang, tr(lang, "Luminance"), l, -100.0, 100.0, 0.0);
                    });
                    c
                };
                let avail = ui.available_width();
                if self.ai.grade_region == 0 {
                    // Midtones on top, centred; shadows and highlights under it.
                    let half = ((avail - ui.spacing().item_spacing.x) / 2.0).floor();
                    ui.horizontal(|ui| {
                        ui.add_space(((avail - half) / 2.0).floor());
                        changed |= wheel(ui, tr(lang, "Midtones"), half, &mut cg.midtone_hue, &mut cg.midtone_sat, &mut cg.midtone_lum);
                    });
                    ui.add_space(SPACE_SM);
                    ui.horizontal(|ui| {
                        changed |= wheel(ui, tr(lang, "Shadows"), half, &mut cg.shadow_hue, &mut cg.shadow_sat, &mut cg.shadow_lum);
                        changed |= wheel(ui, tr(lang, "Highlights"), half, &mut cg.highlight_hue, &mut cg.highlight_sat, &mut cg.highlight_lum);
                    });
                } else {
                    let width = (avail * 0.7).floor();
                    ui.horizontal(|ui| {
                        ui.add_space(((avail - width) / 2.0).floor());
                        changed |= wheel(ui, tr(lang, "Global"), width, &mut cg.global_hue, &mut cg.global_sat, &mut cg.global_lum);
                    });
                }
                // Blending/Balance shape the WHOLE grade, not one wheel — a
                // scope caption keeps them from reading as a wheel's own.
                sub_caption(ui, tr(lang, "All regions"));
                changed |= Self::slider(ui, lang, tr(lang, "Blending"), &mut cg.blending, 0.0, 100.0, 50.0);
                changed |= Self::slider(ui, lang, tr(lang, "Balance"), &mut cg.balance, -100.0, 100.0, 0.0);
            });
        changed
    }
}
