//! The card strip's pure index rules: which card is the negative, which one the
//! reverse-fit solves from, and which cards may be a fit source. They take the
//! strip as data, so the rules are unit-testable without a frame or a facade.

use std::path::{Path, PathBuf};

use crate::model::{Variant, VariantKind};

/// Where this photo's base negative sits in the strip — `None` when the strip
/// has none (a cold restore can list a fitted active card and no Original).
pub(crate) fn original_index(variants: &[Variant]) -> Option<usize> {
    variants.iter().position(|v| v.kind == VariantKind::Original)
}

fn remade_negative(v: &Variant) -> bool {
    v.kind.is_remade_negative() && v.origin.is_some()
}

/// The card the photo's NEGATIVE is: the active card when it is a remade
/// negative carrying a master, else the first such card, else the Original.
pub(crate) fn negative_index(variants: &[Variant], active: usize) -> Option<usize> {
    if variants.get(active).is_some_and(remade_negative) {
        return Some(active);
    }
    variants.iter().position(remade_negative).or_else(|| original_index(variants))
}

/// The baked master the negative carries, if any.
pub(crate) fn negative_origin(variants: &[Variant], active: usize) -> Option<PathBuf> {
    negative_index(variants, active).and_then(|i| variants[i].origin.clone())
}

/// The negative's pixels on disk: its master when it carries one, else the
/// loaded file — the input a whole-frame reimagine develops from.
pub(crate) fn negative_path(variants: &[Variant], active: usize, src_path: Option<&Path>) -> Option<PathBuf> {
    negative_origin(variants, active).or_else(|| src_path.map(Path::to_path_buf))
}

/// Can this card be the reverse-fit's source? The negative in any of its
/// forms: the Original (its in-place master, else the loaded file's frame) or
/// a remade negative that carries a master.
pub(crate) fn is_fit_source(v: &Variant) -> bool {
    v.kind == VariantKind::Original || remade_negative(v)
}

/// The card the reverse-fit solves FROM: the user's pick in the fit fold while
/// that card still stands and is a source, else the automatic rule.
pub(crate) fn fit_source_index(variants: &[Variant], active: usize, fit_from: Option<&str>) -> Option<usize> {
    fit_from
        .and_then(|id| {
            variants
                .iter()
                .position(|v| !v.id.is_empty() && v.id == id && is_fit_source(v))
        })
        .or_else(|| negative_index(variants, active))
}

/// [`fit_source_index`]'s master on disk (`None` = the loaded file's frame).
pub(crate) fn fit_negative(variants: &[Variant], active: usize, fit_from: Option<&str>) -> Option<PathBuf> {
    fit_source_index(variants, active, fit_from).and_then(|i| variants[i].origin.clone())
}

/// Which card the reverse-fit's 「Automatic」 entry names: the negative rule.
pub(crate) fn automatic_fit_card(variants: &[Variant], active: usize) -> Option<usize> {
    negative_index(variants, active)
}

/// The cards the fit's 「From:」 menu offers, in strip order: every fit source.
pub(crate) fn fit_source_cards(variants: &[Variant]) -> Vec<usize> {
    (0..variants.len()).filter(|&i| is_fit_source(&variants[i])).collect()
}

