//! The develop form's scalar capability table: one row per control, the single
//! place a control's recipe field, its catalogue spelling and its reset value
//! are named. The RANGE is not repeated here — it is read from the core
//! catalogue (`advisor::catalogue::Control::range`), the same table the advisor
//! and the XMP writer read, so the form cannot state a band the engine does not
//! enforce. The renderer turns one row into one slider; the label (and its
//! translation) stays at the draw site.

use autoshade::advisor::catalogue::global_control;
use autoshade::recipe::EditRecipe;

/// How one row is drawn. `Companion` is the Lightroom companion encoding (a
/// stored 0 means "the sidecar said nothing", read and written through
/// [`EditRecipe::resolved`]/[`EditRecipe::set_resolved`]); the others are plain
/// recipe fields differing only in slider feel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SliderKind {
    Plain,
    Fine,
    Companion,
}

/// Which scalar the form edits — the handle the renderer's read/write pair is
/// dispatched on. One variant per table row.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
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
    PostCropVignette,
    CompanionPostCropMid,
    CompanionPostCropFeather,
    PostCropVignetteRound,
    CompanionPostCropStyle,
    PostCropVignetteHl,
    Grain,
    CompanionGrainSize,
    CompanionGrainRough,
    HdrMaxEv,
    SdrBlend,
    SdrBrightness,
    SdrContrast,
    SdrHighlights,
    SdrShadows,
    SdrWhites,
    SdrClarity,
    Straighten,
}

/// One row of the form: the field, the catalogue spelling it must resolve to,
/// the double-click reset value and how it is drawn.
pub(crate) struct Capability {
    pub id: CapabilityId,
    pub control: &'static str,
    pub reset: f32,
    pub kind: SliderKind,
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
    Capability { id: CapabilityId::Exposure, control: "exposure_ev", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Contrast, control: "contrast", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Highlights, control: "highlights", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Shadows, control: "shadows", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Whites, control: "whites", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Blacks, control: "blacks", reset: 0.0, kind: SliderKind::Plain },
];

/// Presence, in the engine's own order (texture → clarity → dehaze, then the
/// two colour dials).
pub(crate) const PRESENCE: &[Capability] = &[
    Capability { id: CapabilityId::Texture, control: "texture", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Clarity, control: "clarity", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Dehaze, control: "dehaze", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Vibrance, control: "vibrance", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::Saturation, control: "saturation", reset: 0.0, kind: SliderKind::Plain },
];

/// Post-crop vignetting, in the panel's row order.
pub(crate) const VIGNETTE: &[Capability] = &[
    Capability { id: CapabilityId::PostCropVignette, control: "post_crop_vignette", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::CompanionPostCropMid, control: "post_crop_vignette_mid", reset: 0.0, kind: SliderKind::Companion },
    Capability { id: CapabilityId::CompanionPostCropFeather, control: "post_crop_vignette_feather", reset: 0.0, kind: SliderKind::Companion },
    Capability { id: CapabilityId::PostCropVignetteRound, control: "post_crop_vignette_round", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::CompanionPostCropStyle, control: "post_crop_vignette_style", reset: 0.0, kind: SliderKind::Companion },
    Capability { id: CapabilityId::PostCropVignetteHl, control: "post_crop_vignette_hl", reset: 0.0, kind: SliderKind::Plain },
];

/// Grain, below the vignette rows.
pub(crate) const GRAIN: &[Capability] = &[
    Capability { id: CapabilityId::Grain, control: "grain", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::CompanionGrainSize, control: "grain_size", reset: 0.0, kind: SliderKind::Companion },
    Capability { id: CapabilityId::CompanionGrainRough, control: "grain_rough", reset: 0.0, kind: SliderKind::Companion },
];

/// HDR edit mode's own dial (above the SDR block).
pub(crate) const HDR_HEADROOM: &[Capability] = &[
    Capability { id: CapabilityId::HdrMaxEv, control: "hdr_max_ev", reset: 0.0, kind: SliderKind::Fine },
];

/// The SDR rendition's seven dials, in panel order.
pub(crate) const HDR_SDR: &[Capability] = &[
    Capability { id: CapabilityId::SdrBlend, control: "sdr_blend", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::SdrBrightness, control: "sdr_brightness", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::SdrContrast, control: "sdr_contrast", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::SdrHighlights, control: "sdr_highlights", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::SdrShadows, control: "sdr_shadows", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::SdrWhites, control: "sdr_whites", reset: 0.0, kind: SliderKind::Plain },
    Capability { id: CapabilityId::SdrClarity, control: "sdr_clarity", reset: 0.0, kind: SliderKind::Plain },
];

/// The crop section's one scalar row (the box itself is a canvas tool).
pub(crate) const CROP: &[Capability] = &[
    Capability { id: CapabilityId::Straighten, control: "straighten_deg", reset: 0.0, kind: SliderKind::Fine },
];

