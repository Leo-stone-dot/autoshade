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

pub(crate) mod lens_blur;
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
    /// Lens blur: mute/unmute without losing the settings.
    ToggleLensBlur(bool),
    SetBlurAmount(f32),
    SetFocusRange(f32),
    RemoveLensBlur,
    /// Arm / disarm the click-on-the-photo focus pick.
    SetFocusPicking(bool),
    /// Ask for the depth estimate (the worker runs the sidecar).
    EstimateDepth,
    /// The armed focus pick landed on this point, in normalized original-frame
    /// coordinates.
    FocusPicked([f32; 2]),
}

/// One line the caller should show: the i18n key plus its named arguments.
/// Rendering lives outside this layer (`i18n::text`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct UiText {
    pub(crate) key: &'static str,
    pub(crate) args: Vec<(&'static str, String)>,
}

impl UiText {
    pub(crate) fn of(key: &'static str) -> Self {
        Self { key, args: Vec::new() }
    }

    /// A key with named arguments (`{name}` placeholders, as `trf` takes them).
    pub(crate) fn with(key: &'static str, args: &[(&'static str, &str)]) -> Self {
        Self {
            key,
            args: args.iter().map(|(name, value)| (*name, (*value).to_owned())).collect(),
        }
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
    /// One canvas tool at a time: every arming site disarms the rest.
    DisarmTools,
    MarkDirty,
}
