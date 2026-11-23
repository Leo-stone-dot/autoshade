mod support;

use std::path::Path;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use autoshade::config::Config;
use autoshade::pipeline::{BatchNames, GradeRequest};
use autoshade::recipe::{
    Crop, CurvePoint, EditRecipe, LocalAdjustment, MaskGeometry, PointColor, RangeMask,
};
use image::{GenericImageView, RgbImage};
use rawler::decoders::Orientation;
use support::{ScopedEnv, TempTree};

fn test_radial(feather: f32) -> MaskGeometry {
    MaskGeometry::Radial {
        top: 0.25,
        left: 0.25,
        bottom: 0.75,
        right: 0.75,
        feather,
        roundness: 0.0,
        flipped: false,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    }
}

#[test]
fn out_of_range_develop_values_are_clamped_to_the_documented_ranges() {
    let mut recipe = EditRecipe {
        exposure_ev: 99.0,
        contrast: -500.0,
        ..EditRecipe::default()
    };

    recipe.clamp();

    assert_eq!(recipe.exposure_ev, 5.0);
    assert_eq!(recipe.contrast, -100.0);
}

#[test]
fn a_non_finite_white_balance_returns_to_as_shot() {
    let mut recipe = EditRecipe {
        temperature_k: Some(f32::INFINITY),
        ..EditRecipe::default()
    };

    recipe.clamp();

    assert_eq!(recipe.temperature_k, None);
}

#[test]
fn an_oversized_rationale_is_cut_and_the_loss_is_reported() {
    let mut recipe = EditRecipe {
        rationale: "x".repeat(600_000),
        ..EditRecipe::default()
    };

    let summary = recipe.clamp();

    assert!(recipe.rationale.len() <= 512 * 1024);
    assert!(summary.truncated_string_bytes > 0);
}

#[test]
fn an_unknown_recipe_field_is_refused_instead_of_silently_ignored() {
    let parsed = serde_json::from_str::<EditRecipe>(r#"{"not_a_develop_control":1}"#);

    assert!(parsed.is_err());
}

#[test]
fn a_round_trip_through_our_xmp_preserves_visible_develop_controls() {
    let recipe = EditRecipe {
        exposure_ev: 0.75,
        contrast: 23.0,
        temperature_k: Some(6500.0),
        tint: 12.0,
        saturation: -15.0,
        ..EditRecipe::default()
    };

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);

    assert!((restored.exposure_ev - recipe.exposure_ev).abs() < 0.011);
    assert_eq!(restored.contrast, recipe.contrast);
    assert_eq!(restored.temperature_k, recipe.temperature_k);
    assert_eq!(restored.tint, recipe.tint);
    assert_eq!(restored.saturation, recipe.saturation);
}

#[test]
fn positive_exposure_renders_a_mid_grey_preview_brighter() {
    let source = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        8,
        8,
        image::Rgb([128, 128, 128]),
    ));
    let bright = EditRecipe {
        exposure_ev: 1.0,
        ..EditRecipe::default()
    };

    let neutral = autoshade::render::develop_preview(&source, &EditRecipe::default()).to_rgb8();
    let developed = autoshade::render::develop_preview(&source, &bright).to_rgb8();
    let mean = |image: &image::RgbImage| {
        image
            .pixels()
            .map(|p| p.0.iter().map(|&v| v as u64).sum::<u64>())
            .sum::<u64>() as f64
            / (image.width() as f64 * image.height() as f64 * 3.0)
    };

    assert!(mean(&developed) > mean(&neutral));
}

#[test]
fn two_same_stem_photos_in_one_batch_get_distinct_deliverables() {
    let tree = TempTree::new("batch-names");
    let mut names = BatchNames::rooted(tree.path().to_path_buf());

    let first = names.claim(Path::new("camera-a/DSC0001.ARW"), "develop", "jpg");
    let second = names.claim(Path::new("camera-b/DSC0001.ARW"), "develop", "jpg");

    assert_ne!(first, second);
    assert_eq!(names.renamed.len(), 1);
}

#[test]
fn claiming_a_numbered_deliverable_does_not_replace_existing_bytes() {
    let tree = TempTree::new("numbered-delivery");
    let first = tree.path().join("photo.jpg");
    std::fs::write(&first, b"first delivery").unwrap();

    let second = autoshade::pipeline::claim_numbered(&first).unwrap();
    let third = autoshade::pipeline::claim_numbered(&first).unwrap();

    assert_eq!(second.file_name().unwrap(), "photo-2.jpg");
    assert_eq!(third.file_name().unwrap(), "photo-3.jpg");
    assert_eq!(std::fs::read(&first).unwrap(), b"first delivery");
}

#[test]
fn library_names_cannot_escape_their_store_file() {
    assert!(autoshade::library::valid_name("evening-walk"));
    assert!(!autoshade::library::valid_name("../evening-walk"));
    assert!(!autoshade::library::valid_name("evening/walk"));
}

#[test]
fn creating_a_recipe_library_entry_refuses_to_replace_an_existing_name() {
    let tree = TempTree::new("recipe-library-collision");
    let recipe = EditRecipe {
        exposure_ev: 1.0,
        ..EditRecipe::default()
    };
    autoshade::recipe_library::create_in(tree.path(), "warm", &recipe).unwrap();

    let result = autoshade::recipe_library::create_in(tree.path(), "warm", &EditRecipe::default());

    assert!(result.is_err());
    assert_eq!(
        autoshade::recipe_library::read_in(tree.path(), "warm")
            .unwrap()
            .exposure_ev,
        1.0
    );
}

#[test]
fn renaming_a_recipe_library_entry_moves_its_file_and_preserves_the_recipe() {
    let tree = TempTree::new("recipe-library-rename");
    let recipe = EditRecipe {
        contrast: 17.0,
        ..EditRecipe::default()
    };
    autoshade::recipe_library::create_in(tree.path(), "before", &recipe).unwrap();

    autoshade::recipe_library::rename_in(tree.path(), "before", "after").unwrap();

    assert!(!autoshade::recipe_library::entry_path_in(tree.path(), "before").exists());
    assert_eq!(
        autoshade::recipe_library::read_in(tree.path(), "after").unwrap(),
        recipe
    );
}

#[test]
fn saving_an_empty_reference_library_is_refused() {
    let tree = TempTree::new("empty-library");
    let file = tree.path().join("reference-library.json");
    let library = autoshade::library::Library {
        version: autoshade::library::VERSION,
        dir: "fixtures".to_string(),
        model: "synthetic".to_string(),
        entries: Vec::new(),
    };

    assert!(library.save(&file).is_err());
    assert!(!file.exists());
}

