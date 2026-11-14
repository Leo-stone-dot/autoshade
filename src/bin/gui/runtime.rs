//! The facade between the view and the headless logic.
//!
//! The view draws a frame and returns [`Action`]s; this module hands them to
//! `logic::*`, then executes the [`Effect`]s that come back. Nothing in the
//! view mutates a recipe, spawns a worker or renders a logic-authored line —
//! those three crossings live here, on purpose.

use crate::logic::look::{AnalysisJob, LookIntent, LookKind, LookLine, OneClick};
use crate::logic::{self, Action, Effect};
use crate::*;

impl AutoShadeApp {
    /// Apply what the view saw the user do. One arm per domain; each arm ends
    /// in `logic::<domain>`, and the effects run at the end, in the order the
    /// logic asked for them.
    pub(crate) fn apply_actions(&mut self, actions: Vec<Action>) {
        let mut effects = Vec::new();
        for action in actions {
            match action {
                Action::Look(intent) => self.start_look(intent),
                Action::References(dir) => {
                    let photo = self.src_path.clone();
                    let effects = logic::look::references_start(
                        photo.as_deref(),
                        dir,
                        self.busy,
                        self.analyze_inflight,
                        self.normal.building,
                    );
                    self.run_effects(effects);
                }
                Action::ToggleLensBlur(_)
                | Action::SetBlurAmount(_)
                | Action::SetFocusRange(_)
                | Action::RemoveLensBlur
                | Action::SetFocusPicking(_)
                | Action::FocusPicked(_)
                | Action::EstimateDepth => {
                    let start = match action {
                        Action::EstimateDepth => self
                            .src_path
                            .clone()
                            .zip(self.original_frame_base())
                            .map(|(photo, base)| (photo, base)),
                        _ => None,
                    };
                    effects.extend(logic::lens_blur::apply(
                        &mut self.recipe,
                        action,
                        start,
                        self.busy,
                    ));
                }
            }
        }
        self.run_effects(effects);
    }

    /// Execute the effects the logic asked for: worker spawns, lines, flags.
    pub(crate) fn run_effects(&mut self, effects: Vec<Effect>) {
        for effect in effects {
            match effect {
                Effect::EstimateDepth { photo, base } => self.spawn_depth_worker(photo, base),
                Effect::Status(text) => self.status = crate::i18n::text(self.lang, &text),
                Effect::ToastError(text) => {
                    self.toast(ToastKind::Error, crate::i18n::text(self.lang, &text));
                }
                Effect::LookLine { kind, line } => self.apply_look_line(kind, line),
                Effect::RememberOneClick { photo, fine } => {
                    self.normal.result = Some((photo, fine));
                }
                Effect::StartLook(intent) => self.start_look(intent),
                Effect::ReferenceBuild { photo, dir } => self.start_reference_build(photo, dir),
                Effect::ArmFocusPick(on) => self.lens_focus_picking = on,
                Effect::DisarmTools => self.disarm_tools(),
                Effect::MarkDirty => self.dirty = true,
            }
        }
    }

    /// `Msg::DepthEstimated`'s landing: the wire is free either way, so the
    /// in-flight flag clears here; the recipe change and its words are the
    /// logic's.
    pub(crate) fn on_depth_estimated(&mut self, res: anyhow::Result<std::path::PathBuf>) {
        self.busy = false;
        let effects = logic::lens_blur::landed(&mut self.recipe, res);
        self.run_effects(effects);
    }




    /// Read the picked folder into Normal's reference library, for the photo
    /// that was open then. Progress and the built hand-off arrive as messages;
    /// the decisions are the logic layer's.
    fn start_reference_build(&mut self, photo: std::path::PathBuf, dir: std::path::PathBuf) {
        self.busy = true;
        self.normal.build_photo = Some(photo);
        self.normal.building = true;
        self.normal.build_progress = None;
        self.normal.ref_dir = Some(dir.clone());
        let tx = self.tx.clone();
        let ctx = self.egui_ctx.clone();
        self.spawn_worker(
            move || {
                let progress = |p: autoshade::library::BuildProgress| {
                    let _ = tx.send(Msg::OneClickBuildProgress { done: p.done, total: p.total });
                    ctx.request_repaint();
                };
                let sidecars = autoshade::library::Sidecars::from_config(&autoshade::config::Config::load());
                let built = autoshade::library::rebuild_named(
                    autoshade::library::NORMAL_NAME,
                    &dir,
                    &sidecars,
                    autoshade::library::DescribeSwitch::OFF,
                    None,
                    &progress,
                );
                Msg::OneClickBuilt(built.map(|lib| lib.entries.len()).map_err(|e| format!("{e:#}")))
            },
            |e| Msg::OneClickBuilt(Err(e.to_string())),
        );
    }

