//! The develop form's command layer: a widget reports a change as a Command and
//! the app applies it, so the mutation path is one place instead of every draw
//! site. Both scalar encodings are here: a plain field write, and the Lightroom
//! companion's explicit-zero write (a stored 0 means "the sidecar said
//! nothing").

use autoshade::recipe::EditRecipe;

/// One edit the GUI asks the document to make.
#[derive(Clone, Debug)]
pub(crate) enum Command {
    /// Write one scalar capability's value. The row carries the write (so a
    /// new control is a table row, not a variant + arm); `id` names the
    /// control for diagnostics and tests.
    SetCapability { write: fn(&mut EditRecipe, f32), value: f32 },
    /// Write one COMPANION control: the value becomes explicit, or the control
    /// returns to absent when the value is its Lightroom default.
    SetCompanion { name: &'static str, value: f32 },
    /// A local-adjustment list operation (the mask panel's verbs).
    Mask(MaskCommand),
    /// Insert `mask` after slot `after` (the duplicate verb: the caller has
    /// already detached the rasters and named the copy).
    InsertMask { after: usize, mask: Box<autoshade::recipe::LocalAdjustment> },
}

/// The mask-list verbs a client can ask for. Selection remapping and the
/// overlay invalidation stay with the caller: this layer owns the recipe list.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum MaskCommand {
    /// Drop the mask at `index` (out of range: nothing).
    Delete { index: usize },
    /// Flip the mask's eye (out of range: nothing).
    ToggleEnabled { index: usize },
    /// Move `from` to the final slot `to` (both pre-checked by the caller).
    Reorder { from: usize, to: usize },
    /// Swap the mask with the one above it (the first row: nothing).
    MoveUp { index: usize },
    /// Swap the mask with the one below it (the last row: nothing).
    MoveDown { index: usize },
    /// Replace the mask's display name (out of range: nothing).
    Rename { index: usize, name: String },
    /// Replace the mask's range mode (the panel builds the neutral start).
    SetRange { index: usize, range: Option<autoshade::recipe::RangeMask> },
    /// Replace one component's combine mode.
    SetComponentMode { index: usize, component: usize, mode: autoshade::recipe::MaskCombine },
    /// Replace one component's inverted flag.
    SetComponentInverted { index: usize, component: usize, inverted: bool },
    /// Drop one component.
    RemoveComponent { index: usize, component: usize },
}

