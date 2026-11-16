//! The open photo's edit as ONE aggregate: what a save persists, what a
//! render reads, what an undo step swaps — owned together so a feature has a
//! home that is not `AutoShadeApp`.
//!
//! Its six state objects are PRIVATE. The only ways in are the read accessors
//! (`doc()`, `photo()`, …), the mutation accessors, and the methods that own a
//! multi-step invariant: `active_variant_mut`, `sync_active_recipe`,
//! `flush_active_variant`, `insert_variant_after_active` and `history_mut`.
//! The point of the aggregate is ownership: these state objects die together
//! when another photo opens and are never splintered across clients.

use crate::*;
use crate::actions::PersistedCard;
use crate::actions::ActiveBaseline;

pub(crate) struct Editor {

    /// The open photo and its decoded plates (see [`PhotoPixels`]).
    photo: PhotoPixels,
    /// The canvas textures and their identity (see [`CanvasPlates`]).
    plates: CanvasPlates,
    /// The latest-wins develop scheduler's counters (see [`DevelopScheduler`]).
    scheduler: DevelopScheduler,
    /// The open document — recipe, cards, active index (see [`Document`]).
    doc: Document,
    /// The AI analysis session (see [`AnalysisSession`]).
    analysis: AnalysisSession,
    // WHERE an export lands (R22-7) — the setting that replaced the
    // Export / Download… button pair. See [`ExportDest`].
    // The folder the last export landed in; ExportDest::LastUsed delivers
    // there, and the ask-dialog reopens there.
    // The 「export .xmp beside the photo」 button is armed for a SECOND click:
    // a sidecar already sits beside this photo (Lightroom's own, or an earlier
    // copy) and the next click overwrites it. Session state, cleared on every
    // photo open and after the copy lands — a two-click confirm rather than a
    // modal, matching how the rest of this panel confirms in place.
    // --- undo / redo (a drag is one step, committed on release). Each step
    // carries the recipe AND the active variant's pixel identity (base Arc +
    // origin), so a baked pixel retouch (heal / clone / denoise) is
    // one undoable step instead of a point of no return. Arc clones share the
    // allocation — only a retouch introduces a new raster into history.
    /// The undo history and the ● baseline; see [`HistoryState`].
    /// The per-photo editing session (see [`EditorSession`]).
    session: EditorSession,
}

impl Editor {
    pub(crate) fn photo(&self) -> &PhotoPixels {
        &self.photo
    }

    pub(crate) fn photo_mut(&mut self) -> &mut PhotoPixels {
        &mut self.photo
    }

    pub(crate) fn plates(&self) -> &CanvasPlates {
        &self.plates
    }

    pub(crate) fn plates_mut(&mut self) -> &mut CanvasPlates {
        &mut self.plates
    }

    pub(crate) fn scheduler(&self) -> &DevelopScheduler {
        &self.scheduler
    }

    pub(crate) fn scheduler_mut(&mut self) -> &mut DevelopScheduler {
        &mut self.scheduler
    }

    pub(crate) fn doc(&self) -> &Document {
        &self.doc
    }

    pub(crate) fn doc_mut(&mut self) -> &mut Document {
        &mut self.doc
    }

    pub(crate) fn analysis(&self) -> &AnalysisSession {
        &self.analysis
    }

    pub(crate) fn analysis_mut(&mut self) -> &mut AnalysisSession {
        &mut self.analysis
    }

    pub(crate) fn session(&self) -> &EditorSession {
        &self.session
    }

    pub(crate) fn session_mut(&mut self) -> &mut EditorSession {
        &mut self.session
    }

    /// The variant the document shows.
    pub(crate) fn active(&self) -> usize {
        self.doc.active
    }

    /// The active variant, mutably — the canvas→card sync's one borrow.
    pub(crate) fn active_variant_mut(&mut self) -> Option<&mut Variant> {
        let active = self.doc.active;
        self.doc.variants.get_mut(active)
    }

