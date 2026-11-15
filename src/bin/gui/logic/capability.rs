//! The develop form's scalar capability table: one row per control, the single
//! place a control's recipe field and its catalogue spelling are named. The
//! BAND and the reset are not repeated here — both come from the core
//! catalogue through `advisor::catalogue::form_control` (the top-level
//! `RECIPE_CONTROLS` rows the advisor and XMP writer read, plus the nested
//! `COLOR_GRADE_FIELDS`), so the form cannot state a band the engine does not
//! enforce nor a reset `EditRecipe::default` does not store. The renderer turns
//! one row into one slider; the label (and its translation) stays at the draw
//! site.

use autoshade::advisor::catalogue::form_control;
use autoshade::recipe::EditRecipe;

/// How one row is drawn. `Companion*` is the Lightroom companion encoding (a
/// stored 0 means "the sidecar said nothing", read and written through
/// [`EditRecipe::resolved`]/[`EditRecipe::set_resolved`]); the siblings are
/// plain recipe fields differing only in slider feel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SliderKind {
    Plain,
    Fine,
    CompanionInt,
    CompanionFine,
    /// A plain field whose slider carries one disclosure line in its tooltip.
    Hinted,
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
    SharpenRadius,
    SharpenDetail,
    SharpenMasking,
    NoiseReduction,
    NoiseDetail,
    NoiseContrast,
    ColorNoiseReduction,
    ColorNoiseDetail,
    ColorNoiseSmooth,
    PerspectiveVertical,
    PerspectiveHorizontal,
    PerspectiveRotate,
    PerspectiveScale,
    PerspectiveAspect,
    PerspectiveX,
    PerspectiveY,
    LensVignetting,
    LensVignettingMid,
    LensDistortion,
    LensProfileDistortionScale,
    LensProfileVignettingScale,
    CaRed,
    CaBlue,
    DefringePurple,
    DefringePurpleLo,
    DefringePurpleHi,
    DefringeGreen,
    DefringeGreenLo,
    DefringeGreenHi,
    CalShadowTint,
    CalRedHue,
    CalRedSat,
    CalGreenHue,
    CalGreenSat,
    CalBlueHue,
    CalBlueSat,
    WhiteBalanceTint,
    GradeBlending,
    GradeBalance,
}

impl CapabilityId {
    #[cfg(test)]
    fn is_companion(self) -> bool {
        matches!(
            self,
            CapabilityId::CompanionPostCropMid
                | CapabilityId::CompanionPostCropFeather
                | CapabilityId::CompanionPostCropStyle
                | CapabilityId::CompanionGrainSize
                | CapabilityId::CompanionGrainRough
                | CapabilityId::SharpenRadius
                | CapabilityId::SharpenDetail
                | CapabilityId::NoiseDetail
                | CapabilityId::ColorNoiseDetail
                | CapabilityId::ColorNoiseSmooth
        )
    }
}

/// One row of the form: the field, the catalogue spelling it must resolve to,
/// and how it is drawn. The BAND and the double-click reset come from the core
/// catalogue's `form_control` lookup — a row never restates either.
pub(crate) struct Capability {
    pub id: CapabilityId,
    pub control: &'static str,
    pub kind: SliderKind,
}

impl Capability {
    /// The band the slider draws plus the double-click reset, both from the
    /// catalogue's one `form_controls` answer (top-level scalars and the
    /// nested `color_grade.<field>` rows). `None` = the catalogue has no band
    /// for this spelling; the headless table tests reject such a row.
    pub(crate) fn band_and_reset(&self) -> Option<((f32, f32), f32)> {
        let (band, reset) = form_control(self.control)?;
        Some((band, reset.unwrap_or(0.0)))
    }
}

macro_rules! cap {
    ($id:ident, $control:literal, $kind:ident) => {
        Capability { id: CapabilityId::$id, control: $control, kind: SliderKind::$kind }
    };
}

/// Lightroom's Basic-panel tone controls, in the panel's order.
pub(crate) const TONE: &[Capability] = &[
    cap!(Exposure, "exposure_ev", Plain),
    cap!(Contrast, "contrast", Plain),
    cap!(Highlights, "highlights", Plain),
    cap!(Shadows, "shadows", Plain),
    cap!(Whites, "whites", Plain),
    cap!(Blacks, "blacks", Plain),
];

