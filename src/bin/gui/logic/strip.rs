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