    /// Copy the live recipe into the ACTIVE variant (the edit→card flush).
    pub(crate) fn sync_active_recipe(&mut self) {
        let recipe = self.doc.recipe.clone();
        if let Some(cur) = self.doc.variants.get_mut(self.doc.active) {
            cur.recipe = recipe;
        }
    }

    /// Sync the active variant and, when no frame landed for the snapshot,
    /// drop its stale thumb (the switch/push card rule, L06#6).
    pub(crate) fn flush_active_variant(&mut self, drop_thumb: bool) {
        let recipe = self.doc.recipe.clone();
        if let Some(cur) = self.doc.variants.get_mut(self.doc.active) {
            cur.recipe = recipe;
            if drop_thumb {
                cur.thumb = None;
            }
        }
    }

    /// Insert a variant after the active one and make it active.
    pub(crate) fn insert_variant_after_active(&mut self, variant: Variant) {
        let at = (self.doc.active + 1).min(self.doc.variants.len());
        self.doc.variants.insert(at, variant);
        self.doc.active = at;
    }

    /// The undo/redo history — one mutable borrow for a multi-stack move.
    pub(crate) fn history_mut(&mut self) -> &mut HistoryState {
        &mut self.session.hist
    }
}

impl Default for Editor {
    fn default() -> Self {
        Self {
            photo: PhotoPixels::default(),
            plates: Default::default(),
            scheduler: Default::default(),
            doc: Default::default(),
            analysis: Default::default(),
            session: Default::default(),
        }
    }
}

impl Editor {
    pub(crate) fn persisted_card_for(&self, live_position: usize) -> Option<PersistedCard<'_>> {
        let live = self.doc().variants.get(live_position)?;
        let rec = match &self.session().hist.saved_strip {
            Some(rec) => rec,
            None => {
                let trivial = self.doc().variants.len() == 1
                    && live_position == 0
                    && crate::model::strip_is_trivial(
                        live.kind,
                        &live.id,
                        live.name.as_deref(),
                        0,
                    );
                return trivial.then_some(PersistedCard {
                    recipe: &self.session().hist.saved_recipe,
                    origin: self.session().hist.pixels_on_disk.as_deref(),
                });
            }
        };

        if let Some(kind) = VariantKind::from_store_str(&rec.active_kind)
            && AutoShadeApp::persisted_identity_matches(
                live,
                live_position,
                kind,
                rec.active_pos,
                rec.active_id.as_deref(),
            )
        {
            return Some(PersistedCard {
                recipe: &self.session().hist.saved_recipe,
                origin: self.session().hist.pixels_on_disk.as_deref(),
            });
        }

