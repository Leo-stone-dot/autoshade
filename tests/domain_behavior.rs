mod support;

use std::io::Cursor;

use base64::Engine as _;
use autoshade::describe::{MODEL_REVISION, PROMPT_VERSION};
use autoshade::embed::EMBED_DIM;
use autoshade::face::{FaceReport, background_adjustment, face_mask, parse_faces, skin_seed};
use autoshade::config::Config;
use autoshade::recipe::{
    BrushStroke, Crop, CurvePoint, EditRecipe, LensProfile, LocalAdjustment, MaskGeometry,
    MaskRole, RangeMask,
};
use autoshade::retouch::{HealSpot, heal_planar, plan_from_alpha, spots_inside};
use autoshade::session::{
    assistant_turns, budget_exhausted, commit_paths, failure_kind, resolve_session, tool_calls,
    tool_results, usage_of, verdict_lines,
};
use autoshade::stack::align::{Affine, Warp};
use image::{DynamicImage, GrayImage, ImageBuffer, Luma, Rgb, RgbImage};
use rawler::decoders::Orientation;
use serde_json::json;
use support::{ScopedEnv, TempTree};

fn unit_vector() -> Vec<f32> {
    let mut vector = vec![0.0; EMBED_DIM];
    vector[0] = 1.0;
    vector
}

fn face_json(confidence: Option<f32>) -> String {
    json!({
        "version": 1,
        "backend": "synthetic",
        "width": 20,
        "height": 20,
        "faces": [{
            "confidence": confidence,
            "box": [2.0, 2.0, 18.0, 18.0],
            "oval": [[2.0, 10.0], [10.0, 2.0], [18.0, 10.0], [10.0, 18.0]],
            "left_eye": [[6.0, 7.0], [8.0, 7.0], [8.0, 9.0], [6.0, 9.0]],
            "right_eye": [[12.0, 7.0], [14.0, 7.0], [14.0, 9.0], [12.0, 9.0]]
        }]
    })
    .to_string()
}

#[cfg(unix)]
fn fake_face_sidecar(tree: &TempTree, report: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt;
    let sidecar = tree.path().join("fake-face");
    let script = tree.path().join("fake-face.py");
    let body = format!(
        "#!/bin/sh\nset -eu\nout=\nwhile [ $# -gt 0 ]; do\n  case \"$1\" in\n    --output) out=\"$2\"; shift 2;;\n    *) shift;;\n  esac\ndone\ncat > \"$out\" <<'AUTOSHADE_FACE_JSON'\n{report}\nAUTOSHADE_FACE_JSON\n"
    );
    std::fs::write(&sidecar, body).unwrap();
    std::fs::write(&script, b"fake").unwrap();
    std::fs::set_permissions(&sidecar, std::fs::Permissions::from_mode(0o755)).unwrap();
    (sidecar, script)
}

fn minimal_xmp(crs_attribute: &str) -> String {
    format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" {crs_attribute}/></rdf:RDF></x:xmpmeta>"#
    )
}

#[test]
fn session_usage_sums_only_assistant_messages() {
    let transcript = json!({
        "messages": [
            {"role": "user", "usage": {"input": 999, "output": 999}},
            {"role": "assistant", "usage": {"input": 10, "output": 5, "cacheRead": 3, "totalTokens": 18, "cost": {"total": 0.25}}},
            {"role": "toolResult", "usage": {"input": 500, "output": 500}},
            {"role": "assistant", "usage": {"input": 2, "output": 1, "cacheRead": 4, "totalTokens": 7, "cost": {"total": 0.05}}}
        ]
    });

    let usage = usage_of(&transcript);

    assert_eq!(usage.input, 12);
    assert_eq!(usage.output, 6);
    assert_eq!(usage.cache_read, 7);
    assert_eq!(usage.total_tokens, 25);
    assert!((usage.cost_usd - 0.30).abs() < 1e-9);
}

#[test]
fn a_transcript_without_messages_has_zero_usage() {
    assert_eq!(
        usage_of(&json!({})),
        autoshade::session::Usage::default()
    );
}

#[test]
fn session_turn_and_tool_counts_ignore_non_assistant_rows() {
    let transcript = json!({
        "messages": [
            {"role": "assistant", "content": [{"type": "toolCall"}, {"type": "text"}]},
            {"role": "toolResult"},
            {"role": "assistant", "content": [{"type": "toolCall"}]},
            {"role": "user", "content": [{"type": "toolCall"}]}
        ]
    });

    assert_eq!(assistant_turns(&transcript), 2);
    assert_eq!(tool_calls(&transcript), 2);
    assert_eq!(tool_results(&transcript), 1);
}

#[test]
fn a_transcript_without_messages_has_no_turns_or_tools() {
    let transcript = json!({});

    assert_eq!(assistant_turns(&transcript), 0);
    assert_eq!(tool_calls(&transcript), 0);
    assert_eq!(tool_results(&transcript), 0);
}

#[test]
fn session_verdict_lines_keep_decision_and_score_together() {
    let transcript = json!({
        "verdicts": [
            {"decision": "revise", "score": 80},
            {"decision": "accept", "score": 91}
        ]
    });

    assert_eq!(verdict_lines(&transcript), ["revise 80", "accept 91"]);
}

#[test]
fn a_transcript_without_verdicts_has_no_verdict_lines() {
    assert!(verdict_lines(&json!({})).is_empty());
}

#[test]
fn a_failed_transcript_without_verdicts_does_not_invent_one() {
    assert!(verdict_lines(&json!({"error": "model_error"})).is_empty());
}

#[test]
fn a_legacy_commit_string_exposes_the_same_three_paths() {
    let transcript = json!({
        "commit": "{\"develop\":\"photo.develop.json\",\"xmp\":\"photo.xmp\",\"note\":\"kept\"}"
    });

    assert_eq!(
        commit_paths(&transcript),
        (
            "photo.develop.json".to_string(),
            "photo.xmp".to_string(),
            "kept".to_string()
        )
    );
}

#[test]
fn a_commit_object_exposes_its_develop_xmp_and_note_paths() {
    let transcript = json!({
        "commit": {
            "develop": "photo.develop.json",
            "xmp": "photo.xmp",
            "note": "kept"
        }
    });

    assert_eq!(
        commit_paths(&transcript),
        (
            "photo.develop.json".to_string(),
            "photo.xmp".to_string(),
            "kept".to_string()
        )
    );
}

#[test]
fn a_transcript_without_a_commit_has_no_commit_paths() {
    assert_eq!(
        commit_paths(&json!({})),
        (String::new(), String::new(), String::new())
    );
}

#[test]
fn a_legacy_budget_error_is_reported_as_budget_exhaustion() {
    let transcript = json!({"error": "budget_exhausted: turn limit"});

    assert_eq!(failure_kind(&transcript).as_deref(), Some("budget_exhausted"));
    assert!(budget_exhausted(&transcript));
}

