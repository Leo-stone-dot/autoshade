//! Develop panel: sliders, sections, variant strip.

use crate::*;

// The section ● predicate and the table it reads (R25 P0) — see
// `develop_panel` and `dev_lens`.
use autoshade::advisor::catalogue::{family_is_active, CONTROL_FAMILIES};
use crate::panels::grade_wheel::{grade_wheel, GRADE_VIEWS};


impl AutoShadeApp {
    /// One labelled slider; double-click resets to `default` (the Lightroom
    /// gesture), hover + ↑/↓ nudges by a domain-appropriate step (Shift ×10 —
    /// LR's arrow grammar; ←/→ stay library navigation). Returns true if the
    /// value changed this frame. Callers pass an already-translated `label`.
    /// Whole-number domains (range ≥ 20: the ±100 family, 0..150, hue °) snap
    /// to integers while dragging, like Lightroom — the web UI already had to
    /// work around raw floats ("13.4849996…" overflowing its value box).
    pub(crate) fn slider(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
        default: f32,
    ) -> bool {
        Self::slider_hinted(ui, lang, label, value, min, max, default, "")
    }

    /// [`slider`] plus ONE disclosure line at the head of its tooltip — for a
    /// control whose value is SHARED with another panel, where the tooltip is
    /// the only place that fact can be told. (`slider` returns `bool`, not a
    /// `Response`, so a caller cannot chain its own `on_hover_text`; a second
    /// tooltip on a wrapper rect would just stack two bubbles over one
    /// widget.) The brush radius is the case: one number behind the mask
    /// brush AND Fill / Heal / Stamp.
    #[allow(clippy::too_many_arguments)] // `slider` + the one extra tooltip line
    pub(crate) fn slider_hinted(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
        default: f32,
        hint: &str,
    ) -> bool {
        let feel =
            if max - min >= 20.0 { SliderFeel::Int } else { SliderFeel::Frac };
        Self::slider_impl(ui, lang, label, value, min, max, default, feel, hint)
    }

    /// A COMPANION control's slider (`recipe::LR_COMPANION_DEFAULTS`, v1.5.0):
    /// it SHOWS what the engine renders — Lightroom's own default while the
    /// recipe holds no value — and writes back through
    /// `EditRecipe::set_resolved`, so a drag to 0 is a real 0 rather than
    /// "absent". The reset (double- or right-click) and a drag that lands on
    /// the default both return the control to absent, which renders the same
    /// and lets Lightroom keep its own default in the sidecar.
    #[allow(clippy::too_many_arguments)] // `slider_impl`'s track + the recipe and the control name
    pub(crate) fn companion_slider(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        recipe: &mut EditRecipe,
        name: &str,
        min: f32,
        max: f32,
        feel: SliderFeel,
    ) -> bool {
        let default = autoshade::recipe::LR_COMPANION_DEFAULTS
            .iter()
            .find(|(n, _)| *n == name)
            .map_or(0.0, |(_, d)| *d);
        let mut shown = recipe.resolved(name);
        let changed = Self::slider_impl(ui, lang, label, &mut shown, min, max, default, feel, "");
        if changed {
            if shown == default {
                recipe.clear_resolved(name);
            } else {
                recipe.set_resolved(name, shown);
            }
        }
        changed
    }

    /// The Sharpening amount's slider (v1.6.0): a COMPANION whose default is
    /// Lightroom's own FOR THE KIND of source — 40 on a RAW negative, none on
    /// a baked raster — so `raw_source` (`AutoShadeApp::source_is_raw`) picks
    /// the default it shows, resets to and returns to absent on. The value
    /// shown is the one the engine renders (`EditRecipe::capture_sharpening`);
    /// a drag to 0 on a RAW is a real 0, stated in the sidecar.
    pub(crate) fn sharpening_slider(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        recipe: &mut EditRecipe,
        raw_source: bool,
    ) -> bool {
        let default = if raw_source { autoshade::recipe::LR_RAW_SHARPENING } else { 0.0 };
        let mut shown = recipe.capture_sharpening(raw_source);
        let changed = Self::slider_impl(ui, lang, label, &mut shown, 0.0, 150.0, default, SliderFeel::Int, "");
        if changed {
            if shown == default {
                recipe.clear_resolved("sharpening");
            } else {
                recipe.set_resolved("sharpening", shown);
            }
        }
        changed
    }

    /// A 0..=1-stored fraction shown on Lightroom's 0..100 track (Amount,
    /// feathers, range tolerance): the panel used to mix "Amount 0.65" with
    /// "Shadows 40" in one column, which read as two unit systems. Storage
    /// stays the fraction (the XMP contract); only the display scales. The
    /// ×100 track crosses the ≥ 20 width rule above, so these snap to whole
    /// numbers exactly like LR's.
    pub(crate) fn slider_pct(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        value: &mut f32,
        max: f32,
        default: f32,
    ) -> bool {
        Self::slider_pct_hinted(ui, lang, label, value, max, default, "")
    }

    /// [`slider_pct`] plus ONE disclosure line at the head of its tooltip — the
    /// percent-track twin of [`slider_hinted`], same reason (the helpers return
    /// `bool`, so a caller cannot chain its own `on_hover_text`). For a fraction
    /// slider whose meaning cannot be read off its label: the AI 「Style」
    /// strength, which has to say what it leans on and that 0 means "ignore my
    /// habits" (R22 #16 — it was a bare `egui::Slider` carrying that sentence
    /// itself, which is exactly why it had no reset gesture and no ↑/↓ nudge).
    pub(crate) fn slider_pct_hinted(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        value: &mut f32,
        max: f32,
        default: f32,
        hint: &str,
    ) -> bool {
        let mut disp = *value * 100.0;
        let changed = Self::slider_hinted(
            ui,
            lang,
            label,
            &mut disp,
            0.0,
            max * 100.0,
            default * 100.0,
            hint,
        );
        if changed {
            *value = disp / 100.0;
        }
        changed
    }

    /// Sub-unit precision variant (Straighten °): the whole-number snap of the
    /// wide-range class would destroy 0.1° levelling on a ±45 track.
    pub(crate) fn slider_fine(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
        default: f32,
    ) -> bool {
        Self::slider_impl(ui, lang, label, value, min, max, default, SliderFeel::Fine, "")
    }

    /// Log-scaled variant for values whose useful band is a small fraction of
    /// the range — Temp (K): 2000–50000 linear puts 4000–8000 K (where nearly
    /// every photo lives) on ~40 px of a 320 px track, making a routine 100 K
    /// nudge sub-pixel.
    pub(crate) fn slider_log(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
        default: f32,
    ) -> bool {
        Self::slider_impl(ui, lang, label, value, min, max, default, SliderFeel::LogK, "")
    }

