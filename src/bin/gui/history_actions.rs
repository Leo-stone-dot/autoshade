//! The edit history and the pixel re-develops it drives: the undo/redo stacks
//! and their commits, the step application, pixel refresh/rebind and the
//! quarter-turn rotate/sync. Same `AutoShadeApp` impl as `actions.rs`, split by
//! domain.

use crate::*;

impl AutoShadeApp {
    /// The live working state as an [`UndoStep`]: current recipe + the active
    /// variant's pixel identity. Arc/path clones only — never pixel copies.
    pub(crate) fn current_step(&self) -> UndoStep {
        UndoStep {
            recipe: self.editor.doc().recipe.clone(),
            base: self.editor.active_variant().and_then(|v| v.base.clone()),
            origin: self.editor.active_variant().and_then(|v| v.origin.clone()),
            keep_skin: self.skin_switch(),
        }
    }

    /// Reset undo history — call when a brand-new photo opens, when the active
    /// variant changes (you can't undo across either), or when a photo GOES
    /// AWAY: a failed fresh open leaves no canvas for its steps to apply to,
    /// and an undo there restored one that no longer existed. `committed`
    /// becomes the current head — with no photo open that is the empty step,
    /// which is exactly what stops the next settled frame pushing a phantom.
    pub(crate) fn reset_history(&mut self) {
        self.editor.session_mut().hist.committed = self.current_step();
        self.editor.session_mut().hist.undo_stack.clear();
        self.editor.session_mut().hist.redo_stack.clear();
    }

    /// Commit the current state as one undo step RIGHT NOW (when it differs
    /// from the head). Used directly by programmatic pixel swaps (the retouch
    /// bake-in): waiting for `commit_if_settled` left a one-frame window where
    /// a same-frame Ctrl+Z saw only the older history and undid a recipe step
    /// while keeping the freshly retouched pixels installed.
    pub(crate) fn commit_now(&mut self) {
        let cur = self.current_step();
        if cur.recipe != self.editor.session().hist.committed.recipe || !cur.same_pixels(&self.editor.session().hist.committed) {
            let hist = self.editor.history_mut();
            hist.undo_stack.push(std::mem::replace(&mut hist.committed, cur));
            if self.editor.session().hist.undo_stack.len() > 100 {
                self.editor.session_mut().hist.undo_stack.remove(0); // cap history memory
            }
            self.editor.session_mut().hist.redo_stack.clear();
        } else {
            // A flip of the skin switch that changed no value belongs to the
            // step on screen, so a later undo/redo hands it back as set.
            self.editor.session_mut().hist.committed.keep_skin = cur.keep_skin;
        }
    }

    /// Commit the current state as ONE undo step once the edit gesture settles
    /// (pointer released) — dragging a slider is one step, not one per frame.
    /// Programmatic edits (Analyze, Reset) land here on the next frame.
    pub(crate) fn commit_if_settled(&mut self, ctx: &egui::Context) {
        if !ctx.input(|i| i.pointer.any_down()) {
            self.commit_now();
        }
    }

    pub(crate) fn resync_recipe_display(&mut self) {
        self.editor.analysis_mut().rationale = self.editor.doc().recipe.rationale.clone();
        // Typed notes described the PREVIOUS rationale — a swap without a
        // clear would render a stale zh text over a different develop. The
        // landings that produce fresh notes install them AFTER this call.
        self.editor.analysis_mut().rationale_notes.clear();
        self.editor.analysis_mut().verdict = None;
        // A wholesale recipe swap (undo/redo/Reset/version load/AI apply)
        // disarms EVERY canvas tool — an armed index-carrying tool would fire
        // into whatever now sits at its remembered index, and a live
        // crop/rotate drag would re-apply its stale start angle over the
        // freshly restored recipe on the very next drag frame.
        self.disarm_tools();
        self.workspace.masks_mut().mask_name_buf = None; // stale (index, text) must not cross-commit
        // A curve-point drag in flight when the recipe is swapped (Ctrl+Z
        // mid-drag) kept mutating point i of the RESTORED curve on the next
        // drag frame (M17).
        self.workspace.set_curve_drag(None);
        // A wholesale recipe swap can shrink the mask list — an out-of-range
        // selection must not linger (every consumer bounds-checks, but the
        // panel would silently deselect anyway; do it deterministically).
        // Same-index-different-mask after undo is a known cosmetic residue —
        // mask identity isn't tracked across history steps.
        self.workspace.masks_mut().sel_mask = self.workspace.masks().sel_mask.filter(|&i| i < self.editor.doc().recipe.masks.len());
        // The component selection rides on the mask's OWN list, which the
        // swap can also reshape — same deterministic bound.
        self.workspace.masks_mut().sel_component = self.workspace.masks().sel_component.filter(|&c| {
            self.workspace.masks().sel_mask
                .and_then(|i| self.editor.doc().recipe.masks.get(i))
                .is_some_and(|m| c < m.components.len())
        });
    }

