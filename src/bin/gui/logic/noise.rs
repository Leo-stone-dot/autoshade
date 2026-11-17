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
