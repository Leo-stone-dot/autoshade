mod support;

use std::path::PathBuf;
#[cfg(unix)]
use std::time::{Duration, Instant};

use autoshade::denoise::{DenoiseOpts, MosaicDenoise, denoise_buffer, denoise_mosaic};
use image::{DynamicImage, ImageBuffer, Luma};
use rawler::pixarray::PixU16;
use rawler::rawimage::{
    BlackLevel, RawImageData, RawPhotometricInterpretation, WhiteLevel,
};
use rawler::{CFA, RawImage, decoders::Camera, rawimage::CFAConfig};
use support::{ScopedEnv, TempTree};

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

fn synthetic_raw() -> RawImage {
    let mut camera = Camera::new();
    camera.cfa = CFA::new("RGGB");
    let photometric = RawPhotometricInterpretation::Cfa(CFAConfig::new_from_camera(&camera));
    RawImage::new(
        camera,
        PixU16::new_with((0..16 * 16).map(|i| (i * 17 % 4096) as u16).collect(), 16, 16),
        1,
        [1.0, 1.0, 1.0, 1.0],
        photometric,
        Some(BlackLevel::new(&[0_u32, 0, 0, 0], 2, 2, 1)),
        Some(WhiteLevel::new_bits(12, 1)),
        false,
    )
}

fn options(strength: f32, raw_script: PathBuf) -> DenoiseOpts {
    DenoiseOpts {
        python_bin: "autoshade-test-python".to_string(),
        script: PathBuf::from("unused-denoise.py"),
        raw_script,
        cache: PathBuf::from("unused-cache"),
        model: "test".to_string(),
        strength,
    }
}

#[test]
fn a_zero_strength_mosaic_denoise_is_the_identity_without_a_sidecar() {
    let mut raw = synthetic_raw();
    let before = match &raw.data {
        RawImageData::Integer(pixels) => pixels.clone(),
        RawImageData::Float(_) => panic!("fixture is integer"),
    };

    let result = denoise_mosaic(
        &options(0.0, PathBuf::from("missing-raw-denoise.py")),
        &mut raw,
    )
    .unwrap();

    assert_eq!(result, MosaicDenoise::Denoised);
    match &raw.data {
        RawImageData::Integer(after) => assert_eq!(after, &before),
        RawImageData::Float(_) => panic!("zero-strength denoise changed the mosaic type"),
    }
}

#[test]
fn a_zero_strength_developed_frame_is_the_identity_without_a_sidecar() {
    let mut pixels = [[1.2_f32, 0.5, -0.1]];
    let before = pixels;

    denoise_buffer(
        &options(0.0, PathBuf::from("missing-raw-denoise.py")),
        &mut pixels,
        1,
        1,
    )
    .unwrap();

    assert_eq!(pixels, before);
}

#[test]
fn a_float_mosaic_is_not_applicable_to_raw_denoise() {
    let mut raw = synthetic_raw();
    raw.data = RawImageData::Float(vec![0.5; 16 * 16]);

    let result = denoise_mosaic(
        &options(1.0, PathBuf::from("missing-raw-denoise.py")),
        &mut raw,
    )
    .unwrap();

    assert!(matches!(result, MosaicDenoise::NotApplicable(reason) if reason.contains("floating point")));
}

#[test]
fn a_missing_raw_denoise_sidecar_is_reported() {
    let mut raw = synthetic_raw();

    let error = denoise_mosaic(
        &options(1.0, PathBuf::from("missing-raw-denoise.py")),
        &mut raw,
    )
    .unwrap_err();

    assert!(error.to_string().contains("RAW denoise sidecar not found"));
}

#[cfg(unix)]
#[test]
fn a_successful_raw_denoise_adopts_the_same_sized_16_bit_mosaic() {
    let tree = TempTree::new("denoise-raw-success");
    let output = tree.path().join("denoised.png");
    let plane = ImageBuffer::<Luma<u16>, Vec<u16>>::from_raw(
        16,
        16,
        vec![1234; 16 * 16],
    )
    .unwrap();
    DynamicImage::ImageLuma16(plane).save(&output).unwrap();
    let sidecar = tree.path().join("fake-denoise-raw");
    let script = tree.path().join("fake-denoise-raw.py");
    std::fs::write(
        &sidecar,
        b"#!/bin/sh\nset -eu\nout=\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    --output) out=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\ncp \"$AUTOSHADE_TEST_DENOISE_OUTPUT\" \"$out\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&script, b"synthetic sidecar").unwrap();
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_TEST_DENOISE_OUTPUT", &output);
    let mut raw = synthetic_raw();
    let mut opts = options(1.0, script);
    opts.python_bin = sidecar.to_string_lossy().into_owned();

    let result = denoise_mosaic(&opts, &mut raw).unwrap();

    assert_eq!(result, MosaicDenoise::Denoised);
    assert_eq!((raw.width, raw.height), (16, 16));
    match &raw.data {
        RawImageData::Integer(pixels) => assert_eq!(pixels[0], 1234),
        RawImageData::Float(_) => panic!("a successful mosaic denoise must keep integer samples"),
    }
}

#[cfg(unix)]
#[test]
fn a_timed_out_denoise_sidecar_leaves_the_buffer_untouched_and_reaps_its_group() {
    let tree = TempTree::new("denoise-timeout");
    let sidecar = tree.path().join("slow-denoise");
    let script = tree.path().join("slow-denoise.py");
    let pidfile = tree.path().join("grandchild.pid");
    std::fs::write(
        &sidecar,
        b"#!/bin/sh\nset -eu\nsleep 30 &\necho $! > \"$AUTOSHADE_TEST_TIMEOUT_PIDFILE\"\nwait\n",
    )
    .unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&script, b"synthetic sidecar").unwrap();
    let mut env = ScopedEnv::new();
    // Leave enough time for the shell to start and publish the grandchild pid
    // before the timeout kills the group; one second flaked under a loaded
    // release battery even though the child is spawned immediately.
    env.set("AUTOSHADE_SIDECAR_TIMEOUT_SECS", "2");
    env.set("AUTOSHADE_TEST_TIMEOUT_PIDFILE", &pidfile);
    let mut opts = options(1.0, tree.path().join("unused-raw.py"));
    opts.python_bin = sidecar.to_string_lossy().into_owned();
    opts.script = script;
    let mut pixels = vec![[0.25, 0.5, 0.75]];

    let error = denoise_buffer(&opts, &mut pixels, 1, 1).unwrap_err();

    assert!(error.to_string().contains("timed out"), "error={error:#}");
    assert_eq!(pixels, [[0.25, 0.5, 0.75]]);
    let grandchild = std::fs::read_to_string(&pidfile)
        .expect("the timed-out sidecar starts its descendant before the deadline")
        .trim()
        .to_string();
    let deadline = Instant::now() + Duration::from_secs(2);
    let alive = loop {
        let mut command = autoshade::sidecar_command("kill");
        command.args(["-0", &grandchild]);
        let status = autoshade::spawn_child(&mut command).unwrap().wait().unwrap();
        if !status.success() {
            break false;
        }
        if Instant::now() >= deadline {
            break true;
        }
        std::thread::yield_now();
    };
    assert!(!alive, "the timed-out sidecar descendant {grandchild} survived");
}