        rec.others.iter().enumerate().find_map(|(i, entry)| {
            let position = if i < rec.active_pos { i } else { i + 1 };
            let kind = VariantKind::from_store_str(&entry.kind)?;
            AutoShadeApp::persisted_identity_matches(
                live,
                live_position,
                kind,
                position,
                entry.id.as_deref(),
            )
            .then_some(PersistedCard {
                recipe: &entry.recipe,
                origin: entry.origin.as_deref(),
            })
        })
}

    /// How many persisted-card mismatches hold work that quitting would discard.
    ///
    /// Dirty means: a live card cannot be matched to the union of the saved
    /// active card (`recipe.json` / `pixels.json`) and `variants.json.others`,
    /// or a matched card differs in kind/name/develop, or a persisted card was
    /// deleted. IDs match first; id-less records fall back to kind + position.
    /// Which card is selected is navigation, not an edit. The OPEN photo's
    /// strip only — the per-photo half of
    /// [`Self::inactive_dirty_variants`]; the stash decision in `open_path`
    /// keys on THIS (summing other photos' stashed variants into the gate
    /// chain-stashed every clean photo the user merely visited).
    ///
    /// The pre-v0.22 rule compared each background variant against the
    /// ACTIVE canvas's `saved_recipe`/`pixels_on_disk`. Those describe a
    /// DIFFERENT variant, so any strip whose cards disagree in origin (every
    /// generate→fit flow) counted dirty forever, no save could clear it, and
    /// the quit dialog re-armed until 「Discard」.
    pub(crate) fn open_dirty_variants(&self) -> usize {
        let saved = self.session().hist.saved_strip.as_ref().map(|rec| {
            use crate::logic::strip::SavedCard;
            let mut cards: Vec<SavedCard<'_>> = Vec::with_capacity(rec.others.len() + 1);
            if let Some(kind) = VariantKind::from_store_str(&rec.active_kind) {
                cards.push(SavedCard {
                    kind,
                    position: rec.active_pos,
                    id: rec.active_id.as_deref(),
                    name: rec.active_name.as_deref(),
                    recipe: &self.session().hist.saved_recipe,
                    origin: self.session().hist.pixels_on_disk.as_deref(),
                });
            }
            cards.extend(rec.others.iter().enumerate().filter_map(|(i, entry)| {
                let kind = VariantKind::from_store_str(&entry.kind)?;
                Some(SavedCard {
                    kind,
                    position: if i < rec.active_pos { i } else { i + 1 },
                    id: entry.id.as_deref(),
                    name: entry.name.as_deref(),
                    recipe: &entry.recipe,
                    origin: entry.origin.as_deref(),
                })
            }));
            cards
        });
        crate::logic::strip::open_dirty_count(&self.doc().variants, self.doc().active, saved.as_deref())
}

    /// The active variant, if a photo is open.
    pub(crate) fn active_variant(&self) -> Option<&Variant> {
        self.doc().variants.get(self.doc().active)
}

    /// Toggle the clipping layer WITHOUT a full redevelop: the overlay is a
    /// pure function of the last developed pixels (retained from the last
    /// accepted frame), so the J key / ▲ button / histogram triangles respond
    /// instantly instead of paying a whole develop (100-300 ms at 2560/4096).
    /// Falls back to a redevelop only when no frame is retained yet.
    /// The pixel reference a Range Mask is judged against: this recipe cut
    /// down to the masks BEFORE mask `i`, with every geometry field stripped.
    ///
    /// The PREFIX, not masks-cleared: the engine evaluates a range on the
    /// pixel as it stands when THIS mask runs — masks stack sequentially, so
    /// a later mask's range sees earlier masks' output (`render.rs`
    /// `apply_masks`); a cleared reference judged mask 2's range on pixels
    /// mask 0/1 had already moved (CX5-6). Geometry runs after develop, so
    /// it is no part of the reference, and `develop_preview_framed` has no
    /// geometry stage — left in, the fields only broke the cache key.
    ///
    /// ONE builder for the coverage overlay (`refresh_mask_overlay`) and the
    /// range eyedropper (`handle_range_pick`), which SHARE the one-slot
    /// `overlay_ref` cache compared by recipe equality. The two used to be
    /// hand-copied, and the copy that forgot the manual CA pair missed the
    /// cache on every click under a non-zero Red/cyan or Blue/yellow, paying
    /// a full preview develop on the UI thread for a 5×5 read.
    pub(crate) fn range_reference_recipe(&self, i: usize) -> EditRecipe {
        crate::logic::range::reference_recipe(&self.doc().recipe, i)
}

    /// The geometric mapping context every interaction boundary needs:
    /// original preview pixel dims + the current straighten angle + the lens
    /// geometry (in-camera profile + manual distortion amount).
    pub(crate) fn geom_ctx(&self) -> ((f32, f32), f32, LensArg) {
        let dims = self.photo().base_preview
            .as_ref()
            .map(|b| {
                let (w, h) = b.dimensions();
                (w as f32, h as f32)
            })
            .unwrap_or((1.0, 1.0));
        (
            dims,
            self.doc().recipe.straighten_deg,
            LensArg {
                // COMPOSED (R25 B3): every interaction map downstream of this
                // bundle must see the same profile the pixels went through,
                // manual CA included.
                profile: autoshade::render::geometry_profile(&self.doc().recipe).into_owned(),
                amount: self.doc().recipe.lens_distortion,
            },
        )
}

    /// Can this photo's sidecar be handed to Lightroom at all? RAW only, and
    /// keyed to the ORIGINAL photo (`src_path`), never the active variant: the
    /// projection belongs to the RAW no matter which rendition is on screen.
    ///
    /// Only a camera RAW has the sidecar convention — `store::
    /// lightroom_sidecar` and `pipeline::write_xmp` draw the same line, and a
    /// baked PNG/TIFF's neighbouring `.xmp` is another program's file, which
    /// this must never offer to overwrite.
    ///
    /// Deliberately NOT gated on the projection existing: that would be a disk
    /// stat per frame, and "save the develop first" is a better answer from the
    /// click (the store's own NotFound message says exactly that) than a button
    /// that is greyed out for a reason nothing explains.
    pub(crate) fn can_export_xmp_beside(&self) -> bool {
        self.photo().src_path.as_deref().is_some_and(autoshade::decode::is_raw)
}

    /// The strip index of the card with `id` (an empty id matches nothing).
    pub(crate) fn card_index(&self, id: &str) -> Option<usize> {
        (!id.is_empty()).then(|| self.doc().variants.iter().position(|v| v.id == id)).flatten()
}

    /// Whether the last analysis left anything to read.
    pub(crate) fn has_analysis_output(&self) -> bool {
        self.analysis().verdict.is_some() || !self.analysis().rationale.is_empty()
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
        self.doc().recipe.colour_field.as_ref().is_some_and(|f| f.renderable())
            || self.doc().recipe.masks.iter().any(mask_active)
}
}

