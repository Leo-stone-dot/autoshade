//! The variant strip's identity rule: how a live card is matched to a
//! persisted one. Pure over the strip; the app owns the record's construction
//! and the store reads/writes.

use crate::model::VariantKind;

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

#[cfg(test)]
mod tests {
    use super::*;

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
