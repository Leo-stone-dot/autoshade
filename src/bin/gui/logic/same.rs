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

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    #[test]
    fn one_master_written_two_ways_is_the_same_master() {
        assert!(same_master(Path::new("out/a.png"), Path::new("out/a.png")));
        assert!(same_master(Path::new("out/a.png"), &std::path::absolute("out/a.png").unwrap()));
        assert!(!same_master(Path::new("out/a.png"), Path::new("out/b.png")));
    }

    #[test]
    fn no_master_matches_only_no_master() {
        assert!(same_master_opt(None, None));
        assert!(!same_master_opt(None, Some(Path::new("out/a.png"))));
        assert!(!same_master_opt(Some(Path::new("out/a.png")), None));
        assert!(same_master_opt(Some(Path::new("out/a.png")), Some(&PathBuf::from("out/a.png"))));
    }

    #[test]
    fn calibration_provenance_is_not_an_edit_but_a_recipe_change_is() {
        let base = EditRecipe::default();
        let mut repaired = base.clone();
        repaired.version = 2;
        repaired.base_curve = vec![[0.0, 0.0], [1.0, 1.0]];
        assert!(!dirty_vs(&repaired, &base), "stamp + curve are neutralised");
        let mut edited = base.clone();
        edited.exposure_ev = 0.4;
        assert!(dirty_vs(&edited, &base));
    }
}
