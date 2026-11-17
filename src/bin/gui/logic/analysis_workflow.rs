//! The analysis landing workflow: the epoch guard and the landing decision,
//! as data the host executes.
//!
//! The worker's result is either stale (discard), failed (word it), or a
//! landing to install. The host still owns the commit/fork/persist sequence;
//! this module owns WHICH sequence is due and what the landing contains.

use std::path::PathBuf;

use autoshade::advisor::Verdict;
use autoshade::rationale::Note;
use autoshade::recipe::EditRecipe;

/// What the analysis landing asks the host to do.
pub(crate) enum AnalysisCommand {
    /// A cancelled or superseded result: never install it.
    Ignore,
    /// Install this landing, then persist `stamped` when `accepted`.
    Install(Box<AnalysisInstall>),
    /// The worker failed; the host words the error.
    Failed(anyhow::Error),
}

/// One analysis landing's payload: the canvas recipe, the store's stamped
/// recipe, the verdict and its notes.
pub(crate) struct AnalysisInstall {
    pub(crate) canvas: EditRecipe,
    pub(crate) stamped: EditRecipe,
    pub(crate) verdict: Verdict,
    pub(crate) notes: Vec<Note>,
    pub(crate) accepted: bool,
    pub(crate) photo: Option<PathBuf>,
}

/// Decide one worker result. `epoch` is the epoch the worker ran under;
/// `current_epoch` is the app's live one.
pub(crate) fn land(
    epoch: u64,
    current_epoch: u64,
    result: anyhow::Result<(EditRecipe, Verdict, Vec<Note>)>,
    skin_held: bool,
    ai_pixels: bool,
    photo: Option<PathBuf>,
) -> AnalysisCommand {
    if epoch != current_epoch {
        return AnalysisCommand::Ignore;
    }
    match result {
        Ok((recipe, verdict, notes)) => {
            let shaped = super::analysis_landing::shape(recipe, skin_held, ai_pixels);
            AnalysisCommand::Install(Box::new(AnalysisInstall {
                canvas: shaped.canvas,
                stamped: shaped.stamped,
                accepted: super::analysis_landing::accepted(&verdict),
                verdict,
                notes,
                photo,
            }))
        }
        Err(e) => AnalysisCommand::Failed(e),
    }
}
