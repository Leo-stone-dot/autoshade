//! The develop form's scalar capability table: one row per control, and the
//! row owns its own behaviour — the label resource, the read and the write.
//!
//! The BAND and the reset still come from the core catalogue through
//! `advisor::catalogue::form_control` (the top-level `RECIPE_CONTROLS` rows and
//! the nested `COLOR_GRADE_FIELDS`), so the form cannot state a band the engine
//! does not enforce. Everything ELSE about a control is in its row: adding one
//! is a row here plus its core catalogue row, never an edit to an enum or a
//! central `match` in the renderer, the command layer or the label dispatch.
//!
//! A row is one of three shapes:
//!   * [`cap!`] — a plain field: `label`, read and write closures;
//!   * [`cap_hinted!`] — a plain field whose slider carries a disclosure line;
//!   * [`capc!`] — a Lightroom COMPANION (a stored 0 means "the sidecar said
//!     nothing"), read and written through [`EditRecipe::resolved`] /
//!     [`EditRecipe::set_resolved`] by `Command::SetCompanion`.

use autoshade::advisor::catalogue::form_control;
use autoshade::recipe::EditRecipe;
use autoshade::text::generated::*;

/// How one row is drawn. `Companion*` is the Lightroom companion encoding; the
/// siblings are plain recipe fields differing only in slider feel.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SliderKind {
    Plain,
    Fine,
    CompanionInt,
    CompanionFine,
    /// A plain field whose slider carries one disclosure line in its tooltip.
    Hinted,
}

/// How a row reads and writes its value. `Companion` rows carry no closures:
/// their encoding is the `resolved` pair, which needs the control NAME, so the
/// renderer's stable two-arm match on the access kind handles them.
pub(crate) enum Access {
    Plain { get: fn(&EditRecipe) -> f32, set: fn(&mut EditRecipe, f32) },
    Companion,
}

/// One row of the form.
pub(crate) struct Capability {
    /// The recipe field / catalogue spelling the band and reset resolve through.
    pub control: &'static str,
    /// The translated label's resource id (`develop_forms` renders it).
    pub label: autoshade::text::Key,
    pub kind: SliderKind,
    pub access: Access,
    /// The one disclosure line a [`SliderKind::Hinted`] row carries.
    pub hint: Option<autoshade::text::Key>,
}

impl Capability {
    /// The band the slider draws plus the double-click reset, both from the
    /// catalogue's one `form_controls` answer. `None` = no band for this
    /// spelling; the table tests reject such a row.
    pub(crate) fn band_and_reset(&self) -> Option<((f32, f32), f32)> {
        let (band, reset) = form_control(self.control)?;
        Some((band, reset.unwrap_or(0.0)))
    }

    /// The plain read/write pair, or `None` for a companion row.
    pub(crate) fn plain(&self) -> Option<(fn(&EditRecipe) -> f32, fn(&mut EditRecipe, f32))> {
        match self.access {
            Access::Plain { get, set } => Some((get, set)),
            Access::Companion => None,
        }
    }
}

macro_rules! cap {
    ($control:literal, $label:expr, $kind:ident, $get:expr, $set:expr) => {
        Capability {
            control: $control,
            label: $label,
            kind: SliderKind::$kind,
            access: Access::Plain { get: $get, set: $set },
            hint: None,
        }
    };
}

macro_rules! cap_hinted {
    ($control:literal, $label:expr, $kind:ident, $hint:expr, $get:expr, $set:expr) => {
        Capability {
            control: $control,
            label: $label,
            kind: SliderKind::$kind,
            access: Access::Plain { get: $get, set: $set },
            hint: Some($hint),
        }
    };
}

macro_rules! capc {
    ($control:literal, $label:expr, $kind:ident) => {
        Capability {
            control: $control,
            label: $label,
            kind: SliderKind::$kind,
            access: Access::Companion,
            hint: None,
        }
    };
}

