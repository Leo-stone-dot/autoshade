//! The develop panel's section table: display order, the control families whose
//! activity lights the section's ●, and the section's OWN draw.
//!
//! A row here is the whole registration: `develop_panel` walks the table and
//! calls `(section.draw)(self, ui, active)` — adding a section is a row plus its
//! draw function in the section's module, never an enum variant and a central
//! `match` in the panel. The table lives in the view layer because a row owns
//! view behaviour; the headless logic layer never sees it.

use crate::*;
use autoshade::advisor::catalogue::{family_is_active, CONTROL_FAMILIES};
use autoshade::recipe::EditRecipe;

/// Where a section's draw lives: still on the command context, or already a
/// PanelView method. The table is the single place either is named.
pub(crate) type SectionDraw = fn(&mut PanelView<'_>, &mut egui::Ui, bool) -> bool;

/// One row of the panel.
pub(crate) struct Section {
    /// The control families the section's ● reads (empty = no dot of its own).
    pub families: &'static [&'static str],
    /// The section's draw, in one uniform shape so the panel needs no match.
    pub draw: SectionDraw,
}

fn panel_dev_presence(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_presence(ui, active)
}

fn panel_dev_calibration(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_calibration(ui, active)
}

fn panel_dev_effects(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_effects(ui, active)
}

fn panel_dev_hdr(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_hdr(ui, active)
}

fn panel_dev_transform(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_transform(ui, active)
}

fn panel_dev_lens(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_lens(ui, active)
}

fn panel_dev_curves(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_curves(ui, active)
}

fn panel_dev_grading(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_grading(ui, active)
}

fn panel_dev_stack(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_stack(ui, active)
}

fn panel_dev_lens_blur(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_lens_blur(ui, active)
}

fn panel_dev_detail(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_detail(ui, active)
}

fn panel_dev_hsl(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_hsl(ui, active)
}

fn panel_dev_versions(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_versions(ui, active)
}

fn panel_dev_tone_wb(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_tone_wb(ui, active)
}

fn panel_dev_crop(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_crop(ui, active)
}

fn panel_dev_export(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_export(ui, active)
}

fn panel_dev_masks(view: &mut PanelView<'_>, ui: &mut egui::Ui, active: bool) -> bool {
    view.dev_masks(ui, active)
}

/// The panel's sections, in the order it draws them.
pub(crate) const SECTIONS: &[Section] = &[
    Section {
        families: &["tone", "white_balance"],
        draw: (panel_dev_tone_wb),
    },
    Section {
        families: &["presence"],
        draw: (panel_dev_presence),
    },
    Section {
        families: &["curves", "parametric"],
        draw: (panel_dev_curves),
    },
    Section {
        families: &["hsl", "black_white", "point_color"],
        draw: (panel_dev_hsl),
    },
    Section {
        families: &["color_grade"],
        draw: (panel_dev_grading),
    },
    Section {
        families: &["calibration"],
        draw: (panel_dev_calibration),
    },
    Section {
        families: &["detail", "detail_effects"],
        draw: (panel_dev_detail),
    },
    Section {
        families: &["effects"],
        draw: (panel_dev_effects),
    },
    // Lens Blur returns ACTIONS (its build is a worker verb): the row's draw
    // applies them, and the section has no ● of its own and no family.
    Section {
        families: &[],
        draw: (panel_dev_lens_blur),
    },
    Section {
        families: &["lens", "lens_effects"],
        draw: (panel_dev_lens),
    },
    Section {
        families: &["transform"],
        draw: (panel_dev_transform),
    },
    // Crop/straighten live in the `framing` family; the section derives that ●
    // itself (the same rule as the others).
    Section {
        families: &["framing"],
        draw: (panel_dev_crop),
    },
    Section {
        families: &[],
        draw: (panel_dev_masks),
    },
    Section {
        families: &[],
        draw: (panel_dev_versions),
    },
    Section {
        families: &[],
        draw: (panel_dev_stack),
    },
    Section {
        families: &["hdr"],
        draw: (panel_dev_hdr),
    },
    Section {
        families: &[],
        draw: (panel_dev_export),
    },
];

/// Is any of `section`'s families active on this recipe (its ● predicate)?
pub(crate) fn section_active(recipe: &EditRecipe, section: &Section) -> bool {
    section.families.iter().any(|name| {
        CONTROL_FAMILIES
            .iter()
            .find(|f| f.name == *name)
            .is_some_and(|f| family_is_active(f, recipe))
    })
}
