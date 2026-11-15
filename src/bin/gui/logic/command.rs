//! The develop form's command layer: a widget reports a change as a Command and
//! the app applies it, so the mutation path is one place instead of every draw
//! site. Both scalar encodings are here: a plain field write, and the Lightroom
//! companion's explicit-zero write (a stored 0 means "the sidecar said
//! nothing").

use super::capability::{self, CapabilityId};
use autoshade::recipe::EditRecipe;

/// One edit the GUI asks the document to make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
    /// Write one scalar capability's value.
    SetCapability { id: CapabilityId, value: f32 },
    /// Write one COMPANION control: the value becomes explicit, or the control
    /// returns to absent when the value is its Lightroom default.
    SetCompanion { name: &'static str, value: f32 },
}

/// Apply one command to the recipe. The GUI calls this now; the CLI and the
/// web form are meant to call the same function.
pub(crate) fn apply(recipe: &mut EditRecipe, command: Command) {
    match command {
        Command::SetCapability { id, value } => capability::set_value(recipe, id, value),
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
        let mut recipe = EditRecipe::default();
        apply(&mut recipe, Command::SetCapability { id: CapabilityId::Exposure, value: 1.5 });
        apply(&mut recipe, Command::SetCapability { id: CapabilityId::Contrast, value: -20.0 });
        assert_eq!(recipe.exposure_ev, 1.5);
        assert_eq!(recipe.contrast, -20.0);
        assert_eq!(recipe.highlights, 0.0, "an unnamed field is untouched");
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
        for table in [capability::VIGNETTE, capability::GRAIN, capability::SHARPEN, capability::NOISE_LUMA, capability::NOISE_COLOR, capability::LENS_PROFILE, capability::LENS_DEFRINGE] {
            for row in table {
                if row.kind != capability::SliderKind::CompanionInt && row.kind != capability::SliderKind::CompanionFine {
                    continue;
                }
                let mut recipe = EditRecipe::default();
                apply(&mut recipe, Command::SetCompanion { name: row.control, value: 7.0 });
                assert_eq!(recipe.resolved(row.control), 7.0, "`{}` is not commandable", row.control);
            }
        }
    }

    #[test]
    fn the_command_round_trips_through_the_capability_reader() {
        for table in [capability::TONE, capability::TRANSFORM, capability::CAL_RED] {
            for row in table {
                if row.kind == capability::SliderKind::CompanionInt
                    || row.kind == capability::SliderKind::CompanionFine
                {
                    continue;
                }
                let mut recipe = EditRecipe::default();
                apply(&mut recipe, Command::SetCapability { id: row.id, value: 7.25 });
                assert_eq!(capability::value(&recipe, row.id), 7.25, "`{}` is not written", row.control);
            }
        }
    }
}
