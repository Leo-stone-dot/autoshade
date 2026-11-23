use autoshade::fit::fit_recipe;
use autoshade::recipe::{
    CurvePoint, EditRecipe, LocalAdjustment, MaskGeometry, MaskRole, RangeMask,
};
use autoshade::segment::SegmentOpts;
use image::{DynamicImage, Rgb, RgbImage};

mod support;

struct ScopedDataDir {
    _tree: support::TempTree,
    _env: support::ScopedEnv,
}

impl ScopedDataDir {
    fn new() -> Self {
        let tree = support::TempTree::new("fit-workflow-store");
        let mut env = support::ScopedEnv::new();
        env.set("AUTOSHADE_DATA_DIR", tree.path());
        Self {
            _tree: tree,
            _env: env,
        }
    }
}

fn textured_frame(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        let wave = ((x * 7 + y * 11) % 53) as u8;
        let band = if (x / 4 + y / 4) % 2 == 0 { 28 } else { 0 };
        let r = 42u8.saturating_add(wave).saturating_add(band);
        let g = 32u8.saturating_add(wave / 2).saturating_add(band / 2);
        let b = 24u8.saturating_add(wave / 3);
        Rgb([r, g, b])
    }))
}

fn fit_fixture() -> DynamicImage {
    let (width, height) = (192, 128);
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        let light = x as f32 / (width - 1) as f32;
        let pixel = match y * 4 / height {
            0 => [light, light, light],
            1 => [light, light * 0.6, light * 0.2],
            2 => [light * 0.2, light * 0.7, light],
            _ => [light * 0.3, light, light * 0.4],
        };
        Rgb(pixel.map(|channel| (channel * 255.0).round() as u8))
    }))
}

fn hazy(source: &DynamicImage) -> DynamicImage {
    let recipe = EditRecipe {
        exposure_ev: -0.3,
        contrast: -45.0,
        blacks: 40.0,
        saturation: -40.0,
        blue_curve: vec![
            CurvePoint {
                input: 0,
                output: 25,
            },
            CurvePoint {
                input: 128,
                output: 132,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ],
        ..EditRecipe::default()
    };
    autoshade::render::develop_preview(source, &recipe)
}

fn canyon(warm_rocks: bool) -> DynamicImage {
    let (width, height) = (192, 128);
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        let light = 0.15 + 0.80 * x as f32 / (width - 1) as f32;
        let pixel = if y < 16 {
            [light, light, light]
        } else if y < 112 {
            let red = if warm_rocks {
                (0.85 * light + 0.18).min(1.0)
            } else {
                0.85 * light
            };
            [red, 0.52 * light, 0.30 * light]
        } else {
            [0.64, 0.68, 0.73]
        };
        Rgb(pixel.map(|channel| (channel * 255.0).round() as u8))
    }))
}

fn flat_sky_and_cloud_deck() -> (DynamicImage, DynamicImage) {
    let (width, height) = (384, 256);
    let frame = |clouds: bool| {
        DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
            let xf = x as f32 / (width - 1) as f32;
            let yf = y as f32 / (height - 1) as f32;
            let light = if y < height / 2 {
                if clouds {
                    let broad = (xf * 8.0 + yf * 5.0).sin();
                    let billow = (xf * 31.0 - yf * 17.0).sin();
                    let knots = if ((x / 18) + (y / 12)) % 2 == 0 {
                        -0.16
                    } else {
                        0.16
                    };
                    (0.55 + 0.28 * broad + 0.18 * billow + knots).clamp(0.04, 0.98)
                } else {
                    0.62 + 0.05 * xf - 0.03 * yf
                }
            } else {
                let ridge = if yf > 0.68 + 0.10 * (xf * 9.0).sin() {
                    0.22
                } else {
                    0.42
                };
                (ridge + 0.18 * xf + 0.03 * (xf * 43.0).sin()).clamp(0.02, 0.90)
            };
            let pixel = if y < height / 2 {
                [light * 0.83, light * 0.92, light]
            } else {
                [light, light * 0.78, light * 0.55]
            };
            Rgb(pixel.map(|channel| (channel * 255.0).round() as u8))
        }))
    };
    (frame(false), frame(true))
}

const BAND_DIRECTIONS: [[f32; 3]; 4] = [
    [0.701, -0.299, -0.299],
    [-0.587, 0.413, -0.587],
    [-0.114, -0.114, 0.886],
    [0.587, -0.413, 0.587],
];

fn colour_band_frame() -> DynamicImage {
    let (width, height) = (384, 256);
    DynamicImage::ImageRgb8(RgbImage::from_fn(width, height, |x, y| {
        let xf = x as f32 / (width - 1) as f32;
        let quarter = (y * 4 / height) as usize;
        let light = 0.26 + 0.26 * xf + 0.04 * (xf * 41.0).sin();
        let direction = BAND_DIRECTIONS[quarter];
        let pixel =
            direction.map(|channel| (light * (1.0 + 0.55 * channel)).clamp(0.0, 1.0));
        Rgb(pixel.map(|channel| (channel * 255.0).round() as u8))
    }))
}

