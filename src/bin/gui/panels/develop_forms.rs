//! The develop form's renderer: one capability row -> one slider, in table
//! order. The row (field, catalogue spelling, reset) comes from
//! `logic::capability`; this file owns only the widget mapping and the label
//! dispatch — every label stays a literal `tr()` call so the i18n audit checks
//! every key.

use crate::*;
use crate::logic::capability::{self, Capability, CapabilityId, SliderKind};

/// The translated label of one row.
fn label(lang: Lang, id: CapabilityId) -> &'static str {
    match id {
        CapabilityId::Exposure => tr(lang, "Exposure"),
        CapabilityId::Contrast => tr(lang, "Contrast"),
        CapabilityId::Highlights => tr(lang, "Highlights"),
        CapabilityId::Shadows => tr(lang, "Shadows"),
        CapabilityId::Whites => tr(lang, "Whites"),
        CapabilityId::Blacks => tr(lang, "Blacks"),
        CapabilityId::Texture => tr(lang, "Texture"),
        CapabilityId::Clarity => tr(lang, "Clarity"),
        CapabilityId::Dehaze => tr(lang, "Dehaze"),
        CapabilityId::Vibrance => tr(lang, "Vibrance"),
        CapabilityId::Saturation => tr(lang, "Saturation"),
        CapabilityId::PostCropVignette => tr(lang, "Vignette amount"),
        CapabilityId::CompanionPostCropMid => tr(lang, "Midpoint"),
        CapabilityId::CompanionPostCropFeather => tr(lang, "Vignette feather"),
        CapabilityId::PostCropVignetteRound => tr(lang, "Vignette roundness"),
        CapabilityId::CompanionPostCropStyle => tr(lang, "Vignette style"),
        CapabilityId::PostCropVignetteHl => tr(lang, "Vignette highlights"),
        CapabilityId::Grain => tr(lang, "Grain amount"),
        CapabilityId::CompanionGrainSize => tr(lang, "Grain size"),
        CapabilityId::CompanionGrainRough => tr(lang, "Grain roughness"),
        CapabilityId::HdrMaxEv => tr(lang, "Headroom (stops)"),
        CapabilityId::SdrBlend => tr(lang, "Blend"),
        CapabilityId::SdrBrightness => tr(lang, "Brightness"),
        CapabilityId::SdrContrast => tr(lang, "Contrast"),
        CapabilityId::SdrHighlights => tr(lang, "Highlights"),
        CapabilityId::SdrShadows => tr(lang, "Shadows"),
        CapabilityId::SdrWhites => tr(lang, "Whites"),
        CapabilityId::SdrClarity => tr(lang, "Clarity"),
        CapabilityId::Straighten => tr(lang, "Straighten (°)"),
        CapabilityId::SharpenRadius => tr(lang, "Sharpen radius"),
        CapabilityId::SharpenDetail => tr(lang, "Sharpen detail"),
        CapabilityId::SharpenMasking => tr(lang, "Sharpen masking"),
        CapabilityId::NoiseReduction => tr(lang, "Noise Reduction"),
        CapabilityId::NoiseDetail => tr(lang, "Noise detail"),
        CapabilityId::NoiseContrast => tr(lang, "Noise contrast"),
        CapabilityId::ColorNoiseReduction => tr(lang, "Colour noise reduction"),
        CapabilityId::ColorNoiseDetail => tr(lang, "Colour noise detail"),
        CapabilityId::ColorNoiseSmooth => tr(lang, "Colour noise smoothness"),
        CapabilityId::PerspectiveVertical => tr(lang, "Vertical"),
        CapabilityId::PerspectiveHorizontal => tr(lang, "Horizontal"),
        CapabilityId::PerspectiveRotate => tr(lang, "Rotate (°)"),
        CapabilityId::PerspectiveScale => tr(lang, "Transform scale"),
        CapabilityId::PerspectiveAspect => tr(lang, "Aspect"),
        CapabilityId::PerspectiveX => tr(lang, "X offset"),
        CapabilityId::PerspectiveY => tr(lang, "Y offset"),
        CapabilityId::LensVignetting => tr(lang, "Lens vignetting"),
        CapabilityId::LensVignettingMid => tr(lang, "Midpoint"),
        CapabilityId::LensDistortion => tr(lang, "Distortion"),
        CapabilityId::LensProfileDistortionScale => tr(lang, "Distortion amount"),
        CapabilityId::LensProfileVignettingScale => tr(lang, "Vignetting amount"),
        CapabilityId::CaRed => tr(lang, "Red / cyan"),
        CapabilityId::CaBlue => tr(lang, "Blue / yellow"),
        CapabilityId::DefringePurple => tr(lang, "Purple amount"),
        CapabilityId::DefringePurpleLo => tr(lang, "Purple hue low"),
        CapabilityId::DefringePurpleHi => tr(lang, "Purple hue high"),
        CapabilityId::DefringeGreen => tr(lang, "Green amount"),
        CapabilityId::DefringeGreenLo => tr(lang, "Green hue low"),
        CapabilityId::DefringeGreenHi => tr(lang, "Green hue high"),
    }
}

/// The one tooltip line a Hinted row carries (the disclosure the control needs
/// because its number alone does not say what the ends mean).
fn hint(lang: Lang, id: CapabilityId) -> Option<&'static str> {
    match id {
        CapabilityId::LensProfileDistortionScale | CapabilityId::LensProfileVignettingScale => {
            Some(tr(lang, "100 = exactly what the profile says; 0 switches that component off."))
        }
        CapabilityId::DefringePurpleLo
        | CapabilityId::DefringePurpleHi
        | CapabilityId::DefringeGreenLo
        | CapabilityId::DefringeGreenHi => Some(tr(lang, "A hue window corrects nothing until its Amount is above 0.")),
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
) -> bool {
    let mut changed = false;
    for cap in table {
        let Some((min, max)) = cap.range() else { continue };
        let text = label(lang, cap.id);
        let row_changed = match cap.kind {
            SliderKind::CompanionInt | SliderKind::CompanionFine => AutoShadeApp::companion_slider(
                ui,
                lang,
                text,
                recipe,
                cap.control,
                min,
                max,
                if cap.kind == SliderKind::CompanionFine { SliderFeel::Fine } else { SliderFeel::Int },
            ),
            SliderKind::Hinted => {
                let mut current = capability::value(recipe, cap.id);
                let dirty = AutoShadeApp::slider_hinted(
                    ui,
                    lang,
                    text,
                    &mut current,
                    min,
                    max,
                    cap.reset,
                    hint(lang, cap.id).unwrap_or(""),
                );
                if dirty {
                    capability::set_value(recipe, cap.id, current);
                }
                dirty
            }
            SliderKind::Plain | SliderKind::Fine => {
                let mut current = capability::value(recipe, cap.id);
                let dirty = if cap.kind == SliderKind::Fine {
                    AutoShadeApp::slider_fine(ui, lang, text, &mut current, min, max, cap.reset)
                } else {
                    AutoShadeApp::slider(ui, lang, text, &mut current, min, max, cap.reset)
                };
                if dirty {
                    capability::set_value(recipe, cap.id, current);
                }
                dirty
            }
        };
        changed |= row_changed;
    }
    changed
}
