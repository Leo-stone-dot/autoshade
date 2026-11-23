mod support;

use std::path::PathBuf;

use autoshade::adobe::{camera_raw_roots, walk_extension};
use support::{ScopedEnv, TempTree};

const ADOBE_OVERRIDE: &str = "AUTOSHADE_TEST_ADOBE_BEHAVIOR_DIR";

#[test]
fn an_existing_adobe_override_is_the_first_resource_root() {
    let tree = TempTree::new("adobe-override");
    let mut env = ScopedEnv::new();
    env.set(ADOBE_OVERRIDE, tree.path());

    let roots = camera_raw_roots(ADOBE_OVERRIDE, "Profiles");

    assert_eq!(roots.first().map(PathBuf::as_path), Some(tree.path()));
    assert!(roots.iter().all(|path| path.is_dir()));
}

#[test]
fn a_missing_adobe_override_is_not_a_resource_root() {
    let tree = TempTree::new("adobe-missing");
    let missing = tree.path().join("missing");
    let mut env = ScopedEnv::new();
    env.set(ADOBE_OVERRIDE, &missing);

    assert!(!camera_raw_roots(ADOBE_OVERRIDE, "Profiles").contains(&missing));
}

#[test]
fn an_empty_adobe_override_does_not_make_the_working_directory_a_resource_root() {
    let mut env = ScopedEnv::new();
    env.set(ADOBE_OVERRIDE, "");

    assert!(!camera_raw_roots(ADOBE_OVERRIDE, "Profiles").contains(&PathBuf::new()));
}

#[test]
fn an_adobe_resource_walk_is_recursive_and_extension_case_insensitive() {
    let tree = TempTree::new("adobe-recursive");
    let nested = tree.path().join("vendor").join("nested");
    std::fs::create_dir_all(&nested).unwrap();
    std::fs::write(tree.path().join("top.lcp"), b"x").unwrap();
    std::fs::write(nested.join("deep.LCP"), b"x").unwrap();
    std::fs::write(nested.join("other.dcp"), b"x").unwrap();
    std::fs::write(nested.join("notes.txt"), b"x").unwrap();

    let mut names = walk_extension(vec![tree.path().to_path_buf()], "lcp", 1000)
        .into_iter()
        .map(|path| path.file_name().unwrap().to_string_lossy().into_owned())
        .collect::<Vec<_>>();
    names.sort();

    assert_eq!(names, ["deep.LCP", "top.lcp"]);
}

#[test]
fn an_adobe_resource_walk_uses_the_requested_extension() {
    let tree = TempTree::new("adobe-extension");
    std::fs::write(tree.path().join("profile.lcp"), b"x").unwrap();
    std::fs::write(tree.path().join("camera.dcp"), b"x").unwrap();

    let found = walk_extension(vec![tree.path().to_path_buf()], "dcp", 1000);

    assert_eq!(found.len(), 1);
    assert_eq!(found[0].file_name().unwrap(), "camera.dcp");
}

#[test]
fn a_zero_adobe_walk_budget_reads_no_entries() {
    let tree = TempTree::new("adobe-zero-budget");
    std::fs::write(tree.path().join("profile.lcp"), b"x").unwrap();

    assert!(walk_extension(vec![tree.path().to_path_buf()], "lcp", 0).is_empty());
}

#[test]
fn a_missing_adobe_resource_root_is_a_quiet_empty_result() {
    let tree = TempTree::new("adobe-missing-root");
    let missing = tree.path().join("missing");

    assert!(walk_extension(vec![missing], "lcp", 10).is_empty());
}
