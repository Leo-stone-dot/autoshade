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