/// Presence, in the engine's own order (texture → clarity → dehaze, then the
/// two colour dials).
pub(crate) const PRESENCE: &[Capability] = &[
    cap!(Texture, "texture", Plain),
    cap!(Clarity, "clarity", Plain),
    cap!(Dehaze, "dehaze", Plain),
    cap!(Vibrance, "vibrance", Plain),
    cap!(Saturation, "saturation", Plain),
];

/// Post-crop vignetting, in the panel's row order.
pub(crate) const VIGNETTE: &[Capability] = &[
    cap!(PostCropVignette, "post_crop_vignette", Plain),
    cap!(CompanionPostCropMid, "post_crop_vignette_mid", CompanionInt),
    cap!(CompanionPostCropFeather, "post_crop_vignette_feather", CompanionInt),
    cap!(PostCropVignetteRound, "post_crop_vignette_round", Plain),
    cap!(CompanionPostCropStyle, "post_crop_vignette_style", CompanionInt),
    cap!(PostCropVignetteHl, "post_crop_vignette_hl", Plain),
];

/// Grain, below the vignette rows.
pub(crate) const GRAIN: &[Capability] = &[
    cap!(Grain, "grain", Plain),
    cap!(CompanionGrainSize, "grain_size", CompanionInt),
    cap!(CompanionGrainRough, "grain_rough", CompanionInt),
];

/// HDR edit mode's own dial (above the SDR block).
pub(crate) const HDR_HEADROOM: &[Capability] = &[cap!(HdrMaxEv, "hdr_max_ev", Fine)];

/// The SDR rendition's seven dials, in panel order.
pub(crate) const HDR_SDR: &[Capability] = &[
    cap!(SdrBlend, "sdr_blend", Plain),
    cap!(SdrBrightness, "sdr_brightness", Plain),
    cap!(SdrContrast, "sdr_contrast", Plain),
    cap!(SdrHighlights, "sdr_highlights", Plain),
    cap!(SdrShadows, "sdr_shadows", Plain),
    cap!(SdrWhites, "sdr_whites", Plain),
    cap!(SdrClarity, "sdr_clarity", Plain),
];

/// Lens correction's manual rows, above the profile-strength block.
pub(crate) const LENS_MANUAL: &[Capability] = &[
    cap!(LensVignetting, "lens_vignette", Plain),
    cap!(LensVignettingMid, "lens_vignette_mid", Plain),
    cap!(LensDistortion, "lens_distortion", Plain),
];

/// The profile's own two strengths (a Hinted row: the tooltip says what 0 and
/// 100 mean).
pub(crate) const LENS_PROFILE: &[Capability] = &[
    cap!(LensProfileDistortionScale, "lens_profile_distortion_scale", Hinted),
    cap!(LensProfileVignettingScale, "lens_profile_vignetting_scale", Hinted),
];

/// The manual lateral CA pair.
pub(crate) const LENS_CA: &[Capability] = &[
    cap!(CaRed, "ca_r", Plain),
    cap!(CaBlue, "ca_b", Plain),
];

/// Adobe's de-fringe block: two amounts plus the hue windows, whose neutrals
/// are NOT zero (30/70 and 40/60).
pub(crate) const LENS_DEFRINGE: &[Capability] = &[
    cap!(DefringePurple, "defringe_purple", Plain),
    cap!(DefringePurpleLo, "defringe_purple_lo", Hinted),
    cap!(DefringePurpleHi, "defringe_purple_hi", Hinted),
    cap!(DefringeGreen, "defringe_green", Plain),
    cap!(DefringeGreenLo, "defringe_green_lo", Hinted),
    cap!(DefringeGreenHi, "defringe_green_hi", Hinted),
];

/// Calibration's shadows tint (its own captioned group in the panel).
pub(crate) const CAL_TINT: &[Capability] = &[cap!(CalShadowTint, "cal_shadow_tint", Plain)];

/// Calibration's red primary pair: hue then saturation, the panel drawing the
/// group label between the tables.
pub(crate) const CAL_RED: &[Capability] = &[
    cap!(CalRedHue, "cal_red_hue", Plain),
    cap!(CalRedSat, "cal_red_sat", Plain),
];

/// Calibration's green primary pair.
pub(crate) const CAL_GREEN: &[Capability] = &[
    cap!(CalGreenHue, "cal_green_hue", Plain),
    cap!(CalGreenSat, "cal_green_sat", Plain),
];

