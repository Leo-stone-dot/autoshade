//! The develop form's scalar capability table: one row per control, the single
//! place a control's recipe field, its catalogue spelling and its reset value
//! are named. The RANGE is not repeated here — it is read from the core
//! catalogue (`advisor::catalogue::Control::range`), the same table the advisor
//! and the XMP writer read, so the form cannot state a band the engine does not
//! enforce. The renderer turns one row into one slider; the label (and its
//! translation) stays at the draw site.

use autoshade::advisor::catalogue::global_control;
use autoshade::recipe::EditRecipe;

/// Which scalar the form edits — the handle the renderer's read/write pair is
/// dispatched on. One variant per table row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CapabilityId {
    Exposure,
    Contrast,
    Highlights,
    Shadows,
    Whites,
    Blacks,
    Texture,
    Clarity,
    Dehaze,
    Vibrance,
    Saturation,
}

/// One row of the form: the field, the catalogue spelling it must resolve to,
/// and the double-click reset value.
pub(crate) struct Capability {
    pub id: CapabilityId,
    pub control: &'static str,
    pub reset: f32,
}

impl Capability {
    /// The band the slider draws, from the catalogue (None = the catalogue has
    /// no band for this spelling; the unit test below rejects such a row).
    pub(crate) fn range(&self) -> Option<(f32, f32)> {
        global_control(self.control).and_then(|c| c.range)
    }
}

/// Lightroom's Basic-panel tone controls, in the panel's order.
pub(crate) const TONE: &[Capability] = &[
    Capability { id: CapabilityId::Exposure, control: "exposure_ev", reset: 0.0 },
    Capability { id: CapabilityId::Contrast, control: "contrast", reset: 0.0 },
    Capability { id: CapabilityId::Highlights, control: "highlights", reset: 0.0 },
    Capability { id: CapabilityId::Shadows, control: "shadows", reset: 0.0 },
    Capability { id: CapabilityId::Whites, control: "whites", reset: 0.0 },
    Capability { id: CapabilityId::Blacks, control: "blacks", reset: 0.0 },
];

/// Presence, in the engine's own order (texture → clarity → dehaze, then the
/// two colour dials).
pub(crate) const PRESENCE: &[Capability] = &[
    Capability { id: CapabilityId::Texture, control: "texture", reset: 0.0 },
    Capability { id: CapabilityId::Clarity, control: "clarity", reset: 0.0 },
    Capability { id: CapabilityId::Dehaze, control: "dehaze", reset: 0.0 },
    Capability { id: CapabilityId::Vibrance, control: "vibrance", reset: 0.0 },
    Capability { id: CapabilityId::Saturation, control: "saturation", reset: 0.0 },
];

/// Read the control's current value.
pub(crate) fn value(recipe: &EditRecipe, id: CapabilityId) -> f32 {
    match id {
        CapabilityId::Exposure => recipe.exposure_ev,
        CapabilityId::Contrast => recipe.contrast,
        CapabilityId::Highlights => recipe.highlights,
        CapabilityId::Shadows => recipe.shadows,
        CapabilityId::Whites => recipe.whites,
        CapabilityId::Blacks => recipe.blacks,
        CapabilityId::Texture => recipe.texture,
        CapabilityId::Clarity => recipe.clarity,
        CapabilityId::Dehaze => recipe.dehaze,
        CapabilityId::Vibrance => recipe.vibrance,
        CapabilityId::Saturation => recipe.saturation,
    }
}

/// Write the control's value back.
pub(crate) fn set_value(recipe: &mut EditRecipe, id: CapabilityId, value: f32) {
    match id {
        CapabilityId::Exposure => recipe.exposure_ev = value,
        CapabilityId::Contrast => recipe.contrast = value,
        CapabilityId::Highlights => recipe.highlights = value,
        CapabilityId::Shadows => recipe.shadows = value,
        CapabilityId::Whites => recipe.whites = value,
        CapabilityId::Blacks => recipe.blacks = value,
        CapabilityId::Texture => recipe.texture = value,
        CapabilityId::Clarity => recipe.clarity = value,
        CapabilityId::Dehaze => recipe.dehaze = value,
        CapabilityId::Vibrance => recipe.vibrance = value,
        CapabilityId::Saturation => recipe.saturation = value,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[&[Capability]] = &[TONE, PRESENCE];

    #[test]
    fn every_row_resolves_to_a_catalogue_band() {
        for table in ALL {
            for cap in *table {
                let range = cap.range().unwrap_or_else(|| panic!("`{}` has no catalogue band", cap.control));
                assert!(range.0 < range.1, "`{}` band is empty: {range:?}", cap.control);
            }
        }
    }

    #[test]
    fn the_tables_are_the_known_controls_in_panel_order() {
        let names = |table: &[Capability]| table.iter().map(|c| c.control).collect::<Vec<_>>();
        assert_eq!(
            names(TONE),
            ["exposure_ev", "contrast", "highlights", "shadows", "whites", "blacks"],
        );
        assert_eq!(names(PRESENCE), ["texture", "clarity", "dehaze", "vibrance", "saturation"]);
    }

    #[test]
    fn every_id_appears_once_and_only_once() {
        let mut ids: Vec<CapabilityId> = ALL.iter().flat_map(|t| t.iter().map(|c| c.id)).collect();
        let before = ids.len();
        ids.sort_by_key(|id| *id as u8);
        ids.dedup();
        assert_eq!(ids.len(), before, "a capability id is in two tables");
    }

    #[test]
    fn the_read_and_write_halves_round_trip_each_field() {
        let mut recipe = EditRecipe::default();
        for table in ALL {
            for cap in *table {
                set_value(&mut recipe, cap.id, 12.5);
                assert_eq!(value(&recipe, cap.id), 12.5, "`{}` does not round-trip", cap.control);
                set_value(&mut recipe, cap.id, cap.reset);
                assert_eq!(value(&recipe, cap.id), cap.reset);
            }
        }
    }
}
