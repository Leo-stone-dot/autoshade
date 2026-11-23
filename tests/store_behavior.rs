use std::path::{Path, PathBuf};
use std::io::{BufRead, BufReader, Write};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use autoshade::config::{
    LocalSettings, load_local_settings, local_settings_path, save_local_settings,
    update_local_settings,
};
use autoshade::recipe::{EditRecipe, LocalAdjustment, MaskGeometry};
use autoshade::store::{
    ActiveWrite, CommitMember, DevelopCommit, EditStateKind, VariantEntry, VariantsRead,
    VariantsRecord, DevelopLockMode, LrSidecar, MAX_STORE_JSON, OwnedRaster,
    backup_saved_develop, clear_develop, commit_develop, develop_dir, export_xmp_beside,
    lightroom_sidecar, list_edits, migrate_legacy, photo_key, pixel_source_path,
    read_develop_snapshot, read_pixel_source, read_variants_checked, recipe_revision,
    recipe_target, recover_orphan_baks, saved_quarter_turns, set_version_name,
    variants_member, variants_path, version_target, volume_folds_case,
    with_develop_lock, with_settings_lock, write_variants, xmp_target,
};
use autoshade::pipeline::{ProjectionOutcome, commit_analyzed_develop};

mod support;

/// Store integration tests must isolate the process-wide `AUTOSHADE_DATA_DIR`.
/// `ScopedEnv` serializes the override inside this test binary and restores the
/// previous values on drop.
struct TempStore {
    tree: support::TempTree,
    env: support::ScopedEnv,
}

impl TempStore {
    fn new(tag: &str) -> Self {
        let tree = support::TempTree::new(tag);
        let mut env = support::ScopedEnv::new();
        env.set("AUTOSHADE_DATA_DIR", tree.path());
        Self { tree, env }
    }

    fn with_legacy(tag: &str) -> (Self, PathBuf) {
        let mut store = Self::new(tag);
        let legacy = store.tree.path().join("legacy-out");
        std::fs::create_dir_all(&legacy).expect("legacy out is creatable");
        store.env.set("AUTOSHADE_LEGACY_OUT", &legacy);
        (store, legacy)
    }

    fn root(&self) -> &Path {
        self.tree.path()
    }
}

fn photo(root: &Path, name: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::write(&path, b"synthetic photo").expect("synthetic photo is writable");
    path
}

fn valid_recipe() -> EditRecipe {
    EditRecipe {
        exposure_ev: 0.5,
        ..EditRecipe::default()
    }
}

#[test]
fn photo_keys_follow_the_volume_when_two_case_spellings_name_one_object() {
    let store = TempStore::new("photo-key-case");
    let upper = photo(store.root(), "CasePhoto.ARW");
    let lower = store.root().join("cASEpHOTO.arw");
    let same_object = std::fs::metadata(&lower).is_ok();

    assert_eq!(photo_key(&upper) == photo_key(&lower), same_object);
}

#[cfg(windows)]
#[test]
fn a_windows_photo_key_folds_only_ascii_letters_in_a_non_ascii_stem() {
    let store = TempStore::new("photo-key-unicode");
    let photo = photo(store.root(), "\u{130}MG_001.ARW");
    let key = photo_key(&photo);
    let stem = key.rsplit_once('-').unwrap().0;

    assert_eq!(stem, "\u{130}mg_001");
    assert_eq!(stem.chars().count(), "\u{130}MG_001".chars().count());
}

#[cfg(windows)]
#[test]
fn a_windows_photo_key_unifies_trailing_dot_case_and_dotdot_aliases() {
    let store = TempStore::new("photo-key-aliases");
    let photo = photo(store.root(), "DSC001.NEF");
    let plain = photo_key(&photo);

    assert_eq!(photo_key(&store.root().join("dsc001.nef")), plain);
    assert_eq!(photo_key(&store.root().join("DSC001.NEF.")), plain);
    assert_eq!(
        photo_key(&store.root().join("sub").join("..").join("DSC001.NEF")),
        plain
    );
    assert_ne!(photo_key(&store.root().join("DSC002.NEF")), plain);
}

#[test]
fn volume_case_detection_matches_the_filesystem_and_a_missing_leaf() {
    let store = TempStore::new("volume-case");
    let path = photo(store.root(), "Ab.txt");
    let flipped = store.root().join("aB.txt");
    let truth = std::fs::metadata(&flipped).is_ok();

    assert_eq!(volume_folds_case(&path), truth);
    assert_eq!(
        volume_folds_case(&store.root().join("not-written-yet.json")),
        truth
    );
}

#[test]
fn an_unprobeable_empty_directory_uses_the_platform_case_default() {
    let store = TempStore::new("volume-case-empty");
    let empty = store.root().join("empty");
    std::fs::create_dir(&empty).unwrap();

    assert_eq!(
        volume_folds_case(&empty.join("photo.arw")),
        cfg!(windows) || cfg!(target_os = "macos")
    );
}

#[test]
fn two_case_spellings_fold_only_when_they_cannot_be_separate_files() {
    let store = TempStore::new("volume-case-object");
    let first = photo(store.root(), "Cd.txt");
    let second = store.root().join("cD.txt");
    let created_second = std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&second)
        .is_ok();

    assert_eq!(volume_folds_case(&first), !created_second);
}