#[test]
fn an_explicit_budget_exhaustion_flag_is_reported() {
    assert!(budget_exhausted(&json!({"budget_exhausted": true})));
}

#[test]
fn a_false_budget_exhaustion_flag_is_not_reported() {
    assert!(!budget_exhausted(&json!({"budget_exhausted": false})));
}

#[test]
fn a_model_error_is_not_reported_as_budget_exhaustion() {
    assert!(!budget_exhausted(&json!({"error": "model_error: 401"})));
}

#[test]
fn session_resolution_prefers_an_existing_direct_path() {
    let tree = TempTree::new("session-direct");
    let direct = tree.path().join("direct.json");
    let sessions = tree.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(&direct, "{}").unwrap();
    std::fs::write(sessions.join("direct.json"), "{}").unwrap();

    let resolved = resolve_session(direct.to_str().unwrap(), &sessions).unwrap();

    assert_eq!(resolved, direct);
}

#[test]
fn a_session_id_resolves_to_its_json_file() {
    let tree = TempTree::new("session-id");
    let sessions = tree.path().join("sessions");
    let expected = sessions.join("analyze-9.json");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(&expected, "{}").unwrap();

    assert_eq!(resolve_session("analyze-9", &sessions).unwrap(), expected);
}

#[test]
fn a_session_filename_resolves_inside_the_sessions_directory() {
    let tree = TempTree::new("session-filename");
    let sessions = tree.path().join("sessions");
    let expected = sessions.join("named.json");
    std::fs::create_dir_all(&sessions).unwrap();
    std::fs::write(&expected, "{}").unwrap();

    assert_eq!(resolve_session("named.json", &sessions).unwrap(), expected);
}

#[test]
fn a_missing_session_reports_where_resolution_was_attempted() {
    let tree = TempTree::new("session-missing");
    let sessions = tree.path().join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();

    let error = resolve_session("analyze-missing", &sessions)
        .unwrap_err()
        .to_string();

    assert!(error.contains("no harness session"), "error={error}");
    assert!(
        error.contains(&sessions.display().to_string()),
        "error={error}"
    );
}

#[test]
fn a_structured_session_failure_keeps_its_failure_class() {
    let transcript = json!({
        "failure": "model_error",
        "error": "model_error: 401"
    });

    assert_eq!(failure_kind(&transcript).as_deref(), Some("model_error"));
}

#[test]
fn an_unclassified_legacy_session_error_uses_the_generic_failure_class() {
    let transcript = json!({"error": "the critic never produced a JSON verdict"});

    assert_eq!(failure_kind(&transcript).as_deref(), Some("error"));
}

#[test]
fn a_session_without_an_error_has_no_failure_class() {
    assert_eq!(failure_kind(&json!({"verdicts": []})), None);
}

#[test]
fn description_sanitising_collapses_spaces_and_removes_invisible_controls() {
    let cleaned = autoshade::describe::sanitize_desc("  warm\n\tfaded\u{200b}  sky  ").unwrap();

    assert_eq!(cleaned, "warm faded sky");
}

#[test]
fn description_sanitising_caps_by_characters_without_splitting_utf8() {
    let cleaned = autoshade::describe::sanitize_desc(&"\u{6c49}".repeat(300)).unwrap();

    assert_eq!(cleaned.chars().count(), autoshade::describe::MAX_DESC_CHARS);
}

#[test]
fn description_records_keep_a_per_photo_sidecar_error_as_data() {
    let records = autoshade::describe::parse_records(
        "{\"path\":\"a.jpg\",\"error\":\"model could not read the frame\"}\n",
    )
    .unwrap();

    assert_eq!(records.len(), 1);
    assert_eq!(records[0].path, "a.jpg");
    assert!(records[0].desc.is_none());
    assert_eq!(records[0].error.as_deref(), Some("model could not read the frame"));
}

#[test]
fn description_records_refuse_an_answer_from_a_different_model() {
    let line = json!({
        "path": "a.jpg",
        "model": "other/model",
        "revision": MODEL_REVISION,
        "prompt_version": PROMPT_VERSION,
        "desc": "warm"
    })
    .to_string();

    assert!(autoshade::describe::parse_records(&line).is_err());
}

#[test]
fn embedding_parser_accepts_a_normalised_vector() {
    let record = json!({"vector": unit_vector()}).to_string();

    let parsed = autoshade::embed::parse_record(&record).unwrap();

    assert_eq!(parsed.vector.len(), EMBED_DIM);
    assert!((parsed.vector[0] - 1.0).abs() < 1e-6);
}

#[test]
fn embedding_parser_refuses_an_unnormalised_vector() {
    let record = json!({"vector": vec![0.0; EMBED_DIM]}).to_string();

    assert!(autoshade::embed::parse_record(&record).is_err());
}

#[test]
fn embedding_batch_keeps_one_bad_record_as_a_partial_failure() {
    let bad = json!({"path": "bad.jpg", "vector": vec![0.0; EMBED_DIM]}).to_string();
    let good = json!({"path": "good.jpg", "vector": unit_vector()}).to_string();

    let records = autoshade::embed::parse_batch_records(&format!("{bad}\n{good}\n")).unwrap();

    assert_eq!(records.len(), 2);
    assert_eq!(records[0].path, "bad.jpg");
    assert!(records[0].record.is_none());
    assert!(records[0].error.is_some());
    assert_eq!(records[1].path, "good.jpg");
    assert!(records[1].record.is_some());
    assert!(records[1].error.is_none());
}

#[test]
fn embedding_text_parser_refuses_a_batch_of_the_wrong_size() {
    let answer = json!({"text_vectors": [unit_vector()]}).to_string();

    assert!(autoshade::embed::parse_text_vectors(&answer, 2).is_err());
}

#[test]
fn image_generation_models_are_not_offered_as_chat_models() {
    assert!(autoshade::openai_models::is_image_model("Qwen/Qwen-Image"));
    assert!(!autoshade::openai_models::is_chat_model("Qwen/Qwen-Image"));
}

#[test]
fn vendor_prefixed_chat_models_are_offered_by_the_chat_filter() {
    assert!(autoshade::openai_models::is_chat_model("anthropic/claude-4"));
    assert!(autoshade::openai_models::is_chat_model("deepseek-chat"));
}

#[test]
fn completion_only_models_are_not_offered_as_chat_models() {
    assert!(!autoshade::openai_models::is_chat_model("davinci-002"));
}

#[test]
fn endpoint_comparison_ignores_surrounding_whitespace_and_one_trailing_slash() {
    assert!(autoshade::config::same_endpoint(
        " https://example.test/v1/ ",
        "https://example.test/v1"
    ));
}