    /// Apply a history step: recipe always; the active variant's pixels only
    /// when the step's identity differs (so plain slider undos stay pixel-free
    /// and O(1)). A restored `base: None` reverts a retouched source variant
    /// to the shared source neutral.
    pub(crate) fn apply_step(&mut self, step: UndoStep, ctx: &egui::Context) {
        let pixels_differ = !step.same_pixels(&self.editor.session().hist.committed);
        let keep_skin = step.keep_skin;
        self.editor.session_mut().hist.committed = step.clone();
        self.editor.doc_mut().recipe = step.recipe;
        // The history is a READER too: a step recorded while the canvas was
        // washed reinstalls the pre-era pair on undo/redo — and the actor
        // that later promoted the canvas (Reset, Analyze, load_version,
        // Ctrl+S) cannot know which stack entries still hold it (the
        // save-time restamp covers only the pair IT replaced). Repairing on
        // the way back keeps every door to the canvas behind one rule.
        // Cost honesty: when the OPEN succeeded, the worker primed the
        // memo and this is a lookup. But washed steps enter history
        // precisely when the open-time estimate was an INABILITY — uncached
        // by design — so the first traversal here IS the retry point and
        // pays one estimate (the accepted class shared with load_active and
        // the open gate); its answer memoizes and the rest of the washed
        // history heals by lookup. Era-2 installs short-circuit before any
        // I/O; the Generated guard matches every other ordering site; and a
        // CROSS-PHOTO in-flight open is refused — src_path already points
        // at the INCOMING photo while this history belongs to the outgoing
        // one (the live-canvas override records the same hazard), so
        // repairing would estimate the WRONG photo onto this canvas. A
        // same-path preview-resolution re-decode (request AND fact: the
        // keep_recipe request verified against the recorded open_same_path)
        // keeps the pair coherent AND its keep arm preserves history —
        // refusing there made a washed install DURABLE, so it is admitted.
        // A same-path FRESH reopen is NOT: its rebuild discards this repair,
        // so admitting it bought a concurrent decode and a false success
        // toast. `committed` follows the heal, or the next commit_now would
        // push the washed head straight back onto the stack.
        //
        // DEFENCE IN DEPTH since the keyboard undo took the buttons' busy
        // gate: `open_in_flight` implies `busy` (open_path early-returns while
        // busy, sets both, and clears both in one handler), and undo/redo are
        // this fn's only callers — so no live caller reaches here mid-flight
        // any more. The gate stays because ungating the keyboard would
        // resurrect every scenario above, and the test drives it directly.
        if (!self.workspace.open().open_in_flight || (self.workspace.open().open_same_path && self.workspace.cards().keep_recipe))
            && self.editor.doc().variants.get(self.editor.doc().active).is_none_or(|v| v.kind.is_source_based())
            && let Some(p) = self.editor.photo().src_path.clone()
            && autoshade::pipeline::repair_pre_era_base_curve(&p, &mut self.editor.doc_mut().recipe).is_some()
        {
            self.editor.session_mut().hist.committed.recipe = self.editor.doc().recipe.clone();
            self.editor.sync_active_recipe();
            let lang = self.lang;
            self.toast(
                ToastKind::Success,
                tr(
                    lang,
                    K_EXPORT_CAMERA_BASE_LOOK_RE_ESTIMATED_THIS_PHOTO_WAS_SAV,
                )
                .to_string(),
            );
        }
        if pixels_differ {
            if let Some(v) = self.editor.active_variant_mut() {
                v.base = step.base;
                v.origin = step.origin;
            }
            self.refresh_active_pixels(ctx);
            // The quietest door: undoing to a SUPERSEDED master installs a
            // raster the preference cannot re-decode (the resolution switch
            // repoints only the Arc the canvas held), and undo/redo wrote no
            // status at all — so the combo was left to imply the canvas had
            // followed it. BOTH directions: the redo back to a reachable
            // canvas has to retract the claim, or it stands there false.
            self.set_canvas_status("restored the canvas pixels");
        }
        self.editor.session_mut().dirty = true;
        self.resync_recipe_display();
        // The skin switch comes back as the step had it; a step past the
        // skin limits is then put back as it was (Portraits).
        self.restore_skin_switch(keep_skin);
        self.release_skin_for_history();
    }