fn hue_degrees(pixel: &Rgb<u8>) -> Option<f64> {
    let [r, g, b] = pixel.0.map(|channel| channel as f64 / 255.0);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    if delta < 0.03 {
        return None;
    }
    let hue = if max == r {
        60.0 * ((g - b) / delta).rem_euclid(6.0)
    } else if max == g {
        60.0 * ((b - r) / delta + 2.0)
    } else {
        60.0 * ((r - g) / delta + 4.0)
    };
    Some(hue.rem_euclid(360.0))
}

fn mean_hue(image: &RgbImage, rows: std::ops::Range<u32>) -> f64 {
    let mut sin = 0.0;
    let mut cos = 0.0;
    let mut count = 0;
    for y in rows {
        for x in 0..image.width() {
            if let Some(hue) = hue_degrees(image.get_pixel(x, y)) {
                let radians = hue.to_radians();
                sin += radians.sin();
                cos += radians.cos();
                count += 1;
            }
        }
    }
    assert!(count > 0, "the measured region must contain chromatic pixels");
    sin.atan2(cos).to_degrees().rem_euclid(360.0)
}

fn hue_distance(left: f64, right: f64) -> f64 {
    (left - right + 180.0).rem_euclid(360.0) - 180.0
}

fn relit(source: &DynamicImage, gains: [f32; 3]) -> DynamicImage {
    let source = source.to_rgb8();
    DynamicImage::ImageRgb8(RgbImage::from_fn(source.width(), source.height(), |x, y| {
        let p = source.get_pixel(x, y).0;
        Rgb(std::array::from_fn(|c| {
            (p[c] as f32 * gains[c]).round().clamp(0.0, 255.0) as u8
        }))
    }))
}

#[cfg(unix)]
fn fake_sky_segmenter(tree: &support::TempTree) -> (String, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let sidecar = tree.path().join("fake-sky-segment");
    let script = tree.path().join("fake-sky-segment.py");
    std::fs::write(
        &sidecar,
        b"#!/bin/sh\nset -eu\nout=\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    --output) out=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\ncp \"$AUTOSHADE_TEST_SKY_MASK\" \"$out\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&script, b"fake").unwrap();
    (sidecar.to_string_lossy().into_owned(), script)
}

#[cfg(unix)]
struct SkyZonedFixture {
    _tree: support::TempTree,
    _env: support::ScopedEnv,
    source: DynamicImage,
    target: DynamicImage,
    options: SegmentOpts,
    mask: autoshade::store::OwnedRaster,
}

#[cfg(unix)]
fn sky_zoned_fixture(tag: &str) -> SkyZonedFixture {
    let tree = support::TempTree::new(tag);
    let photo = tree.path().join("photo.png");
    let sky_file = tree.path().join("sky-mask.png");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let mut sky = image::GrayImage::new(64, 64);
    for (_, y, pixel) in sky.enumerate_pixels_mut() {
        *pixel = image::Luma([if y < 32 { 255 } else { 0 }]);
    }
    sky.save(&sky_file).unwrap();
    let (python_bin, script) = fake_sky_segmenter(&tree);
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_TEST_SKY_MASK", &sky_file);
    let mask = autoshade::store::OwnedRaster::claim(&photo, "mask-zone-sky").unwrap();
    let source = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |_, y| {
        if y < 32 {
            Rgb([20, 40, 120])
        } else {
            Rgb([60, 120, 40])
        }
    }));
    let target = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |_, y| {
        if y < 32 {
            Rgb([70, 130, 230])
        } else {
            Rgb([60, 120, 40])
        }
    }));
    let options = SegmentOpts {
        python_bin,
        script,
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };
    SkyZonedFixture {
        _tree: tree,
        _env: env,
        source,
        target,
        options,
        mask,
    }
}

