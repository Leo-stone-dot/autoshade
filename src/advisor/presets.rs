//! The fixed one-click looks (the desktop app's Normal mode, user ruling
//! 2026-10-23: 「就这 6 档」).
//!
//! A preset is not a set of slider numbers: it is a DIRECTION the model reads
//! (`pipeline::Guide::Direction`), so the same answer schema as every other
//! analysis comes back and the model still looks at the photo it is grading.
//! Each text says what to do AND what not to do; only the black-and-white
//! preset may turn the photo grey, and every other one says not to.
//!
//! The ids are stable keys (the GUI translates the display names); the texts
//! are English because the model reads them, and they never reach the UI.

/// One fixed look.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Preset {
    /// The stable key, also the English display name's source.
    pub id: &'static str,
    /// The direction the model is given, verbatim.
    pub direction: &'static str,
}

/// The six looks, in the order the GUI lays them (3 × 2).
pub const PRESETS: [Preset; 6] = [
    Preset {
        id: "natural",
        direction: "Natural: a faithful, true-to-life version of this scene. Correct the exposure and the white \
                    balance so neutral things read neutral, bring back clipped highlights and open blocked shadows \
                    gently, and keep contrast and saturation close to what the eye saw. Do not stylise: no colour \
                    grading, no strong vignette, no heavy clarity, texture or dehaze. Keep the photo in colour: \
                    leave convert_to_grayscale false.",
    },
    Preset {
        id: "bright_clean",
        direction: "Bright and clean: an airy, light look. Lift the exposure and the shadows so the photo feels \
                    bright, keep the whites clean and neutral without clipping them, keep contrast moderate and \
                    colours fresh but not loud. Do not crush the blacks, do not add a colour cast, grain or a dark \
                    vignette. Keep the photo in colour: leave convert_to_grayscale false.",
    },
    Preset {
        id: "warm_sunset",
        direction: "Warm: a golden, sunlit look. Move the white balance warmer and lean the highlights toward gold \
                    and amber with colour grading, keep skin and natural tones believable and contrast moderate. \
                    Do not turn the whole photo orange or muddy, and do not clip the bright areas. Keep the photo \
                    in colour: leave convert_to_grayscale false.",
    },
    Preset {
        id: "cool_night",
        direction: "Cool night: a cool, clean night look. Move the white balance cooler toward blue, keep the sky \
                    and the shadows deep but not crushed, keep street lights, windows and stars from blowing out, \
                    and keep noise under control with modest contrast. Do not brighten the night into daylight and \
                    do not add a magenta or green cast. Keep the photo in colour: leave convert_to_grayscale false.",
    },
    Preset {
        id: "film",
        direction: "Film: a soft analogue film look. Lift the blacks slightly with the tone curve for a gentle fade, \
                    keep contrast soft, mute the saturation a little, add a mild warm-highlight / cool-shadow split \
                    with colour grading and a little fine grain. Do not overdo the fade or the grain, and do not \
                    push colours to unnatural hues. Keep the photo in colour: leave convert_to_grayscale false.",
    },
    Preset {
        id: "mono",
        direction: "Black and white: convert the photo to grayscale (set convert_to_grayscale to true) and shape \
                    it with the grey mix bands (gray_red, gray_orange, gray_yellow, gray_green, gray_aqua, \
                    gray_blue, gray_purple, gray_magenta) so the sky, skin and foliage separate into distinct greys; \
                    give it rich contrast with clean blacks and clean whites. Do not add colour grading or a colour \
                    tint, and do not leave the photo in colour.",
    },
];

/// The skin guardrail as the model reads it (design-portraits-mode.md §4,
/// the "Keep skin natural" switch's limits from `face::keep_skin_natural`):
/// part of the portrait direction, and the black-and-white exception named.
pub const SKIN_GUARDRAIL: &str = "Skin is never pushed orange. Inside the face mask keep saturation at \
                                  +15 or below, hue within -26..+26 (8 degrees), temperature within \
                                  -20..+20, texture no lower than -60, clarity no lower than -40 and \
                                  sharpness never above 0; the only exception is a black-and-white \
                                  look, where skin colour does not apply.";

/// The Portraits mode's one-click look (v1.14.0): a direction like the six,
/// usable wherever a preset id is (`preset("portrait")`), and not one of
/// Normal's six cards — that grid is the user's ruling (「就这 6 档」). It
/// asks for the three portrait masks by content (`kind=ai`, `what=face` /
/// `eyes` / `background`, resolved on this computer) and carries
/// [`SKIN_GUARDRAIL`] word for word.
pub const PORTRAIT: Preset = Preset {
    id: "portrait",
    direction: "Portrait: a flattering, natural portrait of the people in this photo. Put the people's \
                edits on local masks named by content: a kind=ai mask with what=face for the skin — \
                soften it gently with negative texture and clarity and keep its colour true; one with \
                what=eyes to brighten the eyes a little and give them a touch of clarity; and one with \
                what=background to calm everything behind the person — darken it slightly and fade its \
                colour a little so the person stands out. Keep the overall exposure and white balance \
                kind to skin. Skin is never pushed orange. Inside the face mask keep saturation at \
                +15 or below, hue within -26..+26 (8 degrees), temperature within \
                -20..+20, texture no lower than -60, clarity no lower than -40 and \
                sharpness never above 0; the only exception is a black-and-white \
                look, where skin colour does not apply. Do not smooth the skin into plastic, do not \
                sharpen it, and do not blur or reshape the face. Keep the photo in colour: leave \
                convert_to_grayscale false.",
};

/// The preset called `id`: one of the six, or [`PORTRAIT`].
pub fn preset(id: &str) -> Option<&'static Preset> {
    PRESETS.iter().chain(std::iter::once(&PORTRAIT)).find(|p| p.id == id)
}

/// The direction text of the preset called `id`.
pub fn preset_guidance(id: &str) -> Option<&'static str> {
    preset(id).map(|p| p.direction)
}
