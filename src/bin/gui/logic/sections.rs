//! The develop panel's section table: display order, the control families whose
//! activity lights the section's ●, and whether the section is a capability
//! form or a custom widget. `develop_panel` walks this table; adding a section
//! is one row here plus its draw arm, not another line in the panel body.

use autoshade::advisor::catalogue::{family_is_active, CONTROL_FAMILIES};
use autoshade::recipe::EditRecipe;

/// The panel's sections, in display order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionId {
    ToneWb,
    Presence,
    Curves,
    Hsl,
    Grading,
    Calibration,
    Detail,
    Effects,
    LensBlur,
    Lens,
    Transform,
    Crop,
    Masks,
    Versions,
    Stack,
    Hdr,
    Export,
}

/// The widgets a capability table cannot draw — the custom half of the panel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CustomWidget {
    /// The global tone-curve editor (`curve_editor_for`).
    ToneCurve,
    /// The colour mixer grid (HSL bands / B&W mix) with its Point Color rows.
    ColourMixer,
    /// The three colour-grading wheels and the global wheel.
    GradeWheels,
    /// The local-adjustments column (`panels/develop_masks.rs`).
    MaskPanel,
}

/// How a section draws.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SectionKind {
    /// Capability-form rows (the scalar tables) inside the section's fold.
    Form,
    /// A registered custom widget.
    Custom(CustomWidget),
}

/// One row of the panel: the id, the families its ● reads, and how it draws.
pub(crate) struct Section {
    pub id: SectionId,
    pub families: &'static [&'static str],
    /// The registration contract: what the panel draws for this row. Read by
    /// the table's tests; the draw match is the panel's half of it.
    #[allow(dead_code)]
    pub kind: SectionKind,
}

/// The panel's sections, in the order it draws them. The stack fold, the
/// lens-blur actions and the export fold are rows too: they are part of the
/// panel's composition even though two of them are verbs rather than forms.
pub(crate) const SECTIONS: &[Section] = &[
    Section { id: SectionId::ToneWb, families: &["tone", "white_balance"], kind: SectionKind::Form },
    Section { id: SectionId::Presence, families: &["presence"], kind: SectionKind::Form },
    Section { id: SectionId::Curves, families: &["curves", "parametric"], kind: SectionKind::Custom(CustomWidget::ToneCurve) },
    Section { id: SectionId::Hsl, families: &["hsl", "black_white", "point_color"], kind: SectionKind::Custom(CustomWidget::ColourMixer) },
    Section { id: SectionId::Grading, families: &["color_grade"], kind: SectionKind::Custom(CustomWidget::GradeWheels) },
    Section { id: SectionId::Calibration, families: &["calibration"], kind: SectionKind::Form },
    Section { id: SectionId::Detail, families: &["detail", "detail_effects"], kind: SectionKind::Form },
    Section { id: SectionId::Effects, families: &["effects"], kind: SectionKind::Form },
    // Lens Blur returns ACTIONS (its build is a worker verb): the panel applies
    // them, so it has no ● of its own and no family.
    Section { id: SectionId::LensBlur, families: &[], kind: SectionKind::Form },
    Section { id: SectionId::Lens, families: &["lens", "lens_effects"], kind: SectionKind::Form },
    Section { id: SectionId::Transform, families: &["transform"], kind: SectionKind::Form },
    // Crop/straighten live in the `framing` family; the panel derives that ●
    // itself (the same rule as the others).
    Section { id: SectionId::Crop, families: &["framing"], kind: SectionKind::Form },
    Section { id: SectionId::Masks, families: &[], kind: SectionKind::Custom(CustomWidget::MaskPanel) },
    Section { id: SectionId::Versions, families: &[], kind: SectionKind::Form },
    Section { id: SectionId::Stack, families: &[], kind: SectionKind::Form },
    Section { id: SectionId::Hdr, families: &["hdr"], kind: SectionKind::Form },
    Section { id: SectionId::Export, families: &[], kind: SectionKind::Form },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_section_family_is_a_registered_control_family() {
        for section in SECTIONS {
            for family in section.families {
                assert!(
                    CONTROL_FAMILIES.iter().any(|f| f.name == *family),
                    "section {:?} names the unknown family `{family}`",
                    section.id,
                );
            }
        }
    }

    #[test]
    fn every_section_id_appears_exactly_once_in_display_order() {
        let mut ids: Vec<SectionId> = SECTIONS.iter().map(|s| s.id).collect();
        let before = ids.len();
        ids.sort_by_key(|id| *id as u8);
        ids.dedup();
        assert_eq!(ids.len(), before, "a section id is listed twice");
        assert_eq!(SECTIONS.len(), 17, "the display order is the panel's; a new row changes it deliberately");
    }

    #[test]
    fn the_custom_sections_register_a_custom_widget() {
        let custom: Vec<SectionId> = SECTIONS
            .iter()
            .filter(|s| matches!(s.kind, SectionKind::Custom(_)))
            .map(|s| s.id)
            .collect();
        assert_eq!(
            custom,
            [SectionId::Curves, SectionId::Hsl, SectionId::Grading, SectionId::Masks],
            "the custom widgets are the curve editor, the mixer, the wheels and the mask panel",
        );
    }

    #[test]
    fn the_tone_section_lights_on_either_of_its_two_families() {
        let tone = SECTIONS.iter().find(|s| s.id == SectionId::ToneWb).unwrap();
        let mut recipe = EditRecipe::default();
        assert!(!section_active(&recipe, tone), "a stock recipe lights nothing");
        recipe.exposure_ev = 0.5;
        assert!(section_active(&recipe, tone), "a tone edit lights the section");
        let mut wb = EditRecipe::default();
        wb.temperature_k = Some(6000.0);
        assert!(section_active(&wb, tone), "a WB edit lights it too");
    }
}
