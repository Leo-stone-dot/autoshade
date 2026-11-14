//! The mask-refine landing, headless: which mask a refined full-resolution
//! raster belongs to, and what to say when the list moved underneath it.
//!
//! The worker decoded the full-res source and wrote a new raster; by landing
//! time the mask list may have changed. Index and stored reference are
//! validated TOGETHER: index-with-matching-raster wins, else a unique raster
//! match is adopted, else the landing says so and applies nothing — it never
//! guesses onto the wrong mask.

use super::{Effect, UiText};
use autoshade::recipe::EditRecipe;
use std::path::PathBuf;

/// One refine outcome, as the landing sees it (the path display is rendered by
/// the caller).
pub(crate) enum Outcome {
    /// The source would not fit the mask budget: nothing was written.
    OverBudget { w: u32, h: u32 },
    /// The initiating index, the stored raster reference the job started from,
    /// and the refined raster (plus its display form).
    Refined { index: usize, stored_ref: String, out: PathBuf, out_display: String },
}

/// What the landing did: the mask to select (when one was found), and the
/// effects (repoint/dirty + the line, or the refusal).
pub(crate) struct Refined {
    pub(crate) selection: Option<usize>,
    pub(crate) effects: Vec<Effect>,
}

/// Apply one refine outcome to the recipe.
pub(crate) fn apply(recipe: &mut EditRecipe, outcome: &Outcome) -> Refined {
    match outcome {
        Outcome::OverBudget { w, h } => {
            // Nothing was written: the mask keeps the raster it had.
            let text = UiText::with(
                "this source is {w}×{h} — a full-resolution refined mask would exceed the mask budget and could never be read back",
                &[("w", &w.to_string()), ("h", &h.to_string())],
            );
            Refined {
                selection: None,
                effects: vec![Effect::ToastError(text.clone()), Effect::Status(text)],
            }
        }
        Outcome::Refined { index, stored_ref, out, out_display } => {
            // By FILE, not by variant: a refined reverse-fit zone is a Select
            // Sky component carrying the same raster.
            let at = |m: &autoshade::recipe::LocalAdjustment| {
                autoshade::render::geometry_raster_path(&m.mask).is_some_and(|p| p == stored_ref)
            };
            let hit = match recipe.masks.get(*index) {
                Some(m) if at(m) => Some(*index),
                _ => {
                    let matches: Vec<usize> =
                        recipe.masks.iter().enumerate().filter(|(_, m)| at(m)).map(|(i, _)| i).collect();
                    // Only an UNAMBIGUOUS survivor is adopted.
                    (matches.len() == 1).then(|| matches[0])
                }
            };
            match hit {
                Some(i) => {
                    // Repoint, never re-shape (`masks.rs::commit_mask_brush`);
                    // `at` already proved this geometry carries a raster.
                    let out_s = out.to_string_lossy().into_owned();
                    if let Some(slot) = autoshade::render::geometry_raster_path_mut(&mut recipe.masks[i].mask) {
                        *slot = out_s;
                    }
                    Refined {
                        selection: Some(i),
                        effects: vec![
                            Effect::MarkDirty,
                            Effect::Status(UiText::of(
                                "mask refined to full resolution — boundaries now follow the source's own edges",
                            )),
                        ],
                    }
                }
                None => {
                    let text = UiText::with(
                        "the mask changed while refining — the result was saved at {path} but not applied",
                        &[("path", out_display)],
                    );
                    Refined {
                        selection: None,
                        effects: vec![Effect::ToastError(text.clone()), Effect::Status(text)],
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use autoshade::recipe::{LocalAdjustment, MaskGeometry};

    fn bitmap(path: &str) -> MaskGeometry {
        MaskGeometry::Bitmap { path: path.into() }
    }

    fn refined(index: usize, stored: &str, out: &str) -> Outcome {
        Outcome::Refined {
            index,
            stored_ref: stored.into(),
            out: PathBuf::from(out),
            out_display: out.into(),
        }
    }

    /// The initiating index wins when it still carries that raster; the mask is
    /// re-pointed (never re-shaped), selected, dirtied and worded.
    #[test]
    fn a_refined_raster_repoints_and_selects_its_mask() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(LocalAdjustment { mask: bitmap("d/mask-sky.png"), ..Default::default() });
        let landed = apply(&mut recipe, &refined(0, "d/mask-sky.png", "d/mask-sky-refined.png"));
        assert_eq!(landed.selection, Some(0));
        match &recipe.masks[0].mask {
            MaskGeometry::Bitmap { path } => assert_eq!(path, "d/mask-sky-refined.png"),
            other => panic!("{other:?}"),
        }
        assert!(matches!(landed.effects[0], Effect::MarkDirty));
        match &landed.effects[1] {
            Effect::Status(t) => assert!(t.key.starts_with("mask refined to full resolution")),
            other => panic!("{other:?}"),
        }
    }

    /// A stale index falls back to the UNIQUE raster match; two masks sharing
    /// the raster are ambiguous, so nothing is applied and the user is told.
    #[test]
    fn a_stale_index_adopts_a_unique_match_and_refuses_an_ambiguous_one() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(LocalAdjustment { mask: bitmap("d/mask-a.png"), ..Default::default() });
        recipe.masks.push(LocalAdjustment { mask: bitmap("d/mask-b.png"), ..Default::default() });
        // Index 9 is stale; mask-b still carries the stored raster uniquely.
        let landed = apply(&mut recipe, &refined(9, "d/mask-b.png", "d/mask-b-refined.png"));
        assert_eq!(landed.selection, Some(1));

        let mut ambiguous = EditRecipe::default();
        for _ in 0..2 {
            ambiguous.masks.push(LocalAdjustment { mask: bitmap("d/mask-x.png"), ..Default::default() });
        }
        let refused = apply(&mut ambiguous, &refined(9, "d/mask-x.png", "d/mask-x-refined.png"));
        assert_eq!(refused.selection, None, "never guess between two masks");
        assert!(matches!(refused.effects[0], Effect::ToastError(_)));
        assert!(ambiguous.masks.iter().all(|m| matches!(&m.mask, MaskGeometry::Bitmap { path } if path == "d/mask-x.png")), "nothing re-pointed");
    }

    /// An over-budget refusal keeps the old raster and says which source it is
    /// about (dimensions in the line).
    #[test]
    fn an_over_budget_refusal_names_the_source_size() {
        let mut recipe = EditRecipe::default();
        recipe.masks.push(LocalAdjustment { mask: bitmap("d/mask-sky.png"), ..Default::default() });
        let landed = apply(&mut recipe, &Outcome::OverBudget { w: 12000, h: 8000 });
        assert_eq!(landed.selection, None);
        assert!(matches!(landed.effects[0], Effect::ToastError(_)));
        match &landed.effects[1] {
            Effect::Status(t) => {
                assert_eq!(t.args[0].1, super::super::Arg::Text("12000".into()));
                assert_eq!(t.args[1].1, super::super::Arg::Text("8000".into()));
            }
            other => panic!("{other:?}"),
        }
    }
}