    /// Rebuild the pixel-derived per-variant state after the active variant's
    /// base changed under the same variant (retouch undo/redo): before pane,
    /// paint canvas, overlay reference and the retained frame all described
    /// the old raster. History is deliberately NOT touched here.
    pub(crate) fn refresh_active_pixels(&mut self, ctx: &egui::Context) {
        let Some(v) = self.editor.doc().variants.get(self.editor.doc().active) else { return };
        let base = v.base.clone().or_else(|| self.editor.photo().source_preview.clone());
        if let Some(base) = base {
            let (mw, mh) = base.dimensions();
            // Same rule as load_active: the canvas recipe's own curve serves
            // every variant (a Generated recipe's curve is empty by
            // construction; an InPlace master is neutral and needs it).
            let start = self.editor.doc().recipe.start_only();
            self.set_before(ctx, &base, &start);
            self.editor.photo_mut().base_preview = Some(base);
            self.rebind_paint_canvas(mw, mh);
        }
        self.workspace.overlay_mut().overlay_ref = None;
        self.workspace.overlay_mut().overlay_stale = true;
        self.workspace.chrome_mut().last_rgb = None;
    }

    /// The base plate under the canvas was REPLACED (a preview-resolution
    /// re-decode, a cold master landing, a retouch bake, a fresh open): the
    /// paint canvas is resized to it — and any live mask-brush session dies
    /// with the old plate, DISCLOSED. The session's weight buffer is
    /// dimension-locked to the plate it was armed on (`start_mask_brush`
    /// sizes both together), so a stroke stamped after the swap lands at
    /// the wrong coordinates in it — and the always-visible 「✓ Apply」 row
    /// would bake those misplaced weights into a durably-adopted raster. A
    /// variant switch already gets the teardown via `load_active`'s
    /// `disarm_tools`; the async landings never called it (L06#5). The
    /// toast lives HERE so no future plate-replacement door can forget it;
    /// a second rebind in the same landing finds no session and stays
    /// silent.
    pub(crate) fn rebind_paint_canvas(&mut self, w: u32, h: u32) {
        if self.end_mask_brush() {
            let t = tr(
                self.lang,
                K_ACTIONS_THE_MASK_BRUSH_SESSION_ENDED_THE_CANVAS_PIXELS_U,
            );
            self.toast(ToastKind::Error, t.to_string());
        }
        self.workspace.masks_mut().mask_paint = Some(image::RgbaImage::new(w, h));
        self.paint_mask_changed(Some(false));
        // The other tools' areas were in the old frame too.
        self.workspace.masks_mut().paint_store = Default::default();
        self.workspace.overlay_mut().mask_tex = None;
        self.workspace.overlay_mut().mask_dirty = false;
        self.workspace.masks_mut().paint_last = None;
    }