#[test]
fn claiming_a_raster_for_a_corpus_photo_owns_only_a_store_path() {
    let mut store = TempStore::new("owned-raster-corpus");
    let corpus = store.root().join("calibration-corpus");
    std::fs::create_dir(&corpus).unwrap();
    let photo = photo(&corpus, "source.png");
    store.env.set("AUTOSHADE_FIT_CALIBRATION_DIR", &corpus);

    let raster = OwnedRaster::claim(&photo, "mask-zone-sky").unwrap();

    assert!(raster.path().exists());
    assert!(!raster.path().starts_with(&corpus));
}

#[test]
fn an_orphan_recipe_backup_is_restored_when_the_live_file_is_missing() {
    let store = TempStore::new("orphan-recipe");
    let raw = photo(store.root(), "orphan.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let recipe = serde_json::to_vec_pretty(&valid_recipe()).unwrap();
    std::fs::write(dev.join("recipe.json.bak"), &recipe).unwrap();

    recover_orphan_baks(&raw).unwrap();

    assert_eq!(std::fs::read(recipe_target(&raw)).unwrap(), recipe);
    assert!(!dev.join("recipe.json.bak").exists());
}

#[test]
#[allow(clippy::disallowed_methods)] // this test exercises the kernel lock across threads
fn a_nowait_develop_lock_refuses_while_another_thread_holds_it() {
    let store = TempStore::new("develop-lock-contention");
    let raw = photo(store.root(), "lock.dng");
    let held_raw = raw.clone();
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = thread::spawn(move || {
        with_develop_lock(&held_raw, DevelopLockMode::Wait, || {
            held_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok::<_, std::io::Error>(())
        })
        .unwrap();
    });
    held_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let attempt = thread::spawn(move || {
        with_develop_lock(&raw, DevelopLockMode::NoWait, || Ok::<_, std::io::Error>(()))
    });
    let result = attempt
        .join()
        .expect("the NoWait lock attempt returns without panicking");
    assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));

    release_tx.send(()).unwrap();
    holder.join().unwrap();
}

#[ignore = "worker spawned by a_develop_lock_serializes_across_processes"]
#[test]
fn lock_worker_holds_the_develop_lock_until_released() {
    let Ok(raw) = std::env::var("AUTOSHADE_LOCK_RAW") else {
        return;
    };
    let raw = PathBuf::from(raw);
    with_develop_lock(&raw, DevelopLockMode::Wait, || {
        println!("LOCKED");
        std::io::stdout().flush().unwrap();
        let stdin = std::io::stdin();
        let mut stdin = stdin.lock();
        let mut release = String::new();
        stdin.read_line(&mut release).unwrap();
        Ok::<_, std::io::Error>(())
    })
    .unwrap();
}

#[test]
#[allow(clippy::disallowed_methods)] // the subprocess reader must keep draining stdout
fn a_develop_lock_serializes_across_processes() {
    let store = TempStore::new("develop-lock-process");
    let raw = photo(store.root(), "process-lock.dng");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args([
            "--ignored",
            "--exact",
            "lock_worker_holds_the_develop_lock_until_released",
            "--nocapture",
        ])
        .env("AUTOSHADE_DATA_DIR", store.root())
        .env("AUTOSHADE_LOCK_RAW", &raw)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let stdout = child.stdout.take().unwrap();
    let (locked_tx, locked_rx) = mpsc::channel();
    let reader = thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            if line.is_ok_and(|line| line == "LOCKED") {
                let _ = locked_tx.send(());
            }
        }
    });
    let locked = locked_rx.recv_timeout(Duration::from_secs(5)).is_ok();
    let result = with_develop_lock(&raw, DevelopLockMode::NoWait, || Ok::<_, std::io::Error>(()));
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(b"release\n");
    }
    let status = child.wait().unwrap();
    reader.join().unwrap();

    assert!(locked);
    assert!(matches!(result, Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));
    assert!(status.success());
    assert!(with_develop_lock(&raw, DevelopLockMode::Wait, || Ok::<_, std::io::Error>(())).is_ok());
}

#[test]
fn the_settings_lock_reenters_on_its_own_thread() {
    let _store = TempStore::new("settings-lock-reentrant");

    let value = with_settings_lock(DevelopLockMode::Wait, || {
        with_settings_lock(DevelopLockMode::NoWait, || Ok::<_, std::io::Error>(7))
    })
    .unwrap();

    assert_eq!(value, 7);
}

