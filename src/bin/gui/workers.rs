//! Background workers: spawn, master loads, the Msg pump.

use super::*;

impl AutoShadeApp {
    /// Spawn a worker whose PANIC still delivers a terminal message. Every
    /// worker's last act is sending the `Msg` that clears `busy` (or an
    /// inflight counter); a panic before that send unwinds the thread, the
    /// message never arrives, and the whole app soft-locks — every action
    /// gates on `!busy`, and only killing the process recovers. One decode
    /// panic inside `rawler`/`image` on a malformed file is enough. So: run
    /// the body under `catch_unwind` and synthesize the site's failure `Msg`
    /// from the panic payload. `AssertUnwindSafe` is sound here — all
    /// captured state is moved in and dropped on unwind; the UI only ever
    /// observes the channel message.
    pub(crate) fn spawn_worker(
        &self,
        body: impl FnOnce() -> Msg + Send + 'static,
        on_panic: impl FnOnce(anyhow::Error) -> Msg + Send + 'static,
    ) {
        let tx = self.tx.clone();
        let ctx = self.egui_ctx.clone();
        // Starting a worker schedules the next frame itself: the repaint
        // pump's gate (`poll_workers`, at the top of update) has already run
        // by the time a panel can start one in the same frame, so without
        // this the "fetching…"/busy state is not drawn and the 100 ms pump
        // never re-arms until the next input event.
        self.egui_ctx.request_repaint();
        std::thread::spawn(move || {
            // This body's panic is CONTAINED by the `catch_unwind` below.
            // Declared, so the panic hook does not announce a crash over an
            // app that is about to report the failure and carry on
            // (`autoshade::panic_guard`).
            let _contained = autoshade::panic_guard::Recoverable::enter();
            let msg = std::panic::catch_unwind(std::panic::AssertUnwindSafe(body))
                .unwrap_or_else(|p| {
                    let s = p
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown panic".into());
                    on_panic(anyhow::anyhow!("worker panicked: {s}"))
                });
            let _ = tx.send(msg);
            // An mpsc send does not wake egui: with the pointer held still, a
            // completed result used to sit unread in the channel until the
            // next mouse or key event happened to produce a frame.
            ctx.request_repaint();
        });
    }