    /// May the photographer turn this photo right now? (Drives both the
    /// toolbar buttons' enabled state and the plate reconciler's own gate, so
    /// the two can never disagree about which photos rotate.)
    ///
    /// The one exclusion is BAKED PIXELS. A retouched/generated master is a
    /// raster on disk in the frame it was baked in, and this build has no way
    /// to record that a file was written before a turn — so turning the canvas
    /// under it would put the master back sideways on the next open, which is
    /// the silent-corruption shape the whole orientation chain exists to
    /// prevent. Rotate FIRST, bake after: the master is then written in the
    /// turned frame and everything agrees. The whole strip counts, not just
    /// the active card — an inactive variant's master is on disk exactly the
    /// same way. (Registered limitation, R27 A6.)
    ///
    /// An ARMED tool is excluded for a smaller reason: brush and retouch
    /// gestures bind screen coordinates to plate pixels mid-drag, and the
    /// plate is about to move under them.
    pub(crate) fn can_rotate(&self) -> bool {
        self.editor.photo().src_path.is_some()
            && self.editor.photo().base_preview.is_some()
            && !self.workspace.busy()
            && !self.workspace.open().open_in_flight
            && !self.workspace.masks().paint_mode
            && self.workspace.overlay().region.is_none()
            && self.editor.doc().variants.iter().all(|v| v.base.is_none() && v.origin.is_none())
            // A fill layer is pixels in the frame it was made in, like a
            // baked master (2026-10-01): the live card and every stored one.
            && self.editor.doc().recipe.pixel_layers.is_empty()
            && self.editor.doc().variants.iter().all(|v| v.recipe.pixel_layers.is_empty())
            // …and so is a lens blur's depth map (2026-10-13): estimated in
            // the frame the photo had, its texels would not follow a turn.
            && self.editor.doc().recipe.lens_blur.is_none()
            && self.editor.doc().variants.iter().all(|v| v.recipe.lens_blur.is_none())
    }

    /// Turn the open photo by `delta` clockwise quarter turns — ONE undo step,
    /// the whole develop moving together ([`autoshade::pipeline::rotate_recipe`]
    /// owns geometry + raster masks + the turn count).
    ///
    /// The plates are NOT turned here: `sync_base_turns` sees the changed
    /// `recipe.quarter_turns` on the next frame and moves them, which is the
    /// same path an UNDO of this action takes. One mover, so the two
    /// directions cannot drift.
    pub(crate) fn rotate_photo(&mut self, delta: u8, ctx: &egui::Context) {
        if !self.can_rotate() {
            return;
        }
        let Some(src) = self.editor.photo().src_path.clone() else { return };
        let lang = self.lang;
        // The step BEFORE the turn, so one Ctrl+Z puts it back (7.7).
        self.commit_now();
        let mut next = self.editor.doc().recipe.clone();
        match autoshade::pipeline::rotate_recipe(&mut next, &src, delta) {
            Ok(_) => {
                self.editor.doc_mut().recipe = next;
                self.editor.session_mut().dirty = true;
                self.sync_base_turns(ctx);
                self.resync_recipe_display();
                // …and SEAL it, rather than waiting for `commit_if_settled`
                // on the next frame: a click is already a settled gesture, and
                // the one-frame window let a same-frame Ctrl+Z undo the step
                // BEFORE this one (the hazard `commit_now`'s own doc records).
                self.commit_now();
            }
            Err(e) => {
                // All-or-nothing (see `rotate_recipe`): the recipe is the
                // untouched clone, so nothing moved and nothing needs undoing.
                // The sentence below covers the DISK too since R28 Batch-3 3b —
                // a failed turn used to leave one orphan PNG per attempt in the
                // develop dir while this toast said nothing was changed.
                let t = trf(
                    lang,
                    K_ACTIONS_COULD_NOT_TURN_THIS_PHOTO_NOTHING_WAS_CHANGED,
                    &[("err", &e.to_string())],
                );
                self.toast(ToastKind::Error, t);
            }
        }
    }