impl Editor {
    /// The ONE owner for the persisted `(recipe, origin)` of the active card.
    /// A card created after the last save has no baseline and is therefore
    /// unsaved. With no strip record, only the trivial lone Original maps to
    /// `recipe.json` / `pixels.json`.
    pub(crate) fn active_baseline(&self) -> Option<ActiveBaseline<'_>> {
        let saved = self.persisted_card_for(self.doc().active)?;
        Some(ActiveBaseline { recipe: saved.recipe, origin: saved.origin })
    }

    /// The live strip as a persistable [`store::VariantsRecord`]: `None` when
    /// the strip is trivial ([`crate::model::strip_is_trivial`] — recipe.json
    /// plus pixels.json already say everything, and a record would be pure
    /// sidecar noise). Since R24-3 a lone Original card that carries a NAME or
    /// a minted identity is no longer trivial: the record is the only home
    /// either of those has.
    pub(crate) fn current_strip_record(&self) -> Option<autoshade::store::VariantsRecord> {
        let ak = self.active_variant().map_or(VariantKind::Original, |v| v.kind);
        if crate::model::strip_is_trivial(
            ak,
            self.active_variant().map_or("", |v| v.id.as_str()),
            self.active_variant().and_then(|v| v.name.as_deref()),
            self.doc().variants.len().saturating_sub(1),
        ) {
            return None;
        }
        Some(autoshade::store::VariantsRecord {
            extra: Default::default(),
            v: 1,
            active_kind: ak.store_str().to_string(),
            active_pos: self.doc().active,
            others: self.doc().variants
                .iter()
                .enumerate()
                .filter(|(i, _)| *i != self.doc().active)
                .map(|(_, v)| autoshade::store::VariantEntry {
                    extra: Default::default(),
                    kind: v.kind.store_str().to_string(),
                    recipe: v.recipe.clone(),
                    origin: v.origin.clone(),
                    // Hop 1 of 6 (R24-2): the live strip's identities + names
                    // to disk. An EMPTY id is written as absent — the record
                    // must not claim an identity the card does not have.
                    id: crate::model::variant_id_field(&v.id),
                    name: v.name.clone(),
                })
                .collect(),
            active_id: self.active_variant().and_then(|v| crate::model::variant_id_field(&v.id)),
            active_name: self.active_variant().and_then(|v| v.name.clone()),
        })
    }

    /// The on-disk PIXEL SOURCE the active variant renders / retouches /
    /// exports FROM. Any variant whose pixels are baked into a ./out raster — a
    /// reimagine or fill (Generated), OR an in-place heal/clone/denoise on ANY variant —
    /// carries that full-resolution artifact in `origin` and renders from it;
    /// a pristine source-based variant (原片 / 反推 with no pixel retouch) has
    /// `origin = None` and renders from `src_path` (the RAW / loaded image)
    /// developed by the recipe. Retouch and export key off THIS — not raw
    /// `src_path` — so what exports matches what's on screen (WYSIWYG), never
    /// the untouched negative underneath a generated / retouched rendition.
    pub(crate) fn active_source_path(&self) -> Option<PathBuf> {
        match self.active_variant() {
            Some(v) => v.origin.clone().or_else(|| self.photo().src_path.clone()),
            None => self.photo().src_path.clone(),
        }
    }

    /// Does the active card develop an AI-generated raster — the pristine
    /// 「✨ AI generated」 card or a 「✎ Edited AI image」 over the same
    /// pixels ([`VariantKind::on_ai_pixels`])? Such a canvas carries no
    /// calibration (the raster already has the camera look, the lens
    /// geometry and a baked white balance), projects to no Lightroom XMP (no
    /// sidecar reproduces AI pixels), and is what the `pixels.json` flag
    /// records. The one place the two kinds part — the ✨ card is immutable,
    /// the ✎ card is where its edits live — asks `== Generated` directly.
    pub(crate) fn active_on_ai_pixels(&self) -> bool {
        self.active_variant().is_some_and(|v| v.kind.on_ai_pixels())
    }

    /// The one door every click-on-the-photo tool shares: a crosshair while
    /// armed, and on a click the point in the ORIGINAL frame (the frame
    /// the develop and every raster live in), mapped out of the transformed
    /// view. `None` until the click. A tool that samples the canvas base reads
    /// it at [`Self::orig_to_base_norm`] of this point.
    pub(crate) fn clicked_orig_norm(&self, ui: &egui::Ui, resp: &egui::Response, xf: ViewXform) -> Option<(f32, f32)> {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
        if !resp.clicked() {
            return None;
        }
        let (nx, ny) = xf.to_norm(resp.interact_pointer_pos()?);
        let ((bw, bh), deg, dist) = self.geom_ctx();
        Some(view_norm_to_orig(nx, ny, (bw, bh), deg, &dist))
    }

    /// The focus point as a small ring while the pick is armed, so the
    /// photographer sees where the current focus sits before moving it.
    pub(crate) fn draw_lens_focus(&self, ui: &egui::Ui, xf: ViewXform) {
        let Some(lb) = self.doc().recipe.lens_blur.as_ref() else { return };
        let ((bw, bh), deg, dist) = self.geom_ctx();
        let (vx, vy) = orig_norm_to_view(lb.focus[0], lb.focus[1], (bw, bh), deg, &dist);
        let c = xf.to_screen(vx, vy);
        let p = ui.painter();
        p.circle_stroke(c, 9.0, egui::Stroke::new(3.0, egui::Color32::from_black_alpha(150)));
        p.circle_stroke(c, 9.0, egui::Stroke::new(1.5, egui::Color32::from_white_alpha(235)));
        p.circle_filled(c, 2.0, egui::Color32::WHITE);
    }
}

