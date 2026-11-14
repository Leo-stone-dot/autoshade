//! A finished retouch's facts and their landing-time words.
//!
//! Workers return [`RetouchNote`] FACTS (never prose): which tool ran, what it
//! wrote, the numbers worth reading. This module turns one note into the parts
//! of its status line — the sentence, the " · " facts that ride it, and the
//! " — ⚠ " warnings from the engine's typed notes — so the landing only renders
//! and appends. `RetouchNote` itself lives here because it is data, not a
//! view type.

use super::{Arg, UiText};
use std::path::PathBuf;

/// What one finished retouch DID (L12#4): facts, not prose — the landing
/// renders them in the landing-time language.
pub(crate) enum RetouchNote {
    /// Generative fill landed at this pixel master — as a NEW ✨ card since
    /// 2026-09-15 (the model is shown the card's picture; see `start_fill`).
    Filled(PathBuf),
    /// A fill with no painted area: the model chose where, and the layer
    /// covers this share of the frame (`generative::auto_fill_onto`).
    FoundFilled { out: PathBuf, coverage: f32 },
    /// An adjust of a generated card: region fills have no whole-frame D.
    Adjusted { out: PathBuf, region: bool, divergence: Option<f32> },
    /// Heal: spot count + artifact + the heal report's rationale split per
    /// the L12#2B suffix contract (AI prose prefix + typed notes). `skipped`
    /// is the engine's own count of planned spots it left untouched because
    /// no donor of that size fit inside the frame (`HealReport::skipped`) —
    /// rendered as its own line, so "healed N" never stands for "asked N".
    /// `in_face`: the heal was held to the face mask (the Portraits panel's
    /// 🤖 Remove blemishes), so `n` counts the spots healed on the face.
    Healed {
        n: usize,
        out: PathBuf,
        ai_prose: String,
        notes: Vec<autoshade::rationale::Note>,
        in_face: bool,
    },
    /// A stack landed as a new ▦ card at this pixel master. Facts, not prose:
    /// which merge ran, how many frames it took, how far the aligner had to
    /// move the worst of them, how much of the frame no source could cover,
    /// and the stops of highlight an HDR merge recovered.
    Stacked {
        out: PathBuf,
        kind: autoshade::stack::merge::StackKind,
        frames: usize,
        travel: f32,
        uncovered: f32,
        headroom_ev: f32,
    },
    /// AI denoise landed as a new ◈ card at this pixel master; `on_mosaic`
    /// says the source was a RAW, denoised on its sensor mosaic before
    /// demosaic (a baked source goes through SCUNet on developed pixels).
    Denoised { out: PathBuf, on_mosaic: bool },
    /// Clone stamp: spot count + artifact.
    Cloned { n: usize, out: PathBuf },
    /// Whole-frame reimagine → a new Generated variant. Carries the
    /// generation-side fidelity reading: the kept result's structural
    /// divergence D, and the discarded attempt's D when the opt-in retry
    /// bought a second generation.
    Reimagined { out: PathBuf, divergence: f32, discarded: Option<f32> },
}

/// The parts of a landing line. The landing renders `text`, appends
/// `path_suffix` (after a " · "), appends every `suffix` verbatim (their keys
/// carry their own leading " · "), and folds `prose` + rendered `warnings`
/// into one " — ⚠ " tail when either is present.
pub(crate) struct RetouchWords {
    pub(crate) text: UiText,
    pub(crate) suffixes: Vec<UiText>,
    pub(crate) path_suffix: Option<String>,
    pub(crate) prose: String,
    pub(crate) warnings: Vec<UiText>,
}

fn path_of(p: &std::path::Path) -> String {
    p.display().to_string()
}

