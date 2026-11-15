//! Which snapshot belongs to which card: the version filter's identity rule
//! and the "where it came from" line. Pure; the store read is the app's.

/// Does snapshot `meta` record a develop of the active card? A recorded id is
/// authoritative while the card still carries one; otherwise the recorded kind
/// decides — and a matching kind must never smuggle in another card's history.
pub(crate) fn is_from(
    meta: Option<&autoshade::store::VersionMetaEntry>,
    active_id: &str,
    active_kind: Option<&str>,
) -> bool {
    let Some(meta) = meta else { return false };
    if let Some(id) = meta.from_id.as_deref()
        && !active_id.is_empty()
    {
        return id == active_id;
    }
    match (meta.from_kind.as_deref(), active_kind) {
        (Some(k), Some(ak)) => k == ak,
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn meta(from_id: Option<&str>, from_kind: Option<&str>) -> autoshade::store::VersionMetaEntry {
        autoshade::store::VersionMetaEntry {
            n: 1,
            name: None,
            from_kind: from_kind.map(str::to_string),
            from_id: from_id.map(str::to_string),
            origin: None,
        }
    }

    #[test]
    fn a_recorded_id_is_authoritative_and_survives_a_re_kind() {
        let m = meta(Some("card-a"), Some("original"));
        assert!(is_from(Some(&m), "card-a", Some("original")));
        assert!(is_from(Some(&m), "card-a", Some("fitted")), "the id outranks the recorded kind");
        assert!(!is_from(Some(&m), "card-b", Some("original")), "a matching KIND must not smuggle in another card's history");
    }

    #[test]
    fn without_an_id_the_recorded_kind_decides() {
        let old = meta(None, Some("fitted"));
        assert!(is_from(Some(&old), "card-a", Some("fitted")));
        assert!(!is_from(Some(&old), "card-a", Some("original")));
    }

    #[test]
    fn an_idless_active_card_falls_back_to_the_kind() {
        let m = meta(Some("card-a"), Some("original"));
        assert!(is_from(Some(&m), "", Some("original")), "no id on the card: kind decides");
    }

    #[test]
    fn no_meta_no_kind_or_no_recorded_origin_never_matches() {
        assert!(!is_from(None, "card-a", Some("original")));
        assert!(!is_from(Some(&meta(None, None)), "card-a", Some("original")));
        assert!(is_from(Some(&meta(Some("card-a"), None)), "card-a", None), "the id alone is enough");
        assert!(!is_from(Some(&meta(None, None)), "", None), "no meta identity and no card identity");
    }
}