    /// `Msg::OneClickBuilt`'s landing: free the wire, then let the logic decide
    /// whether this build hands over to the reference look for the photo it was
    /// started for.
    pub(crate) fn on_one_click_built(&mut self, _lang: Lang, res: Result<usize, String>) {
        self.busy = false;
        self.normal.building = false;
        self.normal.build_progress = None;
        let for_photo = self.normal.build_photo.take();
        let effects = logic::look::references_built(
            &res,
            for_photo.as_deref(),
            self.src_path.as_deref(),
            self.analyze_inflight,
        );
        self.run_effects(effects);
    }

    /// Plan and start one AI look: snapshot the panel prefs on this thread, let
    /// `logic::look::plan` decide, store the pending look and run the worker.
    fn start_look(&mut self, intent: LookIntent) {
        let prefs = logic::look::LookPrefs {
            guide: self.guide,
            style: self.style_strength,
            send_reference_image: self.send_style_ref_image,
            strength: self.analysis_strength(),
            think: self.deep_think,
            adherence: autoshade::recipe::DirectionAdherence::new(self.direction_adherence),
            embed: autoshade::library::EmbeddingSwitch::resolve(None, self.style_embed),
        };
        let photo = self.src_path.clone();
        let Some(started) = logic::look::plan(
            &prefs,
            intent,
            photo.as_deref(),
            self.busy,
            self.analyze_inflight,
            self.gen_epoch,
        ) else {
            return;
        };
        self.look.pending = Some(started.pending);
        self.run_effects(started.effects);
        self.spawn_analysis(started.job);
    }

    /// Run one planned analysis. The job's epoch is the one `plan` read from
    /// the app when the verb was pressed; the worker arms the cancel for it.
    fn spawn_analysis(&mut self, job: AnalysisJob) {
        debug_assert_eq!(job.epoch, self.gen_epoch, "a planned job starts under the current epoch");
        self.busy = true;
        self.spawn_analyze(job.photo, job.pixels, job.guidance, job.base, job.req, job.library);
    }

    /// Map a look's presentation line onto the panel that owns it. The wording
    /// for a failure is the status the shared landing already wrote; the line
    /// only carries the fact.
    fn apply_look_line(&mut self, kind: LookKind, line: LookLine) {
        match kind {
            LookKind::OneClick(OneClick::Preset(_)) | LookKind::OneClick(OneClick::References) => {
                self.normal.line = match line {
                    LookLine::Idle => crate::panels::normal::NormalLine::Idle,
                    LookLine::Working => crate::panels::normal::NormalLine::Working,
                    LookLine::Done => crate::panels::normal::NormalLine::Done,
                    LookLine::BuiltIn => {
                        self.normal.key_present = Some(false);
                        crate::panels::normal::NormalLine::BuiltIn
                    }
                    LookLine::Failed => crate::panels::normal::NormalLine::Failed(self.status.clone()),
                    LookLine::Looked => return,
                };
                // A landed one-click says its own plain sentence instead of the
                // shared landing's (and in the toast that landing raised).
                if matches!(line, LookLine::Done | LookLine::BuiltIn) {
                    let plain = self.normal_line_text(self.lang).to_string();
                    self.reword_landing(plain);
                }
            }
            LookKind::Portrait => {
                self.portrait.line = match line {
                    LookLine::Idle => crate::panels::portrait::PortraitLine::Idle,
                    LookLine::Working => crate::panels::portrait::PortraitLine::Looking,
                    LookLine::Looked => crate::panels::portrait::PortraitLine::Looked,
                    LookLine::Failed => crate::panels::portrait::PortraitLine::Failed(self.status.clone()),
                    LookLine::Done | LookLine::BuiltIn => return,
                };
            }
            LookKind::Analyze | LookKind::Adjust => {}
        }
    }


    /// Render one landing outcome's words: the one place a logic-authored
    /// status line and toast become the view's strings.
    pub(crate) fn show_words(&mut self, lang: Lang, words: logic::analysis_landing::Words) -> String {
        let status = crate::i18n::text(lang, &words.status);
        self.status = status.clone();
        if let Some((tone, text)) = words.toast {
            let kind = match tone {
                logic::analysis_landing::Tone::Error => ToastKind::Error,
                logic::analysis_landing::Tone::Success => ToastKind::Success,
            };
            self.toast(kind, crate::i18n::text(lang, &text));
        }
        status
    }

    fn spawn_depth_worker(
        &mut self,
        photo: std::path::PathBuf,
        base: std::sync::Arc<image::DynamicImage>,
    ) {
        self.busy = true;
        self.spawn_worker(
            move || Msg::DepthEstimated(logic::lens_blur::estimate_depth(&photo, &base)),
            |e| Msg::DepthEstimated(Err(e)),
        );
    }
}