    #[allow(clippy::too_many_arguments)] // private impl detail shared by four thin public shapes
    pub(crate) fn slider_impl(
        ui: &mut egui::Ui,
        lang: Lang,
        label: &str,
        value: &mut f32,
        min: f32,
        max: f32,
        default: f32,
        feel: SliderFeel,
        // Prepended to the shared tooltip; "" for every ordinary slider. See
        // `slider_hinted` — the ONE tooltip is the point, not two stacked.
        hint: &str,
    ) -> bool {
        let range = max - min;
        // (drag snap, ↑/↓ nudge, shown decimals) per feel class. Frac's snap
        // matches its 2 shown decimals exactly — egui's fixed_decimals ALSO
        // rounds the stored value (slider.rs), so a finer snap would silently
        // lose to the display rounding. The Frac NUDGE is floored at that
        // same 0.01 grid: a finer step (range < 1) rounds back onto the
        // current value below and the `next != *value` guard drops it —
        // ArrowDown on the 0–0.5 Feather was a permanent no-op (D13).
        // EV: 0.01-drag / 0.1-arrow. LogK's
        // nudge is ~1% of the current Kelvin (≈55 K at 5500) — a fixed step
        // would be sub-pixel at one end of a log track and huge at the other.
        let (snap, nudge, decimals) = match feel {
            SliderFeel::Int => (1.0, 1.0, 0usize),
            SliderFeel::Fine => (0.1, 0.1, 1),
            SliderFeel::Frac => (0.01, (range / 100.0).max(0.01), 2),
            SliderFeel::LogK => (1.0, ((*value).abs() * 0.01).max(1.0).round(), 0),
        };
        let grammar = tr(lang, "double-click / right-click resets · hover + ↑/↓ nudges (Shift ×10)");
        let tip = if hint.is_empty() {
            grammar.to_string()
        } else {
            format!("{hint}\n{grammar}")
        };
        // The value box is egui's DragValue: a button that GROWS with its
        // text, so its width is budgeted at the widest text the track can
        // show (both ends, at the shown decimals) plus the button's padding —
        // the label then truncates into what is left, and a row that fits at
        // its default still fits at either end of a drag (the 2026-09-15
        // 「100」 that ran the panel 4 px wide every frame).
        let value_w = {
            let style = ui.style().drag_value_text_style.resolve(ui.style());
            let widest = ui.fonts_mut(|f| {
                [min, max]
                    .into_iter()
                    .map(|v| f.layout_no_wrap(format!("{v:.decimals$}"), style.clone(), egui::Color32::WHITE).size().x)
                    .fold(0.0f32, f32::max)
            });
            (widest + 2.0 * ui.spacing().button_padding.x).max(ui.spacing().interact_size.x)
        };
        // Lightroom / RapidRaw's slider (2026-10-13, user: 「UI 整体还是不够
        // 现代化」): the name on the left and the number on the right of one
        // line, a thin rail the full width of the column under them. egui's
        // stock row — rail, framed number, name trailing on the right — put
        // the eye on the number box and gave every rail a different length.
        // Both lines take exactly the column's width, so no label length can
        // widen the panel (R38's runaway came from a fixed rail beside one).
        // A vertical child, not a scope: a caller that lays the slider in a
        // row (the Style dial and its warning) gets the two lines as one item
        // of that row instead of the rail continuing the row.
        let scope = ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            ui.spacing_mut().interact_size.y = SLIDER_LINE_H;
            let full = ui.available_width();
            let head = ui.horizontal(|ui| {
                let name_w = (full - value_w - ui.spacing().item_spacing.x).max(0.0);
                let name = ui
                    .allocate_ui_with_layout(
                        egui::vec2(name_w, SLIDER_LINE_H),
                        egui::Layout::left_to_right(egui::Align::Center),
                        |ui| {
                            ui.set_min_width(name_w);
                            ui.add(
                                egui::Label::new(label)
                                    .truncate()
                                    .sense(egui::Sense::click()),
                            )
                        },
                    )
                    .inner;
                // The number reads as text until it is touched: no frame at
                // rest, egui's own frame on hover / drag / typing.
                let number = ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let w = &mut ui.visuals_mut().widgets.inactive;
                    w.weak_bg_fill = egui::Color32::TRANSPARENT;
                    w.bg_stroke = egui::Stroke::NONE;
                    // Hug the text so the digits end where the rail ends.
                    ui.spacing_mut().interact_size.x = 0.0;
                    // …and keep the line one text high: the button's stock
                    // 5 px vertical padding made it 28 px, pushing the rail
                    // a whole row away from its name.
                    ui.spacing_mut().button_padding = egui::vec2(SPACE_XS, 0.0);
                    let speed = if matches!(feel, SliderFeel::LogK) { (*value).abs() * 0.002 } else { range / 300.0 };
                    ui.add(
                        egui::DragValue::new(value)
                            .range(min..=max)
                            .speed(f64::from(speed).max(snap * 0.5))
                            .fixed_decimals(decimals),
                    )
                });
                (name, number.inner)
            });
            let (name, number) = head.inner;
            ui.spacing_mut().slider_width = full;
            let rail = ui.add(
                egui::Slider::new(value, min..=max)
                    .logarithmic(matches!(feel, SliderFeel::LogK))
                    .step_by(snap)
                    .fixed_decimals(decimals)
                    .show_value(false),
            );
            paint_slider_knob(ui, &rail, *value, min, max, matches!(feel, SliderFeel::LogK));
            (name, number, rail)
        });
        let (name, number, rail) = scope.inner;
        let name = name.on_hover_text(&tip);
        let rail = rail.on_hover_text(&tip);
        // Double-click (the Lightroom gesture) or right-click resets, on the
        // rail or on the name — LR resets from either.
        if [&name, &rail].iter().any(|r| r.double_clicked() || r.secondary_clicked()) && *value != default {
            *value = default;
            return true;
        }
        let mut changed = rail.changed() || number.changed();
        // The nudge is gated on "no widget owns the keyboard", so arrows typed
        // into a text field can never double-apply here.
        if (name.hovered() || rail.hovered() || number.hovered()) && ui.memory(|m| m.focused().is_none()) {
            let shift = ui.input(|i| i.modifiers.shift);
            let (up, down) = ui.input_mut(|i| {
                (
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowUp)
                        || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowUp),
                    i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowDown)
                        || i.consume_key(egui::Modifiers::SHIFT, egui::Key::ArrowDown),
                )
            });
            if up != down {
                let step = nudge * if shift { 10.0 } else { 1.0 };
                let next = crate::logic::slider::nudged(*value, up, step, decimals, min, max);
                if next != *value {
                    *value = next;
                    changed = true;
                }
            }
        }
        changed
    }

    pub(crate) fn develop_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang; // Copy — never borrows self, safe inside egui closures.
        let mut changed = false;
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, "Develop"), false, |_| {});
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
        self.histogram_ui(ui);
        ui.add_space(SPACE_SM);

        // Mid-open (decode in flight) these controls would edit the STASHED
        // photo A while B lands and replaces the whole recipe — silent input
        // loss (L15-2). Busy alone must NOT gate here: a 600 s analyze keeps
        // the panel live; only the open transition freezes it.
        // The AI area carries its OWN copy of this gate (panels/ai.rs) — it
        // used to ride inside this closure as `dev_ai`.
        let editable = !self.open.open_in_flight;
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
        let (
            presence_active,
            detail_active,
            hsl_active,
            grade_active,
            curves_active,
            effects_active,
            calibration_active,
            transform_active,
            hdr_active,
        ) = {
            let r = &self.recipe;
            let fam = |name: &str| {
                CONTROL_FAMILIES
                    .iter()
                    .find(|f| f.name == name)
                    .is_some_and(|f| family_is_active(f, r))
            };
            (
                fam("presence"),
                // R25 B3: the Detail section holds BOTH halves — the two
                // sliders the AI plans with and the eight carried shaping
                // axes — so its ● is the OR of the two families. They are
                // separate families because one is AI-visible and one is not
                // (see `CONTROL_FAMILIES`), not because the panel splits them.
                fam("detail") || fam("detail_effects"),
                // v1.5.0: the Color Mixer section holds Lightroom's whole
                // mixer — the colour bands, the B&W treatment that replaces
                // them and the point colours — so its ● is the OR of the
                // three families, for the Detail section's reason above.
                fam("hsl") || fam("black_white") || fam("point_color"),
                fam("color_grade"),
                // v1.5.0: the Curves section holds the point curves AND the
                // parametric curve — two families for Detail's reason (one is
                // AI-visible, one is not), one section, so one ● for both.
                fam("curves") || fam("parametric"),
                // R25 B2 made `effects` the first family with no AI-visible
                // member; it is planned with since 2026-10-21, and either
                // way the section's ● comes from the same table.
                fam("effects"),
                fam("calibration"),
                // v1.5.0 F6: the Transform section's own family. It has no
                // AI-visible member either, so like `effects` it exists in the
                // table for exactly this.
                fam("transform"),
                // v1.5.0 F8: likewise — the `hdr` family exists for this ●.
                fam("hdr"),
            )
        };
        changed |= self.dev_tone_wb(ui);
        changed |= self.dev_presence(ui, presence_active);
        changed |= self.dev_curves(ui, curves_active);
        changed |= self.dev_hsl(ui, hsl_active);
        changed |= self.dev_grading(ui, grade_active);
        // v1.5.0: Lightroom's Calibration panel, which Lightroom lists last
        // and this panel closes the colour group with — it is a colour
        // control, and #14b groups by kind.
        changed |= self.dev_calibration(ui, calibration_active);
        changed |= self.dev_detail(ui, detail_active);
        changed |= self.dev_effects(ui, effects_active);
        // A migrated panel returns what the user did instead of mutating the
        // recipe; the facade applies it (and the clamp below still runs once).
        let lens_actions = self.dev_lens_blur(ui);
        if !lens_actions.is_empty() {
            self.apply_actions(lens_actions);
            changed = true;
        }
        changed |= self.dev_lens(ui);
        // In Lightroom's own panel order (Transform follows Lens Corrections).
        // R25 B4 drew this section read-only because nothing here moved a pixel;
        // v1.5.0 F6 renders all of it, so the sliders are real and only the
        // Upright solver's own bookkeeping is still carried verbatim.
        changed |= self.dev_transform(ui, transform_active);
        changed |= self.dev_crop(ui);
        changed |= self.dev_masks(ui);
        changed |= self.dev_versions(ui);
        // v1.5.0 Track S. Not a develop control at all — a verb that makes a
        // new negative out of several frames — and it sits here, after the
        // version history and before the rendition/delivery pair, for that
        // reason: it belongs to the PHOTOGRAPH's identity, which is what the
        // sections above it are about, rather than to the file that leaves.
        self.dev_stack(ui);
        // v1.5.0 F8, beside Export and not beside Tone: what this section
        // governs is the file that leaves the program. The rendition is the
        // develop's LAST stage (`render::hdr`), and every control in it is
        // about the mapping into SDR rather than about the photograph.
        changed |= self.dev_hdr(ui, hdr_active);
        changed |= self.dev_export(ui);
        });

        if changed {
            self.recipe.clamp();
            self.dirty = true;
        }
    }

    /// The variant strip (版本条): one card per rendition — 原片 / AI 生成 /
    /// 反推 — with a live developed thumbnail. Click a card to switch (lossless;
    /// each variant keeps its own base + recipe), × to drop one. This is the
    /// selector that makes an AI develop a first-class, non-reverting version.
    pub(crate) fn variant_strip(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        let accent = self.ui.theme.colors().accent_text; // Copy — safe in closures
        // ONE deferred action for the whole strip (R24-5): the five separate
        // `Option`s this replaced were dispatched by an `else if` chain whose
        // exclusivity was a property of the chain; a single value makes it a
        // property of the TYPE, and it is the same value the edit-state list
        // hands to the same owner. Last writer wins where the chain had a
        // fixed priority — reachable only if two widgets were clicked in one
        // frame, which one pointer cannot do.
        let mut act: Option<VariantAction> = None;
        let busy = self.busy; // Copy — read before the card closures borrow self
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
            tr(lang, "Variants").to_owned(),
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
                    for i in 0..self.variants.len() {
                        let active = i == self.active;
                        let kind = self.variants[i].kind;
                        ui.vertical(|ui| {
                            // Developed thumbnail (or a placeholder until the
                            // variant has been developed once).
                            let resp = if let Some(t) = &self.variants[i].thumb {
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
                                let ring = self.ui.theme.colors().accent_fill;
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
                                    "Click to switch to this variant (lossless); Ctrl+S saves every card for this photo",
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
                                        "A generated image stays as generated: the first edit here continues on a new ✎ Edited AI image card",
                                    ));
                                } else if kind == VariantKind::Stacked {
                                    // The ▦ card (v1.5.0): what it is and what
                                    // a .xmp from it can carry, said where the
                                    // card is.
                                    label.on_hover_text(tr(
                                        lang,
                                        "Several frames of this scene merged into their own master: develop it like the ▣ card, reverse-fit and reimagine read it as the negative while it exists; a .xmp from it carries the sliders only — the merged pixels live in the master beside it",
                                    ));
                                } else if kind == VariantKind::Denoised {
                                    // The ◈ card (2026-09-15): what it is and
                                    // what a .xmp from it can carry, said where
                                    // the card is.
                                    label.on_hover_text(tr(
                                        lang,
                                        "The negative AI-denoised into its own master: develop it like the ▣ card, reverse-fit and reimagine read it as the negative while it exists; a .xmp from it carries the sliders only — run Lightroom's own Denoise there",
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
                                let card_id = self.variants[i].id.clone();
                                let named = self.variants[i].name.clone();
                                let editing = active
                                    && self.cards.variant_name_buf.as_ref().is_some_and(|(p, id, ..)| {
                                        *id == card_id
                                            && Some(p.as_path()) == self.src_path.as_deref()
                                    });
                                if editing {
                                    let resp = {
                                        let buf =
                                            &mut self.cards.variant_name_buf.as_mut().expect("checked just above").3;
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
                                                .hint_text(tr(lang, "Name")),
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
                                        self.cards.variant_name_buf = None;
                                    } else if !resp.has_focus() {
                                        resp.request_focus();
                                    }
                                } else if active {
                                    let shown = match &named {
                                        Some(n) => egui::RichText::new(format!("「{n}」")).small(),
                                        None => egui::RichText::new(tr(lang, "Name…").to_string())
                                            .small()
                                            .weak(),
                                    };
                                    let resp = ui
                                        .add(egui::Label::new(shown).sense(egui::Sense::click()))
                                        .on_hover_text(tr(lang, "Name this variant"));
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
        let kind = self.variants[i].kind;
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
                .on_hover_text(tr(lang, "Snapshot this card's develop only as a numbered version (v<N>.recipe.json with frozen mask rasters); Ctrl+S saves every card"))
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
                tr(lang, "Copy this variant's develop onto the ▣ Original card — its baked pixels and this card both stay. One Ctrl+Z undoes it; Ctrl+S then saves it as this photo's develop")
            } else if kind == VariantKind::Edited {
                tr(lang, "This ✎ card's develop is tuned over AI-generated pixels — on the ▣ Original card those sliders would land on the source frame; run 「Reverse-fit」 on the ✨ card first")
            } else {
                tr(lang, "A generated variant's look lives in its pixels — there are no develop parameters to copy onto the ▣ Original card; run 「Reverse-fit」 first")
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
        let armed = self.cards.variant_delete_confirm == Some(i);
        if self.variants.len() > 1
            && kind != VariantKind::Original
            && glyph(ui, true, if armed { "✕?" } else { "✕" })
                .on_hover_text(if armed {
                    tr(lang, "Click again to delete this variant — it cannot be brought back (Ctrl+Z does not cross variants)")
                } else {
                    tr(lang, "Delete this variant")
                })
                .clicked()
        {
            act = Some(VariantAction::Delete(i));
        }
        #[cfg(test)]
        if self.variants.len() > 1 && kind != VariantKind::Original {
            drawn.push(if armed { "✕?" } else { "✕" });
        }
        #[cfg(test)]
        if surface == VariantSurface::List {
            self.seams.edit_list_actions.push(format!("{i}:{}", drawn.join("")));
        }
        act
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    /// One capability table through the command layer: the renderer reports
    /// each change as a `Command`, the app applies it. Companion rows still
    /// write their resolved encoding directly (that command arm is not built
    /// yet), and both paths mark the frame changed the same way.
    fn capability_rows(&mut self, ui: &mut egui::Ui, table: &[crate::logic::capability::Capability]) -> bool {
        let mut commands = Vec::new();
        let changed = crate::panels::develop_forms::render_rows(ui, self.lang, &mut self.recipe, table, &mut commands);
        for command in commands {
            logic::command::apply(&mut self.recipe, command);
        }
        changed
    }

    fn dev_tone_wb(&mut self, ui: &mut egui::Ui) -> bool {
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
            .any(|f| family_is_active(f, &self.recipe));

        // #14b: the FIRST of the panel's group captions (its air is its own:
        // `headings::group_caption`). The two existing group
        // fences (before Detail, before Local Masks) marked boundaries without
        // ever saying what they divided; the five groups the panel actually has
        // are AI (its own panel above) → tone & colour → detail & lens → local
        // & pixel → versions & export. Captions only: no section moved, no
        // fence rhythm changed.
        group_caption(ui, tr(lang, "Tone & Colour"));
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Tone & WB"), tone_active))
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
                        self.recipe.temperature_k.is_some() || self.recipe.tint != 0.0;
                    if ui.checkbox(&mut custom_wb, tr(lang, "Custom white balance (off = as-shot)")).changed() {
                        // Arm AT the as-shot anchor when stamped: checking
                        // the box must not shift the image — the slider then
                        // starts from the camera's real Kelvin, not 5500.
                        self.recipe.temperature_k =
                            if custom_wb { Some(self.recipe.as_shot_k.unwrap_or(5500.0)) } else { None };
                        if !custom_wb {
                            // "As-shot" is the WHOLE white balance: a custom
                            // Tint surviving the un-check kept a WB edit
                            // active behind a label claiming there was none.
                            self.recipe.tint = 0.0;
                        }
                        changed = true;
                    }
                    let label = if self.tools.wb_picking { tr(lang, "💧 Click in image…") } else { tr(lang, "💧 Eyedropper") };
                    if action_in(ui, cell, true, label)
                        .on_hover_text(tr(lang,
                            "Click a spot in the image that should be neutral grey/white to auto-solve Temp/Tint (same forward model as the engine). Click again to cancel.",
                        ))
                        .clicked()
                    {
                        let on = !self.tools.wb_picking;
                        self.disarm_tools();
                        self.tools.wb_picking = on;
                        if on {
                            self.status = tr(lang, "WB eyedropper: click a spot that should be neutral grey/white").into();
                        }
                    }
                });
                if let (Some(k), Some(t)) = (self.recipe.as_shot_k, self.recipe.as_shot_tint) {
                    // The camera's own WB in absolute terms — the reference
                    // the Temp slider is anchored on for this photo. BOTH
                    // halves required: the era pin (anchor 5500, tint None)
                    // knows the anchor but NOT the camera, and showing it as
                    // "as shot" would be a false claim.
                    ui.weak(trf(
                        lang,
                        "as shot ≈ {k} K · tint {t}",
                        &[("k", &format!("{k:.0}")), ("t", &format!("{t:+.0}"))],
                    ));
                }
                // Double-click reset lands on the as-shot Kelvin when stamped
                // (the honest "no shift" position), 5500 for legacy recipes.
                let anchor_k = self.recipe.as_shot_k.unwrap_or(5500.0);
                if let Some(mut k) = self.recipe.temperature_k
                    && Self::slider_log(ui, lang, tr(lang, "Temp (K)"), &mut k, 2000.0, 50000.0, anchor_k)
                {
                    self.recipe.temperature_k = Some(k);
                    changed = true;
                }
                let custom_wb = self.recipe.temperature_k.is_some() || self.recipe.tint != 0.0;
                // Tint is HALF of the white balance: editable only under
                // Custom WB, or a tint move silently contradicted the
                // checkbox's "off = as-shot" promise. A legacy tint-only
                // recipe counts as Custom (see the checkbox above) so its
                // slider stays reachable.
                ui.add_enabled_ui(custom_wb, |ui| {
                    changed |= self.capability_rows(ui, crate::logic::capability::WB_TINT);
                });
                // The tone band is a capability table row each: the panel owns
                // the fold and the white balance controls above; the fields,
                // bands and resets live in `logic::capability::TONE`.
                changed |= self.capability_rows(ui, crate::logic::capability::TONE);
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    fn dev_presence(&mut self, ui: &mut egui::Ui, presence_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;

        // LR-Basic order (UX batch): Presence sits directly under Tone & WB —
        // the two halves of Lightroom's Basic panel — then Curves, then the
        // two colour sections; Detail moved BELOW them so colour work isn't
        // interrupted by sharpening. add_space fences each header so the ten
        // sections read as groups, not one undivided list.
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Presence"), presence_active))
            .id_salt("sec_presence")
            .default_open(true)
            , |ui| {
                // Lightroom's Basic-panel order inside Presence is Texture →
                // Clarity → Dehaze, and the engine runs them in that order too
                // (R25 B2 put the global texture stage between clarity and
                // saturation). The rows live in `logic::capability::PRESENCE`;
                // rows and order are one table.
                changed |= self.capability_rows(ui, crate::logic::capability::PRESENCE);
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    fn dev_curves(&mut self, ui: &mut egui::Ui, curves_active: bool) -> bool {
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
    fn dev_hsl(&mut self, ui: &mut egui::Ui, hsl_active: bool) -> bool {
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

    /// 校准 — Lightroom's Calibration panel (v1.5.0): the shadows tint and each
    /// primary's hue and saturation.
    ///
    /// Rendered as the develop's FIRST stage (`render::Calibration`), which is
    /// why it is a section of sliders and not a read-out: every other section
    /// works on the colours these seven leave behind. Lightroom writes all
    /// seven keys on nearly every sidecar, so a photo arriving from Lightroom
    /// usually lands here with values already in it.
    fn dev_calibration(&mut self, ui: &mut egui::Ui, calibration_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Calibration"), calibration_active))
            .id_salt("sec_calibration")
            .default_open(false)
            , |ui| {
                // Lightroom's own order and grouping: the shadows tint first,
                // then a hue + saturation pair per primary. The group captions
                // stay here; the rows are capability tables.
                ui.label(egui::RichText::new(tr(lang, "Shadows")).weak().small());
                changed |= self.capability_rows(ui, crate::logic::capability::CAL_TINT);
                for (primary, table) in [
                    (tr(lang, "Red primary"), crate::logic::capability::CAL_RED),
                    (tr(lang, "Green primary"), crate::logic::capability::CAL_GREEN),
                    (tr(lang, "Blue primary"), crate::logic::capability::CAL_BLUE),
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
    fn dev_hdr(&mut self, ui: &mut egui::Ui, hdr_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "HDR & SDR"), hdr_active))
            .id_salt("sec_hdr")
            .default_open(false)
            , |ui| {
                if ui
                    .checkbox(&mut self.recipe.hdr_edit, tr(lang, "HDR edit mode"))
                    .on_hover_text(tr(
                        lang,
                        "This photograph was edited above diffuse white. Every file this program writes is SDR, so the rendition below is what gets published.",
                    ))
                    .changed()
                {
                    changed = true;
                }
                changed |= self.capability_rows(ui, crate::logic::capability::HDR_HEADROOM);
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, "SDR rendition")).weak().small());
                ui.label(
                    egui::RichText::new(tr(lang, "renders only in HDR mode"))
                        .weak()
                        .small(),
                );
                changed |= self.capability_rows(ui, crate::logic::capability::HDR_SDR);
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    fn dev_grading(&mut self, ui: &mut egui::Ui, grade_active: bool) -> bool {
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

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    fn dev_detail(&mut self, ui: &mut egui::Ui, detail_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;

        // Look ends here — detail, then geometry (lens BEFORE crop: the lens
        // profile / manual distortion redefine the frame the crop sits in).
        // Group boundary: the group head's own air and gold caption (2026-10-01:
        // the full-width hairline that used to fence each group is gone).
        group_caption(ui, tr(lang, "Detail & Lens")); // #14b, group 2 of 4 here
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Detail"), detail_active))
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
                    // holds none — and reset to it (`companion_slider`,
                    // `sharpening_slider`).
                    let raw_source = self.source_is_raw();
                    changed |= Self::sharpening_slider(ui, lang, tr(lang, "Sharpening"), &mut self.recipe, raw_source);
                    // The rows below the amount are one capability table each;
                    // their bands, feels and companion defaults live in
                    // `logic::capability` (the radius is a Fine companion: the
                    // sidecar writes one decimal, so the track shows one).
                    changed |= self.capability_rows(ui, crate::logic::capability::SHARPEN);
                    ui.add_space(SPACE_SM);
                    changed |= self.capability_rows(ui, crate::logic::capability::NOISE_LUMA);
                    ui.add_space(SPACE_SM);
                    changed |= self.capability_rows(ui, crate::logic::capability::NOISE_COLOR);
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
                Self::slider_pct_hinted(
                    ui,
                    lang,
                    tr(lang, "AI denoise strength"),
                    if denoise_raw { &mut self.exp.denoise_strength } else { &mut self.exp.baked_denoise_strength },
                    1.0,
                    if denoise_raw { autoshade::denoise::DEFAULT_STRENGTH_RAW } else { autoshade::denoise::DEFAULT_STRENGTH },
                    tr(lang,
                        "Detail AI denoise strength, independent of the Export section. On a RAW, the dial removes luminance grain after demosaic in linear light; the default targets Lightroom Denoise 50. Higher is cleaner, 100% is the complete network output. Every positive strength keeps clean colour; exactly 0% leaves the input untouched. Already-rendered images (JPEG, PNG, TIFF, a retouched or AI result) use a different denoiser with its own 50% default. Double-click to reset this source type.",
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
                    let ready = self.src_path.is_some() && !self.busy && has_helper;
                    let missing = tr(lang,
                        "This feature needs AutoShade's Python helpers, which this download does not include. Install the full package, or (advanced) set AUTOSHADE_DENOISE_SCRIPT and AUTOSHADE_DENOISE_RAW_SCRIPT to the paths of denoise.py and denoise_raw.py",
                    );
                    if action_row(ui, ready, tr(lang, "🤖 AI Denoise now"))
                        // 🤖 + the cross-reference line (#4): this verb stays
                        // beside Noise Reduction on purpose, so its tooltip is
                        // where it says the rest of the AI moved to. On the arm
                        // that CANNOT run, the tooltip is about the missing
                        // sidecar and nothing else — same discipline as the
                        // segmentation pair.
                        .on_hover_text(if has_helper { ai_xref(lang, tr(lang,
                            "Denoise this card's pixels at full resolution — a RAW on its sensor mosaic before demosaic, \
                             an already-rendered image through its own denoiser — and land the result as a new ◈ Denoised negative card \
                             carrying this card's develop; the card you started from keeps its pixels (first run \
                             downloads the model)",
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
    fn dev_stack(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        // The section's ● says this photo HAS a stacked card, which is the
        // only state this fold owns that outlives the click.
        let has = self.variants.iter().any(|v| v.kind == VariantKind::Stacked);
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Stack"), has))
            .id_salt("sec_stack")
            .default_open(false)
            , |ui| {
                explainer(ui, lang, tr(lang, "Merge several frames of one scene into a new ▦ negative. This card's frame is the reference — its framing and its exposure are the ones the result keeps — and the frames you pick join it."));
                ui.add_space(SPACE_SM);
                egui::ComboBox::from_id_salt("stack_kind")
                    .selected_text(tr(lang, self.tools.stack_kind.label()))
                    .show_ui(ui, |ui| {
                        for k in autoshade::stack::merge::StackKind::ALL {
                            ui.selectable_value(&mut self.tools.stack_kind, k, tr(lang, k.label()));
                        }
                    })
                    .response
                    .on_hover_text(tr(
                        lang,
                        "HDR merge: an exposure bracket into one frame holding the whole range, with the recovered stops handed to the SDR rendition section. Exposure fusion: the same bracket blended where each frame looks best, with no HDR in between — a finished picture rather than data. Focus stack: a focus sweep into one frame sharp throughout. Noise stack: repeated frames of a still scene averaged, so the signal adds and the noise does not, and anything that moved through one frame is dropped.",
                    ));
                ui.checkbox(&mut self.tools.stack_tripod, tr(lang, "Shot on a tripod (skip alignment)"))
                    .on_hover_text(tr(
                        lang,
                        "Alignment is the expensive half of a stack, and on frames that really are registered there is nothing for it to find — measured on a five-frame noise stack, the grain fell 2.03x with it skipped and 1.97x with it running. Leave it off for handheld frames.",
                    ));
                ui.add_space(SPACE_SM);
                ui.horizontal(|ui| {
                    let ready = self.src_path.is_some() && !self.busy;
                    if action_row(ui, ready, tr(lang, "▦ Stack with other frames…"))
                        .on_hover_text(tr(
                            lang,
                            "Pick the other frames of this scene. They must be the same size as this one; the merge runs at full resolution and lands as a new ▦ card carrying this card's develop, and the frame you started from keeps its pixels.",
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
    }

    /// 效果 — Lightroom's post-crop vignette and film grain.
    ///
    /// All nine RENDER since v1.5.0 (`render/finish.rs`, the stage this engine
    /// grew after its crop), so none of them carries the "moves a number and
    /// no pixel" disclosure any more. The five COMPANIONS (Midpoint, Feather,
    /// Style, Grain size and roughness) show the value the engine actually
    /// renders — Lightroom's own default while the recipe holds none — and a
    /// reset returns them to absent (`companion_slider`).
    fn dev_effects(&mut self, ui: &mut egui::Ui, effects_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;

        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Effects"), effects_active))
            .id_salt("sec_effects")
            .default_open(false)
            , |ui| {
                ui.label(egui::RichText::new(tr(lang, "Post-crop vignetting")).weak().small());
                // Two tables because the panel draws two captioned groups; the
                // rows, their bands (the catalogue), their feels and their
                // companion encoding live in `logic::capability`. The style row
                // is an operator INDEX (1/2/3); the track starts at 1 because
                // 0 means "the sidecar named none" and only a reset reaches it.
                changed |= self.capability_rows(ui, crate::logic::capability::VIGNETTE);
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, "Grain")).weak().small());
                changed |= self.capability_rows(ui, crate::logic::capability::GRAIN);
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    fn dev_lens(&mut self, ui: &mut egui::Ui) -> bool {
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
            .any(|f| family_is_active(f, &self.recipe))
            || !self.recipe.lens_profile.is_as_opened();
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Lens"), lens_active))
            .id_salt("sec_lens")
            .default_open(false)
            , |ui| {
                // In-camera profile corrections (lensmeta): the manufacturer's
                // exact per-shot data read from the RAW itself — no lens
                // database. Toggling a component ON whose data a legacy
                // restore dropped re-fills it from this photo's metadata.
                let photo = self.open.photo_lens.clone();
                let lp = &mut self.recipe.lens_profile;
                let has_v = !lp.vignette.is_empty() || !photo.vignette.is_empty();
                let has_d = !lp.distortion.is_empty() || !photo.distortion.is_empty();
                let has_c = (!lp.ca_r.is_empty() || !photo.ca_r.is_empty())
                    && (!lp.ca_b.is_empty() || !photo.ca_b.is_empty());
                if has_v || has_d || has_c {
                    ui.label(tr(lang, "Profile corrections (from camera metadata)"));
                    let mut pch = false;
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(has_v, egui::Checkbox::new(&mut lp.vignette_on, tr(lang, "Vignetting")))
                            .on_hover_text(tr(lang, "The camera's own falloff map for this shot, applied in linear light"))
                            .changed()
                        {
                            if lp.vignette_on && lp.vignette.is_empty() {
                                lp.vignette = photo.vignette.clone();
                            }
                            pch = true;
                        }
                        if ui
                            .add_enabled(has_d, egui::Checkbox::new(&mut lp.distortion_on, tr(lang, "Distortion")))
                            .on_hover_text(tr(lang, "The camera's own geometric correction; masks and crop follow the corrected frame"))
                            .changed()
                        {
                            if lp.distortion_on && lp.distortion.is_empty() {
                                lp.distortion = photo.distortion.clone();
                            }
                            pch = true;
                        }
                        if ui
                            .add_enabled(has_c, egui::Checkbox::new(&mut lp.ca_on, tr(lang, "Chromatic aberration")))
                            .on_hover_text(tr(lang, "Per-channel radius correction: removes red/blue colour fringing at edges"))
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
                        self.overlay.overlay_stale = true;
                        changed = true;
                    }
                    ui.add_space(SPACE_MD);
                } else if self.src_path.as_deref().is_some_and(autoshade::decode::is_raw) {
                    ui.label(
                        egui::RichText::new(tr(lang, "No in-camera lens correction data in this file"))
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
                changed |= self.capability_rows(ui, crate::logic::capability::LENS_MANUAL);
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
                ui.label(egui::RichText::new(tr(lang, "Profile correction strength")).weak().small());
                changed |= self.capability_rows(ui, crate::logic::capability::LENS_PROFILE);
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, "Chromatic aberration (manual)")).weak().small());
                changed |= self.capability_rows(ui, crate::logic::capability::LENS_CA);
                // …and the rest of the same panel: Adobe's auto switch and the
                // whole de-fringe block. Carried and unrendered until v1.5.0;
                // the switch is an INSTRUCTION, so its tooltip says what the
                // instruction does rather than what the number means.
                if ui
                    .checkbox(&mut self.recipe.auto_lateral_ca, tr(lang, "Auto lateral CA"))
                    .on_hover_text(tr(
                        lang,
                        "Measures this frame's own red/blue misalignment and adds the answer to the manual pair above.",
                    ))
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(SPACE_SM);
                ui.label(egui::RichText::new(tr(lang, "Defringe")).weak().small());
                // Adobe's bands and neutrals: amounts rest at 0, the hue
                // windows at 30/70 and 40/60 (the one block whose neutral is
                // not zero — the row resets carry them).
                changed |= self.capability_rows(ui, crate::logic::capability::LENS_DEFRINGE);
                ui.add_space(SPACE_SM);
                explainer(ui, lang, tr(lang, "Vignette: positive brightens the corners (compensates falloff), negative darkens; a radial gain in linear light. Distortion: positive fixes barrel (wide-angle bulge), negative fixes pincushion (tele pinch); auto-scales to fill the frame, and masks / brush still position on the corrected image. Preview / export / XMP match. Manual CA, the auto-CA switch and de-fringe all render here too."));
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
    fn dev_transform(&mut self, ui: &mut egui::Ui, transform_active: bool) -> bool {
        let lang = self.lang;
        let mut changed = false;
        ui.add_space(SPACE_MD);
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Transform"), transform_active))
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
                let mut mode = self.recipe.perspective_upright.round().clamp(0.0, 5.0) as usize;
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr(lang, "Upright")).small());
                    egui::ComboBox::from_id_salt("upright_mode")
                        .selected_text(tr(lang, MODES[mode]))
                        .width(90.0)
                        .show_ui(ui, |ui| {
                            for (i, name) in MODES.iter().enumerate() {
                                ui.selectable_value(&mut mode, i, tr(lang, name));
                            }
                        });
                });
                if mode as f32 != self.recipe.perspective_upright {
                    self.recipe.perspective_upright = mode as f32;
                    changed = true;
                }
                // Which side will answer, said out loud — the difference
                // between "Adobe's numbers to the last digit" and "our own
                // solver, which may differ slightly" is exactly what a
                // photographer comparing the two apps needs to know.
                if mode > 0 {
                    let adobes = self
                        .recipe
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
                changed |= self.capability_rows(ui, crate::logic::capability::TRANSFORM);
                if ui
                    .checkbox(&mut self.recipe.crop_constrain_to_warp, tr(lang, "Constrain crop"))
                    .on_hover_text(tr(lang, "Shrink the crop until it lies inside the warped frame instead of showing the empty corners"))
                    .changed()
                {
                    changed = true;
                }
                ui.add_space(SPACE_SM);
                explainer(ui, lang, tr(lang, "One projective map, after the lens correction and before the straighten. Preview / export / XMP match. Upright renders the matrix Lightroom published when there is one; the sliders were measured against Lightroom's own exports. A keystone frames a little differently — Lightroom also stretches that axis, by an amount its lens sets."));
                // v1.5.0 F7 — WHICH rendering this photograph starts from.
                // Read-only here on purpose: the profile is a file on the
                // photographer's machine and the Look is Adobe's own creative
                // block, so the panel's job is to say what is being used, not
                // to offer a picker for something the sidecar decides.
                let look = self.recipe.look.clone();
                if !self.recipe.camera_profile.is_empty() || look.is_some() {
                    ui.add_space(SPACE_MD);
                    let pair = |ui: &mut egui::Ui, label: String, value: &str| {
                        ui.horizontal(|ui| {
                            ui.label(egui::RichText::new(label).small());
                            ui.label(egui::RichText::new(value).small().strong());
                        });
                    };
                    if !self.recipe.camera_profile.is_empty() {
                        pair(
                            ui,
                            tr(lang, "Camera profile").to_string(),
                            &self.recipe.camera_profile,
                        );
                    }
                    if let Some(look) = look.as_ref().filter(|l| !l.name.is_empty()) {
                        pair(ui, tr(lang, "Creative profile").to_string(), &look.name);
                        // Named, not hidden: a Look is colour data (v1.17.0) —
                        // its creative colour table renders when Camera Raw's
                        // install carries it (`dcp::find_look_table`,
                        // answered once per process) and the Look is at full
                        // amount, the render's own two conditions; its baked
                        // tone curve and sliders never do. A panel that showed
                        // the name without saying which half arrived would be
                        // claiming the whole profile.
                        let line = if look.table.is_empty() {
                            tr(lang, "Its baked tone curve and sliders do not render")
                        } else if (look.amount - 1.0).abs() <= 1e-3
                            && autoshade::dcp::find_look_table(&look.table).is_ok()
                        {
                            tr(lang, "Its creative colour table renders; its baked tone curve and sliders do not")
                        } else {
                            tr(lang, "Neither its creative colour table nor its baked tone curve and sliders render")
                        };
                        ui.label(egui::RichText::new(line).weak().small());
                    }
                }
                if self.recipe.passthrough.is_empty() {
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
                            self.recipe.passthrough.get(*k).map(|v| (*k, v.as_str()))
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
                rows(ui, Some(tr(lang, "Upright solver bookkeeping")), &autoshade::xmp::PASSTHROUGH_CRS);
                ui.add_space(SPACE_SM);
                ui.label(
                    egui::RichText::new(tr(
                        lang,
                        "Carried through to the sidecar unchanged; AutoShade never interprets these",
                    ))
                    .weak()
                    .small(),
                );
            });
        changed
    }

    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    fn dev_crop(&mut self, ui: &mut egui::Ui) -> bool {
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
            .any(|f| family_is_active(f, &self.recipe));
        card_fold(ui, egui::CollapsingHeader::new(section_title(tr(lang, "Crop"), crop_active))
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
                    let label = if self.crop.mode { tr(lang, "✓ Done") } else { tr(lang, "Enter crop") };
                    if action_in(ui, cell, true, label).clicked() {
                        let on = !self.crop.mode;
                        self.disarm_tools();
                        self.set_crop_mode(on);
                    }
                    if action_in(ui, cell, true, tr(lang, "Clear crop")).clicked()
                        && self.recipe.crop.take().is_some()
                    {
                        // Through the panel's own change path: clamp + dirty,
                        // so the crop-restricted histogram/clipping refresh.
                        changed = true;
                    }
                });
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new(tr(lang, "Aspect")).weak().small());
                    let prev_aspect = self.crop.aspect;
                    egui::ComboBox::from_id_salt("crop_aspect")
                        .selected_text(tr(lang, CROP_ASPECTS[self.crop.aspect].0))
                        .width(70.0)
                        .show_ui(ui, |ui| {
                            for (i, (name, _)) in CROP_ASPECTS.iter().enumerate() {
                                ui.selectable_value(&mut self.crop.aspect, i, tr(lang, name));
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
                    if self.crop.aspect != prev_aspect && self.crop.mode {
                        self.crop.aspect_pending = true;
                    }
                });
                // Straighten: rotate + auto-crop (engine rotate_straighten);
                // the preview shows exactly the export geometry. Fine class:
                // levelling needs 0.1°, not the wide-range integer snap.
                changed |= self.capability_rows(ui, crate::logic::capability::CROP);
                explainer(ui, lang, tr(lang, "Once in (R): drag corner/edge handles to resize, drag inside to move, drag OUTSIDE the box (or the canvas border while the box is full-frame) to rotate-straighten; arrows nudge the box, Enter commits; preview, export and XMP all match. Straighten auto-crops the black corners."));
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
        self.recipe.colour_field.as_ref().is_some_and(|f| f.renderable())
            || self.recipe.masks.iter().any(mask_active)
    }



    /// One develop-panel section — body extracted verbatim from
    /// develop_panel (round-12 decomposition; spacing included).
    fn dev_versions(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;

        // --- 版本: recipe snapshots ≈ LR virtual copies (gap batch G) --------
        // #14b, group 4 — DELIVERY (versions + export): what leaves the app,
        // as opposed to what is being edited. This boundary gets the same
        // fence + hairline + breather rhythm the other two group fences use
        // (Detail, Local Masks) — the rhythm is completed here, not changed:
        // Versions previously opened with a bare sibling-section fence while
        // being the head of a different KIND of group.
        group_caption(ui, tr(lang, "Versions & Export"));
        let n_ver = self.cards.versions.len();
        let n_ver_s = n_ver.to_string();
        // Deferred like the strip's, and dispatched through the same owner
        // after the section's closures release `self` (R24-5).
        let mut act: Option<VariantAction> = None;
        card_fold(ui, egui::CollapsingHeader::new(section_title(&trf(lang, "Versions ({n})", &[("n", &n_ver_s)]), n_ver > 0))
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
                ui.add_enabled_ui(!self.busy, |ui| {
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
                let n_var = self.variants.len();
                let busy = self.busy;
                #[cfg(test)]
                {
                    self.seams.edit_list_rows.clear();
                    self.seams.edit_list_actions.clear();
                }
                if n_var > 0 {
                    let accent = self.ui.theme.colors().accent_text;
                    ui.label(
                        egui::RichText::new(trf(
                            lang,
                            "Variants ({n})",
                            &[("n", &n_var.to_string())],
                        ))
                        .weak()
                        .small(),
                    );
                    let active_i = self.active;
                    for i in 0..n_var {
                        // Read the three facts the row shows BEFORE the layout
                        // closure borrows `self` (Variant is not Clone, and
                        // making it so for a display row would be the wrong
                        // fix — the card's texture handle rides on it).
                        let (kind, name, on_pixels) = {
                            let v = &self.variants[i];
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
                                        egui::RichText::new(tr(lang, "· current"))
                                            .weak()
                                            .small(),
                                    );
                                }
                                if kind == VariantKind::Edited {
                                    ui.label(
                                        egui::RichText::new(tr(lang, "· develop over AI pixels (no XMP)"))
                                            .weak()
                                            .small(),
                                    );
                                } else if !kind.is_source_based() {
                                    ui.label(
                                        egui::RichText::new(tr(lang, "· pixel-state (no XMP)"))
                                            .weak()
                                            .small(),
                                    );
                                } else if on_pixels {
                                    ui.label(
                                        egui::RichText::new(tr(lang, "· on baked pixels"))
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
                                    "Click to switch to this variant (lossless)",
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
                sub_caption(ui, tr(lang, "Snapshot history"));
                if action_row(ui, true, tr(lang, "＋ Save as version"))
                    .on_hover_text(tr(lang, "Snapshot this card's develop only as a numbered version (v<N>.recipe.json with frozen mask rasters); Ctrl+S saves every card"))
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
                if !self.cards.versions.is_empty() {
                    ui.checkbox(&mut self.cards.versions_current_only, tr(lang, "Only this variant"))
                        .on_hover_text(tr(
                            lang,
                            "Show only snapshots taken from the variant you are on. Versions with no recorded source (saved before this) are hidden while it is on.",
                        ));
                }
                let mut load: Option<u32> = None;
                let mut delete: Option<u32> = None;
                let mut rename: Option<u32> = None;
                let mut hidden = 0usize;
                // The list is cloned so the rows may borrow `self` mutably
                // (the rename buffer) — it is a handful of u32.
                let versions = self.cards.versions.clone();
                let photo = self.src_path.clone();
                for n in versions {
                    let meta = self.cards.version_meta.get(&n).cloned();
                    if self.cards.versions_current_only
                        && !Self::version_is_from(meta.as_ref(), &active_id, active_kind)
                    {
                        hidden += 1;
                        continue;
                    }
                    #[cfg(test)]
                    self.seams.edit_list_rows.push(format!("version:{n}"));
                    let named = meta.as_ref().and_then(|m| m.name.clone());
                    let editing = self
                        .cards.version_name_buf
                        .as_ref()
                        .is_some_and(|(p, j, ..)| *j == n && Some(p) == photo.as_ref());
                    ui.horizontal(|ui| {
                        ui.label(format!("v{n}"));
                        let field_id = ui.make_persistent_id(("version_name", n));
                        if editing {
                            let resp = {
                                let buf = &mut self
                                    .cards.version_name_buf
                                    .as_mut()
                                    .expect("checked just above")
                                    .3;
                                ui.add(
                                    egui::TextEdit::singleline(buf)
                                        .id(field_id)
                                        .desired_width(110.0)
                                        .hint_text(tr(lang, "Name")),
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
                                self.cards.version_name_buf = None;
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
                                None => tr(lang, "Name…").to_string(),
                            };
                            let text = match &named {
                                Some(_) => egui::RichText::new(shown),
                                None => egui::RichText::new(shown).weak(),
                            };
                            if ui
                                .add(egui::Label::new(text).sense(egui::Sense::click()))
                                .on_hover_text(tr(lang, "Name this snapshot"))
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
                                    "· from {kind}",
                                    &[("kind", tr(lang, kind.label()))],
                                )),
                                (None, Some(autoshade::store::VERSION_ORIGIN_AUTO)) => {
                                    Some(tr(lang, "· auto-archived").to_string())
                                }
                                _ => None,
                            };
                            if let Some(note) = note {
                                ui.label(egui::RichText::new(note).weak().small());
                            }
                        }
                        if action(ui, true, tr(lang, "Load")).on_hover_text(tr(lang, "Replace current parameters (one Ctrl+Z to undo)")).clicked() {
                            load = Some(n);
                        }
                        if glyph(ui, true, "🗑")
                            .on_hover_text(tr(lang, "Delete this snapshot (its frozen mask rasters go with it)"))
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
                            "{n} hidden — saved from another variant",
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
                    let cur = self
                        .cards.version_meta
                        .get(&n)
                        .and_then(|m| m.name.clone())
                        .unwrap_or_default();
                    self.cards.version_name_buf = Some((p, n, cur.clone(), cur));
                }
                if let Some(n) = load {
                    self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::LoadVersion(n),
                    )]);
                }
                if let Some(n) = delete
                    && let Some(src) = self.src_path.clone()
                {
                    // A rename buffer aimed at the row being deleted dies
                    // WITH it (R24-2): committing it afterwards would write a
                    // name for a number that no longer exists and can never
                    // be re-issued — a record only the next delete would
                    // clean up.
                    if self.cards.version_name_buf.as_ref().is_some_and(|(p, j, ..)| *j == n && *p == src)
                    {
                        self.cards.version_name_buf = None;
                    }
                    self.apply_actions(vec![logic::Action::Develop(
                        logic::develop::DevelopAction::DeleteVersion(n),
                    )]);
                }
                if n_ver == 0 {
                    explainer(ui, lang, tr(lang, "Like LR virtual copies: store multiple parameter sets for one photo (B&W, cropped…) without overwriting."));
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
    fn dev_export(&mut self, ui: &mut egui::Ui) -> bool {
        let lang = self.lang;

        // --- 导出设置 (UX batch): moved out of the toolbar — touched once per
        // delivery, these are Export-dialog contents, not toolbar chrome. The
        // toolbar keeps the ACTIONS; their hover echoes this section's state.
        ui.add_space(SPACE_MD);
        // Delivery preferences have no per-photo neutral, so this header has
        // no adjustment dot. The toolbar's Export hover carries the summary.
        card_fold(ui, egui::CollapsingHeader::new(tr(lang, "Export"))
            .id_salt("sec_export")
            .default_open(false)
            , |ui| {
                // WHERE first (R22-7): the delivery target is now a setting
                // rather than a property of which toolbar button was pressed,
                // and it is the one whose wrong value sends the file somewhere
                // the user has to hunt for — so it leads the section instead of
                // hiding under the encoder dials.
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Destination"));
                    egui::ComboBox::from_id_salt("exp_dest")
                        .selected_text(tr(lang, self.exp.dest.label()))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for d in ExportDest::ALL {
                                ui.selectable_value(&mut self.exp.dest, d, tr(lang, d.label()));
                            }
                        });
                });
                // The RESOLVED target, spelled out: "./out" is relative to
                // whichever directory the app was launched from, and "last used
                // folder" says nothing about which folder that is.
                ui.label(
                    egui::RichText::new(match self.export_dest_dir() {
                        Some(d) => abs_display(&d),
                        None => tr(lang, "a save dialog opens on every export").to_string(),
                    })
                    .weak()
                    .small(),
                );
                // One setting per row: two label+combo pairs in a non-wrapping
                // horizontal overflowed the 320px panel in Chinese.
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Format"));
                    egui::ComboBox::from_id_salt("save_fmt")
                        .selected_text(tr(lang, self.exp.format.label()))
                        .width(130.0)
                        .show_ui(ui, |ui| {
                            for f in ExportFormat::ALL {
                                ui.selectable_value(&mut self.exp.format, f, tr(lang, f.label()));
                            }
                        });
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Long edge"));
                    egui::ComboBox::from_id_salt("exp_long_edge")
                        .selected_text(if self.exp.long_edge == 0 {
                            tr(lang, "Original size").to_string()
                        } else {
                            format!("{} px", self.exp.long_edge)
                        })
                        .width(110.0)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.exp.long_edge, 0, tr(lang, "Original size"));
                            for px in [1600u32, 2048, 2560, 3840, 5120] {
                                ui.selectable_value(&mut self.exp.long_edge, px, format!("{px} px"));
                            }
                        });
                });
                Self::slider(ui, lang, tr(lang, "Output sharpening"), &mut self.exp.sharpen, 0.0, 100.0, 0.0);
                // Always allocated, merely disabled for TIFF — the old
                // appear/disappear reflowed every control to its right.
                ui.add_enabled_ui(self.exp.format == ExportFormat::Jpeg, |ui| {
                    Self::slider(ui, lang, tr(lang, "JPEG quality"), &mut self.exp.quality, 60.0, 100.0, 95.0);
                });
                ui.horizontal(|ui| {
                    ui.label(tr(lang, "Colour space"));
                    egui::ComboBox::from_id_salt("exp_space")
                        .selected_text(tr(lang, EXPORT_SPACES[(self.exp.space as usize).min(2)]))
                        .width(170.0)
                        .show_ui(ui, |ui| {
                            for (i, name) in EXPORT_SPACES.iter().enumerate() {
                                ui.selectable_value(&mut self.exp.space, i as u8, tr(lang, name));
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
                ui.checkbox(&mut self.exp.save_denoise, tr(lang, "🤖 AI Denoise on export")).on_hover_text(
                    ai_xref(lang, tr(lang, "AI denoise before developing, at the Export denoise strength below — a RAW on its sensor mosaic, an already-rendered image through its own denoiser — for high-ISO / astro (slow, GPU; needs the python sidecar). Batch render skips it, and so does a ◈ Denoised card, whose master is already denoised.")),
                );
                // The checkbox's OWN dial, directly under it (user decision
                // 2026-09-12): the Detail fold's 「AI Denoise now」 has its own
                // and neither reaches the other. Always allocated, merely
                // disabled while the checkbox is off — the appear/disappear
                // reflow rule the JPEG quality slider above follows.
                let denoise_raw = self.denoise_uses_raw();
                ui.add_enabled_ui(self.exp.save_denoise, |ui| {
                    Self::slider_pct_hinted(
                        ui,
                        lang,
                        tr(lang, "Export denoise strength"),
                        if denoise_raw { &mut self.exp.save_denoise_strength } else { &mut self.exp.baked_save_denoise_strength },
                        1.0,
                        if denoise_raw { autoshade::denoise::DEFAULT_STRENGTH_RAW } else { autoshade::denoise::DEFAULT_STRENGTH },
                        tr(lang,
                            "Export AI denoise strength, independent of the Detail section. On a RAW, the dial removes luminance grain after demosaic in linear light; the default targets Lightroom Denoise 50. Higher is cleaner, 100% is the complete network output. Every positive strength keeps clean colour; exactly 0% leaves the input untouched. Already-rendered images (JPEG, PNG, TIFF, a retouched or AI result) use a different denoiser with its own 50% default. Double-click to reset this source type.",
                        ),
                    );
                });
                explainer(ui, lang, tr(lang, "Applied by 「Export」 in the toolbar (Ctrl+Shift+E, or Ctrl+E) and by 「Render selected」 in the library. The ▾ beside Export delivers one file to a path you pick without touching the Destination."));
                // --- SF8-A: hand the Lightroom sidecar over ----------------
                // The XMP projection has always been written INSIDE the hashed
                // develop folder, which is the right home for state — but the
                // one thing users do with an XMP is give it to Lightroom, and
                // Lightroom only looks for it beside the photo. That left the
                // hand-off as "browse into %LOCALAPPDATA% yourself".
                ui.add_space(SPACE_LG);
                let raw = self.can_export_xmp_beside();
                let label = if self.exp.xmp_beside_confirm {
                    tr(lang, "⚠ Overwrite the .xmp already there")
                } else {
                    tr(lang, "Export .xmp beside the photo")
                };
                let hint = if !raw {
                    // The disabled REASON, not a repeat of the label: only a
                    // camera RAW has the sidecar convention (store::
                    // lightroom_sidecar and pipeline::write_xmp draw the same
                    // line), so a baked PNG/TIFF's neighbouring .xmp belongs to
                    // some other program and must not be written over.
                    tr(lang, "RAW only — a baked PNG/TIFF has no Lightroom sidecar convention, so its neighbouring .xmp belongs to another program")
                } else if self.exp.xmp_beside_confirm {
                    tr(lang, "A .xmp already sits beside this photo (Lightroom's own, or an earlier copy) — clicking again replaces it")
                } else {
                    tr(lang, "Copy this photo's stored Lightroom/ACR sidecar into the photo's own folder, where Lightroom reads it. Save the develop first — this delivers what is stored, not what is unsaved on the canvas.")
                };
                if action_row(ui, raw && !self.busy, label)
                    .on_hover_text(hint)
                    .clicked()
                {
                    self.export_xmp_beside();
                }
            });
        false
    }
}