#[test]
fn a_reference_library_round_trips_through_its_file() {
    let tree = TempTree::new("library-round-trip");
    let file = tree.path().join("reference-library.json");
    let mut image = vec![0.0; autoshade::embed::EMBED_DIM];
    image[0] = 1.0;
    let library = autoshade::library::Library {
        version: autoshade::library::VERSION,
        dir: "fixtures".to_string(),
        model: "synthetic".to_string(),
        entries: vec![autoshade::library::Entry {
            id: "one".to_string(),
            path: "one.jpg".to_string(),
            stamp: autoshade::library::Stamp {
                len: 3,
                modified: 0,
            },
            image,
            phrases: Some("quiet evening".to_string()),
            settings: None,
            settings_checked: None,
        }],
    };

    library.save(&file).unwrap();

    assert_eq!(autoshade::library::Library::read(&file).unwrap(), library);
}

#[test]
fn a_job_plan_never_exceeds_the_memory_budget() {
    let plan = autoshade::jobs::plan_with_peak(8, 8, Some(2_000), 600);

    assert_eq!(plan.jobs, 1);
    assert!(plan.note.is_some());
}

#[test]
fn a_zero_job_request_still_runs_one_worker() {
    let plan = autoshade::jobs::plan_with_peak(0, 3, Some(10_000), 1);

    assert_eq!(plan.jobs, 1);
    assert!(plan.note.is_none());
}

#[test]
fn endpoints_ignore_trailing_slashes_and_surrounding_whitespace() {
    assert!(autoshade::config::same_endpoint(
        "  https://api.example.test/v1/  ",
        "https://api.example.test/v1",
    ));
    assert!(!autoshade::config::same_endpoint(
        "https://api.example.test/v1",
        "http://api.example.test/v1",
    ));
}

#[test]
fn same_named_photos_in_different_folders_get_distinct_store_keys() {
    let tree = TempTree::new("store-keys");
    let first = tree.path().join("one/DSC0001.ARW");
    let second = tree.path().join("two/DSC0001.ARW");
    std::fs::create_dir_all(first.parent().unwrap()).unwrap();
    std::fs::create_dir_all(second.parent().unwrap()).unwrap();
    std::fs::write(&first, b"first").unwrap();
    std::fs::write(&second, b"second").unwrap();

    let first_key = autoshade::store::photo_key(&first);
    let second_key = autoshade::store::photo_key(&second);
    let prefix = if cfg!(windows) {
        "dsc0001-"
    } else {
        "DSC0001-"
    };

    assert!(first_key.starts_with(prefix), "key={first_key}");
    assert!(second_key.starts_with(prefix), "key={second_key}");
    assert_ne!(first_key, second_key);
    assert_eq!(first_key, autoshade::store::photo_key(&first));
}

#[test]
fn a_synthetic_png_decodes_with_its_original_dimensions() {
    let tree = TempTree::new("decode-png");
    let path = tree.path().join("synthetic.png");
    image::RgbImage::from_pixel(7, 5, image::Rgb([20, 40, 60]))
        .save(&path)
        .unwrap();

    let decoded = autoshade::decode::load_image(&path).unwrap();

    assert_eq!(decoded.dimensions(), (7, 5));
}

#[test]
fn raw_extension_matching_is_case_insensitive_and_keeps_baked_files_out() {
    assert!(autoshade::decode::is_raw(Path::new("photo.ARW")));
    assert!(autoshade::decode::is_raw(Path::new("photo.DNG")));
    assert!(!autoshade::decode::is_raw(Path::new("photo.jpg")));
    assert!(!autoshade::decode::is_raw(Path::new("photo.png")));
}

#[test]
fn evidence_bins_clamp_luminance_and_ignore_grey_pixels_for_hue() {
    assert_eq!(autoshade::fit::evidence_luma_bin(-1.0), 0);
    assert_eq!(autoshade::fit::evidence_luma_bin(0.0), 0);
    assert!(autoshade::fit::evidence_luma_bin(1.0) > 0);
    assert!(autoshade::fit::evidence_luma_bin(f32::INFINITY) > 0);
    assert_eq!(autoshade::fit::evidence_hue_band(&[0.5, 0.5, 0.5]), None);
    assert!(autoshade::fit::evidence_hue_band(&[1.0, 0.0, 0.0]).is_some());
    assert_eq!(
        autoshade::fit::evidence_hue_band(&[1.0, 0.0, 0.0]),
        autoshade::fit::evidence_hue_band(&[0.5, 0.0, 0.0])
    );
}

#[test]
fn a_structured_pair_spans_multiple_luma_ranges_while_a_flat_pair_stays_in_one() {
    let (w, h) = (8, 8);
    let mut structured = Vec::with_capacity(w * h);
    for y in 0..h {
        for x in 0..w {
            let v = (x as f32 + y as f32) / ((w + h - 2) as f32);
            structured.push([v, v, v]);
        }
    }
    let flat = vec![[0.5, 0.5, 0.5]; w * h];

    let structured_model =
        autoshade::fit::evidence_model_paired(&structured, &structured, None, w as u32, h as u32);
    let flat_model =
        autoshade::fit::evidence_model_paired(&flat, &flat, None, w as u32, h as u32);

    let populated = |model: &autoshade::fit::EvidenceModel| {
        model.luma.iter().filter(|range| range.weight > 0.0).count()
    };

    assert!(populated(&structured_model) > 1);
    assert_eq!(populated(&flat_model), 1);
}

#[test]
fn evidence_model_with_no_present_target_pixels_has_no_population() {
    let pixels = (0..64)
        .map(|i| {
            let v = i as f32 / 63.0;
            [v, v * 0.6, v * 0.3]
        })
        .collect::<Vec<_>>();
    let absent = vec![0.0; pixels.len()];

    let model = autoshade::fit::evidence_model_paired(&pixels, &pixels, Some(&absent), 8, 8);

    assert_eq!(model.population, 0.0);
    assert!(model.source_weights.iter().all(|weight| *weight == 0.0));
    assert!(model.target_weights.iter().all(|weight| *weight == 0.0));
}