/// TONE.
pub(crate) const TONE: &[Capability] = &[
    cap!("exposure_ev", K_DEVELOP_EXPOSURE, Plain, |r| r.exposure_ev, |r, v| r.exposure_ev = v),
    cap!("contrast", K_DEVELOP_CONTRAST, Plain, |r| r.contrast, |r, v| r.contrast = v),
    cap!("highlights", K_DEVELOP_HIGHLIGHTS, Plain, |r| r.highlights, |r, v| r.highlights = v),
    cap!("shadows", K_DEVELOP_SHADOWS, Plain, |r| r.shadows, |r, v| r.shadows = v),
    cap!("whites", K_DEVELOP_WHITES, Plain, |r| r.whites, |r, v| r.whites = v),
    cap!("blacks", K_DEVELOP_BLACKS, Plain, |r| r.blacks, |r, v| r.blacks = v),
];

/// PRESENCE.
pub(crate) const PRESENCE: &[Capability] = &[
    cap!("texture", K_DEVELOP_TEXTURE, Plain, |r| r.texture, |r, v| r.texture = v),
    cap!("clarity", K_DEVELOP_CLARITY, Plain, |r| r.clarity, |r, v| r.clarity = v),
    cap!("dehaze", K_DEVELOP_DEHAZE, Plain, |r| r.dehaze, |r, v| r.dehaze = v),
    cap!("vibrance", K_DEVELOP_VIBRANCE, Plain, |r| r.vibrance, |r, v| r.vibrance = v),
    cap!("saturation", K_DEVELOP_SATURATION, Plain, |r| r.saturation, |r, v| r.saturation = v),
];

/// VIGNETTE.
pub(crate) const VIGNETTE: &[Capability] = &[
    cap!("post_crop_vignette", K_DEVELOP_VIGNETTE_AMOUNT, Plain, |r| r.post_crop_vignette, |r, v| r.post_crop_vignette = v),
    capc!("post_crop_vignette_mid", K_DEVELOP_MIDPOINT, CompanionInt),
    capc!("post_crop_vignette_feather", K_DEVELOP_VIGNETTE_FEATHER, CompanionInt),
    cap!("post_crop_vignette_round", K_DEVELOP_VIGNETTE_ROUNDNESS, Plain, |r| r.post_crop_vignette_round, |r, v| r.post_crop_vignette_round = v),
    capc!("post_crop_vignette_style", K_DEVELOP_VIGNETTE_STYLE, CompanionInt),
    cap!("post_crop_vignette_hl", K_DEVELOP_VIGNETTE_HIGHLIGHTS, Plain, |r| r.post_crop_vignette_hl, |r, v| r.post_crop_vignette_hl = v),
];

/// GRAIN.
pub(crate) const GRAIN: &[Capability] = &[
    cap!("grain", K_DEVELOP_GRAIN_AMOUNT, Plain, |r| r.grain, |r, v| r.grain = v),
    capc!("grain_size", K_DEVELOP_GRAIN_SIZE, CompanionInt),
    capc!("grain_rough", K_DEVELOP_GRAIN_ROUGHNESS, CompanionInt),
];

/// HDR_HEADROOM.
pub(crate) const HDR_HEADROOM: &[Capability] = &[
    cap!("hdr_max_ev", K_DEVELOP_HEADROOM_STOPS, Fine, |r| r.hdr_max_ev, |r, v| r.hdr_max_ev = v),
];

/// HDR_SDR.
pub(crate) const HDR_SDR: &[Capability] = &[
    cap!("sdr_blend", K_DEVELOP_BLEND, Plain, |r| r.sdr_blend, |r, v| r.sdr_blend = v),
    cap!("sdr_brightness", K_DEVELOP_BRIGHTNESS, Plain, |r| r.sdr_brightness, |r, v| r.sdr_brightness = v),
    cap!("sdr_contrast", K_DEVELOP_CONTRAST, Plain, |r| r.sdr_contrast, |r, v| r.sdr_contrast = v),
    cap!("sdr_highlights", K_DEVELOP_HIGHLIGHTS, Plain, |r| r.sdr_highlights, |r, v| r.sdr_highlights = v),
    cap!("sdr_shadows", K_DEVELOP_SHADOWS, Plain, |r| r.sdr_shadows, |r, v| r.sdr_shadows = v),
    cap!("sdr_whites", K_DEVELOP_WHITES, Plain, |r| r.sdr_whites, |r, v| r.sdr_whites = v),
    cap!("sdr_clarity", K_DEVELOP_CLARITY, Plain, |r| r.sdr_clarity, |r, v| r.sdr_clarity = v),
];

