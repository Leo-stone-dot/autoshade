//! The develop form's renderer: one capability row -> one slider, in table
//! order. The row (field, catalogue spelling, reset) comes from
//! `logic::capability`; this file owns only the widget mapping and the label
//! dispatch — every label stays a literal `tr()` call so the i18n audit checks
//! every key.

use crate::*;
use crate::logic::capability::{self, Capability, CapabilityId, SliderKind};
use crate::logic::command::Command;

/// The translated label of one row.
fn label(lang: Lang, id: CapabilityId) -> &'static str {
    match id {
        CapabilityId::Exposure => tr(lang, K_DEVELOP_EXPOSURE),
        CapabilityId::Contrast => tr(lang, K_DEVELOP_CONTRAST),
        CapabilityId::Highlights => tr(lang, K_DEVELOP_HIGHLIGHTS),
        CapabilityId::Shadows => tr(lang, K_DEVELOP_SHADOWS),
        CapabilityId::Whites => tr(lang, K_DEVELOP_WHITES),
        CapabilityId::Blacks => tr(lang, K_DEVELOP_BLACKS),
        CapabilityId::Texture => tr(lang, K_DEVELOP_TEXTURE),
        CapabilityId::Clarity => tr(lang, K_DEVELOP_CLARITY),
        CapabilityId::Dehaze => tr(lang, K_DEVELOP_DEHAZE),
        CapabilityId::Vibrance => tr(lang, K_DEVELOP_VIBRANCE),
        CapabilityId::Saturation => tr(lang, K_DEVELOP_SATURATION),
        CapabilityId::PostCropVignette => tr(lang, K_DEVELOP_VIGNETTE_AMOUNT),
        CapabilityId::CompanionPostCropMid => tr(lang, K_DEVELOP_MIDPOINT),
        CapabilityId::CompanionPostCropFeather => tr(lang, K_DEVELOP_VIGNETTE_FEATHER),
        CapabilityId::PostCropVignetteRound => tr(lang, K_DEVELOP_VIGNETTE_ROUNDNESS),
        CapabilityId::CompanionPostCropStyle => tr(lang, K_DEVELOP_VIGNETTE_STYLE),
        CapabilityId::PostCropVignetteHl => tr(lang, K_DEVELOP_VIGNETTE_HIGHLIGHTS),
        CapabilityId::Grain => tr(lang, K_DEVELOP_GRAIN_AMOUNT),
        CapabilityId::CompanionGrainSize => tr(lang, K_DEVELOP_GRAIN_SIZE),
        CapabilityId::CompanionGrainRough => tr(lang, K_DEVELOP_GRAIN_ROUGHNESS),
        CapabilityId::HdrMaxEv => tr(lang, K_DEVELOP_HEADROOM_STOPS),
        CapabilityId::SdrBlend => tr(lang, K_DEVELOP_BLEND),
        CapabilityId::SdrBrightness => tr(lang, K_DEVELOP_BRIGHTNESS),
        CapabilityId::SdrContrast => tr(lang, K_DEVELOP_CONTRAST),
        CapabilityId::SdrHighlights => tr(lang, K_DEVELOP_HIGHLIGHTS),
        CapabilityId::SdrShadows => tr(lang, K_DEVELOP_SHADOWS),
        CapabilityId::SdrWhites => tr(lang, K_DEVELOP_WHITES),
        CapabilityId::SdrClarity => tr(lang, K_DEVELOP_CLARITY),
        CapabilityId::Straighten => tr(lang, K_DEVELOP_STRAIGHTEN),
        CapabilityId::SharpenRadius => tr(lang, K_DEVELOP_SHARPEN_RADIUS),
        CapabilityId::SharpenDetail => tr(lang, K_DEVELOP_SHARPEN_DETAIL),
        CapabilityId::SharpenMasking => tr(lang, K_DEVELOP_SHARPEN_MASKING),
        CapabilityId::NoiseReduction => tr(lang, K_DEVELOP_NOISE_REDUCTION),
        CapabilityId::NoiseDetail => tr(lang, K_DEVELOP_NOISE_DETAIL),
        CapabilityId::NoiseContrast => tr(lang, K_DEVELOP_NOISE_CONTRAST),
        CapabilityId::ColorNoiseReduction => tr(lang, K_DEVELOP_COLOUR_NOISE_REDUCTION),
        CapabilityId::ColorNoiseDetail => tr(lang, K_DEVELOP_COLOUR_NOISE_DETAIL),
        CapabilityId::ColorNoiseSmooth => tr(lang, K_DEVELOP_COLOUR_NOISE_SMOOTHNESS),
        CapabilityId::PerspectiveVertical => tr(lang, K_DEVELOP_VERTICAL),
        CapabilityId::PerspectiveHorizontal => tr(lang, K_DEVELOP_HORIZONTAL),
        CapabilityId::PerspectiveRotate => tr(lang, K_DEVELOP_ROTATE),
        CapabilityId::PerspectiveScale => tr(lang, K_DEVELOP_TRANSFORM_SCALE),
        CapabilityId::PerspectiveAspect => tr(lang, K_DEVELOP_ASPECT),
        CapabilityId::PerspectiveX => tr(lang, K_DEVELOP_X_OFFSET),
        CapabilityId::PerspectiveY => tr(lang, K_DEVELOP_Y_OFFSET),
        CapabilityId::LensVignetting => tr(lang, K_DEVELOP_LENS_VIGNETTING),
        CapabilityId::LensVignettingMid => tr(lang, K_DEVELOP_MIDPOINT),
        CapabilityId::LensDistortion => tr(lang, K_DEVELOP_DISTORTION),
        CapabilityId::LensProfileDistortionScale => tr(lang, K_DEVELOP_DISTORTION_AMOUNT),
        CapabilityId::LensProfileVignettingScale => tr(lang, K_DEVELOP_VIGNETTING_AMOUNT),
        CapabilityId::CaRed => tr(lang, K_DEVELOP_RED_CYAN),
        CapabilityId::CaBlue => tr(lang, K_DEVELOP_BLUE_YELLOW),
        CapabilityId::DefringePurple => tr(lang, K_DEVELOP_PURPLE_AMOUNT),
        CapabilityId::DefringePurpleLo => tr(lang, K_DEVELOP_PURPLE_HUE_LOW),
        CapabilityId::DefringePurpleHi => tr(lang, K_DEVELOP_PURPLE_HUE_HIGH),
        CapabilityId::DefringeGreen => tr(lang, K_DEVELOP_GREEN_AMOUNT),
        CapabilityId::DefringeGreenLo => tr(lang, K_DEVELOP_GREEN_HUE_LOW),
        CapabilityId::DefringeGreenHi => tr(lang, K_DEVELOP_GREEN_HUE_HIGH),
        CapabilityId::CalShadowTint => tr(lang, K_DEVELOP_TINT),
        CapabilityId::CalRedHue | CapabilityId::CalGreenHue | CapabilityId::CalBlueHue => tr(lang, K_DEVELOP_HUE),
        CapabilityId::CalRedSat | CapabilityId::CalGreenSat | CapabilityId::CalBlueSat => tr(lang, K_DEVELOP_SATURATION),
        CapabilityId::WhiteBalanceTint => tr(lang, K_DEVELOP_TINT),
        CapabilityId::GradeBlending => tr(lang, K_DEVELOP_BLENDING),
        CapabilityId::GradeBalance => tr(lang, K_DEVELOP_BALANCE),
    }
}

