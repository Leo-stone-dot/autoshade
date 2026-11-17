//! The paste landing workflow: a partial failure must never read as a clean
//! success, and only a clean landing may advance the ● baseline.

use crate::*;

/// What the paste worker's result asks the host to install.
pub(crate) enum PasteLanding {
    /// Every target was written and every XMP half succeeded.
    Clean(PasteOutcome),
    /// At least one target failed; the others may still have been written.
    Partial(PasteOutcome),
    /// The worker itself failed.
    Failed(anyhow::Error),
}

pub(crate) fn land(res: anyhow::Result<PasteOutcome>) -> PasteLanding {
    match res {
        Ok(out) if out.errs.is_empty() => PasteLanding::Clean(out),
        Ok(out) => PasteLanding::Partial(out),
        Err(e) => PasteLanding::Failed(e),
    }
}
