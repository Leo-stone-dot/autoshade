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
                    self.exp.batch_progress = None; // the bar belongs to ONE batch run
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
                                self.exp.last_dir = Some(dest.clone());
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
                    self.exp.batch_progress = None;
                    self.normal.delivering = false;
                    self.fail(tr(lang, "export failed"), e);
                }
                Msg::BatchProgress { done, total } => {
                    self.exp.batch_progress = Some((done, total));
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
                    self.ai.style_info_loading = false;
                    self.ai.style_info = Some(*info);
                }
                Msg::StyleBuildProgress { stage, done, total } => {
                    self.ai.style_build_progress = Some((stage, done, total));
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
                    self.gal.edited_badge.clear(); // imported sidecars light ● badges
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
            || self.gal.thumb_inflight > 0
            || self.settings.image_models.fetching
            || self.settings.analysis_models.fetching
            || !self.master_loads.is_empty()
            // R23-2: the style-library build is NOT busy-gated (it runs for
            // minutes and the app stays usable), so without it here the
            // "building…" state would only repaint when a progress tick
            // happens to arrive — a single slow 60 MP decode would freeze the
            // counter for as long as it takes. Same reason the two model
            // fetches are on this list.
            || self.ai.style_build_inflight
            || self.ai.style_info_loading
        {
            ctx.request_repaint_after(Duration::from_millis(100));
        }
    }

    }
