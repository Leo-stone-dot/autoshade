//! A folder scan's landing, headless: the count line and the skipped-items
//! disclosure. The scan, the gallery reset and the thumbnail generation bump
//! are the runtime's.
use autoshade::text::generated::*;


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
        UiText::of(K_LOGIC_NO_PHOTOS_FOUND_IN_THIS_FOLDER)
    } else if photos == 1 {
        UiText::of(K_LOGIC_1_PHOTO_CLICK_A_THUMBNAIL_TO_OPEN)
    } else {
        UiText::with(K_LOGIC_PHOTOS_CLICK_A_THUMBNAIL_TO_OPEN, &[("n", &photos.to_string())])
    };
    let skipped = (skipped > 0).then(|| {
        UiText::with(
            K_LOGIC_UNREADABLE_ITEM_S_SKIPPED_DURING_THE_FOLDER_SCAN,
            &[("n", &skipped.to_string())],
        )
    });
    FolderWords { status, skipped }
}
