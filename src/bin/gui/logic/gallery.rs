//! The library gallery, headless: what a click means, what the batch selection
//! holds, and how many decoded textures may stay resident.
//!
//! The panel draws rows and emits [`GalleryAction`]s; the facade applies them
//! here. A folder dialog, a thumbnail worker and the texture map are runtime
//! concerns — the view no longer opens, requests or retains anything itself.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::ops::Range;

/// How many decoded thumbnails may stay resident. Every scrolled-past row
/// otherwise pins its 160px texture until the folder changes (~68 KB each — a
/// 10k-photo folder reached ~0.7 GB).
pub(crate) const THUMB_TEX_CAP: usize = 1500;

/// How many rows around the viewport the trim window keeps. Generous on
/// purpose: a scroll-back inside it re-materialises from the disk cache in
/// ~1 ms instead of queueing a decode.
const TRIM_MARGIN: usize = 600;

/// What the gallery's view asks for. A closed per-domain enum.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum GalleryAction {
    /// Pick the working folder with the system dialog, then open it.
    OpenFolder,
    /// A row was clicked; `multi` is the command/ctrl modifier held then.
    Click { index: usize, multi: bool },
    /// Copy the open photo's whole develop as the paste source.
    CopyRecipe,
    /// Paste the copied develop onto the selected photos.
    PasteToSelected,
    /// Render the selected photos by their own saved develops.
    RenderSelected,
    /// Open the named-recipe library window.
    OpenRecipeLibrary,
    /// Clear the batch selection.
    ClearSelection,
    /// This visible row's thumbnail has to be decoded.
    ThumbNeeded(usize),
    /// Drop the textures outside `start..end` (the bounded-texture window).
    Trim { start: usize, end: usize },
}

/// What a row click means: open that photo, or add/remove it from the batch.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Click {
    Open(usize),
    Toggle(usize),
}

/// A plain click opens the photo; the command/ctrl click toggles the batch
/// selection, which is what "paste to selected"/"render selected" read.
pub(crate) fn click(index: usize, multi: bool) -> Click {
    if multi { Click::Toggle(index) } else { Click::Open(index) }
}

/// Add or remove one row from the batch selection (a toggle, never a duplicate).
pub(crate) fn toggle(selection: &mut HashSet<usize>, index: usize) {
    if !selection.remove(&index) {
        selection.insert(index);
    }
}

/// Whether this row shows the ● badge: a develop in the store, a legacy
/// sidecar, or a foreign sidecar beside the photo. Cached per index — two
/// filesystem stats per visible row per repaint added up to thousands of
/// syscalls per second while thumbnails decode. Dropped whenever the app writes
/// a sidecar or the folder changes.
pub(crate) fn edited_badge(cache: &mut HashMap<usize, bool>, index: usize, path: &Path) -> bool {
    *cache.entry(index).or_insert_with(|| autoshade::store::has_develop_or_sidecar(path))
}

/// The texture window to keep: `None` while nothing needs trimming, otherwise
/// the visible rows plus [`TRIM_MARGIN`] either side, clamped to the folder.
pub(crate) fn trim_window(visible: Range<usize>, total: usize, cap: usize) -> Option<Range<usize>> {
    if total <= cap {
        return None;
    }
    let start = visible.start.saturating_sub(TRIM_MARGIN);
    let end = visible.end.saturating_add(TRIM_MARGIN).min(total);
    Some(start..end)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A plain click opens the row; cmd/ctrl-click toggles it in the batch.
    #[test]
    fn a_click_opens_or_toggles() {
        assert_eq!(click(3, false), Click::Open(3));
        assert_eq!(click(3, true), Click::Toggle(3));
    }

    /// The selection is a set: the same toggle twice removes, and the ordered
    /// view is sorted by the folder's own index.
    #[test]
    fn the_selection_toggles_and_reads_in_folder_order() {
        let mut selection = HashSet::new();
        toggle(&mut selection, 4);
        toggle(&mut selection, 1);
        toggle(&mut selection, 4);
        toggle(&mut selection, 2);
        assert!(selection.contains(&1) && selection.contains(&2) && !selection.contains(&4));
    }

    /// Nothing is trimmed under the cap; over it the window is the visible rows
    /// plus the margin, clamped to the folder's own end.
    #[test]
    fn the_texture_window_follows_the_cap_and_the_folder() {
        assert_eq!(trim_window(0..10, 200, THUMB_TEX_CAP), None, "under the cap, retain everything");
        assert_eq!(trim_window(0..10, 1600, THUMB_TEX_CAP), Some(0..610));
        assert_eq!(trim_window(1500..1520, 1600, THUMB_TEX_CAP), Some(900..1600));
        assert_eq!(trim_window(900..920, 1600, THUMB_TEX_CAP), Some(300..1520));
    }

    /// The badge is cached per row and survives a second ask (the frame's hot
    /// path must not stat the same photo again); a different row is its own
    /// answer.
    #[test]
    fn the_edited_badge_is_cached_per_row() {
        let mut cache = HashMap::new();
        let photo = Path::new("_autoshade_badge_probe_.arw");
        assert!(!edited_badge(&mut cache, 3, photo), "a path with no develop/sidecar has no badge");
        assert_eq!(cache.get(&3), Some(&false));
        assert!(!edited_badge(&mut cache, 4, Path::new("_autoshade_badge_probe_2_.arw")));
        assert_eq!(cache.len(), 2);
    }
}
