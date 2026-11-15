//! The canvas's master cache, source frame and re-develop helpers: which pixel
//! master a card hangs off, the film edge/RAW facts the develop needs, and the
//! re-develop flight. `workers.rs` keeps the message pump.

use crate::*;

impl AutoShadeApp {
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
    let edge = self.genai.preview_edge.clamp(640, 8192);
    // Cache probe first: a revisited card installs synchronously, with
    // no worker and no "loading…" status to flash.
    if let Some(arc) = self.cached_master(&origin, edge) {
        self.install_master(ctx, &photo, &origin, arc);
        return;
    }
    if !self.open.master_loads.insert((photo.clone(), origin.clone())) {
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
    crate::logic::lru::get(&mut self.open.master_cache, |(p, e, t)| {
        p == origin && *e == edge && *t == stamp
    })
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
    crate::logic::lru::insert(
        &mut self.open.master_cache,
        (origin.to_path_buf(), edge, stamp),
        img,
        |(p, e, _)| p == origin && *e == edge,
        MASTER_CACHE_CAP,
    );
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
    if self.photo.src_path.as_ref() != Some(photo) {
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
    if self.gal.thumbs.contains_key(&idx) || self.gal.thumb_requested.contains(&idx) {
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
    if self.gal.thumb_fail.get(&idx).is_some_and(|n| *n >= 3) {
        return;
    }
    if self.gal.thumb_inflight >= MAX_THUMB_INFLIGHT {
        return;
    }
    let Some(path) = self.gal.gallery.get(idx).cloned() else { return };
    self.gal.thumb_requested.insert(idx);
    self.gal.thumb_inflight += 1;
    let generation = self.gal.gallery_gen;
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
        self.session.dirty = false;
        return;
    }
    let Some(base) = self.photo.base_preview.clone() else {
        self.session.dirty = false;
        return;
    };
    // The frame of the pixels (v1.17.0): a master's own marker says
    // whether it is lens-corrected, and a recipe that develops other lens
    // geometry than the one baked into it is refused, as the export
    // refuses it — the canvas keeps its last frame and says why.
    let source = match self.source_frame().and_then(|f| f.check(&self.recipe).map(|()| f)) {
        Ok(f) => f,
        Err(e) => {
            self.session.dirty = false;
            self.status = format!("{}: {e:#}", tr(self.lang, "preview develop failed"));
            return;
        }
    };
    let recipe = self.recipe.clone();
    let show_clipping = self.chrome.show_clipping;
    let film = self.film_short_edge();
    self.develop_inflight = true;
    self.session.dirty = false;
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
    if want == self.open.source_part {
        return true;
    }
    if self.open.source_part_inflight.as_ref() == Some(&want) {
        return false;
    }
    let Some(path) = self.photo.src_path.clone() else { return true };
    let edge = self.genai.preview_edge.clamp(640, 8192);
    self.open.source_part_inflight = Some(want.clone());
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
    if let Some((cached, when, edge)) = &self.photo.film_edge
        && *cached == path
        && *when == modified
    {
        return *edge;
    }
    let edge = autoshade::decode::film_short_edge(&path);
    self.photo.film_edge = Some((path, modified, edge));
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
        .photo.base_preview
        .as_ref()
        .is_some_and(|base| Arc::ptr_eq(base, &frame.base))
        && self.recipe == frame.recipe;
    if !current {
        // The frame was solved for a recipe that changed mid-flight. The
        // redispatch gate is `dirty && !develop_inflight`, so a caller
        // that mutated the recipe WITHOUT setting dirty (historically the
        // crop path) would freeze the preview here forever — always
        // re-arm, the gate coalesces to a single fresh develop.
        self.session.dirty = true;
        ctx.request_repaint();
        return;
    }
    self.develop_count += 1;
    self.chrome.histogram = Some(frame.histogram);

    // The ColorImage arrives worker-built (see PreviewDone.after) — the UI
    // thread only uploads. The raw RGB is retained so the clipping toggle
    // can rebuild its overlay later without a redevelop.
    // A frame of the SAME photo and version re-renders in place (a slider
    // drag must not flicker); the first frame of a different one lands on
    // a fresh texture and the old one fades out over it (MOTION_FADE_S).
    let shows = self.photo.src_path.clone().map(|p| (p, self.active));
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
    let clip = if self.chrome.show_clipping {
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
    self.chrome.last_rgb = Some(frame.rgb);
    match clip {
        Some(clip) => {
            if let Some(tex) = &mut self.chrome.clip_tex {
                tex.set(clip, egui::TextureOptions::NEAREST);
            } else {
                self.chrome.clip_tex =
                    Some(ctx.load_texture("clip", clip, egui::TextureOptions::NEAREST));
            }
        }
        None => self.chrome.clip_tex = None,
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
    self.overlay.overlay_stale = true;
    ctx.request_repaint();
}
}