/// The words for one retouch note.
pub(crate) fn words(note: &RetouchNote) -> RetouchWords {
    let mut suffixes = Vec::new();
    let mut path_suffix = None;
    let mut prose = String::new();
    let mut warnings = Vec::new();
    let text = match note {
        RetouchNote::Filled(p) => UiText::with(
            "filled → {path} (a layer over this card's develop — Ctrl+Z takes it off; the card's own pixels are untouched)",
            &[("path", &path_of(p))],
        ),
        RetouchNote::FoundFilled { out: path, coverage } => UiText::with(
            "changed {pct}% of the picture → {path} (a layer over this card's develop — Ctrl+Z takes it off; the card's own pixels are untouched)",
            &[("pct", &format!("{:.0}", coverage * 100.0)), ("path", &path_of(path))],
        ),
        RetouchNote::Adjusted { out: path, region, divergence } => {
            let text = if *region {
                UiText::of("adjusted → new ✨ card (painted area only)")
            } else if let Some(d) = divergence {
                UiText::with(
                    "adjusted → new ✨ card (whole image, divergence D = {d})",
                    &[("d", &format!("{d:.2}"))],
                )
            } else {
                UiText::of("adjusted → new ✨ card (whole image)")
            };
            path_suffix = Some(path_of(path));
            text
        }
        RetouchNote::Healed { n, out: path, ai_prose, notes, in_face } => {
            let args = [("n", n.to_string()), ("path", path_of(path))];
            let args: Vec<(&str, &str)> = args.iter().map(|(k, v)| (*k, &v[..])).collect();
            // The spots left untouched (no donor of their size fits the frame)
            // ride in `notes` as a typed key, rendered by the same catalogue as
            // every other note — one sentence, owned by the engine.
            prose = ai_prose.clone();
            warnings = notes
                .iter()
                .map(|note| {
                    let args: Vec<(&str, &str)> =
                        note.args.iter().map(|(k, v)| (*k, &v[..])).collect();
                    UiText::with(note.key, &args)
                })
                .collect();
            if *in_face {
                UiText::with(
                    "healed {n} spot(s) on the face → {path} (spots elsewhere were left alone — Ctrl+Z goes back)",
                    &args,
                )
            } else {
                UiText::with("healed {n} spot(s) → {path}", &args)
            }
        }
        RetouchNote::Stacked { out: path, kind, frames, travel, uncovered, headroom_ev } => {
            // The three numbers worth reading before trusting a stack, and only
            // when they have something to say: how far the aligner had to move
            // the worst frame, how much of the frame no source could cover, and
            // — an HDR merge only — the stops of highlight it recovered.
            if *travel >= 0.5 {
                suffixes.push(UiText::with(
                    " · aligned by up to {px} px",
                    &[("px", &format!("{travel:.1}"))],
                ));
            }
            if *uncovered > 0.0 {
                suffixes.push(UiText::with(
                    " · {pct}% of the frame was outside at least one source",
                    &[("pct", &format!("{:.2}", 100.0 * uncovered))],
                ));
            }
            if *headroom_ev > 0.0 {
                suffixes.push(UiText::with(
                    " · recovered {ev} EV above the first frame's white — the SDR rendition sliders now mean something",
                    &[("ev", &format!("{headroom_ev:.2}"))],
                ));
            }
            UiText::with_args(
                "stacked {n} frames ({how}) → {path} (new ▦ card; the frame you started from is untouched)",
                &[
                    ("n", Arg::Text(frames.to_string())),
                    ("how", Arg::Key(kind.label())),
                    ("path", Arg::Text(path_of(path))),
                ],
            )
        }
        RetouchNote::Denoised { out: path, on_mosaic: true } => UiText::with(
            "AI denoised on the sensor mosaic → {path} (new ◈ card; the card you started from is untouched)",
            &[("path", &path_of(path))],
        ),
        RetouchNote::Denoised { out: path, on_mosaic: false } => UiText::with(
            "AI denoised (a baked source, so SCUNet on developed pixels) → {path} (new ◈ card; the card you started from is untouched)",
            &[("path", &path_of(path))],
        ),
        RetouchNote::Cloned { n, out: path } => UiText::with(
            "Cloned {n} spot(s) → {path}",
            &[("n", &n.to_string()), ("path", &path_of(path))],
        ),
        RetouchNote::Reimagined { out: path, divergence, discarded } => {
            suffixes.push(reimagine_fidelity(*divergence));
            if let Some(d0) = discarded {
                suffixes.push(UiText::with(
                    " · the opt-in retry kept the closer of two results (discarded D={d0})",
                    &[("d0", &format!("{d0:.3}"))],
                ));
            }
            UiText::with(
                "「AI generated」variant created → {path} · edits on it continue on an ✎ card, or 「Reverse-fit」",
                &[("path", &path_of(path))],
            )
        }
    };
    RetouchWords { text, suffixes, path_suffix, prose, warnings }
}