/// Calibration's blue primary pair.
pub(crate) const CAL_BLUE: &[Capability] = &[
    cap!(CalBlueHue, "cal_blue_hue", Plain),
    cap!(CalBlueSat, "cal_blue_sat", Plain),
];

/// The white balance's Tint half (the Temp slider and the Custom-WB checkbox
/// stay custom: one is a log Kelvin anchor, the other an enable switch).
pub(crate) const WB_TINT: &[Capability] = &[cap!(WhiteBalanceTint, "tint", Plain)];

/// Colour grading's two WHOLE-GRADE dials, under the wheel block. The wheels
/// themselves stay a custom widget (`develop_widgets.rs` cannot draw a wheel
/// from a scalar row), but these two are plain scalars of the nested object, so
/// their band and reset come from the catalogue like every other form row.
pub(crate) const GRADE_GLOBALS: &[Capability] = &[
    cap!(GradeBlending, "color_grade.blending", Plain),
    cap!(GradeBalance, "color_grade.balance", Plain),
];

/// The crop section's one scalar row (the box itself is a canvas tool).
pub(crate) const CROP: &[Capability] = &[cap!(Straighten, "straighten_deg", Fine)];

/// Detail's sharpening rows below the amount (whose RAW/baked default is the
/// custom `sharpening_slider` arm, so it stays out of this table).
pub(crate) const SHARPEN: &[Capability] = &[
    cap!(SharpenRadius, "sharpen_radius", CompanionFine),
    cap!(SharpenDetail, "sharpen_detail", CompanionInt),
    cap!(SharpenMasking, "sharpen_mask", Plain),
];

/// Luminance noise reduction's rows.
pub(crate) const NOISE_LUMA: &[Capability] = &[
    cap!(NoiseReduction, "noise_reduction", Plain),
    cap!(NoiseDetail, "nr_detail", CompanionInt),
    cap!(NoiseContrast, "nr_contrast", Plain),
];

/// Colour noise reduction's rows.
pub(crate) const NOISE_COLOR: &[Capability] = &[
    cap!(ColorNoiseReduction, "color_nr", Plain),
    cap!(ColorNoiseDetail, "color_nr_detail", CompanionInt),
    cap!(ColorNoiseSmooth, "color_nr_smooth", CompanionInt),
];

