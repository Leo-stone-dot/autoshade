//! A folder scan's landing, headless: the count line and the skipped-items
//! disclosure. The scan, the gallery reset and the thumbnail generation bump
//! are the runtime's.

use super::UiText;

/// The scan's words: the count line, plus the skipped-items warning when the
/// scan had to drop entries.
pub(crate) struct FolderWords {
    pub(crate) status: UiText,
    pub(crate) skipped: Option<UiText>,
}

/// The words for a finished folder scan.
pub(crate) fn words(photos: usize, skipped: usize) -> FolderWords {
    let status = if photos == 0 {
        // The plural line says "click a thumbnail" — with zero rows there is
        // none to click.
        UiText::of("no photos found in this folder")
    } else if photos == 1 {
        UiText::of("1 photo — click a thumbnail to open")
    } else {
        UiText::with("{n} photos — click a thumbnail to open", &[("n", &photos.to_string())])
    };
    let skipped = (skipped > 0).then(|| {
        UiText::with(
            "{n} unreadable item(s) skipped during the folder scan",
            &[("n", &skipped.to_string())],
        )
    });
    FolderWords { status, skipped }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Zero photos says so (the plural line would promise a thumbnail), one
    /// is its own sentence, more carries the count.
    #[test]
    fn the_count_line_matches_the_row_count() {
        assert_eq!(words(0, 0).status.key, "no photos found in this folder");
        assert_eq!(words(1, 0).status.key, "1 photo — click a thumbnail to open");
        let many = words(7, 0);
        assert_eq!(many.status.key, "{n} photos — click a thumbnail to open");
        assert_eq!(many.status.args[0].1, super::super::Arg::Text("7".into()));
    }

    /// Unreadable entries are disclosed with their count; a clean scan has no
    /// warning to append.
    #[test]
    fn skipped_items_ride_the_scan_line() {
        assert!(words(3, 0).skipped.is_none());
        let dirty = words(3, 2);
        let skip = dirty.skipped.expect("skipped entries are disclosed");
        assert_eq!(skip.key, "{n} unreadable item(s) skipped during the folder scan");
        assert_eq!(skip.args[0].1, super::super::Arg::Text("2".into()));
    }
}
