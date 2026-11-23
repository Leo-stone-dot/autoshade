mod support;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use autoshade::describe::DescribeOpts;
use autoshade::embed::{EMBED_DIM, EmbedOpts};
use autoshade::library::{DescribeSwitch, Sidecars, build};
use autoshade::recipe::EditRecipe;
use image::{Rgb, RgbImage};
use rawler::decoders::Orientation;
use support::TempTree;

fn unit_vector() -> Vec<f32> {
    let mut vector = vec![0.0; EMBED_DIM];
    vector[0] = 1.0;
    vector
}

#[cfg(unix)]
#[test]
fn a_library_build_reuses_unchanged_embeddings_and_embeds_only_new_photos() {
    let tree = TempTree::new("library-build-reuse");
    let photos = tree.path().join("photos");
    let scratch = tree.path().join("scratch");
    std::fs::create_dir_all(&photos).unwrap();
    let first = photos.join("first.png");
    RgbImage::from_pixel(8, 8, Rgb([40, 80, 120])).save(&first).unwrap();
    let vector = tree.path().join("vector.json");
    std::fs::write(&vector, serde_json::to_vec(&unit_vector()).unwrap()).unwrap();
    let counter = tree.path().join("embed-count");
    let sidecar = tree.path().join("fake-embed");
    let script = tree.path().join("fake-embed.py");
    std::fs::write(
        &sidecar,
        "#!/bin/sh\nset -eu\nmanifest=\nout=\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    --manifest-jsonl) manifest=\"$2\"; shift 2;;\n    --output) out=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\ncount=0\n[ -f \"$AUTOSHADE_TEST_EMBED_COUNT\" ] && count=$(cat \"$AUTOSHADE_TEST_EMBED_COUNT\")\ncount=$((count + 1))\nprintf '%s' \"$count\" > \"$AUTOSHADE_TEST_EMBED_COUNT\"\nvec=$(cat \"$AUTOSHADE_TEST_VECTOR\")\n: > \"$out\"\nwhile IFS= read -r line; do\n  path=$(printf '%s\\n' \"$line\" | sed -n 's/.*\"path\":\"\\([^\"]*\\)\".*/\\1/p')\n  [ -n \"$path\" ] || continue\n  printf '{\"path\":\"%s\",\"vector\":%s}\\n' \"$path\" \"$vec\" >> \"$out\"\ndone < \"$manifest\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&script, b"fake").unwrap();
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_TEST_EMBED_COUNT", &counter);
    env.set("AUTOSHADE_TEST_VECTOR", &vector);
    let sidecars = Sidecars {
        embed: EmbedOpts {
            python_bin: sidecar.to_string_lossy().into_owned(),
            script: script.clone(),
            text_file: None,
        },
        describe: DescribeOpts {
            python_bin: "unused".to_string(),
            script: tree.path().join("unused-describe.py"),
        },
        scratch: scratch.clone(),
    };
    let first_library = build(
        &photos,
        &sidecars,
        None,
        DescribeSwitch::OFF,
        None,
        &|_| {},
    )
    .unwrap();
    assert_eq!(first_library.entries.len(), 1);
    assert_eq!(std::fs::read_to_string(&counter).unwrap(), "1");
    let first_id = first_library.entries[0].id.clone();

    let second_library = build(
        &photos,
        &sidecars,
        Some(&first_library),
        DescribeSwitch::OFF,
        None,
        &|_| {},
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&counter).unwrap(), "1");
    assert_eq!(second_library.entries.len(), 1);
    assert_eq!(second_library.entries[0].id, first_id);

    let second = photos.join("second.png");
    RgbImage::from_pixel(8, 8, Rgb([120, 80, 40])).save(&second).unwrap();
    let third_library = build(
        &photos,
        &sidecars,
        Some(&second_library),
        DescribeSwitch::OFF,
        None,
        &|_| {},
    )
    .unwrap();

    assert_eq!(std::fs::read_to_string(&counter).unwrap(), "2");
    assert_eq!(third_library.entries.len(), 2);
    assert!(third_library.entries.iter().any(|entry| entry.id == first_id));
}

#[cfg(unix)]
#[test]
fn a_reference_sidecar_is_attached_only_when_it_renders_to_the_finished_photo() {
    let tree = TempTree::new("library-settings-match");
    let photos = tree.path().join("photos");
    let raws = tree.path().join("raws");
    let scratch = tree.path().join("scratch");
    std::fs::create_dir_all(&photos).unwrap();
    std::fs::create_dir_all(&raws).unwrap();
    let raw = raws.join("photo.dng");
    std::fs::write(
        &raw,
        support::synthetic_dng(64, 48, Orientation::Normal, None, false),
    )
    .unwrap();
    let matching = autoshade::render::render_to_image_in(
        &raw,
        &EditRecipe::default(),
        None,
        Some(512),
        autoshade::render::ExportColorSpace::Srgb,
        autoshade::diag::silent(),
    )
    .unwrap();
    matching.save(photos.join("photo.png")).unwrap();
    RgbImage::from_pixel(64, 48, Rgb([255, 0, 255]))
        .save(photos.join("photo-bad.png"))
        .unwrap();
    std::fs::write(
        raw.with_extension("xmp"),
        autoshade::xmp::recipe_to_xmp(&EditRecipe::default()),
    )
    .unwrap();
    let vector = tree.path().join("vector.json");
    std::fs::write(&vector, serde_json::to_vec(&unit_vector()).unwrap()).unwrap();
    let sidecar = tree.path().join("fake-embed");
    let script = tree.path().join("fake-embed.py");
    std::fs::write(
        &sidecar,
        "#!/bin/sh\nset -eu\nmanifest=\nout=\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    --manifest-jsonl) manifest=\"$2\"; shift 2;;\n    --output) out=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\nvec=$(cat \"$AUTOSHADE_TEST_VECTOR\")\n: > \"$out\"\nwhile IFS= read -r line; do\n  path=$(printf '%s\\n' \"$line\" | sed -n 's/.*\"path\":\"\\([^\"]*\\)\".*/\\1/p')\n  [ -n \"$path\" ] || continue\n  printf '{\"path\":\"%s\",\"vector\":%s}\\n' \"$path\" \"$vec\" >> \"$out\"\ndone < \"$manifest\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&script, b"fake").unwrap();
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_TEST_VECTOR", &vector);
    let sidecars = Sidecars {
        embed: EmbedOpts {
            python_bin: sidecar.to_string_lossy().into_owned(),
            script,
            text_file: None,
        },
        describe: DescribeOpts {
            python_bin: "unused".to_string(),
            script: tree.path().join("unused-describe.py"),
        },
        scratch,
    };

    let library = build(
        &photos,
        &sidecars,
        None,
        DescribeSwitch::OFF,
        Some(&raws),
        &|_| {},
    )
    .unwrap();

    let matching = library
        .entries
        .iter()
        .find(|entry| entry.path.ends_with("photo.png"))
        .expect("the matching finished photo is in the library");
    let bad = library
        .entries
        .iter()
        .find(|entry| entry.path.ends_with("photo-bad.png"))
        .expect("the mismatching finished photo is in the library");
    assert!(matching.settings.is_some());
    assert!(matching.settings.as_ref().unwrap().match_de <= autoshade::library::MATCH_DE_MAX);
    assert!(bad.settings.is_none());
}