#[test]
fn a_valid_synthetic_face_report_builds_a_face_mask_at_the_center() {
    let report: FaceReport = parse_faces(&face_json(Some(0.9))).unwrap();

    let mask = face_mask(&report, 20, 20);

    assert!(mask.get_pixel(10, 10).0[0] > 0);
    assert_eq!(mask.get_pixel(0, 0).0[0], 0);
}

#[test]
fn a_face_report_with_an_out_of_range_confidence_is_refused() {
    assert!(parse_faces(&face_json(Some(1.1))).is_err());
}

#[test]
fn skin_seed_samples_only_pixels_the_face_mask_holds() {
    let mut reference = RgbImage::from_pixel(3, 3, Rgb([10, 20, 30]));
    reference.put_pixel(1, 1, Rgb([200, 100, 50]));
    let mut face = GrayImage::new(3, 3);
    face.put_pixel(1, 1, Luma([255]));

    let (seed, at) = skin_seed(&reference, &face).unwrap();

    assert_eq!(seed, [200.0 / 255.0, 100.0 / 255.0, 50.0 / 255.0]);
    assert!((at[0] - 0.5).abs() < 1e-6 && (at[1] - 0.5).abs() < 1e-6);
}

#[test]
fn skin_seed_returns_none_without_face_coverage() {
    let reference = RgbImage::from_pixel(3, 3, Rgb([10, 20, 30]));
    let face = GrayImage::new(3, 3);

    assert!(skin_seed(&reference, &face).is_none());
}

#[cfg(unix)]
#[test]
fn resolving_face_intents_writes_face_and_eyes_rasters_and_reuses_cache() {
    let tree = TempTree::new("resolve-face-valid");
    let raw = tree.path().join("photo.dng");
    let src = tree.path().join("photo.png");
    RgbImage::from_pixel(20, 20, Rgb([80, 100, 120])).save(&src).unwrap();
    let (sidecar, script) = fake_face_sidecar(&tree, &face_json(Some(0.9)));
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    let mut config = Config::load();
    config.python_bin = sidecar.to_string_lossy().into_owned();
    config.face_script = script.to_string_lossy().into_owned();
    let intent = |role| LocalAdjustment {
        mask: MaskGeometry::Bitmap { path: String::new() },
        role,
        ..LocalAdjustment::default()
    };
    let mut recipe = EditRecipe {
        masks: vec![intent(MaskRole::Face), intent(MaskRole::Eyes)],
        ..EditRecipe::default()
    };

    let first = autoshade::face::resolve_face_masks(&config, &raw, &src, &mut recipe);

    assert_eq!(first.resolved, 2, "dropped: {:?}", first.dropped);
    assert!(first.dropped.is_empty());
    assert!(recipe.masks.iter().all(|mask| matches!(
        mask.mask,
        MaskGeometry::Bitmap { ref path } if !path.is_empty()
    )));
    std::fs::remove_file(&sidecar).unwrap();
    std::fs::remove_file(&script).unwrap();
    let mut second_recipe = EditRecipe {
        masks: vec![intent(MaskRole::Face), intent(MaskRole::Eyes)],
        ..EditRecipe::default()
    };
    let second = autoshade::face::resolve_face_masks(&config, &raw, &src, &mut second_recipe);
    assert_eq!(second.resolved, 2);
    assert_eq!(second.cached, 2);
}

#[cfg(unix)]
#[test]
fn an_unusable_face_report_removes_face_intents_and_reports_the_reason() {
    let tree = TempTree::new("resolve-face-invalid");
    let raw = tree.path().join("photo.dng");
    let src = tree.path().join("photo.png");
    RgbImage::from_pixel(20, 20, Rgb([80, 100, 120])).save(&src).unwrap();
    let (sidecar, script) = fake_face_sidecar(&tree, "not a face report");
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    let mut config = Config::load();
    config.python_bin = sidecar.to_string_lossy().into_owned();
    config.face_script = script.to_string_lossy().into_owned();
    let mut recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: String::new() },
            role: MaskRole::Face,
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let resolution = autoshade::face::resolve_face_masks(&config, &raw, &src, &mut recipe);

    assert!(recipe.masks.is_empty());
    assert_eq!(resolution.dropped.len(), 1);
    assert!(!resolution.dropped[0].1.is_empty());
}

#[test]
fn a_background_portrait_adjustment_is_inverted_and_names_its_role() {
    let adjustment = background_adjustment("subject.png".to_string());

    assert!(adjustment.inverted);
    assert_eq!(adjustment.role, MaskRole::Background);
}

#[test]
fn a_quarter_turn_moves_the_pixel_at_the_bottom_left_to_the_top_left() {
    let source = RgbImage::from_fn(2, 2, |x, y| match (x, y) {
        (0, 0) => Rgb([1, 2, 3]),
        (1, 0) => Rgb([4, 5, 6]),
        (0, 1) => Rgb([7, 8, 9]),
        _ => Rgb([10, 11, 12]),
    });

    let turned = autoshade::render::turn_image(DynamicImage::ImageRgb8(source), 1).to_rgb8();

    assert_eq!(turned.get_pixel(0, 0), &Rgb([7, 8, 9]));
    assert_eq!(turned.get_pixel(1, 0), &Rgb([1, 2, 3]));
}

#[test]
fn a_quarter_turn_maps_points_and_vectors_with_the_same_rotation() {
    let point = autoshade::render::orient_point(Orientation::Rotate90, 0.1, 0.2);
    let vector = autoshade::render::orient_vector(Orientation::Rotate90, 0.3, -0.4);

    assert!((point.0 - 0.8).abs() < 1e-6 && (point.1 - 0.1).abs() < 1e-6);
    assert!((vector.0 - 0.4).abs() < 1e-6 && (vector.1 - 0.3).abs() < 1e-6);
}

#[test]
fn a_rotated_inscribed_rectangle_stays_positive_and_inside_the_frame() {
    let (width, height) = autoshade::render::inscribed_dims(100.0, 50.0, 45.0);

    assert!(width.is_finite() && height.is_finite());
    assert!(width > 0.0 && width <= 100.0);
    assert!(height > 0.0 && height <= 50.0);
}

#[test]
fn local_temperature_moves_from_cool_to_warm_around_its_anchor() {
    let cool = autoshade::render::local_temp_to_kelvin(-100.0);
    let neutral = autoshade::render::local_temp_to_kelvin(0.0);
    let warm = autoshade::render::local_temp_to_kelvin(100.0);

    assert!(cool < neutral && neutral < warm);
}

#[test]
fn a_point_colour_swatch_round_trips_to_the_colour_it_was_sampled_from() {
    let source = [0.8, 0.2, 0.1];
    let swatch = autoshade::render::point_color_at(source).unwrap();

    let restored = autoshade::render::point_color_rgb(&swatch);

    for (actual, expected) in restored.into_iter().zip(source) {
        assert!((actual - expected).abs() < 1e-4);
    }
}

