//! Which denoise dial a verb reads: the active card's pixels decide the path
//! (a baked card under a RAW original still uses the baked dials) and the export
//! verb reads the export fold while the in-app verb reads the Detail fold.

use std::path::Path;

/// The active card's actual pixels choose the path: no card open (or a RAW
/// source) uses the RAW dials, a baked master uses the baked ones.
pub(crate) fn uses_raw(active_source: Option<&Path>) -> bool {
    active_source.is_none_or(autoshade::decode::is_raw)
}

/// The strength the calling verb should use, in RAW and baked variants.
pub(crate) fn selected_strength(
    uses_raw: bool,
    on_export: bool,
    raw_detail: f32,
    raw_export: f32,
    baked_detail: f32,
    baked_export: f32,
) -> f32 {
    match (uses_raw, on_export) {
        (true, false) => raw_detail,
        (true, true) => raw_export,
        (false, false) => baked_detail,
        (false, true) => baked_export,
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn no_card_or_a_raw_source_uses_the_raw_dials() {
        assert!(uses_raw(None), "an unopened app displays the RAW dials");
        assert!(uses_raw(Some(Path::new("photo.ARW"))));
    }

    #[test]
    fn a_baked_master_uses_the_baked_dials() {
        assert!(!uses_raw(Some(Path::new("out/denoise.png"))));
        assert!(!uses_raw(Some(Path::new("out/stack.png"))));
    }

    #[test]
    fn each_verb_reads_its_own_fold_in_both_variants() {
        assert_eq!(selected_strength(true, false, 0.71, 0.5, 0.3, 0.9), 0.71, "RAW Detail");
        assert_eq!(selected_strength(true, true, 0.71, 0.5, 0.3, 0.9), 0.5, "RAW Export");
        assert_eq!(selected_strength(false, false, 0.71, 0.5, 0.3, 0.9), 0.3, "baked Detail");
        assert_eq!(selected_strength(false, true, 0.71, 0.5, 0.3, 0.9), 0.9, "baked Export");
    }

    #[test]
    fn the_raw_and_export_pass_through_as_the_ints_both_directions() {
        // The four dials never bleed into each other: a sentinel on one slot
        // stays in that slot.
        let s = |on_export| selected_strength(true, on_export, 0.11, 0.22, 0.33, 0.44);
        assert_eq!((s(false), s(true)), (0.11, 0.22));
        let b = |on_export| selected_strength(false, on_export, 0.11, 0.22, 0.33, 0.44);
        assert_eq!((b(false), b(true)), (0.33, 0.44));
    }
}
