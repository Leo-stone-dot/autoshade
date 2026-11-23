mod support;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use image::{Rgb, RgbImage};
use support::{ScopedEnv, TempTree};

#[cfg(unix)]
fn fake_comfy(tree: &TempTree, generated: &std::path::Path) -> std::path::PathBuf {
    let script = tree.path().join("fake-comfy");
    std::fs::write(
        &script,
        "#!/bin/sh\nset -eu\nif [ \"${AUTOSHADE_TEST_COMFY_FAIL:-}\" = 1 ]; then printf '%s\\n' '{\"ok\":false,\"error\":\"synthetic comfy failure\"}'; exit 0; fi\nargs=\"$*\"\ncase \"$args\" in\n  *\"--json env\"*) printf '%s\\n' '{\"ok\":true,\"data\":{\"server\":{\"running\":true}}}' ;;\n  *\"--json upload\"*) printf '%s\\n' '{\"ok\":true,\"data\":{\"uploads\":[{\"name\":\"input.png\"}]}}' ;;\n  *\"run --workflow\"*) : ;;\n  *\"download --out-dir\"*) out=\"\"; while [ $# -gt 0 ]; do case \"$1\" in --out-dir) out=\"$2\"; shift 2;; *) shift;; esac; done; cp \"$AUTOSHADE_TEST_GENERATED\" \"$out/generated.png\" ;;\n  *) printf '%s\\n' '{\"ok\":true,\"data\":{}}' ;;\nesac\n",
    )
    .unwrap();
    std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
    let workflow = tree.path().join("workflow.json");
    std::fs::write(&workflow, b"{}").unwrap();
    let _ = generated;
    script
}

#[cfg(unix)]
fn configure(tree: &TempTree, script: &std::path::Path, generated: &std::path::Path) -> ScopedEnv {
    let workflow = tree.path().join("workflow.json");
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_GENERATIVE", "comfy");
    env.set("AUTOSHADE_COMFY_CLI", script);
    env.set("AUTOSHADE_COMFY_WORKSPACE", tree.path());
    env.set("AUTOSHADE_COMFY_MAX_EDGE", "128");
    env.set("AUTOSHADE_COMFY_REIMAGINE_WORKFLOW", &workflow);
    env.set("AUTOSHADE_TEST_GENERATED", generated);
    env
}

#[cfg(unix)]
#[test]
fn a_successful_reimagine_publishes_one_complete_generated_image() {
    let tree = TempTree::new("generative-success");
    let source = tree.path().join("source.png");
    let generated = tree.path().join("generated.png");
    let out = tree.path().join("out/generated.png");
    RgbImage::from_pixel(16, 12, Rgb([40, 80, 120]))
        .save(&source)
        .unwrap();
    RgbImage::from_pixel(64, 64, Rgb([180, 80, 40]))
        .save(&generated)
        .unwrap();
    let script = fake_comfy(&tree, &generated);
    let _env = configure(&tree, &script, &generated);
    let config = autoshade::config::Config::load();

    let report = autoshade::generative::reimagine(
        &config,
        &source,
        "synthetic direction",
        "high",
        "high",
        false,
        &out,
    )
    .unwrap();

    let bytes = std::fs::read(&out).unwrap();
    assert!(bytes.starts_with(b"\x89PNG\r\n\x1a\n"));
    assert_eq!(
        image::load_from_memory(&bytes).unwrap().to_rgba8(),
        image::open(&generated).unwrap().to_rgba8(),
        "the published rendition must contain the complete model image",
    );
    assert!(report.divergence.d.is_finite());
}

#[cfg(unix)]
#[test]
fn a_failed_reimagine_does_not_install_a_partial_generated_image() {
    let tree = TempTree::new("generative-failure");
    let source = tree.path().join("source.png");
    let generated = tree.path().join("generated.png");
    let out = tree.path().join("out/generated.png");
    RgbImage::from_pixel(16, 12, Rgb([40, 80, 120]))
        .save(&source)
        .unwrap();
    RgbImage::from_pixel(64, 64, Rgb([180, 80, 40]))
        .save(&generated)
        .unwrap();
    let script = fake_comfy(&tree, &generated);
    let mut env = configure(&tree, &script, &generated);
    env.set("AUTOSHADE_TEST_COMFY_FAIL", "1");
    let config = autoshade::config::Config::load();

    let error = autoshade::generative::reimagine(
        &config,
        &source,
        "synthetic direction",
        "high",
        "high",
        false,
        &out,
    )
    .unwrap_err();

    assert!(!out.exists());
    assert!(error.to_string().contains("synthetic") || error.to_string().contains("comfy"));
}
