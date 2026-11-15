//! The variant strip's identity rule: how a live card is matched to a
//! persisted one. Pure over the strip; the app owns the record's construction
//! and the store reads/writes.

use std::path::Path;

use autoshade::recipe::EditRecipe;

use crate::model::{Variant, VariantKind};

/// Does a live card match a persisted one? IDs decide first when BOTH sides
/// carry one; otherwise identity falls back to the card's kind and position
/// (the pre-R24-2 strip record had no ids).
pub(crate) fn identity_matches(
    live_id: &str,
    live_kind: VariantKind,
    live_position: usize,
    saved_kind: VariantKind,
    saved_position: usize,
    saved_id: Option<&str>,
) -> bool {
    if !live_id.is_empty()
        && let Some(id) = saved_id.filter(|id| !id.is_empty())
    {
        return live_id == id;
    }
    live_kind == saved_kind && live_position == saved_position
}

/// One persisted card as the dirty comparison sees it: the app translates the
/// stored record into these (it owns the store spellings).
pub(crate) struct SavedCard<'a> {
    pub kind: VariantKind,
    pub position: usize,
    pub id: Option<&'a str>,
    pub name: Option<&'a str>,
    pub recipe: &'a EditRecipe,
    pub origin: Option<&'a Path>,
}

/// How many live cards hold work that quitting would discard.
///
/// Dirty means: a live card matches no saved card, or a matched card differs in
/// kind/name/develop, or a saved card was deleted. IDs match first; id-less
/// records fall back to kind + position. WHICH card is selected is navigation,
/// not an edit, so the active card's develop is never compared.
pub(crate) fn open_dirty_count(variants: &[Variant], active: usize, saved: Option<&[SavedCard]>) -> usize {
    if variants.is_empty() {
        return 0;
    }
    let Some(saved) = saved else {
        let active_card = variants.get(active);
        let trivial = variants.len() == 1
            && active == 0
            && active_card.is_some_and(|v| {
                crate::model::strip_is_trivial(v.kind, &v.id, v.name.as_deref(), 0)
            });
        let background = variants.len().saturating_sub(1);
        return if background > 0 { background } else if trivial { 0 } else { 1 };
    };
    let matches = |live: &Variant, position: usize, s: &SavedCard| {
        identity_matches(&live.id, live.kind, position, s.kind, s.position, s.id)
    };
    let mut dirty = 0usize;
    for (position, live) in variants.iter().enumerate() {
        let Some(matched) = saved.iter().find(|s| matches(live, position, s)) else {
            dirty += 1;
            continue;
        };
        let develop_dirty = position != active
            && (crate::logic::same::dirty_vs(&live.recipe, matched.recipe)
                || !crate::logic::same::same_master_opt(live.origin.as_deref(), matched.origin));
        if live.kind != matched.kind || live.name.as_deref() != matched.name || develop_dirty {
            dirty += 1;
        }
    }
    // Saved cards with no live identity are deletions.
    dirty
        + saved
            .iter()
            .filter(|s| !variants.iter().enumerate().any(|(position, live)| matches(live, position, s)))
            .count()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn live(kind: VariantKind, id: &str, name: Option<&str>) -> Variant {
        Variant {
            kind,
            id: id.into(),
            name: name.map(str::to_string),
            recipe: Default::default(),
            base: None,
            origin: None,
            thumb: None,
        }
    }

    #[test]
    fn no_record_counts_background_work_but_not_a_trivial_lone_original() {
        let one = [live(VariantKind::Original, "", None)];
        assert_eq!(open_dirty_count(&one, 0, None), 0);
        let named = [live(VariantKind::Original, "", Some("keep"))];
        assert_eq!(open_dirty_count(&named, 0, None), 1, "a name is unsaved work");
        let two = [live(VariantKind::Original, "o", None), live(VariantKind::Fitted, "f", None)];
        assert_eq!(open_dirty_count(&two, 0, None), 1, "the background card");
    }

    #[test]
    fn a_matching_record_is_clean_and_a_changed_develop_is_dirty() {
        let strip = [live(VariantKind::Original, "o", None), live(VariantKind::Fitted, "f", None)];
        let saved_recipe = Default::default();
        let saved = [
            SavedCard { kind: VariantKind::Original, position: 0, id: Some("o"), name: None, recipe: &saved_recipe, origin: None },
            SavedCard { kind: VariantKind::Fitted, position: 1, id: Some("f"), name: None, recipe: &saved_recipe, origin: None },
        ];
        assert_eq!(open_dirty_count(&strip, 0, Some(&saved)), 0, "same kinds/ids/develops");
        let mut changed = [live(VariantKind::Original, "o", None), live(VariantKind::Fitted, "f", None)];
        changed[1].recipe.exposure_ev = 0.5;
        assert_eq!(open_dirty_count(&changed, 0, Some(&saved)), 1, "the background card's develop moved");
        changed[1].name = Some("renamed".into());
        assert_eq!(open_dirty_count(&changed, 0, Some(&saved)), 1);
    }

    #[test]
    fn the_active_cards_own_develop_is_navigation_not_an_edit() {
        let mut strip = [live(VariantKind::Original, "o", None), live(VariantKind::Fitted, "f", None)];
        strip[0].recipe.exposure_ev = 0.7;
        let saved_recipe = Default::default();
        let saved = [
            SavedCard { kind: VariantKind::Original, position: 0, id: Some("o"), name: None, recipe: &saved_recipe, origin: None },
            SavedCard { kind: VariantKind::Fitted, position: 1, id: Some("f"), name: None, recipe: &saved_recipe, origin: None },
        ];
        assert_eq!(open_dirty_count(&strip, 0, Some(&saved)), 0, "the ACTIVE card's develop is the canvas, compared elsewhere");
    }

    #[test]
    fn a_deleted_saved_card_counts_as_dirty() {
        let strip = [live(VariantKind::Original, "o", None)];
        let saved_recipe = Default::default();
        let saved = [
            SavedCard { kind: VariantKind::Original, position: 0, id: Some("o"), name: None, recipe: &saved_recipe, origin: None },
            SavedCard { kind: VariantKind::Fitted, position: 1, id: Some("f"), name: None, recipe: &saved_recipe, origin: None },
        ];
        assert_eq!(open_dirty_count(&strip, 0, Some(&saved)), 1, "the saved fitted card has no live identity");
    }

    #[test]
    fn ids_decide_when_both_sides_carry_one() {
        assert!(identity_matches("a", VariantKind::Generated, 0, VariantKind::Original, 5, Some("a")));
        assert!(!identity_matches("a", VariantKind::Generated, 0, VariantKind::Generated, 0, Some("b")));
    }

    #[test]
    fn without_ids_the_kind_and_position_decide() {
        assert!(identity_matches("", VariantKind::Denoised, 2, VariantKind::Denoised, 2, None));
        assert!(!identity_matches("", VariantKind::Denoised, 2, VariantKind::Denoised, 1, None));
        assert!(identity_matches("", VariantKind::Original, 0, VariantKind::Original, 0, Some("")));
        assert!(identity_matches("live", VariantKind::Original, 0, VariantKind::Original, 0, Some("")), "an empty SAVED id falls back");
    }
}
