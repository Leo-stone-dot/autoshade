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
            self.panel_view(&mut Vec::new()).dev_mask_ai_verbs(ui, n_masks);
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
                            if self.workspace.disarm_selection_bound_tools(self.workspace.masks().sel_mask) {
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
                self.workspace.remap_mask_indices(|s| Some(remap(s)));
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
                self.workspace.remap_mask_indices(|s| {
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
            changed |= self.panel_view(&mut Vec::new()).dev_mask_selected_or_hint(ui, n_masks);
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
                self.panel_view(&mut Vec::new()).disarm_tools();
                if !lin_armed {
                    self.workspace.masks_mut().placing_mask = Some((MaskKind::Linear, PlaceTarget::NewMask));
                    self.workspace.set_status(tr(lang, K_APP_DRAG_ON_THE_IMAGE_TO_DRAW_A_LINEAR_GRADIENT_STAR).into());
                }
            }
            let rad_armed = matches!(self.workspace.masks().placing_mask, Some((MaskKind::Radial, PlaceTarget::NewMask)));
            if toggle_in(ui, cell, rad_armed, tr(lang, K_DEVELOP_RADIAL__2)).on_hover_text(tr(lang, K_DEVELOP_DRAG_ON_THE_IMAGE_TO_DRAW_AN_ELLIPTICAL_AREA)).clicked() {
                self.panel_view(&mut Vec::new()).disarm_tools();
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
                    self.panel_view(&mut Vec::new()).disarm_tools();
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
                    self.panel_view(&mut Vec::new()).disarm_tools();
                }
            });
            // #9: the radius belongs AT THE TOOL. This is the same
            // `self.workspace.masks().brush` the Retouch panel's slider drives (and the
            // `[` / `]` keys) — one session block covers both arms, the
            // 「＋ Brush」 above and 「🖌 Edit raster」 down in the mask row.
            // Outside the row closure: that closure holds `&mut self`.
            self.panel_view(&mut Vec::new()).brush_size_slider(ui);
        }
    }








}