#[test]
fn colour_range_tolerance_round_trips_inside_the_schema() {
    let amount = 0.37;
    let tolerance = autoshade::render::colour_range_tolerance(amount);

    let restored = autoshade::render::colour_range_amount(tolerance);

    assert!((restored - amount).abs() < 1e-6);
}

#[test]
fn a_black_colour_range_reference_selects_nothing() {
    let range = RangeMask::Color {
        r: 0.0,
        g: 0.0,
        b: 0.0,
        amount: 1.0,
        px: 0.5,
        py: 0.5,
    };

    assert_eq!(autoshade::render::range_weight(&range, &[0.2, 0.2, 0.2]), 0.0);
}

#[test]
fn a_single_midpoint_curve_keeps_the_neutral_endpoints_and_hits_its_point() {
    let lut = autoshade::render::curve_lut(&[CurvePoint { input: 128, output: 192 }]);

    assert!(lut[0].abs() < 1e-6);
    assert!((lut[128] - 192.0 / 255.0).abs() < 0.01);
    assert!((lut[255] - 1.0).abs() < 1e-6);
}

#[test]
fn srgb_export_leaves_pixels_unchanged() {
    let source = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1, 1, Rgb([1234, 2345, 3456])));

    let converted = autoshade::render::convert_export_color_space(
        source,
        autoshade::render::ExportColorSpace::Srgb,
    );

    assert_eq!(converted.to_rgb16().get_pixel(0, 0), &Rgb([1234, 2345, 3456]));
}

#[test]
fn a_white_pixel_stays_white_in_display_p3() {
    let source = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1, 1, Rgb([65535, 65535, 65535])));

    let converted = autoshade::render::convert_export_color_space(
        source,
        autoshade::render::ExportColorSpace::DisplayP3,
    );

    assert!(converted.to_rgb16().get_pixel(0, 0).0.iter().all(|&v| v >= 65534));
}

#[test]
fn a_saturated_srgb_colour_changes_when_encoded_as_display_p3() {
    let source = DynamicImage::ImageRgb16(ImageBuffer::from_pixel(1, 1, Rgb([65535, 0, 0])));

    let converted = autoshade::render::convert_export_color_space(
        source,
        autoshade::render::ExportColorSpace::DisplayP3,
    );

    let pixel = converted.to_rgb16().get_pixel(0, 0).0;
    assert_ne!(pixel, [65535, 0, 0]);
    assert!(pixel[1] > 0 && pixel[2] > 0, "P3 encoding must move the sRGB red primary: {pixel:?}");
}

#[test]
fn a_delivered_tiff_carries_the_selected_icc_profile() {
    let tree = TempTree::new("export-icc");
    let source = tree.path().join("source.png");
    let srgb = tree.path().join("srgb.tif");
    let p3 = tree.path().join("display-p3.tif");
    RgbImage::from_pixel(8, 8, Rgb([32, 96, 160])).save(&source).unwrap();
    let base = autoshade::render::ExportOpts {
        eight_bit: true,
        ..autoshade::render::ExportOpts::default()
    };
    autoshade::render::render_to_file(
        &source,
        &EditRecipe::default(),
        &srgb,
        None,
        Some(&base),
        autoshade::diag::silent(),
    )
    .unwrap();
    autoshade::render::render_to_file(
        &source,
        &EditRecipe::default(),
        &p3,
        None,
        Some(&autoshade::render::ExportOpts {
            color_space: autoshade::render::ExportColorSpace::DisplayP3,
            ..base
        }),
        autoshade::diag::silent(),
    )
    .unwrap();

    let profile = |path: &std::path::Path| {
        let mut decoder =
            tiff::decoder::Decoder::new(Cursor::new(std::fs::read(path).unwrap())).unwrap();
        decoder
            .get_tag_u8_vec(tiff::tags::Tag::IccProfile)
            .expect("the TIFF must carry an ICC profile tag")
    };
    let srgb_profile = profile(&srgb);
    let p3_profile = profile(&p3);

    assert!(!srgb_profile.is_empty());
    assert!(!p3_profile.is_empty());
    assert_ne!(srgb_profile, p3_profile);
}

#[test]
fn a_horizontal_perspective_keystone_changes_the_rendered_frame() {
    let source = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |x, y| {
        let line = (x / 8 + y / 8) % 2 == 0;
        Rgb(if line { [220, 220, 220] } else { [40, 40, 40] })
    }));
    let neutral = autoshade::render::frame_and_finish(
        source.clone(),
        &EditRecipe::default(),
        &LensProfile::default(),
        autoshade::render::FilmScale::NATIVE,
        autoshade::render::CropPolicy::Cut,
    )
    .to_rgb8();
    let keystone = autoshade::render::frame_and_finish(
        source,
        &EditRecipe {
            perspective_horizontal: 40.0,
            ..EditRecipe::default()
        },
        &LensProfile::default(),
        autoshade::render::FilmScale::NATIVE,
        autoshade::render::CropPolicy::Cut,
    )
    .to_rgb8();

    assert_ne!(keystone.as_raw(), neutral.as_raw());
}

#[test]
fn a_vertical_perspective_keystone_changes_the_rendered_frame() {
    let source = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |x, y| {
        let line = (x / 8 + y / 8) % 2 == 0;
        Rgb(if line { [220, 220, 220] } else { [40, 40, 40] })
    }));
    let neutral = autoshade::render::frame_and_finish(
        source.clone(),
        &EditRecipe::default(),
        &LensProfile::default(),
        autoshade::render::FilmScale::NATIVE,
        autoshade::render::CropPolicy::Cut,
    )
    .to_rgb8();
    let keystone = autoshade::render::frame_and_finish(
        source,
        &EditRecipe {
            perspective_vertical: 40.0,
            ..EditRecipe::default()
        },
        &LensProfile::default(),
        autoshade::render::FilmScale::NATIVE,
        autoshade::render::CropPolicy::Cut,
    )
    .to_rgb8();

    assert_ne!(keystone.as_raw(), neutral.as_raw());
}

#[test]
fn an_hdr_rendition_compresses_a_highlight_without_lifting_black() {
    let source = DynamicImage::ImageRgb32F(
        ImageBuffer::from_raw(2, 1, vec![0.0, 0.0, 0.0, 2.0, 2.0, 2.0]).unwrap(),
    );
    let plain = autoshade::render::develop_preview(&source, &EditRecipe::default()).to_rgb32f();
    let hdr = autoshade::render::develop_preview(
        &source,
        &EditRecipe {
            hdr_edit: true,
            hdr_max_ev: 2.0,
            ..EditRecipe::default()
        },
    )
    .to_rgb32f();

    assert!(hdr.get_pixel(0, 0)[0] < 1e-6);
    assert!(plain.get_pixel(1, 0)[0] > 0.9);
    assert!(
        hdr.get_pixel(1, 0)[0] < plain.get_pixel(1, 0)[0] - 0.2,
        "plain={}, hdr={}",
        plain.get_pixel(1, 0)[0],
        hdr.get_pixel(1, 0)[0]
    );
}