#[test]
fn a_missing_segmentation_sidecar_is_reported_before_it_creates_a_mask() {
    let tree = TempTree::new("segment-missing-sidecar");
    let input = tree.path().join("input.png");
    let output = tree.path().join("mask.png");
    std::fs::write(&input, b"synthetic input").unwrap();
    let opts = autoshade::segment::SegmentOpts {
        python_bin: "autoshade-test-python".to_string(),
        script: tree.path().join("missing-segment.py"),
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let error = autoshade::segment::segment_file(&opts, &input, &output).unwrap_err();

    assert!(error.to_string().contains("segmentation sidecar not found"));
    assert!(!output.exists());
}

#[cfg(unix)]
#[test]
fn a_segmentation_sidecar_that_exits_zero_without_output_is_a_failure() {
    let tree = TempTree::new("segment-exit-zero-no-output");
    let input = tree.path().join("input.png");
    let output = tree.path().join("mask.png");
    let sidecar = tree.path().join("fake-segment");
    std::fs::write(&input, b"synthetic input").unwrap();
    std::fs::write(&sidecar, b"#!/bin/sh\nexit 0\n").unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    let opts = autoshade::segment::SegmentOpts {
        python_bin: sidecar.to_string_lossy().into_owned(),
        script: sidecar,
        target: "sky".to_string(),
        reference_point: None,
        prompt_points: None,
    };

    let error = autoshade::segment::segment_file(&opts, &input, &output).unwrap_err();

    assert!(error.to_string().contains("exited 0 but wrote no output"));
    assert!(!output.exists());
}

#[test]
fn produce_recipe_without_a_harness_key_refuses_before_decoding() {
    let tree = TempTree::new("produce-recipe-no-key");
    let raw = tree.path().join("not-a-raw.dng");
    std::fs::write(&raw, b"not a real raw").unwrap();
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("DEEPSEEK_API_KEY", "");
    let config = Config::load();

    let error = match autoshade::pipeline::produce_recipe(
        &raw,
        None,
        &config,
        false,
        None,
        None,
        GradeRequest::default(),
        false,
        autoshade::diag::silent(),
    ) {
        Ok(_) => panic!("a missing harness key must refuse before decoding"),
        Err(error) => error,
    };

    assert!(error.to_string().contains("no key for the pi harness"));
    assert_eq!(std::fs::read(&raw).unwrap(), b"not a real raw");
}

#[cfg(unix)]
#[test]
fn a_fresh_analysis_returns_the_deferred_harness_recipe_and_verdict() {
    let tree = TempTree::new("produce-recipe-deferred");
    let photo = tree.path().join("photo.png");
    RgbImage::from_pixel(16, 16, image::Rgb([96, 120, 144])).save(&photo).unwrap();
    let expected = EditRecipe {
        exposure_ev: 0.75,
        contrast: 18.0,
        rationale: "synthetic harness result".to_string(),
        ..EditRecipe::default()
    };
    let result_fixture = tree.path().join("result.json");
    let summary_fixture = tree.path().join("summary.json");
    std::fs::write(
        &result_fixture,
        serde_json::json!({
            "recipe": expected,
            "verdict": "accept",
            "score": 91,
            "session": "synthetic-session"
        })
        .to_string(),
    )
    .unwrap();
    std::fs::write(&summary_fixture, r#"{"verdict":"accept","score":91}"#).unwrap();

    let fake_node = tree.path().join("fake-node");
    let harness = tree.path().join("analyze.mjs");
    let cli = tree.path().join("autoshade");
    std::fs::write(
        &fake_node,
        b"#!/bin/sh\nset -eu\ncp \"$AUTOSHADE_TEST_RESULT_FIXTURE\" \"$AUTOSHADE_HARNESS_RESULT\"\ncp \"$AUTOSHADE_TEST_SUMMARY_FIXTURE\" \"$AUTOSHADE_SESSION_SUMMARY\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&fake_node, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&harness, b"synthetic harness").unwrap();
    std::fs::write(&cli, b"synthetic cli").unwrap();

    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_PIPELINE", "harness");
    env.set("AUTOSHADE_NODE", &fake_node);
    env.set("AUTOSHADE_PI_HARNESS", &harness);
    env.set("AUTOSHADE_MCP_EXE", &cli);
    env.set("AUTOSHADE_TEST_RESULT_FIXTURE", &result_fixture);
    env.set("AUTOSHADE_TEST_SUMMARY_FIXTURE", &summary_fixture);
    env.set("DEEPSEEK_API_KEY", "synthetic-key");
    let config = Config::load();

    let (recipe, verdict, notes) = autoshade::pipeline::produce_recipe(
        &photo,
        None,
        &config,
        false,
        None,
        None,
        GradeRequest::default(),
        false,
        autoshade::diag::silent(),
    )
    .unwrap();

    assert_eq!(recipe, expected);
    assert_eq!(verdict.decision, autoshade::advisor::Decision::Accept);
    assert_eq!(verdict.reasons, ["critic score 91/100"]);
    assert!(notes.is_empty());
}

#[test]
fn a_baked_image_with_legacy_coordinates_is_stamped_without_reframing() {
    let tree = TempTree::new("baked-coord-era");
    let raw = tree.path().join("baked.png");
    std::fs::write(&raw, b"synthetic baked image").unwrap();
    let mut recipe = EditRecipe {
        coord_era: 0,
        ..EditRecipe::default()
    };

    let result = autoshade::pipeline::migrate_recipe_coord_frame(&raw, &mut recipe).unwrap();

    assert!(result.is_none());
    assert_eq!(recipe.coord_era, autoshade::recipe::COORD_ERA);
}

#[test]
fn an_old_raw_recipe_migrates_its_crop_once_for_the_exif_turn() {
    let tree = TempTree::new("raw-coord-era");
    let raw = tree.path().join("portrait.dng");
    std::fs::write(
        &raw,
        support::synthetic_dng(16, 8, Orientation::Rotate90, None, false),
    )
    .unwrap();
    let mut recipe = EditRecipe {
        version: autoshade::recipe::CALIB_ERA,
        coord_era: 0,
        crop: Some(Crop {
            left: 0.1,
            top: 0.2,
            right: 0.6,
            bottom: 0.8,
        }),
        ..EditRecipe::default()
    };

    let migration = autoshade::pipeline::migrate_loaded_recipe(&raw, &mut recipe);

    let reframe = migration.reframe.expect("the rotated RAW needs a coordinate migration");
    assert_eq!(reframe.orientation, Orientation::Rotate90);
    let crop = recipe.crop.unwrap();
    assert!((crop.left - 0.2).abs() < 1e-6);
    assert!((crop.top - 0.1).abs() < 1e-6);
    assert!((crop.right - 0.8).abs() < 1e-6);
    assert!((crop.bottom - 0.6).abs() < 1e-6);
    assert_eq!(recipe.coord_era, autoshade::recipe::COORD_ERA);

    let once = recipe.clone();
    let second = autoshade::pipeline::migrate_loaded_recipe(&raw, &mut recipe);
    assert!(second.reframe.is_none());
    assert_eq!(recipe, once);
}

#[test]
fn decoding_a_baked_image_with_a_quarter_turn_swaps_pixels_and_meta_dimensions() {
    let tree = TempTree::new("baked-quarter-turn");
    let path = tree.path().join("baked.png");
    RgbImage::from_pixel(4, 2, image::Rgb([20, 40, 60])).save(&path).unwrap();

    let decoded = autoshade::decode::decode_any_turned(&path, 1).unwrap();

    assert_eq!(decoded.preview.dimensions(), (2, 4));
    assert_eq!((decoded.meta.width, decoded.meta.height), (2, 4));
}

#[test]
fn resolving_an_ai_mask_with_a_missing_sidecar_reports_unresolved_and_keeps_no_raster() {
    let tree = TempTree::new("resolve-ai-missing-sidecar");
    let raw = tree.path().join("photo.dng");
    let src = tree.path().join("photo.png");
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_SEGMENT_SCRIPT", tree.path().join("missing-segment.py"));
    let config = Config::load();
    let mut recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::AiMask {
                name: "sky".to_string(),
                subtype: 2,
                ref_x: 0.5,
                ref_y: 0.5,
                blend_mode: 0,
                value: 1.0,
                inverted: false,
                mask_version: 1,
                provenance: Vec::new(),
                gesture: Vec::new(),
                raster: None,
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let resolution = autoshade::segment::resolve_ai_masks(&config, &raw, &src, &mut recipe);

    assert_eq!(resolution.unresolved.len(), 1);
    assert!(resolution.unresolved[0].1.contains("not at"));
    assert!(matches!(
        recipe.masks[0].mask,
        MaskGeometry::AiMask { raster: None, .. }
    ));
}

#[cfg(unix)]
#[test]
fn resolving_an_ai_mask_reuses_a_decodable_cached_alpha_without_the_sidecar() {
    let tree = TempTree::new("resolve-ai-cache-hit");
    let raw = tree.path().join("photo.dng");
    let src = tree.path().join("photo.png");
    let mask = tree.path().join("mask.png");
    RgbImage::from_pixel(8, 8, image::Rgb([40, 80, 120])).save(&src).unwrap();
    image::GrayImage::from_pixel(8, 8, image::Luma([255])).save(&mask).unwrap();
    let fake_sidecar = tree.path().join("fake-segment");
    let fake_script = tree.path().join("fake-segment.py");
    std::fs::write(
        &fake_sidecar,
        "#!/bin/sh\nset -eu\nout=\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    --output) out=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\ncp \"$AUTOSHADE_FAKE_MASK\" \"$out\"\n",
    )
    .unwrap();
    std::fs::write(&fake_script, b"fake").unwrap();
    std::fs::set_permissions(&fake_sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_FAKE_MASK", &mask);
    let mut config = Config::load();
    config.python_bin = fake_sidecar.to_string_lossy().into_owned();
    config.segment_script = fake_script.to_string_lossy().into_owned();
    let mut recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::AiMask {
                name: "sky".to_string(),
                subtype: 2,
                ref_x: 0.5,
                ref_y: 0.5,
                blend_mode: 0,
                value: 1.0,
                inverted: false,
                mask_version: 1,
                provenance: Vec::new(),
                gesture: Vec::new(),
                raster: None,
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    let first = autoshade::segment::resolve_ai_masks(&config, &raw, &src, &mut recipe);
    assert_eq!(first.resolved, 1);
    assert_eq!(first.cached, 0);
    assert!(first.unresolved.is_empty());
    assert!(matches!(
        recipe.masks[0].mask,
        MaskGeometry::AiMask { raster: Some(_), .. }
    ));

    std::fs::remove_file(&fake_sidecar).unwrap();
    std::fs::remove_file(&fake_script).unwrap();
    let second = autoshade::segment::resolve_ai_masks(&config, &raw, &src, &mut recipe);

    assert_eq!(second.resolved, 1);
    assert_eq!(second.cached, 1);
    assert!(second.unresolved.is_empty());
}

#[cfg(unix)]
#[test]
fn a_fallback_alpha_is_renewed_when_the_primary_segmenter_becomes_available() {
    let tree = TempTree::new("resolve-ai-renew");
    let raw = tree.path().join("photo.dng");
    let src = tree.path().join("photo.png");
    let first_mask = tree.path().join("first-mask.png");
    let second_mask = tree.path().join("second-mask.png");
    RgbImage::from_pixel(8, 8, image::Rgb([40, 80, 120])).save(&src).unwrap();
    image::GrayImage::from_pixel(8, 8, image::Luma([255])).save(&first_mask).unwrap();
    image::GrayImage::from_pixel(8, 8, image::Luma([64])).save(&second_mask).unwrap();
    let fake_sidecar = tree.path().join("fake-segment");
    let fake_script = tree.path().join("fake-segment.py");
    std::fs::write(
        &fake_sidecar,
        "#!/bin/sh\nset -eu\nprobe=0\nout=\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    --probe-backend) probe=1; shift;;\n    --output) out=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\nif [ \"$probe\" -eq 1 ]; then\n  printf '%s\\n' 'segment.py: subject backend deps [ok]'\n  exit 0\nfi\ncp \"$AUTOSHADE_FAKE_MASK\" \"$out\"\nprintf '%s\\n' 'segment.py: subject mask [BiRefNet synthetic]'\nprintf '%s\\n' 'segment.py: subject backend deps [ok]'\n",
    )
    .unwrap();
    std::fs::write(&fake_script, b"fake").unwrap();
    std::fs::set_permissions(&fake_sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_FAKE_MASK", &first_mask);
    let mut config = Config::load();
    config.python_bin = fake_sidecar.to_string_lossy().into_owned();
    config.segment_script = fake_script.to_string_lossy().into_owned();
    let mut recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::AiMask {
                name: "subject".to_string(),
                subtype: 0,
                ref_x: 0.5,
                ref_y: 0.5,
                blend_mode: 0,
                value: 1.0,
                inverted: false,
                mask_version: 1,
                provenance: Vec::new(),
                gesture: Vec::new(),
                raster: None,
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    let first = autoshade::segment::resolve_ai_masks(&config, &raw, &src, &mut recipe);
    assert_eq!(first.resolved, 1);
    let alpha = match &recipe.masks[0].mask {
        MaskGeometry::AiMask { raster: Some(path), .. } => std::path::PathBuf::from(path),
        other => panic!("the first resolve must install an alpha: {other:?}"),
    };
    let marker = alpha.with_extension("backend");
    std::fs::write(
        &marker,
        b"autoshade-ai-mask-backend v1\nbackend=U^2-Net (FALLBACK - BiRefNet did not run)\nbirefnet-deps=missing\n",
    )
    .unwrap();
    let old_alpha = std::fs::read(&alpha).unwrap();
    env.set("AUTOSHADE_FAKE_MASK", &second_mask);

    let second = autoshade::segment::resolve_ai_masks(&config, &raw, &src, &mut recipe);

    assert_eq!(second.resolved, 1);
    assert_eq!(second.renewed, 1);
    assert_eq!(second.cached, 0);
    assert_ne!(std::fs::read(&alpha).unwrap(), old_alpha);
}

#[test]
fn a_staged_export_publish_replaces_the_delivery_once_written() {
    let tree = TempTree::new("stage-publish-success");
    let out = tree.path().join("delivery.bin");
    std::fs::write(&out, b"old bytes").unwrap();

    autoshade::render::stage_and_publish(&out, |staged| {
        std::fs::write(staged, b"new bytes").map_err(Into::into)
    })
    .unwrap();

    assert_eq!(std::fs::read(&out).unwrap(), b"new bytes");
    assert_eq!(std::fs::read_dir(tree.path()).unwrap().count(), 1);
}

#[test]
fn a_failed_staged_export_publish_leaves_the_existing_delivery_untouched() {
    let tree = TempTree::new("stage-publish-failure");
    let out = tree.path().join("delivery.bin");
    std::fs::write(&out, b"old bytes").unwrap();

    let error = autoshade::render::stage_and_publish(&out, |staged| {
        std::fs::write(staged, b"partial bytes")?;
        Err(anyhow::anyhow!("synthetic encode failure"))
    })
    .unwrap_err();

    assert!(error.to_string().contains("synthetic encode failure"));
    assert_eq!(std::fs::read(&out).unwrap(), b"old bytes");
    assert_eq!(std::fs::read_dir(tree.path()).unwrap().count(), 1);
}

#[test]
fn same_frame_plausibility_rejects_a_cropped_aspect() {
    assert!(autoshade::fit::same_frame_plausible_dims((3000, 2000), (3000, 2000)));
    assert!(!autoshade::fit::same_frame_plausible_dims((3000, 2000), (1600, 900)));
}

#[test]
fn structure_divergence_abstains_when_the_eroded_core_is_too_small() {
    let (w, h) = (8, 8);
    let pixels = vec![[0.5, 0.5, 0.5]; w * h];
    let weights = vec![1.0; w * h];

    assert!(autoshade::fit::structure_divergence(
        &pixels,
        &pixels,
        w as u32,
        h as u32,
        &weights,
    )
    .is_none());
}

#[test]
fn writing_an_xmp_projection_publishes_a_readable_sidecar() {
    let tree = TempTree::new("write-xmp-projection");
    let out = tree.path().join("projection.xmp");
    let recipe = EditRecipe {
        exposure_ev: 0.75,
        ..EditRecipe::default()
    };

    let (path, _note, _losses) = autoshade::pipeline::write_xmp_at(
        out.clone(),
        &recipe,
        &autoshade::diag::dropped(),
    )
    .unwrap();

    assert_eq!(path, out);
    let text = std::fs::read_to_string(&out).unwrap();
    let restored = autoshade::xmp::xmp_to_recipe(&text);
    assert!((restored.exposure_ev - recipe.exposure_ev).abs() < 0.011);
}

#[test]
fn an_unparsable_xmp_number_is_reported() {
    let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="not-a-number"/></rdf:RDF></x:xmpmeta>"#;

    assert!(
        autoshade::xmp::unparsable_crs_numbers(xmp)
            .iter()
            .any(|key| key == "Exposure2012")
    );
}

#[test]
fn the_sha256_known_answer_matches_the_standard_vector() {
    assert_eq!(
        autoshade::sha256::sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
    );
}

#[test]
fn the_sha256_empty_input_matches_the_standard_vector() {
    assert_eq!(
        autoshade::sha256::sha256_hex(b""),
        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
    );
}

#[test]
fn the_sha256_multi_block_input_matches_the_standard_vector() {
    assert_eq!(
        autoshade::sha256::sha256_hex(
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
        ),
        "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    );
}

#[test]
fn the_sha256_million_byte_input_matches_the_standard_vector() {
    assert_eq!(
        autoshade::sha256::sha256_hex(&vec![b'a'; 1_000_000]),
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0",
    );
}

#[test]
fn deleting_a_recipe_library_entry_removes_only_the_named_entry() {
    let tree = TempTree::new("recipe-library-delete");
    autoshade::recipe_library::create_in(tree.path(), "keep", &EditRecipe::default()).unwrap();
    autoshade::recipe_library::create_in(tree.path(), "remove", &EditRecipe::default()).unwrap();

    autoshade::recipe_library::delete_in(tree.path(), "remove").unwrap();

    assert!(autoshade::recipe_library::entry_path_in(tree.path(), "keep").is_file());
    assert!(!autoshade::recipe_library::entry_path_in(tree.path(), "remove").exists());
}

#[test]
fn a_recipe_library_name_with_a_path_separator_is_refused() {
    assert!(matches!(
        autoshade::recipe_library::check_name("../escape"),
        Err(autoshade::recipe_library::NameError::Illegal('/')),
    ));
}

#[test]
fn a_store_read_past_its_byte_cap_is_refused() {
    let tree = TempTree::new("store-cap");
    let file = tree.path().join("oversized.json");
    std::fs::write(&file, b"12345678").unwrap();
    assert_eq!(
        autoshade::store::read_text_capped(&file, 8).unwrap(),
        "12345678"
    );

    let error = autoshade::store::read_text_capped(&file, 7).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
    assert!(error.to_string().contains("limit"), "error={error}");
    assert_eq!(
        autoshade::store::read_text_capped(&tree.path().join("missing.json"), 8)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::NotFound
    );
    std::fs::write(&file, [0xff, 0xfe]).unwrap();
    assert_eq!(
        autoshade::store::read_text_capped(&file, 8)
            .unwrap_err()
            .kind(),
        std::io::ErrorKind::InvalidData
    );
}

#[test]
fn an_empty_tone_curve_lut_is_the_identity() {
    let lut = autoshade::render::curve_lut(&[]);

    assert_eq!(lut.len(), 256);
    assert_eq!(lut[0], 0.0);
    assert!((lut[255] - 1.0).abs() < 1e-6);
    assert!(lut.windows(2).all(|pair| pair[0] <= pair[1]));
}

#[test]
fn a_sixteen_bit_stack_round_trip_preserves_pixels() {
    let pixels = vec![[0.0, 0.5, 1.0], [0.25, 0.75, 0.0]];
    let image = autoshade::stack::image16(&pixels, 2, 1).unwrap();

    let restored = autoshade::stack::pixels16(&image);

    for (want, got) in pixels.iter().flatten().zip(restored.iter().flatten()) {
        assert!((want - got).abs() <= 1.0 / 65_535.0);
    }
}

#[test]
fn keep_skin_natural_clamps_each_skin_control_to_its_limit() {
    let mut mask = autoshade::recipe::LocalAdjustment {
        role: autoshade::recipe::MaskRole::Skin,
        saturation: 50.0,
        hue: 100.0,
        temperature: 50.0,
        texture: -100.0,
        clarity: -100.0,
        sharpness: 50.0,
        ..Default::default()
    };

    let moved = autoshade::face::keep_skin_natural(&mut mask);

    assert!(moved);
    assert_eq!(mask.saturation, autoshade::face::SKIN_SATURATION_MAX);
    assert_eq!(mask.hue, autoshade::face::skin_hue_max());
    assert_eq!(mask.temperature, autoshade::face::SKIN_TEMPERATURE_MAX);
    assert_eq!(mask.texture, autoshade::face::SKIN_TEXTURE_MIN);
    assert_eq!(mask.clarity, autoshade::face::SKIN_CLARITY_MIN);
    assert_eq!(mask.sharpness, autoshade::face::SKIN_SHARPNESS_MAX);
}

#[test]
fn quarter_turns_compose_and_invert_across_every_exif_orientation() {
    let orientations = [
        Orientation::Normal,
        Orientation::HorizontalFlip,
        Orientation::Rotate180,
        Orientation::VerticalFlip,
        Orientation::Transpose,
        Orientation::Rotate90,
        Orientation::Transverse,
        Orientation::Rotate270,
    ];

    for exif in orientations {
        for turns in 0..4 {
            let composed = autoshade::render::compose_orientation(exif, turns);
            assert_eq!(
                autoshade::render::quarter_turns_between(exif, composed),
                Some(turns)
            );
        }
    }
}

#[test]
fn a_radial_mask_covers_its_centre_and_not_the_far_corner() {
    let mask = LocalAdjustment {
        mask: test_radial(0.0),
        ..Default::default()
    };
    let reference = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        20,
        20,
        image::Rgb([128, 128, 128]),
    ));

    let coverage = autoshade::render::mask_coverage(
        &mask,
        &reference,
        autoshade::render::MaskFrame::AsRendered,
    );

    assert_eq!(coverage.get_pixel(10, 10)[0], 255);
    assert_eq!(coverage.get_pixel(0, 0)[0], 0);
}

#[test]
fn a_disabled_mask_has_zero_coverage() {
    let mask = LocalAdjustment {
        mask: test_radial(0.0),
        enabled: false,
        ..Default::default()
    };
    let reference = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        20,
        20,
        image::Rgb([128, 128, 128]),
    ));

    let coverage = autoshade::render::mask_coverage(
        &mask,
        &reference,
        autoshade::render::MaskFrame::AsRendered,
    );

    assert!(coverage.pixels().all(|pixel| pixel[0] == 0));
}

#[test]
fn a_luminance_range_keeps_its_upper_bound_inclusive() {
    let range = RangeMask::Luminance {
        lo_outer: 0.0,
        lo: 0.2,
        hi: 0.8,
        hi_outer: 0.8,
    };

    assert_eq!(
        autoshade::render::range_weight(&range, &[0.8, 0.8, 0.8]),
        1.0
    );
}

#[test]
fn a_colour_range_ignores_brightness_but_rejects_a_different_hue() {
    let range = RangeMask::Color {
        r: 1.0,
        g: 0.0,
        b: 0.0,
        amount: 0.5,
        px: 0.5,
        py: 0.5,
    };

    assert_eq!(
        autoshade::render::range_weight(&range, &[0.4, 0.0, 0.0]),
        1.0
    );
    assert_eq!(
        autoshade::render::range_weight(&range, &[0.0, 0.0, 1.0]),
        0.0
    );
}

#[test]
fn a_float_thumbnail_does_not_gain_half_a_code() {
    let source = image::DynamicImage::ImageRgb32F(
        image::ImageBuffer::from_raw(2, 2, vec![0.5; 12]).unwrap(),
    );

    let thumb = autoshade::render::thumbnail_exact(&source, 1, 1).to_rgb32f();

    assert!((thumb.get_pixel(0, 0)[0] - 0.5).abs() < 1e-6);
}

#[test]
fn manual_distortion_and_its_inverse_return_the_same_point() {
    let point = (0.3, 0.7);
    let distorted = autoshade::render::distort_norm(point.0, point.1, (1_000.0, 800.0), 50.0);
    let restored =
        autoshade::render::undistort_norm(distorted.0, distorted.1, (1_000.0, 800.0), 50.0);

    assert!((restored.0 - point.0).abs() < 1e-4);
    assert!((restored.1 - point.1).abs() < 1e-4);
}

#[test]
fn stack_kind_store_spellings_round_trip() {
    for kind in autoshade::stack::merge::StackKind::ALL {
        assert_eq!(
            autoshade::stack::merge::StackKind::from_store_str(kind.store_str()),
            Some(kind)
        );
    }
}

#[test]
fn a_stack_with_one_frame_is_refused() {
    let frame = vec![[0.5, 0.5, 0.5]; 4];
    let error = autoshade::stack::merge::merge(
        &[frame],
        2,
        2,
        &autoshade::stack::merge::MergeOptions::new(autoshade::stack::merge::StackKind::Noise),
    )
    .unwrap_err();

    assert!(error.to_string().contains("at least two frames"));
}

#[test]
fn a_registered_noise_stack_averages_corresponding_pixels() {
    let first = vec![[0.2, 0.4, 0.6]; 4];
    let second = vec![[0.6, 0.8, 0.2]; 4];
    let merged = autoshade::stack::merge::merge(
        &[first, second],
        2,
        2,
        &autoshade::stack::merge::MergeOptions {
            kind: autoshade::stack::merge::StackKind::Noise,
            align: None,
        },
    )
    .unwrap();

    assert!(
        merged
            .pixels
            .iter()
            .all(|pixel| pixel[0] > 0.25 && pixel[0] < 0.55)
    );
}

#[test]
fn xmp_round_trip_preserves_a_radial_masks_geometry_and_inversion() {
    let mut recipe = EditRecipe::default();
    recipe.masks.push(LocalAdjustment {
        mask: test_radial(0.5),
        inverted: true,
        amount: 0.75,
        name: "portrait".to_string(),
        ..Default::default()
    });

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    assert!(xmp.contains("crs:MaskInverted=\"true\""));
    assert!(xmp.contains("crs:Flipped=\"false\""));
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);

    assert_eq!(restored.masks.len(), 1);
    assert!(restored.masks[0].inverted);
    assert_eq!(restored.masks[0].amount, 0.75);
    let MaskGeometry::Radial { feather, .. } = restored.masks[0].mask else {
        panic!("the radial mask must stay radial");
    };
    assert!((feather - 0.5).abs() < 0.01);
}

#[test]
fn xmp_round_trip_preserves_crop_and_straighten() {
    let recipe = EditRecipe {
        crop: Some(Crop {
            left: 0.1,
            top: 0.2,
            right: 0.9,
            bottom: 0.8,
        }),
        straighten_deg: 3.5,
        ..Default::default()
    };

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    assert!(xmp.contains("CropLeft=\"0.100000\""));
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);

    let crop = restored.crop.unwrap();
    assert!((crop.left - 0.1).abs() < 1e-4);
    assert!((crop.top - 0.2).abs() < 1e-4);
    assert!((crop.right - 0.9).abs() < 1e-4);
    assert!((crop.bottom - 0.8).abs() < 1e-4);
    assert!((restored.straighten_deg - 3.5).abs() < 1e-3);
}

#[test]
fn xmp_round_trip_preserves_tone_curve_and_point_colour() {
    let mut swatch = PointColor::sampled(0.2, 0.7, 0.5);
    swatch.hue_shift = 0.2;
    let recipe = EditRecipe {
        tone_curve: vec![
            CurvePoint {
                input: 0,
                output: 0,
            },
            CurvePoint {
                input: 128,
                output: 150,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ],
        point_colors: vec![swatch],
        ..Default::default()
    };

    let xmp = autoshade::xmp::recipe_to_xmp(&recipe);
    assert!(xmp.contains("crs:PointColors"));
    let restored = autoshade::xmp::xmp_to_recipe(&xmp);

    assert_eq!(restored.tone_curve.len(), 3);
    assert_eq!(restored.point_colors.len(), 1);
    assert!((restored.point_colors[0].hue_shift - 0.2).abs() < 1e-3);
}

#[test]
fn changing_a_point_colour_range_rebuilds_all_three_windows() {
    let mut swatch = PointColor::sampled(0.2, 0.7, 0.5);
    let before = (swatch.hue_range, swatch.sat_range, swatch.lum_range);

    swatch.set_range(0.9);

    assert!((swatch.range_amount - 0.9).abs() < 1e-6);
    assert_ne!(swatch.hue_range, before.0);
    assert_ne!(swatch.sat_range, before.1);
    assert_ne!(swatch.lum_range, before.2);
    assert!(swatch.hue_range[0] <= 0.5 && swatch.hue_range[3] >= 0.5);
    assert!(swatch.sat_range[0] <= swatch.src_sat && swatch.sat_range[3] >= swatch.src_sat);
    assert!(swatch.lum_range[0] <= swatch.src_lum && swatch.lum_range[3] >= swatch.src_lum);
}

#[test]
fn a_synthetic_face_report_rejects_an_outline_with_too_few_points() {
    let json = r#"{
        "version": 1,
        "backend": "synthetic",
        "width": 32,
        "height": 32,
        "faces": [{
            "confidence": 0.9,
            "box": [8, 8, 24, 24],
            "oval": [[8, 8], [24, 24]],
            "left_eye": [[10, 12], [12, 12], [11, 14]],
            "right_eye": [[20, 12], [22, 12], [21, 14]]
        }]
    }"#;

    let error = autoshade::face::parse_faces(json).unwrap_err();

    assert!(error.to_string().contains("face outline"));
}

#[test]
fn a_face_mask_covers_the_face_centre_and_not_the_corner() {
    let report = autoshade::face::FaceReport {
        backend: "synthetic".to_string(),
        width: 32,
        height: 32,
        faces: vec![autoshade::face::Face {
            confidence: Some(0.9),
            bbox: [8.0, 8.0, 24.0, 24.0],
            oval: vec![[8.0, 24.0], [24.0, 24.0], [16.0, 8.0]],
            left_eye: Vec::new(),
            right_eye: Vec::new(),
        }],
    };

    let mask = autoshade::face::face_mask(&report, 32, 32);

    assert!(mask.get_pixel(16, 18)[0] > 200);
    assert_eq!(mask.get_pixel(0, 0)[0], 0);
}

#[test]
fn a_manifest_plane_path_cannot_escape_the_manifest_folder() {
    let tree = TempTree::new("semantic-path");
    let manifest = r#"{
        "version": 1,
        "width": 4,
        "height": 4,
        "planes": [{
            "class_id": 1,
            "label": "sky",
            "mean_confidence": 0.9,
            "share": 0.5,
            "path": "../escape.png"
        }]
    }"#;

    let error = autoshade::segment::parse_multi_manifest(manifest, tree.path()).unwrap_err();

    assert!(error.to_string().contains("one relative file name"));
}

#[test]
fn a_multi_class_manifest_sorts_planes_by_class_id() {
    let tree = TempTree::new("semantic-sort");
    image::GrayImage::from_pixel(4, 4, image::Luma([10]))
        .save(tree.path().join("seven.png"))
        .unwrap();
    image::GrayImage::from_pixel(4, 4, image::Luma([20]))
        .save(tree.path().join("two.png"))
        .unwrap();
    let manifest = r#"{
        "version": 1,
        "width": 4,
        "height": 4,
        "planes": [
            {"class_id": 7, "label": "seven", "mean_confidence": 0.9, "share": 0.5, "path": "seven.png"},
            {"class_id": 2, "label": "two", "mean_confidence": 0.8, "share": 0.5, "path": "two.png"}
        ]
    }"#;

    let parsed = autoshade::segment::parse_multi_manifest(manifest, tree.path()).unwrap();

    assert_eq!(
        parsed.planes.iter().map(|p| p.class_id).collect::<Vec<_>>(),
        [2, 7]
    );
}

#[test]
fn an_ai_mask_disclosure_names_unresolved_masks_and_reasons() {
    let disclosure = autoshade::segment::AiMaskResolution {
        resolved: 1,
        cached: 1,
        renewed: 0,
        unresolved: vec![("sky".to_string(), "sidecar unavailable".to_string())],
    };

    let text = disclosure.describe().unwrap();

    assert!(text.contains("sky"));
    assert!(text.contains("sidecar unavailable"));
}

#[test]
fn a_recipe_with_too_many_masks_is_truncated_and_reported() {
    let mut recipe = EditRecipe::default();
    for _ in 0..65 {
        recipe.masks.push(LocalAdjustment::default());
    }

    let summary = recipe.clamp();

    assert_eq!(recipe.masks.len(), 64);
    assert_eq!(summary.dropped_masks, 1);
}

#[test]
fn direction_adherence_clamps_and_reports_its_tier() {
    let direct = autoshade::recipe::DirectionAdherence::new(0.6);
    let nan = autoshade::recipe::DirectionAdherence::new(f32::NAN);

    assert_eq!(direct.tier(), autoshade::recipe::AdherenceTier::Direct);
    assert_eq!(nan.get(), autoshade::recipe::DirectionAdherence::DEFAULT);
}

#[test]
fn grade_strength_clamps_and_reports_its_tier() {
    let committed = autoshade::recipe::GradeStrength::new(2.0);
    let nan = autoshade::recipe::GradeStrength::new(f32::NAN);

    assert_eq!(committed.get(), 1.0);
    assert_eq!(committed.tier(), autoshade::recipe::StrengthTier::Committed);
    assert_eq!(nan.get(), autoshade::recipe::GradeStrength::DEFAULT);
}

#[test]
fn a_library_settings_line_lists_only_non_neutral_controls() {
    let recipe = EditRecipe {
        exposure_ev: 1.0,
        contrast: -10.0,
        masks: vec![LocalAdjustment::default()],
        tone_curve: vec![
            CurvePoint {
                input: 0,
                output: 0,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ],
        ..Default::default()
    };

    let line = autoshade::library::settings_line(&recipe);

    assert!(line.contains("exposure_ev=+1.00"));
    assert!(line.contains("contrast=-10.00"));
    assert!(line.contains("tone_curve=2 points"));
    assert!(line.contains("local_masks=1"));
}

#[test]
fn a_local_exposure_mask_changes_only_its_covered_pixels() {
    let source = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        11,
        11,
        image::Rgb([128, 128, 128]),
    ));
    let mut recipe = EditRecipe::default();
    recipe.masks.push(LocalAdjustment {
        mask: MaskGeometry::Radial {
            top: 0.3,
            left: 0.3,
            bottom: 0.7,
            right: 0.7,
            feather: 0.0,
            roundness: 0.0,
            flipped: false,
            angle: 0.0,
            midpoint: 50.0,
            mask_version: 2,
        },
        exposure_ev: 1.0,
        ..Default::default()
    });

    let developed = autoshade::render::develop_preview(&source, &recipe).to_rgb8();

    assert!(developed.get_pixel(5, 5)[0] > developed.get_pixel(0, 0)[0] + 5);
}

