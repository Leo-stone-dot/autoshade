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