    /// Bring the base plates into the frame `recipe.quarter_turns` asks for.
    ///
    /// Called once per frame (`app.rs` update) rather than from each of the
    /// eight places that can change the turn — undo, redo, a variant switch, a
    /// version load, a paste, the rotate buttons. `image`'s rotate is
    /// LOSSLESS and the plate is preview-sized, and the common case is a `u8`
    /// comparison that returns immediately.
    ///
    /// Turning the PIXELS rather than teaching the canvas a rotation is
    /// deliberate, and it is the same decision `render_to_image_in` makes:
    /// every screen↔frame mapping downstream (`view_norm_to_orig`, the crop
    /// handles, the paint canvas, the coverage overlay, the retouch region)
    /// is defined against the plate, so one turn here moves all of them and
    /// none of them needs to know.
    pub(crate) fn sync_base_turns(&mut self, ctx: &egui::Context) {
        let want = self.editor.doc().recipe.quarter_turns % 4;
        let delta = (want + 4 - self.editor.photo().base_turns % 4) % 4;
        if delta == 0 {
            return;
        }
        // A baked master is on disk in its own frame; `can_rotate` refuses to
        // create a delta over one, and this is the same gate seen from the
        // other side. Every master is written in the EXIF frame (the pixel
        // doors refuse a turned photo — `refuse_pixel_work_on_a_turned_photo`),
        // so what this branch protects is the per-variant `v.base` plate,
        // whose frame nothing tracks: turning the previews under it would
        // put the canvas and the cards in two frames.
        if self.editor.doc().variants.iter().any(|v| v.base.is_some() || v.origin.is_some()) {
            self.editor.photo_mut().base_turns = want;
            return;
        }
        let turn = |p: &Arc<image::DynamicImage>| {
            Arc::new(autoshade::render::turn_image((**p).clone(), delta))
        };
        self.editor.photo_mut().source_preview = self.editor.photo().source_preview.as_ref().map(&turn);
        self.editor.photo_mut().base_preview = self.editor.photo().base_preview.as_ref().map(&turn);
        self.editor.photo_mut().base_turns = want;
        // Everything derived from the plate's frame. The Before texture is
        // rebuilt here rather than invalidated: `set_before` is the only
        // writer, and leaving a stale one on screen for a frame is the
        // sideways-preview bug in miniature. The paint canvas is REBOUND
        // (`refresh_active_pixels`'s rule for a replaced plate): its buffer
        // is dimension-locked to the plate and an odd turn transposes the
        // plate, so a canvas left W×H under an H×W plate put every later
        // brush stroke — and the fill/heal mask exported from it — at the
        // wrong coordinates. A live brush session dies with the old frame,
        // disclosed, exactly as at every other replacement door.
        if let Some(b) = self.editor.photo().base_preview.clone() {
            let start = self.editor.doc().recipe.start_only();
            self.set_before(ctx, &b, &start);
            let (mw, mh) = b.dimensions();
            self.rebind_paint_canvas(mw, mh);
        }
        self.workspace.overlay_mut().overlay_ref = None;
        self.workspace.overlay_mut().overlay_stale = true;
        self.workspace.chrome_mut().last_rgb = None;
        self.workspace.overlay_mut().mask_tex = None;
        self.workspace.overlay_mut().mask_overlay_tex = None;
        self.workspace.overlay_mut().overlay_key = None;
        self.editor.session_mut().dirty = true;
    }

    pub(crate) fn undo(&mut self, ctx: &egui::Context) {
        if let Some(prev) = self.editor.session_mut().hist.undo_stack.pop() {
            let hist = self.editor.history_mut();
            hist.redo_stack.push(hist.committed.clone());
            self.apply_step(prev, ctx);
        }
    }

    pub(crate) fn redo(&mut self, ctx: &egui::Context) {
        if let Some(next) = self.editor.session_mut().hist.redo_stack.pop() {
            let hist = self.editor.history_mut();
            hist.undo_stack.push(hist.committed.clone());
            self.apply_step(next, ctx);
        }
    }
}
