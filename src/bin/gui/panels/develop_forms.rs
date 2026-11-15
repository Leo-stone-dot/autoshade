//! The develop form's renderer: one capability row -> one slider, in table
//! order. The row owns its label resource and its read/write pair
//! (`logic::capability`); this file owns only the widget mapping. Adding a
//! control is a row there, not a line here or in a central label/`match`.

use crate::logic::capability::{Capability, SliderKind};
use crate::logic::command::Command;
use crate::*;

/// Render one capability table into `ui`, in table order. A row whose catalogue
/// band is missing is skipped (the table test forbids such a row). Returns true
/// when any row changed this frame.
pub(crate) fn render_rows(
    ui: &mut egui::Ui,
    lang: Lang,
    recipe: &mut autoshade::recipe::EditRecipe,
    table: &[Capability],
    out: &mut Vec<Command>,
) -> bool {
    let mut changed = false;
    for cap in table {
        let Some(((min, max), reset)) = cap.band_and_reset() else { continue };
        let text = tr(lang, cap.label);
        let Some((get, write)) = cap.plain() else {
            // A COMPANION row: the resolved encoding (a stored 0 means "the
            // sidecar said nothing"), so the slider edits `resolved` and the
            // command restores that encoding.
            let feel = if cap.kind == SliderKind::CompanionFine { SliderFeel::Fine } else { SliderFeel::Int };
            if let Some(value) =
                AutoShadeApp::companion_slider_edit(ui, lang, text, recipe, cap.control, min, max, feel)
            {
                out.push(Command::SetCompanion { name: cap.control, value });
                changed = true;
            }
            continue;
        };
        let mut current = get(recipe);
        let dirty = match cap.kind {
            SliderKind::Hinted => AutoShadeApp::slider_hinted(
                ui,
                lang,
                text,
                &mut current,
                min,
                max,
                reset,
                cap.hint.map_or("", |hint| tr(lang, hint)),
            ),
            SliderKind::Fine => AutoShadeApp::slider_fine(ui, lang, text, &mut current, min, max, reset),
            _ => AutoShadeApp::slider(ui, lang, text, &mut current, min, max, reset),
        };
        if dirty {
            out.push(Command::SetCapability { write, value: current });
        }
        changed |= dirty;
    }
    changed
}