/// A reimagine's generation-side fidelity reading, " · "-led: the landing's
/// status line ends with it and the fit prompt states it — one wording, so the
/// two never drift. The SAME statistic the reverse-fit's mode selector
/// computes, so the warning names the consequence in that vocabulary.
pub(crate) fn reimagine_fidelity(divergence: f32) -> UiText {
    if divergence >= autoshade::fit::DIVERGENCE_GLOBAL {
        UiText::with(
            " · ⚠ structure diverged from the original (D={d} ≥ {limit}) — a reverse-fit will fall back to atmosphere mode",
            &[
                ("d", &format!("{divergence:.3}")),
                ("limit", &format!("{:.2}", autoshade::fit::DIVERGENCE_GLOBAL)),
            ],
        )
    } else {
        UiText::with(
            " · faithful to the frame (structural divergence D={d})",
            &[("d", &format!("{divergence:.3}"))],
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A fill's line names its artifact; a found fill also names the share of
    /// the frame it changed.
    #[test]
    fn a_fill_names_its_artifact_and_coverage() {
        let filled = words(&RetouchNote::Filled(PathBuf::from("d/fill.png")));
        assert_eq!(filled.text.args[0].1, Arg::Text("d/fill.png".into()));
        assert!(filled.prose.is_empty() && filled.warnings.is_empty());

        let found = words(&RetouchNote::FoundFilled { out: PathBuf::from("d/f.png"), coverage: 0.42 });
        assert_eq!(found.text.args[0].1, Arg::Text("42".into()));
        assert_eq!(found.text.args[1].1, Arg::Text("d/f.png".into()));
    }

    /// Heal: the face variant says spots elsewhere were left alone; the engine
    /// notes and the AI prose ride the warning tail (prose first, notes after).
    #[test]
    fn a_heal_folds_prose_and_typed_notes_into_the_warning_tail() {
        let note = autoshade::rationale::Note {
            key: "heal skipped {n} spot(s)",
            args: vec![("n", "2".to_string())],
        };
        let healed = words(&RetouchNote::Healed {
            n: 3,
            out: PathBuf::from("d/heal.png"),
            ai_prose: "cleaned the sensor dust".into(),
            notes: vec![note],
            in_face: true,
        });
        assert!(healed.text.key.starts_with("healed {n} spot(s) on the face"));
        assert_eq!(healed.prose, "cleaned the sensor dust");
        assert_eq!(healed.warnings.len(), 1);
        assert_eq!(healed.warnings[0].key, "heal skipped {n} spot(s)");
        assert_eq!(healed.warnings[0].args[0].1, Arg::Text("2".into()));

        let plain = words(&RetouchNote::Healed {
            n: 1,
            out: PathBuf::from("d/heal.png"),
            ai_prose: String::new(),
            notes: Vec::new(),
            in_face: false,
        });
        assert_eq!(plain.text.key, "healed {n} spot(s) → {path}");
        assert!(plain.prose.is_empty() && plain.warnings.is_empty());
    }

    /// A stack's three facts only appear when they have something to say, and
    /// the merge kind travels as a catalogue KEY.
    #[test]
    fn a_stack_shows_its_facts_only_when_present() {
        let quiet = words(&RetouchNote::Stacked {
            out: PathBuf::from("d/stack.png"),
            kind: autoshade::stack::merge::StackKind::Hdr,
            frames: 3,
            travel: 0.0,
            uncovered: 0.0,
            headroom_ev: 0.0,
        });
        assert_eq!(quiet.text.args[0].1, Arg::Text("3".into()));
        assert!(quiet.suffixes.is_empty(), "nothing worth reading");
        assert_eq!(quiet.text.args[1].0, "how");
        assert!(matches!(quiet.text.args[1].1, Arg::Key(_)), "the merge kind is a key");

        let loud = words(&RetouchNote::Stacked {
            out: PathBuf::from("d/stack.png"),
            kind: autoshade::stack::merge::StackKind::Hdr,
            frames: 3,
            travel: 1.5,
            uncovered: 0.1,
            headroom_ev: 2.0,
        });
        assert_eq!(loud.suffixes.len(), 3, "alignment, uncovered share and recovered stops");
    }

    /// A reimagine appends the fidelity reading and, when the opt-in retry ran,
    /// the discarded attempt's D; the reading itself warns at the global limit.
    #[test]
    fn a_reimagine_carries_its_fidelity_and_the_discarded_attempt() {
        let below = words(&RetouchNote::Reimagined {
            out: PathBuf::from("d/gen.png"),
            divergence: 0.01,
            discarded: None,
        });
        assert_eq!(below.suffixes.len(), 1);
        assert!(below.suffixes[0].key.contains("faithful to the frame"));

        let above = words(&RetouchNote::Reimagined {
            out: PathBuf::from("d/gen.png"),
            divergence: autoshade::fit::DIVERGENCE_GLOBAL + 0.1,
            discarded: Some(0.4),
        });
        assert_eq!(above.suffixes.len(), 2);
        assert!(above.suffixes[0].key.contains("structure diverged"));
        assert!(above.suffixes[1].key.contains("discarded D="));
    }
}
