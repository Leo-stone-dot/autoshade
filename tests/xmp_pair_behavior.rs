use std::path::{Path, PathBuf};

use autoshade::xmp_pair::XmpPairing;

mod support;

fn write(path: &Path, bytes: &[u8]) -> PathBuf {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, bytes).unwrap();
    path.to_path_buf()
}

#[test]
fn xmp_pairing_matches_the_extension_in_any_ascii_case() {
    let tree = support::TempTree::new("xmp-pair-extension");
    let raw = write(&tree.path().join("SHOT.ARW"), b"raw");
    let sidecar = write(&tree.path().join("SHOT.XmP"), b"xmp");

    assert_eq!(XmpPairing::beside().find(&raw), Some(sidecar));
}

#[test]
fn xmp_pairing_finds_a_sidecar_in_the_mirrored_library_tree() {
    let tree = support::TempTree::new("xmp-pair-mirror");
    let library = tree.path().join("library");
    let sidecars = tree.path().join("sidecars");
    let raw = write(&library.join("trip/day/shot.arw"), b"raw");
    let expected = write(&sidecars.join("trip/day/shot.xmp"), b"xmp");

    assert_eq!(
        XmpPairing::new(&library, Some(&sidecars)).find(&raw),
        Some(expected)
    );
}

#[test]
fn xmp_pairing_finds_a_sidecar_in_a_flat_mirror() {
    let tree = support::TempTree::new("xmp-pair-flat");
    let library = tree.path().join("library");
    let sidecars = tree.path().join("sidecars");
    let raw = write(&library.join("trip/day/shot.arw"), b"raw");
    let expected = write(&sidecars.join("shot.xmp"), b"xmp");

    assert_eq!(
        XmpPairing::new(&library, Some(&sidecars)).find(&raw),
        Some(expected)
    );
}

#[test]
fn xmp_pairing_uses_the_sibling_when_a_separate_tree_has_no_match() {
    let tree = support::TempTree::new("xmp-pair-sibling");
    let library = tree.path().join("library");
    let sidecars = tree.path().join("sidecars");
    std::fs::create_dir_all(&sidecars).unwrap();
    let raw = write(&library.join("trip/day/shot.arw"), b"raw");
    let expected = write(&library.join("trip/day/shot.xmp"), b"xmp");

    assert_eq!(
        XmpPairing::new(&library, Some(&sidecars)).find(&raw),
        Some(expected)
    );
}

#[test]
fn xmp_pairing_prefers_mirror_then_flat_then_sibling() {
    let tree = support::TempTree::new("xmp-pair-priority");
    let library = tree.path().join("library");
    let sidecars = tree.path().join("sidecars");
    let raw = write(&library.join("trip/day/shot.arw"), b"raw");
    let sibling = write(&library.join("trip/day/shot.xmp"), b"sibling");
    let flat = write(&sidecars.join("shot.xmp"), b"flat");
    let mirror = write(&sidecars.join("trip/day/shot.xmp"), b"mirror");

    assert_eq!(
        XmpPairing::new(&library, Some(&sidecars)).find(&raw),
        Some(mirror.clone())
    );
    std::fs::remove_file(&mirror).unwrap();
    assert_eq!(
        XmpPairing::new(&library, Some(&sidecars)).find(&raw),
        Some(flat.clone())
    );
    std::fs::remove_file(&flat).unwrap();
    assert_eq!(
        XmpPairing::new(&library, Some(&sidecars)).find(&raw),
        Some(sibling)
    );
}

#[test]
fn xmp_pairing_returns_none_when_no_candidate_exists() {
    let tree = support::TempTree::new("xmp-pair-none");
    let library = tree.path().join("library");
    let sidecars = tree.path().join("sidecars");
    let raw = write(&library.join("trip/shot.arw"), b"raw");

    assert!(XmpPairing::new(&library, Some(&sidecars)).find(&raw).is_none());
    assert!(XmpPairing::beside().find(&raw).is_none());
}

#[test]
fn an_absent_xmp_directory_still_allows_the_sibling_sidecar() {
    let tree = support::TempTree::new("xmp-pair-absent-dir");
    let library = tree.path().join("library");
    let raw = write(&library.join("trip/shot.arw"), b"raw");
    let expected = write(&library.join("trip/shot.xmp"), b"xmp");
    let missing = tree.path().join("missing-sidecars");

    assert_eq!(
        XmpPairing::new(&library, Some(&missing)).find(&raw),
        Some(expected)
    );
}