#[test]
fn a_cropped_render_returns_only_the_crop_rectangle() {
    let source = image::DynamicImage::ImageRgb8(image::RgbImage::from_pixel(
        12,
        8,
        image::Rgb([80, 120, 160]),
    ));
    let recipe = EditRecipe {
        crop: Some(Crop {
            left: 0.25,
            top: 0.25,
            right: 0.75,
            bottom: 0.75,
        }),
        ..Default::default()
    };

    let rendered = autoshade::render::render_baked_to_image(
        &source,
        &recipe,
        None,
        None,
        None,
        &autoshade::diag::dropped(),
    )
    .unwrap();

    assert_eq!(rendered.dimensions(), (6, 4));
}

#[test]
fn exporting_a_baked_photo_writes_a_decodable_deliverable() {
    let tree = TempTree::new("render-export");
    let source = tree.path().join("source.png");
    let out = tree.path().join("delivery.jpg");
    image::RgbImage::from_pixel(10, 6, image::Rgb([40, 80, 120]))
        .save(&source)
        .unwrap();

    let dimensions = autoshade::render::render_to_file(
        &source,
        &EditRecipe::default(),
        &out,
        None,
        None,
        autoshade::diag::silent(),
    )
    .unwrap();
    let decoded = image::open(&out).unwrap();

    assert_eq!(dimensions, (10, 6));
    assert_eq!(decoded.dimensions(), (10, 6));
}

