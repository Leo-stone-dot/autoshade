//! Develop panel: sliders, sections, variant strip.

use crate::*;

// The section ● predicate and the table it reads (R25 P0) — see
// `develop_panel` and `dev_lens`.
use autoshade::advisor::catalogue::{family_is_active, CONTROL_FAMILIES};
use crate::sections::{section_active, SECTIONS};


impl AutoShadeApp {









    pub(crate) fn develop_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang; // Copy — never borrows self, safe inside egui closures.
        let mut changed = false;
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, K_DEVELOP_DEVELOP), false, |_| {});
        // OUTSIDE the editable gate below, deliberately (#4 review point): the
        // histogram is a READOUT. Its only interaction is the pair of clipping
        // triangles, whose click reaches `toggle_clipping` — that writes
        // `show_clipping` + the overlay texture and, on a fresh open with no
        // retained frame, `dirty`. None of those is EDIT state: no recipe field
        // is read into a widget or written back, so the L15-2 input-loss class
        // (edit photo A's sliders while B lands) cannot arise here. The worst
        // mid-open outcome is one preview build whose frame `finish_redevelop`
        // then discards by its base+recipe identity check. Greying the tone
        // readout during every open would be the bigger regression.
        self.panel_view(&mut Vec::new()).histogram_ui(ui);
        ui.add_space(SPACE_SM);

        // Mid-open (decode in flight) these controls would edit the STASHED
        // photo A while B lands and replaces the whole recipe — silent input
        // loss (L15-2). Busy alone must NOT gate here: a 600 s analyze keeps
        // the panel live; only the open transition freezes it.
        // The AI area carries its OWN copy of this gate (panels/ai.rs) — it
        // used to ride inside this closure as `dev_ai`.
        let editable = !self.workspace.open().open_in_flight;
        ui.add_enabled_ui(editable, |ui| {
        // Lightroom-style grouping: a wall of 16 sliders scans terribly; four
        // titled sections (tone open, the rest by activity) scan at a glance.
        // A section whose values are non-neutral shows a ● so a collapsed
        // active adjustment is never invisible. Flags are snapshot up front —
        // Copy bools, so no borrow spans the section closures (E0500).
        //
        // The section predicates are DERIVED from the control registry's families
        // (R25 P0): each was a hand-written field tuple that had to be widened
        // by hand whenever its section gained a control, and R22 #16 had
        // already found four of them one field short. `family_is_active` reads
        // the same table the AI's tool plan is built from, so a control that
        // joins a family joins its dot.
        // The section table (`sections.rs`) owns the display order, the
        // family set each ● reads, and which sections are registered custom
        // widgets. This loop is the only place the panel enumerates them.
        for section in SECTIONS {
            let active = section_active(&self.editor.doc().recipe, section);
            changed |= (section.draw)(self, ui, active);
        }
        });

        if changed {
            self.editor.doc_mut().recipe.clamp();
            self.editor.session_mut().dirty = true;
        }
    }

    /// The variant strip (版本条): one card per rendition — 原片 / AI 生成 /
    /// 反推 — with a live developed thumbnail. Click a card to switch (lossless;
    /// each variant keeps its own base + recipe), × to drop one. This is the
    /// selector that makes an AI develop a first-class, non-reverting version.
    pub(crate) fn variant_strip(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let accent = self.workspace.ui().theme.colors().accent_text; // Copy — safe in closures
        // ONE deferred action for the whole strip (R24-5): the five separate
        // `Option`s this replaced were dispatched by an `else if` chain whose
        // exclusivity was a property of the chain; a single value makes it a
        // property of the TYPE, and it is the same value the edit-state list
        // hands to the same owner. Last writer wins where the chain had a
        // fixed priority — reachable only if two widgets were clicked in one
        // frame, which one pointer cannot do.
        let mut act: Option<VariantAction> = None;
        let busy = self.workspace.busy(); // Copy — read before the card closures borrow self
        // The card column is centered by an EXPLICIT top pad, not by
        // cross-align: Align::Center positioned the row before its content
        // grew — egui seeds a horizontal row at interact_size.y, centered
        // that 26 px stub in the panel, and the 84 px column then grew
        // downward from it (37 px of air above, 21 px of column OVERFLOW
        // below the panel; R14 user report). The column height is knowable
        // up front — thumb + item gap + label row (egui rows are at least
        // interact_size.y tall) — so the pad is exact and the geometry test
        // asserts the air above equals the air below.
        // Named STRIP_ to stay clear of model.rs's gallery THUMB_H (40.0),
        // which this local used to SHADOW inside variant_strip — one glob
        // import away from a silent geometry swap in either direction.
        const STRIP_THUMB_H: f32 = 52.0;
        let card_h = STRIP_THUMB_H + ui.spacing().item_spacing.y + ui.spacing().interact_size.y;
        #[cfg(test)]
        {
            self.seams.strip_row_rect = Some(ui.max_rect());
        }
        ui.add_space(((ui.available_height() - card_h) / 2.0).max(0.0));
        // LAYOUT INVARIANT (probed, R14+R15): egui places the scroll area
        // centered by its 26 px DECLARED seed against the row's height at
        // placement time, then grows content DOWNWARD — (current−26)/2 of
        // offset. Keeping every child before the scroll area at ZERO height
        // (pure horizontal spacing) pins that offset at exactly 0, which the
        // geometry test asserts. (Every cross-align variant re-broke it,
        // measured 37/29/11.8 px — all equal to (row_height−26)/2.) The
        // title and divider therefore do not participate in layout at all
        // (R16): they are PAINTED at rects computed from the same knowns as
        // the pad — the one way to center them on the card column without
        // re-breaking the row.
        let row_top = ui.cursor().top();
        let strip_left = ui.max_rect().left();
        let title_galley = ui.painter().layout_no_wrap(
            tr(lang, K_DEVELOP_VARIANTS__2).to_owned(),
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().strong_text_color(),
        );
        let title_w = title_galley.size().x;
        let title_pos = egui::pos2(
            strip_left + SPACE_SM,
            row_top + (card_h - title_galley.size().y) / 2.0,
        );
        #[cfg(test)]
        {
            self.seams.strip_title_rect =
                Some(egui::Rect::from_min_size(title_pos, title_galley.size()));
        }
        ui.painter().galley(title_pos, title_galley, ui.visuals().strong_text_color());
        // The divider, at full card height — the stock separator filled the
        // 26 px seed, a stub over the card's top third.
        let sep_x = strip_left + SPACE_SM + title_w + SPACE_SM + 3.0;
        ui.painter().vline(
            sep_x,
            egui::Rangef::new(row_top, row_top + card_h),
            ui.visuals().widgets.noninteractive.bg_stroke,
        );
        ui.horizontal(|ui| {
            // Horizontal footprint of the PAINTED title + divider: width
            // only, zero height (the invariant above).
            ui.add_space(SPACE_SM + title_w + SPACE_SM + 6.0 + SPACE_SM);
            egui::ScrollArea::horizontal().show(ui, |ui| {
                ui.horizontal(|ui| {
                    for i in 0..self.editor.doc().variants.len() {
                        let active = i == self.editor.doc().active;
                        let kind = self.editor.doc().variants[i].kind;
                        ui.vertical(|ui| {
                            // Developed thumbnail (or a placeholder until the
                            // variant has been developed once).
                            let resp = if let Some(t) = &self.editor.doc().variants[i].thumb {
                                let s = t.size_vec2();
                                let h = STRIP_THUMB_H;
                                let w = (s.x / s.y.max(1.0) * h).clamp(30.0, 104.0);
                                let (rect, resp) =
                                    ui.allocate_exact_size(egui::vec2(w, h), egui::Sense::click());
                                let uv = egui::Rect::from_min_max(
                                    egui::pos2(0.0, 0.0),
                                    egui::pos2(1.0, 1.0),
                                );
                                // Card chrome tokens (L15-1): the glow is
                                // the chrome accent at low alpha and its
                                // radius is DERIVED — the literal
                                // 0xc9a14a/7.0 copy silently detached from
                                // any theme change.
                                const CARD_RADIUS: f32 = 4.0;
                                const GLOW_EXPAND: f32 = 3.0;
                                let ring = self.workspace.ui().theme.colors().accent_fill;
                                if active {
                                    ui.painter().rect_filled(
                                        rect.expand(GLOW_EXPAND),
                                        CARD_RADIUS + GLOW_EXPAND,
                                        egui::Color32::from_rgba_unmultiplied(
                                            ring.r(),
                                            ring.g(),
                                            ring.b(),
                                            46,
                                        ),
                                    );
                                }
                                ui.painter().image(t.id(), rect, uv, egui::Color32::WHITE);
                                if active {
                                    ui.painter().rect_stroke(
                                        rect,
                                        CARD_RADIUS,
                                        egui::Stroke::new(2.0, ring),
                                        egui::StrokeKind::Middle,
                                    );
                                }
                                resp
                            } else {
                                ui.add_sized([64.0, STRIP_THUMB_H], egui::Button::new("…"))
                            };
                            if resp
                                .on_hover_text(tr(
                                    lang,
                                    K_DEVELOP_CLICK_TO_SWITCH_TO_THIS_VARIANT_LOSSLESS_CTRL_S,
                                ))
                                .clicked()
                            {
                                act = Some(VariantAction::Switch(i));
                            }
                            ui.horizontal(|ui| {
                                let label = egui::RichText::new(tr(lang, kind.label())).small();
                                let label = ui.label(if active { label.strong().color(accent) } else { label });
                                // The immutability rule, said where the card
                                // is (2026-09-13): a ✨ card never takes an
                                // edit — the first one continues on a new ✎
                                // card — so the label says so before the
                                // user wonders where their slider went.
                                if kind == VariantKind::Generated {
                                    label.on_hover_text(tr(
                                        lang,
                                        K_DEVELOP_A_GENERATED_IMAGE_STAYS_AS_GENERATED_THE_FIRST_E,
                                    ));
                                } else if kind == VariantKind::Stacked {
                                    // The ▦ card (v1.5.0): what it is and what
                                    // a .xmp from it can carry, said where the
                                    // card is.
                                    label.on_hover_text(tr(
                                        lang,
                                        K_DEVELOP_SEVERAL_FRAMES_OF_THIS_SCENE_MERGED_INTO_THEIR_O,
                                    ));
                                } else if kind == VariantKind::Denoised {
                                    // The ◈ card (2026-09-15): what it is and
                                    // what a .xmp from it can carry, said where
                                    // the card is.
                                    label.on_hover_text(tr(
                                        lang,
                                        K_DEVELOP_THE_NEGATIVE_AI_DENOISED_INTO_ITS_OWN_MASTER_DEV,
                                    ));
                                }
                                // The card's NAME (R24-3), which until now
                                // had a persistence path (variants.json,
                                // R24-2) and no producer. The ACTIVE card
                                // edits it in place — the version rows'
                                // discipline, keyed by the card's own id so a
                                // background push or delete cannot land the
                                // text on a different card; a background card
                                // only DISPLAYS the name it has (offering a
                                // 「Name…」 placeholder on every card would
                                // widen the whole strip for an affordance
                                // that acts on one of them).
                                let card_id = self.editor.doc().variants[i].id.clone();
                                let named = self.editor.doc().variants[i].name.clone();
                                let editing = active
                                    && self.workspace.cards().variant_name_buf.as_ref().is_some_and(|(p, id, ..)| {
                                        *id == card_id
                                            && Some(p.as_path()) == self.editor.photo().src_path.as_deref()
                                    });
                                if editing {
                                    let resp = {
                                        let buf =
                                            &mut self.workspace.cards_mut().variant_name_buf.as_mut().expect("checked just above").3;
                                        ui.add(
                                            // Keyed by the CARD's id, like
                                            // the buffer: an index key made
                                            // egui hand the box a new
                                            // identity (and drop the caret)
                                            // the moment an async push
                                            // renumbered the strip.
                                            egui::TextEdit::singleline(buf)
                                                .id(ui.make_persistent_id((
                                                    "variant_name",
                                                    card_id.as_str(),
                                                )))
                                                .desired_width(90.0)
                                                .hint_text(tr(lang, K_DEVELOP_NAME)),
                                        )
                                    };
                                    // The +1 boundary: this box's own
                                    // lost-focus commit (Enter counts as
                                    // losing focus in egui), and the frame it
                                    // appears in has to claim the caret — the
                                    // click that opened it landed on the
                                    // label that used to be here.
                                    #[cfg(test)]
                                    {
                                        self.seams.strip_name_rect = Some(resp.rect);
                                    }
                                    if resp.lost_focus() {
                                        self.commit_variant_name_buf();
                                        self.workspace.cards_mut().variant_name_buf = None;
                                    } else if !resp.has_focus() {
                                        resp.request_focus();
                                    }
                                } else if active {
                                    let shown = match &named {
                                        Some(n) => egui::RichText::new(format!("「{n}」")).small(),
                                        None => egui::RichText::new(tr(lang, K_DEVELOP_NAME__2).to_string())
                                            .small()
                                            .weak(),
                                    };
                                    let resp = ui
                                        .add(egui::Label::new(shown).sense(egui::Sense::click()))
                                        .on_hover_text(tr(lang, K_DEVELOP_NAME_THIS_VARIANT));
                                    #[cfg(test)]
                                    {
                                        self.seams.strip_name_rect = Some(resp.rect);
                                    }
                                    if resp.clicked() {
                                        act = Some(VariantAction::Rename(card_id.clone()));
                                    }
                                } else if let Some(n) = &named {
                                    ui.label(egui::RichText::new(format!("「{n}」")).small().weak());
                                }
                                if let Some(a) = self.variant_card_buttons(
                                    ui,
                                    i,
                                    active,
                                    busy,
                                    VariantSurface::Strip,
                                ) {
                                    act = Some(a);
                                }
                            });
                            #[cfg(test)]
                            if i == 0 {
                                self.seams.strip_card_rect = Some(ui.min_rect());
                            }
                        });
                        ui.add_space(SPACE_MD);
                    }
                });
            });
        });
        if let Some(a) = act {
            self.dispatch_variant_action(a, ui.ctx());
        }
    }

    /// The card's ACTION buttons — ＋ archive, ▣ apply-to-Original, ✕ delete
    /// — for whichever surface is drawing the card (R24-5).
    ///
    /// Extracted from `variant_strip` verbatim when the Versions section's
    /// edit-state list grew the same three buttons: a card action must mean
    /// one thing and be implemented once, or the two lists answer 「what does
    /// ✕ do here」 differently the first time either is touched. The strip
    /// keeps the rename box (see [`VariantSurface`]).
    ///
    /// Returns the deferred action; the caller performs it through
    /// `dispatch_variant_action` after the layout closure has released `self`.
    // `surface` decides only which headless seam records the row, so it is
    // genuinely unused in a release build — the alternative (two near-copies
    // of this function) is the thing the extraction exists to remove.
    #[cfg_attr(not(test), allow(unused_variables))]
    pub(crate) fn variant_card_buttons(
        &mut self,
        ui: &mut egui::Ui,
        i: usize,
        active: bool,
        busy: bool,
        surface: VariantSurface,
    ) -> Option<VariantAction> {
        let lang = self.lang;
        let kind = self.editor.doc().variants[i].kind;
        let mut act: Option<VariantAction> = None;
        // Which glyphs this row actually laid out — the list surface's test
        // seam, since it has no rects of its own to assert on.
        #[cfg(test)]
        let mut drawn: Vec<&str> = Vec::new();
        // R24-2 (user decision ②, in place of save-as-you-go snapshots): a
        // one-click archive entry point ON the card whose develop it
        // snapshots. Active card only — the button acts on the LIVE canvas,
        // and offering it on a background card would promise a snapshot of
        // something else. A row-height glyph square in the existing label row
        // (R38): the row is already interact_size.y tall, so the strip's
        // card-height arithmetic (and its geometry test) is untouched.
        // Inert (visibly, not silently) while a photo is still opening — the
        // canvas then still holds the OUTGOING photo's develop, and the
        // snapshot would land it in the incoming photo's store. Same rule the
        // Versions section applies to its own copy.
        if active {
            #[cfg(test)]
            drawn.push("＋");
            if glyph(ui, !busy, "＋")
                .on_hover_text(tr(lang, K_DEVELOP_SNAPSHOT_THIS_CARD_S_DEVELOP_ONLY_AS_A_NUMBERED))
                .clicked()
            {
                act = Some(VariantAction::SaveVersion);
            }
        }
        // R24-3 (#7) 「apply to Original」, on the ACTIVE card for the same
        // reason ＋ is: it copies the LIVE canvas. Two concepts the hover keeps
        // apart — it overwrites the ▣ Original CARD's develop parameters (its
        // pixels stay, this card stays), and Ctrl+S is what afterwards makes
        // that card's develop the photo's SAVED develop.
        // A pixel-state card shows the button DISABLED with the reason rather
        // than hiding it (the R24-2 save_version judgement, same remedy): its
        // look is in its raster, so there is nothing parametric to copy.
        // Hidden entirely when this card IS the negative, or when the strip
        // has no Original card to apply onto.
        if active && kind != VariantKind::Original && self.original_index().is_some() {
            let can = kind.is_source_based();
            let hover = if can {
                tr(lang, K_DEVELOP_COPY_THIS_VARIANT_S_DEVELOP_ONTO_THE_ORIGINAL_CA)
            } else if kind == VariantKind::Edited {
                tr(lang, K_DEVELOP_THIS_CARD_S_DEVELOP_IS_TUNED_OVER_AI_GENERATED_P)
            } else {
                tr(lang, K_DEVELOP_A_GENERATED_VARIANT_S_LOOK_LIVES_IN_ITS_PIXELS_T)
            };
            let resp = glyph(ui, can && !busy, "▣").on_hover_text(hover);
            #[cfg(test)]
            drawn.push(if can && !busy { "▣" } else { "▣(off)" });
            #[cfg(test)]
            if surface == VariantSurface::Strip {
                // The STRIP owns this seam: a full frame draws both surfaces,
                // and letting the list overwrite it would silently retarget
                // every existing assertion at the other widget.
                self.seams.strip_apply = Some((resp.rect, can && !busy));
            }
            if resp.clicked() {
                act = Some(VariantAction::Apply(i));
            }
        }
        // Any variant except the sole Original can be dropped.
        // ✕ — the ONE close/cancel glyph app-wide (the old bare × was the
        // fourth variant of it). ARMED (R24-4): the first click asks, the
        // second deletes — the button says which, because a deleted card
        // cannot be brought back (unlike a version, whose number the registry
        // keeps burned rather than the card recoverable).
        let armed = self.workspace.cards().variant_delete_confirm == Some(i);
        if self.editor.doc().variants.len() > 1
            && kind != VariantKind::Original
            && glyph(ui, true, if armed { "✕?" } else { "✕" })
                .on_hover_text(if armed {
                    tr(lang, K_DEVELOP_CLICK_AGAIN_TO_DELETE_THIS_VARIANT_IT_CANNOT_BE)
                } else {
                    tr(lang, K_DEVELOP_DELETE_THIS_VARIANT)
                })
                .clicked()
        {
            act = Some(VariantAction::Delete(i));
        }
        #[cfg(test)]
        if self.editor.doc().variants.len() > 1 && kind != VariantKind::Original {
            drawn.push(if armed { "✕?" } else { "✕" });
        }
        #[cfg(test)]
        if surface == VariantSurface::List {
            self.seams.edit_list_actions.push(format!("{i}:{}", drawn.join("")));
        }
        act
    }

    /// One capability table: the renderer draws each control through its own
    /// implementation and reports a data change (id + value); the registry
    /// applies it. The panel does not know a control by name.
    pub(crate) fn capability_rows(&mut self, ui: &mut egui::Ui, table: &[&dyn crate::controls::ControlWidget]) -> bool {
        crate::panels::develop_forms::render_and_apply(ui, self.lang, &mut self.editor.doc_mut().recipe, table)
    }

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
                    let raw_source = self.source_is_raw();
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
                let denoise_raw = self.denoise_uses_raw();
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
                        self.apply_actions(vec![logic::Action::Develop(
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
                        self.apply_actions(vec![logic::Action::Develop(
                            logic::develop::DevelopAction::Stack(extra),
                        )]);
                    }
                });
            });
            false
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
                        self.set_crop_mode(on);
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

    /// Does the Local Masks header earn its ●? Exactly the rule the mask ROWS
    /// below use — [`mask_active`], the engine's own non-neutrality test plus the
    /// eye — applied over the list. The header used to carry `n_masks > 0`
    /// instead (R22 #16), which lit the dot for a list of muted or parked masks
    /// and said nothing the「Local Masks ({n})」count did not already say.
    pub(crate) fn masks_section_active(&self) -> bool {
        // R33 §G: the field is in this section and it MOVES PIXELS, so the
        // section's dot has to see it. `renderable()` is the engine's own
        // question, asked here for the same reason `mask_active` is — one
        // rule, so the dot and the render cannot disagree.
        self.editor.doc().recipe.colour_field.as_ref().is_some_and(|f| f.renderable())
            || self.editor.doc().recipe.masks.iter().any(mask_active)
    }



    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    pub(crate) fn dev_versions(&mut self, ui: &mut egui::Ui, _active: bool) -> bool {
        let lang = self.lang;

        // --- 版本: recipe snapshots ≈ LR virtual copies (gap batch G) --------
        // #14b, group 4 — DELIVERY (versions + export): what leaves the app,
        // as opposed to what is being edited. This boundary gets the same
        // fence + hairline + breather rhythm the other two group fences use
        // (Detail, Local Masks) — the rhythm is completed here, not changed:
        // Versions previously opened with a bare sibling-section fence while
        // being the head of a different KIND of group.
        group_caption(ui, tr(lang, K_DEVELOP_VERSIONS_EXPORT));
        let n_ver = self.workspace.cards().versions.len();
        let n_ver_s = n_ver.to_string();
        // Deferred like the strip's, and dispatched through the same owner
        // after the section's closures release `self` (R24-5).
        let mut act: Option<VariantAction> = None;
        card_fold(ui, egui::CollapsingHeader::new(section_title(&trf(lang, K_DEVELOP_VERSIONS, &[("n", &n_ver_s)]), n_ver > 0))
            .id_salt("sec_versions")
            .default_open(false)
            , |ui| {
                // Clearing the list on open (see `open_path`) already stops a
                // STALE row from acting on the incoming photo. This is the
                // second half: while a photo is still decoding, "Save as
                // version" would snapshot the OUTGOING photo's canvas — the
                // fresh recipe has not landed yet — into the incoming photo's
                // develop. Nothing here is meaningful until the open settles,
                // so the whole section goes inert rather than each button
                // carrying its own guard.
                ui.add_enabled_ui(!self.workspace.busy(), |ui| {
                // R24-4 (#1, phase 2 — minimal): ONE place that answers
                // "what edit states does this photo have". A photo is one
                // negative + N variant CARDS + one version history, so the
                // cards are listed first and the numbered snapshots under
                // them, in the vocabulary the data model now uses: the
                // taxonomy's binary (parametric vs pixel-state) and the
                // orthogonal "hangs off a baked master" attribute are the
                // two notes a row can carry (R24-1).
                //
                // R24-5: the rows are no longer a read-only view — each one
                // carries the card's own ＋ / ▣ / ✕, drawn by the SAME owner
                // the strip draws them with (`variant_card_buttons`), so the
                // two surfaces cannot come to mean different things. Clicking
                // a row switches to that card, exactly as clicking its
                // thumbnail does. Renaming stays on the strip on purpose: two
                // `TextEdit`s over one buffer fight for the caret every frame
                // (see `VariantSurface`).
                //
                // The buttons take a SECOND line rather than joining the
                // label row. A 320 px panel already holds a kind label, a
                // 「name」 and a vocabulary note; three more widgets on that
                // line is how a side panel starts growing per frame, which is
                // a defect this repo has shipped twice (R19, R22-3) and now
                // has width-stability tests for.
                let n_var = self.editor.doc().variants.len();
                let busy = self.workspace.busy();
                #[cfg(test)]
                {
                    self.seams.edit_list_rows.clear();
                    self.seams.edit_list_actions.clear();
                }
                if n_var > 0 {
                    let accent = self.workspace.ui().theme.colors().accent_text;
                    ui.label(
                        egui::RichText::new(trf(
                            lang,
                            K_DEVELOP_VARIANTS,
                            &[("n", &n_var.to_string())],
                        ))
                        .weak()
                        .small(),
                    );
                    let active_i = self.editor.doc().active;
                    for i in 0..n_var {
                        // Read the three facts the row shows BEFORE the layout
                        // closure borrows `self` (Variant is not Clone, and
                        // making it so for a display row would be the wrong
                        // fix — the card's texture handle rides on it).
                        let (kind, name, on_pixels) = {
                            let v = &self.editor.doc().variants[i];
                            (v.kind, v.name.clone(), v.origin.is_some())
                        };
                        #[cfg(test)]
                        self.seams.edit_list_rows.push(format!("variant:{}", kind.store_str()));
                        let hit = ui
                            .horizontal(|ui| {
                                let label = egui::RichText::new(tr(lang, kind.label())).small();
                                ui.label(if i == active_i {
                                    label.strong().color(accent)
                                } else {
                                    label
                                });
                                if let Some(n) = &name {
                                    ui.label(egui::RichText::new(format!("「{n}」")).small());
                                }
                                if i == active_i {
                                    ui.label(
                                        egui::RichText::new(tr(lang, K_DEVELOP_CURRENT))
                                            .weak()
                                            .small(),
                                    );
                                }
                                if kind == VariantKind::Edited {
                                    ui.label(
                                        egui::RichText::new(tr(lang, K_DEVELOP_DEVELOP_OVER_AI_PIXELS_NO_XMP))
                                            .weak()
                                            .small(),
                                    );
                                } else if !kind.is_source_based() {
                                    ui.label(
                                        egui::RichText::new(tr(lang, K_DEVELOP_PIXEL_STATE_NO_XMP))
                                            .weak()
                                            .small(),
                                    );
                                } else if on_pixels {
                                    ui.label(
                                        egui::RichText::new(tr(lang, K_DEVELOP_ON_BAKED_PIXELS))
                                            .weak()
                                            .small(),
                                    );
                                }
                            })
                            .response
                            .interact(egui::Sense::click());
                        // The whole label row is the switch target — the same
                        // gesture the strip's thumbnail offers, on the row that
                        // names the card. A BACKGROUND row only: re-selecting
                        // the active card would run a switch (and its commit)
                        // for no change.
                        if i != active_i
                            && hit
                                .on_hover_text(tr(
                                    lang,
                                    K_DEVELOP_CLICK_TO_SWITCH_TO_THIS_VARIANT_LOSSLESS,
                                ))
                                .clicked()
                        {
                            act = Some(VariantAction::Switch(i));
                        }
                        // The card's own actions, one owner with the strip.
                        // Indented under the label so the two lines read as
                        // one row rather than as two entries.
                        let has_actions =
                            i == active_i || (n_var > 1 && kind != VariantKind::Original);
                        if has_actions {
                            ui.horizontal(|ui| {
                                ui.add_space(SPACE_LG);
                                if let Some(a) = self.variant_card_buttons(
                                    ui,
                                    i,
                                    i == active_i,
                                    busy,
                                    VariantSurface::List,
                                ) {
                                    act = Some(a);
                                }
                            });
                        }
                    }
                }
                sub_caption(ui, tr(lang, K_DEVELOP_SNAPSHOT_HISTORY));
                if action_row(ui, true, tr(lang, K_DEVELOP_SAVE_AS_VERSION))
                    .on_hover_text(tr(lang, K_DEVELOP_SNAPSHOT_THIS_CARD_S_DEVELOP_ONLY_AS_A_NUMBERED))
                    .clicked()
                {
                    self.save_version();
                }
                // R24-2: a version row is 「v3 「日落偏暖」· 来自 ◭ 反推」 —
                // the number, the user's own name, and which VARIANT the
                // snapshot was taken from. The filter answers "show me only
                // this card's history"; rows it hides are COUNTED below, so
                // a shrinking list is never mistaken for lost versions.
                let active_id = self.active_variant().map(|v| v.id.clone()).unwrap_or_default();
                let active_kind = self.active_variant().map(|v| v.kind);
                if !self.workspace.cards().versions.is_empty() {
                    ui.checkbox(&mut self.workspace.cards_mut().versions_current_only, tr(lang, K_DEVELOP_ONLY_THIS_VARIANT))
                        .on_hover_text(tr(
                            lang,
                            K_DEVELOP_SHOW_ONLY_SNAPSHOTS_TAKEN_FROM_THE_VARIANT_YOU_A,
                        ));
                }
                let mut load: Option<u32> = None;
                let mut delete: Option<u32> = None;
                let mut rename: Option<u32> = None;
                let mut hidden = 0usize;
                // The list is cloned so the rows may borrow `self` mutably
                // (the rename buffer) — it is a handful of u32.
                let versions = self.workspace.cards().versions.clone();
                let photo = self.editor.photo().src_path.clone();
                for n in versions {
                    let meta = self.workspace.cards().version_meta.get(&n).cloned();
                    if self.workspace.cards().versions_current_only
                        && !version_is_from(meta.as_ref(), &active_id, active_kind)
                    {
                        hidden += 1;
                        continue;
                    }
                    #[cfg(test)]
                    self.seams.edit_list_rows.push(format!("version:{n}"));
                    let named = meta.as_ref().and_then(|m| m.name.clone());
                    let editing = self.workspace.cards().version_name_buf
                        .as_ref()
                        .is_some_and(|(p, j, ..)| *j == n && Some(p) == photo.as_ref());
                    ui.horizontal(|ui| {
                        ui.label(format!("v{n}"));
                        let field_id = ui.make_persistent_id(("version_name", n));
                        if editing {
                            let resp = {
                                let buf = &mut self.workspace.cards_mut().version_name_buf
                                    .as_mut()
                                    .expect("checked just above")
                                    .3;
                                ui.add(
                                    egui::TextEdit::singleline(buf)
                                        .id(field_id)
                                        .desired_width(110.0)
                                        .hint_text(tr(lang, K_DEVELOP_NAME)),
                                )
                            };
                            // The +1 boundary: this row's own lost-focus
                            // commit, the twin of the mask panel's (U10).
                            // Enter counts as losing focus in egui. Leaving
                            // the box also leaves EDIT MODE — a row that
                            // stayed a TextEdit forever would keep asking
                            // for focus below and never give it back.
                            if resp.lost_focus() {
                                self.commit_version_name_buf();
                                self.workspace.cards_mut().version_name_buf = None;
                            } else if !resp.has_focus() {
                                // The frame the box first appears in: the
                                // click that opened it landed on the LABEL
                                // that used to be here, so the caret has to
                                // be put here explicitly or the user's next
                                // keystroke goes nowhere.
                                resp.request_focus();
                            }
                        } else {
                            let shown = match &named {
                                Some(name) => format!("「{name}」"),
                                None => tr(lang, K_DEVELOP_NAME__2).to_string(),
                            };
                            let text = match &named {
                                Some(_) => egui::RichText::new(shown),
                                None => egui::RichText::new(shown).weak(),
                            };
                            if ui
                                .add(egui::Label::new(text).sense(egui::Sense::click()))
                                .on_hover_text(tr(lang, K_DEVELOP_NAME_THIS_SNAPSHOT))
                                .clicked()
                            {
                                rename = Some(n);
                            }
                        }
                        // Provenance, only when it was actually recorded —
                        // a version from before this existed says nothing
                        // rather than guessing.
                        if let Some(m) = &meta {
                            let from = m.from_kind.as_deref().and_then(VariantKind::from_store_str);
                            let note = match (from, m.origin.as_deref()) {
                                // `kind` by name, not `k`: the i18n audit's
                                // dynamic-key registry keys on the receiver
                                // spelling (`kind.label()`), which is how a
                                // new VariantKind without a zh pair is caught.
                                (Some(kind), _) => Some(trf(
                                    lang,
                                    K_DEVELOP_FROM,
                                    &[("kind", tr(lang, kind.label()))],
                                )),
                                (None, Some(autoshade::store::VERSION_ORIGIN_AUTO)) => {
                                    Some(tr(lang, K_DEVELOP_AUTO_ARCHIVED).to_string())
                                }
                                _ => None,
                            };
                            if let Some(note) = note {
                                ui.label(egui::RichText::new(note).weak().small());
                            }
                        }
                        if action(ui, true, tr(lang, K_DEVELOP_LOAD)).on_hover_text(tr(lang, K_DEVELOP_REPLACE_CURRENT_PARAMETERS_ONE_CTRL_Z_TO_UNDO)).clicked() {
                            load = Some(n);
                        }
                        if glyph(ui, true, "🗑")
                            .on_hover_text(tr(lang, K_DEVELOP_DELETE_THIS_SNAPSHOT_ITS_FROZEN_MASK_RASTERS_GO))
                            .clicked()
                        {
                            delete = Some(n);
                        }
                    });
                }
                if hidden > 0 {
                    ui.label(
                        egui::RichText::new(trf(
                            lang,
                            K_DEVELOP_HIDDEN_SAVED_FROM_ANOTHER_VARIANT,
                            &[("n", &hidden.to_string())],
                        ))
                        .weak()
                        .small(),
                    );
                }
                if let Some(n) = rename
                    && let Some(p) = photo.clone()
                {
                    // Switching rows commits the previous one to ITS OWN
                    // (photo, number) first — the mask panel's cross-commit
                    // rule, made structural by keying on the number.
                    self.commit_version_name_buf();
                    let cur = self.workspace.cards().version_meta
                        .get(&n)
                        .and_then(|m| m.name.clone())
                        .unwrap_or_default();
                    self.workspace.cards_mut().version_name_buf = Some((p, n, cur.clone(), cur));
                }
                if let Some(n) = load {
                    self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::LoadVersion(n),
                    )]);
                }
                if let Some(n) = delete
                    && let Some(src) = self.editor.photo().src_path.clone()
                {
                    // A rename buffer aimed at the row being deleted dies
                    // WITH it (R24-2): committing it afterwards would write a
                    // name for a number that no longer exists and can never
                    // be re-issued — a record only the next delete would
                    // clean up.
                    if self.workspace.cards().version_name_buf.as_ref().is_some_and(|(p, j, ..)| *j == n && *p == src)
                    {
                        self.workspace.cards_mut().version_name_buf = None;
                    }
                    self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::DeleteVersion(n),
                    )]);
                }
                if n_ver == 0 {
                    explainer(ui, lang, tr(lang, K_DEVELOP_LIKE_LR_VIRTUAL_COPIES_STORE_MULTIPLE_PARAMETER));
                }
                }); // add_enabled_ui: inert while a photo is still opening
            });
        // The edit-state list's card action, performed through the SAME owner
        // the strip uses — after the section's closures have released `self`.
        if let Some(a) = act {
            self.dispatch_variant_action(a, ui.ctx());
        }
        false
    }

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
                    egui::RichText::new(match self.export_dest_dir() {
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
                let denoise_raw = self.denoise_uses_raw();
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
                let raw = self.can_export_xmp_beside();
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
                    self.export_xmp_beside();
                }
            });
        false
    }
}
