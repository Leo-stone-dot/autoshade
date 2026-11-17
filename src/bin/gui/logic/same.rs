//! Pixel and develop identity, headless: one baked master written two ways is
//! one master, and calibration provenance is not a user edit.

use std::path::Path;

use autoshade::recipe::EditRecipe;

/// Same master file? The same file written relative and absolute must read as
/// one master — the store and the canvas spell it differently.
pub(crate) fn same_master(a: &Path, b: &Path) -> bool {
    a == b || matches!((std::path::absolute(a), std::path::absolute(b)), (Ok(x), Ok(y)) if x == y)
}

/// The optional form: `None` means "no baked master" and matches only itself.
pub(crate) fn same_master_opt(a: Option<&Path>, b: Option<&Path>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => same_master(a, b),
        _ => false,
    }
}

/// A develop differs from its baseline? The calibration provenance stamp and
/// the base-look curve are not user edits: a repaired canvas against an
/// unrepaired baseline (either direction) must not read as unsaved work.
pub(crate) fn dirty_vs(canvas: &EditRecipe, baseline: &EditRecipe) -> bool {
    canvas != baseline
        && EditRecipe { version: 0, base_curve: Vec::new(), ..canvas.clone() }
            != EditRecipe { version: 0, base_curve: Vec::new(), ..baseline.clone() }
}