#[test]
fn library_search_prefers_the_closest_scene_and_can_exclude_the_query_photo() {
    let tree = TempTree::new("library-search");
    let same_path = tree.path().join("same.jpg");
    let entry = |id: &str, path: &Path, image: Vec<f32>| autoshade::library::Entry {
        id: id.to_string(),
        path: path.display().to_string(),
        stamp: autoshade::library::Stamp {
            len: 1,
            modified: 0,
        },
        image,
        phrases: None,
        settings: None,
        settings_checked: None,
    };
    let library = autoshade::library::Library {
        version: autoshade::library::VERSION,
        dir: "fixtures".to_string(),
        model: "synthetic".to_string(),
        entries: vec![
            entry("same", &same_path, vec![1.0, 0.0]),
            entry("near", &tree.path().join("near.jpg"), vec![0.8, 0.6]),
            entry("far", &tree.path().join("far.jpg"), vec![-1.0, 0.0]),
        ],
    };
    let query = autoshade::library::Query {
        image: &[1.0, 0.0],
        exclude: Some(&same_path),
    };

    let hits = library.search(&query, 2);

    assert_eq!(
        hits.iter()
            .map(|hit| hit.entry.id.as_str())
            .collect::<Vec<_>>(),
        ["near", "far"]
    );
}