/// LENS_MANUAL.
pub(crate) const LENS_MANUAL: &[Capability] = &[
    cap!("lens_vignette", K_DEVELOP_LENS_VIGNETTING, Plain, |r| r.lens_vignette, |r, v| r.lens_vignette = v),
    cap!("lens_vignette_mid", K_DEVELOP_MIDPOINT, Plain, |r| r.lens_vignette_mid, |r, v| r.lens_vignette_mid = v),
    cap!("lens_distortion", K_DEVELOP_DISTORTION, Plain, |r| r.lens_distortion, |r, v| r.lens_distortion = v),
];

/// LENS_PROFILE.
pub(crate) const LENS_PROFILE: &[Capability] = &[
    cap_hinted!("lens_profile_distortion_scale", K_DEVELOP_DISTORTION_AMOUNT, Hinted, K_DEVELOP_100_EXACTLY_WHAT_THE_PROFILE_SAYS_0_SWITCHES_THA, |r| r.lens_profile_distortion_scale, |r, v| r.lens_profile_distortion_scale = v),
    cap_hinted!("lens_profile_vignetting_scale", K_DEVELOP_VIGNETTING_AMOUNT, Hinted, K_DEVELOP_100_EXACTLY_WHAT_THE_PROFILE_SAYS_0_SWITCHES_THA, |r| r.lens_profile_vignetting_scale, |r, v| r.lens_profile_vignetting_scale = v),
];

/// LENS_CA.
pub(crate) const LENS_CA: &[Capability] = &[
    cap!("ca_r", K_DEVELOP_RED_CYAN, Plain, |r| r.ca_r, |r, v| r.ca_r = v),
    cap!("ca_b", K_DEVELOP_BLUE_YELLOW, Plain, |r| r.ca_b, |r, v| r.ca_b = v),
];

/// LENS_DEFRINGE.
pub(crate) const LENS_DEFRINGE: &[Capability] = &[
    cap!("defringe_purple", K_DEVELOP_PURPLE_AMOUNT, Plain, |r| r.defringe_purple, |r, v| r.defringe_purple = v),
    cap_hinted!("defringe_purple_lo", K_DEVELOP_PURPLE_HUE_LOW, Hinted, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I, |r| r.defringe_purple_lo, |r, v| r.defringe_purple_lo = v),
    cap_hinted!("defringe_purple_hi", K_DEVELOP_PURPLE_HUE_HIGH, Hinted, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I, |r| r.defringe_purple_hi, |r, v| r.defringe_purple_hi = v),
    cap!("defringe_green", K_DEVELOP_GREEN_AMOUNT, Plain, |r| r.defringe_green, |r, v| r.defringe_green = v),
    cap_hinted!("defringe_green_lo", K_DEVELOP_GREEN_HUE_LOW, Hinted, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I, |r| r.defringe_green_lo, |r, v| r.defringe_green_lo = v),
    cap_hinted!("defringe_green_hi", K_DEVELOP_GREEN_HUE_HIGH, Hinted, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I, |r| r.defringe_green_hi, |r, v| r.defringe_green_hi = v),
];

/// CAL_TINT.
pub(crate) const CAL_TINT: &[Capability] = &[
    cap!("cal_shadow_tint", K_DEVELOP_TINT, Plain, |r| r.cal_shadow_tint, |r, v| r.cal_shadow_tint = v),
];

/// CAL_RED.
pub(crate) const CAL_RED: &[Capability] = &[
    cap!("cal_red_hue", K_DEVELOP_HUE, Plain, |r| r.cal_red_hue, |r, v| r.cal_red_hue = v),
    cap!("cal_red_sat", K_DEVELOP_SATURATION, Plain, |r| r.cal_red_sat, |r, v| r.cal_red_sat = v),
];

