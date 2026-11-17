//! The DOMAIN command layer: the mask list's verbs as data intents, applied in
//! one place. A scalar control's write is not here — the control registry
//! (`controls.rs`) owns it, because a control's behaviour is polymorphic and a
//! `Command` should stay a plain, serialisable intent, not a function pointer.

use autoshade::recipe::EditRecipe;

/// One edit the GUI asks the document to make.
#[derive(Clone, Debug)]
pub(crate) enum Command {
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
            if let Some(m) = recipe.masks.get_mut(index)
                && component < m.components.len() {
                    m.components.remove(component);
                }
        }
        Command::InsertMask { after, mask } => {
            // The recipe's own cap (64) is a domain invariant, so it is
            // enforced HERE as well as by the panel's toast.
            if recipe.masks.len() < 64 {
                recipe.masks.insert((after + 1).min(recipe.masks.len()), *mask);
            }
        }
    }
}
