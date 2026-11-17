//! The retouch landing workflow: classify a worker result into the landing
//! shape the host installs.
//!
//! The worker returns pixels, a typed note, the saved master and the kind of
//! card the result belongs on. This module owns that mapping and the HDR
//! headroom rule; the host owns the commit/fork/switch/persist sequence.

use std::path::PathBuf;
use std::sync::Arc;

use crate::*;
use crate::logic::retouch::RetouchNote;

/// What a retouch result asks the host to install.
pub(crate) enum RetouchLanding {
    /// A cancelled or superseded result. `discard` is the pixel master the
    /// worker already saved; the host removes it so a cancel leaves no orphan.
    Ignore { discard: Option<PathBuf> },
    /// The worker failed; the host words the error.
    Failed(anyhow::Error),
    /// A new card whose base is the retouched raster.
    NewVariant {
        kind: VariantKind,
        recipe: Box<EditRecipe>,
        image: Arc<image::DynamicImage>,
        origin: PathBuf,
        note: RetouchNote,
    },
    /// A layer over the card on screen.
    Layer { path: PathBuf, note: RetouchNote },
    /// A pixel touch-up of the active card's base.
    InPlace { image: Arc<image::DynamicImage>, origin: PathBuf, note: RetouchNote },
}

/// Decide one retouch landing. `epoch` is the epoch the worker ran under;
/// `current_epoch` is the app's live one.
pub(crate) fn land(
    epoch: u64,
    current_epoch: u64,
    done: RetouchDone,
    live_recipe: &EditRecipe,
) -> RetouchLanding {
    if epoch != current_epoch {
        let discard = done.ok().map(|(_, _, saved, _)| saved);
        return RetouchLanding::Ignore { discard };
    }
    match done {
        Ok((img, note, saved, kind)) => match kind {
            RetouchKind::NewGenerated => RetouchLanding::NewVariant {
                kind: VariantKind::Generated,
                recipe: Box::new(EditRecipe::default()),
                image: Arc::new(img),
                origin: saved,
                note,
            },
            RetouchKind::Layer => RetouchLanding::Layer {
                // Absolute, so the saved recipe finds it from any working
                // directory.
                path: std::path::absolute(&saved).unwrap_or(saved),
                note,
            },
            RetouchKind::NewDenoised | RetouchKind::NewStacked => {
                let mut recipe = live_recipe.clone();
                // An HDR merge hands over stops of highlight above the
                // reference frame's white. They are only reachable through
                // the SDR rendition stage, so the card arrives with that
                // stage ON and told how much room it has.
                if let RetouchNote::Stacked { headroom_ev, .. } = &note
                    && *headroom_ev > 0.0
                {
                    recipe.hdr_edit = true;
                    recipe.hdr_max_ev = *headroom_ev;
                }
                RetouchLanding::NewVariant {
                    kind: if kind == RetouchKind::NewStacked {
                        VariantKind::Stacked
                    } else {
                        VariantKind::Denoised
                    },
                    recipe: Box::new(recipe),
                    image: Arc::new(img),
                    origin: saved,
                    note,
                }
            }
            RetouchKind::InPlace => RetouchLanding::InPlace {
                image: Arc::new(img),
                origin: saved,
                note,
            },
        },
        Err(e) => RetouchLanding::Failed(e),
    }
}