/// CAL_GREEN.
pub(crate) const CAL_GREEN: &[Capability] = &[
    cap!("cal_green_hue", K_DEVELOP_HUE, Plain, |r| r.cal_green_hue, |r, v| r.cal_green_hue = v),
    cap!("cal_green_sat", K_DEVELOP_SATURATION, Plain, |r| r.cal_green_sat, |r, v| r.cal_green_sat = v),
];

/// CAL_BLUE.
pub(crate) const CAL_BLUE: &[Capability] = &[
    cap!("cal_blue_hue", K_DEVELOP_HUE, Plain, |r| r.cal_blue_hue, |r, v| r.cal_blue_hue = v),
    cap!("cal_blue_sat", K_DEVELOP_SATURATION, Plain, |r| r.cal_blue_sat, |r, v| r.cal_blue_sat = v),
];

/// WB_TINT.
pub(crate) const WB_TINT: &[Capability] = &[
    cap!("tint", K_DEVELOP_TINT, Plain, |r| r.tint, |r, v| r.tint = v),
];

/// GRADE_GLOBALS.
pub(crate) const GRADE_GLOBALS: &[Capability] = &[
    cap!("color_grade.blending", K_DEVELOP_BLENDING, Plain, |r| r.color_grade.blending, |r, v| r.color_grade.blending = v),
    cap!("color_grade.balance", K_DEVELOP_BALANCE, Plain, |r| r.color_grade.balance, |r, v| r.color_grade.balance = v),
];

/// CROP.
pub(crate) const CROP: &[Capability] = &[
    cap!("straighten_deg", K_DEVELOP_STRAIGHTEN, Fine, |r| r.straighten_deg, |r, v| r.straighten_deg = v),
];

/// SHARPEN.
pub(crate) const SHARPEN: &[Capability] = &[
    capc!("sharpen_radius", K_DEVELOP_SHARPEN_RADIUS, CompanionFine),
    capc!("sharpen_detail", K_DEVELOP_SHARPEN_DETAIL, CompanionInt),
    cap!("sharpen_mask", K_DEVELOP_SHARPEN_MASKING, Plain, |r| r.sharpen_mask, |r, v| r.sharpen_mask = v),
];

/// NOISE_LUMA.
pub(crate) const NOISE_LUMA: &[Capability] = &[
    cap!("noise_reduction", K_DEVELOP_NOISE_REDUCTION, Plain, |r| r.noise_reduction, |r, v| r.noise_reduction = v),
    capc!("nr_detail", K_DEVELOP_NOISE_DETAIL, CompanionInt),
    cap!("nr_contrast", K_DEVELOP_NOISE_CONTRAST, Plain, |r| r.nr_contrast, |r, v| r.nr_contrast = v),
];

/// NOISE_COLOR.
pub(crate) const NOISE_COLOR: &[Capability] = &[
    cap!("color_nr", K_DEVELOP_COLOUR_NOISE_REDUCTION, Plain, |r| r.color_nr, |r, v| r.color_nr = v),
    capc!("color_nr_detail", K_DEVELOP_COLOUR_NOISE_DETAIL, CompanionInt),
    capc!("color_nr_smooth", K_DEVELOP_COLOUR_NOISE_SMOOTHNESS, CompanionInt),
];

/// TRANSFORM.
pub(crate) const TRANSFORM: &[Capability] = &[
    cap!("perspective_vertical", K_DEVELOP_VERTICAL, Plain, |r| r.perspective_vertical, |r, v| r.perspective_vertical = v),
    cap!("perspective_horizontal", K_DEVELOP_HORIZONTAL, Plain, |r| r.perspective_horizontal, |r, v| r.perspective_horizontal = v),
    cap!("perspective_rotate", K_DEVELOP_ROTATE, Plain, |r| r.perspective_rotate, |r, v| r.perspective_rotate = v),
    cap!("perspective_scale", K_DEVELOP_TRANSFORM_SCALE, Plain, |r| r.perspective_scale, |r, v| r.perspective_scale = v),
    cap!("perspective_aspect", K_DEVELOP_ASPECT, Plain, |r| r.perspective_aspect, |r, v| r.perspective_aspect = v),
    cap!("perspective_x", K_DEVELOP_X_OFFSET, Plain, |r| r.perspective_x, |r, v| r.perspective_x = v),
    cap!("perspective_y", K_DEVELOP_Y_OFFSET, Plain, |r| r.perspective_y, |r, v| r.perspective_y = v),
];

