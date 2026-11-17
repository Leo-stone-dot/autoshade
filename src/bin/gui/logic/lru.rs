//! The two pixel caches' LRU discipline, as pure operations on a Vec kept
//! least-recent first (most-recent last). The app owns the keys and values; the
//! ordering rule lives here once.

/// A cache hit: the matching entry moves to the back (most recent) and its
/// value is cloned. A miss leaves the order untouched.
pub(crate) fn get<K, V: Clone>(entries: &mut Vec<(K, V)>, matches: impl Fn(&K) -> bool) -> Option<V> {
    let pos = entries.iter().position(|(key, _)| matches(key))?;
    let entry = entries.remove(pos);
    let value = entry.1.clone();
    entries.push(entry);
    Some(value)
}

/// Insert with replace-by-key, then evict least-recent entries past `cap`.
pub(crate) fn insert<K, V>(entries: &mut Vec<(K, V)>, key: K, value: V, matches: impl Fn(&K) -> bool, cap: usize) {
    entries.retain(|(existing, _)| !matches(existing));
    entries.push((key, value));
    while entries.len() > cap {
        entries.remove(0);
    }
}

/// Drop every entry whose key matches (a stale pixel identity, a closed photo).
pub(crate) fn retain<K, V>(entries: &mut Vec<(K, V)>, keep: impl Fn(&K) -> bool) {
    entries.retain(|(key, _)| keep(key));
}