/// The one tooltip line a Hinted row carries (the disclosure the control needs
/// because its number alone does not say what the ends mean).
fn hint(lang: Lang, id: CapabilityId) -> Option<&'static str> {
    match id {
        CapabilityId::LensProfileDistortionScale | CapabilityId::LensProfileVignettingScale => {
            Some(tr(lang, K_DEVELOP_100_EXACTLY_WHAT_THE_PROFILE_SAYS_0_SWITCHES_THA))
        }
        CapabilityId::DefringePurpleLo
        | CapabilityId::DefringePurpleHi
        | CapabilityId::DefringeGreenLo
        | CapabilityId::DefringeGreenHi => Some(tr(lang, K_DEVELOP_A_HUE_WINDOW_CORRECTS_NOTHING_UNTIL_ITS_AMOUNT_I)),
        _ => None,
    }
}

/// Render one capability table into `ui`, in table order. A row whose catalogue
/// band is missing is skipped (the headless table test forbids such a row).
/// Returns true when any row changed this frame.
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
        let text = label(lang, cap.id);
        let row_changed = match cap.kind {
            SliderKind::CompanionInt | SliderKind::CompanionFine => {
                let feel = if cap.kind == SliderKind::CompanionFine { SliderFeel::Fine } else { SliderFeel::Int };
                match AutoShadeApp::companion_slider_edit(ui, lang, text, recipe, cap.control, min, max, feel) {
                    Some(value) => {
                        out.push(Command::SetCompanion { name: cap.control, value });
                        true
                    }
                    None => false,
                }
            }
            SliderKind::Hinted => {
                let mut current = capability::value(recipe, cap.id);
                let dirty = AutoShadeApp::slider_hinted(
                    ui,
                    lang,
                    text,
                    &mut current,
                    min,
                    max,
                    reset,
                    hint(lang, cap.id).unwrap_or(""),
                );
                if dirty {
                    out.push(Command::SetCapability { id: cap.id, value: current });
                }
                dirty
            }
            SliderKind::Plain | SliderKind::Fine => {
                let mut current = capability::value(recipe, cap.id);
                let dirty = if cap.kind == SliderKind::Fine {
                    AutoShadeApp::slider_fine(ui, lang, text, &mut current, min, max, reset)
                } else {
                    AutoShadeApp::slider(ui, lang, text, &mut current, min, max, reset)
                };
                if dirty {
                    out.push(Command::SetCapability { id: cap.id, value: current });
                }
                dirty
            }
        };
        changed |= row_changed;
    }
    changed
}
