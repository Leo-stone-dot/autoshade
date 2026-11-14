//! Headless application logic: no widget layer lives here.
//!
//! Every function takes data and returns data — the recipe it may change, plus
//! the [`Effect`]s the caller has to run. The view (`panels/`, `canvas.rs`,
//! `app.rs`) draws a frame and emits [`Action`]s; the facade (`runtime.rs`)
//! applies those actions here and executes the returned effects. User-facing
//! wording leaves this layer as [`UiText`] — an i18n key plus its named
//! arguments, not a rendered sentence.
//!
//! The rules are pinned by `tests/architecture.rs`: nothing under `logic/`
//! mentions the widget library, renders text, spawns workers, opens dialogs or
//! touches the filesystem; and a migrated panel does not mutate business state
//! or reach past the seam.

pub(crate) mod ai;
pub(crate) mod analysis_landing;
pub(crate) mod crop;
pub(crate) mod folder;
pub(crate) mod gallery;
pub(crate) mod lens_blur;
pub(crate) mod mask_edit;
pub(crate) mod normal;
pub(crate) mod mask_place;
pub(crate) mod pick;
pub(crate) mod recipes;
pub(crate) mod settings;
pub(crate) mod style;
pub(crate) mod region;
pub(crate) mod look;

use std::path::PathBuf;
use std::sync::Arc;

/// What the user did. Emitted by the view, applied by the facade.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum Action {
    /// Start the AI-look protocol for this verb (`logic::look::plan`).
    Look(look::LookIntent),
    /// Read this folder as the reference library, then look at the photo that
    /// was open when it was picked.
    References(PathBuf),
    /// The Lens Blur section.
    Lens(lens_blur::LensAction),
    /// The library gallery.
    Gallery(gallery::GalleryAction),
    /// An on-canvas eyedropper sample.
    Pick(pick::PickAction),
    /// A crop gesture (preset pick, handle drag, rotate-straighten).
    Crop(crop::CropAction),
    /// A knob drag on the selected mask's geometry.
    MaskEdit(mask_edit::MaskEditAction),
    /// A box-selected AI region.
    Region(region::RegionAction),
    /// The recipe-library window's verbs.
    Recipe(recipes::RecipeAction),
    /// The AI panel's folder verbs.
    Ai(ai::AiAction),
    /// The settings panel's folder verbs.
    Settings(settings::SettingsAction),
    /// The Normal panel's Save group.
    Normal(normal::NormalAction),
}

/// The tone of a toast a landing asks for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Tone {
    Error,
    Success,
}

/// One landing's user-facing outcome: the status line, and a toast when the
/// outcome needs to interrupt.
pub(crate) struct Words {
    pub(crate) status: UiText,
    pub(crate) toast: Option<(Tone, UiText)>,
}

/// One named argument of a [`UiText`]: literal text, or a catalogue key that
/// the renderer translates (a verdict word, say) — the logic layer carries the
/// KEY, never the rendered word.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Arg {
    Key(&'static str),
    Text(String),
}

/// One line the caller should show: the i18n key plus its named arguments.
/// Rendering lives outside this layer (`i18n::text`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiText {
    pub(crate) key: &'static str,
    pub(crate) args: Vec<(&'static str, Arg)>,
}

impl UiText {
    pub(crate) fn of(key: &'static str) -> Self {
        Self { key, args: Vec::new() }
    }

    /// A key with named arguments (`{name}` placeholders, as `trf` takes them).
    pub(crate) fn with(key: &'static str, args: &[(&'static str, &str)]) -> Self {
        Self {
            key,
            args: args.iter().map(|(name, value)| (*name, Arg::Text((*value).to_owned()))).collect(),
        }
    }

    /// A key with named arguments, some of which are catalogue keys to
    /// translate at render time.
    pub(crate) fn with_args(key: &'static str, args: &[(&'static str, Arg)]) -> Self {
        Self { key, args: args.to_vec() }
    }
}

/// Something the caller must do FOR the logic — the effects that need a
/// thread, a frame or the app's own state, kept out of the functions above so
/// they stay synchronous and testable.
#[derive(Debug)]
pub(crate) enum Effect {
    /// The line a view verb owns (the panel maps it to its own line enum).
    LookLine { kind: look::LookKind, line: look::LookLine },
    /// A one-click landed: remember the values 「back to the one-click result」
    /// restores for this photo.
    RememberOneClick { photo: PathBuf, fine: look::FineTune },
    /// Hand a started verb over to the look protocol (the reference-folder
    /// build's landing starts the look it built for).
    StartLook(look::LookIntent),
    /// Read this folder into Normal's reference library, for this photo.
    ReferenceBuild { photo: PathBuf, dir: PathBuf },
    /// Run the depth sidecar over this photo's original-frame base.
    EstimateDepth { photo: PathBuf, base: Arc<image::DynamicImage> },
    Status(UiText),
    /// An error toast. The only tone the logic raises so far — a success tone
    /// is a variant away when a domain needs one.
    ToastError(UiText),
    ArmFocusPick(bool),
    /// Put one armed eyedropper (WB / range / point colour) down.
    DisarmPicker(pick::Picker),
    /// The AI region box (the logic layer decided and worded it).
    SetRegion(Option<[f32; 4]>),
    /// One canvas tool at a time: every arming site disarms the rest.
    DisarmTools,
    MarkDirty,
}