    /// Decode a variant's retouched master off-thread — a 61 MP TIFF takes
    /// seconds, so the UI thread never does this inline — and install it at
    /// `preview_edge`, like every other master install (open: `load_active`'s
    /// worker; retouch: the retouch panel). This door used to keep the raster
    /// at SENSOR size: ~0.5 GiB resident, a ~244 MB paint mask from
    /// `refresh_active_pixels`, and a Before texture past the 8192 max
    /// texture dimension of common backends (L02).
    ///
    /// `preview_edge`, not `canvas_edge()`: at cold-switch time the canvas
    /// still holds the OUTGOING variant's raster, so `canvas_edge()` would
    /// key the install to a stale edge — and `preview_edge` is exactly what
    /// the open path files the same master under, so the two caches agree.
    ///
    /// One in-flight decode per (photo, origin): repeat entries (re-clicks
    /// into a still-decoding card, a stash restore) coalesce instead of
    /// stacking a fresh decode thread each time; an already-decoded master
    /// comes straight from `master_cache`. And `decode::load_image`, not
    /// `image::open`: the master rides the same decode budget and
    /// orientation handling as every other source.
    pub(crate) fn spawn_master_load(
        &mut self,
        ctx: &egui::Context,
        photo: PathBuf,
        origin: PathBuf,
    ) {
        let edge = self.preview_edge.clamp(640, 8192);
        // Cache probe first: a revisited card installs synchronously, with
        // no worker and no "loading…" status to flash.
        if let Some(arc) = self.cached_master(&origin, edge) {
            self.install_master(ctx, &photo, &origin, arc);
            return;
        }
        if !self.master_loads.insert((photo.clone(), origin.clone())) {
            return;
        }
        self.status = tr(
            self.lang,
            "loading this variant's retouched master… (showing the source develop meanwhile)",
        )
        .into();
        // Identity BEFORE the read (the curve-memo / remember_base rule,
        // L12-2): a master rewritten DURING the decode must MISS on the next
        // probe — stat'ing at landing filed the OLD pixels under the NEW
        // stamp, a false HIT. The spawn edge rides along too (L12-6): the
        // landing used to read the CURRENT preference, so a mid-flight px
        // change filed these pixels under a resolution they never had.
        let stamp = file_stamp(&origin);
        let (p2, o2) = (photo.clone(), origin.clone());
        self.spawn_worker(
            move || {
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&origin))); // full-frame commit budget (budget.rs)
                // The same >24 MP permit `request_thumb` takes: a master
                // decode is deliberately not `busy`-gated, so N cold cards
                // with distinct origins could otherwise decode N 60 MP
                // rasters at once.
                let _big_permit = image::ImageReader::open(&origin)
                    .and_then(|r| r.into_dimensions().map_err(std::io::Error::other))
                    .is_ok_and(|(w, h)| w as u64 * h as u64 > 24_000_000)
                    .then(|| big_decode_gate().lock().unwrap_or_else(|p| p.into_inner()));
                // baked-by-construction: `origin` is the develop store's baked master.
                let img = autoshade::decode::load_image(&origin).map(|im| {
                    // GUARDED shrink: plain `thumbnail` UPSCALES, which would
                    // inflate a small master instead of bounding a large one.
                    if im.width().max(im.height()) > edge {
                        autoshade::render::thumbnail(&im, edge, edge)
                    } else {
                        im
                    }
                });
                Msg::MasterLoaded { photo, origin, edge, stamp, img: Box::new(img) }
            },
            move |e| {
                Msg::MasterLoaded { photo: p2, origin: o2, edge, stamp, img: Box::new(Err(e)) }
            },
        );
    }

    /// Cold-master LRU lookup (most-recent kept last) — same shape and same
    /// staleness guard as `cached_base`.
    pub(crate) fn cached_master(
        &mut self,
        origin: &std::path::Path,
        edge: u32,
    ) -> Option<Arc<image::DynamicImage>> {
        let stamp = file_stamp(origin);
        let pos = self
            .master_cache
            .iter()
            .position(|((p, e, t), _)| p == origin && *e == edge && *t == stamp)?;
        let entry = self.master_cache.remove(pos);
        let hit = entry.1.clone();
        self.master_cache.push(entry);
        Some(hit)
    }

    /// Remember a freshly decoded cold master under its own identity —
    /// `remember_base`'s twin. The stamp is the SPAWN-TIME capture riding
    /// the message (L12-2): the old landing-time stat argued it made a
    /// mid-decode rewrite miss, but it did the opposite — the NEW stamp was
    /// filed with the OLD pixels, so the next probe HIT on pixels the file
    /// never held. Keyed pre-read, a rewrite moves the stamp on and misses.
    pub(crate) fn remember_master(
        &mut self,
        origin: &std::path::Path,
        edge: u32,
        stamp: FileStamp,
        img: Arc<image::DynamicImage>,
    ) {
        self.master_cache.retain(|((p, e, _), _)| !(p == origin && *e == edge));
        self.master_cache.push(((origin.to_path_buf(), edge, stamp), img));
        if self.master_cache.len() > MASTER_CACHE_CAP {
            self.master_cache.remove(0); // least-recent first
        }
    }

    /// Install decoded master pixels into every strip entry that still
    /// references this exact master and still awaits pixels — by IDENTITY,
    /// never by index, so a delete/reorder mid-decode cannot misfile them.
    pub(crate) fn install_master(
        &mut self,
        ctx: &egui::Context,
        photo: &PathBuf,
        origin: &PathBuf,
        arc: Arc<image::DynamicImage>,
    ) {
        if self.src_path.as_ref() != Some(photo) {
            return;
        }
        let mut hit_active = false;
        for (i, v) in self.variants.iter_mut().enumerate() {
            if v.base.is_none() && v.origin.as_ref() == Some(origin) {
                v.base = Some(arc.clone());
                hit_active |= i == self.active;
            }
        }
        if hit_active {
            // The canvas was showing the disclosed source stand-in — swap in
            // the real pixels now that they exist.
            self.refresh_active_pixels(ctx);
            self.set_canvas_status("restored the canvas pixels");
        }
    }

    /// Queue a thumbnail decode for `idx` if it isn't cached/queued and we're
    /// under the concurrency cap. Uses the camera's embedded preview (fast) — the
    /// double-processing concern only applies to the develop base, not a 56px chip.
    /// A persistent disk cache (keyed on path+mtime+size) makes every later
    /// session — and every scroll-back after texture eviction — a ~1 ms JPEG
    /// read instead of a full decode.
    pub(crate) fn request_thumb(&mut self, idx: usize) {
        if self.thumbs.contains_key(&idx) || self.thumb_requested.contains(&idx) {
            return;
        }
        // A BOUNDED retry, because both extremes are wrong. Dropping the
        // marker on failure re-requested every frame — six decode threads per
        // 100 ms for as long as the row was visible. Keeping it forever
        // blanked the row for the session, and the LRU that was supposed to
        // provide the retry only prunes above THUMB_TEX_CAP, so a folder
        // under that size never retried at all. Three attempts covers the
        // transient causes (AV lock, a slow share) and stops dead on a file
        // that simply cannot be decoded.
        if self.thumb_fail.get(&idx).is_some_and(|n| *n >= 3) {
            return;
        }
        if self.thumb_inflight >= MAX_THUMB_INFLIGHT {
            return;
        }
        let Some(path) = self.gallery.get(idx).cloned() else { return };
        self.thumb_requested.insert(idx);
        self.thumb_inflight += 1;
        let generation = self.gallery_gen;
        self.spawn_worker(
            move || {
                let res = (|| -> anyhow::Result<image::DynamicImage> {
                    let cache = thumb_cache_file(&path);
                    if let Some(p) = &cache
                        && let Ok(img) = image::open(p)
                    {
                        return Ok(img);
                    }
                    // Large baked rasters (the app's own 60 MP TIFF/PNG exports)
                    // decode whole before the 160px shrink — ~360 MB each, and
                    // MAX_THUMB_INFLIGHT of them at once spiked to ~2 GB. Gate
                    // decodes above 24 MP behind one permit; the header read is
                    // cheap and RAWs (embedded-JPEG previews) stay concurrent.
                    let _big_permit = if !autoshade::decode::is_raw(&path)
                        && image::ImageReader::open(&path)
                            .and_then(|r| r.into_dimensions().map_err(std::io::Error::other))
                            .is_ok_and(|(w, h)| w as u64 * h as u64 > 24_000_000)
                    {
                        Some(big_decode_gate().lock().unwrap_or_else(|p| p.into_inner()))
                    } else {
                        None
                    };
                    // In the frame the develop asks for, so a rotated photo
                    // reads the same way in the grid as on the canvas and in
                    // the export. The same number keys the cache file above —
                    // read once here rather than threaded from it, because the
                    // cache may legitimately be absent (unstattable source).
                    let thumb = autoshade::render::thumbnail(
                        &autoshade::decode::preview_only_turned(&path, autoshade::store::saved_quarter_turns(&path))?,
                        THUMB_EDGE,
                        THUMB_EDGE,
                    );
                    if let Some(p) = &cache {
                        save_thumb_cache(p, &thumb); // best-effort write-through
                    }
                    Ok(thumb)
                })();
                Msg::Thumb { generation, idx, img: Box::new(res) }
            },
            // The Err handler decrements thumb_inflight like any decode failure.
            move |e| Msg::Thumb { generation, idx, img: Box::new(Err(e)) },
        );
    }

    /// Queue the latest preview state on the single CPU worker. Dispatch is O(1):
    /// base pixels are Arc-shared and the recipe is small. While a frame is in
    /// flight, edits only set `dirty`; no parallel render storm is possible.
    pub(crate) fn start_redevelop(&mut self) {
        if self.develop_inflight {
            return;
        }
        // The canvas is only right over pixels that carry its recipe's base
        // part (v1.17.0): hold the frame until they do.
        if !self.ensure_source_part() {
            self.dirty = false;
            return;
        }
        let Some(base) = self.base_preview.clone() else {
            self.dirty = false;
            return;
        };
        // The frame of the pixels (v1.17.0): a master's own marker says
        // whether it is lens-corrected, and a recipe that develops other lens
        // geometry than the one baked into it is refused, as the export
        // refuses it — the canvas keeps its last frame and says why.
        let source = match self.source_frame().and_then(|f| f.check(&self.recipe).map(|()| f)) {
            Ok(f) => f,
            Err(e) => {
                self.dirty = false;
                self.status = format!("{}: {e:#}", tr(self.lang, "preview develop failed"));
                return;
            }
        };
        let recipe = self.recipe.clone();
        let show_clipping = self.show_clipping;
        let film = self.film_short_edge();
        self.develop_inflight = true;
        self.dirty = false;
        self.spawn_worker(
            move || Msg::Developed(Box::new(Ok(build_preview(base, recipe, show_clipping, film, source)))),
            |e| Msg::Developed(Box::new(Err(e))),
        );
    }

    /// Is the source base the right one for the canvas recipe — `true` — or is
    /// a re-develop needed first? (v1.17.0)
    ///
    /// A source-based card develops over `source_preview` with the preview
    /// develop, which runs no profile stage: the camera profile and the Look's
    /// colour table must already be in those pixels, developed under the
    /// recipe's own [`autoshade::render::BasePart`]. The open develops the
    /// part the photo's develop starts from, so this answers `true` on the
    /// common path; a recipe that names another (an imported sidecar's own
    /// profile, a paste, an undo across one, a version) re-develops the
    /// source in the background at the canvas's edge and answers `false`
    /// until it lands ([`Msg::BaseRedeveloped`]). A baked source, or a card
    /// on its own pixels, has nothing to re-develop.
    pub(crate) fn ensure_source_part(&mut self) -> bool {
        if !self.source_is_raw() || self.active_variant().is_some_and(|v| v.base.is_some()) {
            return true;
        }
        let want = autoshade::render::BasePart::of(&self.recipe);
        if want == self.source_part {
            return true;
        }
        if self.source_part_inflight.as_ref() == Some(&want) {
            return false;
        }
        let Some(path) = self.src_path.clone() else { return true };
        let edge = self.preview_edge.clamp(640, 8192);
        self.source_part_inflight = Some(want.clone());
        let (p, part) = (path.clone(), want.clone());
        self.spawn_worker(
            move || {
                let _mem = crate::budget::heavy_permit(crate::budget::estimate_mb(Some(&p)));
                let img = autoshade::render::base_develop(&p, &part, Some(edge)).map(Arc::new);
                Msg::BaseRedeveloped { path: p, part: Box::new(part), img: Box::new(img) }
            },
            move |e| Msg::BaseRedeveloped { path, part: Box::new(want), img: Box::new(Err(e)) },
        );
        false
    }

    /// Install a re-developed source base ([`Self::ensure_source_part`]),
    /// when it is still the one wanted: same photo, and the canvas recipe
    /// still names that part. Turned into the plates' current frame first
    /// (`sync_base_turns` owns the turn; a fresh develop is in the EXIF frame).
        /// The full-resolution SHORT EDGE of the active card's pixel source — the
    /// film the Detail panel states its radii in (`render::FilmScale`,
    /// v1.5.0), so the canvas shows sharpening and noise reduction the way the
    /// export looks once it is downscaled to the canvas.
    ///
    /// A header read (`decode::film_short_edge`), cached against the path AND its
    /// modification time: the preview asks on every slider tick, and the
    /// answer changes only when the source does — a re-baked raster at the
    /// same path included. `None` (no photo, an unreadable header) develops the
    /// canvas as its own film, which is what every build before v1.5.0 did.
    pub(crate) fn film_short_edge(&mut self) -> Option<u32> {
        let path = self.active_source_path()?;
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if let Some((cached, when, edge)) = &self.film_edge
            && *cached == path
            && *when == modified
        {
            return *edge;
        }
        let edge = autoshade::decode::film_short_edge(&path);
        self.film_edge = Some((path, modified, edge));
        edge
    }

    /// Does the active card develop a RAW negative (`decode::is_raw` on the
    /// same path `film_short_edge` reads)? The Sharpening default is
    /// Lightroom's own for the kind — 40 on a RAW, none on a baked raster
    /// (`EditRecipe::capture_sharpening`, v1.6.0) — so every surface that
    /// develops the canvas's pixels passes this beside the film edge. No
    /// photo is a baked raster: nothing to sharpen by default.
    pub(crate) fn source_is_raw(&self) -> bool {
        self.active_source_path().is_some_and(|p| autoshade::decode::is_raw(&p))
    }

    /// The frame of the active card's pixel source (v1.17.0): a RAW, or a
    /// baked file with the lens geometry its own marker says is in it
    /// (`render::SourceFrame::of`, cached per file for the process).
    pub(crate) fn source_frame(&self) -> anyhow::Result<autoshade::render::SourceFrame> {
        match self.active_source_path() {
            Some(p) => autoshade::render::SourceFrame::of(&p),
            None => Ok(autoshade::render::SourceFrame::Baked(None)),
        }
    }

    /// Accept one worker-built frame if it still describes the active base +
    /// recipe. Old frames are dropped without touching textures; `dirty` remains
    /// set by the newer edit and starts next. Texture handles are UPDATED in
    /// place, avoiding a manager allocate/free cycle on every slider tick.
    pub(crate) fn finish_redevelop(&mut self, ctx: &egui::Context, done: anyhow::Result<PreviewDone>) {
        self.develop_inflight = false;
        let frame = match done {
            Ok(frame) => frame,
            // Pure preview work has no ordinary error path; only a caught panic
            // lands here. Keep the last good frame and surface the failure —
            // WITHOUT fail(): that helper also clears the global `busy` flag,
            // which the preview path never set, and a panicking preview worker
            // mid-export used to unlock every busy-gated action.
            Err(e) => {
                let text = format!("{}: {e}", tr(self.lang, "preview develop failed"));
                self.status = text.clone();
                self.toast(ToastKind::Error, text);
                return;
            }
        };
        let current = self
            .base_preview
            .as_ref()
            .is_some_and(|base| Arc::ptr_eq(base, &frame.base))
            && self.recipe == frame.recipe;
        if !current {
            // The frame was solved for a recipe that changed mid-flight. The
            // redispatch gate is `dirty && !develop_inflight`, so a caller
            // that mutated the recipe WITHOUT setting dirty (historically the
            // crop path) would freeze the preview here forever — always
            // re-arm, the gate coalesces to a single fresh develop.
            self.dirty = true;
            ctx.request_repaint();
            return;
        }
        self.develop_count += 1;
        self.histogram = Some(frame.histogram);

        // The ColorImage arrives worker-built (see PreviewDone.after) — the UI
        // thread only uploads. The raw RGB is retained so the clipping toggle
        // can rebuild its overlay later without a redevelop.
        // A frame of the SAME photo and version re-renders in place (a slider
        // drag must not flicker); the first frame of a different one lands on
        // a fresh texture and the old one fades out over it (MOTION_FADE_S).
        let shows = self.src_path.clone().map(|p| (p, self.active));
        match &mut self.after_tex {
            Some(tex) if self.after_shows == shows => tex.set(frame.after, egui::TextureOptions::LINEAR),
            old => {
                let fresh = ctx.load_texture("after", frame.after, egui::TextureOptions::LINEAR);
                self.canvas_fade = Some((old.replace(fresh), Instant::now()));
                ctx.request_repaint();
            }
        }
        self.after_shows = shows;
        // The clipping layer follows the CURRENT toggle, not the one baked at
        // dispatch: pressing J changes no recipe field, so an in-flight frame
        // is still accepted — its stale decision used to blank the layer under
        // a lit ▲ (or resurrect a ghost layer after J-off).
        let clip = if self.show_clipping {
            // Crop-blanked like the worker path — the J-mid-flight fallback
            // used to warn outside the crop while the worker's layer did not.
            Some(
                frame
                    .clipping
                    .unwrap_or_else(|| clipping_overlay_for(&frame.rgb, frame.recipe.crop)),
            )
        } else {
            None
        };
        self.last_rgb = Some(frame.rgb);
        match clip {
            Some(clip) => {
                if let Some(tex) = &mut self.clip_tex {
                    tex.set(clip, egui::TextureOptions::NEAREST);
                } else {
                    self.clip_tex =
                        Some(ctx.load_texture("clip", clip, egui::TextureOptions::NEAREST));
                }
            }
            None => self.clip_tex = None,
        }
        if let Some(v) = self.variants.get_mut(self.active) {
            if let Some(tex) = &mut v.thumb {
                tex.set(frame.thumb, egui::TextureOptions::LINEAR);
            } else {
                v.thumb = Some(ctx.load_texture("vthumb", frame.thumb, egui::TextureOptions::LINEAR));
            }
        }
        // A global change can alter a Range Mask's coverage reference. The
        // coverage-aware key makes this a cheap no-op for ordinary local effect
        // sliders, whose coverage is independent of their pixel adjustment.
        self.overlay_stale = true;
        ctx.request_repaint();
    }

    pub(crate) fn poll_workers(&mut self, ctx: &egui::Context) {
        // UI-thread language for the status/toast strings built HERE, at
        // landing. Worker results carry typed FACTS (FitOutcome, RetouchNote,
        // PasteOutcome, …) that the landing arms render in the language live
        // NOW (L12#4 — a sentence translated inside the spawn closure carried
        // the language captured at spawn, minutes stale by the time it
        // landed). The Strings that still ride the channel are payloads, not
        // pre-translated sentences: a generative worker's heartbeat line, a
        // mask's stable English label and the sidecar's backend name, an
        // extracted style prompt, a model id, an error's own text.
        let lang = self.lang;
        // Drain a bounded batch each frame so a burst of thumbnails doesn't take
        // one-per-frame to land (try_recv borrow is released before we mutate).
        for _ in 0..64 {
            let Some(msg) = self.rx.as_ref().and_then(|rx| rx.try_recv().ok()) else { break };
            match msg {
                Msg::Opened(boxed) => {
                    self.on_opened(ctx, lang, boxed);
                    // A Lightroom AI mask arrives without an alpha: cut it
                    // on this computer now, or the canvas skips it.
                    self.start_ai_mask_resolve(lang);
                }
                Msg::BaseRedeveloped { path, part, img } => {
                    self.on_base_redeveloped(ctx, path, *part, *img)
                }
                Msg::AiMasksResolved(boxed) => self.on_ai_masks_resolved(lang, *boxed),
                Msg::Developed(boxed) => self.finish_redevelop(ctx, *boxed),
                Msg::Analyzed(epoch, mut boxed) => {
                    // The look protocol's claim: only the pending look's own,
                    // current epoch may touch this result. A looked-for verb
                    // also puts back what its fresh look replaced, BEFORE the
                    // shared landing writes.
                    let claim = logic::look::claim(&mut self.look, epoch, self.gen_epoch, &boxed);
                    if let Some(claim) = &claim
                        && let Ok((recipe, _, _)) = &mut *boxed
                    {
                        logic::look::restore(claim, recipe);
                    }
                    let landed = boxed.is_ok();
                    self.on_analyzed(lang, epoch, boxed);
                    // The claimed look's own line, memory and view facts.
                    if let Some(claim) = claim {
                        let effects = logic::look::landed(&claim, &self.recipe, landed);
                        self.run_effects(effects);
                    }
                }
                Msg::OneClickBuildProgress { done, total } => {
                    self.normal.build_progress = Some((done, total));
                }
                Msg::OneClickBuilt(res) => self.on_one_click_built(lang, res),
                Msg::PythonEnvLine(line) => self.python_env_line = Some(line),
                Msg::PythonEnvReady(res) => self.on_python_env_ready(lang, res),
                Msg::PortraitFound(found) => self.on_portrait_found(lang, *found),
                Msg::Exported(Ok(outcome)) => {
                    // Normal's Save group words its own landing (`normal.rs`).
                    let plain = self.normal_delivery_words(lang, &outcome);
                    self.batch_progress = None; // the bar belongs to ONE batch run
                    match outcome {
                        ExportOutcome::Single { out, relooked, warn } => {
                            // ABSOLUTE (R22-7): the deliverable exists NOW, and
                            // a folder relative to the launch directory was the
                            // single most-reported "where did my file go".
                            let p = abs_display(&out);
                            if relooked
                                && let Some(photo) = self.src_path.clone()
                                && autoshade::pipeline::repair_pre_era_base_curve(
                                    &photo,
                                    &mut self.recipe,
                                )
                                .is_some()
                            {
                                // The export repaired its own COPY: the canvas
                                // is repaired too, or export and preview stay
                                // divergent for the rest of the session.
                                self.dirty = true;
                            }
                            let words =
                                logic::export_landing::single_words(&p, relooked, warn.as_deref());
                            let mut line = crate::i18n::text(lang, &words.line);
                            if let Some(relook) = words.relook {
                                line = format!("{line} — {}", crate::i18n::text(lang, &relook));
                            }
                            match words.warn {
                                Some(warn) => {
                                    let t = format!(
                                        "{line} · {}",
                                        crate::i18n::text(lang, &warn)
                                    );
                                    self.status = t.clone();
                                    self.toast(ToastKind::Error, t);
                                }
                                None => self.done(line),
                            }
                        }
                        ExportOutcome::Batch { ok, errs, renamed, relooked, warns, dest } => {
                            // Where the batch actually DELIVERED becomes the
                            // remembered destination — here, not at the dialog
                            // (R22 M1): a folder inside the photo library is
                            // refused per photo, so a batch that picked one
                            // lands nothing and must not be remembered. `ok > 0`
                            // is the proof that at least one file is there.
                            if ok > 0 {
                                self.last_export_dir = Some(dest.clone());
                            }
                            let words = logic::export_landing::batch_words(
                                ok,
                                &errs,
                                &renamed,
                                relooked,
                                &warns,
                                &abs_display(&dest),
                            );
                            let mut line = crate::i18n::text(lang, &words.base);
                            for part in [words.renamed, words.relooked, words.develops]
                                .into_iter()
                                .flatten()
                            {
                                line.push_str(&crate::i18n::text(lang, &part));
                            }
                            if errs.is_empty() {
                                self.done(line);
                            } else {
                                // A partial failure keeps the error channel,
                                // exactly as the worker-side bail! did.
                                self.fail(tr(lang, "export failed"), anyhow::anyhow!("{line}"));
                            }
                        }
                    }
                    if let Some(plain) = plain {
                        self.reword_landing(plain);
                    }
                }
                Msg::Exported(Err(e)) => {
                    self.batch_progress = None;
                    self.normal.delivering = false;
                    self.fail(tr(lang, "export failed"), e);
                }
                Msg::BatchProgress { done, total } => {
                    self.batch_progress = Some((done, total));
                    // No destination here: the tick fires once per photo and the
                    // root was already named when the batch started (and is
                    // named again at the landing). Repeating it 500 times only
                    // pushed the counts out of a truncated status line — and
                    // the old hardcoded "./out" became a lie the moment the
                    // destination became a setting (R22-7).
                    self.status = trf(
                        lang,
                        "Batch-rendering {done}/{total} …",
                        &[("done", &done.to_string()), ("total", &total.to_string())],
                    );
                }
                Msg::StyleInfo(info) => {
                    self.style_info_loading = false;
                    self.style_info = Some(*info);
                }
                Msg::StyleBuildProgress { stage, done, total } => {
                    self.style_build_progress = Some((stage, done, total));
                    // The panel's own line renders these counts; the status bar
                    // gets them too, because a minutes-long job must be visible
                    // from wherever the user is looking.
                    self.status = trf(
                        lang,
                        "Building the style library… {stage}: {done} / {total} photos",
                        &[
                            ("stage", tr(lang, stage.label())),
                            ("done", &done.to_string()),
                            ("total", &total.to_string()),
                        ],
                    );
                }
                Msg::StyleBuilt(outcome) => self.on_style_built(lang, *outcome),
                Msg::Segmented(res) => self.on_segmented(lang, res),
                Msg::DepthEstimated(res) => {
                    self.on_depth_estimated(res);
                    self.after_portrait_depth();
                }
                Msg::MaskRefined(res) => self.on_mask_refined(lang, res),
                Msg::Folder(boxed) => {
                    self.on_folder(lang, *boxed);
                    self.show_startup_note();
                }
                Msg::Thumb { generation, idx, img } => self.on_thumb(ctx, generation, idx, *img),
                Msg::MasterLoaded { photo, origin, edge, stamp, img } => {
                    self.on_master_loaded(ctx, lang, photo, origin, edge, stamp, *img)
                }
                Msg::Progress(epoch, m) => {
                    // Liveness lines from a running generative worker. Epoch-
                    // gated: after Cancel + an immediate re-run, the abandoned
                    // worker's heartbeats must not overwrite the new task's
                    // status line.
                    if self.busy && self.gen_cancel.is_some() && epoch == self.gen_epoch {
                        self.status = m;
                    }
                }
                Msg::Retouched(epoch, boxed) => {
                    // Normal's noise switch lands through the same door —
                    // with the look's undo history kept across its card.
                    let history = self.normal_denoise_history();
                    self.on_retouched(ctx, lang, epoch, *boxed);
                    self.after_normal_denoise(lang, history);
                }
                Msg::Fitted(boxed) => self.on_fitted(ctx, lang, boxed),
                Msg::Styled(boxed) => match *boxed {
                    Ok((prompt, note)) => {
                        // Into the Reimagine prompt: ready to restyle OTHER photos.
                        self.reimagine_prompt = prompt;
                        let s = match &note {
                            StyleNote::SavedCopy => tr(
                                lang,
                                "Style prompt extracted → filled into the Generate image prompt (also saved <stem>.style.txt in the delivery folder)",
                            )
                            .to_string(),
                            StyleNote::SaveFailed(err) => trf(
                                lang,
                                "Style prompt extracted → filled into the Generate image prompt (saving <stem>.style.txt in the delivery folder failed: {err})",
                                &[("err", err)],
                            ),
                            StyleNote::NotSaved => tr(
                                lang,
                                "Style prompt extracted → filled into the Generate image prompt",
                            )
                            .to_string(),
                        };
                        self.done(s);
                    }
                    Err(e) => {
                        self.fail(tr(lang, "Style extraction failed"), e);
                    }
                },
                Msg::Pasted(res) => {
                    // Normal's 「apply to every photo」 renders once the paste
                    // has landed whole (`normal.rs`).
                    let then_render = self
                        .normal
                        .apply_all
                        .take()
                        .map(|dest| (dest, res.as_ref().is_ok_and(|o| o.errs.is_empty())));
                    self.on_pasted(lang, res);
                    if let Some((dest, pasted)) = then_render {
                        self.after_apply_all_paste(lang, dest, pasted);
                    }
                }
                Msg::LegacyImported(res) => {
                    self.edited_badge.clear(); // imported sidecars light ● badges
                    match res {
                        Ok((n, dir)) => self.done(trf(
                            lang,
                            "Imported saved develops for {n} photo(s) from {path}",
                            &[("n", &n.to_string()), ("path", &dir.display().to_string())],
                        )),
                        Err(e) => self.fail(tr(lang, "import failed"), e),
                    }
                }
                Msg::Models(role, generation, res) => self.on_models(lang, role, generation, res),
            }
        }
    }

    /// Keep the frame loop alive while any worker (analyze/export/thumbs/
    /// models/master decodes) runs — at a 100 ms poll, not frame rate:
    /// worker completion only surfaces through the mpsc poll, and a
    /// full-rate repaint burned CPU for the whole life of a stalled 600 s AI
    /// call. Input still repaints immediately; 100 ms only bounds COMPLETION
    /// latency. `master_loads` is in the gate because a cold-variant master
    /// decode is NOT `busy` (16-lane scan L06). Called at the END of
    /// update() — after every panel had its chance to spawn (L12-3): inside
    /// poll_workers (the top of the frame) it judged a frame whose spawns
    /// had not happened yet, and the first batch of workers ran with no
    /// completion pump armed.
    pub(crate) fn pump_repaint_gate(&mut self, ctx: &egui::Context) {
        if self.busy
            || self.thumb_inflight > 0
            || self.settings.image_models.fetching
            || self.settings.analysis_models.fetching
            || !self.master_loads.is_empty()
            // R23-2: the style-library build is NOT busy-gated (it runs for
            // minutes and the app stays usable), so without it here the
            // "building…" state would only repaint when a progress tick
            // happens to arrive — a single slow 60 MP decode would freeze the
            // counter for as long as it takes. Same reason the two model
            // fetches are on this list.
            || self.style_build_inflight
            || self.style_info_loading
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    /// `Msg::Opened` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
        /// `Msg::Analyzed` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
    pub(crate) fn on_analyzed(
        &mut self,
        lang: Lang,
        epoch: u64,
        boxed: Box<
            anyhow::Result<(
                EditRecipe,
                autoshade::advisor::Verdict,
                Vec<autoshade::rationale::Note>,
            )>,
        >,
    ) {
                    // Cleared whatever the epoch: the wire is free either way,
                    // and this is what re-arms the Analyze button.
                    self.analyze_inflight = false;
                    if epoch != self.gen_epoch {
                        // A cancelled Analyze's late result: the user already
                        // moved on. The Ok arm below also PERSISTS (backup +
                        // recipe.json + XMP) — a stale install would not just
                        // repaint the canvas, it would save over the photo's
                        // develop. Discard entirely (Err is just as silent).
                        return; // was `continue` — the match was the loop's last statement
                    }
                    self.gen_cancel = None;
                    match *boxed {
                    Ok((recipe, verdict, rationale_notes)) => {
                        // Sliders stay live while Analyze runs (10-30 s):
                        // flush any name typed during the wait (the resync
                        // below clears the mask buffer — unflushed, the
                        // rename died with it, M16), then commit any
                        // mid-flight edit as its own undo step NOW, or the
                        // wholesale install below folds it into the
                        // pre-analyze step and Ctrl+Z skips it.
                        self.commit_pending_names();
                        self.commit_now();
                        let accepted = logic::analysis_landing::accepted(&verdict);
                        // The recipe arrives with its base_curve already
                        // stamped by produce_recipe (saved-curve-first, else a
                        // fresh estimate) — the single authority all three
                        // surfaces share; the canvas must not override it.
                        // One exception, canvas-side only: a GENERATED variant
                        // already carries the camera look AND the lens
                        // corrections in its pixels, so developing it through
                        // the RAW's curve would cook both twice — the canvas
                        // copy drops them; the persist below still writes the
                        // full stamped recipe (it is the RAW's saved develop,
                        // and reopening lands on the source). An InPlace
                        // retouch master is a NEUTRAL develop (see the
                        // RetouchKind::InPlace arm): its recipe legitimately
                        // keeps the calibration — stripping there rendered the
                        // canvas neutral-dark while the disk recipe kept the
                        // look.
                        // The landing's two recipes: the canvas copy a
                        // GENERATED card needs (baked look stripped), and the
                        // store's own (the RAW's develop). See
                        // `logic::analysis_landing::shape`.
                        let shaped = logic::analysis_landing::shape(
                            recipe,
                            self.skin_held(),
                            self.active_on_ai_pixels(),
                        );
                        let stamped = shaped.stamped;
                        self.recipe = shaped.canvas;
                        // The immutability rule, BEFORE the persist below
                        // reads the strip: an analysis landing on the
                        // pristine ✨ card is an edit, and lands on a new ✎
                        // card — the record this landing writes names it.
                        self.fork_edited_card();
                        // Wholesale replacement: disarm index-carrying tools +
                        // refresh rationale, THEN install the fresh verdict
                        // and the typed notes (resync cleared both).
                        self.resync_recipe_display();
                        self.rationale_notes = rationale_notes;
                        self.verdict =
                            Some((verdict.decision.clone(), verdict.reasons.clone()));
                        self.dirty = true;
                        self.busy = false;
                        // Persist like the CLI and the web do — the badge,
                        // batch render and reopening all read recipe.json, so
                        // an unpersisted analyze silently diverges from every
                        // one of them. An existing explicit save is snapshotted
                        // to v<N> first, never destroyed.
                        self.status = if !accepted {
                            // The verifier itself judged this result not
                            // ready, and a non-Accept verdict may not
                            // auto-save (user decision). The result stays on
                            // the canvas as an UNSAVED edit — ● lit, the nav
                            // stash and the close guard protect it — and the
                            // user decides: Ctrl+S keeps it, Ctrl+Z steps
                            // back to the pre-analyze edit.
                            self.show_words(
                                lang,
                                logic::analysis_landing::words(
                                    &logic::analysis_landing::SaveReport::NotAccepted {
                                        decision: autoshade::advisor::decision_key(&verdict.decision),
                                    },
                                ),
                            )
                        } else { match self.src_path.clone() {
                            // The backup gate comes FIRST: if the existing save
                            // cannot be snapshotted, it is not overwritten —
                            // the analyze result stays on the canvas only.
                            // Persist `stamped`, not the canvas copy: the
                            // sidecar is the RAW's develop and keeps its curve.
                            // ONE NoWait lock across gate → recipe → pixel
                            // link → XMP (the Ctrl+S pairing): busy must
                            // degrade to "applied but NOT saved", never hang
                            // the UI thread behind another process.
                            Some(p) => self.persist_analysis_landing(lang, p, stamped),
                            None => self.show_words(
                                lang,
                                logic::analysis_landing::words(&logic::analysis_landing::SaveReport::NoPhoto),
                            ),
                        } };
                    }
                    Err(e) => {
                        self.fail(tr(lang, "analyze failed"), e);
                    }
                }
    }

    }
