//! The reverse-fit landing workflow: turn the worker's outcome into the
//! variant the host installs, including the "same negative" base rule.

use std::path::PathBuf;
use std::sync::Arc;

use crate::*;

/// What the fit landing asks the host to install.
pub(crate) enum FitLanding {
    Failed(anyhow::Error),
    Install {
        persisted: bool,
        recipe: Box<EditRecipe>,
        err_before: f32,
        err_after: f32,
        status: Vec<FitNote>,
        rationale_notes: Vec<autoshade::rationale::Note>,
        negative: Option<PathBuf>,
        /// The negative's decoded base when the original card still carries
        /// the same master; otherwise the host re-decodes from `origin`.
        base: Option<Arc<image::DynamicImage>>,
    },
}

/// Classify one fit result. `original` is the photo's Original card, when
/// there is one; the ◭ card shares its decoded pixels only when that card
/// still hangs off the same negative master.
pub(crate) fn land(out: anyhow::Result<FitOutcome>, original: Option<&Variant>) -> FitLanding {
    match out {
        Err(e) => FitLanding::Failed(e),
        Ok(out) => {
            let base = out.negative.as_deref().and_then(|m| {
                let v = original?;
                (v.origin.as_deref() == Some(m)).then(|| v.base.clone()).flatten()
            });
            FitLanding::Install {
                persisted: out.persisted,
                recipe: Box::new(out.recipe),
                err_before: out.err_before,
                err_after: out.err_after,
                status: out.status,
                rationale_notes: out.rationale_notes,
                negative: out.negative,
                base,
            }
        }
    }
}
