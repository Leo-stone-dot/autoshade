//! Retouch panel: paint/clone/fill/heal/AI tools.

use crate::*;

/// The brush radius range, in After-image display px. The ceiling was 80 —
/// a 160 px disc, too small to paint a sky on a large screen in one pass.
pub(crate) const BRUSH_R_MIN: f32 = 4.0;
pub(crate) const BRUSH_R_MAX: f32 = 200.0;
/// How long the ring stays up, with its size, after the size changes.
const BRUSH_SIZE_FLASH_S: f64 = 1.2;

impl AutoShadeApp {
    /// Brush-paint into the mask while dragging on the After image. The canvas
    /// is full-frame at preview resolution; pointer→canvas goes through the
    /// view transform so painting stays accurate at any zoom, and the brush
    /// radius converts display px → canvas px by the current pixel scale.
    pub(crate) fn handle_paint(&mut self, resp: &egui::Response, xf: ViewXform) {
        let brush = self.masks.brush;
        let tip = self.brush_tip();
        let (dims, deg, dist) = self.geom_ctx();
        // Mask-brush session: strokes write the display canvas AND the
        // greyscale weight buffer; erase clears both.
        let erase = self.masks.mask_brush.is_some_and(|(_, e)| e);
        let display_px = if erase {
            image::Rgba([0, 0, 0, 0])
        } else {
            image::Rgba([255, 64, 64, 160])
        };
        let gray_v: u8 = if erase { 0 } else { 255 };
        let mut gray = self.masks.mask_brush_gray.take();
        let Some(m) = self.masks.mask_paint.as_mut() else {
            self.masks.mask_brush_gray = gray;
            return;
        };
        let (mw, mh) = (m.width() as f32, m.height() as f32);
        // The canvas lives in the ORIGINAL frame (fill/heal edit source
        // pixels), so pointer positions map out of the transformed view.
        let to_mask = |p: egui::Pos2| {
            let (nx, ny) = xf.to_norm(p);
            let (ox, oy) = view_norm_to_orig(nx, ny, dims, deg, &dist);
            (ox * mw, oy * mh)
        };
        // Brush radius in MASK pixels, measured through the SAME nonlinear
        // view→original chain the centers use: map the pointer and a point one
        // display-radius to its right, take their mask-space distance. Under
        // straighten/distortion a fixed display brush covers a position-
        // dependent original-frame extent — the old constant horizontal-UV
        // conversion mismatched the painted footprint there. Neutral geometry
        // degenerates to exactly the old constant.
        let brush_at = |p: egui::Pos2| {
            let a = to_mask(p);
            // Probe BOTH directions and keep the larger. `ViewXform::to_norm`
            // CLAMPS to the viewport, so a rightward-only probe mapped to the
            // same point as the centre once the pointer reached the right
            // edge: the distance went to zero, the `.max(1.0)` floor took
            // over, and a 30 px brush stamped a single mask pixel exactly
            // where the user was painting along that edge. The chain is
            // smooth, so the direction with room measures the same radius.
            let probe = |dx: f32| {
                let b = to_mask(p + egui::vec2(dx, 0.0));
                ((b.0 - a.0).powi(2) + (b.1 - a.1).powi(2)).sqrt()
            };
            probe(brush).max(probe(-brush)).max(1.0)
        };
        // Union a brush segment's bounding box (± radius, canvas px) into the
        // pending dirty rect — ensure_mask_tex's partial-upload path.
        let grow = |rect: &mut Option<[u32; 4]>, a: (f32, f32), b: (f32, f32), r: f32| {
            let r = r + 1.0;
            let n = [
                (a.0.min(b.0) - r).floor().clamp(0.0, mw) as u32,
                (a.1.min(b.1) - r).floor().clamp(0.0, mh) as u32,
                (a.0.max(b.0) + r).ceil().clamp(0.0, mw) as u32,
                (a.1.max(b.1) + r).ceil().clamp(0.0, mh) as u32,
            ];
            *rect = Some(match rect {
                Some(o) => [o[0].min(n[0]), o[1].min(n[1]), o[2].max(n[2]), o[3].max(n[3])],
                None => n,
            });
        };
        let mut stamped = false;
        // Ink from the moment the PRIMARY button goes down on the canvas
        // (2026-10-13, user: 「操作感觉也不连贯」): egui reports a drag only
        // once the pointer has travelled ~6 px, and a press that stayed put
        // only as a click on RELEASE, so a stroke used to start late and a dab
        // appeared only when the button came up. Primary only — a secondary
        // or middle drag is the caller's pan — and never with Space held, the
        // pan chord, whose press frame comes before the caller sees a drag.
        let down = resp.is_pointer_button_down_on()
            && resp.ctx.input(|i| i.pointer.primary_down() && !i.key_down(egui::Key::Space));
        if down {
            if let Some(p) = resp.interact_pointer_pos().or(resp.hover_pos()) {
                let cur = to_mask(p);
                let r = brush_at(p);
                match self.masks.paint_last {
                    Some(prev) => {
                        stamped = stamp_line_px(m, prev, cur, r, display_px, tip);
                        if let Some(g) = gray.as_mut() {
                            stamp_line_gray(g, prev, cur, r, gray_v, tip);
                        }
                    }
                    // First stroke frame: from the PRESS ORIGIN (on the press
                    // frame itself that is the pointer, so a still press
                    // stamps one dab at once).
                    None => {
                        let start = ui_press_origin(resp).map(&to_mask).unwrap_or(cur);
                        stamped = stamp_line_px(m, start, cur, r, display_px, tip);
                        if let Some(g) = gray.as_mut() {
                            stamp_line_gray(g, start, cur, r, gray_v, tip);
                        }
                        grow(&mut self.overlay.mask_dirty_rect, start, cur, r);
                    }
                }
                grow(&mut self.overlay.mask_dirty_rect, self.masks.paint_last.unwrap_or(cur), cur, r);
                self.masks.paint_last = Some(cur);
            }
        } else {
            // No stroke is in flight while the button is up — clearing here
            // (not just on drag_stopped) also covers strokes interrupted by
            // Esc / hold-B / an Alt source-pick, whose drag_stopped frame
            // never reaches this handler and used to leave a stale anchor
            // that drew a full-width connecting streak into the next stroke.
            self.masks.paint_last = None;
        }
        self.masks.mask_brush_gray = gray;
        if stamped {
            if erase {
                self.paint_mask_changed(None);
            } else {
                self.paint_mask_changed(Some(true));
            }
        }
    }

