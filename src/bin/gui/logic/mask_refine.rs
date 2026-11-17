//! The mask-refine landing, headless: which mask a refined full-resolution
//! raster belongs to, and what to say when the list moved underneath it.
//!
//! The worker decoded the full-res source and wrote a new raster; by landing
//! time the mask list may have changed. Index and stored reference are
//! validated TOGETHER: index-with-matching-raster wins, else a unique raster
//! match is adopted, else the landing says so and applies nothing — it never
//! guesses onto the wrong mask.
use autoshade::text::generated::*;


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
                K_LOGIC_THIS_SOURCE_IS_A_FULL_RESOLUTION_REFINED_MASK_WO,
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
                                K_LOGIC_MASK_REFINED_TO_FULL_RESOLUTION_BOUNDARIES_NOW_F,
                            )),
                        ],
                    }
                }
                None => {
                    let text = UiText::with(
                        K_LOGIC_THE_MASK_CHANGED_WHILE_REFINING_THE_RESULT_WAS_S,
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