#[test]
fn a_shifted_frame_is_aligned_before_noise_stacking() {
    let (w, h) = (64, 64);
    let mut reference = vec![[0.0; 3]; w * h];
    for y in 0..h {
        for x in 0..w {
            let value = (0.45 + 0.25 * (x as f32 * 0.31).sin() + 0.2 * (y as f32 * 0.23).cos())
                .clamp(0.0, 1.0);
            reference[y * w + x] = [value, value, value];
        }
    }
    let mut moving = reference.clone();
    for y in 0..h {
        for x in 0..w {
            let sx = x.saturating_sub(3);
            let sy = y.saturating_sub(2);
            moving[y * w + x] = reference[sy * w + sx];
        }
    }
    let params = autoshade::stack::align::AlignParams {
        levels: 3,
        iters: 20,
        blocks: 0,
        min_texture: 1e-4,
        max_residual: 24.0,
    };
    let warp = autoshade::stack::align::solve(&reference, &moving, w, h, &params);
    let aligned = autoshade::stack::align::warp_rgb(&moving, w, h, &warp);
    let mean_error = |image: &[[f32; 3]]| {
        let mut sum = 0.0;
        let mut count = 0;
        for y in 8..56 {
            for x in 8..56 {
                let i = y * w + x;
                sum += (image[i][0] - reference[i][0]).abs();
                count += 1;
            }
        }
        sum / count as f32
    };

    assert!(mean_error(&aligned) < mean_error(&moving) * 0.25);
}

