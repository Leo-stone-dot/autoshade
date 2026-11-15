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

#[cfg(test)]
mod tests {
    use super::*;

    /// A saved build says its count and folder, and whether look phrases were
    /// written; it is a quiet success (status + one toast).
    #[test]
    fn a_saved_build_names_its_count_folder_and_phrases() {
        let with_phrases = words(&Outcome::Saved {
            total: 120,
            dir_display: "refs".into(),
            phrased: 40,
        });
        assert_eq!(
            with_phrases.status.key,
            "Reference library built: {n} finished photos from {path}, {m} with look phrases"
        );
        assert_eq!(with_phrases.status.args[1].1, super::super::Arg::Text("refs".into()));
        assert!(matches!(with_phrases.toast, Some((Tone::Success, _))));

        let without = words(&Outcome::Saved { total: 7, dir_display: "refs".into(), phrased: 0 });
        assert_eq!(without.status.key, "Reference library built: {n} finished photos from {path}");
        assert_eq!(without.status.args.len(), 2, "no phrase count when none was written");
        assert!(matches!(without.toast, Some((Tone::Success, _))));
    }

    /// A failed build interrupts with the error in both the line and toast.
    #[test]
    fn a_failed_build_names_the_error() {
        let failed = words(&Outcome::Failed { err: "no photos".into() });
        assert_eq!(failed.status.key, "Building the reference library failed: {err}");
        assert_eq!(failed.status.args[0].1, super::super::Arg::Text("no photos".into()));
        assert!(matches!(failed.toast, Some((Tone::Error, _))));
    }
}