/// Apply one command to the recipe. The GUI calls this now; the CLI and the
/// web form are meant to call the same function.
pub(crate) fn apply(recipe: &mut EditRecipe, command: Command) {
    match command {
        Command::SetCapability { write, value, .. } => (write)(recipe, value),
        Command::Mask(MaskCommand::Delete { index }) => {
            if index < recipe.masks.len() {
                recipe.masks.remove(index);
            }
        }
        Command::Mask(MaskCommand::ToggleEnabled { index }) => {
            if let Some(m) = recipe.masks.get_mut(index) {
                m.enabled = !m.enabled;
            }
        }
        Command::Mask(MaskCommand::Reorder { from, to }) => {
            if from < recipe.masks.len() && to < recipe.masks.len() {
                let m = recipe.masks.remove(from);
                recipe.masks.insert(to, m);
            }
        }
        Command::Mask(MaskCommand::MoveUp { index }) => {
            if index > 0 && index < recipe.masks.len() {
                recipe.masks.swap(index, index - 1);
            }
        }
        Command::Mask(MaskCommand::MoveDown { index }) => {
            if index + 1 < recipe.masks.len() {
                recipe.masks.swap(index, index + 1);
            }
        }
        Command::Mask(MaskCommand::Rename { index, name }) => {
            if let Some(m) = recipe.masks.get_mut(index) {
                m.name = name;
            }
        }
        Command::Mask(MaskCommand::SetRange { index, range }) => {
            if let Some(m) = recipe.masks.get_mut(index) {
                m.range = range;
            }
        }
        Command::Mask(MaskCommand::SetComponentMode { index, component, mode }) => {
            if let Some(c) = recipe.masks.get_mut(index).and_then(|m| m.components.get_mut(component)) {
                c.mode = mode;
            }
        }
        Command::Mask(MaskCommand::SetComponentInverted { index, component, inverted }) => {
            if let Some(c) = recipe.masks.get_mut(index).and_then(|m| m.components.get_mut(component)) {
                c.inverted = inverted;
            }
        }
        Command::Mask(MaskCommand::RemoveComponent { index, component }) => {
            if let Some(m) = recipe.masks.get_mut(index) {
                if component < m.components.len() {
                    m.components.remove(component);
                }
            }
        }
        Command::InsertMask { after, mask } => {
            // The recipe's own cap (64) is a domain invariant, so it is
            // enforced HERE as well as by the panel's toast.
            if recipe.masks.len() < 64 {
                recipe.masks.insert((after + 1).min(recipe.masks.len()), *mask);
            }
        }
        Command::SetCompanion { name, value } => {
            let default = autoshade::recipe::LR_COMPANION_DEFAULTS
                .iter()
                .find(|(n, _)| *n == name)
                .map_or(0.0, |(_, d)| *d);
            if value == default {
                recipe.clear_resolved(name);
            } else {
                recipe.set_resolved(name, value);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_set_capability_command_writes_the_named_field() {
        use crate::logic::capability::TONE;
        let mut recipe = EditRecipe::default();
        for (control, value) in [("exposure_ev", 1.5), ("contrast", -20.0)] {
            let cap = TONE.iter().find(|c| c.control == control).expect("row listed");
            let (_, write) = cap.plain().expect("a plain row");
            apply(&mut recipe, Command::SetCapability { write, value });
        }
        assert_eq!(recipe.exposure_ev, 1.5);
        assert_eq!(recipe.contrast, -20.0);
        assert_eq!(recipe.highlights, 0.0, "an unnamed field is untouched");
    }

    #[test]
    fn the_mask_list_verbs_edit_the_recipe_and_ignore_stale_indices() {
        use autoshade::recipe::LocalAdjustment;
        let mut recipe = EditRecipe::default();
        for name in ["a", "b", "c"] {
            recipe.masks.push(LocalAdjustment { name: name.into(), ..Default::default() });
        }
        apply(&mut recipe, Command::Mask(MaskCommand::ToggleEnabled { index: 1 }));
        assert!(!recipe.masks[1].enabled);
        apply(&mut recipe, Command::Mask(MaskCommand::Reorder { from: 0, to: 2 }));
        assert_eq!(recipe.masks.iter().map(|m| m.name.trim()).collect::<Vec<_>>(), ["b", "c", "a"]);
        apply(&mut recipe, Command::Mask(MaskCommand::Delete { index: 1 }));
        assert_eq!(recipe.masks.iter().map(|m| m.name.trim()).collect::<Vec<_>>(), ["b", "a"]);
        // A stale index from a drag started before the list shrank is a no-op,
        // not a panic.
        apply(&mut recipe, Command::Mask(MaskCommand::Delete { index: 9 }));
        apply(&mut recipe, Command::Mask(MaskCommand::ToggleEnabled { index: 9 }));
        apply(&mut recipe, Command::Mask(MaskCommand::Reorder { from: 9, to: 0 }));
        assert_eq!(recipe.masks.len(), 2);
    }

    #[test]
    fn the_component_and_rename_verbs_edit_only_the_named_slot() {
        use autoshade::recipe::{LocalAdjustment, MaskCombine, MaskComponent, MaskGeometry, RangeMask};
        let mut recipe = EditRecipe::default();
        recipe.masks.push(LocalAdjustment {
            mask: MaskGeometry::Linear { zero_x: 0.1, zero_y: 0.1, full_x: 0.9, full_y: 0.9 },
            components: vec![MaskComponent {
                inverted: false,
                geometry: MaskGeometry::Linear { zero_x: 0.2, zero_y: 0.2, full_x: 0.8, full_y: 0.8 },
                mode: MaskCombine::Add,
            }],
            ..Default::default()
        });
        apply(&mut recipe, Command::Mask(MaskCommand::Rename { index: 0, name: "sky 2".into() }));
        assert_eq!(recipe.masks[0].name.trim(), "sky 2");
        apply(&mut recipe, Command::Mask(MaskCommand::SetComponentMode {
            index: 0, component: 0, mode: MaskCombine::Subtract,
        }));
        assert_eq!(recipe.masks[0].components[0].mode, MaskCombine::Subtract);
        apply(&mut recipe, Command::Mask(MaskCommand::SetComponentInverted {
            index: 0, component: 0, inverted: true,
        }));
        assert!(recipe.masks[0].components[0].inverted);
        apply(&mut recipe, Command::Mask(MaskCommand::SetRange {
            index: 0, range: Some(RangeMask::Luminance { lo_outer: 0.0, lo: 0.0, hi: 1.0, hi_outer: 1.0 }),
        }));
        assert!(matches!(recipe.masks[0].range, Some(RangeMask::Luminance { .. })));
        apply(&mut recipe, Command::Mask(MaskCommand::RemoveComponent { index: 0, component: 0 }));
        assert!(recipe.masks[0].components.is_empty());
        // Missing mask / component arms are no-ops.
        apply(&mut recipe, Command::Mask(MaskCommand::Rename { index: 9, name: "x".into() }));
        apply(&mut recipe, Command::Mask(MaskCommand::RemoveComponent { index: 0, component: 9 }));
        assert_eq!(recipe.masks.len(), 1);
    }

    #[test]
    fn move_and_insert_guard_the_ends_and_the_mask_cap() {
        use autoshade::recipe::LocalAdjustment;
        let mut recipe = EditRecipe::default();
        for name in ["a", "b", "c"] {
            recipe.masks.push(LocalAdjustment { name: name.into(), ..Default::default() });
        }
        let names = |r: &EditRecipe| r.masks.iter().map(|m| m.name.trim().to_string()).collect::<Vec<_>>();
        apply(&mut recipe, Command::Mask(MaskCommand::MoveDown { index: 0 }));
        assert_eq!(names(&recipe), ["b", "a", "c"]);
        apply(&mut recipe, Command::Mask(MaskCommand::MoveUp { index: 2 }));
        assert_eq!(names(&recipe), ["b", "c", "a"]);
        apply(&mut recipe, Command::Mask(MaskCommand::MoveUp { index: 0 }));
        apply(&mut recipe, Command::Mask(MaskCommand::MoveDown { index: 2 }));
        assert_eq!(names(&recipe), ["b", "c", "a"], "the ends do not move");
        apply(
            &mut recipe,
            Command::InsertMask { after: 1, mask: Box::new(LocalAdjustment { name: "copy".into(), ..Default::default() }) },
        );
        assert_eq!(names(&recipe), ["b", "c", "copy", "a"]);
        // The 64-mask cap is the domain's, not only the panel's toast.
        while recipe.masks.len() < 64 {
            recipe.masks.push(Default::default());
        }
        apply(&mut recipe, Command::InsertMask { after: 0, mask: Box::new(Default::default()) });
        assert_eq!(recipe.masks.len(), 64);
    }

    #[test]
    fn a_companion_command_sets_an_explicit_value_and_clears_at_the_default() {
        let mut recipe = EditRecipe::default();
        assert_eq!(recipe.resolved("post_crop_vignette_mid"), 50.0, "absent reads the Lightroom default");
        apply(&mut recipe, Command::SetCompanion { name: "post_crop_vignette_mid", value: 80.0 });
        assert_eq!(recipe.resolved("post_crop_vignette_mid"), 80.0);
        apply(&mut recipe, Command::SetCompanion { name: "post_crop_vignette_mid", value: 50.0 });
        assert_eq!(recipe.resolved("post_crop_vignette_mid"), 50.0, "the default returns the control to absent");
    }

    #[test]
    fn every_companion_row_is_commandable() {
        use crate::logic::capability::{SliderKind, TABLES};
        for table in TABLES {
            for row in *table {
                if !matches!(row.kind, SliderKind::CompanionInt | SliderKind::CompanionFine) {
                    continue;
                }
                let mut recipe = EditRecipe::default();
                apply(&mut recipe, Command::SetCompanion { name: row.control, value: 7.0 });
                assert_eq!(recipe.resolved(row.control), 7.0, "`{}` is not commandable", row.control);
            }
        }
    }

    #[test]
    fn every_plain_row_is_commandable_and_round_trips() {
        use crate::logic::capability::TABLES;
        for table in TABLES {
            for row in *table {
                let Some((get, write)) = row.plain() else { continue };
                let mut recipe = EditRecipe::default();
                apply(&mut recipe, Command::SetCapability { write, value: 7.25 });
                assert_eq!(get(&recipe), 7.25, "`{}` is not written", row.control);
            }
        }
    }
}