#[test]
fn a_fully_rendered_global_recipe_has_no_render_gap_disclosures() {
    assert!(autoshade::xmp::global_render_gaps(&EditRecipe::default()).is_empty());
    let neutral_defringe = autoshade::xmp::xmp_to_recipe(
        r#"<rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/"
 crs:DefringePurpleAmount="0" crs:DefringePurpleHueLo="30"
 crs:DefringePurpleHueHi="70" crs:DefringeGreenAmount="0"
 crs:DefringeGreenHueLo="40" crs:DefringeGreenHueHi="60"/>"#,
    );
    assert!(autoshade::xmp::global_render_gaps(&neutral_defringe).is_empty());
    assert!(autoshade::xmp::global_render_gaps(&EditRecipe {
        passthrough: [("UprightVersion".to_string(), "151388160".to_string())]
            .into_iter()
            .collect(),
        ..EditRecipe::default()
    })
    .is_empty());

    let recipe = EditRecipe {
        exposure_ev: 0.5,
        texture: 20.0,
        post_crop_vignette: 25.0,
        grain: 15.0,
        ..EditRecipe::default()
    };

    assert!(autoshade::xmp::global_render_gaps(&recipe).is_empty());
}

#[test]
fn camera_base_knots_fit_a_known_tone_curve() {
    let neutral = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |x, y| {
        let v = ((x + y) * 255 / 126) as u8;
        Rgb([v, v, v])
    }));
    let camera = DynamicImage::ImageRgb8(RgbImage::from_fn(64, 64, |x, y| {
        let v = (x + y) as f32 / 126.0;
        let encoded = (v.sqrt() * 255.0).round().clamp(0.0, 255.0) as u8;
        Rgb([encoded, encoded, encoded])
    }));

    let knots = autoshade::render::camera_base_knots(&neutral, &camera)
        .expect("a 64x64 tone-curve pair is large enough to estimate");

    assert!(knots.len() >= 2, "a non-identity curve must produce knots: {knots:?}");
    assert!(knots.windows(2).all(|pair| pair[0][0] <= pair[1][0]));
    assert!(knots.iter().all(|knot| knot[1] >= knot[0] - 1e-5));
}

#[test]
fn camera_base_knots_refuse_a_frame_smaller_than_the_grid() {
    let tiny = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([128, 128, 128])));

    assert!(autoshade::render::camera_base_knots(&tiny, &tiny).is_none());
}

#[test]
fn chromatic_aberration_alone_does_not_move_the_shared_mask_frame() {
    let profile = LensProfile {
        ca_on: true,
        ca_r: vec![1.01],
        ca_b: vec![0.99],
        ..Default::default()
    };

    assert!(!autoshade::render::geometry_moves_frame(&profile, 0.0));
}

#[test]
fn lens_distortion_moves_the_shared_mask_frame() {
    let profile = LensProfile {
        distortion_on: true,
        distortion: vec![1.01],
        ..Default::default()
    };

    assert!(autoshade::render::geometry_moves_frame(&profile, 0.0));
}

#[test]
fn exposure_ratio_ignores_an_outlier_when_most_pixels_are_usable() {
    let dark = vec![[0.2, 0.2, 0.2]; 5];
    let mut bright = vec![[0.2, 0.2, 0.2]; 5];
    bright[2] = [0.9, 0.9, 0.9];
    let cover = vec![vec![true; 5]];

    let ratio = autoshade::stack::merge::exposure_ratio(&dark, &bright, &cover);

    assert!(ratio.abs() < 1e-6);
}

#[test]
fn an_affine_inverse_returns_the_original_point() {
    let affine = Affine([1.0, 0.1, 12.0, -0.2, 0.9, -4.0]);
    let (x, y) = affine.apply(31.0, 47.0, 200, 100);

    let restored = affine.inverse().unwrap().apply(x, y, 200, 100);

    assert!((restored.0 - 31.0).abs() < 1e-4);
    assert!((restored.1 - 47.0).abs() < 1e-4);
}

#[test]
fn an_identity_warp_keeps_every_pixel_and_covers_the_whole_frame() {
    let pixels = vec![[0.1, 0.2, 0.3], [0.4, 0.5, 0.6], [0.7, 0.8, 0.9], [1.0, 0.0, 0.0]];
    let warp = Warp::identity();

    let warped = autoshade::stack::align::warp_rgb(&pixels, 2, 2, &warp);
    let coverage = autoshade::stack::align::coverage(2, 2, &warp);

    assert_eq!(warped, pixels);
    assert_eq!(coverage, vec![true; 4]);
}

