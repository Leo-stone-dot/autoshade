//! The AI-look protocol, headless.
//!
//! A "look" is the one analysis the app has in flight at a time. Four VIEW
//! verbs start one — 「AI Analyze」, 「AI adjust」, Normal's one-click (a fixed
//! look or the reference photos) and the Portraits ✨ look — and they share ONE
//! protocol: a start under an epoch, a claim only the current epoch can win, and
//! a landing whose per-verb facts (line, remembered fine-tune, kept portrait
//! masks) come back as data. The panels draw the buttons; the words and the
//! decisions live here.
//!
//! WIRED IN THE NEXT SLICE: the panels still call the old starters; this
//! allowance goes away with them.
#![allow(dead_code)]
//!
//! This is the reflection layer, not a second business core: the model, the
//! request type, the presets and the WB/library rules are the library's; this
//! module maps a view verb onto them and back.
use autoshade::text::generated::*;


use super::{Effect, UiText};
use autoshade::advisor::Verdict;
use autoshade::library::{EmbeddingSwitch, DEFAULT_NAME, NORMAL_NAME};
use autoshade::pipeline::{Guide, GradeRequest, region_guidance};
use autoshade::rationale::Note;
use autoshade::recipe::{DirectionAdherence, EditRecipe, GradeStrength};
use autoshade::render::LensBlur;
use autoshade::recipe::LocalAdjustment;
use std::path::{Path, PathBuf};

/// What a one-click asks for: one of the fixed looks, or the photos in the
/// chosen reference folder. (It is a domain choice, so it lives here; the panel
/// only draws the buttons.)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum OneClick {
    Preset(&'static str),
    References,
}

/// The one-click policy's fixed numbers: a preset look is followed closely but
/// only by direction (style 0, no reference images, no deep thinking), while
/// the reference folder is followed as the library at full style and the photos
/// are shown to the model.
pub(crate) const PRESET_ADHERENCE: f32 = 0.8;
pub(crate) const REFERENCE_STYLE: f32 = 1.0;

/// The four fine-tune values as a one-click left them, so 「back to the
/// one-click result」 can put them back.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct FineTune {
    pub(crate) exposure_ev: f32,
    pub(crate) contrast: f32,
    pub(crate) temperature_k: Option<f32>,
    pub(crate) saturation: f32,
}

impl FineTune {
    pub(crate) fn of(r: &EditRecipe) -> Self {
        FineTune {
            exposure_ev: r.exposure_ev,
            contrast: r.contrast,
            temperature_k: r.temperature_k,
            saturation: r.saturation,
        }
    }

    pub(crate) fn apply(self, r: &mut EditRecipe) {
        r.exposure_ev = self.exposure_ev;
        r.contrast = self.contrast;
        r.temperature_k = self.temperature_k;
        r.saturation = self.saturation;
    }
}

/// Which view verb asked for the analysis.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum LookKind {
    Analyze,
    Adjust,
    OneClick(OneClick),
    Portrait,
}

/// The panel preferences a start reads, snapshotted on the UI thread so a
/// toggle flipped after the button cannot change what the call pays for.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LookPrefs {
    pub(crate) guide: Guide,
    pub(crate) style: f32,
    pub(crate) send_reference_image: bool,
    pub(crate) strength: GradeStrength,
    pub(crate) think: bool,
    pub(crate) adherence: DirectionAdherence,
    pub(crate) embed: EmbeddingSwitch,
}

/// What the user asked for, as facts: which verb, the typed direction, a
/// box-selected region, and (for adjust / portrait) the payload the worker
/// needs.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct LookIntent {
    pub(crate) kind: Option<LookKind>,
    pub(crate) typed: String,
    pub(crate) region: Option<[f32; 4]>,
    pub(crate) base: Option<EditRecipe>,
    pub(crate) pixels: Option<PathBuf>,
    /// A portrait look replaces the recipe whole; these come back on top of it.
    pub(crate) kept_masks: Vec<LocalAdjustment>,
    pub(crate) kept_blur: Option<LensBlur>,
}

impl LookIntent {
    pub(crate) fn of(kind: LookKind) -> Self {
        Self { kind: Some(kind), ..Default::default() }
    }
}

/// What a landing has to put back on the recipe, and what the verb remembers.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) enum Keep {
    #[default]
    None,
    OneClick,
    Portrait { masks: Vec<LocalAdjustment>, blur: Option<LensBlur> },
}

/// The analysis one start put in flight.
#[derive(Clone, Debug)]
pub(crate) struct PendingLook {
    pub(crate) kind: LookKind,
    pub(crate) photo: PathBuf,
    pub(crate) epoch: u64,
    pub(crate) keep: Keep,
}

/// Everything the worker needs, and nothing it must ask the UI for later.
#[derive(Clone, Debug)]
pub(crate) struct AnalysisJob {
    pub(crate) photo: PathBuf,
    pub(crate) pixels: Option<PathBuf>,
    pub(crate) guidance: Option<String>,
    pub(crate) base: Option<EditRecipe>,
    pub(crate) req: GradeRequest,
    /// Which library NAME the worker's scope opens (`library::with_library`).
    pub(crate) library: &'static str,
    pub(crate) epoch: u64,
}

