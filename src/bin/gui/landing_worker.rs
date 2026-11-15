//! The worker landings that are not about the base canvas: the style build,
//! segmentation, mask refine, folder scan, master load, retouch, paste, models
//! and the reverse-fit. `workers.rs` keeps the message pump and worker starts.

use crate::*;

impl AutoShadeApp {
pub(crate) fn on_style_built(&mut self, lang: Lang, outcome: StyleBuildOutcome) {
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

pub(crate) fn on_segmented(&mut self, lang: Lang, res: anyhow::Result<(String, PathBuf, String)>) {
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

pub(crate) fn on_mask_refined(&mut self, lang: Lang, res: anyhow::Result<MaskRefineOutcome>) {
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

pub(crate) fn on_folder(&mut self, lang: Lang, res: anyhow::Result<(PathBuf, Vec<PathBuf>, usize)>) {
            match res {
                Ok((dir, list, skipped)) => {
                    let n = list.len();
                    self.gal.gallery = list;
                    self.gal.gallery_dir = Some(dir);
                    self.gal.gallery_gen += 1; // invalidate any in-flight old thumbs
                    self.gal.thumbs.clear();
                    self.gal.thumb_requested.clear();
                    // Keyed by gallery INDEX, so it is meaningless across
                    // folders: without this, index 0 failing three times
                    // here silently denied index 0 a thumbnail in every
                    // folder opened afterwards.
                    self.gal.thumb_fail.clear();
                    self.gal.thumb_inflight = 0;
                    self.gal.edited_badge.clear(); // badge stats belong to the old folder
                    self.gal.selected = None;
                    self.gal.multi_sel.clear(); // indices belong to the old folder
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

pub(crate) fn on_master_loaded(&mut self, ctx: &egui::Context, lang: Lang, photo: PathBuf, origin: PathBuf, edge: u32, stamp: FileStamp, img: anyhow::Result<image::DynamicImage>) {
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

pub(crate) fn render_retouch_note(lang: Lang, n: &RetouchNote) -> String {
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

pub(crate) fn on_fitted(&mut self, ctx: &egui::Context, lang: Lang, boxed: Box<anyhow::Result<FitOutcome>>) {
            match *boxed {
                // Either way the worker may have persisted a recipe.json
                // (an Err can land after that write) — recompute badges.
                Ok(out) => {
                    self.gal.edited_badge.clear();
                    // Advance the ● baseline ONLY when the store actually
                    // holds the fit. A backup-gate refusal means nothing
                    // was written: leaving the baseline alone keeps
                    // ● unsaved lit and lets nav_stash protect the fit —
                    // the ordinary unsaved-edit path takes over.
                    if out.persisted {
                        self.hist.saved_recipe = out.recipe.clone();
                        self.nav_stash.remove(
                            &self.src_path.clone().unwrap_or_default(),
                        );
                        // The pixel link the worker committed beside the
                        // recipe — the negative's in-place master when
                        // the ▣ card carries one (the ◭ card below hangs
                        // off it), cleared otherwise (a fit is
                        // source-based) — so the per-frame ● pixel
                        // comparison's disk mirror stays in step with it.
                        self.hist.pixels_on_disk = out.negative.clone();
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
                    self.gal.edited_badge.clear();
                    // The pre-fit snapshot may exist even when the fit
                    // errored — keep the Versions list truthful.
                    self.refresh_versions();
                    self.fail(tr(lang, "Reverse-fit failed"), e);
                }
            }
}

pub(crate) fn on_pasted(&mut self, lang: Lang, res: anyhow::Result<PasteOutcome>) {
    // Sidecars were written (possibly partially on error) — recompute the
    // gallery badges either way.
    self.gal.edited_badge.clear();
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
                self.hist.saved_recipe = r;
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