#[test]
fn a_lens_profile_false_spelling_is_read_as_disabled() {
    let xmp = minimal_xmp(r#"crs:LensProfileEnable="False""#);

    assert_eq!(autoshade::xmp::lens_profile_enabled(&xmp), Some(false));
}

#[test]
fn an_unknown_lens_profile_switch_spelling_is_not_guessed() {
    let xmp = minimal_xmp(r#"crs:LensProfileEnable="sometimes""#);

    assert_eq!(autoshade::xmp::lens_profile_enabled(&xmp), None);
}

#[test]
fn approximate_focus_distance_reads_a_rational_value() {
    let xmp = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:aux="http://ns.adobe.com/exif/1.0/aux/" aux:ApproximateFocusDistance="3/2"/></rdf:RDF></x:xmpmeta>"#;

    let distance = autoshade::xmp::approximate_focus_distance(xmp).unwrap();

    assert!((distance - 1.5).abs() < 1e-6);
}

#[test]
fn a_reference_block_names_the_attached_image_and_trusted_settings() {
    let entry = autoshade::library::Entry {
        id: "synthetic".to_string(),
        path: "reference.jpg".to_string(),
        stamp: autoshade::library::Stamp { len: 1, modified: 0 },
        image: unit_vector(),
        phrases: Some("warm\nfaded".to_string()),
        settings: Some(autoshade::library::Settings {
            raw: "reference.dng".to_string(),
            xmp: "reference.xmp".to_string(),
            xmp_stamp: autoshade::library::Stamp { len: 1, modified: 0 },
            match_de: 1.0,
            line: "exposure_ev=+0.50".to_string(),
        }),
        settings_checked: None,
    };
    let hit = autoshade::library::Hit { entry: &entry, scene: 1.0 };

    let block = autoshade::library::reference_block(
        &[hit],
        &[Some(2)],
        autoshade::library::StyleVoice::Target,
    )
    .unwrap();

    assert!(block.contains("(IMAGE 2)"));
    assert!(block.contains("warm faded"));
    assert!(block.contains("exposure_ev=+0.50"));
}

#[test]
fn a_look_summary_uses_the_first_reference_with_phrases() {
    let empty = autoshade::library::Entry {
        id: "empty".to_string(),
        path: "empty.jpg".to_string(),
        stamp: autoshade::library::Stamp { len: 1, modified: 0 },
        image: unit_vector(),
        phrases: None,
        settings: None,
        settings_checked: None,
    };
    let phrased = autoshade::library::Entry {
        id: "phrased".to_string(),
        path: "phrased.jpg".to_string(),
        stamp: autoshade::library::Stamp { len: 1, modified: 0 },
        image: unit_vector(),
        phrases: Some("cool\tquiet".to_string()),
        settings: None,
        settings_checked: None,
    };
    let hits = [
        autoshade::library::Hit { entry: &empty, scene: 0.8 },
        autoshade::library::Hit { entry: &phrased, scene: 0.7 },
    ];

    assert_eq!(autoshade::library::look_summary(&hits).as_deref(), Some("cool quiet"));
}

#[test]
fn references_without_phrases_have_no_look_summary() {
    let entry = autoshade::library::Entry {
        id: "empty".to_string(),
        path: "empty.jpg".to_string(),
        stamp: autoshade::library::Stamp { len: 1, modified: 0 },
        image: unit_vector(),
        phrases: None,
        settings: None,
        settings_checked: None,
    };
    let hits = [autoshade::library::Hit { entry: &entry, scene: 0.8 }];

    assert_eq!(autoshade::library::look_summary(&hits), None);
}

#[test]
fn a_connected_painted_alpha_blob_becomes_one_retouch_spot() {
    let mut alpha = GrayImage::from_pixel(9, 9, Luma([0]));
    for y in 3..6 {
        for x in 3..6 {
            alpha.put_pixel(x, y, Luma([255]));
        }
    }

    let (spots, note) = plan_from_alpha(&alpha, 0, &HealSpot::default());

    assert_eq!(spots.len(), 1);
    assert!(note.is_none());
    assert!((0.3..=0.6).contains(&spots[0].cx));
    assert!((0.3..=0.6).contains(&spots[0].cy));
}

#[test]
fn a_retouch_limit_mask_keeps_only_spots_whose_centers_are_inside_it() {
    let spots = vec![
        HealSpot { cx: 0.2, cy: 0.5, ..HealSpot::default() },
        HealSpot { cx: 0.8, cy: 0.5, ..HealSpot::default() },
    ];
    let mut limit = GrayImage::from_pixel(10, 10, Luma([0]));
    for y in 0..10 {
        for x in 0..5 {
            limit.put_pixel(x, y, Luma([255]));
        }
    }

    let kept = spots_inside(spots, &limit);

    assert_eq!(kept.len(), 1);
    assert!((kept[0].cx - 0.2).abs() < 1e-6);
}

#[test]
fn a_planar_heal_replaces_a_bright_painted_dot_from_its_surroundings() {
    let mut plane = vec![[0.5, 0.5, 0.5]; 21 * 21];
    for y in 9..12 {
        for x in 9..12 {
            plane[y * 21 + x] = [0.9, 0.9, 0.9];
        }
    }
    let spot = HealSpot {
        cx: 0.5,
        cy: 0.5,
        radius: 0.1,
        feather: 0.0,
        ..HealSpot::default()
    };

    let healed = heal_planar(&mut plane, 21, 21, &[spot]);

    assert_eq!(healed, 1);
    assert!(plane[10 * 21 + 10][0] < 0.9);
}

#[test]
fn a_planar_heal_with_too_short_a_buffer_is_a_no_op() {
    let mut plane = vec![[0.5, 0.5, 0.5]; 3];

    assert_eq!(heal_planar(&mut plane, 9, 9, &[HealSpot::default()]), 0);
}

#[test]
fn portable_recipe_drops_photo_bound_masks_and_geometry_but_keeps_the_look() {
    let recipe = EditRecipe {
        exposure_ev: 0.75,
        crop: Some(Crop { left: 0.1, top: 0.1, right: 0.9, bottom: 0.9 }),
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Bitmap { path: "photo-mask.png".to_string() },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let (portable, losses) = autoshade::recipe_library::portable(&recipe, false);

    assert_eq!(portable.exposure_ev, 0.75);
    assert!(portable.crop.is_none());
    assert!(portable.masks.is_empty());
    assert_eq!(losses.masks, 1);
    assert!(losses.geometry);
}

#[test]
fn replacing_a_recipe_library_entry_changes_its_saved_look() {
    let tree = TempTree::new("recipe-replace");
    autoshade::recipe_library::create_in(tree.path(), "look", &EditRecipe::default()).unwrap();
    let changed = EditRecipe { contrast: 22.0, ..EditRecipe::default() };

    autoshade::recipe_library::replace_in(tree.path(), "look", &changed).unwrap();

    assert_eq!(autoshade::recipe_library::read_in(tree.path(), "look").unwrap(), changed);
}

#[test]
fn listing_a_malformed_recipe_library_entry_reports_its_error() {
    let tree = TempTree::new("recipe-list-error");
    let dir = autoshade::recipe_library::library_dir_in(tree.path());
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("broken.json"), b"not a recipe").unwrap();

    let listed = autoshade::recipe_library::list_in(tree.path());

    assert_eq!(listed.len(), 1);
    assert_eq!(listed[0].name, "broken");
    assert!(listed[0].error.is_some());
}

#[test]
fn merging_a_recipe_preserves_an_unowned_sidecar_property() {
    let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:xmp="http://ns.adobe.com/xap/1.0/" xmp:CreatorTool="synthetic"/></rdf:RDF></x:xmpmeta>"#;
    let recipe = EditRecipe { exposure_ev: 0.5, ..EditRecipe::default() };

    let merged = autoshade::xmp::merge_recipe_into_xmp(existing, &recipe).unwrap();

    assert!(merged.doc.contains("xmp:CreatorTool=\"synthetic\""));
}

#[test]
fn merging_the_same_recipe_twice_is_byte_stable() {
    let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="0"/></rdf:RDF></x:xmpmeta>"#;
    let recipe = EditRecipe { exposure_ev: 0.5, ..EditRecipe::default() };

    let first = autoshade::xmp::merge_recipe_into_xmp(existing, &recipe).unwrap();
    let second = autoshade::xmp::merge_recipe_into_xmp(&first.doc, &recipe).unwrap();

    assert_eq!(second.doc, first.doc);
}

#[test]
fn a_merged_recipe_reads_back_with_its_exposure() {
    let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:Exposure2012="0"/></rdf:RDF></x:xmpmeta>"#;
    let recipe = EditRecipe { exposure_ev: 0.5, ..EditRecipe::default() };

    let merged = autoshade::xmp::merge_recipe_into_xmp(existing, &recipe).unwrap();
    let restored = autoshade::xmp::xmp_to_recipe(&merged.doc);

    assert!((restored.exposure_ev - 0.5).abs() < 0.011);
}

#[test]
fn an_external_brush_group_preserves_its_strokes_and_dab_stream() {
    let dabs = "r 0.25\nf 0.7\nh 0.5\nd 0.2 0.3\nd 0.4 0.5".to_string();
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::Brush {
                name: "brush group".to_string(),
                blend_mode: 0,
                value: 1.0,
                inverted: false,
                strokes: vec![BrushStroke {
                    value: 0.8,
                    radius: 0.25,
                    flow: 0.7,
                    center_weight: 0.5,
                    sync_id: "source-sync-id".to_string(),
                    dabs: dabs.clone(),
                }],
            },
            name: "painted sky".to_string(),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    let projected = autoshade::xmp::recipe_to_xmp(&recipe);
    let external = projected.replacen("asr:Payload=\"1\"", "asr:Payload=\"999\"", 1);
    assert_ne!(external, projected, "the fixture must disable the private payload");

    let restored = autoshade::xmp::xmp_to_recipe(&external);

    let MaskGeometry::Brush { name, strokes, .. } = &restored.masks[0].mask else {
        panic!("the public XMP projection must remain a brush group");
    };
    assert_eq!(name, "brush group");
    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].dabs, dabs);
    assert!((strokes[0].value - 0.8).abs() < 1e-6);
    assert!((strokes[0].radius - 0.25).abs() < 1e-6);
    assert!((strokes[0].flow - 0.7).abs() < 1e-6);
    assert!((strokes[0].center_weight - 0.5).abs() < 1e-6);
}