/// A start's output: the job the facade runs, the pending look it stores, and
/// the opening effects (status line, the verb's working line).
pub(crate) struct Started {
    pub(crate) job: AnalysisJob,
    pub(crate) pending: PendingLook,
    pub(crate) effects: Vec<Effect>,
}

/// The presentation line a verb owns. The view maps these to its own line
/// enums; the words are rendered there.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum LookLine {
    Idle,
    Working,
    Done,
    /// No image key: the built-in look stood in.
    BuiltIn,
    Looked,
    Failed,
}

/// The state the protocol needs: at most one look in flight.
#[derive(Default)]
pub(crate) struct LookState {
    pub(crate) pending: Option<PendingLook>,
}

/// A result's claim on the pending look.
#[derive(Clone, Debug)]
pub(crate) struct Claim {
    pub(crate) kind: LookKind,
    pub(crate) photo: PathBuf,
    pub(crate) keep: Keep,
    /// The built-in look stood in for the model (a rationale note says so).
    pub(crate) built_in: bool,
}

/// Plan one start: guard, request, payload, pending facts and the opening
/// lines. `None` means the verb cannot start (no photo, or an analysis is
/// already in flight) — the same refusal the panels grey their buttons for.
pub(crate) fn plan(
    prefs: &LookPrefs,
    intent: LookIntent,
    photo: Option<&Path>,
    busy: bool,
    inflight: bool,
    epoch: u64,
) -> Option<Started> {
    let kind = intent.kind.clone()?;
    let photo = photo?.to_path_buf();
    if busy || inflight {
        return None;
    }
    let adjusting = matches!(kind, LookKind::Adjust);
    // A box-selected region is a LOCAL direction, so it guides by direction
    // whatever the panel's switch says; an adjust is guided by its request.
    let region_guided = intent.region.is_some();
    let guide = if adjusting || region_guided { Guide::Direction } else { prefs.guide };
    let req = GradeRequest {
        guide,
        style: prefs.style,
        send_reference_image: prefs.send_reference_image,
        strength: prefs.strength,
        think: prefs.think,
        adherence: prefs.adherence,
        embed: prefs.embed,
    };
    let typed = intent.typed.trim().to_string();
    let guidance = match intent.region {
        Some([l, t, r, b]) => Some(region_guidance(l, t, r, b, &typed)),
        None => (guide == Guide::Direction && !typed.is_empty()).then(|| typed.clone()),
    };
    let (req, guidance, library, keep, status, line) = match &kind {
        LookKind::Analyze => (
            req,
            guidance,
            DEFAULT_NAME,
            Keep::None,
            UiText::of(K_LOGIC_ANALYZING_WITH_THE_CONFIGURED_AI_SERVICE),
            None,
        ),
        LookKind::Adjust => (
            req,
            guidance,
            DEFAULT_NAME,
            Keep::None,
            UiText::of(K_LOGIC_ADJUSTING_THE_SELECTED_VARIANT_WITH_AI),
            None,
        ),
        LookKind::OneClick(OneClick::Preset(id)) => (
            GradeRequest {
                guide: Guide::Direction,
                style: 0.0,
                send_reference_image: false,
                strength: prefs.strength,
                think: false,
                adherence: DirectionAdherence::new(PRESET_ADHERENCE),
                embed: EmbeddingSwitch::OFF,
            },
            autoshade::advisor::presets::preset_guidance(id).map(str::to_owned),
            NORMAL_NAME,
            Keep::OneClick,
            UiText::of(K_LOGIC_MAKING_THE_LOOK),
            Some(LookLine::Working),
        ),
        LookKind::OneClick(OneClick::References) => (
            GradeRequest {
                guide: Guide::Library,
                style: REFERENCE_STYLE,
                send_reference_image: true,
                strength: prefs.strength,
                think: false,
                adherence: DirectionAdherence::default(),
                // The folder was just read by the same model, so its download
                // is done; the environment override still wins.
                embed: EmbeddingSwitch::resolve(None, true),
            },
            None,
            NORMAL_NAME,
            Keep::OneClick,
            UiText::of(K_LOGIC_MAKING_THE_LOOK),
            Some(LookLine::Working),
        ),
        LookKind::Portrait => (
            GradeRequest {
                guide: Guide::Direction,
                style: 0.0,
                send_reference_image: false,
                strength: prefs.strength,
                think: false,
                adherence: DirectionAdherence::new(PRESET_ADHERENCE),
                embed: EmbeddingSwitch::OFF,
            },
            autoshade::advisor::presets::preset_guidance("portrait").map(str::to_owned),
            DEFAULT_NAME,
            Keep::Portrait {
                masks: intent.kept_masks.clone(),
                blur: intent.kept_blur.clone(),
            },
            UiText::of(K_LOGIC_MAKING_THE_PORTRAIT_LOOK),
            Some(LookLine::Working),
        ),
    };
    let job = AnalysisJob {
        photo: photo.clone(),
        pixels: intent.pixels,
        guidance,
        base: intent.base,
        req,
        library,
        epoch,
    };
    let mut effects = vec![Effect::Status(status)];
    if let Some(line) = line {
        effects.push(Effect::LookLine { kind: kind.clone(), line });
    }
    Some(Started {
        job,
        pending: PendingLook { kind, photo, epoch, keep },
        effects,
    })
}

