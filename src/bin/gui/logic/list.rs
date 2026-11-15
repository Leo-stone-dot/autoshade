//! Drag-reorder bookkeeping, headless: element `from` moves to sit before
//! `insert` (both indices in the PRE-move order; `insert == len` appends).
//! Returns the element's final index plus the remap for every OTHER stored
//! index (e.g. the selection), composed as remove-at-`from` then
//! insert-at-`to`.

pub(crate) fn reorder_move(from: usize, insert: usize) -> (usize, impl Fn(usize) -> usize) {
    let to = if insert > from { insert - 1 } else { insert };
    (to, move |s: usize| {
        if s == from {
            to
        } else {
            let after_rm = if s > from { s - 1 } else { s };
            if after_rm >= to { after_rm + 1 } else { after_rm }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The remap agrees with what a physical remove-then-insert does to every
    /// stored index — for every drag on a small list.
    #[test]
    fn the_remap_matches_a_real_remove_and_insert() {
        for len in 1..5usize {
            let original: Vec<usize> = (0..len).collect();
            for from in 0..len {
                for insert in 0..=len {
                    let (to, remap) = reorder_move(from, insert);
                    let mut physical = original.clone();
                    let moved = physical.remove(from);
                    physical.insert(to.min(physical.len()), moved);
                    // Every element's remap must equal its physical position.
                    for (index, &value) in original.iter().enumerate() {
                        assert_eq!(physical[remap(index)], value, "len {len}: {from} -> {insert}");
                    }
                }
            }
        }
    }

    #[test]
    fn the_two_drop_slots_around_the_element_are_no_ops() {
        let (to, remap) = reorder_move(2, 2);
        assert_eq!(to, 2);
        assert_eq!((remap(0), remap(1), remap(2), remap(3)), (0, 1, 2, 3));
        let (to, remap) = reorder_move(2, 3);
        assert_eq!(to, 2);
        assert_eq!((remap(0), remap(1), remap(2), remap(3)), (0, 1, 2, 3));
    }

    #[test]
    fn the_ends_move_the_element_and_shift_the_rest() {
        let (to, remap) = reorder_move(0, 10);
        assert_eq!(to, 9, "append moves it to the last slot");
        assert_eq!(remap(0), 9);
        assert_eq!((remap(1), remap(2)), (0, 1));
        let (to, remap) = reorder_move(9, 0);
        assert_eq!(to, 0);
        assert_eq!((remap(0), remap(1)), (1, 2));
    }
}
