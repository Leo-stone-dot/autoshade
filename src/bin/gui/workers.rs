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
    fn install_master(
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

    /// `Msg::StyleBuilt` landing (R23-2): the typed outcome becomes a sentence
    /// in the language live NOW (L12#4), the button re-arms, and a successful
    /// build both remembers its folder (so a rebuild starts there) and triggers
    /// a fresh status read — the panel must not keep showing the OLD counts.
    fn on_style_built(&mut self, lang: Lang, outcome: StyleBuildOutcome) {
        self.style_build_inflight = false;
        self.style_build_progress = None;
        // The words are the logic layer's; the state below is this landing's.
        let (words, dir) = match &outcome {
            StyleBuildOutcome::Saved { total, dir, phrased } => (
                logic::style::words(&logic::style::Outcome::Saved {
                    total: *total,
                    dir_display: abs_display(dir),
                    phrased: *phrased,
                }),
                Some(dir.clone()),
            ),
            StyleBuildOutcome::Failed { err } => (
                logic::style::words(&logic::style::Outcome::Failed { err: err.clone() }),
                None,
            ),
        };
        self.show_words(lang, words);
        if let Some(dir) = dir {
            self.style_src_dir = Some(dir);
            // A build has just run the embedding model, so its one-time
            // download is done: searching is what the user built it for.
            self.style_embed = true;
            self.start_style_info();
        }
    }

    /// `Msg::Segmented` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
    fn on_segmented(&mut self, lang: Lang, res: anyhow::Result<(String, PathBuf, String)>) {
        match res {
            Ok((label, path, backend)) => {
                // The mask choice, the words and the fallback disclosure are
                // the logic layer's (`logic/segmented.rs`).
                let photo_stem = self.src_path.clone().map(|p| autoshade::pipeline::stem(&p).to_owned());
                let landed =
                    logic::segmented::apply(&mut self.recipe, &label, &path, &backend, photo_stem.as_deref());
                self.sel_mask = Some(landed.selection);
                // The landing MOVED the selection — index-armed tools bound to
                // another row die with it, exactly like the row-click sites
                // (Codex L06-1: this async door was missed and a stranded brush
                // baked into the old mask).
                if self.disarm_selection_bound_tools(self.sel_mask) {
                    let t = crate::i18n::text(lang, &logic::segmented::selection_taken_words());
                    self.toast(ToastKind::Error, t);
                }
                self.overlay_stale = true; // fresh raster ⇒ fresh coverage
                self.busy = false;
                // MarkDirty + the status line (+ the fallback toast).
                self.run_effects(landed.effects);
                if let Some(note) = landed.backend_note {
                    self.status = format!("{}{}", self.status, crate::i18n::text(lang, &note));
                }
            }
            Err(e) => {
                self.fail(tr(lang, "AI segmentation failed"), e);
            }
        }
    }

    /// `Msg::MaskRefined` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
    fn on_mask_refined(&mut self, lang: Lang, res: anyhow::Result<MaskRefineOutcome>) {
        match res {
            Ok(outcome) => {
                // Which mask the raster belongs to, and the refusal lines, are
                // the logic layer's (`logic/mask_refine.rs`).
                let outcome = match outcome {
                    MaskRefineOutcome::OverBudget { w, h } => {
                        logic::mask_refine::Outcome::OverBudget { w, h }
                    }
                    MaskRefineOutcome::Refined(index, stored_ref, out) => {
                        logic::mask_refine::Outcome::Refined {
                            index,
                            stored_ref,
                            out_display: out.display().to_string(),
                            out,
                        }
                    }
                };
                let landed = logic::mask_refine::apply(&mut self.recipe, &outcome);
                if let Some(i) = landed.selection {
                    self.sel_mask = Some(i);
                    // Same rule as the segmentation landing: the selection
                    // moved, selection-bound tools on another row follow it.
                    if self.disarm_selection_bound_tools(self.sel_mask) {
                        let t = crate::i18n::text(lang, &logic::segmented::selection_taken_words());
                        self.toast(ToastKind::Error, t);
                    }
                    self.overlay_stale = true;
                }
                self.busy = false;
                self.run_effects(landed.effects);
            }
            Err(e) => {
                self.fail(tr(lang, "mask refine failed"), e);
            }
        }
    }

    /// `Msg::Folder` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
    fn on_folder(&mut self, lang: Lang, res: anyhow::Result<(PathBuf, Vec<PathBuf>, usize)>) {
                match res {
                    Ok((dir, list, skipped)) => {
                        let n = list.len();
                        self.gallery = list;
                        self.gallery_dir = Some(dir);
                        self.gallery_gen += 1; // invalidate any in-flight old thumbs
                        self.thumbs.clear();
                        self.thumb_requested.clear();
                        // Keyed by gallery INDEX, so it is meaningless across
                        // folders: without this, index 0 failing three times
                        // here silently denied index 0 a thumbnail in every
                        // folder opened afterwards.
                        self.thumb_fail.clear();
                        self.thumb_inflight = 0;
                        self.edited_badge.clear(); // badge stats belong to the old folder
                        self.selected = None;
                        self.multi_sel.clear(); // indices belong to the old folder
                        self.busy = false;
                        // The count line and the skipped-items disclosure are
                        // the logic layer's words.
                        let words = logic::folder::words(n, skipped);
                        self.status = crate::i18n::text(lang, &words.status);
                        if let Some(skip) = words.skipped {
                            self.status = format!("{} — ⚠ {}", self.status, crate::i18n::text(lang, &skip));
                        }
                        // L12#3 tofu disclosure: a gallery name in a script
                        // no installed face covers renders as boxes — say
                        // so ONCE per script per session, at folder open
                        // (the one cheap point; never per frame).
                        self.disclose_undrawable_names();
                    }
                    Err(e) => {
                        self.fail(tr(lang, "scan failed"), e);
                    }
                }
    }

    /// `Msg::Thumb` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
        /// `Msg::MasterLoaded` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
    #[allow(clippy::too_many_arguments)]
    fn on_master_loaded(&mut self, ctx: &egui::Context, lang: Lang, photo: PathBuf, origin: PathBuf, edge: u32, stamp: FileStamp, img: anyhow::Result<image::DynamicImage>) {
                    // The in-flight marker clears on EVERY outcome — photo
                    // mismatch included — or one failed decode would block
                    // all retries for the rest of the session.
                    self.master_loads.remove(&(photo.clone(), origin.clone()));
                    // Install by identity, not index: only while the SAME
                    // photo is open, and only into strip entries that still
                    // reference this exact master and still await pixels.
                    if self.src_path.as_ref() == Some(&photo) {
                        match img {
                            Ok(im) => {
                                let arc = Arc::new(im);
                                // Remember before installing, under the
                                // SPAWN-time edge + stamp (L12-2/L12-6): the
                                // current preference may have moved while
                                // this decoded, and these pixels are the
                                // spawn's, not the preference's.
                                self.remember_master(&origin, edge, stamp, arc.clone());
                                self.install_master(ctx, &photo, &origin, arc);
                            }
                            Err(e) => {
                                // A ▣ Original card's in-place master (an
                                // old-version in-place denoise, a heal or
                                // clone) DELETED from disk — e.g. the user
                                // cleaned the delivery folder: the card's
                                // pixels are the loaded file again, which is
                                // what the canvas already shows. Keeping the
                                // dead link sent every later pixel action
                                // (AI denoise, heal, export) to a file that
                                // is not there (user report 2026-10-18). Only
                                // a VANISHED file drops the link; a master
                                // that exists but will not decode keeps it.
                                let mut dropped = false;
                                if !origin.exists() {
                                    for v in &mut self.variants {
                                        if v.kind == VariantKind::Original
                                            && v.base.is_none()
                                            && v.origin.as_ref() == Some(&origin)
                                        {
                                            v.origin = None;
                                            dropped = true;
                                        }
                                    }
                                }
                                let t = if dropped {
                                    trf(
                                        lang,
                                        "the ▣ Original card's retouch master {path} was deleted — the card now works from the photo file itself (Ctrl+S saves that)",
                                        &[("path", &origin.display().to_string())],
                                    )
                                } else {
                                    trf(
                                        lang,
                                        "this variant's saved master could not be loaded ({err}) — showing the un-retouched source develop instead",
                                        &[("err", &e.to_string())],
                                    )
                                };
                                self.toast(ToastKind::Error, t);
                            }
                        }
                    }
    }

    /// Render one retouch fact in the CURRENT language (L12#4) — the
    /// Render one retouch note's words: the logic layer returns the parts,
    /// the landing renders and appends them (`logic/retouch.rs`).
    fn render_retouch_note(lang: Lang, n: &RetouchNote) -> String {
        let words = logic::retouch::words(n);
        let mut s = crate::i18n::text(lang, &words.text);
        if let Some(path) = words.path_suffix {
            s = format!("{s} · {path}");
        }
        for suffix in words.suffixes {
            s.push_str(&crate::i18n::text(lang, &suffix));
        }
        if !words.prose.is_empty() || !words.warnings.is_empty() {
            let mut tail = words.prose;
            for warning in words.warnings {
                tail.push_str(&crate::i18n::text(lang, &warning));
            }
            s = format!("{s} — ⚠ {tail}");
        }
        s
    }

    /// `Msg::Retouched` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
    // pub(crate): the L06#4 commit-ordering test drives this landing directly.
    pub(crate) fn on_retouched(&mut self, ctx: &egui::Context, lang: Lang, epoch: u64, done: RetouchDone) {
                    if epoch != self.gen_epoch {
                        // A cancelled task's late result: the user already
                        // moved on — never let it mutate the canvas. And the
                        // cancel toast PROMISES "the late result is
                        // discarded" (L12-7): a late SUCCESS's pixel master
                        // is unreferenced by construction (unique_out claimed
                        // a fresh name; nothing installed it), so it is
                        // removed instead of accumulating one orphan
                        // full-res file per cancel. Err stays silent.
                        if let Ok((_, _, saved, _)) = &done {
                            let _ = std::fs::remove_file(saved);
                        }
                        return; // was `continue` — the match was the loop's last statement
                    }
                    self.gen_cancel = None;
                    match done {
                    Ok((img, note, saved, kind)) => {
                        let msg = Self::render_retouch_note(lang, &note);
                        // The tool this result consumed starts clean; the
                        // other tools' areas are theirs to keep (2026-09-27).
                        if let Some(owner) = note.consumed_area() {
                            self.clear_area(owner);
                        }
                        match kind {
                            RetouchKind::NewGenerated => {
                                // Whole-frame reimagine → a NEW「AI 生成」variant
                                // whose base IS this raster. Auto-switch to it, so
                                // editing works on the generated pixels — a slider
                                // no longer reverts to the source develop. Its path
                                // is the reverse-fit / full-res export source.
                                self.push_variant(
                                    Variant {
                                        kind: VariantKind::Generated,
                                        id: new_variant_id(),
                                        name: None,
                                        recipe: EditRecipe::default(),
                                        base: Some(Arc::new(img)),
                                        origin: Some(saved),
                                        thumb: None,
                                    },
                                    ctx,
                                );
                            }
                            RetouchKind::Layer => {
                                // A fill layer (2026-10-01): the patch joins
                                // THIS card's develop as one undo step, the
                                // Analyze landing's order — commit what was
                                // there first, so Ctrl+Z takes the layer off
                                // and keeps a slider moved while it ran. On a
                                // pristine ✨ card the first edit continues on
                                // an ✎ card, as every edit there does.
                                self.commit_pending_names();
                                self.commit_now();
                                self.fork_from_generated();
                                // Absolute, so the saved recipe finds it from
                                // any working directory.
                                let path = std::path::absolute(&saved).unwrap_or(saved);
                                self.recipe.pixel_layers.push(autoshade::render::PixelLayer {
                                    path: path.display().to_string(),
                                    ..Default::default()
                                });
                                self.dirty = true;
                                self.commit_now();
                                drop(img);
                            }
                            RetouchKind::NewDenoised | RetouchKind::NewStacked => {
                                // A REMADE NEGATIVE — an AI denoise (◈,
                                // 2026-09-15) or a stack (▦, v1.5.0) — lands
                                // the same way, because it is the same idea:
                                // the saved master is its pixel source, the
                                // develop it was made from is its recipe (the
                                // LIVE one — a slider moved while the job ran
                                // is what the user sees), and the card it was
                                // made from keeps its pixels. push_variant
                                // saves that card's recipe and switches here,
                                // exactly like a reimagine landing. Nothing
                                // enters the outgoing card's undo history — a
                                // card is deleted, not undone.
                                let mut recipe = self.recipe.clone();
                                let stacked = kind == RetouchKind::NewStacked;
                                // An HDR merge hands over stops of highlight
                                // above the reference frame's white. They are
                                // only reachable through the SDR rendition
                                // stage, so the card arrives with that stage
                                // ON and told how much room it has — the same
                                // recipe the CLI writes beside its master.
                                if let RetouchNote::Stacked { headroom_ev, .. } = &note
                                    && *headroom_ev > 0.0
                                {
                                    recipe.hdr_edit = true;
                                    recipe.hdr_max_ev = *headroom_ev;
                                }
                                self.push_variant(
                                    Variant {
                                        kind: if stacked {
                                            VariantKind::Stacked
                                        } else {
                                            VariantKind::Denoised
                                        },
                                        id: new_variant_id(),
                                        name: None,
                                        recipe,
                                        base: Some(Arc::new(img)),
                                        origin: Some(saved),
                                        thumb: None,
                                    },
                                    ctx,
                                );
                            }
                            RetouchKind::InPlace => {
                                // heal/clone: a pixel touch-up of the
                                // NEUTRAL-DEVELOP base (never the developed
                                // rendition — the recipe keeps rendering ON TOP,
                                // so baking developed pixels would cook the tone
                                // twice). Bake it into the active variant's base
                                // AND repoint its origin at the saved full-res
                                // artifact, so export / reverse-fit / a further
                                // retouch all follow the retouched pixels.
                                //
                                // Sliders stay live while a retouch runs (minutes
                                // for a generative fill) and `current_step` reads
                                // the variant's base — committing only AFTER the
                                // swap below folded a mid-flight recipe edit into
                                // the pixel step, so one Ctrl+Z reverted both and
                                // there was no way to keep the slider move while
                                // dropping the retouch. Same rule, same reason as
                                // the Analyze landing: commit (and flush every
                                // typed name) BEFORE the state swap.
                                self.commit_pending_names();
                                self.commit_now();
                                // The immutability rule for PIXELS: a retouch
                                // on the pristine ✨ card bakes into a new ✎
                                // card on the same raster, and the ✨ card
                                // keeps its pixels. Asked directly — the
                                // recipe may well be neutral here.
                                self.fork_from_generated();
                                let img = Arc::new(img);
                                let (mw, mh) = img.dimensions();
                                if let Some(v) = self.variants.get_mut(self.active) {
                                    v.base = Some(img.clone());
                                    v.origin = Some(saved);
                                }
                                // The InPlace master is a NEUTRAL develop (see
                                // above) — Before shows it under the recipe's
                                // own camera curve, exactly like the source
                                // compare; an empty curve here sat 0.6–1.4 EV
                                // under After's starting point.
                                let start = self.recipe.start_only();
                                self.set_before(ctx, &img, &start);
                                // New baked base ⇒ the cached masks-cleared
                                // reference develop no longer matches it.
                                self.overlay_ref = None;
                                self.overlay_stale = true;
                                self.base_preview = Some(img);
                                // Keep the paint canvas sized to the new base (the
                                // retouch result can differ in dimensions — e.g. a
                                // non-square reimagine origin) — and end a brush
                                // session armed on the replaced plate.
                                self.rebind_paint_canvas(mw, mh);
                                self.dirty = true;
                                // The pixel swap must enter history THIS frame
                                // — a same-frame Ctrl+Z otherwise undoes an
                                // older recipe step over the new pixels.
                                self.commit_now();
                                // The decoded-base cache now describes pixels
                                // this photo no longer shows.
                                self.forget_open_base();
                            }
                        }
                        self.done(msg);
                        // A whole-frame reimagine asks whether to reverse-fit
                        // it (v1.17.0) — after `done`, so its job is over. An
                        // adjust lands a ✨ card too, but it edits a look
                        // already generated; the question follows 「AI
                        // generate」.
                        if let RetouchNote::Reimagined { divergence, .. } = &note {
                            self.offer_fit_prompt(ctx, *divergence);
                        }
                    }
                    Err(e) => {
                        // The face-held heal found spots, none on the face:
                        // nothing was changed, and that is an answer, not a
                        // failure (typed, never matched by its English).
                        if let Some(none) = e.downcast_ref::<autoshade::retouch::NoSpotInsideLimit>() {
                            self.done(trf(
                                lang,
                                "No spots on the face to heal — nothing was changed ({n} found elsewhere were left alone)",
                                &[("n", &none.found.to_string())],
                            ));
                        } else {
                            self.fail(tr(lang, "retouch failed"), e);
                        }
                    }
                    }
    }

    /// Render one fit landing fact as its " · "-led fragment(s). The pieces
    /// are the logic layer's (`logic/fit.rs`); only the XMP-failure sentence
    /// is bare and gets its separator here.
    pub(crate) fn render_fit_note(lang: Lang, n: &FitNote) -> String {
        let mut out = String::new();
        for piece in logic::fit::note_words(n) {
            if piece.key.starts_with(" · ") {
                out.push_str(&crate::i18n::text(lang, &piece));
            } else {
                out.push_str(" · ");
                out.push_str(&crate::i18n::text(lang, &piece));
            }
        }
        out
    }

    /// `Msg::Fitted` landing — facts arrive typed and are rendered HERE
    /// with the landing-time language (L12#4); the rationale's typed notes
    /// install after `push_variant`'s reload cleared them (L12#2B).
    // pub(crate): the negative-master landing test drives this directly.
    pub(crate) fn on_fitted(&mut self, ctx: &egui::Context, lang: Lang, boxed: Box<anyhow::Result<FitOutcome>>) {
                match *boxed {
                    // Either way the worker may have persisted a recipe.json
                    // (an Err can land after that write) — recompute badges.
                    Ok(out) => {
                        self.edited_badge.clear();
                        // Advance the ● baseline ONLY when the store actually
                        // holds the fit. A backup-gate refusal means nothing
                        // was written: leaving the baseline alone keeps
                        // ● unsaved lit and lets nav_stash protect the fit —
                        // the ordinary unsaved-edit path takes over.
                        if out.persisted {
                            self.saved_recipe = out.recipe.clone();
                            self.nav_stash.remove(
                                &self.src_path.clone().unwrap_or_default(),
                            );
                            // The pixel link the worker committed beside the
                            // recipe — the negative's in-place master when
                            // the ▣ card carries one (the ◭ card below hangs
                            // off it), cleared otherwise (a fit is
                            // source-based) — so the per-frame ● pixel
                            // comparison's disk mirror stays in step with it.
                            self.pixels_on_disk = out.negative.clone();
                        }
                        self.refresh_versions();
                        // The landing's words are the logic layer's
                        // (`logic/fit.rs`); the variant work below is this
                        // landing's.
                        let mut note = crate::i18n::text(
                            lang,
                            &logic::fit::done_words(out.err_before, out.err_after),
                        );
                        for n in &out.status {
                            note.push_str(&Self::render_fit_note(lang, n));
                        }
                        // The generated look, solved back into an editable recipe,
                        // becomes a NEW「反推」variant: base = the source neutral
                        // (same negative as Original), look carried by the recipe —
                        // so it is fully editable, exports XMP and renders at full
                        // resolution. Auto-switch to it.
                        // SAME negative includes its in-place master
                        // (2026-09-13): the card hangs off the file the fit
                        // was solved on, sharing the ▣ card's decoded pixels
                        // when it has them (a cold ▣ card leaves `base` None
                        // and `load_active` decodes the master, as for any
                        // restored baked card).
                        let base = out.negative.as_deref().and_then(|m| {
                            let i = self.original_index()?;
                            let v = &self.variants[i];
                            (v.origin.as_deref() == Some(m)).then(|| v.base.clone()).flatten()
                        });
                        self.push_variant(
                            Variant {
                                kind: VariantKind::Fitted,
                                id: new_variant_id(),
                                name: None,
                                recipe: out.recipe,
                                base,
                                origin: out.negative,
                                thumb: None,
                            },
                            ctx,
                        );
                        // AFTER the variant load (which cleared them): the
                        // fit rationale's typed copy, for draw-time zh.
                        self.rationale_notes = out.rationale_notes;
                        self.done(note);
                    }
                    Err(e) => {
                        self.edited_badge.clear();
                        // The pre-fit snapshot may exist even when the fit
                        // errored — keep the Versions list truthful.
                        self.refresh_versions();
                        self.fail(tr(lang, "Reverse-fit failed"), e);
                    }
                }
    }

    /// `Msg::Pasted` landing — body extracted verbatim from the
    /// poll_workers pump (round-12 decomposition; indentation kept).
    fn on_pasted(&mut self, lang: Lang, res: anyhow::Result<PasteOutcome>) {
        // Sidecars were written (possibly partially on error) — recompute the
        // gallery badges either way.
        self.edited_badge.clear();
        // Every photo the paste wrote now has the pasted look on disk, and that
        // look REPLACED whatever unsaved work its navigation stash held: left
        // in place, the stash would outrank the paste in a batch render, a
        // reopen and 「Save all」 at exit (the stash beats disk on all three).
        // A partial failure wrote the others all the same.
        if let Ok(out) = &res {
            for p in &out.written {
                self.nav_stash.remove(p);
            }
        }
        match res {
            // A partial failure keeps the error channel — it must never read as
            // a clean success, and the ● baseline must not advance.
            Ok(out) if out.errs.is_empty() => {
                // The open photo's paste is on disk: advance the ● baseline so
                // the marker doesn't cry wolf. Only on FULL success — a partial
                // failure could be exactly the open photo, which must keep
                // warning. Guard on the photo still being open, and compare
                // nothing: edits made while the worker ran leave
                // recipe != saved_recipe, correctly re-lighting ●.
                if let Some((p, r)) = self.pasted_open.take()
                    && self.src_path.as_ref() == Some(&p)
                {
                    self.saved_recipe = r;
                    self.nav_stash.remove(&p);
                }
                // The counts and the sidecar disclosures are the logic layer's
                // (`logic/paste.rs`); the suffixes already carry their " — ".
                let (base, suffixes) =
                    logic::paste::clean_words(out.ok, out.xmp, &out.xmp_fails, &out.xmp_notes);
                let mut s = crate::i18n::text(lang, &base);
                for suffix in suffixes {
                    s.push_str(&crate::i18n::text(lang, &suffix));
                }
                self.done(s);
            }
            Ok(out) => {
                self.pasted_open = None;
                let msg = crate::i18n::text(lang, &logic::paste::partial_words(out.ok, &out.errs));
                self.fail(tr(lang, "batch paste"), anyhow::anyhow!("{}", msg));
            }
            Err(e) => {
                self.pasted_open = None;
                self.fail(tr(lang, "batch paste"), e);
            }
        }
    }

    /// `Msg::Models` landing. Keyed by ROLE: the two roles have their own
    /// endpoint and key, so a result only ever fills the catalogue of the
    /// role that asked for it — and by GENERATION: a completion is installed
    /// only if nothing invalidated the catalogue while it flew.
    pub(crate) fn on_models(
        &mut self,
        lang: Lang,
        role: ModelRole,
        generation: u64,
        res: anyhow::Result<Vec<autoshade::openai_models::ModelInfo>>,
    ) {
        // Single-flight per role (`fetch_models` refuses to start a second
        // while one flies), so whatever lands, THE flight for this role is
        // over — stale or not, the flag must clear or the fetch button and
        // the auto-probe stay disarmed for the rest of the session.
        self.catalogue_mut(role).fetching = false;
        // …but the ids belong to the world the fetch was LAUNCHED in. A key
        // replaced or a URL swapped mid-flight bumped `gen` (via `clear`);
        // installing this completion would resurrect the ids the OLD
        // credential could see and offer them under the new one's name.
        if generation != self.catalogue_mut(role).generation {
            self.settings.status = crate::i18n::text(lang, &logic::models::discarded_words());
            return;
        }
        match res {
            Ok(models) => {
                // The classification and the line are the logic layer's.
                let cat = logic::models::catalogue(&models, role == ModelRole::Image);
                self.settings.status = crate::i18n::text(lang, &logic::models::installed_words(&cat));
                let slot = self.catalogue_mut(role);
                slot.chat = cat.chat;
                slot.image_gen = cat.image;
                slot.efforts = cat.efforts;
                slot.vision = cat.vision;
            }
            Err(e) => {
                self.settings.status =
                    crate::i18n::text(lang, &logic::models::failed_words(&e.to_string()));
            }
        }
    }

}
