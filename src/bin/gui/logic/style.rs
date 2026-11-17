//! The reference-library build's landing, headless: what one build outcome
//! says and how loudly. The build itself (and the library summary refresh) is
//! the runtime's.
use autoshade::text::generated::*;


use super::{Tone, UiText, Words};

/// One finished build, as the landing sees it (the directory is already
/// displayed by the caller — the logic layer does not format paths).
pub(crate) enum Outcome {
    Saved { total: usize, dir_display: String, phrased: usize },
    Failed { err: String },
}

/// The words for one build outcome.
pub(crate) fn words(outcome: &Outcome) -> Words {
    match outcome {
        Outcome::Saved { total, dir_display, phrased } => {
            let text = if *phrased > 0 {
                UiText::with(
                    K_LOGIC_REFERENCE_LIBRARY_BUILT_FINISHED_PHOTOS_FROM_WIT,
                    &[
                        ("n", &total.to_string()),
                        ("path", dir_display),
                        ("m", &phrased.to_string()),
                    ],
                )
            } else {
                UiText::with(
                    K_LOGIC_REFERENCE_LIBRARY_BUILT_FINISHED_PHOTOS_FROM,
                    &[("n", &total.to_string()), ("path", dir_display)],
                )
            };
            Words { status: text.clone(), toast: Some((Tone::Success, text)) }
        }
        Outcome::Failed { err } => {
            let text = UiText::with(K_LOGIC_BUILDING_THE_REFERENCE_LIBRARY_FAILED, &[("err", err)]);
            Words { status: text.clone(), toast: Some((Tone::Error, text)) }
        }
    }
}
