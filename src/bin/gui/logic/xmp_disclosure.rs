//! The Lightroom-sidecar disclosure's RULES, headless: which losses interrupt
//! the save (an error toast plus a ⚠), which slider angles get named, and
//! which render gaps collapse into which section. The sentences are rendered
//! by the view (`util.rs`), from the data these functions return.

/// Does this loss line deserve to INTERRUPT the save — an Error toast plus a ⚠
/// on the status line — or is it a note for the quiet channel beside it?
///
/// The rule: "a save that lost nothing must not interrupt". `base_curve` is
/// stamped onto the canvas by every RAW OPEN (`persist::stamp_calibration`), so
/// `RenderedNotExported` is active on essentially every RAW — and every Ctrl+S
/// then raised an error toast about a loss that is real, universal and
/// impossible to act on. Alarm on every save is alarm the user learns to
/// ignore, and the mask half (which they CAN act on) went with it.
///
/// The judgement comes from the registry: `STAMPED_CALIBRATION` is the
/// engine's own per-photo measurement (the camera base curve, the in-camera
/// lens profile and the two as-shot anchors) — nothing the user chose.
/// `colour_field`, by contrast, is the photographer's own ask, so it
/// interrupts. Actual MASK losses always interrupt.
pub(crate) fn loss_interrupts(
    losses: &[autoshade::xmp::MaskLoss],
    globals: &[&'static str],
) -> bool {
    use autoshade::advisor::catalogue::STAMPED_CALIBRATION;
    // A name with no registry row is not something this can vouch for as
    // engine calibration — an unknown falls to the interrupting side, and so
    // does every known control that is not one of the stamped four.
    !losses.is_empty() || globals.iter().any(|g| !STAMPED_CALIBRATION.contains(g))
}

/// The rotation angles worth naming: non-zero and distinct, in the order the
/// writer raised them, capped at four with the rest counted.
pub(crate) struct RotationList {
    pub(crate) shown: Vec<i32>,
    pub(crate) more: usize,
}

/// Collect a rotation disclosure's angles; `None` when there is no angle to
/// name (an unreadable one, or a tilt under half a degree).
pub(crate) fn rotation_list(degs: impl Iterator<Item = i32>) -> Option<RotationList> {
    let mut seen: Vec<i32> = Vec::new();
    for d in degs {
        if d != 0 && !seen.contains(&d) {
            seen.push(d);
        }
    }
    if seen.is_empty() {
        return None;
    }
    let shown = seen.len().min(4);
    let more = seen.len() - shown;
    Some(RotationList { shown: seen[..shown].to_vec(), more })
}

/// The section a carried-only render gap is named by. Named by SECTION, not by
/// field: `post_crop_vignette_hl` is an internal symbol and R24 pinned those
/// out of user prose; the develop panel's own section headings are the words
/// the photographer already has. The mapping goes through the registry's family
/// table, and a family nobody labelled falls back to the registry name (loud)
/// rather than vanishing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GapSection {
    Effects,
    Lens,
    Raw(&'static str),
}

/// The distinct sections among the carried-only gaps, first-seen order.
pub(crate) fn gap_sections(gaps: &[&'static str]) -> Vec<GapSection> {
    use autoshade::advisor::catalogue::CONTROL_FAMILIES;
    let mut out: Vec<GapSection> = Vec::new();
    for g in gaps {
        let section = match CONTROL_FAMILIES.iter().find(|f| f.members.contains(g)).map(|f| f.name) {
            Some("effects") => GapSection::Effects,
            Some("lens_effects") => GapSection::Lens,
            _ => GapSection::Raw(g),
        };
        if !out.contains(&section) {
            out.push(section);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Stamped calibration is quiet; a user's own control, an unregistered
    /// name, or any mask loss interrupts.
    #[test]
    fn only_actionable_or_unknown_losses_interrupt() {
        assert!(!loss_interrupts(&[], &[]), "nothing lost, nothing to say");
        assert!(!loss_interrupts(&[], &["base_curve"]), "stamped calibration is quiet");
        assert!(!loss_interrupts(&[], &["lens_profile", "as_shot_k", "as_shot_tint", "look"]));
        assert!(loss_interrupts(&[], &["colour_field"]), "the photographer asked for it");
        assert!(loss_interrupts(&[], &["base_curve", "lens_blur"]), "one actionable member interrupts the line");
        assert!(loss_interrupts(&[], &["some_unknown_control"]), "an unknown is never vouched for");
    }

    /// The rotation list keeps distinct non-zero angles in order, names at most
    /// four and counts the rest; no angle is no list.
    #[test]
    fn a_rotation_list_names_distinct_angles() {
        assert!(rotation_list(std::iter::empty()).is_none());
        assert!(rotation_list([0, 0].into_iter()).is_none(), "an unreadable/zero angle is not named");
        let list = rotation_list([7, 7, 0, -3, 12, 4, 5].into_iter()).expect("angles");
        assert_eq!(list.shown, vec![7, -3, 12, 4]);
        assert_eq!(list.more, 1);
    }

    /// Gap fields collapse into their registry family, first-seen order, and a
    /// family nobody labelled keeps its raw name.
    #[test]
    fn gaps_collapse_into_named_sections() {
        assert!(gap_sections(&[]).is_empty());
        let sections = gap_sections(&[
            "post_crop_vignette",
            "lens_profile_distortion_scale",
            "post_crop_vignette",
            "mystery_control",
        ]);
        assert_eq!(
            sections,
            vec![
                GapSection::Effects,
                GapSection::Lens,
                GapSection::Raw("mystery_control"),
            ],
            "two families and one unknown, deduped in first-seen order"
        );
    }
}
