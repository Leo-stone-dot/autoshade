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