#[test]
fn a_directory_output_target_is_refused_before_rendering() {
    let tree = TempTree::new("preflight-directory");
    let source = tree.path().join("source.dng");
    let target = tree.path().join("already-a-directory");
    std::fs::create_dir(&target).unwrap();

    let result = autoshade::pipeline::preflight_out(&target, &source);

    assert!(result.is_err());
    assert!(target.is_dir());
}

#[test]
fn an_existing_photo_at_the_output_path_is_not_replaced() {
    let tree = TempTree::new("preflight-existing-photo");
    let source = tree.path().join("source.dng");
    let target = tree.path().join("deliverable.jpg");
    std::fs::write(&target, b"existing photo bytes").unwrap();

    let result = autoshade::pipeline::preflight_out(&target, &source);

    assert!(result.is_err());
    assert_eq!(std::fs::read(&target).unwrap(), b"existing photo bytes");
}

#[test]
fn a_new_output_target_creates_its_parent_directory() {
    let tree = TempTree::new("preflight-new-target");
    let source = tree.path().join("source.dng");
    let target = tree.path().join("nested").join("deliverable.png");

    autoshade::pipeline::preflight_out(&target, &source).unwrap();

    assert!(target.parent().unwrap().is_dir());
}

#[cfg(unix)]
#[test]
fn a_read_only_output_directory_is_refused_before_rendering() {
    let tree = TempTree::new("preflight-read-only-dir");
    let source = tree.path().join("source.dng");
    let directory = tree.path().join("read-only");
    std::fs::create_dir(&directory).unwrap();
    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o555)).unwrap();

    let result = autoshade::pipeline::preflight_out(&directory.join("deliverable.png"), &source);

    std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.is_err());
}

#[cfg(unix)]
#[test]
fn an_output_symlink_to_a_photo_is_refused() {
    let tree = TempTree::new("preflight-photo-symlink");
    let source = tree.path().join("source.dng");
    std::fs::write(&source, b"photo bytes").unwrap();
    let target = tree.path().join("deliverable.jpg");
    std::os::unix::fs::symlink(&source, &target).unwrap();

    let result = autoshade::pipeline::preflight_out(&target, &source);

    assert!(result.is_err());
    assert!(target.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(std::fs::read(&source).unwrap(), b"photo bytes");
}