/// Every table, for the tests and the command layer's walk.
#[allow(dead_code)] // read by the command tests, not by the renderer
pub(crate) const TABLES: &[&[Capability]] = &[TONE, PRESENCE, VIGNETTE, GRAIN, HDR_HEADROOM, HDR_SDR, LENS_MANUAL, LENS_PROFILE, LENS_CA, LENS_DEFRINGE, CAL_TINT, CAL_RED, CAL_GREEN, CAL_BLUE, WB_TINT, GRADE_GLOBALS, CROP, SHARPEN, NOISE_LUMA, NOISE_COLOR, TRANSFORM];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_row_resolves_to_a_catalogue_band_and_reset() {
        for table in TABLES {
            for cap in *table {
                let ((lo, hi), _) = cap
                    .band_and_reset()
                    .unwrap_or_else(|| panic!("`{}` has no catalogue band", cap.control));
                assert!(lo < hi, "`{}` band is empty: {lo}..{hi}", cap.control);
            }
        }
    }

    #[test]
    fn the_tables_are_the_known_controls_in_panel_order() {
        let names = |table: &[Capability]| table.iter().map(|c| c.control).collect::<Vec<_>>();
        assert_eq!(names(TONE), ["exposure_ev", "contrast", "highlights", "shadows", "whites", "blacks"]);
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
        assert_eq!(names(LENS_PROFILE), ["lens_profile_distortion_scale", "lens_profile_vignetting_scale"]);
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
    fn every_control_appears_once_and_only_once() {
        let mut controls: Vec<&str> = TABLES.iter().flat_map(|t| t.iter().map(|c| c.control)).collect();
        let before = controls.len();
        controls.sort();
        controls.dedup();
        assert_eq!(controls.len(), before, "a control is in two tables");
    }

    #[test]
    fn the_access_kind_and_the_slider_kind_agree_about_companions() {
        for table in TABLES {
            for cap in *table {
                let companion_kind = matches!(
                    cap.kind,
                    SliderKind::CompanionInt | SliderKind::CompanionFine
                );
                assert_eq!(
                    cap.plain().is_none(),
                    companion_kind,
                    "row `{}` disagrees with itself",
                    cap.control
                );
            }
        }
    }

    #[test]
    fn every_row_has_a_label_and_a_hint_only_where_it_draws_one() {
        for table in TABLES {
            for cap in *table {
                assert!(!autoshade::text::name(cap.label).is_empty(), "`{}` has no label", cap.control);
                assert_eq!(
                    cap.hint.is_some(),
                    cap.kind == SliderKind::Hinted,
                    "`{}` hint disagrees with its kind",
                    cap.control
                );
            }
        }
    }

    #[test]
    fn the_read_and_write_halves_round_trip_each_plain_field() {
        let mut recipe = EditRecipe::default();
        for table in TABLES {
            for cap in *table {
                let Some((get, set)) = cap.plain() else { continue };
                set(&mut recipe, 12.5);
                assert_eq!(get(&recipe), 12.5, "`{}` does not round-trip", cap.control);
                let reset = cap.band_and_reset().unwrap().1;
                set(&mut recipe, reset);
                assert_eq!(get(&recipe), reset);
            }
        }
    }

    /// The two colours-grading rows are nested fields, not top-level recipe
    /// controls: their catalogue answer is the band `ColorGrade::clamp`
    /// enforces and the neutral the default recipe stores (blending 50).
    #[test]
    fn the_grading_rows_take_their_band_and_reset_from_the_catalogue() {
        assert_eq!(GRADE_GLOBALS[0].band_and_reset(), Some(((0.0, 100.0), 50.0)), "blending");
        assert_eq!(GRADE_GLOBALS[1].band_and_reset(), Some(((-100.0, 100.0), 0.0)), "balance");
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