#[cfg(unix)]
fn fake_region_segmenter(tree: &support::TempTree) -> (String, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let sidecar = tree.path().join("fake-region-segment");
    let script = tree.path().join("fake-region-segment.py");
    std::fs::write(
        &sidecar,
        b"#!/bin/sh\nset -eu\nout=\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    --output) out=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\ndir=$(dirname \"$out\")\nstem=$(basename \"$out\" .json)\ncp \"$AUTOSHADE_TEST_REGION_1\" \"$dir/$stem.class-1.png\"\ncp \"$AUTOSHADE_TEST_REGION_2\" \"$dir/$stem.class-2.png\"\nprintf '{\"version\":1,\"width\":32,\"height\":32,\"planes\":[{\"class_id\":1,\"label\":\"sky\",\"mean_confidence\":0.9,\"share\":0.5,\"path\":\"%s\"},{\"class_id\":2,\"label\":\"land\",\"mean_confidence\":0.9,\"share\":0.5,\"path\":\"%s\"}]}' \"$stem.class-1.png\" \"$stem.class-2.png\" > \"$out\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&script, b"fake").unwrap();
    (sidecar.to_string_lossy().into_owned(), script)
}

fn mean_abs_diff(a: &RgbImage, b: &RgbImage) -> f64 {
    a.pixels()
        .zip(b.pixels())
        .map(|(a, b)| {
            a.0.iter()
                .zip(b.0.iter())
                .map(|(&a, &b)| (a as f64 - b as f64).abs())
                .sum::<f64>()
        })
        .sum::<f64>()
        / (a.width() as f64 * a.height() as f64 * 3.0)
}

#[ignore = "slow fit workflow: run with cargo test --test fit_workflow -- --ignored"]
#[test]
fn a_textured_pair_fits_a_recipe_that_moves_the_delivered_preview_toward_the_target() {
    let source = textured_frame(4, 4);
    let target = relit(&source, [1.05, 1.0, 0.98]);

    let report = fit_recipe(&source, &target);
    let neutral = autoshade::render::develop_preview(&source, &EditRecipe::default()).to_rgb8();
    let fitted = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let target = target.to_rgb8();

    let before = mean_abs_diff(&neutral, &target);
    let after = mean_abs_diff(&fitted, &target);

    assert!(
        after < before * 0.95,
        "the delivered preview must move toward the target: before={before:.3} after={after:.3} recipe={:?}",
        report.recipe
    );
    assert!(
        report.err_after < report.err_before,
        "the fit's measured error must agree with the delivered result: {} -> {}",
        report.err_before,
        report.err_after
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn an_identity_pair_delivers_a_neutral_fit_without_a_terminal_reset() {
    let source = fit_fixture();

    let report = fit_recipe(&source, &source);
    let delivered = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let source = source.to_rgb8();

    assert!(
        mean_abs_diff(&delivered, &source) < 0.5,
        "an identity fit changed the delivered pixels: recipe={:?}",
        report.recipe
    );
    assert!(
        !report
            .recipe
            .rationale
            .contains("do-no-harm terminal case"),
        "the neutral result came from a terminal reset instead of a neutral solve: {}",
        report.recipe.rationale
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn fitting_an_engine_render_reproduces_its_delivered_look() {
    let source = fit_fixture();
    let target_recipe = EditRecipe {
        exposure_ev: 0.35,
        contrast: 18.0,
        highlights: -25.0,
        whites: 12.0,
        saturation: 15.0,
        ..EditRecipe::default()
    };
    let target = autoshade::render::develop_preview(&source, &target_recipe).to_rgb8();

    let report = fit_recipe(&source, &DynamicImage::ImageRgb8(target.clone()));
    let neutral =
        autoshade::render::develop_preview(&source, &EditRecipe::default()).to_rgb8();
    let delivered = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let before = mean_abs_diff(&neutral, &target);
    let after = mean_abs_diff(&delivered, &target);

    assert!(
        after < before * 0.5,
        "the fitted delivery did not reproduce the rendered look: before={before:.3} after={after:.3} recipe={:?}",
        report.recipe
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn fitting_a_hazy_render_removes_most_of_the_visible_gap() {
    let target = fit_fixture();
    let source = hazy(&target);

    let report = fit_recipe(&source, &target);
    let delivered = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let source = source.to_rgb8();
    let target = target.to_rgb8();
    let before = mean_abs_diff(&source, &target);
    let after = mean_abs_diff(&delivered, &target);

    assert!(
        after < before * 0.35,
        "the haze fit did not remove most of the visible gap: before={before:.3} after={after:.3} recipe={:?}",
        report.recipe
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn fitting_warm_rocks_does_not_turn_the_unchanged_sky_purple() {
    let source = canyon(false);
    let target = canyon(true);

    let report = fit_recipe(&source, &target);
    let delivered = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let target = target.to_rgb8();
    let delivered_sky = mean_hue(&delivered, 112..128);
    let target_sky = mean_hue(&target, 112..128);

    assert!(
        hue_distance(delivered_sky, target_sky).abs() < 30.0,
        "the unchanged sky shifted out of its target hue family: delivered={delivered_sky:.1}° target={target_sky:.1}° recipe={:?}",
        report.recipe
    );
    assert!(
        mean_abs_diff(&delivered, &target)
            <= mean_abs_diff(&source.to_rgb8(), &target) + 0.01,
        "protecting the sky made the delivered look worse overall"
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn fitting_an_illuminant_change_uses_white_balance_and_improves_the_delivery() {
    let source = fit_fixture();
    let target = relit(&source, [1.06, 1.0, 0.95]);

    let report = fit_recipe(&source, &target);
    let neutral =
        autoshade::render::develop_preview(&source, &EditRecipe::default()).to_rgb8();
    let delivered = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let target = target.to_rgb8();

    assert!(
        report.recipe.temperature_k.is_some_and(|kelvin| kelvin > 5500.0),
        "a warmer illuminant change was not represented as editable white balance: {:?}",
        report.recipe
    );
    assert!(
        mean_abs_diff(&delivered, &target) < mean_abs_diff(&neutral, &target),
        "the fitted white balance did not improve the delivered preview"
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn a_changed_sky_structure_is_disclosed_as_unrecoverable() {
    let (source, target) = flat_sky_and_cloud_deck();

    let report = fit_recipe(&source, &target);

    assert_eq!(report.mode, autoshade::fit::FitMode::Atmosphere);
    assert!(
        report
            .recipe
            .rationale
            .contains("structure cannot be reconstructed"),
        "the delivered recipe hid the structural limitation: {}",
        report.recipe.rationale
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn a_differently_shaped_reference_has_a_typed_disclosure() {
    let source = fit_fixture();
    let reference = fit_fixture().crop_imm(0, 0, 96, 128);

    let report = fit_recipe(&source, &reference);

    assert!(
        report
            .notes
            .iter()
            .any(|note| note.key == autoshade::rationale::keys::FIT_NOTE_NOT_SAME_FRAME),
        "the frame mismatch lost the typed disclosure used by localized views: {}",
        report.recipe.rationale
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn a_differently_shaped_reference_returns_a_bounded_fit() {
    let source = fit_fixture();
    let reference = fit_fixture().crop_imm(0, 0, 96, 128);

    let report = fit_recipe(&source, &reference);

    assert!(report.err_after.is_finite(), "the chosen reference was silently refused");
    assert!(
        report.recipe.confidence <= 0.5,
        "a mismatched frame claimed high confidence: {}",
        report.recipe.confidence
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn opposite_colour_family_changes_are_delivered_as_editable_band_controls() {
    let source = colour_band_frame();
    let mut target_recipe = EditRecipe::default();
    target_recipe.hsl.saturation[3] = -15.0;
    target_recipe.hsl.saturation[5] = 15.0;
    let target = autoshade::render::develop_preview(&source, &target_recipe).to_rgb8();

    let report = fit_recipe(&source, &DynamicImage::ImageRgb8(target.clone()));
    let delivered = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let neutral =
        autoshade::render::develop_preview(&source, &EditRecipe::default()).to_rgb8();

    assert!(
        report.recipe.hsl.saturation[3] <= -10.0
            && report.recipe.hsl.saturation[5] >= 10.0,
        "the opposing Green/Blue changes were not represented by their editable band controls: {:?}",
        report.recipe.hsl.saturation
    );
    assert!(
        mean_abs_diff(&delivered, &target) < mean_abs_diff(&neutral, &target),
        "the recovered band controls did not improve the delivered look"
    );
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn a_successful_fit_keeps_the_calibration_base_in_the_delivered_recipe() {
    let source = textured_frame(4, 4);
    let target = relit(&source, [1.05, 1.0, 0.98]);
    let base = EditRecipe {
        as_shot_k: Some(4800.0),
        base_curve: vec![[0.0, 0.05], [1.0, 0.95]],
        ..EditRecipe::default()
    };

    let report = autoshade::fit::fit_recipe_from(&source, &target, &base);

    assert_eq!(report.recipe.as_shot_k, base.as_shot_k);
    assert_eq!(report.recipe.base_curve, base.base_curve);
}

#[ignore = "slow fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn a_fit_from_a_calibration_base_does_not_worsen_the_delivered_preview() {
    let source = textured_frame(4, 4);
    let target = relit(&source, [1.05, 1.0, 0.98]);
    let base = EditRecipe {
        as_shot_k: Some(4800.0),
        base_curve: vec![[0.0, 0.05], [1.0, 0.95]],
        ..EditRecipe::default()
    };

    let report = autoshade::fit::fit_recipe_from(&source, &target, &base);
    let before = autoshade::render::develop_preview(&source, &base).to_rgb8();
    let after = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let target = target.to_rgb8();
    let before_error = mean_abs_diff(&before, &target);
    let after_error = mean_abs_diff(&after, &target);

    assert!(
        after_error <= before_error + 0.01,
        "the delivered preview must not worsen: before={before_error:.3} after={after_error:.3} recipe={:?}",
        report.recipe
    );
}

#[ignore = "slow zoned fit fallback: run with scripts/fit_battery.sh"]
#[test]
fn an_unavailable_segmentation_sidecar_delivers_a_global_fit_and_releases_its_claimed_mask() {
    let store = ScopedDataDir::new();
    let photo = store._tree.path().join("synthetic.png");
    std::fs::write(&photo, b"synthetic photo path").expect("synthetic photo is writable");
    let mask = autoshade::store::OwnedRaster::claim(&photo, "mask-zone-sky")
        .expect("the source mask name is claimable");
    let mask_path = mask.path().to_path_buf();
    assert!(mask_path.exists(), "the claim starts as an owned placeholder file");

    let source = textured_frame(4, 4);
    let target = relit(&source, [1.05, 1.0, 0.98]);
    let options = SegmentOpts {
        python_bin: "autoshade-test-python-that-does-not-exist".to_string(),
        script: store._tree.path().join("missing-segment.py"),
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let report = autoshade::fit_zoned::fit_recipe_zoned(&source, &target, &options, &mask);
    let neutral = autoshade::render::develop_preview(&source, &EditRecipe::default()).to_rgb8();
    let fitted = autoshade::render::develop_preview(&source, &report.recipe).to_rgb8();
    let target = target.to_rgb8();

    assert!(
        report
            .notes
            .iter()
            .any(|note| note.key == autoshade::rationale::keys::ZONED_UNAVAILABLE),
        "the user must be told why the zoned route was unavailable: {:?}",
        report.notes
    );
    assert!(
        mean_abs_diff(&fitted, &target) < mean_abs_diff(&neutral, &target),
        "the fallback must still deliver a usable global fit"
    );
    assert!(
        !mask_path.exists(),
        "a failed segmentation must release the claimed raster: {}",
        mask_path.display()
    );
    let mut fallback_recipe = report.recipe.clone();
    let mut fallback_rasters = Vec::new();
    for mask in &mut fallback_recipe.masks {
        fallback_rasters.extend(mask.bitmap_paths_mut().into_iter().map(|path| path.clone()));
    }
    assert!(
        !fallback_rasters
            .iter()
            .any(|path| std::path::Path::new(path) == mask_path),
        "the fallback recipe must not point at the released claim: {fallback_rasters:?}"
    );
}

#[cfg(unix)]
#[ignore = "slow zoned fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn a_successful_zoned_fit_attaches_a_sky_mask_to_the_delivered_recipe() {
    let fixture = sky_zoned_fixture("fit-zoned-success");
    let report = autoshade::fit_zoned::fit_recipe_zoned(
        &fixture.source,
        &fixture.target,
        &fixture.options,
        &fixture.mask,
    );

    assert!(
        report.recipe.masks.iter().any(|m| matches!(
            m.mask,
            MaskGeometry::AiMask { raster: Some(_), .. } | MaskGeometry::Bitmap { .. }
        )),
        "a successful sky segmentation should attach a raster-backed zone: {:?}",
        report.recipe.masks
    );
    assert!(fixture.mask.path().exists());
}

#[cfg(unix)]
#[ignore = "slow zoned fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn a_fitted_zone_survives_xmp_as_the_same_delivered_pixels() {
    let fixture = sky_zoned_fixture("fit-zoned-xmp-roundtrip");
    let report = autoshade::fit_zoned::fit_recipe_zoned(
        &fixture.source,
        &fixture.target,
        &fixture.options,
        &fixture.mask,
    );
    let fitted = report
        .recipe
        .masks
        .iter()
        .find(|mask| mask.role == MaskRole::ZoneSky)
        .expect("the successful zoned fit must attach a sky zone");
    let mut carrier = fitted.mask.clone();
    match &mut carrier {
        MaskGeometry::AiMask { inverted, .. } => *inverted = false,
        geometry => panic!("the fitted sky zone must use an AI mask: {geometry:?}"),
    }
    let zone = LocalAdjustment {
        mask: carrier,
        role: MaskRole::ZoneLand,
        inverted: true,
        exposure_ev: -0.75,
        ..LocalAdjustment::default()
    };
    let recipe = EditRecipe {
        masks: vec![zone],
        ..EditRecipe::default()
    };
    let expected = autoshade::render::develop_preview(&fixture.source, &recipe).to_rgb8();
    let plain =
        autoshade::render::develop_preview(&fixture.source, &EditRecipe::default()).to_rgb8();
    assert_ne!(expected, plain, "premise: the fitted zone changes its covered pixels");

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    assert!(
        xmp.contains("crs:What=\"Mask/Image\"")
            && xmp.contains("crs:MaskInverted=\"true\""),
        "the public XMP did not carry the inverted Select Sky spelling Lightroom reads"
    );
    let mut restored = autoshade::xmp::xmp_to_recipe(&xmp);
    let original_raster = match &recipe.masks[0].mask {
        MaskGeometry::AiMask {
            raster: Some(path), ..
        } => path.clone(),
        geometry => panic!("the fitted sky zone must use an AI mask: {geometry:?}"),
    };
    match &mut restored.masks[0].mask {
        MaskGeometry::AiMask { raster, .. } => *raster = Some(original_raster),
        geometry => panic!("the XMP changed the zone carrier: {geometry:?}"),
    }
    let delivered = autoshade::render::develop_preview(&fixture.source, &restored).to_rgb8();

    assert_eq!(delivered, expected);
}

#[cfg(unix)]
#[ignore = "slow zoned fit workflow: run with scripts/fit_battery.sh"]
#[test]
fn a_zoned_fit_discloses_supported_tiles_and_withholds_unsupported_range_bands() {
    let tree = support::TempTree::new("fit-zoned-inspect");
    let photo = tree.path().join("photo.png");
    let sky_file = tree.path().join("sky-mask.png");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let mut sky = image::GrayImage::new(64, 64);
    for (_, y, pixel) in sky.enumerate_pixels_mut() {
        *pixel = image::Luma([if y < 32 { 255 } else { 0 }]);
    }
    sky.save(&sky_file).unwrap();
    let (python_bin, script) = fake_sky_segmenter(&tree);
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_TEST_SKY_MASK", &sky_file);
    let mask = autoshade::store::OwnedRaster::claim(&photo, "mask-zone-sky").unwrap();
    let source = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |x, y| {
        if y < 32 { Rgb([20 + x as u8, 40, 120]) } else { Rgb([60, 120, 40]) }
    }));
    let target = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |x, y| {
        if y < 32 { Rgb([70 + x as u8, 130, 230]) } else { Rgb([60, 120, 40]) }
    }));
    let opts = SegmentOpts {
        python_bin,
        script,
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let report = autoshade::fit_zoned::fit_recipe_zoned(&source, &target, &opts, &mask);

    let withheld = report
        .notes
        .iter()
        .find(|note| note.key == autoshade::rationale::keys::FIT_NOTE_EVIDENCE_WITHHELD)
        .expect("the zoned fit must disclose why range bands were withheld");
    let arg = |name: &str| {
        withheld
            .args
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(value.as_str()))
    };
    assert_eq!(arg("luma_ranges"), Some("none"));
    assert!(arg("one_sided").is_some_and(|value| value.contains("Purple")));
    assert!(report
        .notes
        .iter()
        .any(|note| note.key == autoshade::rationale::keys::TILE_ELIGIBLE));
    assert!(report
        .notes
        .iter()
        .any(|note| note.key == autoshade::rationale::keys::TILE_ABSTAINED));
    assert!(report.notes.iter().any(|note| {
        matches!(
            note.key,
            autoshade::rationale::keys::TILE_BOUNDARY_PASSED
                | autoshade::rationale::keys::ZONE_BOUNDARY_PASSED
        ) && note
            .args
            .iter()
            .any(|(key, value)| *key == "transitions" && value.parse::<u32>().is_ok_and(|n| n > 0))
    }));
    assert!(!report.recipe.masks.iter().any(|mask| {
        matches!(mask.range, Some(RangeMask::Luminance { .. } | RangeMask::Color { .. }))
    }));
}

#[ignore = "slow zoned range fallback: run with scripts/fit_battery.sh"]
#[test]
fn a_zoned_fit_reports_unsupported_luminance_and_colour_bands_without_attaching_them() {
    let store = ScopedDataDir::new();
    let photo = store._tree.path().join("range-probe.png");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let mask = autoshade::store::OwnedRaster::claim(&photo, "mask-zone-sky").unwrap();
    let source = DynamicImage::ImageRgb8(RgbImage::from_fn(32, 32, |x, _| {
        if x < 16 { Rgb([30, 40, 50]) } else { Rgb([210, 200, 190]) }
    }));
    let target = DynamicImage::ImageRgb8(RgbImage::from_fn(32, 32, |x, _| {
        if x < 16 { Rgb([190, 180, 170]) } else { Rgb([50, 60, 70]) }
    }));
    let options = SegmentOpts {
        python_bin: "autoshade-test-python-that-does-not-exist".to_string(),
        script: store._tree.path().join("missing-segment.py"),
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let report = autoshade::fit_zoned::fit_recipe_zoned(&source, &target, &options, &mask);

    let luminance = report
        .notes
        .iter()
        .filter(|note| note.key == autoshade::rationale::keys::RANGE_ABSTAINED)
        .collect::<Vec<_>>();
    assert!(luminance.len() >= 2);
    assert!(luminance.iter().any(|note| note.args.iter().any(|(key, value)| {
        *key == "reason" && value.contains("below the 1.5% evidence floor")
    })));
    assert!(luminance.iter().any(|note| note.args.iter().any(|(key, value)| {
        *key == "reason" && value.contains("zero structural evidence")
    })));

    let colour = report
        .notes
        .iter()
        .filter(|note| note.key == autoshade::rationale::keys::COLOUR_RANGE_ABSTAINED)
        .collect::<Vec<_>>();
    assert!(colour.len() >= 3);
    for band in ["Orange", "Aqua", "Blue"] {
        assert!(colour.iter().any(|note| {
            note.args.iter().any(|(key, value)| *key == "band" && value == band)
                && note.args.iter().any(|(key, value)| {
                    *key == "reason" && value.contains("zero structural evidence")
                })
        }));
    }
    assert!(!report.recipe.masks.iter().any(|mask| {
        matches!(mask.range, Some(RangeMask::Luminance { .. } | RangeMask::Color { .. }))
    }));
}

#[cfg(unix)]
#[ignore = "slow zoned flat guard: run with scripts/fit_battery.sh"]
#[test]
fn a_flat_zoned_pair_offers_no_spatial_tile_or_boundary_charge() {
    let tree = support::TempTree::new("fit-zoned-flat");
    let photo = tree.path().join("photo.png");
    let sky_file = tree.path().join("sky-mask.png");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    image::GrayImage::from_pixel(32, 32, image::Luma([255])).save(&sky_file).unwrap();
    let (python_bin, script) = fake_sky_segmenter(&tree);
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_TEST_SKY_MASK", &sky_file);
    let mask = autoshade::store::OwnedRaster::claim(&photo, "mask-zone-sky").unwrap();
    let source = DynamicImage::ImageRgb8(RgbImage::from_pixel(32, 32, Rgb([96, 112, 128])));
    let target = DynamicImage::ImageRgb8(RgbImage::from_pixel(32, 32, Rgb([96, 112, 128])));
    let opts = SegmentOpts {
        python_bin,
        script,
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let report = autoshade::fit_zoned::fit_recipe_zoned(&source, &target, &opts, &mask);

    assert!(!report
        .notes
        .iter()
        .any(|note| note.key == autoshade::rationale::keys::TILE_ELIGIBLE));
    assert!(!report.notes.iter().any(|note| {
        matches!(
            note.key,
            autoshade::rationale::keys::TILE_BOUNDARY_PASSED
                | autoshade::rationale::keys::ZONE_BOUNDARY_PASSED
        )
    }));
}

#[cfg(unix)]
#[ignore = "slow semantic region boundary: run with scripts/fit_battery.sh"]
#[test]
fn a_feathered_semantic_region_boundary_is_measured_before_attachment() {
    let tree = support::TempTree::new("fit-zoned-transition-band");
    let photo = tree.path().join("photo.png");
    let region_1 = tree.path().join("region-1.png");
    let region_2 = tree.path().join("region-2.png");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let soft = |upper: bool| {
        image::GrayImage::from_fn(32, 32, |_, y| {
            let value = if upper { 255 - y as u8 * 8 } else { y as u8 * 8 };
            image::Luma([value])
        })
    };
    soft(true).save(&region_1).unwrap();
    soft(false).save(&region_2).unwrap();
    let (python_bin, script) = fake_region_segmenter(&tree);
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_TEST_REGION_1", &region_1);
    env.set("AUTOSHADE_TEST_REGION_2", &region_2);
    let mask = autoshade::store::OwnedRaster::claim(&photo, "mask-zone-sky").unwrap();
    let source = textured_frame(32, 32);
    let source_rgb = source.to_rgb8();
    let target = DynamicImage::ImageRgb8(RgbImage::from_fn(32, 32, |x, y| {
        let p = source_rgb.get_pixel(x, y).0;
        let gain = if y < 16 { [1.2, 1.1, 0.9] } else { [0.9, 1.0, 1.1] };
        Rgb(std::array::from_fn(|c| (p[c] as f32 * gain[c]).round().clamp(0.0, 255.0) as u8))
    }));
    let opts = SegmentOpts {
        python_bin,
        script,
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let report = autoshade::fit_zoned::fit_recipe_zoned_with_regions(
        &source,
        &target,
        &opts,
        &mask,
        &EditRecipe::default(),
        autoshade::fit::FitOptions { strength: autoshade::recipe::GradeStrength::default(), provider: None },
        3,
    );

    assert!(report.notes.iter().any(|note| {
        note.key == autoshade::rationale::keys::ZONE_BOUNDARY_PASSED
            && note.args.iter().any(|(key, value)| *key == "transitions" && value.parse::<u32>().is_ok_and(|n| n > 0))
    }));
}

#[ignore = "slow showcase colour range: run with scripts/fit_battery.sh"]
#[test]
fn a_zoned_fit_attaches_a_measured_colour_range_when_one_hue_band_improves() {
    let store = ScopedDataDir::new();
    let photo = store._tree.path().join("range-showcase.png");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let mask = autoshade::store::OwnedRaster::claim(&photo, "mask-zone-sky").unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/images/showcase-canyon-reverse-fit.jpg");
    let source = image::open(&fixture)
        .expect("checked-in showcase image opens")
        .resize_exact(96, 96, image::imageops::FilterType::Triangle)
        .to_rgb8();
    let target = RgbImage::from_fn(96, 96, |x, y| {
        let p = source.get_pixel(x, y).0;
        let luma = (0.299 * p[0] as f32 + 0.587 * p[1] as f32 + 0.114 * p[2] as f32) / 255.0;
        let gain = if (0.25..0.55).contains(&luma) { 0.55 } else { 1.0 };
        Rgb(std::array::from_fn(|c| (p[c] as f32 * gain).round().clamp(0.0, 255.0) as u8))
    });
    let options = SegmentOpts {
        python_bin: "autoshade-test-python-that-does-not-exist".to_string(),
        script: store._tree.path().join("missing-segment.py"),
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let report = autoshade::fit_zoned::fit_recipe_zoned(
        &DynamicImage::ImageRgb8(source),
        &DynamicImage::ImageRgb8(target),
        &options,
        &mask,
    );
    let range = report
        .recipe
        .masks
        .iter()
        .find_map(|mask| match mask.range {
            Some(RangeMask::Color { r, g, b, amount, px, py }) => Some((r, g, b, amount, px, py)),
            _ => None,
        })
        .expect("the improving hue band must ship as a colour range mask");
    assert!(range.0.is_finite() && (0.0..=1.0).contains(&range.0));
    assert!(range.1.is_finite() && (0.0..=1.0).contains(&range.1));
    assert!(range.2.is_finite() && (0.0..=1.0).contains(&range.2));
    assert!(range.3.is_finite() && (0.0..=1.0).contains(&range.3));
    assert!(range.4.is_finite() && (0.0..=1.0).contains(&range.4));
    assert!(range.5.is_finite() && (0.0..=1.0).contains(&range.5));
    let attached = report
        .notes
        .iter()
        .find(|note| note.key == autoshade::rationale::keys::COLOUR_RANGE_ATTACHED)
        .expect("the attached colour range must be disclosed");
    let value = |name: &str| {
        attached
            .args
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(value.as_str()))
            .and_then(|value| value.parse::<f32>().ok())
            .expect("the colour-range note carries numeric before/after")
    };
    assert!(value("after") < value("before"));
}

#[ignore = "slow showcase luminance range: run with scripts/fit_battery.sh"]
#[test]
fn a_zoned_fit_attaches_a_measured_luminance_range_when_a_paired_look_improves() {
    let store = ScopedDataDir::new();
    let photo = store._tree.path().join("showcase-pair.png");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let mask = autoshade::store::OwnedRaster::claim(&photo, "mask-zone-sky").unwrap();
    let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("docs/images/showcase-canyon-reverse-fit.jpg");
    let image = image::open(&fixture).expect("checked-in showcase image opens");
    let half = image.width() / 2;
    let source = image
        .crop_imm(0, 0, half, image.height())
        .resize_exact(96, 96, image::imageops::FilterType::Triangle);
    let target = image
        .crop_imm(half, 0, half, image.height())
        .resize_exact(96, 96, image::imageops::FilterType::Triangle);
    let options = SegmentOpts {
        python_bin: "autoshade-test-python-that-does-not-exist".to_string(),
        script: store._tree.path().join("missing-segment.py"),
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let report = autoshade::fit_zoned::fit_recipe_zoned(&source, &target, &options, &mask);
    let range = report
        .recipe
        .masks
        .iter()
        .find_map(|mask| match mask.range {
            Some(RangeMask::Luminance { lo_outer, lo, hi, hi_outer }) => {
                Some((lo_outer, lo, hi, hi_outer))
            }
            _ => None,
        })
        .expect("the improving luminance band must ship as a luminance range mask");
    assert!(range.0.is_finite() && (0.0..=1.0).contains(&range.0));
    assert!(range.1.is_finite() && (0.0..=1.0).contains(&range.1));
    assert!(range.2.is_finite() && (0.0..=1.0).contains(&range.2));
    assert!(range.3.is_finite() && (0.0..=1.0).contains(&range.3));
    assert!(range.0 <= range.1 && range.1 < range.2 && range.2 <= range.3);
    let attached = report
        .notes
        .iter()
        .find(|note| note.key == autoshade::rationale::keys::RANGE_ATTACHED)
        .expect("the attached luminance range must be disclosed");
    let value = |name: &str| {
        attached
            .args
            .iter()
            .find_map(|(key, value)| (*key == name).then_some(value.as_str()))
            .and_then(|value| value.parse::<f32>().ok())
            .expect("the luminance-range note carries numeric before/after")
    };
    assert!(value("after") < value("before"));
}
