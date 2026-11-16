//! What a panel may touch: a projection of the aggregates, never the app.
//!
//! A panel draws with the edit (the widgets that own a recipe vector — the
//! curve editor — mutate it), the view state, the language and the test seams.
//! It cannot reach the workers, the services or another panel's methods: the
//! type simply does not carry them.

use crate::*;
use autoshade::advisor::catalogue::{family_is_active, CONTROL_FAMILIES};
use crate::panels::develop_widgets::parametric_curve;
use crate::panels::ai::{is_working_note, note_args, thinking_notes};

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
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_presence(&mut self, ui: &mut egui::Ui, presence_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // LR-Basic order (UX batch): Presence sits directly under Tone & WB —
        // the two halves of Lightroom's Basic panel — then Curves, then the
        // two colour sections; Detail moved BELOW them so colour work isn't
        // interrupted by sharpening. add_space fences each header so the ten
        // sections read as groups, not one undivided list.
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_PRESENCE), presence_active))
            .id_salt("sec_presence")
            .default_open(true)
            , |ui| {
                // Lightroom's Basic-panel order inside Presence is Texture →
                // Clarity → Dehaze, and the engine runs them in that order too
                // (R25 B2 put the global texture stage between clarity and
                // saturation). The rows live in `logic::capability::PRESENCE`;
                // rows and order are one table.
                changed |= self.capability_rows(ui, crate::controls::PRESENCE);
            });
        changed
    }

    /// 校准 — Lightroom's Calibration panel (v1.5.0): the shadows tint and each
    /// primary's hue and saturation.
    ///
    /// Rendered as the develop's FIRST stage (`render::Calibration`), which is
    /// why it is a section of sliders and not a read-out: every other section
    /// works on the colours these seven leave behind. Lightroom writes all
    /// seven keys on nearly every sidecar, so a photo arriving from Lightroom
    /// usually lands here with values already in it.
    pub(crate) fn dev_calibration(&mut self, ui: &mut egui::Ui, calibration_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_CALIBRATION), calibration_active))
            .id_salt("sec_calibration")
            .default_open(false)
            , |ui| {
                // Lightroom's own order and grouping: the shadows tint first,
                // then a hue + saturation pair per primary. The group captions
                // stay here; the rows are capability tables.
                ui.label(egui::RichText::new(tr(lang, K_DEVELOP_SHADOWS)).weak().small());
                changed |= self.capability_rows(ui, crate::controls::CAL_TINT);
                for (primary, table) in [
                    (tr(lang, K_DEVELOP_RED_PRIMARY), crate::controls::CAL_RED),
                    (tr(lang, K_DEVELOP_GREEN_PRIMARY), crate::controls::CAL_GREEN),
                    (tr(lang, K_DEVELOP_BLUE_PRIMARY), crate::controls::CAL_BLUE),
                ] {
                    ui.add_space(SPACE_XS);
                    ui.label(egui::RichText::new(primary).weak().small());
                    changed |= self.capability_rows(ui, table);
                }
            });
        changed
    }

    /// 效果 — Lightroom's post-crop vignette and film grain.
    ///
    /// All nine RENDER since v1.5.0 (`render/finish.rs`, the stage this engine
    /// grew after its crop), so none of them carries the "moves a number and
    /// no pixel" disclosure any more. The five COMPANIONS (Midpoint, Feather,
    /// Style, Grain size and roughness) show the value the engine actually
    /// renders — Lightroom's own default while the recipe holds none — and a
    /// reset returns them to absent (`companion_slider_edit` + the command).
    pub(crate) fn dev_effects(&mut self, ui: &mut egui::Ui, effects_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_EFFECTS), effects_active))
            .id_salt("sec_effects")
            .default_open(false)
            , |ui| {
                ui.label(egui::RichText::new(tr(lang, K_DEVELOP_POST_CROP_VIGNETTING)).weak().small());
                // Two tables because the panel draws two captioned groups; the
                // rows, their bands (the catalogue), their feels and their
                // companion encoding live in `logic::capability`. The style row
                // is an operator INDEX (1/2/3); the track starts at 1 because
                // 0 means "the sidecar named none" and only a reset reaches it.
                changed |= self.capability_rows(ui, crate::controls::VIGNETTE);
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, K_DEVELOP_GRAIN)).weak().small());
                changed |= self.capability_rows(ui, crate::controls::GRAIN);
            });
        changed
    }

    /// v1.5.0 F8 — Lightroom's HDR edit mode and the SDR rendition it is
    /// published through.
    ///
    /// The switch is drawn FIRST and the seven controls under a heading that
    /// says they need it, because that is the one thing a photographer can get
    /// wrong here: the seven are live widgets whatever the switch says (they
    /// are stored either way, and Lightroom's own sidecars keep them across a
    /// mode change), but nothing they do reaches a pixel while the mode is
    /// off. Greying them out instead would hide values the file really holds.
    pub(crate) fn dev_hdr(&mut self, ui: &mut egui::Ui, hdr_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_HDR_SDR), hdr_active))
            .id_salt("sec_hdr")
            .default_open(false)
            , |ui| {
                if ui
                    .checkbox(&mut self.editor.doc_mut().recipe.hdr_edit, tr(lang, K_DEVELOP_HDR_EDIT_MODE))
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_THIS_PHOTOGRAPH_WAS_EDITED_ABOVE_DIFFUSE_WHITE_E,
                    ))
                    .changed()
                {
                    changed = true;
                }
                changed |= self.capability_rows(ui, crate::controls::HDR_HEADROOM);
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, K_DEVELOP_SDR_RENDITION)).weak().small());
                ui.label(
                    egui::RichText::new(tr(lang, K_DEVELOP_RENDERS_ONLY_IN_HDR_MODE))
                        .weak()
                        .small(),
                );
                changed |= self.capability_rows(ui, crate::controls::HDR_SDR);
            });
        changed
    }

    /// 变换 — Lightroom's Transform panel, RENDERED since v1.5.0 F6, plus what
    /// is still carried verbatim beside it.
    ///
    /// R25 B4 drew this section read-only, and said why: pass-through meant no
    /// band, no clamp, no neutral and no idea what a value did to a pixel, and a
    /// slider needs all four. F6 supplies all four — the eight `crs:Perspective*`
    /// keys are owned controls with measured neutrals, and
    /// `render::perspective` is the stage that moves the pixels — so the sliders
    /// are now honest. What remains carried is the ASSUMPTIONS Lightroom's own
    /// Upright solver worked from (its version stamp, and the centre and focal
    /// length it took the photograph to have) and the camera profile's name,
    /// and those keep the old treatment: Adobe's own property spelling, and a
    /// sentence saying we do not interpret them. Its cached ANSWER — the
    /// matrices, their count, its preview flag and its staleness digest — is
    /// not carried at all; see [`autoshade::xmp::PASSTHROUGH_CRS`] for why.
    ///
    /// (The Calibration panel used to share this heading. It renders since
    /// v1.5.0 — `dev_calibration` above — and was never in this map: R25
    /// listed seven `CameraCalibration*` keys Lightroom does not write.)
    pub(crate) fn dev_transform(&mut self, ui: &mut egui::Ui, transform_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_TRANSFORM), transform_active))
            .id_salt("sec_transform")
            .default_open(false)
            , |ui| {
                // Upright is a MODE, so a dropdown and not a slider — and the
                // one control in this panel whose answer may come from either
                // side: Lightroom publishes the matrix its own solver found
                // (`crs:UprightTransform_N`), and when the photo carries none
                // for the chosen mode this engine solves it
                // (`render::perspective::solve_upright`).
                const MODES: [&str; 6] =
                    ["Off", "Auto", "Level", "Vertical", "Full", "Guided"];
                let mut mode = self.editor.doc().recipe.perspective_upright.round().clamp(0.0, 5.0) as usize;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr(lang, K_DEVELOP_UPRIGHT)).small());
                    egui::ComboBox::from_id_salt("upright_mode")
                        .selected_text(tr(lang, MODES[mode]))
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for (i, name) in MODES.iter().enumerate() {
                                ui.selectable_value(&mut mode, i, tr(lang, name));
                            }
                        });
                });
                if mode as f32 != self.editor.doc().recipe.perspective_upright {
                    self.editor.doc_mut().recipe.perspective_upright = mode as f32;
                    changed = true;
                }
                // Which side will answer, said out loud — the difference
                // between "Adobe's numbers to the last digit" and "our own
                // solver, which may differ slightly" is exactly what a
                // photographer comparing the two apps needs to know.
                if mode > 0 {
                    let adobes = self.editor.doc().recipe
                        .upright_transform
                        .get(mode)
                        .is_some_and(|m| *m != [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
                    // Named, in ONE array, for the i18n audit's dynamic-key
                    // registry: a fourth answer added inline would reach the
                    // screen with no zh pair and nothing would say so.
                    const UPRIGHT_NOTES: [&str; 3] = [
                        "Rendering the matrix Lightroom solved for this photo",
                        "Guided needs the guides drawn in Lightroom; only the matrix it wrote can be rendered",
                        "Lightroom solved no matrix for this photo, so AutoShade solves it from the picture's own lines",
                    ];
                    // `upright_note`, not `note`: the i18n audit registers a
                    // dynamic call site by the SPELLING of its argument, and a
                    // bare `note` would have been a pattern wide enough to
                    // accept some future unaudited key somewhere else.
                    let upright_note = if adobes {
                        UPRIGHT_NOTES[0]
                    } else if mode == 5 {
                        UPRIGHT_NOTES[1]
                    } else {
                        UPRIGHT_NOTES[2]
                    };
                    ui.label(egui::RichText::new(tr(lang, upright_note)).weak().small());
                }
                ui.add_space(SPACE_SM);
                // One capability table: bands and resets (Rotate ±10, scale
                // 50..150 resetting to 100) come from `logic::capability`.
                changed |= self.capability_rows(ui, crate::controls::TRANSFORM);
                if ui
                    .checkbox(&mut self.editor.doc_mut().recipe.crop_constrain_to_warp, tr(lang, K_DEVELOP_CONSTRAIN_CROP))
                    .on_hover_text(tr(lang, K_DEVELOP_SHRINK_THE_CROP_UNTIL_IT_LIES_INSIDE_THE_WARPED))
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(SPACE_SM);
                explainer(ui, lang, tr(lang, K_DEVELOP_ONE_PROJECTIVE_MAP_AFTER_THE_LENS_CORRECTION_AND));
                // v1.5.0 F7 — WHICH rendering this photograph starts from.
                // Read-only here on purpose: the profile is a file on the
                // photographer's machine and the Look is Adobe's own creative
                // block, so the panel's job is to say what is being used, not
                // to offer a picker for something the sidecar decides.
                let look = self.editor.doc().recipe.look.clone();
                if !self.editor.doc().recipe.camera_profile.is_empty() || look.is_some() {
                    ui.add_space(SPACE_MD);
                    let pair = |ui: &mut egui::Ui, label: String, value: &str| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(label).small());
                            ui.label(egui::RichText::new(value).small().strong());
                        });
                    };
                    if !self.editor.doc().recipe.camera_profile.is_empty() {
                        pair(
                            ui,
                            tr(lang, K_DEVELOP_CAMERA_PROFILE).to_string(),
                            &self.editor.doc().recipe.camera_profile,
                        );
                    }
                    if let Some(look) = look.as_ref().filter(|l| !l.name.is_empty()) {
                        pair(ui, tr(lang, K_DEVELOP_CREATIVE_PROFILE).to_string(), &look.name);
                        // Named, not hidden: a Look is colour data (v1.17.0) —
                        // its creative colour table renders when Camera Raw's
                        // install carries it (`dcp::find_look_table`,
                        // answered once per process) and the Look is at full
                        // amount, the render's own two conditions; its baked
                        // tone curve and sliders never do. A panel that showed
                        // the name without saying which half arrived would be
                        // claiming the whole profile.
                        let line = if look.table.is_empty() {
                            tr(lang, K_DEVELOP_ITS_BAKED_TONE_CURVE_AND_SLIDERS_DO_NOT_RENDER)
                        } else if (look.amount - 1.0).abs() <= 1e-3
                            && autoshade::dcp::find_look_table(&look.table).is_ok()
                        {
                            tr(lang, K_DEVELOP_ITS_CREATIVE_COLOUR_TABLE_RENDERS_ITS_BAKED_TONE)
                        } else {
                            tr(lang, K_DEVELOP_NEITHER_ITS_CREATIVE_COLOUR_TABLE_NOR_ITS_BAKED)
                        };
                        ui.label(egui::RichText::new(line).weak().small());
                    }
                }
                if self.editor.doc().recipe.passthrough.is_empty() {
                    return;
                }
                ui.add_space(SPACE_MD);
                // In PASSTHROUGH_CRS order (Adobe's own grouping), which is
                // also the order the writer emits — one order everywhere, so
                // the panel and the sidecar read the same way.
                let rows = |ui: &mut egui::Ui, caption: Option<&str>, keys: &[&str]| {
                    let present: Vec<(&str, &str)> = keys
                        .iter()
                        .filter_map(|k| {
                            self.editor.doc().recipe.passthrough.get(*k).map(|v| (*k, v.as_str()))
                        })
                        .collect();
                    if present.is_empty() {
                        return;
                    }
                    if let Some(caption) = caption {
                        ui.label(egui::RichText::new(caption).weak().small());
                    }
                    for (key, value) in present {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(format!("crs:{key}")).small());
                            ui.label(egui::RichText::new(value).small().strong());
                        });
                    }
                };
                // Every carried key, in the block's own order. The partition
                // this used to make had two groups because the profile NAME was
                // carried alongside the solver's bookkeeping; F7 made the name
                // an owned control shown above, so what is left is one block
                // with one caption.
                rows(ui, Some(tr(lang, K_DEVELOP_UPRIGHT_SOLVER_BOOKKEEPING)), &autoshade::xmp::PASSTHROUGH_CRS);
                ui.add_space(SPACE_SM);
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        K_DEVELOP_CARRIED_THROUGH_TO_THE_SIDECAR_UNCHANGED_AUTOSHA,
                    ))
                    .weak()
                    .small(),
                );
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_lens(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // --- 镜头校正: in-camera profile + manual corrections -----------------
        ui.add_space(SPACE_MD);
        // Two as-opened states are neutral here: the camera's available
        // components on, or all off with the sidecar's DisabledInSidecar stamp.
        // Neither lights the dot; hand-made all-off and other moves away do.
        // This header rule leaves is_noop's saved-develop precedence and
        // unsaved badge alone. The `lens` / `lens_effects` families still cover
        // manual sliders, including the two profile strengths; `DOT_EXEMPT`
        // keeps midpoints and hue windows quiet with their amounts at rest.
        let lens_active = CONTROL_FAMILIES
            .iter()
            .filter(|f| f.name == "lens" || f.name == "lens_effects")
            .any(|f| family_is_active(f, &self.editor.doc().recipe))
            || !self.editor.doc().recipe.lens_profile.is_as_opened();
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_LENS), lens_active))
            .id_salt("sec_lens")
            .default_open(false)
            , |ui| {
                // In-camera profile corrections (lensmeta): the manufacturer's
                // exact per-shot data read from the RAW itself — no lens
                // database. Toggling a component ON whose data a legacy
                // restore dropped re-fills it from this photo's metadata.
                let photo = self.workspace.open().photo_lens.clone();
                let lp = &mut self.editor.doc_mut().recipe.lens_profile;
                let has_v = !lp.vignette.is_empty() || !photo.vignette.is_empty();
                let has_d = !lp.distortion.is_empty() || !photo.distortion.is_empty();
                let has_c = (!lp.ca_r.is_empty() || !photo.ca_r.is_empty())
                    && (!lp.ca_b.is_empty() || !photo.ca_b.is_empty());
                if has_v || has_d || has_c {
                    ui.label(tr(lang, K_DEVELOP_PROFILE_CORRECTIONS_FROM_CAMERA_METADATA));
                    let mut pch = false;
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(has_v, egui::Checkbox::new(&mut lp.vignette_on, tr(lang, K_DEVELOP_VIGNETTING)))
                            .on_hover_text(tr(lang, K_DEVELOP_THE_CAMERA_S_OWN_FALLOFF_MAP_FOR_THIS_SHOT_APPLI))
                            .changed()
                        {
                            if lp.vignette_on && lp.vignette.is_empty() {
                                lp.vignette = photo.vignette.clone();
                            }
                            pch = true;
                        }
                        if ui
                            .add_enabled(has_d, egui::Checkbox::new(&mut lp.distortion_on, tr(lang, K_DEVELOP_DISTORTION)))
                            .on_hover_text(tr(lang, K_DEVELOP_THE_CAMERA_S_OWN_GEOMETRIC_CORRECTION_MASKS_AND))
                            .changed()
                        {
                            if lp.distortion_on && lp.distortion.is_empty() {
                                lp.distortion = photo.distortion.clone();
                            }
                            pch = true;
                        }
                        if ui
                            .add_enabled(has_c, egui::Checkbox::new(&mut lp.ca_on, tr(lang, K_DEVELOP_CHROMATIC_ABERRATION)))
                            .on_hover_text(tr(lang, K_DEVELOP_PER_CHANNEL_RADIUS_CORRECTION_REMOVES_RED_BLUE_C))
                            .changed()
                        {
                            if lp.ca_on && (lp.ca_r.is_empty() || lp.ca_b.is_empty()) {
                                // Refill only the MISSING channel(s): `has_c`
                                // accepts the union of saved + photo data, so
                                // replacing BOTH from the photo could erase a
                                // saved channel the photo can't supply and
                                // leave the pair incomplete (CA that renders
                                // nothing — geometry_active needs both).
                                if lp.ca_r.is_empty() {
                                    lp.ca_r = photo.ca_r.clone();
                                }
                                if lp.ca_b.is_empty() {
                                    lp.ca_b = photo.ca_b.clone();
                                }
                            }
                            pch = true;
                        }
                    });
                    if pch {
                        self.workspace.overlay_mut().overlay_stale = true;
                        changed = true;
                    }
                    ui.add_space(SPACE_MD);
                } else if self.editor.photo().src_path.as_deref().is_some_and(autoshade::decode::is_raw) {
                    ui.label(
                        egui::RichText::new(tr(lang, K_DEVELOP_NO_IN_CAMERA_LENS_CORRECTION_DATA_IN_THIS_FILE))
                            .weak()
                            .small(),
                    );
                    ui.add_space(SPACE_MD);
                }
                // 「Lens vignetting」, not 「Vignette」 (R25 B2): the Effects
                // section above now carries Lightroom's POST-CROP vignette,
                // and two sections labelled 暗角 with different operators
                // behind them is a name collision, not a shorthand. This one
                // is a falloff CORRECTION in linear light before any tonal
                // work; that one is a creative darkening after the crop.
                // Manual vignetting / midpoint / distortion: one capability
                // table (bands and resets from `logic::capability`).
                changed |= self.capability_rows(ui, crate::controls::LENS_MANUAL);
                // --- manual lateral CA (R25 B3) -------------------------
                // RENDERED, so NO disclosure line: these two fold onto the
                // very per-channel radius LUT the profile CA above uses
                // (`render::geometry_profile`), and the preview, the export
                // and the sidecar all agree. The pair sits under the
                // distortion slider because it is the same kind of thing —
                // an optical defect, not a mood.
                // The PROFILE's own two strengths (v1.5.0). They belong with
                // the profile's tick boxes above, not with the manual sliders
                // below: they scale what the profile does, and at 0 they are
                // that component's off switch — which is why they are drawn
                // even when this photo has no profile data, so a recipe
                // imported from Lightroom shows what it carries.
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, K_DEVELOP_PROFILE_CORRECTION_STRENGTH)).weak().small());
                changed |= self.capability_rows(ui, crate::controls::LENS_PROFILE);
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, K_DEVELOP_CHROMATIC_ABERRATION_MANUAL)).weak().small());
                changed |= self.capability_rows(ui, crate::controls::LENS_CA);
                // …and the rest of the same panel: Adobe's auto switch and the
                // whole de-fringe block. Carried and unrendered until v1.5.0;
                // the switch is an INSTRUCTION, so its tooltip says what the
                // instruction does rather than what the number means.
                if ui
                    .checkbox(&mut self.editor.doc_mut().recipe.auto_lateral_ca, tr(lang, K_DEVELOP_AUTO_LATERAL_CA))
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_MEASURES_THIS_FRAME_S_OWN_RED_BLUE_MISALIGNMENT,
                    ))
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, K_DEVELOP_DEFRINGE)).weak().small());
                // Adobe's bands and neutrals: amounts rest at 0, the hue
                // windows at 30/70 and 40/60 (the one block whose neutral is
                // not zero — the row resets carry them).
                changed |= self.capability_rows(ui, crate::controls::LENS_DEFRINGE);
                ui.add_space(SPACE_SM);
                explainer(ui, lang, tr(lang, K_DEVELOP_VIGNETTE_POSITIVE_BRIGHTENS_THE_CORNERS_COMPENSA));
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_curves(&mut self, ui: &mut egui::Ui, curves_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        ui.add_space(SPACE_MD);
        // --- 曲线: master + RGB tone curves (engine + XMP already apply them,
        // this is purely the editing surface — Lightroom's panel order) --------
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_CURVES), curves_active))
            .id_salt("sec_curves")
            .default_open(false)
            , |ui| {
                changed |= self.curve_editor(ui);
                changed |= parametric_curve(ui, lang, &mut self.editor.doc_mut().recipe);
            });
        changed
    }
    /// What one Analyze run wrote — the verdict, the rationale sentence and
    /// the deep-thinking working — drawn by both the Analysis fold's bounded
    /// box and the full-text window, so the two can never disagree.
    pub(crate) fn analysis_output(&self, ui: &mut egui::Ui) {
        let lang = self.lang;
        if let Some((d, reasons)) = &self.editor.analysis().verdict {
            // Accept reads calm; anything else (Revise/Reject)
            // gets the warn colour so it can't be skimmed past.
            // Matched on the TYPED decision, never its rendered
            // spelling — the old `starts_with("Accept")` sniff
            // would have flipped every verdict to warn the moment
            // the word was translated.
            let col = if matches!(d, autoshade::advisor::Decision::Accept) {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().warn_fg_color
            };
            let text = trf(
                lang,
                K_AI_TEXT,
                &[
                    ("decision", tr(lang, autoshade::advisor::decision_key(d))),
                    ("reasons", &reasons.join("; ")),
                ],
            );
            ui.label(egui::RichText::new(text).color(col));
        }
        // The deterministic tail renders LOCALIZED when its typed
        // notes ride along and still match the string's suffix
        // (L12#2B); any mismatch — a truncation, a disk-restored
        // develop with no notes — shows the raw English instead
        // (silent-English fallback, user decision 2026-08-11).
        //
        // The typed tail splits in two at draw time (user feedback
        // 2026-09-11): the deterministic notes stay in the sentence, the
        // deep-thinking WORKING — the notes only a thinking analysis
        // produces, see `is_working_note` — draws in a box of its own
        // under it. The persisted string is untouched (one suffix, five
        // surfaces); the split is a display decision, and on the fallback
        // path the working is still inside `shown`, so no box is drawn.
        let (working, tail): (Vec<_>, Vec<_>) =
            self.editor.analysis().rationale_notes.iter().partition(|n| is_working_note(n.key));
        let localized = (!self.editor.analysis().rationale_notes.is_empty())
            .then(|| {
                let det: String = self.editor.analysis().rationale_notes
                    .iter()
                    .map(autoshade::rationale::render_one)
                    .collect();
                self.editor.analysis().rationale.strip_suffix(det.as_str()).map(|prose| {
                    let mut s = String::from(prose);
                    for n in &tail {
                        s.push_str(&trf(lang, n.key, &note_args(n)));
                    }
                    s
                })
            })
            .flatten();
        let shown = localized.as_deref().unwrap_or(&self.editor.analysis().rationale);
        if !shown.is_empty() {
            ui.label(
                egui::RichText::new(format!("“{shown}”"))
                    .italics()
                    .weak(),
            );
        }
        if localized.is_some() && !working.is_empty() {
            thinking_notes(self.lang, ui, &working);
        }
    }
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