impl Editor {
    /// The active card's actual pixels choose the path, including generated
    /// masters under a RAW original. An unopened app displays the RAW dials.
    pub(crate) fn denoise_uses_raw(&self) -> bool {
        crate::logic::noise::uses_raw(self.active_source_path().as_deref())
    }

    /// [`export_file_name`](Self::export_file_name) with the container given
    /// (Normal's Save group always delivers a jpg).
    pub(crate) fn export_file_name_as(&self, ext: &str) -> String {
        let src = self.active_source_path();
        let stem = src
            .as_deref()
            .and_then(|p| p.file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("out")
            .to_string();
        format!("{stem}.developed.{ext}")
    }

    pub(crate) fn active_canvas_dirty(&self) -> bool {
        if self.photo().src_path.is_none() || self.active_variant().is_none() {
            return false;
        }
        self.active_baseline().is_none_or(|saved| {
            dirty_vs(&self.doc().recipe, saved.recipe)
                || !same_master_opt(
                    self.active_variant().and_then(|v| v.origin.as_deref()),
                    saved.origin,
                )
        })
    }

    /// Per-frame status-bar consumer of [`Self::active_baseline`].
    pub(crate) fn unsaved_marker_dirty(&self) -> bool {
        self.active_canvas_dirty()
    }

    /// Window-close consumer of [`Self::active_baseline`].
    pub(crate) fn quit_guard_open_dirty(&self) -> bool {
        self.active_canvas_dirty()
    }

    /// Navigation-stash consumer: active canvas plus this photo's strip work.
    pub(crate) fn nav_stash_gate_dirty(&self) -> bool {
        self.active_canvas_dirty() || self.open_dirty_variants() > 0
    }

    /// Quit-dialog `PendingSave` consumer: the same open-photo state as the
    /// navigation stash, without counting other photos' stash entries.
    pub(crate) fn pending_save_gate_dirty(&self) -> bool {
        self.active_canvas_dirty() || self.open_dirty_variants() > 0
    }
}

impl Editor {
    /// Persist the open photo's strip beside recipe.json/pixels.json and
    /// advance the mirror. Trivial strip ⇒ clear (no noise file). An error
    /// leaves the mirror untouched, so the unsaved protection stays armed.
    /// Test-only since the single-generation commit (L03): production saves
    /// stage the strip INTO [`autoshade::store::commit_develop`] and advance
    /// the mirror on the commit's success — this remains the strip-half
    /// primitive the guard tests drive in isolation.
    #[cfg(test)]
    pub(crate) fn persist_strip(&mut self, path: &std::path::Path) -> std::io::Result<()> {
        match self.current_strip_record() {
            Some(rec) => {
                autoshade::store::write_variants(path, &rec)?;
                self.session_mut().hist.saved_strip = Some(rec);
            }
            None => {
                autoshade::store::clear_variants(path)?;
                self.session_mut().hist.saved_strip = None;
            }
        }
        Ok(())
    }

