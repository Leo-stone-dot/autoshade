use std::path::PathBuf;
use std::process::{Command, Output};

use image::{GenericImageView, Rgb, RgbImage};

mod support;

fn photo(tree: &support::TempTree) -> PathBuf {
    let path = tree.path().join("photo.png");
    RgbImage::from_pixel(16, 12, Rgb([48, 64, 80]))
        .save(&path)
        .unwrap();
    path
}

fn command(tree: &support::TempTree) -> Command {
    let mut command = autoshade::sidecar_command(env!("CARGO_BIN_EXE_autoshade"));
    command
        .current_dir(tree.path())
        .env_clear()
        .env("HOME", tree.path())
        .env("AUTOSHADE_DATA_DIR", tree.path().join("data"))
        .env("AUTOSHADE_OUT_DIR", tree.path().join("out"));
    command
}

fn run(tree: &support::TempTree, args: &[&std::ffi::OsStr]) -> Output {
    command(tree).args(args).output().unwrap()
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn cli_decode_writes_a_complete_preview_to_the_requested_path() {
    let tree = support::TempTree::new("cli-decode");
    let source = photo(&tree);
    let source_bytes = std::fs::read(&source).unwrap();
    let out = tree.path().join("deliveries").join("preview.jpg");

    let output = run(
        &tree,
        &[
            "decode".as_ref(),
            source.as_os_str(),
            "--out".as_ref(),
            out.as_os_str(),
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    assert_eq!(image::open(&out).unwrap().dimensions(), (16, 12));
    assert_eq!(std::fs::read(source).unwrap(), source_bytes);
}

#[test]
fn cli_apply_renders_the_recipe_without_changing_the_source_photo() {
    let tree = support::TempTree::new("cli-apply");
    let source = photo(&tree);
    let source_bytes = std::fs::read(&source).unwrap();
    let recipe = tree.path().join("recipe.json");
    std::fs::write(
        &recipe,
        serde_json::to_vec_pretty(&autoshade::recipe::EditRecipe {
            exposure_ev: 1.0,
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();
    let out = tree.path().join("deliveries").join("developed.png");

    let output = run(
        &tree,
        &[
            "apply".as_ref(),
            source.as_os_str(),
            recipe.as_os_str(),
            "--out".as_ref(),
            out.as_os_str(),
        ],
    );

    assert!(output.status.success(), "{}", stderr(&output));
    let developed = image::open(&out).unwrap().to_rgb8();
    assert!(
        developed.get_pixel(8, 6).0[0] > 48,
        "the requested exposure did not reach the delivery"
    );
    assert_eq!(std::fs::read(source).unwrap(), source_bytes);
}

#[test]
fn cli_apply_refuses_to_replace_the_source_photo_with_its_delivery() {
    let tree = support::TempTree::new("cli-apply-source");
    let source = photo(&tree);
    let source_bytes = std::fs::read(&source).unwrap();
    let recipe = tree.path().join("recipe.json");
    std::fs::write(
        &recipe,
        serde_json::to_vec_pretty(&autoshade::recipe::EditRecipe {
            exposure_ev: 1.0,
            ..Default::default()
        })
        .unwrap(),
    )
    .unwrap();

    let output = run(
        &tree,
        &[
            "apply".as_ref(),
            source.as_os_str(),
            recipe.as_os_str(),
            "--out".as_ref(),
            source.as_os_str(),
        ],
    );

    assert!(!output.status.success(), "apply unexpectedly replaced its input");
    assert!(
        stderr(&output).contains("refusing to overwrite"),
        "{}",
        stderr(&output)
    );
    assert_eq!(std::fs::read(source).unwrap(), source_bytes);
}
