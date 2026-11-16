//! What a panel may touch: a projection of the aggregates, never the app.
//!
//! A panel draws with the edit (the widgets that own a recipe vector — the
//! curve editor — mutate it), the view state, the language and the test seams.
//! It cannot reach the workers, the services or another panel's methods: the
//! type simply does not carry them.

use crate::*;
use crate::sliders::*;
use crate::panels::grade_wheel::{grade_wheel, GRADE_VIEWS};
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
    pub(crate) fn dev_export(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
    
        // --- 导出设置 (UX batch): moved out of the toolbar — touched once per
        // delivery, these are Export-dialog contents, not toolbar chrome. The
        // toolbar keeps the ACTIONS; their hover echoes this section's state.
        ui.add_space(SPACE_MD);
        // Delivery preferences have no per-photo neutral, so this header has
        // no adjustment dot. The toolbar's Export hover carries the summary.
        card_fold(ui, egui::CollapsingHeader::new(tr(lang, K_APP_EXPORT))
            .id_salt("sec_export")
            .default_open(false)
            , |ui| {
                // WHERE first (R22-7): the delivery target is now a setting
                // rather than a property of which toolbar button was pressed,
                // and it is the one whose wrong value sends the file somewhere
                // the user has to hunt for — so it leads the section instead of
                // hiding under the encoder dials.
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_DEVELOP_DESTINATION));
                    egui::ComboBox::from_id_salt("exp_dest")
                        .selected_text(tr(lang, self.workspace.exp().dest.label()))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for d in ExportDest::ALL {
                                ui.selectable_value(&mut self.workspace.exp_mut().dest, d, tr(lang, d.label()));
                            }
                        });
                });
                // The RESOLVED target, spelled out: "./out" is relative to
                // whichever directory the app was launched from, and "last used
                // folder" says nothing about which folder that is.
                ui.label(
                    egui::RichText::new(match self.workspace.export_dest_dir() {
                        Some(d) => abs_display(&d),
                        None => tr(lang, K_DEVELOP_A_SAVE_DIALOG_OPENS_ON_EVERY_EXPORT).to_string(),
                    })
                    .weak()
                    .small(),
                );
                // One setting per row: two label+combo pairs in a non-wrapping
                // horizontal overflowed the 320px panel in Chinese.
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_DEVELOP_FORMAT));
                    egui::ComboBox::from_id_salt("save_fmt")
                        .selected_text(tr(lang, self.workspace.exp().format.label()))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for f in ExportFormat::ALL {
                                ui.selectable_value(&mut self.workspace.exp_mut().format, f, tr(lang, f.label()));
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_DEVELOP_LONG_EDGE));
                    egui::ComboBox::from_id_salt("exp_long_edge")
                        .selected_text(if self.workspace.exp().long_edge == 0 {
                            tr(lang, K_EXPORT_ORIGINAL_SIZE).to_string()
                        } else {
                            format!("{} px", self.workspace.exp().long_edge)
                        })
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.workspace.exp_mut().long_edge, 0, tr(lang, K_EXPORT_ORIGINAL_SIZE));
                            for px in [1600u32, 2048, 2560, 3840, 5120] {
                                ui.selectable_value(&mut self.workspace.exp_mut().long_edge, px, format!("{px} px"));
                            }
                        });
                });
                slider(ui, lang, tr(lang, K_EXPORT_OUTPUT_SHARPENING), &mut self.workspace.exp_mut().sharpen, 0.0, 100.0, 0.0);
                // Always allocated, merely disabled for TIFF — the old
                // appear/disappear reflowed every control to its right.
                ui.add_enabled_ui(self.workspace.exp().format == ExportFormat::Jpeg, |ui| {
                    slider(ui, lang, tr(lang, K_DEVELOP_JPEG_QUALITY), &mut self.workspace.exp_mut().quality, 60.0, 100.0, 95.0);
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, K_DEVELOP_COLOUR_SPACE));
                    egui::ComboBox::from_id_salt("exp_space")
                        .selected_text(tr(lang, EXPORT_SPACES[(self.workspace.exp().space as usize).min(2)]))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for (i, name) in EXPORT_SPACES.iter().enumerate() {
                                ui.selectable_value(&mut self.workspace.exp_mut().space, i as u8, tr(lang, name));
                            }
                        });
                });
                // 「on export」 completes the pair the 🤖 prefix alone could not
                // separate (R22 #16): this checkbox and the Detail section's
                // 「🤖 AI Denoise now」 are two TIMINGS of one denoiser — this one
                // runs inside every full-resolution delivery and touches no
                // pixel until then, that one bakes a clean base into the current
                // variant immediately. Same-named twins in two panels made the
                // difference invisible until an export took minutes.
                ui.checkbox(&mut self.workspace.exp_mut().save_denoise, tr(lang, K_DEVELOP_AI_DENOISE_ON_EXPORT)).on_hover_text(
                    ai_xref(lang, tr(lang, K_DEVELOP_AI_DENOISE_BEFORE_DEVELOPING_AT_THE_EXPORT_DENOI)),
                );
                // The checkbox's OWN dial, directly under it (user decision
                // 2026-09-12): the Detail fold's 「AI Denoise now」 has its own
                // and neither reaches the other. Always allocated, merely
                // disabled while the checkbox is off — the appear/disappear
                // reflow rule the JPEG quality slider above follows.
                let denoise_raw = self.editor.denoise_uses_raw();
                ui.add_enabled_ui(self.workspace.exp().save_denoise, |ui| {
                    slider_pct_hinted(
                        ui,
                        lang,
                        tr(lang, K_DEVELOP_EXPORT_DENOISE_STRENGTH),
                        if denoise_raw { &mut self.workspace.exp_mut().save_denoise_strength } else { &mut self.workspace.exp_mut().baked_save_denoise_strength },
                        1.0,
                        if denoise_raw { autoshade::denoise::DEFAULT_STRENGTH_RAW } else { autoshade::denoise::DEFAULT_STRENGTH },
                        tr(lang,
                            K_DEVELOP_EXPORT_AI_DENOISE_STRENGTH_INDEPENDENT_OF_THE_DE,
                        ),
                    );
                });
                explainer(ui, lang, tr(lang, K_DEVELOP_APPLIED_BY_EXPORT_IN_THE_TOOLBAR_CTRL_SHIFT_E_OR));
                // --- SF8-A: hand the Lightroom sidecar over ----------------
                // The XMP projection has always been written INSIDE the hashed
                // develop folder, which is the right home for state — but the
                // one thing users do with an XMP is give it to Lightroom, and
                // Lightroom only looks for it beside the photo. That left the
                // hand-off as "browse into %LOCALAPPDATA% yourself".
                ui.add_space(SPACE_LG);
                let raw = self.editor.can_export_xmp_beside();
                let label = if self.workspace.exp().xmp_beside_confirm {
                    tr(lang, K_DEVELOP_OVERWRITE_THE_XMP_ALREADY_THERE)
                } else {
                    tr(lang, K_DEVELOP_EXPORT_XMP_BESIDE_THE_PHOTO)
                };
                let hint = if !raw {
                    // The disabled REASON, not a repeat of the label: only a
                    // camera RAW has the sidecar convention (store::
                    // lightroom_sidecar and pipeline::write_xmp draw the same
                    // line), so a baked PNG/TIFF's neighbouring .xmp belongs to
                    // some other program and must not be written over.
                    tr(lang, K_DEVELOP_RAW_ONLY_A_BAKED_PNG_TIFF_HAS_NO_LIGHTROOM_SIDEC)
                } else if self.workspace.exp().xmp_beside_confirm {
                    tr(lang, K_DEVELOP_A_XMP_ALREADY_SITS_BESIDE_THIS_PHOTO_LIGHTROOM_S)
                } else {
                    tr(lang, K_DEVELOP_COPY_THIS_PHOTO_S_STORED_LIGHTROOM_ACR_SIDECAR_I)
                };
                if action_row(ui, raw && !self.workspace.busy(), label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.editor.export_xmp_beside(&mut self.workspace, self.lang);
                }
            });
        false
    }
    /// One capability table: the renderer draws each control through its own
    /// implementation and reports a data change (id + value); the registry
    /// applies it. The panel does not know a control by name.
    pub(crate) fn dev_tone_wb(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // DERIVED from the registry like the five section dots in
        // `develop_panel` (R25 P8 closed the last two hand-written ones). This
        // section holds BOTH the tone family and the white-balance one — the
        // panel groups them, the registry does not — so its ● is the OR, the
        // same shape the Detail and Lens sections already use for their own
        // pairs.
        let tone_active = CONTROL_FAMILIES
            .iter()
            .filter(|f| f.name == "tone" || f.name == "white_balance")
            .any(|f| family_is_active(f, &self.editor.doc().recipe));
    
        // #14b: the FIRST of the panel's group captions (its air is its own:
        // `headings::group_caption`). The two existing group
        // fences (before Detail, before Local Masks) marked boundaries without
        // ever saying what they divided; the five groups the panel actually has
        // are AI (its own panel above) → tone & colour → detail & lens → local
        // & pixel → versions & export. Captions only: no section moved, no
        // fence rhythm changed.
        group_caption(ui, tr(lang, K_DEVELOP_TONE_COLOUR));
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_TONE_WB), tone_active))
            .id_salt("sec_tone")
            .default_open(true)
            , |ui| {
                // R38: wrapped, so the eyedropper drops under the checkbox when
                // the row is short — the pair is 351 px at the 320 px default,
                // and a plain `horizontal` never wraps: it widened the
                // auto-fitting panel by 64 px, the widest row in this panel.
                ui.horizontal_wrapped(|ui| {
                    // One cell of the grid for the eyedropper, measured at the
                    // row's start (2026-09-27); it wraps under the checkbox.
                    let cell = columns(ui, 2);
                    // A nonzero tint IS a WB edit (recipes saved before the
                    // uncheck-zeroes-tint fix can carry one with no Temp) —
                    // showing "off = as-shot" over an active tint, with its
                    // slider disabled, was a lie with no way out.
                    let mut custom_wb =
                        self.editor.doc().recipe.temperature_k.is_some() || self.editor.doc().recipe.tint != 0.0;
                    if ui.checkbox(&mut custom_wb, tr(lang, K_DEVELOP_CUSTOM_WHITE_BALANCE_OFF_AS_SHOT)).changed() {
                        // Arm AT the as-shot anchor when stamped: checking
                        // the box must not shift the image — the slider then
                        // starts from the camera's real Kelvin, not 5500.
                        self.editor.doc_mut().recipe.temperature_k =
                            if custom_wb { Some(self.editor.doc().recipe.as_shot_k.unwrap_or(5500.0)) } else { None };
                        if !custom_wb {
                            // "As-shot" is the WHOLE white balance: a custom
                            // Tint surviving the un-check kept a WB edit
                            // active behind a label claiming there was none.
                            self.editor.doc_mut().recipe.tint = 0.0;
                        }
                        changed = true;
                    }
                    let label = if self.workspace.tools().wb_picking { tr(lang, K_DEVELOP_CLICK_IN_IMAGE) } else { tr(lang, K_DEVELOP_EYEDROPPER) };
                    if action_in(ui, cell, true, label)
                        .on_hover_text(tr(lang,
                            K_DEVELOP_CLICK_A_SPOT_IN_THE_IMAGE_THAT_SHOULD_BE_NEUTRAL,
                        ))
                        .clicked()
                    {
                        let on = !self.workspace.tools().wb_picking;
                        self.disarm_tools();
                        self.workspace.tools_mut().wb_picking = on;
                        if on {
                            self.workspace.set_status(tr(lang, K_APP_WB_EYEDROPPER_CLICK_A_SPOT_THAT_SHOULD_BE_NEUTRA).into());
                        }
                    }
                });
                if let (Some(k), Some(t)) = (self.editor.doc().recipe.as_shot_k, self.editor.doc().recipe.as_shot_tint) {
                    // The camera's own WB in absolute terms — the reference
                    // the Temp slider is anchored on for this photo. BOTH
                    // halves required: the era pin (anchor 5500, tint None)
                    // knows the anchor but NOT the camera, and showing it as
                    // "as shot" would be a false claim.
                    ui.weak(trf(
                        lang,
                        K_DEVELOP_AS_SHOT_K_TINT,
                        &[("k", &format!("{k:.0}")), ("t", &format!("{t:+.0}"))],
                    ));
                }
                // Double-click reset lands on the as-shot Kelvin when stamped
                // (the honest "no shift" position), 5500 for legacy recipes.
                let anchor_k = self.editor.doc().recipe.as_shot_k.unwrap_or(5500.0);
                if let Some(mut k) = self.editor.doc().recipe.temperature_k
                    && slider_log(ui, lang, tr(lang, K_DEVELOP_TEMP_K), &mut k, 2000.0, 50000.0, anchor_k)
                {
                    self.editor.doc_mut().recipe.temperature_k = Some(k);
                    changed = true;
                }
                let custom_wb = self.editor.doc().recipe.temperature_k.is_some() || self.editor.doc().recipe.tint != 0.0;
                // Tint is HALF of the white balance: editable only under
                // Custom WB, or a tint move silently contradicted the
                // checkbox's "off = as-shot" promise. A legacy tint-only
                // recipe counts as Custom (see the checkbox above) so its
                // slider stays reachable.
                ui.add_enabled_ui(custom_wb, |ui| {
                    changed |= self.capability_rows(ui, crate::controls::WB_TINT);
                });
                // The tone band is a capability table row each: the panel owns
                // the fold and the white balance controls above; the fields,
                // bands and resets live in `logic::capability::TONE`.
                changed |= self.capability_rows(ui, crate::controls::TONE);
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_crop(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // --- 裁剪 + 拉直: recipe.crop / straighten_deg (export + XMP paths) ---
        ui.add_space(SPACE_MD);
        // The `framing` family IS this section — crop rectangle plus
        // straighten angle — so the dot is read straight off the registry
        // (R25 P8, the same derivation as every other section's).
        let crop_active = CONTROL_FAMILIES
            .iter()
            .filter(|f| f.name == "framing")
            .any(|f| family_is_active(f, &self.editor.doc().recipe));
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_CROP), crop_active))
            .id_salt("sec_crop")
            .default_open(false)
            , |ui| {
                // The two verbs share the row as equal cells and the aspect
                // preset sits under them on a captioned row of its own
                // (2026-09-27): a 70 px combo between two own-width verbs was
                // the one three-item row in this panel.
                ui.horizontal(|ui| {
                    let cell = columns(ui, 2);
                    // ✓ (geometric) — same finish-glyph as 「✓ Apply」; the
                    // emoji ✅ was the odd one out of the check family.
                    let label = if self.workspace.crop().mode { tr(lang, K_DEVELOP_DONE) } else { tr(lang, K_DEVELOP_ENTER_CROP) };
                    if action_in(ui, cell, true, label).clicked() {
                        let on = !self.workspace.crop().mode;
                        self.disarm_tools();
                        self.editor.set_crop_mode(&mut self.workspace, on);
                    }
                    if action_in(ui, cell, true, tr(lang, K_DEVELOP_CLEAR_CROP)).clicked()
                        && self.editor.doc_mut().recipe.crop.take().is_some()
                    {
                        // Through the panel's own change path: clamp + dirty,
                        // so the crop-restricted histogram/clipping refresh.
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr(lang, K_DEVELOP_ASPECT)).weak().small());
                    let prev_aspect = self.workspace.crop().aspect;
                    egui::ComboBox::from_id_salt("crop_aspect")
                        .selected_text(tr(lang, CROP_ASPECTS[self.workspace.crop().aspect].0))
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for (i, (name, _)) in CROP_ASPECTS.iter().enumerate() {
                                ui.selectable_value(&mut self.workspace.crop_mut().aspect, i, tr(lang, name));
                            }
                        });
                    // LR applies a picked preset to the box IMMEDIATELY — the
                    // combo used to only arm the ratio for the NEXT handle
                    // drag, so the UI said 4:5 while preview/export kept the
                    // old shape (D13). Consumed in handle_crop, where the
                    // view dims live — and armed ONLY while the crop tool is
                    // up: with the tool away the flag would survive until
                    // some later session's crop entry and rewrite the box as
                    // a surprise (Codex batch-38); tool-off keeps the old
                    // arm-for-the-next-drag meaning.
                    if self.workspace.crop().aspect != prev_aspect && self.workspace.crop().mode {
                        self.workspace.crop_mut().aspect_pending = true;
                    }
                });
                // Straighten: rotate + auto-crop (engine rotate_straighten);
                // the preview shows exactly the export geometry. Fine class:
                // levelling needs 0.1°, not the wide-range integer snap.
                changed |= self.capability_rows(ui, crate::controls::CROP);
                explainer(ui, lang, tr(lang, K_DEVELOP_ONCE_IN_R_DRAG_CORNER_EDGE_HANDLES_TO_RESIZE_DRA));
            });
        changed
    }
    /// Recipe replaced wholesale (undo/redo/Reset/version load/AI apply): the
    /// derived display state must follow, or the panel keeps showing an AI
    /// rationale / verdict that describes a recipe no longer on screen — and
    /// an ARMED index-carrying tool (range sampler, ↻ Redraw) would fire into
    /// whatever mask now happens to sit at its remembered index.
    /// One tool at a time on the canvas — the SINGLE owner of the disarm
    /// list. Ten arming sites used to hand-copy these assignments, and the
    /// comments at two of them record real bugs born from copies drifting;
    /// every arming site now calls this before setting its own flag.
    /// Transient gesture anchors die with the tools: a stale place_start or
    /// crop_drag used to hijack the next drag. clone_src (the sampled source
    /// pin) deliberately survives — samples stay for resuming, like paint.
    pub(crate) fn disarm_tools(&mut self) {
        self.editor.set_crop_mode(self.workspace, false);
        self.workspace.masks_mut().paint_mode = false;
        self.workspace.tools_mut().clone_mode = false;
        self.workspace.tools_mut().wb_picking = false;
        self.workspace.tools_mut().range_picking = None;
        self.workspace.tools_mut().point_color_picking = false;
        self.workspace.tools_mut().lens_focus_picking = false;
        self.workspace.masks_mut().placing_mask = None;
        self.workspace.masks_mut().place_start = None;
        self.workspace.crop_mut().drag = None;
        self.workspace.masks_mut().mask_drag = None;
        self.workspace.masks_mut().paint_last = None;
        self.workspace.end_mask_brush();
        // A preset change armed for the crop tool must die with it — the
        // flag survived Esc / Done / a hold-B detour and rewrote the box as
        // a surprise on the next crop entry (CX5-4).
        self.workspace.crop_mut().aspect_pending = false;
    }
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_detail(&mut self, ui: &mut egui::Ui, detail_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
    
        // Look ends here — detail, then geometry (lens BEFORE crop: the lens
        // profile / manual distortion redefine the frame the crop sits in).
        // Group boundary: the group head's own air and gold caption (2026-10-01:
        // the full-width hairline that used to fence each group is gone).
        group_caption(ui, tr(lang, K_DEVELOP_DETAIL_LENS)); // #14b, group 2 of 4 here
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_DETAIL), detail_active))
            .id_salt("sec_detail")
            .default_open(false)
            , |ui| {
                {
                    // All eleven controls render since v1.5.0
                    // (`render/detail.rs`), so none carries the "not rendered
                    // here" line any more. The six COMPANIONS (the amount since
                    // v1.6.0 — Lightroom's 40 on a RAW, none on a baked raster —
                    // radius, the three Details, Smoothness) show the value the
                    // engine renders — Lightroom's default while the recipe
                    // holds none — and reset to it (`companion_slider_edit`,
                    // `sharpening_slider`).
                    let raw_source = self.editor.source_is_raw();
                    changed |= sharpening_slider(ui, lang, tr(lang, K_DEVELOP_SHARPENING), &mut self.editor.doc_mut().recipe, raw_source);
                    // The rows below the amount are one capability table each;
                    // their bands, feels and companion defaults live in
                    // `logic::capability` (the radius is a Fine companion: the
                    // sidecar writes one decimal, so the track shows one).
                    changed |= self.capability_rows(ui, crate::controls::SHARPEN);
                    ui.add_space(SPACE_SM);
                    changed |= self.capability_rows(ui, crate::controls::NOISE_LUMA);
                    ui.add_space(SPACE_SM);
                    changed |= self.capability_rows(ui, crate::controls::NOISE_COLOR);
                }
                let denoise_raw = self.editor.denoise_uses_raw();
                // AI denoise as an ACTIVE op: run now, see it on canvas —
                // export-time denoise (the Export section toggle) stays for
                // batch/full-res workflows, but nobody should have to export
                // to find out what the denoiser does.
                ui.add_space(SPACE_SM);
                // The verb's OWN dial, directly above it (user decision
                // 2026-09-12: a control belongs to the fold whose function it
                // serves; the Export fold's 「on export」 has a dial of its
                // own, and neither reaches the other). Same helper as the
                // reverse-fit's: 0..100 track, double-click reset, hover nudge.
                slider_pct_hinted(
                    ui,
                    lang,
                    tr(lang, K_DEVELOP_AI_DENOISE_STRENGTH),
                    if denoise_raw { &mut self.workspace.exp_mut().denoise_strength } else { &mut self.workspace.exp_mut().baked_denoise_strength },
                    1.0,
                    if denoise_raw { autoshade::denoise::DEFAULT_STRENGTH_RAW } else { autoshade::denoise::DEFAULT_STRENGTH },
                    tr(lang,
                        K_DEVELOP_DETAIL_AI_DENOISE_STRENGTH_INDEPENDENT_OF_THE_EX,
                    ),
                );
                ui.horizontal(|ui| {
                    // CAPABILITY, not just state — the same rule the two
                    // segmentation buttons follow one panel down (R24 batch 2).
                    // Without `python/denoise.py` on disk this click can only
                    // end in `denoise.rs`'s English "denoise sidecar not found
                    // at …" landing in a status line the rest of the window
                    // renders in the user's language, so the button says so up
                    // front instead of spending the click to find out.
                    let has_helper = denoise_helper_available();
                    let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy() && has_helper;
                    let missing = tr(lang,
                        K_DEVELOP_THIS_FEATURE_NEEDS_AUTOSHADE_S_PYTHON_HELPERS_WH,
                    );
                    if action_row(ui, ready, tr(lang, K_DEVELOP_AI_DENOISE_NOW))
                        // 🤖 + the cross-reference line (#4): this verb stays
                        // beside Noise Reduction on purpose, so its tooltip is
                        // where it says the rest of the AI moved to. On the arm
                        // that CANNOT run, the tooltip is about the missing
                        // sidecar and nothing else — same discipline as the
                        // segmentation pair.
                        .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                            K_DEVELOP_DENOISE_THIS_CARD_S_PIXELS_AT_FULL_RESOLUTION_A,
                        )) } else { missing.to_string() })
                        .clicked()
                    {
                        self.act_all(vec![logic::Action::Develop(
                            logic::develop::DevelopAction::AiDenoise,
                        )]);
                    }
                    // No 「Full-res」 checkbox any more (2026-09-15): the
                    // result is a CARD, and a card baked from a ≤2048 px
                    // working copy capped every later export of it at
                    // 2048 px — the denoise always runs on the full frame.
                });
            });
        changed
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
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_COLOR_MIXER_HSL), hsl_active))
            .id_salt("sec_hsl")
            .default_open(false)
            , |ui| {
                changed |= ui
                    .checkbox(&mut self.editor.doc_mut().recipe.convert_to_grayscale, tr(lang, K_DEVELOP_BLACK_WHITE))
                    .changed();
                // What the CANVAS does, not what the checkbox says (v1.5.0 F7):
                // a monochrome creative profile develops the photograph to grey
                // with the photographer's own switch still off, and a panel
                // offering the colour mixer over a grey canvas is a row of
                // sliders that move numbers and no pixels. `render::apply_develop`
                // asks the same question the same way.
                let mono = self.editor.doc().recipe.renders_grayscale();
                // LR's mixer layout: pick ONE property (Hue / Saturation /
                // Luminance) and see all 8 bands at once. The old band picker
                // showed one band at a time — every other band's state was
                // invisible, the opposite of what a mixer is for. The B&W
                // mixer has ONE property (how light each colour turns), so it
                // has no tabs — its caption stands where they do.
                ui.horizontal(|ui| {
                    if mono {
                        ui.label(egui::RichText::new(tr(lang, K_DEVELOP_B_W_MIX)).weak().small());
                    } else {
                        for (i, name) in ["Hue", "Saturation", "Luminance"].iter().enumerate() {
                            if ui.selectable_label(self.workspace.ai().hsl_tab == i, tr(lang, name)).clicked() {
                                self.workspace.ai_mut().hsl_tab = i;
                            }
                        }
                    }
                    // R38: a glyph square — the same reset in every mixer row;
                    // as a text button it ran the HSL tab row to 318 px at the
                    // 320 px default and widened the auto-fitting panel.
                    if glyph(ui, true, "↺").on_hover_text(tr(lang, K_DEVELOP_RESET_ALL)).clicked() {
                        if mono {
                            for b in 0..HSL_BANDS.len() {
                                if let Some(v) = self.editor.doc_mut().recipe.gray_mixer_mut(b) {
                                    *v = 0.0;
                                }
                            }
                        } else {
                            self.editor.doc_mut().recipe.hsl = Hsl::default();
                        }
                        changed = true;
                    }
                });
                let tab = self.workspace.ai().hsl_tab;
                for (b, name) in HSL_BANDS.iter().enumerate() {
                    // ONE row shape for both mixers: same band, same swatch,
                    // same ±100 track — only the number behind it changes.
                    let v = match (mono, tab) {
                        (true, _) => match self.editor.doc_mut().recipe.gray_mixer_mut(b) {
                            Some(v) => v,
                            None => continue,
                        },
                        (false, 0) => &mut self.editor.doc_mut().recipe.hsl.hue[b],
                        (false, 1) => &mut self.editor.doc_mut().recipe.hsl.saturation[b],
                        (false, _) => &mut self.editor.doc_mut().recipe.hsl.luminance[b],
                    };
                    ui.horizontal(|ui| {
                        // Band swatch so rows scan like LR's mixer.
                        let (rect, _) = ui
                            .allocate_exact_size(egui::vec2(10.0, 10.0), egui::Sense::hover());
                        ui.painter().rect_filled(rect, 2.0, HSL_SWATCH[b]);
                        changed |= slider(ui, lang, tr(lang, name), v, -100.0, 100.0, 0.0);
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
    pub(crate) fn point_color_rows(&mut self, ui: &mut egui::Ui) -> bool {
        use autoshade::recipe::MAX_POINT_COLORS;
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_SM);
        // Point Color has no colour to act on in black and white, whichever of
        // the two switches produced it (v1.5.0 F7 — see the mixer above).
        ui.add_enabled_ui(!self.editor.doc().recipe.renders_grayscale(), |ui| {
            ui.horizontal_wrapped(|ui| {
                // One cell of the grid beside the caption (2026-09-27).
                let cell = columns(ui, 2);
                ui.label(egui::RichText::new(tr(lang, K_DEVELOP_POINT_COLOR)).weak().small());
                let label = if self.workspace.tools().point_color_picking {
                    tr(lang, K_DEVELOP_CLICK_IN_IMAGE)
                } else {
                    tr(lang, K_DEVELOP_PICK_A_COLOR)
                };
                let room = self.editor.doc().recipe.point_colors.len() < MAX_POINT_COLORS;
                if action_in(ui, cell, room, label)
                    .on_hover_text(tr(lang,
                        K_DEVELOP_CLICK_A_COLOUR_IN_THE_IMAGE_TO_ADD_A_SWATCH_ITS,
                    ))
                    // The limit says itself where the button that hit it is —
                    // the canvas handler repeats it for the click that arrives
                    // after a paste filled the list behind an armed tool.
                    .on_disabled_hover_text(trf(
                        lang,
                        K_LOGIC_POINT_COLOR_AT_MOST_SWATCHES,
                        &[("n", &MAX_POINT_COLORS.to_string())],
                    ))
                    .clicked()
                {
                    let on = !self.workspace.tools().point_color_picking;
                    self.disarm_tools();
                    self.workspace.tools_mut().point_color_picking = on;
                    if on {
                        self.workspace.set_status(tr(lang, K_DEVELOP_POINT_COLOR_EYEDROPPER_CLICK_THE_COLOUR_TO_ADJUS).into());
                    }
                }
            });
            // Deferred like every other list row's ✕ (R24-5): removing inside
            // the loop would invalidate the iterator it is drawn from.
            let mut remove = None;
            for (i, p) in self.editor.doc_mut().recipe.point_colors.iter_mut().enumerate() {
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
                    ui.label(trf(lang, K_DEVELOP_SWATCH, &[("n", &(i + 1).to_string())]));
                    // 🗑, the vocabulary's DELETE-a-list-row glyph (masks,
                    // versions) — ✕ is "clear / close" and says the wrong thing
                    // about a swatch that is gone for good.
                    if glyph(ui, true, "🗑").on_hover_text(tr(lang, K_DEVELOP_REMOVE_SWATCH)).clicked() {
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
                    if slider(ui, lang, tr(lang, name), &mut shown, -100.0, 100.0, 0.0) {
                        *shift = shown / 100.0;
                        changed = true;
                    }
                }
                let mut range = p.range_amount;
                if slider_pct(ui, lang, tr(lang, K_DEVELOP_RANGE), &mut range, 1.0, 0.5) {
                    p.set_range(range);
                    changed = true;
                }
            }
            if let Some(i) = remove {
                self.editor.doc_mut().recipe.point_colors.remove(i);
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
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_COLOR_GRADING), grade_active))
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
                        if toggle_in(ui, egui::vec2(w, row_h(ui)), self.workspace.ai().grade_region == i, tr(lang, name)).clicked() {
                            self.workspace.ai_mut().grade_region = i;
                        }
                    }
                    // R38: a glyph square — the same reset in every mixer row.
                    if glyph(ui, true, "↺").on_hover_text(tr(lang, K_DEVELOP_RESET_ALL)).clicked() {
                        self.editor.doc_mut().recipe.color_grade = ColorGrade::default();
                        changed = true;
                    }
                });
                ui.add_space(SPACE_SM);
                let cg = &mut self.editor.doc_mut().recipe.color_grade;
                // One wheel with its luminance slider in a column `width` wide.
                let wheel = |ui: &mut egui::Ui, title: &str, width: f32, h: &mut f32, s: &mut f32, l: &mut f32| {
                    let mut c = false;
                    ui.vertical(|ui| {
                        ui.set_width(width);
                        c |= grade_wheel(ui, lang, title, (width - 8.0).min(170.0), h, s);
                        c |= slider(ui, lang, tr(lang, K_DEVELOP_LUMINANCE), l, -100.0, 100.0, 0.0);
                    });
                    c
                };
                let avail = ui.available_width();
                if self.workspace.ai().grade_region == 0 {
                    // Midtones on top, centred; shadows and highlights under it.
                    let half = ((avail - ui.spacing().item_spacing.x) / 2.0).floor();
                    ui.horizontal(|ui| {
                        ui.add_space(((avail - half) / 2.0).floor());
                        changed |= wheel(ui, tr(lang, K_DEVELOP_MIDTONES), half, &mut cg.midtone_hue, &mut cg.midtone_sat, &mut cg.midtone_lum);
                    });
                    ui.add_space(SPACE_SM);
                    ui.horizontal(|ui| {
                        changed |= wheel(ui, tr(lang, K_DEVELOP_SHADOWS), half, &mut cg.shadow_hue, &mut cg.shadow_sat, &mut cg.shadow_lum);
                        changed |= wheel(ui, tr(lang, K_DEVELOP_HIGHLIGHTS), half, &mut cg.highlight_hue, &mut cg.highlight_sat, &mut cg.highlight_lum);
                    });
                } else {
                    let width = (avail * 0.7).floor();
                    ui.horizontal(|ui| {
                        ui.add_space(((avail - width) / 2.0).floor());
                        changed |= wheel(ui, tr(lang, K_DEVELOP_GLOBAL), width, &mut cg.global_hue, &mut cg.global_sat, &mut cg.global_lum);
                    });
                }
                // Blending/Balance shape the WHOLE grade, not one wheel — a
                // scope caption keeps them from reading as a wheel's own. The
                // two sliders are capability rows: the band and the reset
                // (blending 50, not 0) come from the core catalogue's nested
                // colour-grade table, not from this draw site.
                sub_caption(ui, tr(lang, K_DEVELOP_ALL_REGIONS));
                changed |= self.capability_rows(ui, crate::controls::GRADE_GLOBALS);
            });
        changed
    }

    /// ▦ 堆栈 — several frames of one scene merged into a new negative.
    ///
    /// The ACTIVE card's frame is the reference and the picked frames join it;
    /// the result is a new ▦ card. Every input the verb reads is drawn in this
    /// fold (the user decision of 2026-09-12): which merge to run, and whether
    /// the frames are registered already.
    ///
    /// It returns nothing because it changes no recipe field — the merge
    /// writes a master and pushes a card, which is not an edit of the develop
    /// the panel is holding.
    pub(crate) fn dev_stack(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        // The section's ● says this photo HAS a stacked card, which is the
        // only state this fold owns that outlives the click.
        let has = self.editor.doc().variants.iter().any(|v| v.kind == VariantKind::Stacked);
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, K_DEVELOP_STACK), has))
            .id_salt("sec_stack")
            .default_open(false)
            , |ui| {
                explainer(ui, lang, tr(lang, K_DEVELOP_MERGE_SEVERAL_FRAMES_OF_ONE_SCENE_INTO_A_NEW_NEG));
                ui.add_space(SPACE_SM);
                egui::ComboBox::from_id_salt("stack_kind")
                    .selected_text(tr(lang, self.workspace.tools().stack_kind.label()))
                    .show_ui(ui, |ui| {
                        for k in autoshade::stack::merge::StackKind::ALL {
                            ui.selectable_value(&mut self.workspace.tools_mut().stack_kind, k, tr(lang, k.label()));
                        }
                    })
                    .response
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_HDR_MERGE_AN_EXPOSURE_BRACKET_INTO_ONE_FRAME_HOL,
                    ));
                ui.checkbox(&mut self.workspace.tools_mut().stack_tripod, tr(lang, K_DEVELOP_SHOT_ON_A_TRIPOD_SKIP_ALIGNMENT))
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_ALIGNMENT_IS_THE_EXPENSIVE_HALF_OF_A_STACK_AND_O,
                    ));
                ui.add_space(SPACE_SM);
                ui.horizontal(|ui| {
                    let ready = self.editor.photo().src_path.is_some() && !self.workspace.busy();
                    if action_row(ui, ready, tr(lang, K_DEVELOP_STACK_WITH_OTHER_FRAMES))
                        .on_hover_text(tr(
                            lang,
                            K_DEVELOP_PICK_THE_OTHER_FRAMES_OF_THIS_SCENE_THEY_MUST_BE,
                        ))
                        .clicked()
                        && let Some(extra) = photo_files_dialog()
                    {
                        self.act_all(vec![logic::Action::Develop(
                            logic::develop::DevelopAction::Stack(extra),
                        )]);
                    }
                });
            });
            false
    }

    /// The Lens Blur section of the Develop panel.
    pub(crate) fn dev_lens_blur(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        // A verb section: its body returns the actions the worker start
        // needs, and the row's draw applies them (the facade does this for
        // every surface).
        let actions = self.lens_blur_section(ui);
        let touched = !actions.is_empty();
        if touched {
            self.act_all(actions);
        }
        touched
    }

    pub(crate) fn lens_blur_section(&mut self, ui: &mut egui::Ui) -> Vec<logic::Action> {
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

    pub(crate) fn lens_blur_body(&mut self, ui: &mut egui::Ui) -> Vec<logic::Action> {
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