/// A card's label with its place in the strip, so two cards of one kind stay
/// apart (`name #2`). The caller supplies the label (a user name, else the
/// translated kind label); the numbering is the strip's, not the view's.
pub(crate) fn numbered_label(label: &str, index: usize) -> String {
    format!("{label} #{}", index + 1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn card(kind: VariantKind, id: &str, origin: Option<&str>) -> Variant {
        Variant {
            kind,
            id: id.into(),
            name: None,
            recipe: Default::default(),
            base: None,
            origin: origin.map(PathBuf::from),
            thumb: None,
        }
    }


    #[test]
    fn the_fit_menu_offers_the_sources_and_names_the_automatic_card() {
        let strip = [
            card(VariantKind::Original, "o", None),
            card(VariantKind::Generated, "g", Some("g.png")),
            card(VariantKind::Denoised, "d", Some("d.png")),
        ];
        assert_eq!(fit_source_cards(&strip), vec![0, 2], "Original + the remade master, strip order");
        assert_eq!(automatic_fit_card(&strip, 1), Some(2), "automatic is the negative rule");
        assert_eq!(automatic_fit_card(&[card(VariantKind::Fitted, "f", None)], 0), None);
    }

    #[test]
    fn the_numbered_label_counts_from_one() {
        assert_eq!(numbered_label("▣ Original", 0), "▣ Original #1");
        assert_eq!(numbered_label("clean", 2), "clean #3");
    }
    #[test]
    fn the_original_is_the_negative_when_no_remade_master_exists() {
        let strip = [card(VariantKind::Fitted, "f", None), card(VariantKind::Original, "o", None)];
        assert_eq!(original_index(&strip), Some(1));
        assert_eq!(negative_index(&strip, 0), Some(1));
        assert_eq!(negative_origin(&strip, 0), None);
        assert_eq!(
            negative_path(&strip, 0, Some(Path::new("photo.arw"))),
            Some(PathBuf::from("photo.arw")),
            "no master: the loaded file is the negative's pixels"
        );
        assert!(negative_path(&strip, 0, None).is_none());
    }

    #[test]
    fn a_remade_negative_master_wins_and_the_active_card_goes_first() {
        let strip = [
            card(VariantKind::Original, "o", None),
            card(VariantKind::Denoised, "d1", Some("a.png")),
            card(VariantKind::Stacked, "s1", Some("b.png")),
        ];
        assert_eq!(negative_index(&strip, 0), Some(1), "first remade master");
        assert_eq!(negative_index(&strip, 2), Some(2), "the active remade card goes first");
        assert_eq!(negative_origin(&strip, 2), Some(PathBuf::from("b.png")));
        assert_eq!(
            negative_path(&strip, 2, Some(Path::new("photo.arw"))),
            Some(PathBuf::from("b.png")),
            "the master, not the sensor file"
        );
    }

    #[test]
    fn a_remade_card_without_a_master_is_not_the_negative() {
        let strip = [card(VariantKind::Original, "o", None), card(VariantKind::Denoised, "d", None)];
        assert_eq!(negative_index(&strip, 1), Some(0), "no master: the Original is the negative");
    }

    #[test]
    fn a_fitted_only_strip_has_no_negative() {
        let strip = [card(VariantKind::Fitted, "f", None)];
        assert_eq!(original_index(&strip), None);
        assert_eq!(negative_index(&strip, 0), None);
        assert!(negative_path(&strip, 0, None).is_none());
    }

    #[test]
    fn only_the_negative_forms_are_fit_sources() {
        assert!(is_fit_source(&card(VariantKind::Original, "o", None)));
        assert!(is_fit_source(&card(VariantKind::Denoised, "d", Some("a.png"))));
        assert!(is_fit_source(&card(VariantKind::Stacked, "s", Some("a.png"))));
        assert!(!is_fit_source(&card(VariantKind::Denoised, "d", None)), "no master, not a source");
        for kind in [VariantKind::Generated, VariantKind::Fitted, VariantKind::Edited] {
            assert!(!is_fit_source(&card(kind, "x", Some("a.png"))), "{kind:?}");
        }
    }

    #[test]
    fn the_fit_pick_is_honoured_only_while_it_still_stands_as_a_source() {
        let strip = [
            card(VariantKind::Original, "o", None),
            card(VariantKind::Generated, "g", Some("g.png")),
            card(VariantKind::Denoised, "d", Some("d.png")),
        ];
        assert_eq!(fit_source_index(&strip, 0, Some("d")), Some(2), "the user's pick");
        assert_eq!(fit_source_index(&strip, 0, Some("g")), Some(2), "a non-source pick falls to the rule");
        assert_eq!(fit_source_index(&strip, 0, Some("")), Some(2), "an empty id matches nothing");
        assert_eq!(fit_source_index(&strip, 1, None), Some(2), "the rule ignores the active Generated card");
        assert_eq!(fit_negative(&strip, 0, Some("d")), Some(PathBuf::from("d.png")));
        assert_eq!(fit_negative(&strip, 0, None), Some(PathBuf::from("d.png")));
    }

    #[test]
    fn the_automatic_fit_source_is_the_negative_rule() {
        let strip = [card(VariantKind::Original, "o", Some("in-place.png")), card(VariantKind::Denoised, "d", Some("d.png"))];
        assert_eq!(fit_source_index(&strip, 0, None), Some(1), "the remade master outranks the Original");
        assert_eq!(fit_negative(&strip, 0, None), Some(PathBuf::from("d.png")));
    }
}
