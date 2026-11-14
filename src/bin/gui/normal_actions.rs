//! Normal's business: the Save group's batch hand-off and landing words, and
//! the noise switch's card dance (the ◈ card, its look history, the undo
//! baseline).
//!
//! The panel draws and emits verbs; this app layer decides.

use crate::panels::normal::DenoisePair;
use crate::*;

/// A folder by its own name (its full path is one hover away).
pub(crate) fn folder_name(p: &std::path::Path) -> String {
    p.file_name().map_or_else(|| abs_display(p), |n| n.to_string_lossy().into_owned())
}

impl AutoShadeApp {

    /// The paste half of 「apply to every photo」 landed: render them all into
    /// `dest`, or leave the paste's own error standing.
    pub(crate) fn after_apply_all_paste(&mut self, lang: Lang, dest: PathBuf, pasted: bool) {
        if !pasted {
            return;
        }
        // The paste's own sentence is the workspace's; the render's status
        // replaces it below.
        let old = self.status.clone();
        self.toasts.retain(|t| t.text != old);
        let targets = self.gallery.clone();
        let export = self.normal_export_opts();
        self.start_batch_render_for(targets, "jpg".into(), export, dest, true);
        if self.busy {
            self.normal.delivering = true;
            self.status = tr(lang, "Saving every photo in the folder…").into();
        }
    }

    /// The Save group's landing in plain words — `None` when no Save-group
    /// delivery is running, or a batch had failures (their error stands).
    pub(crate) fn normal_delivery_words(&mut self, lang: Lang, outcome: &ExportOutcome) -> Option<String> {
        if !std::mem::take(&mut self.normal.delivering) {
            return None;
        }
        match outcome {
            // The file actually written: a numbered save names its number.
            ExportOutcome::Single { out, .. } => {
                let dir = out.parent().map(folder_name).unwrap_or_default();
                let file = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
                Some(trf(lang, "Saved {file} to {folder}", &[("file", &file), ("folder", &dir)]))
            }
            ExportOutcome::Batch { ok, errs, dest, renamed, .. } if errs.is_empty() => {
                let saved = trf(lang, "{n} photos saved to {folder}", &[("n", &ok.to_string()), ("folder", &folder_name(dest))]);
                Some(if renamed.is_empty() {
                    saved
                } else {
                    let k = renamed.len().to_string();
                    let numbered = trf(lang, "{k} of them have a number added to the name, so no earlier photo was replaced.", &[("k", &k)]);
                    format!("{saved}. {numbered}")
                })
            }
            // Every failure was the refusal to save into the photos' own
            // folder (counted by type in the worker): the plain sentence.
            ExportOutcome::Batch { .. } => None,
        }
    }

    /// Is the noise switch on: the card on screen is a ◈ card, or the
    /// switch's own AI denoise is running.
    pub(crate) fn normal_denoise_on(&self) -> bool {
        self.active_variant().is_some_and(|v| v.kind == VariantKind::Denoised)
            || (self.busy
                && self
                    .normal
                    .denoise_pair
                    .as_ref()
                    .is_some_and(|d| d.denoised.is_none() && self.src_path.as_ref() == Some(&d.photo)))
    }

    /// The strip index of the card with `id` (an empty id matches nothing).
    fn card_index(&self, id: &str) -> Option<usize> {
        (!id.is_empty()).then(|| self.variants.iter().position(|v| v.id == id)).flatten()
    }

    /// Put card `idx` under the canvas with the look on screen: the switch
    /// changes the grain, never the look the sliders made — nor the look's
    /// undo history.
    fn show_card_keeping_look(&mut self, idx: usize) {
        let look = self.recipe.clone();
        let history = self.take_look_history();
        let ctx = self.egui_ctx.clone();
        self.switch_variant(idx, &ctx);
        if self.active == idx {
            self.recipe = look;
            self.dirty = true;
            self.carry_look_history(history);
        }
    }

    /// The look's undo history, before the noise switch changes the card
    /// under it (a card change resets the history — `load_active`). The
    /// look on screen is committed first, so it is the head.
    pub(crate) fn take_look_history(&mut self) -> (Vec<UndoStep>, Vec<UndoStep>) {
        self.commit_now();
        (self.undo_stack.clone(), self.redo_stack.clone())
    }