#[test]
#[allow(clippy::disallowed_methods)] // this test exercises the kernel lock across threads
fn the_settings_lock_refuses_other_threads_until_the_owner_releases_it() {
    let _store = TempStore::new("settings-lock-contention");
    let (held_tx, held_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let holder = thread::spawn(move || {
        with_settings_lock(DevelopLockMode::Wait, || {
            held_tx.send(()).unwrap();
            release_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            Ok::<_, std::io::Error>(())
        })
        .unwrap();
    });
    held_rx.recv_timeout(Duration::from_secs(5)).unwrap();

    let busy =
        thread::spawn(move || with_settings_lock(DevelopLockMode::NoWait, || Ok::<_, std::io::Error>(())))
            .join()
            .unwrap();
    assert!(matches!(busy, Err(error) if error.kind() == std::io::ErrorKind::WouldBlock));

    release_tx.send(()).unwrap();
    holder.join().unwrap();
    assert!(
        with_settings_lock(DevelopLockMode::NoWait, || Ok::<_, std::io::Error>(())).is_ok(),
        "dropping the owner must release the settings lock"
    );
}

#[test]
fn a_live_recipe_is_not_replaced_by_an_orphan_backup() {
    let store = TempStore::new("orphan-recipe-live-wins");
    let raw = photo(store.root(), "live-wins.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(recipe_target(&raw), b"live recipe").unwrap();
    std::fs::write(dev.join("recipe.json.bak"), b"older backup").unwrap();

    recover_orphan_baks(&raw).unwrap();

    assert_eq!(std::fs::read(recipe_target(&raw)).unwrap(), b"live recipe");
    assert_eq!(std::fs::read(dev.join("recipe.json.bak")).unwrap(), b"older backup");
}

#[test]
fn the_newest_lightroom_sidecar_intent_wins_over_the_store() {
    use std::time::{Duration as StdDuration, SystemTime};

    let store = TempStore::new("lightroom-newest-intent");
    let raw = photo(store.root(), "intent.arw");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(
        recipe_target(&raw),
        serde_json::to_vec(&EditRecipe::default()).unwrap(),
    )
    .unwrap();
    let sidecar = raw.with_extension("xmp");
    let lightroom = autoshade::xmp::recipe_to_xmp(&EditRecipe {
        contrast: 30.0,
        ..EditRecipe::default()
    });
    std::fs::write(&sidecar, &lightroom).unwrap();
    let now = SystemTime::now();
    std::fs::OpenOptions::new()
        .write(true)
        .open(recipe_target(&raw))
        .unwrap()
        .set_modified(now)
        .unwrap();

    std::fs::OpenOptions::new()
        .write(true)
        .open(&sidecar)
        .unwrap()
        .set_modified(now - StdDuration::from_secs(7200))
        .unwrap();
    assert!(matches!(
        lightroom_sidecar(&raw),
        LrSidecar::OlderThanStore
    ));

    std::fs::OpenOptions::new()
        .write(true)
        .open(&sidecar)
        .unwrap()
        .set_modified(now + StdDuration::from_secs(7200))
        .unwrap();
    assert!(
        matches!(lightroom_sidecar(&raw), LrSidecar::NewerThanStore(text) if text == lightroom)
    );
}

#[test]
fn orphan_pixel_and_variant_backups_are_restored_together() {
    let store = TempStore::new("orphan-sidecars");
    let raw = photo(store.root(), "orphan-sidecars.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let pixels = br#"{"origin":"master.png","kind":"inplace"}"#;
    let variants = br#"{"v":1,"active_kind":"original","active_pos":0,"others":[]}"#;
    std::fs::write(dev.join("pixels.json.bak"), pixels).unwrap();
    std::fs::write(dev.join("variants.json.bak"), variants).unwrap();

    recover_orphan_baks(&raw).unwrap();

    assert_eq!(std::fs::read(pixel_source_path(&raw)).unwrap(), pixels);
    assert_eq!(std::fs::read(variants_path(&raw)).unwrap(), variants);
    assert!(!dev.join("pixels.json.bak").exists());
    assert!(!dev.join("variants.json.bak").exists());
}

#[test]
fn a_programmatic_backup_preserves_the_previous_recipe_as_a_version() {
    let store = TempStore::new("backup-recipe");
    let raw = photo(store.root(), "backup.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let previous = serde_json::to_vec_pretty(&EditRecipe::default()).unwrap();
    std::fs::write(recipe_target(&raw), &previous).unwrap();

    let version = backup_saved_develop(
        &raw,
        Some(&EditRecipe {
            exposure_ev: 1.0,
            ..EditRecipe::default()
        }),
    )
    .unwrap();

    assert_eq!(version, Some(1));
    assert_eq!(std::fs::read(version_target(&raw, 1)).unwrap(), previous);
}

#[test]
fn a_backup_that_cannot_freeze_a_raster_leaves_the_live_recipe_untouched() {
    let store = TempStore::new("backup-raster-failure");
    let raw = photo(store.root(), "backup-failure.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    // A directory at the raster name is an existing but uncopyable source;
    // the backup must refuse rather than claim a faithful version exists.
    std::fs::create_dir(dev.join("mask.png")).unwrap();
    let previous = serde_json::to_vec_pretty(&EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap {
                path: "mask.png".to_string(),
            },
            ..Default::default()
        }],
        ..Default::default()
    })
    .unwrap();
    std::fs::write(recipe_target(&raw), &previous).unwrap();

    let result = backup_saved_develop(
        &raw,
        Some(&EditRecipe {
            exposure_ev: 1.0,
            ..EditRecipe::default()
        }),
    );

    assert!(result.is_err());
    assert_eq!(std::fs::read(recipe_target(&raw)).unwrap(), previous);
    assert!(!version_target(&raw, 1).exists());
}

#[test]
fn a_develop_commit_publishes_recipe_pixels_and_variants_together() {
    let store = TempStore::new("commit-generation");
    let raw = photo(store.root(), "commit.dng");
    let recipe = br#"{"exposure_ev":0.25}"#.to_vec();
    let pixels = br#"{"origin":"master.png","kind":"inplace"}"#.to_vec();
    let variants = br#"{"v":1,"active_kind":"edited","active_pos":0,"others":[]}"#.to_vec();
    let xmp = b"<x:xmpmeta>generation</x:xmpmeta>".to_vec();

    commit_develop(
        &raw,
        DevelopCommit {
            recipe: Some(recipe.clone()),
            pixels: CommitMember::Write(pixels.clone()),
            variants: CommitMember::Write(variants.clone()),
            xmp: CommitMember::Write(xmp.clone()),
        },
    )
    .unwrap();

    assert_eq!(std::fs::read(recipe_target(&raw)).unwrap(), recipe);
    assert_eq!(std::fs::read(pixel_source_path(&raw)).unwrap(), pixels);
    assert_eq!(std::fs::read(variants_path(&raw)).unwrap(), variants);
    assert_eq!(std::fs::read(xmp_target(&raw)).unwrap(), xmp);
    assert!(!develop_dir(&raw).join(".commit").exists());
}

#[test]
fn a_second_commit_can_write_clear_and_keep_members_together() {
    let store = TempStore::new("commit-member-actions");
    let raw = photo(store.root(), "member-actions.dng");
    let variants = br#"{"v":1,"active_kind":"edited","active_pos":0,"others":[]}"#.to_vec();
    commit_develop(
        &raw,
        DevelopCommit {
            recipe: Some(b"gen1-recipe".to_vec()),
            pixels: CommitMember::Write(
                br#"{"origin":"master.png","kind":"inplace"}"#.to_vec(),
            ),
            variants: CommitMember::Write(variants.clone()),
            xmp: CommitMember::Write(b"gen1-xmp".to_vec()),
        },
    )
    .unwrap();

    commit_develop(
        &raw,
        DevelopCommit {
            recipe: Some(b"gen2-recipe".to_vec()),
            pixels: CommitMember::Clear,
            variants: CommitMember::Keep,
            xmp: CommitMember::Clear,
        },
    )
    .unwrap();

    assert_eq!(std::fs::read(recipe_target(&raw)).unwrap(), b"gen2-recipe");
    assert!(!pixel_source_path(&raw).exists());
    assert_eq!(std::fs::read(variants_path(&raw)).unwrap(), variants);
    assert!(!xmp_target(&raw).exists());
    assert!(!develop_dir(&raw).join(".commit").exists());
}

#[test]
fn an_unresolved_variant_refuses_the_whole_commit_before_staging() {
    let store = TempStore::new("commit-unresolved-variant");
    let raw = photo(store.root(), "unresolved-commit.dng");
    let develop = develop_dir(&raw);
    std::fs::create_dir_all(&develop).unwrap();
    std::fs::write(variants_path(&raw), b"not json at all").unwrap();

    let error = commit_develop(
        &raw,
        DevelopCommit {
            recipe: Some(b"new recipe".to_vec()),
            pixels: CommitMember::Keep,
            variants: CommitMember::Clear,
            xmp: CommitMember::Write(b"new xmp".to_vec()),
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("cannot be honoured"));
    assert!(!recipe_target(&raw).exists());
    assert!(!xmp_target(&raw).exists());
    assert!(!develop.join(".commit").exists());
    assert_eq!(std::fs::read(variants_path(&raw)).unwrap(), b"not json at all");
}

#[test]
fn a_commit_with_an_oversized_member_leaves_no_partial_generation() {
    let store = TempStore::new("commit-oversized-member");
    let raw = photo(store.root(), "oversized.dng");
    let oversized = vec![b'x'; MAX_STORE_JSON as usize + 1];

    let error = commit_develop(
        &raw,
        DevelopCommit {
            recipe: Some(br#"{"exposure_ev":0.25}"#.to_vec()),
            pixels: CommitMember::Write(br#"{"origin":"master.png","kind":"inplace"}"#.to_vec()),
            variants: CommitMember::Write(oversized),
            xmp: CommitMember::Keep,
        },
    )
    .unwrap_err();

    assert!(error.to_string().contains("store limit"), "error={error}");
    assert!(!recipe_target(&raw).exists());
    assert!(!pixel_source_path(&raw).exists());
    assert!(!variants_path(&raw).exists());
    assert!(!develop_dir(&raw).join(".commit").exists());
}

#[test]
fn a_commit_that_fails_after_its_commit_point_recovers_on_the_next_touch() {
    let store = TempStore::new("commit-recovery-after-point");
    let raw = photo(store.root(), "recover-commit.dng");
    let recipe = br#"{"exposure_ev":0.25}"#.to_vec();
    let pixels = br#"{"origin":"master.png","kind":"inplace"}"#.to_vec();
    let xmp = b"projection".to_vec();
    // A directory at the live member path makes publication fail after the
    // staged generation has crossed its commit point.
    std::fs::create_dir_all(xmp_target(&raw).parent().unwrap()).unwrap();
    std::fs::create_dir(xmp_target(&raw)).unwrap();

    let result = commit_develop(
        &raw,
        DevelopCommit {
            recipe: Some(recipe.clone()),
            pixels: CommitMember::Write(pixels.clone()),
            variants: CommitMember::Keep,
            xmp: CommitMember::Write(xmp.clone()),
        },
    );

    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .to_string()
            .contains("save IS committed"),
        "the caller must be told that recovery, not rollback, is required"
    );

    std::fs::remove_dir(xmp_target(&raw)).unwrap();
    recover_orphan_baks(&raw).unwrap();

    assert_eq!(std::fs::read(recipe_target(&raw)).unwrap(), recipe);
    assert_eq!(std::fs::read(pixel_source_path(&raw)).unwrap(), pixels);
    assert_eq!(std::fs::read(xmp_target(&raw)).unwrap(), xmp);
    commit_develop(
        &raw,
        DevelopCommit {
            recipe: Some(br#"{"exposure_ev":0.5}"#.to_vec()),
            pixels: CommitMember::Keep,
            variants: CommitMember::Keep,
            xmp: CommitMember::Keep,
        },
    )
    .unwrap();
}

#[test]
fn an_unresolved_variant_record_is_not_overwritten_by_a_save() {
    let store = TempStore::new("unresolved-variants");
    let raw = photo(store.root(), "unresolved.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let original = br#"{"v":99,"active_kind":"original","active_pos":0,"others":[]}"#;
    std::fs::write(variants_path(&raw), original).unwrap();

    let result = write_variants(
        &raw,
        &VariantsRecord {
            v: 1,
            active_kind: "original".to_string(),
            active_pos: 0,
            others: Vec::new(),
            active_id: None,
            active_name: None,
            extra: serde_json::Map::new(),
        },
    );

    assert!(result.is_err());
    assert_eq!(std::fs::read(variants_path(&raw)).unwrap(), original);
    assert!(matches!(read_variants_checked(&raw), VariantsRead::Unresolved));
}

#[test]
fn a_variant_origin_outside_the_develop_directory_is_unresolved() {
    let store = TempStore::new("variant-origin-escape");
    let raw = photo(store.root(), "escape.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let mut document = serde_json::json!({
        "v": 1,
        "active_kind": "original",
        "active_pos": 0,
        "others": [{
            "kind": "original",
            "recipe": serde_json::to_value(EditRecipe::default()).unwrap(),
            "origin": "../outside.png"
        }]
    });
    document["future_record"] = serde_json::json!("still here");
    let bytes = serde_json::to_vec_pretty(&document).unwrap();
    std::fs::write(variants_path(&raw), &bytes).unwrap();

    assert!(matches!(read_variants_checked(&raw), VariantsRead::Unresolved));
    assert_eq!(std::fs::read(variants_path(&raw)).unwrap(), bytes);
}

#[test]
fn a_malformed_variant_record_is_not_treated_as_absent() {
    let store = TempStore::new("variant-malformed");
    let raw = photo(store.root(), "malformed.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let bytes = b"not a variants record";
    std::fs::write(variants_path(&raw), bytes).unwrap();

    assert!(matches!(read_variants_checked(&raw), VariantsRead::Unresolved));
    assert_eq!(std::fs::read(variants_path(&raw)).unwrap(), bytes);
}

#[test]
fn an_unknown_variant_kind_is_not_treated_as_absent() {
    let store = TempStore::new("variant-unknown-kind");
    let raw = photo(store.root(), "unknown-kind.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let bytes = br#"{"v":1,"active_kind":"future","active_pos":0,"others":[]}"#;
    std::fs::write(variants_path(&raw), bytes).unwrap();

    assert!(matches!(read_variants_checked(&raw), VariantsRead::Unresolved));
    assert_eq!(std::fs::read(variants_path(&raw)).unwrap(), bytes);
}

#[test]
fn an_unknown_pixel_source_kind_is_refused_without_rewriting_the_record() {
    let store = TempStore::new("pixel-source-unknown-kind");
    let raw = photo(store.root(), "future-pixels.dng");
    let develop = develop_dir(&raw);
    std::fs::create_dir_all(&develop).unwrap();
    std::fs::write(develop.join("future-master.png"), b"future master").unwrap();
    let bytes = br#"{"origin":"future-master.png","kind":"layered"}"#;
    std::fs::write(pixel_source_path(&raw), bytes).unwrap();

    assert!(read_pixel_source(&raw).is_none());
    assert_eq!(std::fs::read(pixel_source_path(&raw)).unwrap(), bytes);
}

#[test]
fn changing_the_active_variant_kind_preserves_unknown_fields_and_its_name() {
    let store = TempStore::new("variant-read-modify-write");
    let raw = photo(store.root(), "variants.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let record = VariantsRecord {
        v: 1,
        active_kind: "generated".to_string(),
        active_pos: 0,
        others: vec![VariantEntry {
            kind: "original".to_string(),
            recipe: EditRecipe::default(),
            origin: None,
            id: Some("background".to_string()),
            name: Some("Background".to_string()),
            extra: serde_json::Map::new(),
        }],
        active_id: Some("hero".to_string()),
        active_name: Some("Hero".to_string()),
        extra: serde_json::Map::new(),
    };
    let mut document = serde_json::to_value(&record).unwrap();
    document["future_record"] = serde_json::json!("keep me");
    document["others"][0]["future_entry"] = serde_json::json!("keep me too");
    std::fs::write(
        variants_path(&raw),
        serde_json::to_vec_pretty(&document).unwrap(),
    )
    .unwrap();

    let member = variants_member(&raw, ActiveWrite::Kind("fitted")).unwrap();
    let CommitMember::Write(bytes) = member else {
        panic!("changing a variant kind must write the record");
    };
    let saved: serde_json::Value = serde_json::from_slice(&bytes).unwrap();

    assert_eq!(saved["active_kind"], "fitted");
    assert_eq!(saved["active_name"], "Hero");
    assert_eq!(saved["active_id"], "hero");
    assert_eq!(saved["future_record"], "keep me");
    assert_eq!(saved["others"][0]["future_entry"], "keep me too");
    assert_eq!(saved["others"][0]["name"], "Background");
}

#[test]
fn a_saved_recipe_quarter_turn_is_normalized_for_cache_readers() {
    let store = TempStore::new("saved-quarter-turns");
    let raw = photo(store.root(), "turns.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(
        recipe_target(&raw),
        br#"{"quarter_turns":5,"exposure_ev":0.0}"#,
    )
    .unwrap();

    assert_eq!(saved_quarter_turns(&raw), 1);
    std::fs::remove_file(recipe_target(&raw)).unwrap();
    assert_eq!(saved_quarter_turns(&raw), 0);
    std::fs::write(recipe_target(&raw), b"not json").unwrap();
    assert_eq!(saved_quarter_turns(&raw), 0);
}

#[test]
fn a_recipe_revision_changes_when_the_saved_bytes_change() {
    let store = TempStore::new("recipe-revision");
    let raw = photo(store.root(), "revision.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();

    assert_eq!(recipe_revision(&raw).as_deref(), Some("none"));
    std::fs::write(recipe_target(&raw), br#"{"exposure_ev":0.0}"#).unwrap();
    let first = recipe_revision(&raw);
    std::fs::write(recipe_target(&raw), br#"{"exposure_ev":0.0}"#).unwrap();
    assert_eq!(recipe_revision(&raw), first);
    std::fs::write(recipe_target(&raw), br#"{"exposure_ev":1.0}"#).unwrap();
    let second = recipe_revision(&raw);

    assert!(first.is_some());
    assert!(second.is_some());
    assert_ne!(first, second);
}

#[test]
fn a_develop_snapshot_recovers_an_orphan_recipe_before_reading_it() {
    let store = TempStore::new("snapshot-recovers-recipe");
    let raw = photo(store.root(), "snapshot-recovery.dng");
    let develop = develop_dir(&raw);
    std::fs::create_dir_all(&develop).unwrap();
    let saved = br#"{"contrast":17.0}"#;
    std::fs::write(develop.join("recipe.json.bak"), saved).unwrap();

    let snapshot = read_develop_snapshot(&raw).unwrap();
    let (recipe, path) = snapshot.recipe.unwrap();

    assert_eq!(recipe.as_bytes(), saved);
    assert_eq!(path, recipe_target(&raw));
    assert!(snapshot.recipe_err.is_none());
    assert!(!snapshot.pixel_recorded);
    assert!(snapshot.pixel_source.is_none());
}

#[test]
fn a_develop_snapshot_reads_recipe_projection_and_pixel_source_together() {
    let store = TempStore::new("develop-snapshot");
    let raw = photo(store.root(), "snapshot.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let recipe = br#"{"exposure_ev":0.75}"#;
    let xmp = b"<x:xmpmeta><rdf:RDF/></x:xmpmeta>";
    let master = store.root().join("master.png");
    std::fs::write(&master, b"master").unwrap();
    std::fs::write(recipe_target(&raw), recipe).unwrap();
    std::fs::write(xmp_target(&raw), xmp).unwrap();
    std::fs::write(
        pixel_source_path(&raw),
        serde_json::to_vec(&serde_json::json!({
            "origin": master,
            "kind": "inplace",
        }))
        .unwrap(),
    )
    .unwrap();

    let snapshot = read_develop_snapshot(&raw).unwrap();

    assert_eq!(snapshot.recipe.unwrap().0.as_bytes(), recipe);
    assert_eq!(snapshot.store_xmp.unwrap().0.as_bytes(), xmp);
    assert_eq!(snapshot.pixel_source.unwrap().0, master);
    assert!(snapshot.pixel_recorded);
}

#[test]
fn an_empty_develop_snapshot_reports_absence_without_an_error() {
    let store = TempStore::new("develop-snapshot-empty");
    let raw = photo(store.root(), "empty-snapshot.dng");

    let snapshot = read_develop_snapshot(&raw).unwrap();

    assert!(snapshot.recipe.is_none());
    assert!(snapshot.recipe_err.is_none());
    assert!(snapshot.store_xmp.is_none());
    assert!(snapshot.pixel_source.is_none());
    assert!(!snapshot.pixel_recorded);
}

#[test]
fn exporting_a_sidecar_does_not_replace_lightrooms_existing_file_without_permission() {
    let store = TempStore::new("export-xmp");
    let raw = photo(store.root(), "sidecar.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let projection = b"<x:xmpmeta><rdf:RDF/></x:xmpmeta>";
    std::fs::write(xmp_target(&raw), projection).unwrap();
    let beside = raw.with_extension("xmp");
    std::fs::write(&beside, b"Lightroom's sidecar").unwrap();

    let refused = export_xmp_beside(&raw, false);
    assert!(refused.is_err());
    assert_eq!(std::fs::read(&beside).unwrap(), b"Lightroom's sidecar");

    export_xmp_beside(&raw, true).unwrap();
    assert_eq!(std::fs::read(&beside).unwrap(), projection);
}

#[test]
fn a_legacy_recipe_and_xmp_are_migrated_into_the_develop_store() {
    let (store, legacy) = TempStore::with_legacy("migrate-legacy");
    let raw = photo(store.root(), "legacy.dng");
    let stem = autoshade::pipeline::stem(&raw);
    let recipe = serde_json::to_vec_pretty(&valid_recipe()).unwrap();
    let xmp = b"<x:xmpmeta><rdf:RDF/></x:xmpmeta>";
    std::fs::write(legacy.join(format!("{stem}.recipe.json")), &recipe).unwrap();
    std::fs::write(legacy.join(format!("{stem}.xmp")), xmp).unwrap();

    assert!(migrate_legacy(&raw));

    assert_eq!(std::fs::read(recipe_target(&raw)).unwrap(), recipe);
    assert_eq!(std::fs::read(xmp_target(&raw)).unwrap(), xmp);
}

#[test]
fn an_analyzed_raw_commit_publishes_recipe_and_xmp_together() {
    let store = TempStore::new("analyzed-raw-commit");
    let raw = photo(store.root(), "analyzed.dng");
    let recipe = EditRecipe {
        exposure_ev: 0.75,
        ..EditRecipe::default()
    };

    let outcome = commit_analyzed_develop(&raw, &recipe, autoshade::diag::silent()).unwrap();

    assert!(matches!(outcome, ProjectionOutcome::Written { .. }));
    assert!(recipe_target(&raw).is_file());
    let xmp = std::fs::read_to_string(xmp_target(&raw)).unwrap();
    assert!((autoshade::xmp::xmp_to_recipe(&xmp).exposure_ev - 0.75).abs() < 0.011);
}

#[test]
fn an_analyzed_commit_after_its_commit_point_recovers_when_xmp_conflict_is_removed() {
    let store = TempStore::new("analyzed-xmp-conflict");
    let raw = photo(store.root(), "conflict.dng");
    let develop = develop_dir(&raw);
    std::fs::create_dir_all(&develop).unwrap();
    let xmp = xmp_target(&raw);
    std::fs::create_dir(&xmp).unwrap();

    let error = match commit_analyzed_develop(&raw, &valid_recipe(), autoshade::diag::silent()) {
        Ok(_) => panic!("the XMP directory conflict must fail the first publication"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("save IS committed"));
    assert!(recipe_target(&raw).is_file());
    assert!(xmp.is_dir());
    std::fs::remove_dir(&xmp).unwrap();

    let outcome = commit_analyzed_develop(&raw, &valid_recipe(), autoshade::diag::silent())
        .expect("the promised generation completes after the conflict is removed");

    assert!(matches!(outcome, ProjectionOutcome::Written { .. }));
    assert!(xmp_target(&raw).is_file());
}

#[test]
fn an_analyzed_baked_commit_clears_the_lightroom_projection() {
    let store = TempStore::new("analyzed-baked-commit");
    let baked = photo(store.root(), "analyzed.png");
    let dev = develop_dir(&baked);
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(xmp_target(&baked), b"old projection").unwrap();
    let recipe = EditRecipe {
        exposure_ev: 0.75,
        ..EditRecipe::default()
    };

    let outcome = commit_analyzed_develop(&baked, &recipe, autoshade::diag::silent()).unwrap();

    assert!(matches!(outcome, ProjectionOutcome::Cleared));
    assert!(recipe_target(&baked).is_file());
    assert!(!xmp_target(&baked).exists());
}

#[test]
fn clearing_a_develop_removes_live_files_but_keeps_saved_versions() {
    let store = TempStore::new("clear-live");
    let raw = photo(store.root(), "clear.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(recipe_target(&raw), serde_json::to_vec(&valid_recipe()).unwrap()).unwrap();
    std::fs::write(xmp_target(&raw), b"projection").unwrap();
    std::fs::write(variants_path(&raw), br#"{"v":1,"active_kind":"original","active_pos":0,"others":[]}"#).unwrap();
    std::fs::write(pixel_source_path(&raw), br#"{"origin":"master.png","kind":"inplace"}"#).unwrap();
    std::fs::write(version_target(&raw, 1), serde_json::to_vec(&valid_recipe()).unwrap()).unwrap();

    let outcome = clear_develop(&raw).unwrap();

    assert!(outcome.removed);
    assert!(!recipe_target(&raw).exists());
    assert!(!xmp_target(&raw).exists());
    assert!(!variants_path(&raw).exists());
    assert!(!pixel_source_path(&raw).exists());
    assert!(version_target(&raw, 1).exists());
}

#[test]
fn clearing_a_develop_removes_backups_that_could_resurrect_it() {
    let store = TempStore::new("clear-backups");
    let raw = photo(store.root(), "clear-backup.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(recipe_target(&raw), b"live recipe").unwrap();
    std::fs::write(dev.join("recipe.json.bak"), b"old recipe").unwrap();
    std::fs::write(dev.join("pixels.json.bak"), b"old pixels").unwrap();

    clear_develop(&raw).unwrap();

    assert!(!recipe_target(&raw).exists());
    assert!(!dev.join("recipe.json.bak").exists());
    assert!(!dev.join("pixels.json.bak").exists());
}

#[test]
fn a_named_snapshot_appears_in_the_edit_state_list_without_changing_its_recipe() {
    let store = TempStore::new("named-snapshot");
    let raw = photo(store.root(), "snapshot.dng");
    let dev = develop_dir(&raw);
    std::fs::create_dir_all(&dev).unwrap();
    let recipe = serde_json::to_vec(&valid_recipe()).unwrap();
    std::fs::write(version_target(&raw, 1), &recipe).unwrap();
    set_version_name(&raw, 1, Some("warm afternoon")).unwrap();

    let edits = list_edits(&raw);

    let version = edits
        .iter()
        .find(|edit| matches!(edit.state, EditStateKind::Version { n: 1, .. }))
        .expect("the snapshot is listed");
    assert_eq!(version.name.as_deref(), Some("warm afternoon"));
    assert_eq!(std::fs::read(version_target(&raw, 1)).unwrap(), recipe);
}

#[test]
fn writing_a_standalone_xmp_projection_creates_a_readable_sidecar() {
    let store = TempStore::new("write-xmp");
    let raw = photo(store.root(), "projection.dng");
    let recipe = EditRecipe {
        exposure_ev: 0.75,
        ..EditRecipe::default()
    };

    let (path, _, losses) =
        autoshade::pipeline::write_xmp(&raw, &recipe, autoshade::diag::silent()).unwrap();

    assert!(path.is_file());
    assert!(losses.is_empty());
    let xmp = std::fs::read_to_string(path).unwrap();
    assert!((autoshade::xmp::xmp_to_recipe(&xmp).exposure_ev - 0.75).abs() < 0.011);
}

#[cfg(unix)]
#[test]
fn a_failed_xmp_publish_leaves_the_previous_projection_intact() {
    let store = TempStore::new("xmp-publish-failure");
    let raw = photo(store.root(), "publish-failure.dng");
    let develop = develop_dir(&raw);
    std::fs::create_dir_all(&develop).unwrap();
    let old_recipe = EditRecipe {
        exposure_ev: 0.25,
        ..EditRecipe::default()
    };
    let (target, _, _) =
        autoshade::pipeline::write_xmp(&raw, &old_recipe, autoshade::diag::silent()).unwrap();
    let old_bytes = std::fs::read(&target).unwrap();
    std::fs::set_permissions(&develop, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = autoshade::pipeline::write_xmp(
        &raw,
        &EditRecipe { exposure_ev: 0.75, ..EditRecipe::default() },
        autoshade::diag::silent(),
    );

    std::fs::set_permissions(&develop, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
    assert_eq!(std::fs::read(&target).unwrap(), old_bytes);
}

#[test]
fn an_unmergeable_sidecar_is_regenerated_with_a_loss_note() {
    let store = TempStore::new("xmp-regenerate");
    let raw = photo(store.root(), "regenerate.dng");
    let out = raw.with_extension("xmp");
    std::fs::write(
        &out,
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="urn:not-camera-raw" crs:Exposure2012="0"/></rdf:RDF></x:xmpmeta>"#,
    )
    .unwrap();
    let recipe = EditRecipe {
        exposure_ev: 0.5,
        ..EditRecipe::default()
    };
    let diag = autoshade::diag::Diag::about(autoshade::diag::silent(), &raw);

    let (path, note, _) = autoshade::pipeline::write_xmp_at(out, &recipe, &diag).unwrap();

    assert_eq!(path, raw.with_extension("xmp"));
    assert!(note.as_deref().is_some_and(|text| text.contains("regenerated")));
    let written = std::fs::read_to_string(path).unwrap();
    assert!((autoshade::xmp::xmp_to_recipe(&written).exposure_ev - 0.5).abs() < 0.011);
}

#[test]
fn saved_local_settings_round_trip_through_the_per_user_store() {
    let store = TempStore::new("settings-roundtrip");
    let settings = LocalSettings {
        analysis_model: Some("synthetic-analysis".to_string()),
        image_model: Some("synthetic-vision".to_string()),
        ..LocalSettings::default()
    };

    let path = save_local_settings(&settings).unwrap();

    assert_eq!(path, local_settings_path());
    assert!(path.starts_with(store.root()));
    let loaded = load_local_settings();
    assert_eq!(loaded.analysis_model, settings.analysis_model);
    assert_eq!(loaded.image_model, settings.image_model);
}

#[test]
fn updating_one_local_setting_preserves_the_others() {
    let _store = TempStore::new("settings-merge");
    save_local_settings(&LocalSettings {
        image_model: Some("keep-this-model".to_string()),
        ..LocalSettings::default()
    })
    .unwrap();

    update_local_settings(|settings| {
        settings.analysis_model = Some("new-analysis-model".to_string());
    })
    .unwrap();

    let loaded = load_local_settings();
    assert_eq!(loaded.analysis_model.as_deref(), Some("new-analysis-model"));
    assert_eq!(loaded.image_model.as_deref(), Some("keep-this-model"));
}

#[cfg(unix)]
#[test]
fn saved_local_settings_are_private_to_the_user() {
    use std::os::unix::fs::PermissionsExt;

    let _store = TempStore::new("settings-private");

    let path = save_local_settings(&LocalSettings::default()).unwrap();

    let mode = std::fs::metadata(path).unwrap().permissions().mode() & 0o777;
    assert_eq!(mode, 0o600);
}
