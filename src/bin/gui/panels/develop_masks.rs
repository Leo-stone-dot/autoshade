//! Local adjustments: the mask list, the selected mask's own controls, the
//! composed shapes, the range editor and the AI segmentation verbs. This is the
//! develop panel's largest dependency, kept apart from the form renderer while
//! it is decomposed into its own sections.

use crate::*;

impl AutoShadeApp {
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_masks(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;

        // Geometry ends here — local adjustments and management below.
        // (Group boundary — see the Detail section's comment.)
        // #14b, group 3: masks and the pixel-level tools they reach.
        group_caption(ui, tr(lang, K_DEVELOP_LOCAL_PIXEL));
        // --- 局部调整: manual masks — the SAME recipe.masks the AI writes -----
        let n_masks = self.editor.doc().recipe.masks.len();
        let n_masks_s = n_masks.to_string();
        card_fold(ui, egui::CollapsingHeader::new(section_title(
            &trf(lang, K_DEVELOP_LOCAL_MASKS, &[("n", &n_masks_s)]),
            self.editor.masks_section_active(),
        ))
        .id_salt("sec_local")
        .default_open(false)
        , |ui| {
            self.dev_mask_create_row(ui);
            self.dev_mask_ai_verbs(ui, n_masks);
            // Mask list: click to select (shows overlay + sliders), 🗑 deletes;
            // HOVERING a row previews that mask's coverage without selecting.
            // hover_mask lives ONE frame: update() takes it each frame and this
            // list re-sets it while the cursor is on a row — so leaving the
            // panel, collapsing this section or switching photos all fall back
            // to the selection with no stale index to chase.
            // Reorder is a dedicated ☰ grip per row (the ONLY drag source),
            // NOT the row itself: `dnd_drag_source` overlays its contents with
            // a drag-sensing interact (grab cursor), so wrapping the whole row
            // made the slightest press-move float the row instead of selecting
            // it (user-reported "click moves instead of selects"). Grip drags,
            // row body clicks — the drop TARGET stays the whole row. egui
            // clears the payload on release/Esc itself, and while a drag is in
            // flight `hovered()` is false everywhere, so the hover preview
            // pauses instead of churning the coverage overlay.
            let mut delete: Option<usize> = None;
            let mut toggle_eye: Option<usize> = None;
            let mut dropped: Option<(usize, usize)> = None; // (from, insert-before)
            for i in 0..n_masks {
                let row_resp = ui
                    .horizontal(|ui| {
                        let m = &self.editor.doc().recipe.masks[i];
                        let kind = match m.range {
                            Some(RangeMask::Luminance { .. }) => {
                                tr(self.lang, K_DEVELOP_LUMINANCE_RANGE)
                            }
                            Some(RangeMask::Color { .. }) => tr(self.lang, K_DEVELOP_COLOR_RANGE),
                            None => match m.mask {
                                MaskGeometry::Linear { .. } => tr(self.lang, K_DEVELOP_LINEAR),
                                MaskGeometry::Shaped { .. } => tr(self.lang, K_DEVELOP_SHAPED_GRADIENT),
                                MaskGeometry::Radial { .. } => tr(self.lang, K_DEVELOP_RADIAL),
                                MaskGeometry::Bitmap { .. } => tr(self.lang, K_DEVELOP_BITMAP),
                                MaskGeometry::Brush { .. } => tr(self.lang, K_DEVELOP_BRUSH),
                                // Named "AI" and never "Sky"/"Subject": the row
                                // must not read as Lightroom's own mask. What it
                                // draws is our segmenter's re-derivation, and the
                                // overlay badge plus the import line carry the
                                // rest of that sentence (R27 Batch-5).
                                MaskGeometry::AiMask { .. } => tr(self.lang, K_DEVELOP_AI),
                            },
                        };
                        // A user-given name wins; else a reverse-fit zone shows
                        // its localised role label; else the generic placeholder.
                        let base: &str = if !m.name.is_empty() {
                            m.name.as_str()
                        } else if let Some(en) = m.role.en_name() {
                            tr(self.lang, en)
                        } else {
                            tr(self.lang, K_DEVELOP_MASK)
                        };
                        // The activity dot IS the engine's own rule
                        // (util::mask_active → render::engine_active): a parked
                        // mask looks parked, a working one shows ● — a 64-row
                        // list where the two were indistinguishable was a real
                        // navigation cost. The SECTION header reads the same
                        // helper over the whole list (masks_section_active), so
                        // the two dots cannot drift apart again (R22 #16).
                        let active = mask_active(m);
                        let enabled = m.enabled;
                        // L08: a dead raster renders INERT (the engine skips
                        // it with a stderr-only warning) while this row still
                        // read "enabled" — the ⚠ puts the fact on the row.
                        // Muted/parked masks skip the probe: nothing renders.
                        let dead = if active {
                            autoshade::render::dead_bitmap_rasters(m)
                        } else {
                            Vec::new()
                        };
                        let label = format!(
                            "{base} · {kind}{}{}",
                            if active { "  ●" } else { "" },
                            if dead.is_empty() { "" } else { "  ⚠" }
                        );
                        let label = if enabled {
                            egui::RichText::new(label)
                        } else {
                            egui::RichText::new(label).weak()
                        };
                        ui.dnd_drag_source(ui.id().with(("mask_row", i)), i, |ui| {
                            ui.label("☰");
                        })
                        .response
                        .on_hover_text(tr(self.lang, K_DEVELOP_DRAG_TO_REORDER));
                        // The eye: Lightroom's lossless mute. Amount-to-0 as a
                        // mute destroyed the tuned value; this keeps it.
                        if glyph_toggle(ui, enabled, "👁")
                            .on_hover_text(tr(
                                self.lang,
                                K_DEVELOP_SHOW_MUTE_THIS_MASK_WITHOUT_LOSING_ITS_SETTINGS,
                            ))
                            .clicked()
                        {
                            toggle_eye = Some(i);
                        }
                        let mut row = ui.selectable_label(self.workspace.masks().sel_mask == Some(i), label);
                        if !dead.is_empty() {
                            row = row.on_hover_text(trf(
                                self.lang,
                                K_DEVELOP_BITMAP_RASTER_UNREADABLE_THIS_MASK_CURRENTLY_HAS,
                                &[("list", &dead.join(", "))],
                            ));
                        }
                        if row.hovered() {
                            self.workspace.overlay_mut().hover_mask = Some(i);
                        }
                        if row.clicked() {
                            self.workspace.masks_mut().sel_mask =
                                if self.workspace.masks().sel_mask == Some(i) { None } else { Some(i) };
                            // Component selection belongs to ONE mask's list.
                            self.workspace.masks_mut().sel_component = None;
                            self.workspace.overlay_mut().overlay_stale = true; // coverage follows the selection
                            // EVERY index-armed tool follows the selection —
                            // the colour sampler was fixed first, then the
                            // same class recurred for ↻ Redraw /
                            // add-component / the raster-edit brush, whose
                            // arming indicators all live inside the
                            // selected-mask block (L06#3). The single owner
                            // sits beside remap_mask_indices; the brush
                            // teardown discards user paint, so it is said.
                            if self.disarm_selection_bound_tools(self.workspace.masks().sel_mask) {
                                let t = tr(
                                    self.lang,
                                    K_ACTIONS_THE_RASTER_EDIT_BRUSH_SESSION_ENDED_YOU_SELECTED,
                                );
                                self.workspace.toast(ToastKind::Error, t.to_string());
                            }
                        }
                        if glyph(ui, true, "🗑")
                            .on_hover_text(tr(self.lang, K_DEVELOP_DELETE_THIS_MASK_ITS_STACK_ORDER_SHIFTS_THE_ONES))
                            .clicked()
                        {
                            delete = Some(i);
                        }
                    })
                    .response;
                // A grip being dragged over this row: mark the insertion edge
                // (above/below the row midline) and take the drop. The whole
                // row rect is the drop zone, so aiming needs no precision.
                if let (Some(from), Some(p)) =
                    (row_resp.dnd_hover_payload::<usize>(), ui.ctx().pointer_interact_pos())
                {
                    let below = p.y > row_resp.rect.center().y;
                    let y = if below { row_resp.rect.max.y } else { row_resp.rect.min.y };
                    ui.painter().hline(
                        row_resp.rect.x_range(),
                        y,
                        egui::Stroke::new(2.0, ui.visuals().selection.bg_fill),
                    );
                    let insert = if below { i + 1 } else { i };
                    if row_resp.dnd_release_payload::<usize>().is_some() {
                        dropped = Some((*from, insert));
                    }
                }
            }
            if let Some((from, insert)) = dropped
                // Bounds re-checked at APPLY time: the payload index was
                // captured at drag start, and the list can shrink mid-drag
                // (Ctrl+Z, an async AI result) — a stale `from` would panic
                // in Vec::remove.
                && from < self.editor.doc().recipe.masks.len()
                && insert <= self.editor.doc().recipe.masks.len()
                && insert != from
                && insert != from + 1
            {
                let (to, remap) = reorder_move(from, insert);
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::Reorder { from, to }),
                );
                self.remap_mask_indices(|s| Some(remap(s)));
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            if let Some(i) = toggle_eye {
                // A recipe mutation like any slider: develop + overlay follow,
                // and commit_if_settled makes it one undo step. Through the
                // command layer like the develop form's rows.
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::ToggleEnabled { index: i }),
                );
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            if let Some(i) = delete {
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::Delete { index: i }),
                );
                self.remap_mask_indices(|s| {
                    if s == i {
                        None
                    } else if s > i {
                        Some(s - 1)
                    } else {
                        Some(s)
                    }
                });
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            changed |= self.panel_view(&mut Vec::new()).dev_mask_colour_field(ui);
            changed |= self.dev_mask_selected_or_hint(ui, n_masks);
        });
        changed
    }

    /// The three create tools, the brush-session controls and the AI
    /// segmentation verbs: the panel's "start a mask" half, above the list.
    fn dev_mask_create_row(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        // R38: three equal cells — three free-width toggles in a
        // non-wrapping row overran the default panel width in English.
        ui.horizontal(|ui| {
            let cell = columns(ui, 3);
            let lin_armed = matches!(self.workspace.masks().placing_mask, Some((MaskKind::Linear, PlaceTarget::NewMask)));
            if toggle_in(ui, cell, lin_armed, tr(lang, K_DEVELOP_LINEAR__2)).on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_START_FULLY_APPLIED_SIDE_END_U)).clicked() {
                self.disarm_tools();
                if !lin_armed {
                    self.workspace.masks_mut().placing_mask = Some((MaskKind::Linear, PlaceTarget::NewMask));
                    self.workspace.set_status(tr(lang, K_APP_DRAG_ON_THE_IMAGE_TO_DRAW_A_LINEAR_GRADIENT_STAR).into());
                }
            }
            let rad_armed = matches!(self.workspace.masks().placing_mask, Some((MaskKind::Radial, PlaceTarget::NewMask)));
            if toggle_in(ui, cell, rad_armed, tr(lang, K_DEVELOP_RADIAL__2)).on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_TO_DRAW_AN_ELLIPTICAL_AREA)).clicked() {
                self.disarm_tools();
                if !rad_armed {
                    self.workspace.masks_mut().placing_mask = Some((MaskKind::Radial, PlaceTarget::NewMask));
                    self.workspace.set_status(tr(lang, K_APP_DRAG_ON_THE_IMAGE_TO_DRAW_A_RADIAL_ELLIPTICAL_AR).into());
                }
            }
            // LR's most-used local tool, finally: free-form brush →
            // Bitmap mask (the raster carrier + bilinear sampler + cache
            // all pre-existed; this wires the paint canvas into
            // recipe.masks — see start_mask_brush / commit_mask_brush).
            let brush_armed = matches!(self.workspace.masks().mask_brush, Some((None, _)));
            if toggle_in(ui, cell, brush_armed, tr(lang, K_DEVELOP_BRUSH__2))
                .on_hover_text(tr(lang, K_DEVELOP_PAINT_A_FREE_FORM_MASK_DRAG_THE_BRUSH_SIZE_SLIDE))
                .clicked()
            {
                if brush_armed {
                    self.disarm_tools();
                } else if self.editor.photo().base_preview.is_some() {
                    self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::MaskBrush(None),
                    )]);
                }
            }
        });
        // Brush-session controls (create OR raster-edit): erase toggle +
        // the bake/cancel pair. Lives up here so it is visible whichever
        // row armed the session.
        if let Some((target, erase)) = self.workspace.masks().mask_brush {
            ui.horizontal(|ui| {
                let cell = columns(ui, 3);
                if toggle_in(ui, cell, erase, tr(lang, K_DEVELOP_ERASE))
                    .on_hover_text(tr(lang, K_DEVELOP_STROKES_REMOVE_FROM_THE_SELECTION_INSTEAD_OF_ADD))
                    .clicked()
                {
                    self.workspace.masks_mut().mask_brush = Some((target, !erase));
                }
                if primary_in(ui, cell, true, tr(lang, K_DEVELOP_APPLY)).clicked() {
                    self.commit_mask_brush();
                }
                if action_in(ui, cell, true, tr(lang, K_APP_CANCEL)).clicked() {
                    self.disarm_tools();
                }
            });
            // #9: the radius belongs AT THE TOOL. This is the same
            // `self.workspace.masks().brush` the Retouch panel's slider drives (and the
            // `[` / `]` keys) — one session block covers both arms, the
            // 「＋ Brush」 above and 「🖌 Edit raster」 down in the mask row.
            // Outside the row closure: that closure holds `&mut self`.
            self.brush_size_slider(ui);
        }
    }

    /// The AI segmentation pair and the empty-list line. `n_masks` is the count
    /// the caller already read for the section title.
    fn dev_mask_ai_verbs(&mut self, ui: &mut egui::Ui, n_masks: usize) {
        let lang = self.lang;
        // --- AI segmentation → bitmap masks (gap batch A②) ---------------
        ui.horizontal(|ui| {
            // CAPABILITY, not just state: without the python sidecar on
            // disk these two can only fail, so the button says so up front
            // instead of spending the click on a "not found at …" toast.
            let has_helper = segment_helper_available();
            let can_seg = !self.workspace.busy() && self.editor.photo().base_preview.is_some() && has_helper;
            let cell = columns(ui, 2);
            let missing = tr(lang,
                K_DEVELOP_THIS_FEATURE_NEEDS_AUTOSHADE_S_PYTHON_HELPER_WHI,
            );
            // 🤖 on BOTH (#4): one prefix marks every AI verb app-wide, and
            // 「☁ AI select sky」 was the lone exception — a weather glyph
            // where its twin one row over already wore the robot. The
            // tooltip carries the AI-panel cross-reference, but only on the
            // arm that CAN run: a missing-sidecar message must stay about
            // the missing sidecar.
            if action_in(ui, cell, can_seg, tr(lang, K_DEVELOP_SELECT_SUBJECT))
                // R29 B4: the model, its dependencies and its download size
                // all changed with the user's ruling of 2026-08-21, and the
                // FALLBACK is named because a machine without torchvision
                // gets a materially softer mask and would otherwise have no
                // way to know which model drew it.
                .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                    K_DEVELOP_BIREFNET_SALIENT_SUBJECT_SEGMENTATION_BITMAP_MAS,
                )) } else { missing.to_string() })
                .clicked()
            {
                self.apply_actions(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::Segment { target: "subject", label: "Subject" },
                )]);
            }
            if action_in(ui, cell, can_seg, tr(lang, K_DEVELOP_SELECT_SKY))
                .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                    K_DEVELOP_ONEFORMER_ADE20K_SKY_SEGMENTATION_BITMAP_MASK_PY,
                )) } else { missing.to_string() })
                .clicked()
            {
                self.apply_actions(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::Segment { target: "sky", label: "Sky" },
                )]);
            }
        });
        // Empty state: an open section with zero rows looked like a bug;
        // one quiet line says where masks come from.
        if n_masks == 0 {
            ui.add_space(SPACE_XS);
            ui.label(
                egui::RichText::new(tr(
                    lang,
                    K_DEVELOP_NO_MASKS_YET_DRAW_ONE_WITH_THE_TOOLS_ABOVE_AI_AN,
                ))
                .weak()
                .small(),
            );
        }
    }

    /// The selected mask's detail block, or the explainer when there are no
    /// masks, or the "select a mask" hint when the list is non-empty and
    /// nothing is selected. All three arms used to live inline in the section.
    fn dev_mask_selected_or_hint(&mut self, ui: &mut egui::Ui, n_masks: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        // Selected mask: its full slider set.
        if let Some(i) = self.workspace.masks().sel_mask.filter(|&i| i < self.editor.doc().recipe.masks.len()) {
            let coverage = self.dev_mask_geometry_and_range(ui, i);
            // Any edit up to here — geometry, range, Amount — can move the
            // coverage wash (the overlay key carries them all); the key
            // compare inside refresh_mask_overlay dedupes rebuilds, so
            // over-flagging is free. The tone sliders below are not coverage
            // and deliberately stay out.
            if coverage {
                self.workspace.overlay_mut().overlay_stale = true;
            }
            changed |= coverage;
            changed |= self.panel_view(&mut Vec::new()).dev_mask_adjustments(ui, i);
        } else if n_masks == 0 {
            explainer(ui, lang, tr(lang, K_DEVELOP_LIGHTROOM_STYLE_LOCAL_ADJUSTMENTS_ADD_A_GRADIENT));
        } else {
            // Masks exist, none is selected — one click on the selected
            // row (it toggles) lands here, and the section then showed a
            // list with no adjustments under it and no reason why.
            ui.add_space(SPACE_XS);
            ui.label(
                egui::RichText::new(tr(lang, K_DEVELOP_SELECT_A_MASK_ABOVE_TO_EDIT_ITS_ADJUSTMENTS))
                    .weak()
                    .small(),
            );
        }
        changed
    }

    /// The selected mask's geometry half: the name/verb row, the radial knobs,
    /// the raster tools, the composed shapes and the range editor — every edit
    /// up to and including Amount, i.e. the edits that can move the coverage
    /// wash. The caller invalidates the overlay when this returns true.
    fn dev_mask_geometry_and_range(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let mut changed = self.dev_mask_verbs_and_geometry(ui, i);
        changed |= self.dev_mask_shapes(ui, i);
        changed |= self.dev_mask_range_and_amount(ui, i);
        changed
    }


    /// The name/verb row, the radial knobs and the raster tools: the edits that
    /// belong to the mask itself rather than to its composition.
    fn dev_mask_verbs_and_geometry(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        ui.horizontal(|ui| {
            // The name edits a LOCAL buffer and commits one recipe
            // mutation on focus loss (Enter included) — binding the
            // TextEdit straight to the recipe pushed one undo step per
            // keystroke, so "sky gradient" burned 12 history slots and
            // Ctrl+Z walked the name back letter by letter. When the
            // selection jumps to another row mid-edit, the pending
            // rename commits to ITS OWN mask first — but only while
            // that mask's name still equals the seed-time snapshot, so
            // index shifts (delete/reorder) can never cross-commit.
            if self.workspace.masks().mask_name_buf.as_ref().is_none_or(|(j, ..)| *j != i) {
                if let Some((j, orig, buf)) = self.workspace.masks_mut().mask_name_buf.take() {
                    let same = self.editor.doc().recipe.masks.get(j).is_some_and(|prev| prev.name == orig);
                    if same && buf != orig {
                        crate::logic::command::apply(
                            &mut self.editor.doc_mut().recipe,
                            crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::Rename { index: j, name: buf }),
                        );
                        changed = true;
                    }
                }
                let cur = self.editor.doc().recipe.masks[i].name.clone();
                self.workspace.masks_mut().mask_name_buf = Some((i, cur.clone(), cur));
            }
            // R38: the name takes what the row leaves after the verbs
            // beside it (↻ Redraw when the geometry has one, then
            // ⬆ ⬇ ⧉), so the row ends where the grid rows below it
            // end. At a fixed 110 px, with the two switches also on
            // this row, it ran 100 px past the panel — and the panel
            // widened to fit, taking every row below it along.
            let has_redraw = matches!(
                self.editor.doc().recipe.masks[i].mask,
                MaskGeometry::Linear { .. } | MaskGeometry::Radial { .. }
            );
            let reserve = {
                let sp = ui.spacing();
                let redraw = if has_redraw {
                    let label = ui.fonts_mut(|f| {
                        f.layout_no_wrap(
                            tr(lang, K_DEVELOP_REDRAW).to_owned(),
                            egui::TextStyle::Button.resolve(ui.style()),
                            egui::Color32::WHITE,
                        )
                        .size()
                        .x
                    });
                    label + 2.0 * sp.button_padding.x + sp.item_spacing.x
                } else {
                    0.0
                };
                redraw + 3.0 * (crate::buttons::row_h(ui) + sp.item_spacing.x)
            };
            // The frame's own margin (egui's 4 px each side) lies
            // outside `desired_width`, so it comes off the budget too.
            let name_margin = egui::Margin::symmetric(4, 2);
            let name_resp = {
                let buf = &mut self.workspace.masks_mut().mask_name_buf.as_mut().expect("seeded above").2;
                ui.add(
                    egui::TextEdit::singleline(buf)
                        .margin(name_margin)
                        .desired_width(
                            (ui.available_width() - reserve - name_margin.sum().x).max(110.0),
                        )
                        .hint_text(tr(lang, K_DEVELOP_NAME)),
                )
            };
            if name_resp.lost_focus()
                && let Some((_, orig, buf)) = self.workspace.masks_mut().mask_name_buf.as_mut()
                && *buf != self.editor.doc().recipe.masks[i].name
            {
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::Rename {
                        index: i,
                        name: buf.clone(),
                    }),
                );
                // Re-baseline the seed: a SECOND rename in this row
                // must still cross-commit correctly on a row switch.
                *orig = buf.clone();
                changed = true;
            }
            let m = &mut self.editor.doc_mut().recipe.masks[i];
            // Raster masks have no drag-to-place geometry — no 重画.
            let kind = match m.mask {
                MaskGeometry::Linear { .. } => Some(MaskKind::Linear),
                MaskGeometry::Radial { .. } => Some(MaskKind::Radial),
                // Neither a raster nor a brush group has drag-to-place
                // geometry — no 重画 for either.
                // An AI mask has no drag-to-place geometry either:
                // its reference point is a MODEL PROMPT, so "redraw"
                // would mean re-running the segmenter, not moving a
                // shape — a different action from this button's.
                // A shaped gradient's curve was measured from a
                // target; redrawing its handles would keep a curve
                // measured for another span.
                MaskGeometry::Bitmap { .. }
                | MaskGeometry::Brush { .. }
                | MaskGeometry::AiMask { .. }
                | MaskGeometry::Shaped { .. } => None,
            };
            if let Some(kind) = kind {
                let redraw_armed = matches!(
                    self.workspace.masks().placing_mask,
                    Some((k, PlaceTarget::Redraw(j))) if k == kind && j == i
                );
                if ui
                    .selectable_label(redraw_armed, tr(lang, K_DEVELOP_REDRAW))
                    .on_hover_text(tr(lang, K_DEVELOP_RE_DRAG_THIS_MASK_S_AREA_ON_THE_IMAGE))
                    .clicked()
                {
                    self.disarm_tools();
                    if !redraw_armed {
                        self.workspace.masks_mut().placing_mask = Some((kind, PlaceTarget::Redraw(i)));
                        self.workspace.set_status(tr(lang, K_DEVELOP_RE_DRAG_THIS_MASK_S_AREA_ON_THE_IMAGE).into());
                    }
                }
            }
            // Mask ORDER is render semantics (masks stack sequentially;
            // a later mask's range sees earlier masks' output) — so the
            // list order is editable, not just cosmetic.
            if glyph(ui, i > 0, "⬆")
                .on_hover_text(tr(lang, K_DEVELOP_MOVE_UP_RENDERS_EARLIER))
                .clicked()
            {
                self.apply_actions(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::MoveMaskUp(i),
                )]);
            }
            if glyph(ui, i + 1 < self.editor.doc().recipe.masks.len(), "⬇")
                .on_hover_text(tr(lang, K_DEVELOP_MOVE_DOWN_RENDERS_LATER))
                .clicked()
            {
                self.apply_actions(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::MoveMaskDown(i),
                )]);
            }
            // Duplicate: a second gradient with the same tuned ten
            // sliders; rasters are DETACHED copies (the version-load
            // rule), so the twins never share a mutable file.
            if glyph(ui, true, "⧉")
                .on_hover_text(tr(lang, K_DEVELOP_DUPLICATE_THIS_MASK_BITMAP_RASTERS_ARE_COPIED_SO))
                .clicked()
            {
                self.apply_actions(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::DuplicateMask(i),
                )]);
            }
        });
        // R38: the two switches on their own row, under the verbs.
        ui.horizontal(|ui| {
            // Inversion flips the mask's coverage — its Response.changed()
            // must drive the develop + overlay like every other mask
            // control (was silently discarded: the toggle mutated the
            // recipe but never re-rendered until an unrelated edit).
            if ui.checkbox(&mut self.editor.doc_mut().recipe.masks[i].inverted, tr(lang, K_DEVELOP_INVERT)).changed() {
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            // ONE view switch, not a property of this row (R22 #16): it
            // is the same flag the O key toggles, and
            // `refresh_mask_overlay` (canvas.rs) always draws whichever
            // mask is hovered-or-selected. Sitting in the selected-mask
            // row is right — that IS the mask it draws — but 「Overlay」
            // beside 「Invert」 (and, before R38, 「↻ Redraw」 and the
            // ⬆/⬇ order buttons) reads as
            // per-mask state that would be remembered per mask. The
            // label now says what it is, in the same words the F1 sheet
            // uses for the O key ("Toggle mask overlay").
            if ui
                .checkbox(&mut self.workspace.overlay_mut().show_mask_overlay, tr(lang, K_DEVELOP_SHOW_MASK_OVERLAY))
                .on_hover_text(tr(lang, K_DEVELOP_ONE_VIEW_SWITCH_SHARED_BY_EVERY_MASK_SHORTCUT_O))
                .changed()
            {
                self.workspace.overlay_mut().overlay_stale = true;
            }
        });
        // Radial geometry: edge feather (shown 0..100, LR's track) +
        // rotation + inside/outside flip — all engine-rendered.
        if let MaskGeometry::Radial { feather, flipped, angle, .. } =
            &mut self.editor.doc_mut().recipe.masks[i].mask
        {
            let mut geo_ch = false;
            ui.horizontal(|ui| {
                geo_ch |= slider_pct(
                    ui,
                    lang,
                    tr(lang, K_DEVELOP_EDGE_FEATHER),
                    feather,
                    1.0,
                    0.5,
                );
                geo_ch |= ui
                    .checkbox(flipped, tr(lang, K_DEVELOP_FLIP))
                    .on_hover_text(tr(
                        lang,
                        K_DEVELOP_SWAP_WHICH_SIDE_OF_THE_ELLIPSE_THE_ADJUSTMENT_AF,
                    ))
                    .changed();
            });
            // Rotation: the on-image grip (the knob parked above the
            // ellipse) drags it too; the slider gives numeric entry.
            // -179.9: clamp_geometry normalizes to (-180, 180], so a
            // drag to the very left snapped the knob to +180 — the
            // ellipse never moved, the knob teleported (L15-6; the
            // hue-slider rule).
            geo_ch |= slider(
                ui,
                lang,
                tr(lang, K_DEVELOP_ANGLE),
                angle,
                -179.9,
                180.0,
                0.0,
            );
            if geo_ch {
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
        }
        // Raster tools: before these, a raster mask was completely
        // un-editable after creation — a clipped treeline's only
        // recourse was re-running the model to get the same raster.
        // Every op BAKES a freshly claimed raster and repoints (never
        // mutates the input file).
        //
        // The gate is "does this mask have PIXELS", not "is it a
        // `Bitmap`": the reverse-fit zones are Select Sky components
        // now and still ride an ordinary claimed PNG, while an AI mask
        // whose alpha has not resolved has nothing to open.
        if autoshade::render::geometry_raster_path(&self.editor.doc().recipe.masks[i].mask).is_some() {
            // R38: two aligned rows — the pair that opens a session
            // or a full-size re-cut, then the three one-step
            // morphologies — instead of five free-width buttons
            // wrapping wherever the panel width fell.
            ui.horizontal(|ui| {
                let cell = columns(ui, 2);
                let edit_armed = matches!(self.workspace.masks().mask_brush, Some((Some(j), _)) if j == i);
                if toggle_in(ui, cell, edit_armed, tr(lang, K_DEVELOP_EDIT_RASTER))
                    .on_hover_text(tr(lang, K_DEVELOP_BRUSH_EDIT_THIS_MASK_PAINT_ADDS_ERASE_REMOVES_AP))
                    .clicked()
                {
                    if edit_armed {
                        self.disarm_tools();
                    } else {
                        self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::MaskBrush(Some(i)),
                    )]);
                    }
                }
                if action_in(ui, cell, !self.workspace.busy(), tr(lang, K_DEVELOP_FULL_RES_REFINE))
                    .on_hover_text(tr(lang, K_DEVELOP_RE_CUT_THIS_MASK_AGAINST_THE_FULL_RESOLUTION_SOU))
                    .clicked()
                {
                    self.apply_actions(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::MaskRefine(i),
                )]);
                }
            });
            ui.horizontal(|ui| {
                let cell = columns(ui, 3);
                if action_in(ui, cell, true, tr(lang, K_DEVELOP_SOFTEN))
                    .on_hover_text(tr(lang, K_DEVELOP_SOFTEN_THE_MASK_BOUNDARY_ONE_STEP_BAKES_A_NEW_RA))
                    .clicked()
                {
                    crate::masks::bake_mask_raster(&mut self.editor, &mut self.workspace, self.lang, 
                        i,
                        |g| {
                            let s = (g.width().max(g.height()) as f32 / 400.0).max(1.0);
                            autoshade::render::feather_mask(g, s)
                        },
                        "mask-edit",
                    );
                }
                if action_in(ui, cell, true, tr(lang, K_DEVELOP_EXPAND))
                    .on_hover_text(tr(lang, K_DEVELOP_GROW_THE_SELECTION_ONE_STEP_BAKES_A_NEW_RASTER))
                    .clicked()
                {
                    crate::masks::bake_mask_raster(&mut self.editor, &mut self.workspace, self.lang, 
                        i,
                        |g| {
                            let r = (g.width().max(g.height()) / 500).max(2) as i32;
                            autoshade::render::morph_mask(g, r)
                        },
                        "mask-edit",
                    );
                }
                if action_in(ui, cell, true, tr(lang, K_DEVELOP_CONTRACT))
                    .on_hover_text(tr(lang, K_DEVELOP_SHRINK_THE_SELECTION_ONE_STEP_BAKES_A_NEW_RASTER))
                    .clicked()
                {
                    crate::masks::bake_mask_raster(&mut self.editor, &mut self.workspace, self.lang, 
                        i,
                        |g| {
                            let r = (g.width().max(g.height()) / 500).max(2) as i32;
                            autoshade::render::morph_mask(g, -r)
                        },
                        "mask-edit",
                    );
                }
            });
        }
        changed
    }

    /// Lightroom's Add / Subtract / Intersect grammar: add or drop one shape on
    /// this mask and pick which shape the on-image knobs edit.
    fn dev_mask_shapes(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        // --- Shapes（组件）: compose extra geometry onto this mask —
        // Lightroom's Add / Subtract / Intersect grammar. Every native
        // shape projects through the same composition spelling; only
        // Bitmap components remain a named geometry loss.
        {
            ui.horizontal(|ui| {
                ui.label(tr(lang, K_DEVELOP_SHAPES));
                let mode_label = |m: autoshade::recipe::MaskCombine| match m {
                    autoshade::recipe::MaskCombine::Add => tr(lang, K_DEVELOP_ADD),
                    autoshade::recipe::MaskCombine::Subtract => tr(lang, K_DEVELOP_SUBTRACT),
                    autoshade::recipe::MaskCombine::Intersect => tr(lang, K_DEVELOP_INTERSECT),
                };
                egui::ComboBox::from_id_salt("comp_mode")
                    .selected_text(mode_label(self.workspace.masks().component_mode))
                    .width(96.0)
                    .show_ui(ui, |ui| {
                        for m in [
                            autoshade::recipe::MaskCombine::Add,
                            autoshade::recipe::MaskCombine::Subtract,
                            autoshade::recipe::MaskCombine::Intersect,
                        ] {
                            ui.selectable_value(&mut self.workspace.masks_mut().component_mode, m, mode_label(m));
                        }
                    });
                let mode = self.workspace.masks().component_mode;
                let lin_armed = matches!(
                    self.workspace.masks().placing_mask,
                    Some((MaskKind::Linear, PlaceTarget::Component(j, _))) if j == i
                );
                if ui
                    .selectable_label(lin_armed, tr(lang, K_DEVELOP_LINEAR))
                    .on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_TO_ADD_A_LINEAR_SHAPE_TO_THIS))
                    .clicked()
                {
                    self.disarm_tools();
                    if !lin_armed {
                        self.workspace.masks_mut().placing_mask =
                            Some((MaskKind::Linear, PlaceTarget::Component(i, mode)));
                        self.workspace.set_status(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_THE_NEW_SHAPE_COMPOSES_ONTO_TH).into());
                    }
                }
                let rad_armed = matches!(
                    self.workspace.masks().placing_mask,
                    Some((MaskKind::Radial, PlaceTarget::Component(j, _))) if j == i
                );
                if ui
                    .selectable_label(rad_armed, tr(lang, K_DEVELOP_RADIAL))
                    .on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_TO_ADD_AN_ELLIPTICAL_SHAPE_TO))
                    .clicked()
                {
                    self.disarm_tools();
                    if !rad_armed {
                        self.workspace.masks_mut().placing_mask =
                            Some((MaskKind::Radial, PlaceTarget::Component(i, mode)));
                        self.workspace.set_status(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_THE_NEW_SHAPE_COMPOSES_ONTO_TH).into());
                    }
                }
            });
            let n_comp = self.editor.doc().recipe.masks[i].components.len();
            let mut del_comp: Option<usize> = None;
            for c in 0..n_comp {
                ui.horizontal(|ui| {
                    let comp = &self.editor.doc().recipe.masks[i].components[c];
                    let icon = match comp.mode {
                        autoshade::recipe::MaskCombine::Add => "＋",
                        autoshade::recipe::MaskCombine::Subtract => "－",
                        autoshade::recipe::MaskCombine::Intersect => "∩",
                    };
                    let kind = match comp.geometry {
                        MaskGeometry::Linear { .. } => tr(lang, K_DEVELOP_LINEAR),
                        MaskGeometry::Shaped { .. } => tr(lang, K_DEVELOP_SHAPED_GRADIENT),
                        MaskGeometry::Radial { .. } => tr(lang, K_DEVELOP_RADIAL),
                        MaskGeometry::Bitmap { .. } => tr(lang, K_DEVELOP_BITMAP),
                        MaskGeometry::Brush { .. } => tr(lang, K_DEVELOP_BRUSH),
                        MaskGeometry::AiMask { .. } => tr(lang, K_DEVELOP_AI),
                    };
                    let selected = self.workspace.masks().sel_component == Some(c);
                    if ui
                        .selectable_label(selected, format!("{icon} {kind}"))
                        .on_hover_text(tr(lang, K_DEVELOP_SELECT_TO_DRAG_THIS_SHAPE_S_KNOBS_ON_THE_IMAGE_T))
                        .clicked()
                    {
                        self.workspace.masks_mut().sel_component = if selected { None } else { Some(c) };
                    }
                    let mut mode = comp.mode;
                    egui::ComboBox::from_id_salt(("comp_row_mode", c))
                        .selected_text(match mode {
                            autoshade::recipe::MaskCombine::Add => tr(lang, K_DEVELOP_ADD__2),
                            autoshade::recipe::MaskCombine::Subtract => tr(lang, K_DEVELOP_SUBTRACT__2),
                            autoshade::recipe::MaskCombine::Intersect => tr(lang, K_DEVELOP_INTERSECT__2),
                        })
                        .width(88.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Add, tr(lang, K_DEVELOP_ADD__2));
                            ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Subtract, tr(lang, K_DEVELOP_SUBTRACT__2));
                            ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Intersect, tr(lang, K_DEVELOP_INTERSECT__2));
                        });
                    if mode != comp.mode {
                        crate::logic::command::apply(
                            &mut self.editor.doc_mut().recipe,
                            crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::SetComponentMode {
                                index: i,
                                component: c,
                                mode,
                            }),
                        );
                        self.workspace.overlay_mut().overlay_stale = true;
                        changed = true;
                    }
                    let mut inverted = self.editor.doc().recipe.masks[i].components[c].inverted;
                    if ui.checkbox(&mut inverted, tr(lang, K_DEVELOP_INVERT)).changed() {
                        crate::logic::command::apply(
                            &mut self.editor.doc_mut().recipe,
                            crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::SetComponentInverted {
                                index: i,
                                component: c,
                                inverted,
                            }),
                        );
                        self.workspace.overlay_mut().overlay_stale = true;
                        changed = true;
                    }
                    if glyph(ui, true, "🗑").clicked() {
                        del_comp = Some(c);
                    }
                });
            }
            if let Some(c) = del_comp {
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::RemoveComponent {
                        index: i,
                        component: c,
                    }),
                );
                self.workspace.masks_mut().sel_component = match self.workspace.masks().sel_component {
                    Some(s) if s == c => None,
                    Some(s) if s > c => Some(s - 1),
                    other => other,
                };
                self.workspace.overlay_mut().overlay_stale = true;
                changed = true;
            }
            if n_comp > 0 {
                explainer(ui, lang, tr(lang, K_DEVELOP_SHAPES_COMPOSE_IN_ORDER_ONTO_THE_BASE_MASK_LINEA));
            }
        }
        changed
    }

    /// The range editor (luminance or colour) and the Amount every range is
    /// judged at — the last coverage-moving control before the adjustments.
    fn dev_mask_range_and_amount(&mut self, ui: &mut egui::Ui, i: usize) -> bool {
        let lang = self.lang;
        let mut changed = false;
        // --- Range Mask（LR 范围蒙版）: refines WHERE the geometry applies —
        // final weight = geometry × range, live in preview + export + XMP.
        // Coverage invalidation happens ONCE below (after Amount): any
        // edit up to there can move the wash, and the overlay key
        // compare dedupes rebuilds — the old "range-section-only"
        // tracker went blind whenever an earlier control had already
        // set `changed` in the same frame.
        {
            let cur = match &self.editor.doc().recipe.masks[i].range {
                None => 0usize,
                Some(RangeMask::Luminance { .. }) => 1,
                Some(RangeMask::Color { .. }) => 2,
            };
            let mut sel = cur;
            ui.horizontal(|ui| {
                ui.label(tr(lang, K_DEVELOP_RANGE_MASK));
                egui::ComboBox::from_id_salt("range_kind")
                    .selected_text([tr(lang, K_DEVELOP_NONE), tr(lang, K_DEVELOP_LUMINANCE), tr(lang, K_DEVELOP_COLOR)][sel])
                    .width(70.0)
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut sel, 0, tr(lang, K_DEVELOP_NONE));
                        ui.selectable_value(&mut sel, 1, tr(lang, K_DEVELOP_LUMINANCE));
                        ui.selectable_value(&mut sel, 2, tr(lang, K_DEVELOP_COLOR));
                    });
            });
            if sel != cur {
                // Leaving Color: the armed sampler goes too, or the
                // canvas keeps advertising (and dispatching) a colour
                // pick for a range that no longer exists.
                if cur == 2 && self.workspace.tools().range_picking == Some(i) {
                    self.workspace.tools_mut().range_picking = None;
                }
                let range = match sel {
                    // Full range = neutral start; narrow from there.
                    1 => Some(RangeMask::Luminance { lo_outer: 0.0, lo: 0.0, hi: 1.0, hi_outer: 1.0 }),
                    2 => Some(RangeMask::Color { r: 0.5, g: 0.5, b: 0.5, amount: 0.5, px: 0.5, py: 0.5 }),
                    _ => None,
                };
                crate::logic::command::apply(
                    &mut self.editor.doc_mut().recipe,
                    crate::logic::command::Command::Mask(crate::logic::command::MaskCommand::SetRange { index: i, range }),
                );
                if sel == 2 {
                    // Jump straight into sampling — a colour range without
                    // a picked colour selects nothing useful.
                    self.disarm_tools();
                    self.workspace.tools_mut().range_picking = Some(i);
                    self.workspace.set_status(tr(lang, K_DEVELOP_COLOR_RANGE_CLICK_THE_COLOR_TO_PICK_IN_THE_IMAGE).into());
                }
                changed = true;
            }
            let picking_this = self.workspace.tools().range_picking == Some(i);
            let mut want_pick = false;
            match &mut self.editor.doc_mut().recipe.masks[i].range {
                Some(RangeMask::Luminance { lo_outer, lo, hi, hi_outer }) => {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, K_DEVELOP_ORDERED_BOUNDS));
                        ui.monospace(format!(
                            "{lo_outer:.3} <= {lo:.3} <= {hi:.3} <= {hi_outer:.3}"
                        ));
                    });
                    // GUI shows lo/hi + one symmetric feather; the recipe keeps
                    // ACR's 4-number trapezoid (asymmetric AI trapezoids show
                    // their averaged feather until a slider is touched).
                    // All three on LR's 0..100 display track (storage
                    // stays the 0..1 fraction — see slider_pct).
                    let mut f = ((*lo - *lo_outer) + (*hi_outer - *hi)) * 0.5;
                    let ch_lo = slider_pct(ui, lang, tr(lang, K_DEVELOP_LUM_LOW), lo, 1.0, 0.0);
                    let ch_hi = slider_pct(ui, lang, tr(lang, K_DEVELOP_LUM_HIGH), hi, 1.0, 1.0);
                    let ch_f = slider_pct(ui, lang, tr(lang, K_DEVELOP_FEATHER), &mut f, 0.5, 0.1);
                    if ch_lo || ch_hi || ch_f {
                        // CLAMP the edited endpoint at the other, never
                        // swap: the active slider stays bound to its
                        // field across drag frames, so swapping made
                        // both bounds follow the pointer and converge.
                        if *lo > *hi {
                            if ch_lo { *lo = *hi } else { *hi = *lo }
                        }
                        *lo_outer = (*lo - f).max(0.0);
                        *hi_outer = (*hi + f).min(1.0);
                        changed = true;
                    }
                }
                Some(RangeMask::Color { r, g, b, amount, .. }) => {
                    ui.horizontal(|ui| {
                        let full = columns(ui, 1);
                        let mut c = [*r, *g, *b];
                        let swatch = ui.color_edit_button_rgb(&mut c);
                        if swatch.changed() {
                            [*r, *g, *b] = [c[0], c[1], c[2]];
                            changed = true;
                        }
                        // The verb fills the rest of the row after the
                        // swatch (2026-09-27), as the gallery's ✕ row does.
                        let cell = egui::vec2(
                            (full.x - swatch.rect.width() - ui.spacing().item_spacing.x).max(0.0),
                            full.y,
                        );
                        let label = if picking_this { tr(lang, K_DEVELOP_CLICK_IN_IMAGE) } else { tr(lang, K_DEVELOP_SAMPLE) };
                        if action_in(ui, cell, true, label).on_hover_text(tr(lang, K_DEVELOP_CLICK_THE_COLOR_TO_PICK_IN_THE_IMAGE_THE_SAME_CO)).clicked() {
                            want_pick = true;
                        }
                    });
                    changed |= slider_pct(ui, lang, tr(lang, K_DEVELOP_TOLERANCE), amount, 1.0, 0.5);
                }
                None => {}
            }
            if want_pick {
                self.disarm_tools();
                if !picking_this {
                    self.workspace.tools_mut().range_picking = Some(i);
                    self.workspace.set_status(tr(lang, K_DEVELOP_COLOR_RANGE_CLICK_THE_COLOR_TO_PICK_IN_THE_IMAGE).into());
                }
            }
        }
        let m = &mut self.editor.doc_mut().recipe.masks[i];
        // LR's 0..100 display track (storage stays 0..1 — slider_pct).
        changed |= slider_pct(ui, lang, tr(lang, K_DEVELOP_AMOUNT), &mut m.amount, 1.0, 1.0);
        changed
    }

}