/// Transform's projective sliders, in panel order. The Upright mode, its notes
/// and the constrain-crop checkbox stay custom.
pub(crate) const TRANSFORM: &[Capability] = &[
    cap!(PerspectiveVertical, "perspective_vertical", Plain),
    cap!(PerspectiveHorizontal, "perspective_horizontal", Plain),
    cap!(PerspectiveRotate, "perspective_rotate", Plain),
    cap!(PerspectiveScale, "perspective_scale", Plain),
    cap!(PerspectiveAspect, "perspective_aspect", Plain),
    cap!(PerspectiveX, "perspective_x", Plain),
    cap!(PerspectiveY, "perspective_y", Plain),
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
        CapabilityId::SharpenMasking => recipe.sharpen_mask,
        CapabilityId::NoiseReduction => recipe.noise_reduction,
        CapabilityId::NoiseContrast => recipe.nr_contrast,
        CapabilityId::ColorNoiseReduction => recipe.color_nr,
        CapabilityId::PerspectiveVertical => recipe.perspective_vertical,
        CapabilityId::PerspectiveHorizontal => recipe.perspective_horizontal,
        CapabilityId::PerspectiveRotate => recipe.perspective_rotate,
        CapabilityId::PerspectiveScale => recipe.perspective_scale,
        CapabilityId::PerspectiveAspect => recipe.perspective_aspect,
        CapabilityId::PerspectiveX => recipe.perspective_x,
        CapabilityId::PerspectiveY => recipe.perspective_y,
        CapabilityId::LensVignetting => recipe.lens_vignette,
        CapabilityId::LensVignettingMid => recipe.lens_vignette_mid,
        CapabilityId::LensDistortion => recipe.lens_distortion,
        CapabilityId::LensProfileDistortionScale => recipe.lens_profile_distortion_scale,
        CapabilityId::LensProfileVignettingScale => recipe.lens_profile_vignetting_scale,
        CapabilityId::CaRed => recipe.ca_r,
        CapabilityId::CaBlue => recipe.ca_b,
        CapabilityId::DefringePurple => recipe.defringe_purple,
        CapabilityId::DefringePurpleLo => recipe.defringe_purple_lo,
        CapabilityId::DefringePurpleHi => recipe.defringe_purple_hi,
        CapabilityId::DefringeGreen => recipe.defringe_green,
        CapabilityId::DefringeGreenLo => recipe.defringe_green_lo,
        CapabilityId::DefringeGreenHi => recipe.defringe_green_hi,
        CapabilityId::CalShadowTint => recipe.cal_shadow_tint,
        CapabilityId::CalRedHue => recipe.cal_red_hue,
        CapabilityId::CalRedSat => recipe.cal_red_sat,
        CapabilityId::CalGreenHue => recipe.cal_green_hue,
        CapabilityId::CalGreenSat => recipe.cal_green_sat,
        CapabilityId::CalBlueHue => recipe.cal_blue_hue,
        CapabilityId::CalBlueSat => recipe.cal_blue_sat,
        CapabilityId::WhiteBalanceTint => recipe.tint,
        CapabilityId::GradeBlending => recipe.color_grade.blending,
        CapabilityId::GradeBalance => recipe.color_grade.balance,
        other => unreachable!("companion row {other:?} is read through `EditRecipe::resolved`"),
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
        CapabilityId::SharpenMasking => recipe.sharpen_mask = value,
        CapabilityId::NoiseReduction => recipe.noise_reduction = value,
        CapabilityId::NoiseContrast => recipe.nr_contrast = value,
        CapabilityId::ColorNoiseReduction => recipe.color_nr = value,
        CapabilityId::PerspectiveVertical => recipe.perspective_vertical = value,
        CapabilityId::PerspectiveHorizontal => recipe.perspective_horizontal = value,
        CapabilityId::PerspectiveRotate => recipe.perspective_rotate = value,
        CapabilityId::PerspectiveScale => recipe.perspective_scale = value,
        CapabilityId::PerspectiveAspect => recipe.perspective_aspect = value,
        CapabilityId::PerspectiveX => recipe.perspective_x = value,
        CapabilityId::PerspectiveY => recipe.perspective_y = value,
        CapabilityId::LensVignetting => recipe.lens_vignette = value,
        CapabilityId::LensVignettingMid => recipe.lens_vignette_mid = value,
        CapabilityId::LensDistortion => recipe.lens_distortion = value,
        CapabilityId::LensProfileDistortionScale => recipe.lens_profile_distortion_scale = value,
        CapabilityId::LensProfileVignettingScale => recipe.lens_profile_vignetting_scale = value,
        CapabilityId::CaRed => recipe.ca_r = value,
        CapabilityId::CaBlue => recipe.ca_b = value,
        CapabilityId::DefringePurple => recipe.defringe_purple = value,
        CapabilityId::DefringePurpleLo => recipe.defringe_purple_lo = value,
        CapabilityId::DefringePurpleHi => recipe.defringe_purple_hi = value,
        CapabilityId::DefringeGreen => recipe.defringe_green = value,
        CapabilityId::DefringeGreenLo => recipe.defringe_green_lo = value,
        CapabilityId::DefringeGreenHi => recipe.defringe_green_hi = value,
        CapabilityId::CalShadowTint => recipe.cal_shadow_tint = value,
        CapabilityId::CalRedHue => recipe.cal_red_hue = value,
        CapabilityId::CalRedSat => recipe.cal_red_sat = value,
        CapabilityId::CalGreenHue => recipe.cal_green_hue = value,
        CapabilityId::CalGreenSat => recipe.cal_green_sat = value,
        CapabilityId::CalBlueHue => recipe.cal_blue_hue = value,
        CapabilityId::CalBlueSat => recipe.cal_blue_sat = value,
        CapabilityId::WhiteBalanceTint => recipe.tint = value,
        CapabilityId::GradeBlending => recipe.color_grade.blending = value,
        CapabilityId::GradeBalance => recipe.color_grade.balance = value,
        other => unreachable!("companion row {other:?} is written through `EditRecipe::set_resolved`"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALL: &[&[Capability]] = &[
        TONE,
        PRESENCE,
        VIGNETTE,
        GRAIN,
        HDR_HEADROOM,
        HDR_SDR,
        CROP,
        SHARPEN,
        NOISE_LUMA,
        NOISE_COLOR,
        TRANSFORM,
        LENS_MANUAL,
        LENS_PROFILE,
        LENS_CA,
        LENS_DEFRINGE,
        CAL_TINT,
        CAL_RED,
        CAL_GREEN,
        CAL_BLUE,
        WB_TINT,
        GRADE_GLOBALS,
    ];

    #[test]
    fn every_row_resolves_to_a_catalogue_band_and_reset() {
        for table in ALL {
            for cap in *table {
                let ((lo, hi), _) =
                    cap.band_and_reset().unwrap_or_else(|| panic!("`{}` has no catalogue band", cap.control));
                assert!(lo < hi, "`{}` band is empty: {lo}..{hi}", cap.control);
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
        assert_eq!(names(SHARPEN), ["sharpen_radius", "sharpen_detail", "sharpen_mask"]);
        assert_eq!(names(NOISE_LUMA), ["noise_reduction", "nr_detail", "nr_contrast"]);
        assert_eq!(names(NOISE_COLOR), ["color_nr", "color_nr_detail", "color_nr_smooth"]);
        assert_eq!(
            names(TRANSFORM),
            [
                "perspective_vertical",
                "perspective_horizontal",
                "perspective_rotate",
                "perspective_scale",
                "perspective_aspect",
                "perspective_x",
                "perspective_y",
            ],
        );
        assert_eq!(names(LENS_MANUAL), ["lens_vignette", "lens_vignette_mid", "lens_distortion"]);
        assert_eq!(
            names(LENS_PROFILE),
            ["lens_profile_distortion_scale", "lens_profile_vignetting_scale"],
        );
        assert_eq!(names(LENS_CA), ["ca_r", "ca_b"]);
        assert_eq!(
            names(LENS_DEFRINGE),
            [
                "defringe_purple",
                "defringe_purple_lo",
                "defringe_purple_hi",
                "defringe_green",
                "defringe_green_lo",
                "defringe_green_hi",
            ],
        );
        assert_eq!(names(CAL_TINT), ["cal_shadow_tint"]);
        assert_eq!(names(CAL_RED), ["cal_red_hue", "cal_red_sat"]);
        assert_eq!(names(CAL_GREEN), ["cal_green_hue", "cal_green_sat"]);
        assert_eq!(names(CAL_BLUE), ["cal_blue_hue", "cal_blue_sat"]);
        assert_eq!(names(WB_TINT), ["tint"]);
        assert_eq!(names(GRADE_GLOBALS), ["color_grade.blending", "color_grade.balance"]);
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
    fn the_table_kind_and_the_id_agree_about_companions() {
        for table in ALL {
            for cap in *table {
                let companion_kind = matches!(cap.kind, SliderKind::CompanionInt | SliderKind::CompanionFine);
                assert_eq!(cap.id.is_companion(), companion_kind, "row `{}` disagrees with itself", cap.control);
            }
        }
    }

    #[test]
    fn the_read_and_write_halves_round_trip_each_plain_field() {
        let mut recipe = EditRecipe::default();
        for table in ALL {
            for cap in *table {
                if cap.id.is_companion() {
                    continue;
                }
                set_value(&mut recipe, cap.id, 12.5);
                assert_eq!(value(&recipe, cap.id), 12.5, "`{}` does not round-trip", cap.control);
                let reset = cap.band_and_reset().unwrap().1;
                set_value(&mut recipe, cap.id, reset);
                assert_eq!(value(&recipe, cap.id), reset);
            }
        }
    }

    /// The two colour-grading rows are nested fields, not top-level recipe
    /// controls, so this pins their catalogue answer explicitly: the band
    /// `ColorGrade::clamp` enforces and the neutral the default recipe stores
    /// (blending 50, not the panel's old literal 50 written by hand twice).
    #[test]
    fn the_grading_rows_take_their_band_and_reset_from_the_catalogue() {
        assert_eq!(
            GRADE_GLOBALS[0].band_and_reset(),
            Some(((0.0, 100.0), 50.0)),
            "blending",
        );
        assert_eq!(
            GRADE_GLOBALS[1].band_and_reset(),
            Some(((-100.0, 100.0), 0.0)),
            "balance",
        );
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
