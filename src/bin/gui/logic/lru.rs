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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_hit_moves_to_most_recent_and_returns_the_value() {
        let mut entries = vec![("a", 1u8), ("b", 2)];
        assert_eq!(get(&mut entries, |k| *k == "a"), Some(1));
        assert_eq!(entries.iter().map(|(k, _)| *k).collect::<Vec<_>>(), ["b", "a"], "the hit is now most recent");
    }

    #[test]
    fn a_miss_returns_none_and_does_not_reorder() {
        let mut entries = vec![("a", 1u8), ("b", 2)];
        assert_eq!(get(&mut entries, |k| *k == "zzz"), None);
        assert_eq!(entries.iter().map(|(k, _)| *k).collect::<Vec<_>>(), ["a", "b"]);
    }

    #[test]
    fn insert_replaces_the_same_key_and_evicts_the_least_recent() {
        let mut entries: Vec<(&str, u8)> = Vec::new();
        insert(&mut entries, "a", 1, |k| *k == "a", 2);
        insert(&mut entries, "a", 9, |k| *k == "a", 2);
        assert_eq!(entries.len(), 1, "the same key replaces rather than stacks");
        assert_eq!(entries[0].1, 9);
        insert(&mut entries, "b", 2, |k| *k == "b", 2);
        insert(&mut entries, "c", 3, |k| *k == "c", 2);
        assert_eq!(entries.iter().map(|(k, _)| *k).collect::<Vec<_>>(), ["b", "c"], "a evicted at the cap");
    }

    #[test]
    fn retain_drops_stale_keys_and_keeps_the_order() {
        let mut entries = vec![("a", 1u8), ("b", 2), ("c", 3)];
        retain(&mut entries, |k| *k != "b");
        assert_eq!(entries.iter().map(|(k, _)| *k).collect::<Vec<_>>(), ["a", "c"]);
    }
}