#[test]
fn an_external_ai_mask_preserves_its_reference_provenance_and_gesture() {
    let dabs = "r 0.12\nf 1\nh 0.9\nd 0.6 0.4".to_string();
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            mask: MaskGeometry::AiMask {
                name: "subject".to_string(),
                subtype: 1,
                ref_x: 0.35,
                ref_y: 0.65,
                blend_mode: 0,
                value: 1.0,
                inverted: false,
                mask_version: 2,
                provenance: vec![
                    ("ModelVersion".to_string(), "synthetic-model".to_string()),
                    ("Origin".to_string(), "synthetic-origin".to_string()),
                ],
                gesture: vec![BrushStroke {
                    value: 1.0,
                    radius: 0.12,
                    flow: 1.0,
                    center_weight: 0.9,
                    sync_id: "gesture-sync".to_string(),
                    dabs: dabs.clone(),
                }],
                raster: None,
            },
            name: "selected subject".to_string(),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    let projected = autoshade::xmp::recipe_to_xmp(&recipe);
    let external = projected.replacen("asr:Payload=\"1\"", "asr:Payload=\"999\"", 1);
    assert_ne!(external, projected, "the fixture must disable the private payload");

    let restored = autoshade::xmp::xmp_to_recipe(&external);

    let MaskGeometry::AiMask {
        name,
        subtype,
        ref_x,
        ref_y,
        mask_version,
        provenance,
        gesture,
        ..
    } = &restored.masks[0].mask
    else {
        panic!("the public XMP projection must remain an AI mask");
    };
    assert_eq!(name, "subject");
    assert_eq!(*subtype, 1);
    assert!((*ref_x - 0.35).abs() < 1e-6);
    assert!((*ref_y - 0.65).abs() < 1e-6);
    assert_eq!(*mask_version, 2);
    assert!(provenance.contains(&("ModelVersion".to_string(), "synthetic-model".to_string())));
    assert!(provenance.contains(&("Origin".to_string(), "synthetic-origin".to_string())));
    assert_eq!(gesture.len(), 1);
    assert_eq!(gesture[0].dabs, dabs);
}

#[test]
fn an_aggregate_brush_table_imports_its_compressed_strokes() {
    let tree = TempTree::new("xmp-brush-table");
    let photo = tree.path().join("photo.dng");
    let compressed = base64::engine::general_purpose::STANDARD
        .decode("H2QAAGTw5t4FGQ3w9izCvnCADyiQiAPMKQesrbW2JFDJz3DwGLZDucVzKPwAkJ1BPK8hplyqumm7cZqXdetOUBlwfBgMrwL+4eBHgNqstxj8NwFsd2wG")
        .unwrap();
    let mut blob = Vec::new();
    for word in [4u32, 1, 64_000, compressed.len() as u32] {
        blob.extend_from_slice(&word.to_le_bytes());
    }
    blob.extend_from_slice(&compressed);
    let key = md5::compute(&blob).0;
    let mut acr = Vec::new();
    acr.extend_from_slice(b"ACR\0");
    acr.extend_from_slice(&1u32.to_le_bytes());
    acr.extend_from_slice(b"ARW\0");
    acr.extend_from_slice(&1u32.to_le_bytes());
    acr.extend_from_slice(&0u32.to_le_bytes());
    acr.extend_from_slice(&key);
    acr.extend_from_slice(&(blob.len() as u64).to_le_bytes());
    acr.extend_from_slice(&52u64.to_le_bytes());
    assert_eq!(acr.len(), 52);
    acr.extend_from_slice(&blob);
    while acr.len() % 4 != 0 {
        acr.push(0);
    }
    std::fs::write(photo.with_extension("acr"), acr).unwrap();
    let token = key.iter().map(|byte| format!("{byte:02x}")).collect::<String>();
    let xmp = format!(
        r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="http://ns.adobe.com/camera-raw-settings/1.0/" crs:CorrectionName="table brush" crs:CorrectionAmount="1"><crs:MaskGroupBasedCorrections><rdf:Seq><rdf:li><rdf:Description crs:What="Correction" crs:CorrectionAmount="1" crs:CorrectionActive="true" crs:CorrectionName="table brush"><crs:CorrectionMasks><rdf:Seq><rdf:li><rdf:Description crs:What="Mask/Aggregate" crs:MaskActive="true" crs:MaskName="brush group" crs:MaskBlendMode="0" crs:MaskValue="1" crs:MaskBrushTable="{token}" crs:MaskBrushUncompressedBytes="101"/></rdf:li></rdf:Seq></crs:CorrectionMasks></rdf:Description></rdf:li></rdf:Seq></crs:MaskGroupBasedCorrections></rdf:Description></rdf:RDF></x:xmpmeta>"#
    );

    let restored = autoshade::xmp::xmp_to_recipe_for_photo(&xmp, &photo);

    let MaskGeometry::Brush { name, strokes, .. } = &restored.masks[0].mask else {
        panic!("the compressed table must import as a brush group");
    };
    assert_eq!(name, "brush group");
    assert_eq!(strokes.len(), 1);
    assert_eq!(strokes[0].sync_id, "0123456789abcdef0123456789abcdef");
    assert_eq!(strokes[0].dabs, "r 0.25\nd 0.2 0.3\nd 0.4 0.5");
    assert!((strokes[0].value - 0.8).abs() < 1e-6);
    assert!((strokes[0].radius - 0.25).abs() < 1e-6);
    assert!((strokes[0].flow - 0.7).abs() < 1e-6);
    assert!((strokes[0].center_weight - 0.5).abs() < 1e-6);
}