/// Claim a finished analysis for the pending look, if it is this one's and it
/// is still current. A result under an old epoch is DISCARDED: its Ok arm
/// would otherwise write a develop over the photo the user has moved on to.
pub(crate) fn claim(
    state: &mut LookState,
    epoch: u64,
    gen_epoch: u64,
    res: &anyhow::Result<(EditRecipe, Verdict, Vec<Note>)>,
) -> Option<Claim> {
    let pending = state.pending.as_ref()?;
    if pending.epoch != epoch {
        return None;
    }
    let pending = state.pending.take()?;
    if epoch != gen_epoch {
        return None;
    }
    let built_in = res.as_ref().is_ok_and(|(_, _, notes)| {
        notes.iter().any(|n| {
            matches!(
                n.key,
                autoshade::rationale::keys::HEURISTIC_NO_KEY
                    | autoshade::rationale::keys::HEURISTIC_UNAVAILABLE
            )
        })
    });
    Some(Claim { kind: pending.kind, photo: pending.photo, keep: pending.keep, built_in })
}

/// Before the landed recipe is written: put back what a fresh look replaced.
pub(crate) fn restore(claim: &Claim, recipe: &mut EditRecipe) {
    if let Keep::Portrait { masks, blur } = &claim.keep {
        autoshade::face::restore_portrait_masks(recipe, masks.clone(), blur.clone());
    }
}

/// After the landed recipe is written: the verb's own line and what it
/// remembers. `ok` is the shared landing's outcome.
pub(crate) fn landed(claim: &Claim, recipe: &EditRecipe, ok: bool) -> Vec<Effect> {
    let mut effects = Vec::new();
    match &claim.kind {
        LookKind::Analyze | LookKind::Adjust => {}
        LookKind::OneClick(_) => {
            if ok {
                effects.push(Effect::LookLine {
                    kind: claim.kind.clone(),
                    line: if claim.built_in { LookLine::BuiltIn } else { LookLine::Done },
                });
                effects.push(Effect::RememberOneClick {
                    photo: claim.photo.clone(),
                    fine: FineTune::of(recipe),
                });
            } else {
                effects.push(Effect::LookLine { kind: claim.kind.clone(), line: LookLine::Failed });
            }
        }
        LookKind::Portrait => {
            effects.push(Effect::LookLine {
                kind: claim.kind.clone(),
                line: if ok { LookLine::Looked } else { LookLine::Failed },
            });
        }
    }
    effects
}

/// 「Match my reference photos…」: read the picked folder into Normal's own
/// library, then hand over to the reference look. Refused while another build
/// or analysis runs, or with no photo open.
pub(crate) fn references_start(
    photo: Option<&Path>,
    dir: PathBuf,
    busy: bool,
    inflight: bool,
    building: bool,
) -> Vec<Effect> {
    if busy || inflight || building {
        return Vec::new();
    }
    let Some(photo) = photo else { return Vec::new() };
    vec![
        Effect::Status(UiText::of(K_LOGIC_READING_YOUR_REFERENCE_PHOTOS)),
        Effect::LookLine { kind: LookKind::OneClick(OneClick::References), line: LookLine::Idle },
        Effect::ReferenceBuild { photo: photo.to_path_buf(), dir },
    ]
}

/// The folder build's landing: a built library hands straight over to the
/// reference look for the photo it was started for; anything else is said on
/// the line (and, for a read failure, as an error toast) instead of dropped.
pub(crate) fn references_built(
    res: &Result<usize, String>,
    built_photo: Option<&Path>,
    current_photo: Option<&Path>,
    inflight: bool,
) -> Vec<Effect> {
    let kind = LookKind::OneClick(OneClick::References);
    match res {
        Ok(_) if built_photo.is_some() && built_photo == current_photo && !inflight => {
            vec![Effect::StartLook(Box::new(LookIntent::of(kind)))]
        }
        Ok(_) => vec![
            // Status FIRST: the failure line shows the status the landing wrote.
            Effect::Status(UiText::of(
                K_LOGIC_YOUR_REFERENCE_PHOTOS_ARE_READ_BUT_THE_LOOK_COUL,
            )),
            Effect::LookLine { kind, line: LookLine::Failed },
        ],
        Err(err) => {
            let text = UiText::with(K_LOGIC_COULD_NOT_READ_YOUR_REFERENCE_PHOTOS, &[("err", err)]);
            vec![
                Effect::Status(text.clone()),
                Effect::LookLine { kind, line: LookLine::Failed },
                Effect::ToastError(text),
            ]
        }
    }
}

/// The ✕ or a cancelled start lets go of the pending look; a late result then
/// has no claim to win.
pub(crate) fn cancel(state: &mut LookState) {
    state.pending = None;
}
