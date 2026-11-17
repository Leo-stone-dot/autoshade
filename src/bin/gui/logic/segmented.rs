//! The AI segmentation landing, headless: what a freshly drawn mask does.
//!
//! The worker draws a raster; the landing re-selects the mask that already
//! references this raster FAMILY (a re-run refreshed the same target) instead
//! of stacking an inert duplicate, repoints a stale reference at the new file,
//! or adds a new mask. It also words the line and says which model drew it —
//! a fallback tier is disclosed with a toast, not only a status line.
use autoshade::text::generated::*;


use super::{Effect, UiText};
use autoshade::recipe::{EditRecipe, LocalAdjustment, MaskGeometry};
use std::path::Path;

/// What the landing did, plus what the caller still has to say/apply.
pub(crate) struct Segmented {
    /// The mask to select (refreshed or freshly added).
    pub(crate) selection: usize,
    /// The backend line suffix, when the worker named its model.
    pub(crate) backend_note: Option<UiText>,
    /// MarkDirty + the status line (+ the fallback toast).
    pub(crate) effects: Vec<Effect>,
}

/// A stored raster name without the photo's own stem prefix, when it carries
/// one (legacy `out/<stem>.mask-sky.png` references do; a central bare name
/// does not).
fn strip_stem<'a>(name: &'a str, prefix: Option<&str>) -> &'a str {
    prefix.and_then(|sp| name.strip_prefix(sp)).unwrap_or(name)
}

/// Land one segmentation raster into the recipe.
pub(crate) fn apply(
    recipe: &mut EditRecipe,
    label: &str,
    raster: &Path,
    backend: &str,
    photo_stem: Option<&str>,
) -> Segmented {
    let new_name = raster.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let stem_prefix = photo_stem.map(|s| format!("{s}."));
    let find = |stored: &str| -> bool {
        let stored = Path::new(stored).file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        autoshade::store::mask_family(strip_stem(&stored, stem_prefix.as_deref()))
            == autoshade::store::mask_family(strip_stem(&new_name, stem_prefix.as_deref()))
    };
    // One raster per (photo, target): a re-run refreshed the SAME file target,
    // so re-select the mask that already references it (comparing by raster
    // FAMILY — a rerun's name is `claim_raster`-unique, e.g. `mask-sky-2`).
    let mut reused = None;
    for (i, m) in recipe.masks.iter_mut().enumerate() {
        if let MaskGeometry::Bitmap { path } = &mut m.mask
            && find(path)
        {
            // Re-point the stored path at the fresh raster so a stale
            // reference is revived instead of stranding the user's sliders.
            *path = raster.to_string_lossy().into_owned();
            reused = Some(i);
            break;
        }
    }
    let reused = match reused {
        Some(i) => Some(i),
        None => {
            recipe.masks.push(LocalAdjustment {
                mask: MaskGeometry::Bitmap { path: raster.to_string_lossy().into_owned() },
                name: label.to_owned(),
                ..Default::default()
            });
            None
        }
    };
    let selection = reused.unwrap_or(recipe.masks.len() - 1);
    // A rerun REFRESHES the existing mask's raster — its sliders may already
    // apply; claiming "added, adjust sliders" there was false on both counts.
    let status = if reused.is_some() {
        UiText::with(
            K_LOGIC_AI_MASK_REFRESHED_THE_EXISTING_MASK_NOW_USES_THE,
            &[("what", label)],
        )
    } else {
        UiText::with(
            K_LOGIC_AI_MASK_ADDED_ADJUST_ITS_SLIDERS_EXPOSURE_CONTRA,
            &[("what", label)],
        )
    };
    // WHICH MODEL DREW IT: the backend name goes on the line for every run, and
    // a FALLBACK also raises an error toast — a line the user is not looking at
    // is not a disclosure of a materially softer tier.
    let backend_note =
        (!backend.is_empty()).then(|| UiText::with(K_LOGIC_DRAWN_BY, &[("backend", backend)]));
    let mut effects = vec![Effect::MarkDirty, Effect::Status(status)];
    if autoshade::segment::backend_is_fallback(backend) {
        effects.push(Effect::ToastError(UiText::with(
            K_LOGIC_WAS_DRAWN_BY_THE_U_2_NET_FALLBACK_NOT_THE_PINNED,
            &[("what", label)],
        )));
    }
    Segmented { selection, backend_note, effects }
}

/// The toast when the landing MOVED the selection — index-armed tools bound to
/// another row die with it (the async door the row-click sites already cover).
pub(crate) fn selection_taken_words() -> UiText {
    UiText::of(
        K_LOGIC_THE_AI_RESULT_TOOK_THE_SELECTION_A_BRUSH_SESSION,
    )
}