#[test]
fn merging_refuses_a_document_with_a_foreign_crs_namespace() {
    let existing = r#"<x:xmpmeta xmlns:x="adobe:ns:meta/"><rdf:RDF xmlns:rdf="http://www.w3.org/1999/02/22-rdf-syntax-ns#"><rdf:Description xmlns:crs="urn:not-camera-raw" crs:Exposure2012="0"/></rdf:RDF></x:xmpmeta>"#;

    assert!(autoshade::xmp::merge_recipe_into_xmp(existing, &EditRecipe::default()).is_none());
}

#[test]
fn an_empty_mask_warp_is_the_identity() {
    assert_eq!(autoshade::render::mask_warp_factor(&[], 0.73), 1.0);
}

#[test]
fn a_solved_mask_warp_round_trips_through_its_inverse() {
    let profile = LensProfile {
        mask_warp: vec![1.02, 1.01, 1.0],
        ..LensProfile::default()
    };
    let point = (0.8, 0.2);

    let warped = autoshade::render::lr_mask_warp_norm(point.0, point.1, (100.0, 50.0), &profile);
    let restored = autoshade::render::lr_mask_unwarp_norm(warped.0, warped.1, (100.0, 50.0), &profile);

    assert!((restored.0 - point.0).abs() < 1e-4);
    assert!((restored.1 - point.1).abs() < 1e-4);
}

#[test]
fn expanding_a_single_mask_pixel_makes_a_three_by_three_block() {
    let mut mask = GrayImage::from_pixel(5, 5, Luma([0]));
    mask.put_pixel(2, 2, Luma([255]));

    let expanded = autoshade::render::morph_mask(&mask, 1);

    for y in 1..=3 {
        for x in 1..=3 {
            assert_eq!(expanded.get_pixel(x, y).0[0], 255);
        }
    }
}

#[test]
fn contracting_a_three_by_three_mask_block_leaves_only_its_center() {
    let mut mask = GrayImage::from_pixel(5, 5, Luma([0]));
    for y in 1..=3 {
        for x in 1..=3 {
            mask.put_pixel(x, y, Luma([255]));
        }
    }

    let contracted = autoshade::render::morph_mask(&mask, -1);

    assert_eq!(contracted.get_pixel(2, 2).0[0], 255);
    assert_eq!(contracted.get_pixel(1, 2).0[0], 0);
}

#[test]
fn feathering_a_hard_mask_edge_produces_intermediate_coverage() {
    let mut mask = GrayImage::from_pixel(9, 9, Luma([0]));
    for y in 0..9 {
        for x in 4..9 {
            mask.put_pixel(x, y, Luma([255]));
        }
    }

    let feathered = autoshade::render::feather_mask(&mask, 1.5);

    assert!(feathered.pixels().any(|pixel| (1..255).contains(&pixel.0[0])));
}

#[test]
fn thumbnail_keeps_the_frame_aspect_inside_the_requested_box() {
    let source = DynamicImage::new_rgb8(8, 4);

    let thumbnail = autoshade::render::thumbnail(&source, 4, 4);

    assert_eq!((thumbnail.width(), thumbnail.height()), (4, 2));
}

#[test]
fn rotating_an_rgba_overlay_by_zero_preserves_its_alpha() {
    let source = image::RgbaImage::from_pixel(2, 2, image::Rgba([10, 20, 30, 40]));

    let rotated = autoshade::render::rotate_straighten_rgba(&source, 0.0);

    assert_eq!(rotated, source);
}

#[test]
fn a_bitmap_geometry_reports_a_raster_path_and_raster_backing() {
    let geometry = MaskGeometry::Bitmap { path: "mask.png".to_string() };

    assert_eq!(autoshade::render::geometry_raster_path(&geometry), Some("mask.png"));
    assert!(autoshade::render::is_raster_backed(&geometry));
}

#[test]
fn a_radial_geometry_reports_no_raster_path_or_raster_backing() {
    let geometry = MaskGeometry::Radial {
        top: 0.1,
        left: 0.1,
        bottom: 0.9,
        right: 0.9,
        feather: 0.2,
        roundness: 0.0,
        flipped: false,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    };

    assert_eq!(autoshade::render::geometry_raster_path(&geometry), None);
    assert!(!autoshade::render::is_raster_backed(&geometry));
}

#[test]
fn a_typed_direction_implicitly_selects_the_direction_guide() {
    let guide = autoshade::pipeline::Guide::resolve(None, Some("warmer shadows")).unwrap();

    assert_eq!(guide, autoshade::pipeline::Guide::Direction);
}

#[test]
fn an_explicit_library_guide_with_a_direction_is_refused() {
    assert!(
        autoshade::pipeline::Guide::resolve(
            Some(autoshade::pipeline::Guide::Library),
            Some("warmer shadows")
        )
        .is_err()
    );
}

#[test]
fn an_empty_region_direction_uses_the_neutral_local_improvement() {
    let guidance = autoshade::pipeline::region_guidance(0.1, 0.2, 0.7, 0.8, "  ");

    assert!(guidance.contains("tasteful local improvement"));
    assert!(guidance.contains("left=0.100"));
}

#[test]
fn a_deepseek_key_is_shared_with_the_other_deepseek_role_when_it_has_no_key() {
    let filled = autoshade::config::fill_deepseek_key(
        Some(autoshade::config::DEEPSEEK_BASE),
        Some(autoshade::config::DEEPSEEK_BASE),
        "synthetic-key",
        false,
    );

    assert_eq!(
        filled,
        Some((
            "synthetic-key".to_string(),
            Some(autoshade::config::DEEPSEEK_BASE.to_string())
        ))
    );
}

#[test]
fn a_deepseek_key_is_not_shared_when_the_other_role_already_has_one() {
    assert!(
        autoshade::config::fill_deepseek_key(
            Some(autoshade::config::DEEPSEEK_BASE),
            Some(autoshade::config::DEEPSEEK_BASE),
            "synthetic-key",
            true,
        )
        .is_none()
    );
}

#[test]
fn a_deepseek_key_is_not_shared_with_a_non_deepseek_role() {
    assert!(
        autoshade::config::fill_deepseek_key(
            Some("https://api.openai.com/v1"),
            Some(autoshade::config::DEEPSEEK_BASE),
            "synthetic-key",
            false,
        )
        .is_none()
    );
}

#[test]
fn an_empty_deepseek_key_is_not_shared() {
    assert!(
        autoshade::config::fill_deepseek_key(
            Some(autoshade::config::DEEPSEEK_BASE),
            Some(autoshade::config::DEEPSEEK_BASE),
            "",
            false,
        )
        .is_none()
    );
}
