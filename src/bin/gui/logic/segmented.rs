//! The AI segmentation landing, headless: what a freshly drawn mask does.
//!
//! The worker draws a raster; the landing re-selects the mask that already
//! references this raster FAMILY (a re-run refreshed the same target) instead
//! of stacking an inert duplicate, repoints a stale reference at the new file,
//! or adds a new mask. It also words the line and says which model drew it —
//! a fallback tier is disclosed with a toast, not only a status line.

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
            "AI「{what}」mask refreshed — the existing mask now uses the new selection (its sliders still apply)",
            &[("what", label)],
        )
    } else {
        UiText::with(
            "AI「{what}」mask added — adjust its sliders (exposure / contrast / saturation…) to take effect",
            &[("what", label)],
        )
    };
    // WHICH MODEL DREW IT: the backend name goes on the line for every run, and
    // a FALLBACK also raises an error toast — a line the user is not looking at
    // is not a disclosure of a materially softer tier.
    let backend_note =
        (!backend.is_empty()).then(|| UiText::with(" · drawn by {backend}", &[("backend", backend)]));
    let mut effects = vec![Effect::MarkDirty, Effect::Status(status)];
    if autoshade::segment::backend_is_fallback(backend) {
        effects.push(Effect::ToastError(UiText::with(
            "「{what}」was drawn by the U^2-Net FALLBACK, not the pinned BiRefNet — its edges are materially softer and it can invent a subject where there is none. Install torchvision + timm + einops matching your torch to get the pinned model.",
            &[("what", label)],
        )));
    }
    Segmented { selection, backend_note, effects }
}

/// The toast when the landing MOVED the selection — index-armed tools bound to
/// another row die with it (the async door the row-click sites already cover).
pub(crate) fn selection_taken_words() -> UiText {
    UiText::of(
        "the AI result took the selection — a brush session on another mask ended; its unbaked strokes were discarded",
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bitmap(path: &str) -> MaskGeometry {
        MaskGeometry::Bitmap { path: path.into() }
    }

    /// A fresh target adds a mask named by the AI's label and selects it.
    #[test]
    fn a_new_raster_adds_and_selects_a_mask() {
        let mut recipe = EditRecipe::default();
        let landed = apply(
            &mut recipe,
            "sky",
            Path::new("develop/DSC1.mask-sky.png"),
            "birefnet",
            Some("DSC1"),
        );
        assert_eq!(recipe.masks.len(), 1);
        assert_eq!(recipe.masks[0].name, "sky");
        assert!(matches!(&recipe.masks[0].mask, MaskGeometry::Bitmap { path } if path.ends_with("mask-sky.png")));
        assert_eq!(landed.selection, 0);
        assert!(matches!(landed.effects[0], Effect::MarkDirty));
        match &landed.effects[1] {
            Effect::Status(t) => assert!(t.key.starts_with("AI「{what}」mask added")),
            other => panic!("{other:?}"),
        }
        assert!(landed.backend_note.is_some(), "the model is named on the line");
    }

    /// A re-run's claim-unique name still matches the same mask family and
    /// repoints it; a legacy "out/<stem>." stored prefix strips on both sides.
    #[test]
    fn a_rerun_refreshes_the_same_family_instead_of_stacking() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(LocalAdjustment {
            mask: bitmap("out/DSC1.mask-sky.png"),
            name: "sky".into(),
            ..Default::default()
        });
        let landed = apply(
            &mut recipe,
            "sky",
            Path::new("develop/DSC1.mask-sky-2.png"),
            "birefnet",
            Some("DSC1"),
        );
        assert_eq!(recipe.masks.len(), 1, "no inert duplicate");
        assert_eq!(landed.selection, 0);
        match &recipe.masks[0].mask {
            MaskGeometry::Bitmap { path } => {
                assert!(path.ends_with("mask-sky-2.png"), "the stale reference is re-pointed: {path}");
            }
            other => panic!("{other:?}"),
        }
        match &landed.effects[1] {
            Effect::Status(t) => assert!(t.key.starts_with("AI「{what}」mask refreshed")),
            other => panic!("{other:?}"),
        }
    }

    /// A fallback backend is disclosed with an error toast; the pinned tier is
    /// not, and an unnamed backend adds no suffix.
    #[test]
    fn the_fallback_tier_interrupts() {
        let mut recipe = EditRecipe::default();
        let fallback = apply(&mut recipe, "subject", Path::new("d/mask-subject.png"), "u2net FALLBACK", None);
        assert!(fallback.backend_note.is_some());
        assert!(fallback.effects.iter().any(|e| matches!(e, Effect::ToastError(_))), "a fallback toast");

        let pinned = apply(&mut recipe, "subject", Path::new("d/mask-subject-2.png"), "birefnet", None);
        assert!(!pinned.effects.iter().any(|e| matches!(e, Effect::ToastError(_))), "no toast for the pinned tier");

        let unnamed = apply(&mut recipe, "subject", Path::new("d/mask-subject-3.png"), "", None);
        assert!(unnamed.backend_note.is_none());
    }
}

#[cfg(test)]
mod family_tests {
    use super::*;
    use std::path::Path;

    fn bitmap(path: &str) -> LocalAdjustment {
        LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: path.into() },
            name: "sky".into(),
            ..Default::default()
        }
    }

    #[test]
    fn a_rerun_refreshes_the_mask_that_references_the_same_raster_family() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(bitmap("out/photo.mask-sky.png"));
        let done = apply(&mut recipe, "sky", Path::new("out/photo.mask-sky-2.png"), "", Some("photo"));
        assert_eq!(done.selection, 0, "the rerun re-selects the existing mask");
        assert_eq!(recipe.masks.len(), 1, "no inert duplicate is stacked");
        assert!(matches!(&recipe.masks[0].mask, MaskGeometry::Bitmap { path } if path.ends_with("mask-sky-2.png")));
    }

    #[test]
    fn a_new_target_adds_its_own_mask_and_keeps_sliders() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(bitmap("out/photo.mask-sky.png"));
        let done = apply(&mut recipe, "subject", Path::new("out/photo.mask-subject.png"), "", Some("photo"));
        assert_eq!(done.selection, 1);
        assert_eq!(recipe.masks.len(), 2);
        assert_eq!(recipe.masks[1].name, "subject");
    }
}