    /// Clone-stamp interaction: Alt+click picks the SOURCE point (stored in
    /// the original frame like every pixel-path coordinate); plain drags paint
    /// the target into the Stamp's own area. The picked source stays marked
    /// with a crosshair ring so the offset is always visible.
    pub(crate) fn handle_clone(&mut self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) {
        let alt = ui.input(|i| i.modifiers.alt);
        if alt {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            if resp.clicked()
                && let Some(q) = resp.interact_pointer_pos()
            {
                let (dims, deg, dist) = self.geom_ctx();
                let (nx, ny) = xf.to_norm(q);
                self.tools.clone_src = Some(view_norm_to_orig(nx, ny, dims, deg, &dist));
                self.status = tr(
                    self.lang,
                    "Clone source sampled — brush the area to cover, then 「⎘ Clone painted area」",
                )
                .into();
            }
        } else {
            self.handle_paint(resp, xf);
        }
        if let Some((sx, sy)) = self.tools.clone_src {
            let (dims, deg, dist) = self.geom_ctx();
            let (vx, vy) = orig_norm_to_view(sx, sy, dims, deg, &dist);
            let q = xf.to_screen(vx, vy);
            let p = ui.painter_at(xf.rect);
            // ACCENT, per the two-tier colour rule: on-canvas tool overlays
            // never wear the PILL chrome gold — a source pin sampled on skin,
            // sand or a sunset sky simply vanished in gold.
            p.circle_stroke(q, 9.0, egui::Stroke::new(2.0, ACCENT));
            p.line_segment(
                [q - egui::vec2(13.0, 0.0), q + egui::vec2(13.0, 0.0)],
                egui::Stroke::new(1.0, ACCENT),
            );
            p.line_segment(
                [q - egui::vec2(0.0, 13.0), q + egui::vec2(0.0, 13.0)],
                egui::Stroke::new(1.0, ACCENT),
            );
        }
    }

