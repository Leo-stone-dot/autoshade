//! The develop form's command layer: a widget reports a change as a Command and
//! the app applies it, so the mutation path is one place instead of every draw
//! site. The first command is a capability write; companion rows still write
//! through their resolved encoding directly until the command grows that arm.

use super::capability::{self, CapabilityId};
use autoshade::recipe::EditRecipe;

/// One edit the GUI asks the document to make.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Command {
    /// Write one scalar capability's value.
    SetCapability { id: CapabilityId, value: f32 },
}

/// Apply one command to the recipe. The GUI calls this now; the CLI and the
/// web form are meant to call the same function.
pub(crate) fn apply(recipe: &mut EditRecipe, command: Command) {
    match command {
        Command::SetCapability { id, value } => capability::set_value(recipe, id, value),
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
