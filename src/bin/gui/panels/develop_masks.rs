//! Local adjustments: the mask list, the selected mask's own controls, the
//! composed shapes, the range editor and the AI segmentation verbs. This is the
//! develop panel's largest dependency, kept apart from the form renderer while
//! it is decomposed into its own sections.

use crate::*;

impl AutoShadeApp {
    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_masks(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;
        let mut changed = false;

        // Geometry ends here — local adjustments and management below.
        // (Group boundary — see the Detail section's comment.)
        // #14b, group 3: masks and the pixel-level tools they reach.
        group_caption(ui, tr(lang, "Local & Pixel"));
        // --- 局部调整: manual masks — the SAME recipe.masks the AI writes -----
        let n_masks = self.recipe.masks.len();
        let n_masks_s = n_masks.to_string();
        card_fold(ui, egui::CollapsingHeader::new(section_title(
            &trf(lang, "Local Masks ({n})", &[("n", &n_masks_s)]),
            self.masks_section_active(),
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
                        let m = &self.recipe.masks[i];
                        let kind = match m.range {
                            Some(RangeMask::Luminance { .. }) => {
                                tr(self.lang, "Luminance range")
                            }
                            Some(RangeMask::Color { .. }) => tr(self.lang, "Color range"),
                            None => match m.mask {
                                MaskGeometry::Linear { .. } => tr(self.lang, "Linear"),
                                MaskGeometry::Shaped { .. } => tr(self.lang, "Shaped gradient"),
                                MaskGeometry::Radial { .. } => tr(self.lang, "Radial"),
                                MaskGeometry::Bitmap { .. } => tr(self.lang, "Bitmap"),
                                MaskGeometry::Brush { .. } => tr(self.lang, "Brush"),
                                // Named "AI" and never "Sky"/"Subject": the row
                                // must not read as Lightroom's own mask. What it
                                // draws is our segmenter's re-derivation, and the
                                // overlay badge plus the import line carry the
                                // rest of that sentence (R27 Batch-5).
                                MaskGeometry::AiMask { .. } => tr(self.lang, "AI"),
                            },
                        };
                        // A user-given name wins; else a reverse-fit zone shows
                        // its localised role label; else the generic placeholder.
                        let base: &str = if !m.name.is_empty() {
                            m.name.as_str()
                        } else if let Some(en) = m.role.en_name() {
                            tr(self.lang, en)
                        } else {
                            tr(self.lang, "mask")
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
                        .on_hover_text(tr(self.lang, "Drag to reorder"));
                        // The eye: Lightroom's lossless mute. Amount-to-0 as a
                        // mute destroyed the tuned value; this keeps it.
                        if glyph_toggle(ui, enabled, "👁")
                            .on_hover_text(tr(
                                self.lang,
                                "Show/mute this mask without losing its settings",
                            ))
                            .clicked()
                        {
                            toggle_eye = Some(i);
                        }
                        let mut row = ui.selectable_label(self.masks.sel_mask == Some(i), label);
                        if !dead.is_empty() {
                            row = row.on_hover_text(trf(
                                self.lang,
                                "bitmap raster unreadable ({list}) — this mask currently has NO effect",
                                &[("list", &dead.join(", "))],
                            ));
                        }
                        if row.hovered() {
                            self.overlay.hover_mask = Some(i);
                        }
                        if row.clicked() {
                            self.masks.sel_mask =
                                if self.masks.sel_mask == Some(i) { None } else { Some(i) };
                            // Component selection belongs to ONE mask's list.
                            self.masks.sel_component = None;
                            self.overlay.overlay_stale = true; // coverage follows the selection
                            // EVERY index-armed tool follows the selection —
                            // the colour sampler was fixed first, then the
                            // same class recurred for ↻ Redraw /
                            // add-component / the raster-edit brush, whose
                            // arming indicators all live inside the
                            // selected-mask block (L06#3). The single owner
                            // sits beside remap_mask_indices; the brush
                            // teardown discards user paint, so it is said.
                            if self.disarm_selection_bound_tools(self.masks.sel_mask) {
                                let t = tr(
                                    self.lang,
                                    "the raster-edit brush session ended — you selected another mask; its unbaked strokes were discarded",
                                );
                                self.toast(ToastKind::Error, t.to_string());
                            }
                        }
                        if glyph(ui, true, "🗑")
                            .on_hover_text(tr(self.lang, "Delete this mask (its stack order shifts the ones below)"))
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
                && from < self.recipe.masks.len()
                && insert <= self.recipe.masks.len()
                && insert != from
                && insert != from + 1
            {
                let (to, remap) = reorder_move(from, insert);
                let m = self.recipe.masks.remove(from);
                self.recipe.masks.insert(to, m);
                self.remap_mask_indices(|s| Some(remap(s)));
                self.overlay.overlay_stale = true;
                changed = true;
            }
            if let Some(i) = toggle_eye {
                // A recipe mutation like any slider: develop + overlay follow,
                // and commit_if_settled makes it one undo step.
                self.recipe.masks[i].enabled = !self.recipe.masks[i].enabled;
                self.overlay.overlay_stale = true;
                changed = true;
            }
            if let Some(i) = delete {
                self.recipe.masks.remove(i);
                self.remap_mask_indices(|s| {
                    if s == i {
                        None
                    } else if s > i {
                        Some(s - 1)
                    } else {
                        Some(s)
                    }
                });
                self.overlay.overlay_stale = true;
                changed = true;
            }
            // R33 §G. The colour field, in the Masks section because that is
            // where a photographer looks for "what else is local about this
            // photo" — one row, never a list: a recipe holds at most one.
            //
            // Its own row rather than a mask entry, because it is not a mask:
            // it has no geometry to select, no coverage to preview and no
            // stack position to drag. Three controls, which are the three
            // questions a user actually has about it — is it on, how much of
            // it, and get rid of it.
            if self.recipe.colour_field.is_some() {
                ui.add_space(SPACE_XS);
                let mut remove_field = false;
                ui.horizontal(|ui| {
                    let field = self.recipe.colour_field.as_mut().expect("just checked");
                    if glyph_toggle(ui, field.enabled, "👁")
                        .on_hover_text(tr(
                            lang,
                            "Show/mute the colour field without losing its amount",
                        ))
                        .clicked()
                    {
                        field.enabled = !field.enabled;
                        changed = true;
                    }
                    let label = format!(
                        "▦ {} · {}",
                        tr(lang, "Colour field"),
                        tr(lang, "engine-only"),
                    );
                    ui.label(if field.enabled {
                        egui::RichText::new(label)
                    } else {
                        egui::RichText::new(label).weak()
                    })
                    .on_hover_text(tr(
                        lang,
                        "A smooth local colour/tone field the reverse fit solved. It renders here and in every export from this app; classic XMP has no way to carry it, so Lightroom sees the rest of this recipe without it.",
                    ));
                    if glyph(ui, true, "🗑")
                        .on_hover_text(tr(lang, "Remove the colour field"))
                        .clicked()
                    {
                        remove_field = true;
                    }
                });
                let field = self.recipe.colour_field.as_mut().expect("just checked");
                // Through the shared slider (its look, reset to the solved
                // field's full amount, ↑/↓) — it was the one bare egui slider.
                let enabled = field.enabled;
                if ui
                    .add_enabled_ui(enabled, |ui| {
                        Self::slider_pct(ui, lang, tr(lang, "Amount"), &mut field.amount, 1.0, 1.0)
                    })
                    .inner
                {
                    changed = true;
                }
                if remove_field {
                    self.recipe.colour_field = None;
                    changed = true;
                }
            }
            // Selected mask: its full slider set.
            if let Some(i) = self.masks.sel_mask.filter(|&i| i < self.recipe.masks.len()) {
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
                    if self.masks.mask_name_buf.as_ref().is_none_or(|(j, ..)| *j != i) {
                        if let Some((j, orig, buf)) = self.masks.mask_name_buf.take()
                            && let Some(prev) = self.recipe.masks.get_mut(j)
                            && prev.name == orig
                            && buf != orig
                        {
                            prev.name = buf;
                            changed = true;
                        }
                        let cur = self.recipe.masks[i].name.clone();
                        self.masks.mask_name_buf = Some((i, cur.clone(), cur));
                    }
                    // R38: the name takes what the row leaves after the verbs
                    // beside it (↻ Redraw when the geometry has one, then
                    // ⬆ ⬇ ⧉), so the row ends where the grid rows below it
                    // end. At a fixed 110 px, with the two switches also on
                    // this row, it ran 100 px past the panel — and the panel
                    // widened to fit, taking every row below it along.
                    let has_redraw = matches!(
                        self.recipe.masks[i].mask,
                        MaskGeometry::Linear { .. } | MaskGeometry::Radial { .. }
                    );
                    let reserve = {
                        let sp = ui.spacing();
                        let redraw = if has_redraw {
                            let label = ui.fonts_mut(|f| {
                                f.layout_no_wrap(
                                    tr(lang, "↻ Redraw").to_owned(),
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
                        let buf = &mut self.masks.mask_name_buf.as_mut().expect("seeded above").2;
                        ui.add(
                            egui::TextEdit::singleline(buf)
                                .margin(name_margin)
                                .desired_width(
                                    (ui.available_width() - reserve - name_margin.sum().x).max(110.0),
                                )
                                .hint_text(tr(lang, "Name")),
                        )
                    };
                    if name_resp.lost_focus()
                        && let Some((_, orig, buf)) = self.masks.mask_name_buf.as_mut()
                        && *buf != self.recipe.masks[i].name
                    {
                        self.recipe.masks[i].name = buf.clone();
                        // Re-baseline the seed: a SECOND rename in this row
                        // must still cross-commit correctly on a row switch.
                        *orig = buf.clone();
                        changed = true;
                    }
                    let m = &mut self.recipe.masks[i];
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
                            self.masks.placing_mask,
                            Some((k, PlaceTarget::Redraw(j))) if k == kind && j == i
                        );
                        if ui
                            .selectable_label(redraw_armed, tr(lang, "↻ Redraw"))
                            .on_hover_text(tr(lang, "Re-drag this mask's area on the image"))
                            .clicked()
                        {
                            self.disarm_tools();
                            if !redraw_armed {
                                self.masks.placing_mask = Some((kind, PlaceTarget::Redraw(i)));
                                self.status =
                                    tr(lang, "Re-drag this mask's area on the image").into();
                            }
                        }
                    }
                    // Mask ORDER is render semantics (masks stack sequentially;
                    // a later mask's range sees earlier masks' output) — so the
                    // list order is editable, not just cosmetic.
                    if glyph(ui, i > 0, "⬆")
                        .on_hover_text(tr(lang, "Move up (renders earlier)"))
                        .clicked()
                    {
                        self.apply_actions(vec![logic::Action::Develop(
                            logic::develop::DevelopAction::MoveMaskUp(i),
                        )]);
                    }
                    if glyph(ui, i + 1 < self.recipe.masks.len(), "⬇")
                        .on_hover_text(tr(lang, "Move down (renders later)"))
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
                        .on_hover_text(tr(lang, "Duplicate this mask (bitmap rasters are copied, so the copies stay independent)"))
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
                    if ui.checkbox(&mut self.recipe.masks[i].inverted, tr(lang, "Invert")).changed() {
                        self.overlay.overlay_stale = true;
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
                        .checkbox(&mut self.overlay.show_mask_overlay, tr(lang, "Show mask overlay"))
                        .on_hover_text(tr(lang, "One view switch shared by every mask (shortcut O): shows the hovered-or-selected mask's actual coverage as a red semi-transparent overlay (geometry × range × strength)"))
                        .changed()
                    {
                        self.overlay.overlay_stale = true;
                    }
                });
                // Radial geometry: edge feather (shown 0..100, LR's track) +
                // rotation + inside/outside flip — all engine-rendered.
                if let MaskGeometry::Radial { feather, flipped, angle, .. } =
                    &mut self.recipe.masks[i].mask
                {
                    let mut geo_ch = false;
                    ui.horizontal(|ui| {
                        geo_ch |= Self::slider_pct(
                            ui,
                            lang,
                            tr(lang, "Edge feather"),
                            feather,
                            1.0,
                            0.5,
                        );
                        geo_ch |= ui
                            .checkbox(flipped, tr(lang, "Flip"))
                            .on_hover_text(tr(
                                lang,
                                "Swap which side of the ellipse the adjustment affects (composes with Invert)",
                            ))
                            .changed();
                    });
                    // Rotation: the on-image grip (the knob parked above the
                    // ellipse) drags it too; the slider gives numeric entry.
                    // -179.9: clamp_geometry normalizes to (-180, 180], so a
                    // drag to the very left snapped the knob to +180 — the
                    // ellipse never moved, the knob teleported (L15-6; the
                    // hue-slider rule).
                    geo_ch |= Self::slider(
                        ui,
                        lang,
                        tr(lang, "Angle"),
                        angle,
                        -179.9,
                        180.0,
                        0.0,
                    );
                    if geo_ch {
                        self.overlay.overlay_stale = true;
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
                if autoshade::render::geometry_raster_path(&self.recipe.masks[i].mask).is_some() {
                    // R38: two aligned rows — the pair that opens a session
                    // or a full-size re-cut, then the three one-step
                    // morphologies — instead of five free-width buttons
                    // wrapping wherever the panel width fell.
                    ui.horizontal(|ui| {
                        let cell = columns(ui, 2);
                        let edit_armed = matches!(self.masks.mask_brush, Some((Some(j), _)) if j == i);
                        if toggle_in(ui, cell, edit_armed, tr(lang, "🖌 Edit raster"))
                            .on_hover_text(tr(lang, "Brush-edit this mask: paint adds, 「Erase」 removes, 「Apply」 bakes"))
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
                        if action_in(ui, cell, !self.busy, tr(lang, "Full-res refine"))
                            .on_hover_text(tr(lang, "Re-cut this mask against the FULL-resolution source (guided filter). Preview-res AI masks smear their boundary at export — this snaps it to real edges. Decodes the full-size source; takes a few seconds."))
                            .clicked()
                        {
                            self.apply_actions(vec![logic::Action::Develop(
                            logic::develop::DevelopAction::MaskRefine(i),
                        )]);
                        }
                    });
                    ui.horizontal(|ui| {
                        let cell = columns(ui, 3);
                        if action_in(ui, cell, true, tr(lang, "Soften"))
                            .on_hover_text(tr(lang, "Soften the mask boundary one step (bakes a new raster; repeat for more)"))
                            .clicked()
                        {
                            self.bake_mask_raster(
                                i,
                                |g| {
                                    let s = (g.width().max(g.height()) as f32 / 400.0).max(1.0);
                                    autoshade::render::feather_mask(g, s)
                                },
                                "mask-edit",
                            );
                        }
                        if action_in(ui, cell, true, tr(lang, "Expand"))
                            .on_hover_text(tr(lang, "Grow the selection one step (bakes a new raster)"))
                            .clicked()
                        {
                            self.bake_mask_raster(
                                i,
                                |g| {
                                    let r = (g.width().max(g.height()) / 500).max(2) as i32;
                                    autoshade::render::morph_mask(g, r)
                                },
                                "mask-edit",
                            );
                        }
                        if action_in(ui, cell, true, tr(lang, "Contract"))
                            .on_hover_text(tr(lang, "Shrink the selection one step (bakes a new raster)"))
                            .clicked()
                        {
                            self.bake_mask_raster(
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
                // --- Shapes（组件）: compose extra geometry onto this mask —
                // Lightroom's Add / Subtract / Intersect grammar. Every native
                // shape projects through the same composition spelling; only
                // Bitmap components remain a named geometry loss.
                {
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Shapes"));
                        let mode_label = |m: autoshade::recipe::MaskCombine| match m {
                            autoshade::recipe::MaskCombine::Add => tr(lang, "＋ Add"),
                            autoshade::recipe::MaskCombine::Subtract => tr(lang, "－ Subtract"),
                            autoshade::recipe::MaskCombine::Intersect => tr(lang, "∩ Intersect"),
                        };
                        egui::ComboBox::from_id_salt("comp_mode")
                            .selected_text(mode_label(self.masks.component_mode))
                            .width(96.0)
                            .show_ui(ui, |ui| {
                                for m in [
                                    autoshade::recipe::MaskCombine::Add,
                                    autoshade::recipe::MaskCombine::Subtract,
                                    autoshade::recipe::MaskCombine::Intersect,
                                ] {
                                    ui.selectable_value(&mut self.masks.component_mode, m, mode_label(m));
                                }
                            });
                        let mode = self.masks.component_mode;
                        let lin_armed = matches!(
                            self.masks.placing_mask,
                            Some((MaskKind::Linear, PlaceTarget::Component(j, _))) if j == i
                        );
                        if ui
                            .selectable_label(lin_armed, tr(lang, "Linear"))
                            .on_hover_text(tr(lang, "Drag on the image to add a linear shape to THIS mask"))
                            .clicked()
                        {
                            self.disarm_tools();
                            if !lin_armed {
                                self.masks.placing_mask =
                                    Some((MaskKind::Linear, PlaceTarget::Component(i, mode)));
                                self.status = tr(lang, "Drag on the image: the new shape composes onto this mask").into();
                            }
                        }
                        let rad_armed = matches!(
                            self.masks.placing_mask,
                            Some((MaskKind::Radial, PlaceTarget::Component(j, _))) if j == i
                        );
                        if ui
                            .selectable_label(rad_armed, tr(lang, "Radial"))
                            .on_hover_text(tr(lang, "Drag on the image to add an elliptical shape to THIS mask"))
                            .clicked()
                        {
                            self.disarm_tools();
                            if !rad_armed {
                                self.masks.placing_mask =
                                    Some((MaskKind::Radial, PlaceTarget::Component(i, mode)));
                                self.status = tr(lang, "Drag on the image: the new shape composes onto this mask").into();
                            }
                        }
                    });
                    let n_comp = self.recipe.masks[i].components.len();
                    let mut del_comp: Option<usize> = None;
                    for c in 0..n_comp {
                        ui.horizontal(|ui| {
                            let comp = &self.recipe.masks[i].components[c];
                            let icon = match comp.mode {
                                autoshade::recipe::MaskCombine::Add => "＋",
                                autoshade::recipe::MaskCombine::Subtract => "－",
                                autoshade::recipe::MaskCombine::Intersect => "∩",
                            };
                            let kind = match comp.geometry {
                                MaskGeometry::Linear { .. } => tr(lang, "Linear"),
                                MaskGeometry::Shaped { .. } => tr(lang, "Shaped gradient"),
                                MaskGeometry::Radial { .. } => tr(lang, "Radial"),
                                MaskGeometry::Bitmap { .. } => tr(lang, "Bitmap"),
                                MaskGeometry::Brush { .. } => tr(lang, "Brush"),
                                MaskGeometry::AiMask { .. } => tr(lang, "AI"),
                            };
                            let selected = self.masks.sel_component == Some(c);
                            if ui
                                .selectable_label(selected, format!("{icon} {kind}"))
                                .on_hover_text(tr(lang, "Select to drag this shape's knobs on the image (the base mask's knobs come back when deselected)"))
                                .clicked()
                            {
                                self.masks.sel_component = if selected { None } else { Some(c) };
                            }
                            let mut mode = comp.mode;
                            egui::ComboBox::from_id_salt(("comp_row_mode", c))
                                .selected_text(match mode {
                                    autoshade::recipe::MaskCombine::Add => tr(lang, "Add"),
                                    autoshade::recipe::MaskCombine::Subtract => tr(lang, "Subtract"),
                                    autoshade::recipe::MaskCombine::Intersect => tr(lang, "Intersect"),
                                })
                                .width(88.0)
                                .show_ui(ui, |ui| {
                                    ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Add, tr(lang, "Add"));
                                    ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Subtract, tr(lang, "Subtract"));
                                    ui.selectable_value(&mut mode, autoshade::recipe::MaskCombine::Intersect, tr(lang, "Intersect"));
                                });
                            if mode != comp.mode {
                                self.recipe.masks[i].components[c].mode = mode;
                                self.overlay.overlay_stale = true;
                                changed = true;
                            }
                            if ui.checkbox(&mut self.recipe.masks[i].components[c].inverted, tr(lang, "Invert")).changed() {
                                self.overlay.overlay_stale = true;
                                changed = true;
                            }
                            if glyph(ui, true, "🗑").clicked() {
                                del_comp = Some(c);
                            }
                        });
                    }
                    if let Some(c) = del_comp {
                        self.recipe.masks[i].components.remove(c);
                        self.masks.sel_component = match self.masks.sel_component {
                            Some(s) if s == c => None,
                            Some(s) if s > c => Some(s - 1),
                            other => other,
                        };
                        self.overlay.overlay_stale = true;
                        changed = true;
                    }
                    if n_comp > 0 {
                        explainer(ui, lang, tr(lang, "Shapes compose in order onto the base mask. Linear, radial, brush and AI components export to Lightroom; bitmap components have a named loss."));
                    }
                }
                // --- Range Mask（LR 范围蒙版）: refines WHERE the geometry applies —
                // final weight = geometry × range, live in preview + export + XMP.
                // Coverage invalidation happens ONCE below (after Amount): any
                // edit up to there can move the wash, and the overlay key
                // compare dedupes rebuilds — the old "range-section-only"
                // tracker went blind whenever an earlier control had already
                // set `changed` in the same frame.
                {
                    let cur = match &self.recipe.masks[i].range {
                        None => 0usize,
                        Some(RangeMask::Luminance { .. }) => 1,
                        Some(RangeMask::Color { .. }) => 2,
                    };
                    let mut sel = cur;
                    ui.horizontal(|ui| {
                        ui.label(tr(lang, "Range mask"));
                        egui::ComboBox::from_id_salt("range_kind")
                            .selected_text([tr(lang, "None"), tr(lang, "Luminance"), tr(lang, "Color")][sel])
                            .width(70.0)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut sel, 0, tr(lang, "None"));
                                ui.selectable_value(&mut sel, 1, tr(lang, "Luminance"));
                                ui.selectable_value(&mut sel, 2, tr(lang, "Color"));
                            });
                    });
                    if sel != cur {
                        // Leaving Color: the armed sampler goes too, or the
                        // canvas keeps advertising (and dispatching) a colour
                        // pick for a range that no longer exists.
                        if cur == 2 && self.tools.range_picking == Some(i) {
                            self.tools.range_picking = None;
                        }
                        self.recipe.masks[i].range = match sel {
                            // Full range = neutral start; narrow from there.
                            1 => Some(RangeMask::Luminance { lo_outer: 0.0, lo: 0.0, hi: 1.0, hi_outer: 1.0 }),
                            2 => Some(RangeMask::Color { r: 0.5, g: 0.5, b: 0.5, amount: 0.5, px: 0.5, py: 0.5 }),
                            _ => None,
                        };
                        if sel == 2 {
                            // Jump straight into sampling — a colour range without
                            // a picked colour selects nothing useful.
                            self.disarm_tools();
                            self.tools.range_picking = Some(i);
                            self.status = tr(lang, "Color range: click the color to pick in the image").into();
                        }
                        changed = true;
                    }
                    let picking_this = self.tools.range_picking == Some(i);
                    let mut want_pick = false;
                    match &mut self.recipe.masks[i].range {
                        Some(RangeMask::Luminance { lo_outer, lo, hi, hi_outer }) => {
                            ui.horizontal(|ui| {
                                ui.label(tr(lang, "Ordered bounds"));
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
                            let ch_lo = Self::slider_pct(ui, lang, tr(lang, "Lum. low"), lo, 1.0, 0.0);
                            let ch_hi = Self::slider_pct(ui, lang, tr(lang, "Lum. high"), hi, 1.0, 1.0);
                            let ch_f = Self::slider_pct(ui, lang, tr(lang, "Feather"), &mut f, 0.5, 0.1);
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
                                let label = if picking_this { tr(lang, "💧 Click in image…") } else { tr(lang, "💧 Sample") };
                                if action_in(ui, cell, true, label).on_hover_text(tr(lang, "Click the color to pick in the image (the same color at other brightnesses is also selected; clicking this button again cancels sampling)")).clicked() {
                                    want_pick = true;
                                }
                            });
                            changed |= Self::slider_pct(ui, lang, tr(lang, "Tolerance"), amount, 1.0, 0.5);
                        }
                        None => {}
                    }
                    if want_pick {
                        self.disarm_tools();
                        if !picking_this {
                            self.tools.range_picking = Some(i);
                            self.status = tr(lang, "Color range: click the color to pick in the image").into();
                        }
                    }
                }
                let m = &mut self.recipe.masks[i];
                // LR's 0..100 display track (storage stays 0..1 — slider_pct).
                changed |= Self::slider_pct(ui, lang, tr(lang, "Amount"), &mut m.amount, 1.0, 1.0);
                // Any edit up to here — geometry, range, Amount — can move the
                // coverage wash (the overlay key carries them all); the key
                // compare inside refresh_mask_overlay dedupes rebuilds, so
                // over-flagging is free. The tone sliders below are not
                // coverage and deliberately stay out.
                if changed {
                    self.overlay.overlay_stale = true;
                }
                // Lightroom's own three groups for a mask's adjustments (user
                // decision 8 of the 17-feedback triage): Tone / Detail /
                // Color, weak captions in the Color-Grading "All regions"
                // style. Twelve sliders in one unbroken column is what the
                // grouping replaces — the same reason the global panel is
                // sectioned. `Tone` is our own caption for LR's "Light" group:
                // the key "Light" already means the light THEME.
                let group = |ui: &mut egui::Ui, title: &str| sub_caption(ui, title);
                group(ui, tr(lang, "Tone"));
                changed |= Self::slider(ui, lang, tr(lang, "Exposure"), &mut m.exposure_ev, -5.0, 5.0, 0.0);
                changed |= Self::slider(ui, lang, tr(lang, "Contrast"), &mut m.contrast, -100.0, 100.0, 0.0);
                changed |= Self::slider(ui, lang, tr(lang, "Highlights"), &mut m.highlights, -100.0, 100.0, 0.0);
                changed |= Self::slider(ui, lang, tr(lang, "Shadows"), &mut m.shadows, -100.0, 100.0, 0.0);
                changed |= Self::slider(ui, lang, tr(lang, "Whites"), &mut m.whites, -100.0, 100.0, 0.0);
                changed |= Self::slider(ui, lang, tr(lang, "Blacks"), &mut m.blacks, -100.0, 100.0, 0.0);
                // Texture / Clarity / Dehaze come out of the old
                // 「More (XMP/Lightroom only)」 fold: R22-3 put all three ON the
                // engine (render::apply_masks), so that title became a lie and
                // the fold merely hid three working sliders one level deep.
                group(ui, tr(lang, "Detail"));
                changed |= Self::slider(ui, lang, tr(lang, "Texture"), &mut m.texture, -100.0, 100.0, 0.0);
                changed |= Self::slider(ui, lang, tr(lang, "Clarity"), &mut m.clarity, -100.0, 100.0, 0.0);
                changed |= Self::slider(ui, lang, tr(lang, "Dehaze"), &mut m.dehaze, -100.0, 100.0, 0.0);
                // R23-1b. SIGNED, unlike the global 0..150 Sharpening: this is
                // ACR's local band, and its negative half (soften) is what
                // throws a background back — the tooltip says so, because the
                // two controls share a name and not a range.
                changed |= Self::slider_hinted(
                    ui,
                    lang,
                    tr(lang, "Sharpness"),
                    &mut m.sharpness,
                    -100.0,
                    100.0,
                    0.0,
                    tr(lang, "Sharpens inside the mask when positive and SOFTENS when negative (the global 「Sharpening」 has no negative half). Same radius as the global one."),
                );
                changed |= Self::slider(ui, lang, tr(lang, "Noise Reduction"), &mut m.noise_reduction, 0.0, 100.0, 0.0);
                group(ui, tr(lang, "Color"));
                changed |= Self::slider(ui, lang, tr(lang, "Saturation"), &mut m.saturation, -100.0, 100.0, 0.0);
                // R23-1b: a hue ROTATION of everything the mask covers — the
                // one colour move the global 8-band mixer cannot make, because
                // that one acts on a band everywhere in the frame.
                changed |= Self::slider_hinted(
                    ui,
                    lang,
                    tr(lang, "Hue shift"),
                    &mut m.hue,
                    -100.0,
                    100.0,
                    0.0,
                    tr(lang, "Rotates every colour inside the mask (±100 = ±30°) — unlike the global color mixer, which moves one color band across the whole frame."),
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
                    "A RELATIVE warm/cool shift (±100) around a fixed 5500 K anchor, not absolute Kelvin: this value renders like ≈ {k} K. The global 「Temp (K)」 is absolute and anchored at this photo's as-shot value — a different axis.",
                    &[("k", &temp_k)],
                );
                changed |= Self::slider_hinted(ui, lang, tr(lang, "Temp shift"), &mut m.temperature, -100.0, 100.0, 0.0, &temp_hint);
                changed |= Self::slider_hinted(
                    ui,
                    lang,
                    tr(lang, "Tint shift"),
                    &mut m.tint,
                    -100.0,
                    100.0,
                    0.0,
                    tr(lang, "A RELATIVE green/magenta shift (±100) inside the mask — positive goes magenta. Unlike the global 「Tint」 it is not solved against the photo's as-shot tint."),
                );
                // Lightroom's mask "Color" swatch (`crs:LocalToningSaturation`
                // / `LocalToningHue`), as its two numbers: how strongly, then
                // toward which hue. Last in the group because the render
                // applies it last (`render::apply_masks`); engine-rendered and
                // carried by the sidecar, and the hue does nothing at 0.
                changed |= Self::slider_hinted(
                    ui,
                    lang,
                    tr(lang, "Color tint"),
                    &mut m.toning_amount,
                    0.0,
                    100.0,
                    0.0,
                    tr(lang, "Lightroom's mask Color swatch: shifts every colour inside the mask, neutral and coloured alike, toward 「Color tint hue」 — most in the midtones, never brighter or darker. Unlike 「Hue shift」, which turns colours by an angle."),
                );
                changed |= Self::slider_hinted(
                    ui,
                    lang,
                    tr(lang, "Color tint hue"),
                    &mut m.toning_hue,
                    0.0,
                    360.0,
                    0.0,
                    tr(lang, "The hue (0–360°) 「Color tint」 pushes toward, on Lightroom's own wheel. Does nothing until 「Color tint」 is above 0."),
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
                            egui::RichText::new(tr(lang, "carries reverse-fit recolour (written to XMP as Temp/Tint)"))
                                .weak()
                                .small(),
                        );
                        if action_in(ui, cell, true, tr(lang, "↺ Clear"))
                            .on_hover_text(tr(lang, "Drop this mask's per-channel recolour gains (one Ctrl+Z to undo)"))
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
                group(ui, tr(lang, "Curve"));
                changed |= self.curve_editor_for(ui, CurveTarget::Mask(i));
            } else if n_masks == 0 {
                explainer(ui, lang, tr(lang, "Lightroom-style local adjustments: add a gradient to darken the sky, a radial to brighten the subject. AI Analyze also writes to this list."));
            } else {
                // Masks exist, none is selected — one click on the selected
                // row (it toggles) lands here, and the section then showed a
                // list with no adjustments under it and no reason why.
                ui.add_space(SPACE_XS);
                ui.label(
                    egui::RichText::new(tr(lang, "Select a mask above to edit its adjustments"))
                        .weak()
                        .small(),
                );
            }
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
            let lin_armed = matches!(self.masks.placing_mask, Some((MaskKind::Linear, PlaceTarget::NewMask)));
            if toggle_in(ui, cell, lin_armed, tr(lang, "＋ Linear")).on_hover_text(tr(lang, "Drag on the image: start = fully-applied side, end = unaffected side (Shift = horizontal/vertical)")).clicked() {
                self.disarm_tools();
                if !lin_armed {
                    self.masks.placing_mask = Some((MaskKind::Linear, PlaceTarget::NewMask));
                    self.status = tr(lang, "Drag on the image to draw a linear gradient (start fully applied → end unaffected; Shift = axis lock)").into();
                }
            }
            let rad_armed = matches!(self.masks.placing_mask, Some((MaskKind::Radial, PlaceTarget::NewMask)));
            if toggle_in(ui, cell, rad_armed, tr(lang, "＋ Radial")).on_hover_text(tr(lang, "Drag on the image to draw an elliptical area")).clicked() {
                self.disarm_tools();
                if !rad_armed {
                    self.masks.placing_mask = Some((MaskKind::Radial, PlaceTarget::NewMask));
                    self.status = tr(lang, "Drag on the image to draw a radial (elliptical) area").into();
                }
            }
            // LR's most-used local tool, finally: free-form brush →
            // Bitmap mask (the raster carrier + bilinear sampler + cache
            // all pre-existed; this wires the paint canvas into
            // recipe.masks — see start_mask_brush / commit_mask_brush).
            let brush_armed = matches!(self.masks.mask_brush, Some((None, _)));
            if toggle_in(ui, cell, brush_armed, tr(lang, "🖌 Brush"))
                .on_hover_text(tr(lang, "Paint a free-form mask (drag the 「Brush size」 slider, or press [ / ]); 「Apply」 bakes it into a new mask"))
                .clicked()
            {
                if brush_armed {
                    self.disarm_tools();
                } else if self.base_preview.is_some() {
                    self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::MaskBrush(None),
                    )]);
                }
            }
        });
        // Brush-session controls (create OR raster-edit): erase toggle +
        // the bake/cancel pair. Lives up here so it is visible whichever
        // row armed the session.
        if let Some((target, erase)) = self.masks.mask_brush {
            ui.horizontal(|ui| {
                let cell = columns(ui, 3);
                if toggle_in(ui, cell, erase, tr(lang, "Erase"))
                    .on_hover_text(tr(lang, "Strokes remove from the selection instead of adding"))
                    .clicked()
                {
                    self.masks.mask_brush = Some((target, !erase));
                }
                if primary_in(ui, cell, true, tr(lang, "✓ Apply")).clicked() {
                    self.commit_mask_brush();
                }
                if action_in(ui, cell, true, tr(lang, "✕ Cancel")).clicked() {
                    self.disarm_tools();
                }
            });
            // #9: the radius belongs AT THE TOOL. This is the same
            // `self.masks.brush` the Retouch panel's slider drives (and the
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
            let can_seg = !self.busy && self.base_preview.is_some() && has_helper;
            let cell = columns(ui, 2);
            let missing = tr(lang,
                "This feature needs AutoShade's Python helper, which this download does not include. Install the full package, or (advanced) set AUTOSHADE_SEGMENT_SCRIPT to the path of segment.py",
            );
            // 🤖 on BOTH (#4): one prefix marks every AI verb app-wide, and
            // 「☁ AI select sky」 was the lone exception — a weather glyph
            // where its twin one row over already wore the robot. The
            // tooltip carries the AI-panel cross-reference, but only on the
            // arm that CAN run: a missing-sidecar message must stay about
            // the missing sidecar.
            if action_in(ui, cell, can_seg, tr(lang, "🤖 Select subject"))
                // R29 B4: the model, its dependencies and its download size
                // all changed with the user's ruling of 2026-08-21, and the
                // FALLBACK is named because a machine without torchvision
                // gets a materially softer mask and would otherwise have no
                // way to know which model drew it.
                .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                    "BiRefNet salient-subject segmentation → bitmap mask (Python helper: pip install torchvision timm einops; first run auto-downloads a ~444MB model; without them it falls back to U²-Net / pip install rembg, whose edges are softer)",
                )) } else { missing.to_string() })
                .clicked()
            {
                self.apply_actions(vec![logic::Action::Develop(
                    logic::develop::DevelopAction::Segment { target: "subject", label: "Subject" },
                )]);
            }
            if action_in(ui, cell, can_seg, tr(lang, "🤖 Select sky"))
                .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                    "OneFormer-ADE20K sky segmentation → bitmap mask (Python helper: pip install transformers; first run auto-downloads a ~880MB model)",
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
                    "No masks yet — draw one with the tools above; AI Analyze adds its own too.",
                ))
                .weak()
                .small(),
            );
        }
    }
}