    /// The After image with its interaction layers — crop tool, mask placement,
    /// paint canvas, box-select — or the SOURCE flashed in the same rect while
    /// `comparing` (B held). Scroll zooms to the cursor; middle-drag or
    /// Space+drag pans; all image-space handlers map through [`ViewXform`].
    /// The canvas image with its two fades: the B-key compare cross-fades
    /// between the source and the edit, and a swap's first frame fades in
    /// over the frame it replaced. `id` is the texture `comparing` selects;
    /// the other one is painted over it while the compare fade runs.
    pub(crate) fn paint_canvas_image(&mut self, ui: &egui::Ui, rect: egui::Rect, uv: egui::Rect, id: egui::TextureId, comparing: bool) {
        let painter = ui.painter_at(rect);
        let ctx = ui.ctx();
        // 1 = showing the source. Driven by `comparing`, so a held B fades
        // in and its release fades back.
        let to_before = ctx.animate_bool_with_time(egui::Id::new("canvas_compare"), comparing, MOTION_FADE_S);
        let other_alpha = if comparing { 1.0 - to_before } else { to_before };
        let swap = self.plates().canvas_fade.as_ref().map(|(old, t)| (old.clone(), ease_out(t.elapsed().as_secs_f32() / MOTION_FADE_S)));
        match &swap {
            // A swap still landing: the outgoing frame under, the new one
            // fading in on top (from the bare canvas when there was none).
            Some((old, k)) if *k < 1.0 => {
                if let Some(old) = old {
                    painter.image(old.id(), rect, uv, egui::Color32::WHITE);
                }
                painter.image(id, rect, uv, egui::Color32::WHITE.gamma_multiply(*k));
                ctx.request_repaint();
            }
            _ => {
                self.plates_mut().canvas_fade = None;
                painter.image(id, rect, uv, egui::Color32::WHITE);
            }
        }
        let other = if comparing { &self.plates().after_tex } else { &self.plates().before_tex };
        if other_alpha > 0.0
            && let Some(t) = other
        {
            painter.image(t.id(), rect, uv, egui::Color32::WHITE.gamma_multiply(other_alpha));
        }
    }
}