    /// R27 — every pixel worker takes this gate: heal, fill, adjust, clone,
    /// AI denoise, stack and reimagine. They take the PHOTO PATH and
    /// re-develop it through `render::source_pixels`, which applies the EXIF
    /// orientation and NOTHING else, while the canvas is in the frame
    /// `recipe.quarter_turns` produces. On a turned photo those are different
    /// frames, and each door suffers it in its own way:
    ///   * the masked four (heal, fill, adjust, clone) export their mask from
    ///     the turned canvas, so the repair lands on the wrong pixels — BAKED,
    ///     into a master no slider can undo;
    ///   * the mask-free three (AI denoise, stack, reimagine) land their
    ///     master as a ◈/▦/✨ card whose raster is in the EXIF frame:
    ///     `load_active` installs it as the plate, and `sync_base_turns`
    ///     cannot turn a baked card — the canvas shows it sideways.
    ///
    /// Refused with a reason rather than approximated. Threading the turn
    /// through `source_pixels` is its own change: that function is THE
    /// raw-vs-baked dispatch and denoise / match / reverse-fit share it.
    /// Registered limitation (ROADMAP, R27 Batch-2 ②).
    ///
    /// The AI mask refine (`masks.rs`) takes it too, since 2026-09-24: its
    /// guide is decoded in the EXIF frame and the raster it refines was
    /// turned into the plate frame.
    ///
    /// Returns `true` when the caller must stop.
    pub(crate) fn refuse_pixel_work_on_a_turned_photo(&mut self) -> bool {
        if self.recipe.quarter_turns.is_multiple_of(4) {
            return false;
        }
        self.status = tr(
            self.lang,
            "this photo is turned, and pixel repairs still work on the un-turned frame — turn it back to 0 first",
        )
        .into();
        true
    }