    /// …and back, on the card now on screen: every step keeps its look and
    /// takes this card's pixels (the switch changes the grain, never the
    /// look), and the look on screen is the head.
    pub(crate) fn carry_look_history(&mut self, (undo, redo): (Vec<UndoStep>, Vec<UndoStep>)) {
        let here = self.current_step();
        let onto = |s: UndoStep| UndoStep { recipe: s.recipe, base: here.base.clone(), origin: here.origin.clone(), keep_skin: s.keep_skin };
        self.undo_stack = undo.into_iter().map(onto).collect();
        self.redo_stack = redo.into_iter().map(onto).collect();
        self.committed = here;
    }

    /// The noise switch. On: the Retouch panel's own AI denoise
    /// (`start_ai_denoise` — the same worker, model and strength; its ◈ card
    /// lands on the canvas), or — when this card was denoised already — that
    /// ◈ card again, with no second run. Off: back to the card it was made
    /// from (the first card that is not a ◈ card when the switch did not
    /// make it).
    pub(crate) fn set_normal_denoise(&mut self, on: bool) {
        let Some(photo) = self.src_path.clone() else { return };
        if self.busy || on == self.normal_denoise_on() {
            return;
        }
        let Some(active) = self.active_variant().map(|v| v.id.clone()) else { return };
        let pair = self.normal.denoise_pair.clone().filter(|d| d.photo == photo);
        let lang = self.lang;
        if on {
            let made = pair.filter(|d| d.plain == active).and_then(|d| d.denoised).and_then(|id| self.card_index(&id));
            if let Some(idx) = made {
                self.show_card_keeping_look(idx);
                self.status = tr(lang, "AI denoise on").into();
                return;
            }
            self.normal.denoise_pair = Some(DenoisePair { photo, plain: active, denoised: None });
            self.start_ai_denoise();
            if self.busy {
                self.status = tr(lang, "Removing the grain… this takes a moment.").into();
            } else {
                // Refused before it ran: its own line stands.
                self.normal.denoise_pair = None;
            }
        } else {
            let back = pair
                .filter(|d| d.denoised.as_deref() == Some(active.as_str()))
                .and_then(|d| self.card_index(&d.plain))
                .or_else(|| self.variants.iter().position(|v| v.kind != VariantKind::Denoised));
            let Some(idx) = back else { return };
            // Remembered, so turning it on again shows this ◈ card.
            self.normal.denoise_pair = Some(DenoisePair { photo, plain: self.variants[idx].id.clone(), denoised: Some(active) });
            self.show_card_keeping_look(idx);
            self.status = tr(lang, "AI denoise off").into();
        }
    }

    /// Before a retouch lands (`Msg::Retouched`): the look's undo history,
    /// when the landing may be the noise switch's own ◈ card.
    pub(crate) fn normal_denoise_history(&mut self) -> Option<(Vec<UndoStep>, Vec<UndoStep>)> {
        self.normal
            .denoise_pair
            .as_ref()
            .is_some_and(|d| d.denoised.is_none() && self.src_path.as_ref() == Some(&d.photo))
            .then(|| self.take_look_history())
    }

    /// After a retouch landed (`Msg::Retouched`): when it was the noise
    /// switch's AI denoise, remember its ◈ card, carry the look's undo
    /// history onto it, and say so plainly.
    pub(crate) fn after_normal_denoise(&mut self, lang: Lang, history: Option<(Vec<UndoStep>, Vec<UndoStep>)>) {
        let Some(d) = self.normal.denoise_pair.as_mut() else { return };
        if d.denoised.is_some() || self.src_path.as_ref() != Some(&d.photo) {
            return;
        }
        let Some(v) = self.variants.get(self.active).filter(|v| v.kind == VariantKind::Denoised && v.id != d.plain) else {
            return;
        };
        d.denoised = Some(v.id.clone());
        if let Some(h) = history {
            self.carry_look_history(h);
        }
        self.reword_landing(tr(lang, "AI denoise done. Turn the switch off to see the photo without it.").into());
    }
}