/// Read a plain/fine row's current value.
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
        CapabilityId::PostCropVignette => recipe.post_crop_vignette,
        CapabilityId::PostCropVignetteRound => recipe.post_crop_vignette_round,
        CapabilityId::PostCropVignetteHl => recipe.post_crop_vignette_hl,
        CapabilityId::Grain => recipe.grain,
        CapabilityId::HdrMaxEv => recipe.hdr_max_ev,
        CapabilityId::SdrBlend => recipe.sdr_blend,
        CapabilityId::SdrBrightness => recipe.sdr_brightness,
        CapabilityId::SdrContrast => recipe.sdr_contrast,
        CapabilityId::SdrHighlights => recipe.sdr_highlights,
        CapabilityId::SdrShadows => recipe.sdr_shadows,
        CapabilityId::SdrWhites => recipe.sdr_whites,
        CapabilityId::SdrClarity => recipe.sdr_clarity,
        CapabilityId::Straighten => recipe.straighten_deg,
        CapabilityId::CompanionPostCropMid
        | CapabilityId::CompanionPostCropFeather
        | CapabilityId::CompanionPostCropStyle
        | CapabilityId::CompanionGrainSize
        | CapabilityId::CompanionGrainRough => {
            unreachable!("companion rows are read through `EditRecipe::resolved`")
        }
    }
}

/// Write a plain/fine row's value back.
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
        CapabilityId::PostCropVignette => recipe.post_crop_vignette = value,
        CapabilityId::PostCropVignetteRound => recipe.post_crop_vignette_round = value,
        CapabilityId::PostCropVignetteHl => recipe.post_crop_vignette_hl = value,
        CapabilityId::Grain => recipe.grain = value,
        CapabilityId::HdrMaxEv => recipe.hdr_max_ev = value,
        CapabilityId::SdrBlend => recipe.sdr_blend = value,
        CapabilityId::SdrBrightness => recipe.sdr_brightness = value,
        CapabilityId::SdrContrast => recipe.sdr_contrast = value,
        CapabilityId::SdrHighlights => recipe.sdr_highlights = value,
        CapabilityId::SdrShadows => recipe.sdr_shadows = value,
        CapabilityId::SdrWhites => recipe.sdr_whites = value,
        CapabilityId::SdrClarity => recipe.sdr_clarity = value,
        CapabilityId::Straighten => recipe.straighten_deg = value,
        CapabilityId::CompanionPostCropMid
        | CapabilityId::CompanionPostCropFeather
        | CapabilityId::CompanionPostCropStyle
        | CapabilityId::CompanionGrainSize
        | CapabilityId::CompanionGrainRough => {
            unreachable!("companion rows are written through `EditRecipe::set_resolved`")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[&[Capability]] = &[TONE, PRESENCE, VIGNETTE, GRAIN, HDR_HEADROOM, HDR_SDR, CROP];

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
        assert_eq!(
            names(VIGNETTE),
            [
                "post_crop_vignette",
                "post_crop_vignette_mid",
                "post_crop_vignette_feather",
                "post_crop_vignette_round",
                "post_crop_vignette_style",
                "post_crop_vignette_hl",
            ],
        );
        assert_eq!(names(GRAIN), ["grain", "grain_size", "grain_rough"]);
        assert_eq!(names(HDR_HEADROOM), ["hdr_max_ev"]);
        assert_eq!(
            names(HDR_SDR),
            [
                "sdr_blend",
                "sdr_brightness",
                "sdr_contrast",
                "sdr_highlights",
                "sdr_shadows",
                "sdr_whites",
                "sdr_clarity",
            ],
        );
        assert_eq!(names(CROP), ["straighten_deg"]);
    }

    #[test]
    fn every_id_appears_once_and_only_once() {
        let mut ids: Vec<CapabilityId> = ALL.iter().flat_map(|t| t.iter().map(|c| c.id)).collect();
        let before = ids.len();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), before, "a capability id is in two tables");
    }

    #[test]
    fn the_read_and_write_halves_round_trip_each_plain_field() {
        let mut recipe = EditRecipe::default();
        for table in ALL {
            for cap in *table {
                if cap.kind == SliderKind::Companion {
                    continue;
                }
                set_value(&mut recipe, cap.id, 12.5);
                assert_eq!(value(&recipe, cap.id), 12.5, "`{}` does not round-trip", cap.control);
                set_value(&mut recipe, cap.id, cap.reset);
                assert_eq!(value(&recipe, cap.id), cap.reset);
            }
        }
    }

    #[test]
    fn companion_rows_are_read_and_written_through_the_explicit_zero_encoding() {
        let mut recipe = EditRecipe::default();
        let default_mid = recipe.resolved("post_crop_vignette_mid");
        assert_eq!(default_mid, 50.0, "an absent companion reads its Lightroom default");
        recipe.set_resolved("post_crop_vignette_mid", 80.0);
        assert_eq!(recipe.resolved("post_crop_vignette_mid"), 80.0);
        recipe.clear_resolved("post_crop_vignette_mid");
        assert_eq!(recipe.resolved("post_crop_vignette_mid"), default_mid);
    }
}