    /// Generative fill: the model is shown THIS card's developed picture —
    /// the card's own pixel source under its live recipe, in the original
    /// frame the mask is painted in (geometry is a view, not a pixel) — the
    /// painted area is regenerated from the prompt (blank = remove), and the
    /// result lands as a NEW「✨ AI generated」card whose look lives in its
    /// pixels; the card you filled from is untouched. Heal / clone / denoise
    /// stay in-place touch-ups of the neutral master: they are pixel
    /// arithmetic, not a model that has to see the picture. Through v1.3.4
    /// this too was in-place on the neutral base, so the model was shown a
    /// develop 0.6–1.4 EV under the card's Before with none of its sliders
    /// or masks, and the card's content-keyed corrections then landed on
    /// content they were never solved for (user, 2026-09-15: 「不是基于当前
    /// 变体生成的，而是原图」). Runs on a worker thread.
        /// Edit the active generated card's live picture. The fold's own painted
    /// area (`BrushOwner::Adjust`) chooses the region path at the click; blank
    /// means remove there.
    /// Every answer is a new ✨ card, so the source stays immutable and the
    /// result can be adjusted again or reverse-fit like any generated card.
        /// Heal: AI auto-detect (use_mask=false) or the painted mask (use_mask=true).
    /// Pixel retouch from surrounding real pixels; saves to ./out. `limit` is
    /// a mask raster the heal is held to (`retouch::heal`'s limit): the
    /// Portraits panel's 🤖 Remove blemishes passes the face mask's; the
    /// Retouch panel passes none.
        /// AI denoise as an ACTIVE canvas operation (2026-09-15): denoise the
    /// current card's pixel source at full resolution — a RAW on its sensor
    /// mosaic before demosaic, a baked source through SCUNet — and land the
    /// master as a NEW ◈ Denoised card (`RetouchKind::NewDenoised`) that
    /// carries this card's develop. The card you started from, the ▣
    /// negative included, keeps its pixels; the ◈ card is deleted, not
    /// undone. The ≤2048 px working-copy tier retired with the in-place
    /// landing: a master baked from it capped every later export at 2048 px.
        /// Stack this card's frame with others (v1.5.0 Track S).
    ///
    /// The ACTIVE card's source is frame 0 — the reference, whose framing and
    /// whose exposure the merge keeps (`stack::merge::merge`) — and `extra`
    /// are the frames the user picked beside it. The merged master lands as a
    /// NEW ▦ card carrying this card's develop, on exactly the ◈ denoise
    /// card's terms: the frame you started from keeps its own pixels.
    ///
    /// Full resolution, always, for the denoise landing's reason: a master
    /// baked from a working copy caps every later export at that size.
        /// Run the clone stamp on a worker: painted target mask + the Alt+picked
    /// source point → `retouch::clone_stamp` (deterministic, no AI) → ./out
    /// pixel master shown in the After pane, exactly like heal.
        /// Full-frame generative re-render via gpt-image — the OPTIONAL "let GPT
    /// directly make the picture" path. Uses the Direction text as the look
    /// prompt. Unlike Analyze (a faithful parametric recipe), this REGENERATES
    /// pixels — a creative restyle (up to ~8 MP on flexible-size models, ~1.5K
    /// on older ones). The result enters the strip as a new「AI 生成」variant
    /// (`origin = Some(out)`); its saved path is the reverse-fit ("反推配方")
    /// target that turns the look back into sliders + XMP at full resolution.
        /// Extract a reusable STYLE PROMPT from the before/after pair via the vision
    /// model. The result lands in the Direction box (ready to reimagine OTHER
    /// photos with the same look) and is saved to ./out/<stem>.style.txt.
        /// The brush-radius slider, rendered wherever a brush is actually IN USE
    /// (#9). There is exactly ONE radius in the app — `self.masks.brush`, also
    /// driven by the `[` / `]` keys (see the shortcut in app.rs) — and until
    /// R22 its only slider lived in the Retouch panel, beside Fill / Heal /
    /// Stamp. The mask brush, which starts from Develop → Local Masks and
    /// paints in its own session, had no size control at its own site at all:
    /// the widget stayed with its historical host instead of following the
    /// tool. Both renders bind this same field on purpose (two live sliders
    /// over one value is house precedent — global and per-mask Exposure), so
    /// the tooltip discloses the sharing rather than pretending there are two
    /// brushes.
    /// The brush's footprint where it will land (2026-10-13, user: 「点了之后
    /// 不知道大小」): a ring of the brush's radius under the pointer — dark
    /// outside, light inside, so it reads on sky and on shadow alike — with
    /// a centre dot, in place of the OS cursor. For a second after the size
    /// changes ([ / ] or the slider) the ring also shows with the size beside
    /// it, at the canvas centre when the pointer is off the canvas. Erasing
    /// marks the centre with a minus.
    pub(crate) fn draw_brush_cursor(&self, ui: &egui::Ui, resp: &egui::Response, rect: egui::Rect) {
        let now = ui.input(|i| i.time);
        let resized = self.masks.brush_resized_at.is_some_and(|t| now - t < BRUSH_SIZE_FLASH_S);
        let Some(c) = resp.hover_pos().or_else(|| resized.then(|| rect.center())) else { return };
        if resp.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::None);
        }
        let p = ui.painter_at(rect);
        let r = self.masks.brush;
        p.circle_stroke(c, r, egui::Stroke::new(3.0, egui::Color32::from_black_alpha(150)));
        p.circle_stroke(c, r, egui::Stroke::new(1.25, egui::Color32::from_white_alpha(235)));
        // Lightroom's second ring: where the feather starts to fade.
        let inner = r * (1.0 - self.masks.brush_feather);
        if self.masks.brush_feather > 0.005 && inner > 2.0 {
            p.circle_stroke(c, inner, egui::Stroke::new(1.0, egui::Color32::from_white_alpha(140)));
        }
        if self.masks.mask_brush.is_some_and(|(_, erase)| erase) {
            let s = (r * 0.35).clamp(3.0, 8.0);
            p.line_segment([c - egui::vec2(s, 0.0), c + egui::vec2(s, 0.0)], egui::Stroke::new(1.5, egui::Color32::WHITE));
        } else {
            p.circle_filled(c, 1.5, egui::Color32::WHITE);
        }
        if resized {
            let label = format!("{} {:.0}", tr(self.lang, "Brush size"), r);
            let at = c + egui::vec2(0.0, r + 8.0);
            let font = egui::TextStyle::Small.resolve(ui.style());
            p.text(at + egui::vec2(1.0, 1.0), egui::Align2::CENTER_TOP, &label, font.clone(), egui::Color32::from_black_alpha(200));
            p.text(at, egui::Align2::CENTER_TOP, &label, font, egui::Color32::WHITE);
            // Keep frames coming until the flash has run out.
            ui.ctx().request_repaint();
        }
    }

    /// The tip every brush paints with: the Feather and Flow dials beside
    /// Brush size (both shared by every brush, as the radius is).
    pub(crate) fn brush_tip(&self) -> BrushTip {
        BrushTip { feather: self.masks.brush_feather, flow: self.masks.brush_flow }
    }

    /// Set the one brush radius, clamped to its range, and flash the ring.
    pub(crate) fn set_brush(&mut self, radius: f32, now: f64) {
        self.masks.brush = radius.clamp(BRUSH_R_MIN, BRUSH_R_MAX);
        self.masks.brush_resized_at = Some(now);
    }

    pub(crate) fn brush_size_slider(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        // The band the slider occupies, read off the layout cursor before and
        // after — a wrapper (scope/horizontal) would be one more thing that
        // can move the geometry the width tests pin. Test seam only.
        #[cfg(test)]
        let before = ui.cursor();
        // The radius is live tool state, never a recipe field: no dirty flag,
        // no redevelop — a change only flashes the ring on the canvas.
        let mut radius = self.masks.brush;
        if Self::slider_hinted(
            ui,
            lang,
            tr(lang, "Brush size"),
            &mut radius,
            BRUSH_R_MIN,
            BRUSH_R_MAX,
            30.0,
            tr(lang, "One radius for every brush: the mask brush and the Fill / Heal / Stamp brush are the same size ([ / ] move it too)"),
        ) {
            let now = ui.input(|i| i.time);
            self.set_brush(radius, now);
        }
        // Feather and Flow (2026-10-13): Lightroom's two brush dials, shared
        // by every brush like the radius. Feather fades the disc's outer part
        // (the inner ring on the canvas marks where the fade starts); Flow is
        // how much one pass lays down, so a low flow builds up stroke by
        // stroke. The defaults are the hard full-strength brush of before.
        Self::slider_pct_hinted(
            ui,
            lang,
            tr(lang, "Feather"),
            &mut self.masks.brush_feather,
            1.0,
            0.0,
            tr(lang, "How soft the brush edge is: 0 = a hard edge, 100 = fading from the centre. The inner ring on the image marks where the fade starts."),
        );
        Self::slider_pct_hinted(
            ui,
            lang,
            tr(lang, "Flow"),
            &mut self.masks.brush_flow,
            1.0,
            1.0,
            tr(lang, "How much one pass lays down: 100 = full strength at once; lower builds up gradually as you paint over the same place."),
        );
        #[cfg(test)]
        {
            // x from the Ui's USED region (cursor().max.x can be infinite in an
            // unbounded layout), y from the cursor the slider just advanced.
            let used = ui.min_rect();
            self.seams.brush_slider_rect = Some(egui::Rect::from_min_max(
                egui::pos2(used.left(), before.top()),
                egui::pos2(used.right(), ui.cursor().top()),
            ));
        }
    }

    /// ONE tool's brush controls, drawn inside that tool's fold (user
    /// decision 2026-09-27: the brush a fold's verb reads sits in that fold,
    /// and each fold has a painted area of its own — `BrushOwner`). The arm
    /// toggle and the clear verb share a two-cell row; the radius slider
    /// follows while this tool's brush is armed. The Clone Stamp arms through
    /// its own 「⎘ Enter stamp」 toggle and lays its row itself.
    pub(crate) fn brush_row(&mut self, ui: &mut egui::Ui, owner: BrushOwner) {
        let lang = self.lang;
        let armed = self.brush_armed(owner);
        ui.horizontal(|ui| {
            let cell = columns(ui, 2);
            if toggle_in(ui, cell, armed, tr(lang, "🖌 Paint area"))
                .on_hover_text(tr(lang, "Brush over the area this tool works on (box-select pauses while the brush is armed); click again or press Esc to put the brush away. Each tool keeps its own painted area."))
                .clicked()
            {
                if armed {
                    self.disarm_brush();
                } else {
                    self.arm_brush(owner);
                }
            }
            if action_in(ui, cell, true, tr(lang, "Clear area"))
                .on_hover_text(tr(lang, "Wipe this tool's painted area (the other tools' areas stay)"))
                .clicked()
            {
                self.clear_area(owner);
            }
        });
        if armed {
            self.brush_size_slider(ui);
        }
    }

    /// The card's fill layers (2026-10-01): one row each — 👁 shows or hides
    /// it (a hidden layer is kept and saved), its name, ✕ removes it from the
    /// card — and its opacity under it. Every change is a recipe edit like a
    /// slider's, so Ctrl+Z covers it; the file in `./out` is never deleted,
    /// so an undone removal has its pixels to come back to.
    fn layer_rows(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang;
        if self.recipe.pixel_layers.is_empty() {
            return;
        }
        sub_caption(ui, tr(lang, "Layers on this card"));
        let mut remove = None;
        let mut changed = false;
        for i in 0..self.recipe.pixel_layers.len() {
            ui.horizontal(|ui| {
                let layer = &mut self.recipe.pixel_layers[i];
                if glyph_toggle(ui, layer.enabled, "👁")
                    .on_hover_text(tr(lang, "Show or hide this layer — a hidden layer is kept and saved with the card"))
                    .clicked()
                {
                    layer.enabled = !layer.enabled;
                    changed = true;
                }
                ui.label(trf(lang, "Fill layer {n}", &[("n", &(i + 1).to_string())]))
                    .on_hover_text(layer.path.as_str());
                if glyph(ui, true, "✕")
                    .on_hover_text(tr(lang, "Remove this layer from the card (Ctrl+Z brings it back)"))
                    .clicked()
                {
                    remove = Some(i);
                }
            });
            changed |= Self::slider_pct_hinted(
                ui,
                lang,
                tr(lang, "Layer opacity"),
                &mut self.recipe.pixel_layers[i].opacity,
                1.0,
                1.0,
                tr(lang, "How strongly this layer covers the develop under it; 100% is the fill as it was made."),
            );
        }
        if let Some(i) = remove {
            self.recipe.pixel_layers.remove(i);
            changed = true;
        }
        if changed {
            self.session.dirty = true;
        }
    }

    pub(crate) fn retouch_panel(&mut self, ui: &mut egui::Ui) {
        let lang = self.lang; // Copy — never borrows self, safe inside egui closures.
        ui.add_space(SPACE_LG);
        panel_heading(ui, tr(lang, "Retouch"), false, |_| {});

        // Whole-image Reimagine + reverse-fit MOVED to the AI panel (R22 #4,
        // panels/ai.rs): they are AI develop verbs, and living here put them
        // between the brush tools — a generative re-render of the whole frame
        // sitting in the middle of dust-spot healing. Everything below is
        // PIXEL work on the current rendition.
        //
        // v1.5.0 F9 — what arrived WITH the photograph, above every tool the
        // photographer might reach for, because it is not a tool at all: it is
        // a statement about what is already on this canvas. Its own group, per
        // the user's 2026-09-12 directive that a control belongs to its own
        // section rather than borrowing a neighbour's.
        //
        // Silent on the photographs that carry none, which is 150 of the
        // reference library's 175.
        if !self.recipe.retouch.is_empty() {
            let total = self.recipe.retouch.len();
            let synth =
                self.recipe.retouch.iter().filter(|a| a.origin.is_synthesised()).count();
            sub_caption(ui, tr(lang, "Imported removal"));
            ui.label(
                egui::RichText::new(trf(
                    lang,
                    "Lightroom removed {n} area(s) here; re-solved from this photo's own pixels.",
                    &[("n", &total.to_string())],
                ))
                .weak()
                .small(),
            );
            if synth > 0 {
                // The sentence that keeps the panel honest: for these areas
                // the picture on screen is THIS app's repair, and Adobe's own
                // result is not in the sidecar to be shown.
                ui.label(
                    egui::RichText::new(trf(
                        lang,
                        "{n} of them were synthesised by Adobe — those pixels are not in the sidecar, so what you see is this app's repair.",
                        &[("n", &synth.to_string())],
                    ))
                    .weak()
                    .small(),
                );
                ui.add_enabled_ui(!self.busy, |ui| {
                    if action_row(ui, true, tr(lang, "✨ Regenerate those areas"))
                        .on_hover_text(tr(lang, "Paint the synthesised areas into Generative Fill's painted area and run the generative model over exactly them — an empty prompt removes, and the result lands as a layer over this card (gpt-image API call — costs per image)"))
                        .clicked()
                    {
                        // Paint first, and only spawn if something landed —
                        // `start_fill` over a blank mask would refuse with a
                        // status line that reads like the button is broken.
                        if self.paint_imported_removals() > 0 {
                            self.start_fill();
                        }
                    }
                });
            }
        }

        // Every brush lives in the fold whose verb reads it, with a painted
        // area of its own (user decision 2026-09-27; `brush_row`, `BrushOwner`)
        // — the one 「Brush (shared)」 group that hung bare above these three
        // folds, and that the AI panel's Adjust fold read without a control of
        // its own, is gone.
        ui.add_space(SPACE_XS);
        card_fold(ui, egui::CollapsingHeader::new(tr(lang, "Generative Fill"))
            .id_salt("sec_fill")
            .default_open(false)
            // Test-only force-open: this fold hides a width-pinned prompt row
            // (see util::fold_open_in_tests).
            .open(fold_open_in_tests())
            , |ui| {
                // #14a: was `desired_width(f32::INFINITY)`.
                let _field = prompt_field(
                    ui,
                    &mut self.genai.fill_prompt,
                    tr(lang, "what to change — e.g. remove the wires on the left, make the sky bluer; or paint an area (empty then removes it)"),
                );
                #[cfg(test)]
                {
                    self.seams.prompt_rects.push(_field.rect);
                }
                self.brush_row(ui, BrushOwner::Fill);
                // R38: wrapped — quality and the full-res switch share a row
                // that wraps when short; the verb fills the row under them
                // (2026-09-27: a row's one verb takes the whole row).
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt("fill_quality")
                        .selected_text(tr(lang, ["high", "medium", "low"][self.genai.fill_quality.min(2)]))
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut self.genai.fill_quality, 0, tr(lang, "high"));
                            ui.selectable_value(&mut self.genai.fill_quality, 1, tr(lang, "medium"));
                            ui.selectable_value(&mut self.genai.fill_quality, 2, tr(lang, "low"));
                        })
                        .response
                        .on_hover_text(tr(lang, "gpt-image render quality — higher looks better and costs more per image"));
                    let src_is_raw = self
                        .active_source_path()
                        .is_some_and(|p| autoshade::decode::is_raw(&p));
                    // Each 「Full-res」 names its verb (R22 #16): fill / heal /
                    // clone / denoise had four identically-labelled checkboxes
                    // across three panels, and this is the only one still gated
                    // RAW-only — a distinction its label could not carry while it
                    // was a twin of the other three.
                    ui.add_enabled(src_is_raw, egui::Checkbox::new(&mut self.genai.fill_fullres, tr(lang, "Full-res fill")))
                        .on_hover_text(tr(lang, "Composite onto the full-sensor develop (slow, RAW only)"));
                });
                // One verb, two ways in (2026-10-12): a painted area is
                // regenerated as before; with none, the words alone go to the
                // model and it chooses where (`start_fill`).
                let painted = self.area_painted(BrushOwner::Fill);
                let (verb, hint) = if painted {
                    (
                        tr(lang, "Remove / Fill"),
                        tr(lang, "Regenerate ONLY the painted area — from your prompt, or as a removal when the prompt is empty (gpt-image API call — costs per image); the model sees this card's look, and the result is a layer over this card's develop — the card's own pixels stay as they are"),
                    )
                } else {
                    (
                        tr(lang, "✨ AI edit"),
                        tr(lang, "Send this card's picture with your words and no painted area: the model decides where to change it, and only what it changed lands as a layer over this card (gpt-image API call — costs per image)"),
                    )
                };
                let ready = !self.busy && (painted || !self.genai.fill_prompt.trim().is_empty());
                if primary_row(ui, ready, verb).on_hover_text(hint).clicked() {
                    self.start_fill();
                }
                explainer(ui, lang, tr(lang, "Write what to change and press ✨ AI edit — the model finds where. Or paint an area first: an empty prompt then removes it, words fill it. Either way the result is a layer over this card (its own pixels never change), levelled to the colours around it; a layer keeps the look it was made with, so after a large slider change, run it again. Needs an image API (OPENAI_API_KEY, or the OAuth image bridge in Settings)."));
                self.layer_rows(ui);
            });

        card_fold(ui, egui::CollapsingHeader::new(tr(lang, "Heal (pixel)"))
            .id_salt("sec_heal")
            .default_open(false)
            , |ui| {
                self.brush_row(ui, BrushOwner::Heal);
                // R38: the two heal verbs share one aligned row and the
                // full-res switch sits under them — three free-width controls
                // in a non-wrapping row overran the default panel width.
                ui.horizontal(|ui| {
                    let cell = columns(ui, 2);
                    ui.add_enabled_ui(!self.busy, |ui| {
                        // 🤖, the one AI prefix app-wide (#4): ✦ was this
                        // verb's private glyph. The tooltip carries the
                        // AI-panel cross-reference — this stays HERE, at
                        // the pixels it heals.
                        if action_in(ui, cell, true, tr(lang, "🤖 AI heal (auto)"))
                            .on_hover_text(ai_xref(lang, tr(lang, "A vision model finds small dust spots / blemishes (API call), then each is healed from surrounding REAL pixels — never generated")))
                            .clicked()
                        {
                            self.start_heal(false, None);
                        }
                        if action_in(ui, cell, true, tr(lang, "Heal area"))
                            .on_hover_text(tr(lang, "Heal the brushed area from surrounding real pixels — local compute, no API"))
                            .clicked()
                        {
                            self.start_heal(true, None);
                        }
                    });
                });
                // Enabled for BAKED sources too (L09#4, user decision
                // 2026-08-11): heal honours the flag on both source
                // types since b4c6c30, but the RAW-only gate left a
                // 61 MP baked TIFF no way to opt out of the 2048px
                // downsample the unchecked path bakes into the master.
                ui.checkbox(&mut self.genai.heal_fullres, tr(lang, "Full-res heal"))
                    .on_hover_text(tr(lang, "Heal at full resolution (slow; without it the retouched image is saved at 2048px)"));
                explainer(ui, lang, tr(lang, "AI auto-detects dust / blemishes, or paint a mask and Heal it. Pixel retouch from surrounding pixels; saved to the delivery folder."));
            });

        card_fold(ui, egui::CollapsingHeader::new(tr(lang, "Clone Stamp"))
            .id_salt("sec_clone")
            .default_open(false)
            , |ui| {
                // The stamp's arm toggle IS its brush toggle (Alt+click samples
                // a source, plain drags paint the Stamp's own area), so the
                // clear verb sits beside it, the radius follows while armed,
                // and the stamp's one verb fills the row under them.
                let armed = self.brush_armed(BrushOwner::Stamp);
                ui.horizontal(|ui| {
                    let cell = columns(ui, 2);
                    let label = if armed { tr(lang, "✓ Done") } else { tr(lang, "⎘ Enter stamp") };
                    if action_in(ui, cell, true, label)
                        .on_hover_text(tr(lang, "Arm the stamp: Alt+click samples a source, the brush paints the target; your painted mask survives"))
                        .clicked()
                    {
                        if armed {
                            self.disarm_brush();
                        } else {
                            // Arming brings the Stamp's own area onto the
                            // canvas; what was painted for Fill / Heal stays
                            // theirs. Only 「Clear area」 wipes.
                            self.arm_brush(BrushOwner::Stamp);
                            self.status =
                                tr(lang, "Stamp: Alt+click to set the source → brush the target area → 「⎘ Clone painted area」").into();
                        }
                    }
                    if action_in(ui, cell, true, tr(lang, "Clear area"))
                        .on_hover_text(tr(lang, "Wipe this tool's painted area (the other tools' areas stay)"))
                        .clicked()
                    {
                        self.clear_area(BrushOwner::Stamp);
                    }
                });
                if armed {
                    self.brush_size_slider(ui);
                }
                if primary_row(ui, !self.busy && self.tools.clone_mode, tr(lang, "⎘ Clone area"))
                    .on_hover_text(tr(lang, "Copy the sampled source over the brushed area verbatim (feathered edges, no tone matching) — local compute"))
                    .clicked()
                {
                    self.start_clone();
                }
                // Enabled for BAKED sources too — clone_stamp honours
                // the flag on both source types (retouch.rs), the same
                // rule as Heal beside it (L15-7).
                ui.checkbox(&mut self.tools.clone_fullres, tr(lang, "Full-res clone"))
                    .on_hover_text(tr(lang, "Clone at full resolution (slow; without it the retouched image is saved at 2048px)"));
                explainer(ui, lang, tr(lang, "Photoshop-style clone stamp: Alt+click to sample a source (cross marker), brush the area to \
                         cover, and pixels are carried over as-is from the source (feathered edges, no tone matching). \
                         Local compute, saves a pixel master in the delivery folder."));
            });
    }
}
