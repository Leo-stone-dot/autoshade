//! The facade between the view and the headless logic.
//!
//! The view draws a frame and returns [`Action`]s; this module hands them to
//! `logic::*`, then executes the [`Effect`]s that come back. Nothing in the
//! view mutates a recipe, spawns a worker or renders a logic-authored line —
//! those three crossings live here, on purpose.

use crate::logic::look::{LookKind, LookLine, OneClick};
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


    /// Map a look's presentation line onto the panel that owns it. The wording
    /// for a failure is the status the shared landing already wrote; the line
    /// only carries the fact.
    fn apply_look_line(&mut self, kind: LookKind, line: LookLine) {
        match kind {
            LookKind::OneClick(OneClick::Preset(_)) | LookKind::OneClick(OneClick::References) => {
                self.normal.line = match line {
                    LookLine::Working => crate::panels::normal::NormalLine::Working,
                    LookLine::Done => crate::panels::normal::NormalLine::Done,
                    LookLine::BuiltIn => {
                        self.normal.key_present = Some(false);
                        crate::panels::normal::NormalLine::BuiltIn
                    }
                    LookLine::Failed => crate::panels::normal::NormalLine::Failed(self.status.clone()),
                    LookLine::Looked => return,
                };
            }
            LookKind::Portrait => {
                self.portrait.line = match line {
                    LookLine::Working => crate::panels::portrait::PortraitLine::Looking,
                    LookLine::Looked => crate::panels::portrait::PortraitLine::Looked,
                    LookLine::Failed => crate::panels::portrait::PortraitLine::Failed(self.status.clone()),
                    LookLine::Done | LookLine::BuiltIn => return,
                };
            }
            LookKind::Analyze | LookKind::Adjust => {}
        }
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
