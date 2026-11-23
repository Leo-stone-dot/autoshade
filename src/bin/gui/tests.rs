use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use autoshade::advisor::{Decision, Verdict};
use autoshade::recipe::{Crop, CurvePoint, EditRecipe, LocalAdjustment, MaskGeometry, RangeMask};

use crate::logic::Effect;
use crate::quit::{QuitState, TerminateReply};
use crate::*;

mod support;

fn recipe(exposure_ev: f32) -> EditRecipe {
    EditRecipe {
        exposure_ev,
        ..EditRecipe::default()
    }
}

fn preview() -> Arc<image::DynamicImage> {
    Arc::new(image::DynamicImage::new_rgb8(2, 2))
}

static QUIT_TEST_LOCK: Mutex<()> = Mutex::new(());

#[test]
fn a_dirty_quit_request_cancels_and_arms_the_in_app_close_prompt() {
    let _guard = QUIT_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = quit::take_close_request();
    quit::publish(QuitState::Dirty);

    assert_eq!(quit::on_terminate_request(), TerminateReply::Cancel);
    assert!(quit::take_close_request());
    quit::publish(QuitState::Clean);
}

#[test]
fn a_clean_quit_request_terminates_without_arming_the_close_prompt() {
    let _guard = QUIT_TEST_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let _ = quit::take_close_request();
    quit::publish(QuitState::Clean);

    assert_eq!(quit::on_terminate_request(), TerminateReply::Now);
    assert!(!quit::take_close_request());
}

#[cfg(target_os = "macos")]
#[test]
fn installing_the_macos_quit_guard_without_a_live_delegate_reports_a_safe_refusal() {
    assert!(macos::install_quit_guard().is_err());
}

#[test]
fn the_panel_title_renders_larger_than_group_captions_with_a_short_rule() {
    fn visit(
        shape: &egui::Shape,
        text_sizes: &mut Vec<(String, f32)>,
        short_rule: &mut bool,
    ) {
        match shape {
            egui::Shape::Text(text) => {
                let size = text
                    .galley
                    .job
                    .sections
                    .first()
                    .map_or(0.0, |section| section.format.font_id.size);
                text_sizes.push((text.galley.job.text.clone(), size));
            }
            egui::Shape::LineSegment { points, .. } => {
                if ((points[0].x - points[1].x).abs() - 28.0).abs() < 0.1
                    && (points[0].y - points[1].y).abs() < 0.1
                {
                    *short_rule = true;
                }
            }
            egui::Shape::Vec(shapes) => {
                for shape in shapes {
                    visit(shape, text_sizes, short_rule);
                }
            }
            _ => {}
        }
    }

    let output = egui::Context::default().run(egui::RawInput::default(), |ctx| {
        egui::CentralPanel::default().show(ctx, |ui| {
            panel_heading(ui, "AI", false, |_| {});
            group_caption(ui, "Group");
        });
    });
    let mut text_sizes = Vec::new();
    let mut short_rule = false;
    for clipped in &output.shapes {
        visit(&clipped.shape, &mut text_sizes, &mut short_rule);
    }
    let heading = text_sizes
        .iter()
        .find_map(|(text, size)| (text == "AI").then_some(*size))
        .expect("the panel title must be rendered");
    let group = text_sizes
        .iter()
        .find_map(|(text, size)| (text == "Group").then_some(*size))
        .expect("the group caption must be rendered");

    assert!(heading > group);
    assert!(short_rule);
}

#[test]
fn pressing_f1_opens_the_keyboard_shortcuts_sheet() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    let mut harness = support::headless_app();
    assert!(
        harness
            .query_by_role_and_label(Role::Window, "⌨ Shortcuts")
            .is_none()
    );

    harness.key_press(egui::Key::F1);
    harness.run();

    harness.get_by_role_and_label(Role::Window, "⌨ Shortcuts");
}

#[test]
fn dropping_a_photo_opens_the_editing_canvas() {
    use egui_kittest::kittest::Queryable;

    let tree = support::TempTree::new("headless-open-photo");
    let photo = tree.path().join("photo.png");
    image::RgbImage::from_fn(32, 24, |x, y| {
        image::Rgb([(x * 7) as u8, (y * 9) as u8, ((x + y) * 4) as u8])
    })
    .save(&photo)
    .unwrap();
    let mut harness = support::headless_app();
    assert!(
        harness
            .query_all_by_value("No photo open.")
            .next()
            .is_some()
    );
    support::drop_file(&mut harness, &photo);

    assert!(
        harness
            .query_all_by_value("No photo open.")
            .next()
            .is_none()
    );
    assert!(
        harness
            .query_all_by_value("Before (source)")
            .next()
            .is_some(),
        "the opened photo did not reach the visible editing canvas"
    );
}

#[test]
fn rotating_and_saving_persists_the_develop_without_changing_the_photo() {
    use egui::accesskit::Role;
    use egui_kittest::kittest::Queryable;

    let tree = support::TempTree::new("headless-edit-save");
    let photo = tree.path().join("photo.png");
    image::RgbImage::from_fn(32, 24, |x, y| {
        image::Rgb([(x * 7) as u8, (y * 9) as u8, ((x + y) * 4) as u8])
    })
    .save(&photo)
    .unwrap();
    let source_before = std::fs::read(&photo).unwrap();
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path().join("store"));
    let mut harness = support::headless_app();
    support::drop_file(&mut harness, &photo);
    harness
        .get_by_role_and_label(Role::Button, "⭮ Turn right")
        .click();
    harness
        .try_run_realtime()
        .expect("the bounded headless app must settle after a quarter turn");

    harness.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::S);
    harness.step();
    harness
        .try_run_realtime()
        .expect("the bounded headless app must settle after saving a tiny photo");

    let saved: EditRecipe = serde_json::from_slice(
        &std::fs::read(autoshade::store::recipe_target(&photo))
            .expect("Ctrl+S must publish the develop recipe"),
    )
    .unwrap();
    assert_eq!(saved.quarter_turns, 1);
    assert_eq!(std::fs::read(&photo).unwrap(), source_before);
}

fn paste_outcome(errs: Vec<String>) -> PasteOutcome {
    PasteOutcome {
        ok: 2,
        xmp: 2,
        errs,
        xmp_fails: Vec::new(),
        xmp_notes: Vec::new(),
        written: vec![PathBuf::from("first.jpg"), PathBuf::from("second.jpg")],
    }
}

fn radial_mask(feather: f32, angle: f32) -> MaskGeometry {
    MaskGeometry::Radial {
        top: 0.2,
        left: 0.2,
        bottom: 0.8,
        right: 0.8,
        feather,
        roundness: 0.0,
        flipped: false,
        angle,
        midpoint: 50.0,
        mask_version: 2,
    }
}

#[test]
fn a_superseded_analysis_never_replaces_the_current_recipe() {
    let verdict = Verdict {
        decision: Decision::Accept,
        reasons: Vec::new(),
        revised_hint: None,
    };
    let landed = logic::analysis_workflow::land(
        7,
        8,
        Ok((recipe(2.0), verdict, Vec::new())),
        false,
        false,
        Some(PathBuf::from("photo.dng")),
    );

    assert!(matches!(
        landed,
        logic::analysis_workflow::AnalysisCommand::Ignore
    ));
}

#[test]
fn an_accepted_analysis_installs_the_recipe_verdict_and_photo() {
    let verdict = Verdict {
        decision: Decision::Accept,
        reasons: Vec::new(),
        revised_hint: None,
    };
    let photo = PathBuf::from("photo.dng");
    let landed = logic::analysis_workflow::land(
        8,
        8,
        Ok((recipe(2.0), verdict, Vec::new())),
        false,
        false,
        Some(photo.clone()),
    );

    match landed {
        logic::analysis_workflow::AnalysisCommand::Install(install) => {
            assert_eq!(install.canvas.exposure_ev, 2.0);
            assert!(install.accepted);
            assert_eq!(install.photo, Some(photo));
        }
        _ => panic!("a current successful analysis must install"),
    }
}

#[test]
fn an_export_batch_with_one_failed_target_uses_the_error_channel() {
    let landed = export_workflow::land(
        Lang::En,
        Ok(ExportOutcome::Batch {
            ok: 1,
            errs: vec!["second.jpg: permission denied".to_string()],
            renamed: Vec::new(),
            relooked: 0,
            warns: Vec::new(),
            dest: PathBuf::from("out"),
        }),
    );

    match landed {
        export_workflow::ExportLanding::Batch { ok, error, .. } => {
            assert_eq!(ok, 1);
            assert!(error);
        }
        _ => panic!("a partial batch must keep its batch facts"),
    }
}

#[test]
fn a_paste_that_wrote_some_targets_but_failed_others_is_partial() {
    let landed = paste_workflow::land(Ok(paste_outcome(vec![
        "second.jpg: write failed".to_string(),
    ])));

    assert!(matches!(landed, paste_workflow::PasteLanding::Partial(_)));
}

#[test]
fn a_stale_retouch_result_discards_the_master_it_saved() {
    let saved = PathBuf::from("stale-master.png");
    let done = Ok((
        image::DynamicImage::new_rgb8(2, 2),
        RetouchNote::Cloned {
            n: 1,
            out: saved.clone(),
        },
        saved.clone(),
        RetouchKind::InPlace,
    ));

    let landed = retouch_workflow::land(3, 4, done, &EditRecipe::default());

    match landed {
        retouch_workflow::RetouchLanding::Ignore { discard } => assert_eq!(discard, Some(saved)),
        _ => panic!("a stale worker must not install its pixels"),
    }
}

#[test]
fn an_hdr_stack_landing_keeps_the_recovered_headroom() {
    let done = Ok((
        image::DynamicImage::new_rgb8(2, 2),
        RetouchNote::Stacked {
            out: PathBuf::from("stacked.png"),
            kind: autoshade::stack::merge::StackKind::Hdr,
            frames: 3,
            travel: 0.0,
            uncovered: 0.0,
            headroom_ev: 2.5,
        },
        PathBuf::from("stacked.png"),
        RetouchKind::NewStacked,
    ));

    let landed = retouch_workflow::land(1, 1, done, &EditRecipe::default());

    match landed {
        retouch_workflow::RetouchLanding::NewVariant { kind, recipe, .. } => {
            assert_eq!(kind, VariantKind::Stacked);
            assert!(recipe.hdr_edit);
            assert_eq!(recipe.hdr_max_ev, 2.5);
        }
        _ => panic!("an HDR stack must land as a new stacked card"),
    }
}

#[test]
fn a_valid_mask_insert_lands_after_the_requested_mask() {
    let mut recipe = EditRecipe::default();
    for name in ["a", "b", "c"] {
        recipe.masks.push(autoshade::recipe::LocalAdjustment {
            name: name.to_string(),
            ..Default::default()
        });
    }
    let inserted = autoshade::recipe::LocalAdjustment {
        name: "copy".to_string(),
        ..Default::default()
    };

    logic::command::apply(
        &mut recipe,
        logic::command::Command::InsertMask {
            after: 1,
            mask: Box::new(inserted),
        },
    );

    let names: Vec<_> = recipe.masks.iter().map(|m| m.name.trim()).collect();
    assert_eq!(names, ["a", "b", "copy", "c"]);
}

#[test]
fn deleting_a_mask_past_the_end_leaves_the_recipe_unchanged() {
    let mut recipe = EditRecipe::default();
    recipe
        .masks
        .push(autoshade::recipe::LocalAdjustment::default());

    logic::command::apply(
        &mut recipe,
        logic::command::Command::Mask(logic::command::MaskCommand::Delete { index: 9 }),
    );

    assert_eq!(recipe.masks.len(), 1);
}

#[test]
fn a_fit_shares_the_original_pixels_only_when_the_negative_matches() {
    let master = PathBuf::from("negative.png");
    let base = preview();
    let original = Variant {
        kind: VariantKind::Original,
        id: "original".to_string(),
        name: None,
        recipe: EditRecipe::default(),
        base: Some(base.clone()),
        origin: Some(master.clone()),
        thumb: None,
    };
    let outcome = |negative: Option<PathBuf>| FitOutcome {
        recipe: EditRecipe::default(),
        err_before: 0.3,
        err_after: 0.2,
        rationale_notes: Vec::new(),
        status: Vec::new(),
        persisted: true,
        negative,
    };

    let same = fit_workflow::land(Ok(outcome(Some(master.clone()))), Some(&original));
    let other = fit_workflow::land(
        Ok(outcome(Some(PathBuf::from("other.png")))),
        Some(&original),
    );

    match same {
        fit_workflow::FitLanding::Install {
            base: Some(shared), ..
        } => {
            assert!(Arc::ptr_eq(&base, &shared))
        }
        _ => panic!("the same negative should share the original base"),
    }
    assert!(matches!(
        other,
        fit_workflow::FitLanding::Install { base: None, .. }
    ));
}

#[test]
fn a_zoom_glide_converges_to_its_target_without_overshoot() {
    let mut zoom = 0.25;
    for _ in 0..200 {
        let next = logic::view::glide_step(zoom, 1.0, 1.0 / 60.0);
        assert!(next >= zoom);
        assert!(next <= 1.0);
        zoom = next;
    }

    assert_eq!(zoom, 1.0);
}

#[test]
fn an_in_flight_develop_does_not_start_another_worker() {
    let planned = develop_workflow::plan_dispatch(
        true,
        true,
        Some(preview()),
        &recipe(1.0),
        false,
        None,
        Ok(autoshade::render::SourceFrame::Raw),
    );

    assert!(matches!(planned, develop_workflow::DevelopDispatch::Skip));
}

#[test]
fn a_source_refusal_reaches_the_user_instead_of_starting_work() {
    let planned = develop_workflow::plan_dispatch(
        false,
        true,
        Some(preview()),
        &recipe(0.0),
        false,
        None,
        Err("this photo is turned".to_string()),
    );

    match planned {
        develop_workflow::DevelopDispatch::Fail(message) => {
            assert_eq!(message, "this photo is turned")
        }
        _ => panic!("a refused source must not be dispatched to a worker"),
    }
}

#[test]
fn a_busy_develop_is_reported_differently_from_a_real_io_failure() {
    let busy_key = "save postponed: this photo is being changed by another AutoShade process ({err}); your canvas remains unsaved — retry";
    let mut workspace = WorkspaceView::default();
    workspace.persist_postponed(
        Lang::En,
        &std::io::Error::from(std::io::ErrorKind::WouldBlock),
        busy_key,
        &[],
    );
    let busy = workspace.status();

    workspace.persist_postponed(
        Lang::En,
        &std::io::Error::other("disk full"),
        busy_key,
        &[],
    );
    let io = workspace.status();

    assert!(busy.contains("another AutoShade process"), "busy status={busy}");
    assert!(!io.contains("another AutoShade process"), "io status={io}");
    assert!(io.contains("disk full"), "io status={io}");
}

#[test]
fn a_ready_develop_carries_its_recipe_and_display_options_to_the_worker() {
    let base = preview();
    let planned = develop_workflow::plan_dispatch(
        false,
        true,
        Some(base.clone()),
        &recipe(1.25),
        true,
        Some(42),
        Ok(autoshade::render::SourceFrame::Raw),
    );

    match planned {
        develop_workflow::DevelopDispatch::Start {
            base: worker_base,
            recipe,
            show_clipping,
            film,
            source,
        } => {
            assert!(Arc::ptr_eq(&base, &worker_base));
            assert_eq!(recipe.exposure_ev, 1.25);
            assert!(show_clipping);
            assert_eq!(film, Some(42));
            assert!(source.is_raw());
        }
        _ => panic!("a ready develop must start one worker"),
    }
}

#[test]
fn a_single_export_with_a_mask_warning_uses_the_error_channel() {
    let landed = export_workflow::land(
        Lang::En,
        Ok(ExportOutcome::Single {
            out: PathBuf::from("finished.jpg"),
            relooked: false,
            warn: Some("mask missing".to_string()),
        }),
    );

    match landed {
        export_workflow::ExportLanding::Single { error, .. } => assert!(error),
        _ => panic!("a single export must land as a single delivery"),
    }
}

#[test]
fn a_failed_paste_is_not_installed_as_a_landing() {
    let landed = paste_workflow::land(Err(anyhow::anyhow!("selection disappeared")));

    assert!(matches!(landed, paste_workflow::PasteLanding::Failed(_)));
}

#[test]
fn a_fill_layer_lands_with_an_absolute_raster_path() {
    let saved = PathBuf::from("patch.png");
    let done = Ok((
        image::DynamicImage::new_rgb8(2, 2),
        RetouchNote::Cloned {
            n: 1,
            out: saved.clone(),
        },
        saved,
        RetouchKind::Layer,
    ));

    let landed = retouch_workflow::land(1, 1, done, &EditRecipe::default());

    match landed {
        retouch_workflow::RetouchLanding::Layer { path, .. } => assert!(path.is_absolute()),
        _ => panic!("a fill must land as a layer"),
    }
}

#[test]
fn a_heal_lands_in_place_on_the_active_card() {
    let done = Ok((
        image::DynamicImage::new_rgb8(2, 2),
        RetouchNote::Cloned {
            n: 1,
            out: PathBuf::from("heal.png"),
        },
        PathBuf::from("heal.png"),
        RetouchKind::InPlace,
    ));

    assert!(matches!(
        retouch_workflow::land(1, 1, done, &EditRecipe::default()),
        retouch_workflow::RetouchLanding::InPlace { .. },
    ));
}

#[test]
fn a_failed_fit_is_reported_as_a_failure() {
    let landed = fit_workflow::land(Err(anyhow::anyhow!("no reference")), None);

    assert!(matches!(landed, fit_workflow::FitLanding::Failed(_)));
}

#[test]
fn renaming_a_mask_past_the_end_leaves_the_recipe_unchanged() {
    let mut recipe = EditRecipe::default();
    recipe.masks.push(autoshade::recipe::LocalAdjustment {
        name: "keep".to_string(),
        ..Default::default()
    });

    logic::command::apply(
        &mut recipe,
        logic::command::Command::Mask(logic::command::MaskCommand::Rename {
            index: 9,
            name: "changed".to_string(),
        }),
    );

    assert_eq!(recipe.masks[0].name.trim(), "keep");
}

#[test]
fn inserting_a_curve_point_keeps_inputs_sorted_and_unique() {
    let mut points = vec![
        CurvePoint {
            input: 0,
            output: 0,
        },
        CurvePoint {
            input: 255,
            output: 255,
        },
    ];

    let at = logic::curve::insert_point(&mut points, 128, 64);
    let replaced = logic::curve::insert_point(&mut points, 128, 96);

    assert_eq!(at, 1);
    assert_eq!(replaced, 1);
    assert_eq!(
        points.iter().map(|p| p.input).collect::<Vec<_>>(),
        [0, 128, 255]
    );
    assert_eq!(points[1].output, 96);
}

#[test]
fn dragging_a_curve_point_stays_strictly_between_its_neighbours() {
    let mut points = vec![
        CurvePoint {
            input: 0,
            output: 0,
        },
        CurvePoint {
            input: 128,
            output: 128,
        },
        CurvePoint {
            input: 255,
            output: 255,
        },
    ];

    logic::curve::drag_point(&mut points, 1, 255, 200);

    assert_eq!(points[1].input, 254);
    assert_eq!(points[1].output, 200);
}

#[test]
fn a_slider_nudge_rounds_to_the_displayed_decimals_and_clamps() {
    assert_eq!(
        logic::slider::nudged(13.485, true, 1.0, 0, 0.0, 100.0),
        14.0
    );
    assert_eq!(logic::slider::nudged(99.9, true, 1.0, 0, 0.0, 100.0), 100.0);
}

#[test]
fn the_export_format_owns_the_extension_but_keeps_equivalent_spellings() {
    assert_eq!(
        logic::fields::normalize_export_target(PathBuf::from("photo.jpeg"), "jpg"),
        PathBuf::from("photo.jpeg"),
    );
    assert_eq!(
        logic::fields::normalize_export_target(PathBuf::from("photo.png"), "jpg"),
        PathBuf::from("photo.jpg"),
    );
}

#[test]
fn era_one_prefs_reset_raw_strength_and_preserve_baked_choices() {
    let migrated = logic::prefs::migrate_era(1, 0.25, 0.75);

    assert_eq!(
        migrated.raw_detail,
        autoshade::denoise::DEFAULT_STRENGTH_RAW
    );
    assert_eq!(
        migrated.raw_export,
        autoshade::denoise::DEFAULT_STRENGTH_RAW
    );
    assert_eq!(migrated.baked_detail, Some(0.25));
    assert_eq!(migrated.baked_export, Some(0.75));
    assert!(migrated.reset_note.is_some());
}

#[test]
fn an_lru_hit_becomes_most_recent_and_a_new_entry_evicts_the_oldest() {
    let mut entries = vec![("a", 1), ("b", 2)];
    let hit = logic::lru::get(&mut entries, |key| *key == "a");
    logic::lru::insert(&mut entries, "c", 3, |key| *key == "c", 2);

    assert_eq!(hit, Some(1));
    assert_eq!(entries, vec![("a", 1), ("c", 3)]);
}

#[test]
fn calibration_provenance_does_not_make_a_develop_dirty() {
    let canvas = EditRecipe {
        exposure_ev: 1.0,
        version: 2,
        ..EditRecipe::default()
    };
    let baseline = EditRecipe {
        exposure_ev: 1.0,
        version: 1,
        ..EditRecipe::default()
    };
    let changed = EditRecipe {
        exposure_ev: 1.5,
        version: 1,
        ..EditRecipe::default()
    };

    assert!(!logic::same::dirty_vs(&canvas, &baseline));
    assert!(logic::same::dirty_vs(&changed, &baseline));
}

#[test]
fn stamping_calibration_preserves_saved_user_values_and_fills_missing_ones() {
    let start = autoshade::pipeline::CanvasStart {
        base_curve: vec![[0.0, 0.0], [1.0, 1.0]],
    };
    let lens = autoshade::recipe::LensProfile {
        distortion_on: true,
        ..Default::default()
    };
    let mut saved = EditRecipe {
        as_shot_k: Some(4800.0),
        as_shot_tint: Some(7.0),
        ..EditRecipe::default()
    };
    let mut fresh = EditRecipe::default();

    persist::stamp_calibration(&mut saved, &start, &lens, Some((5500.0, 10.0)));
    persist::stamp_calibration(&mut fresh, &start, &lens, Some((5500.0, 10.0)));

    assert_eq!(saved.as_shot_k, Some(4800.0));
    assert_eq!(saved.as_shot_tint, Some(7.0));
    assert_eq!(saved.base_curve, start.base_curve);
    assert_eq!(saved.lens_profile, lens);
    assert_eq!(fresh.as_shot_k, Some(5500.0));
    assert_eq!(fresh.as_shot_tint, Some(10.0));
}

#[test]
fn opening_a_saved_recipe_restores_the_develop_without_a_loss_note() {
    let tree = support::TempTree::new("gui-saved-recipe");
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    let photo = tree.path().join("saved.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let dev = autoshade::store::develop_dir(&photo);
    std::fs::create_dir_all(&dev).unwrap();
    let saved = EditRecipe {
        exposure_ev: 1.25,
        ..EditRecipe::default()
    };
    std::fs::write(
        autoshade::store::recipe_target(&photo),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();

    let restored = persist::read_saved_develop(&photo);

    match restored.saved {
        SavedDevelop::Restored(recipe, kind) => {
            assert_eq!(kind, "recipe.json");
            assert_eq!(recipe.exposure_ev, saved.exposure_ev);
        }
        _ => panic!("a readable saved recipe must restore its develop"),
    }
    assert!(restored.xmp_bad.is_empty());
    assert!(restored.mask_import.is_empty());
    assert!(restored.carried_globals.is_empty());
    assert!(restored.clamp.is_empty());
}

#[test]
fn opening_a_corrupt_recipe_falls_back_to_xmp_and_reports_the_primary_loss() {
    let tree = support::TempTree::new("gui-corrupt-recipe");
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    let photo = tree.path().join("corrupt.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let dev = autoshade::store::develop_dir(&photo);
    std::fs::create_dir_all(&dev).unwrap();
    let fallback = EditRecipe {
        exposure_ev: 0.5,
        ..EditRecipe::default()
    };
    std::fs::write(
        autoshade::store::xmp_target(&photo),
        autoshade::xmp::recipe_to_xmp(&fallback),
    )
    .unwrap();
    std::fs::write(autoshade::store::recipe_target(&photo), b"{").unwrap();

    let restored = persist::read_saved_develop(&photo);

    match restored.saved {
        SavedDevelop::Unreadable { err, fallback: Some((recipe, kind)) } => {
            assert!(!err.is_empty());
            assert_eq!(kind, "XMP");
            assert!((recipe.exposure_ev - fallback.exposure_ev).abs() < 0.011);
        }
        _ => panic!("a corrupt recipe must disclose the loss while offering the XMP fallback"),
    }
}

#[test]
fn a_neutral_recipe_does_not_resurrect_an_older_xmp_develop() {
    let tree = support::TempTree::new("gui-neutral-recipe");
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    let photo = tree.path().join("neutral.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let dev = autoshade::store::develop_dir(&photo);
    std::fs::create_dir_all(&dev).unwrap();
    std::fs::write(
        autoshade::store::recipe_target(&photo),
        serde_json::to_vec(&EditRecipe::default()).unwrap(),
    )
    .unwrap();
    std::fs::write(
        autoshade::store::xmp_target(&photo),
        autoshade::xmp::recipe_to_xmp(&EditRecipe {
            exposure_ev: 1.5,
            ..EditRecipe::default()
        }),
    )
    .unwrap();

    let restored = persist::read_saved_develop(&photo);

    assert!(matches!(restored.saved, SavedDevelop::NoopOnly));
}

#[test]
fn an_unreadable_lightroom_sidecar_is_reported_while_the_store_recipe_still_opens() {
    let tree = support::TempTree::new("gui-unreadable-lr-sidecar");
    let mut env = support::ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    let photo = tree.path().join("sidecar.dng");
    std::fs::write(&photo, b"synthetic photo").unwrap();
    let dev = autoshade::store::develop_dir(&photo);
    std::fs::create_dir_all(&dev).unwrap();
    let saved = EditRecipe {
        exposure_ev: 0.25,
        ..EditRecipe::default()
    };
    std::fs::write(
        autoshade::store::recipe_target(&photo),
        serde_json::to_vec(&saved).unwrap(),
    )
    .unwrap();
    std::fs::create_dir_all(photo.with_extension("xmp")).unwrap();

    let restored = persist::read_saved_develop(&photo);

    assert!(matches!(
        restored.saved,
        SavedDevelop::Restored(ref recipe, "recipe.json") if recipe.exposure_ev == saved.exposure_ev
    ));
    assert!(restored.lr_unreadable.is_some());
}

#[test]
fn restoring_a_variant_record_preserves_background_cards_and_their_identity() {
    let record = autoshade::store::VariantsRecord {
        v: 1,
        active_kind: "original".to_string(),
        active_pos: 1,
        others: vec![
            autoshade::store::VariantEntry {
                kind: "generated".to_string(),
                recipe: EditRecipe::default(),
                origin: Some(PathBuf::from("generated.png")),
                id: Some("generated-card".to_string()),
                name: Some("Hero".to_string()),
                extra: Default::default(),
            },
            autoshade::store::VariantEntry {
                kind: "fitted".to_string(),
                recipe: EditRecipe {
                    exposure_ev: 0.75,
                    ..EditRecipe::default()
                },
                origin: None,
                id: Some("fitted-card".to_string()),
                name: Some("Match".to_string()),
                extra: Default::default(),
            },
        ],
        active_id: Some("original-card".to_string()),
        active_name: Some("Base".to_string()),
        extra: Default::default(),
    };

    let (strip, dropped) = persist::strip_from_record(&record, None);

    assert_eq!(strip.len(), 2);
    assert_eq!(strip[0].kind, VariantKind::Generated);
    assert_eq!(strip[0].id, "generated-card");
    assert_eq!(strip[0].name.as_deref(), Some("Hero"));
    assert_eq!(strip[0].origin, Some(PathBuf::from("generated.png")));
    assert_eq!(strip[1].kind, VariantKind::Fitted);
    assert_eq!(strip[1].id, "fitted-card");
    assert_eq!(strip[1].name.as_deref(), Some("Match"));
    assert_eq!(strip[1].recipe.exposure_ev, 0.75);
    assert!(dropped.is_empty());
}

#[test]
fn a_saved_variant_record_keeps_the_active_card_position_when_matching_live_cards() {
    let mut editor = Editor::default();
    editor.doc_mut().variants = vec![
        Variant {
            kind: VariantKind::Generated,
            id: String::new(),
            name: None,
            recipe: EditRecipe {
                exposure_ev: 1.0,
                ..EditRecipe::default()
            },
            base: None,
            origin: None,
            thumb: None,
        },
        Variant {
            kind: VariantKind::Original,
            id: String::new(),
            name: None,
            recipe: EditRecipe {
                exposure_ev: 2.0,
                ..EditRecipe::default()
            },
            base: None,
            origin: None,
            thumb: None,
        },
        Variant {
            kind: VariantKind::Fitted,
            id: String::new(),
            name: None,
            recipe: EditRecipe {
                exposure_ev: 3.0,
                ..EditRecipe::default()
            },
            base: None,
            origin: None,
            thumb: None,
        },
    ];
    editor.doc_mut().active = 1;
    editor.session_mut().hist.saved_recipe = EditRecipe {
        exposure_ev: 9.0,
        ..EditRecipe::default()
    };
    editor.session_mut().hist.saved_strip = Some(autoshade::store::VariantsRecord {
        v: 1,
        active_kind: "original".to_string(),
        active_pos: 1,
        others: vec![
            autoshade::store::VariantEntry {
                kind: "generated".to_string(),
                recipe: EditRecipe {
                    exposure_ev: 0.1,
                    ..EditRecipe::default()
                },
                origin: None,
                id: None,
                name: None,
                extra: Default::default(),
            },
            autoshade::store::VariantEntry {
                kind: "fitted".to_string(),
                recipe: EditRecipe {
                    exposure_ev: 0.3,
                    ..EditRecipe::default()
                },
                origin: None,
                id: None,
                name: None,
                extra: Default::default(),
            },
        ],
        active_id: None,
        active_name: None,
        extra: Default::default(),
    });

    assert_eq!(editor.persisted_card_for(1).unwrap().recipe.exposure_ev, 9.0);
    assert_eq!(editor.persisted_card_for(0).unwrap().recipe.exposure_ev, 0.1);
    assert_eq!(editor.persisted_card_for(2).unwrap().recipe.exposure_ev, 0.3);
}

#[test]
fn restoring_an_oversized_variant_recipe_reports_the_cards_dropped_by_clamping() {
    let recipe = EditRecipe {
        masks: (0..65).map(|_| LocalAdjustment::default()).collect(),
        ..EditRecipe::default()
    };
    let record = autoshade::store::VariantsRecord {
        v: 1,
        active_kind: "original".to_string(),
        active_pos: 0,
        others: vec![autoshade::store::VariantEntry {
            kind: "generated".to_string(),
            recipe,
            origin: None,
            id: Some("oversized-card".to_string()),
            name: None,
            extra: Default::default(),
        }],
        active_id: None,
        active_name: None,
        extra: Default::default(),
    };

    let (strip, dropped) = persist::strip_from_record(&record, None);

    assert_eq!(strip[0].recipe.masks.len(), 64);
    assert_eq!(dropped.dropped_masks, 1);
}

#[test]
fn an_ai_analysis_strips_baked_start_on_canvas_but_keeps_it_in_the_store_recipe() {
    let source = EditRecipe {
        base_curve: vec![[0.0, 0.0], [1.0, 1.0]],
        camera_profile: "camera".to_string(),
        ..EditRecipe::default()
    };

    let shaped = logic::analysis_landing::shape(source.clone(), false, true);

    assert!(shaped.canvas.base_curve.is_empty());
    assert!(shaped.canvas.camera_profile.is_empty());
    assert_eq!(shaped.stamped, source);
}

#[test]
fn a_modified_gallery_click_toggles_selection_without_duplicates() {
    let mut selection = std::collections::HashSet::new();
    let click = logic::gallery::click(2, true);

    logic::gallery::toggle(&mut selection, 2);
    logic::gallery::toggle(&mut selection, 2);

    assert_eq!(click, logic::gallery::Click::Toggle(2));
    assert!(selection.is_empty());
}

#[test]
fn gallery_trim_keeps_the_visible_rows_and_their_margin() {
    let window = logic::gallery::trim_window(900..950, 2_000, 100);

    assert_eq!(window, Some(300..1_550));
}

#[test]
fn moving_a_list_item_remaps_the_selected_index() {
    let (moved_to, remap) = logic::list::reorder_move(2, 0);

    assert_eq!(moved_to, 0);
    assert_eq!(remap(1), 2);
}

#[test]
fn relative_and_absolute_spellings_of_one_master_match() {
    let relative = std::path::Path::new("photo.png");
    let absolute = std::path::absolute(relative).unwrap();

    assert!(logic::same::same_master(relative, &absolute));
}

#[test]
fn a_crop_preset_installs_a_visible_crop_box() {
    let mut recipe = EditRecipe::default();

    logic::crop::apply(
        &mut recipe,
        logic::crop::CropAction::Preset {
            current: None,
            aspect_px: Some(1.0),
            tex: (100.0, 100.0),
        },
    );

    assert!(recipe.crop.is_some());
}

#[test]
fn a_crop_preset_matches_the_requested_pixel_aspect() {
    let mut recipe = EditRecipe::default();

    logic::crop::apply(
        &mut recipe,
        logic::crop::CropAction::Preset {
            current: None,
            aspect_px: Some(2.0),
            tex: (100.0, 100.0),
        },
    );

    let crop = recipe.crop.unwrap();
    assert!((crop.right - crop.left - 1.0).abs() < 1e-6);
    assert!(((crop.right - crop.left) / (crop.bottom - crop.top) - 2.0).abs() < 1e-6);
}

#[test]
fn dragging_a_crop_move_past_the_edge_keeps_the_box_inside_the_frame() {
    let mut recipe = EditRecipe::default();

    logic::crop::apply(
        &mut recipe,
        logic::crop::CropAction::Drag {
            handle: 4,
            start: [0.2, 0.2, 0.4, 0.4],
            delta: (2.0, -2.0),
            aspect_px: None,
            tex: (100.0, 100.0),
        },
    );

    let crop = recipe.crop.unwrap();
    assert!((crop.left - 0.8).abs() < 1e-6);
    assert!(crop.top.abs() < 1e-6);
    assert!((crop.right - 1.0).abs() < 1e-6);
    assert!((crop.bottom - 0.2).abs() < 1e-6);
}

#[test]
fn straightening_wraps_a_small_delta_across_the_leftward_ray() {
    let mut recipe = EditRecipe::default();

    logic::crop::apply(
        &mut recipe,
        logic::crop::CropAction::Rotate {
            base_deg: 44.0,
            raw_delta_deg: 359.0,
        },
    );

    assert!((recipe.straighten_deg - 43.0).abs() < 1e-6);
}

#[test]
fn a_disabled_lens_blur_is_inactive() {
    let recipe = EditRecipe {
        lens_blur: Some(autoshade::render::LensBlur {
            depth_map: "depth.png".to_string(),
            enabled: false,
            ..Default::default()
        }),
        ..Default::default()
    };

    assert!(!logic::lens_blur::active(&recipe));
}

#[test]
fn picking_a_focus_point_clamps_it_and_disarms_the_picker() {
    let mut recipe = EditRecipe {
        lens_blur: Some(autoshade::render::LensBlur {
            depth_map: "depth.png".to_string(),
            ..Default::default()
        }),
        ..Default::default()
    };

    let effects = logic::lens_blur::apply(
        &mut recipe,
        logic::lens_blur::LensAction::FocusPicked([-0.5, 1.5]),
        None,
        false,
    );

    assert_eq!(recipe.lens_blur.unwrap().focus, [0.0, 1.0]);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ArmFocusPick(false)))
    );
}

#[test]
fn a_first_depth_estimate_arms_focus_picking() {
    let mut recipe = EditRecipe::default();

    let effects = logic::lens_blur::landed(&mut recipe, Ok(PathBuf::from("depth.png")));

    assert!(recipe.lens_blur.as_ref().unwrap().enabled);
    assert!(
        effects
            .iter()
            .any(|e| matches!(e, Effect::ArmFocusPick(true)))
    );
}

#[test]
fn a_depth_reestimate_keeps_the_users_focus_point() {
    let mut recipe = EditRecipe {
        lens_blur: Some(autoshade::render::LensBlur {
            depth_map: "old.png".to_string(),
            focus: [0.2, 0.3],
            ..Default::default()
        }),
        ..Default::default()
    };

    let effects = logic::lens_blur::landed(&mut recipe, Ok(PathBuf::from("new.png")));

    let blur = recipe.lens_blur.unwrap();
    assert_eq!(blur.focus, [0.2, 0.3]);
    assert_eq!(blur.depth_map, "new.png");
    assert!(
        !effects
            .iter()
            .any(|e| matches!(e, Effect::ArmFocusPick(true)))
    );
}

#[test]
fn a_baked_export_uses_the_baked_export_strength() {
    let strength = logic::noise::selected_strength(false, true, 0.1, 0.2, 0.3, 0.4);

    assert_eq!(strength, 0.4);
}

#[test]
fn a_range_reference_keeps_only_earlier_masks_and_strips_geometry() {
    let mut recipe = EditRecipe::default();
    for _ in 0..3 {
        recipe.masks.push(LocalAdjustment::default());
    }
    recipe.straighten_deg = 12.0;
    recipe.lens_distortion = 20.0;
    recipe.ca_r = 0.1;
    recipe.ca_b = -0.1;
    recipe.crop = Some(Crop {
        left: 0.1,
        top: 0.1,
        right: 0.9,
        bottom: 0.9,
    });

    let reference = logic::range::reference_recipe(&recipe, 2);

    assert_eq!(reference.masks.len(), 2);
    assert_eq!(reference.straighten_deg, 0.0);
    assert_eq!(reference.lens_distortion, 0.0);
    assert_eq!((reference.ca_r, reference.ca_b), (0.0, 0.0));
    assert_eq!(reference.crop, None);
}

#[test]
fn a_radial_edge_cannot_cross_the_opposite_edge() {
    let mut recipe = EditRecipe::default();
    recipe.masks.push(LocalAdjustment {
        mask: radial_mask(0.5, 0.0),
        ..Default::default()
    });

    logic::mask_edit::apply(
        &mut recipe,
        logic::mask_edit::MaskEditAction::Drag {
            mask: 0,
            component: None,
            handle: 3,
            last: (0.8, 0.5),
            cur: (-1.0, 0.5),
        },
    );

    let MaskGeometry::Radial { left, right, .. } = recipe.masks[0].mask else {
        panic!("the radial mask must stay radial");
    };
    assert!(right >= left + 0.01);
}

#[test]
fn redrawing_a_radial_mask_keeps_its_feather_and_angle() {
    let mut recipe = EditRecipe::default();
    recipe.masks.push(LocalAdjustment {
        mask: radial_mask(0.3, 15.0),
        ..Default::default()
    });

    logic::mask_place::commit(
        &mut recipe,
        logic::mask_place::Place::Redraw(0),
        radial_mask(0.7, -20.0),
        "unused".to_string(),
    );

    let MaskGeometry::Radial { feather, angle, .. } = recipe.masks[0].mask else {
        panic!("the redrawn mask must stay radial");
    };
    assert_eq!((feather, angle), (0.3, 15.0));
}

#[test]
fn a_refined_mask_repoints_the_unique_surviving_raster() {
    let mut recipe = EditRecipe::default();
    recipe.masks.push(LocalAdjustment {
        mask: MaskGeometry::Bitmap {
            path: "old.png".to_string(),
        },
        ..Default::default()
    });

    let refined = logic::mask_refine::apply(
        &mut recipe,
        &logic::mask_refine::Outcome::Refined {
            index: 0,
            stored_ref: "old.png".to_string(),
            out: PathBuf::from("new.png"),
            out_display: "new.png".to_string(),
        },
    );

    assert_eq!(refined.selection, Some(0));
    assert_eq!(
        autoshade::render::geometry_raster_path(&recipe.masks[0].mask),
        Some("new.png")
    );
}

#[test]
fn a_refined_mask_with_two_matching_rasters_is_not_applied() {
    let mut recipe = EditRecipe::default();
    recipe.masks.push(LocalAdjustment::default());
    for _ in 0..2 {
        recipe.masks.push(LocalAdjustment {
            mask: MaskGeometry::Bitmap {
                path: "old.png".to_string(),
            },
            ..Default::default()
        });
    }

    let refined = logic::mask_refine::apply(
        &mut recipe,
        &logic::mask_refine::Outcome::Refined {
            index: 0,
            stored_ref: "old.png".to_string(),
            out: PathBuf::from("new.png"),
            out_display: "new.png".to_string(),
        },
    );

    assert_eq!(refined.selection, None);
    assert_eq!(
        autoshade::render::geometry_raster_path(&recipe.masks[1].mask),
        Some("old.png")
    );
}

#[test]
fn a_range_color_sample_keeps_the_existing_tolerance() {
    let mut recipe = EditRecipe::default();
    recipe.masks.push(LocalAdjustment {
        range: Some(RangeMask::Color {
            r: 0.1,
            g: 0.2,
            b: 0.3,
            amount: 0.73,
            px: 0.0,
            py: 0.0,
        }),
        ..Default::default()
    });

    logic::pick::apply(
        &mut recipe,
        logic::pick::PickAction::RangeColor {
            mask: 0,
            rgb: [0.8, 0.4, 0.2],
            px: 0.25,
            py: 0.75,
        },
    );

    match recipe.masks[0].range {
        Some(RangeMask::Color {
            r, g, b, amount, ..
        }) => {
            assert_eq!([r, g, b], [0.8, 0.4, 0.2]);
            assert_eq!(amount, 0.73);
        }
        _ => panic!("the colour range must keep its type"),
    }
}

#[test]
fn a_stale_range_sample_only_disarms_the_picker() {
    let mut recipe = EditRecipe::default();

    let effects = logic::pick::apply(
        &mut recipe,
        logic::pick::PickAction::RangeColor {
            mask: 4,
            rgb: [0.8, 0.4, 0.2],
            px: 0.25,
            py: 0.75,
        },
    );

    assert_eq!(effects.len(), 1);
    assert!(matches!(
        effects[0],
        Effect::DisarmPicker(logic::pick::Picker::Range)
    ));
}

#[test]
fn a_point_color_sample_that_is_almost_grey_stays_armed() {
    let mut recipe = EditRecipe::default();

    let effects = logic::pick::apply(
        &mut recipe,
        logic::pick::PickAction::PointColor([0.5, 0.5, 0.5]),
    );

    assert!(recipe.point_colors.is_empty());
    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::Status(_)));
}

#[test]
fn a_tiny_region_drag_clears_the_selection() {
    let effects = logic::region::apply(logic::region::RegionAction::Select([
        (0.1, 0.1),
        (0.11, 0.1),
        (0.11, 0.11),
        (0.1, 0.11),
    ]));

    assert_eq!(effects.len(), 1);
    assert!(matches!(effects[0], Effect::SetRegion(None)));
}

#[test]
fn a_large_region_drag_reports_its_bounds_and_size() {
    let effects = logic::region::apply(logic::region::RegionAction::Select([
        (0.6, 0.7),
        (0.1, 0.2),
        (0.6, 0.2),
        (0.1, 0.7),
    ]));

    match &effects[0] {
        Effect::SetRegion(Some(bounds)) => {
            assert_eq!(*bounds, [0.1, 0.2, 0.6, 0.7]);
        }
        _ => panic!("a meaningful drag must set a region"),
    }
    match &effects[1] {
        Effect::Status(text) => {
            assert_eq!(
                text.key,
                K_LOGIC_REGION_TYPE_A_DIRECTION_THEN_AI_ANALYZE_CLICK_TO
            );
            assert!(text.args.iter().any(|(name, value)| {
                *name == "w" && matches!(value, logic::Arg::Text(v) if v == "50")
            }));
        }
        _ => panic!("a selected region must explain its size"),
    }
}

#[test]
fn a_segmentation_rerun_refreshes_the_same_mask_family() {
    let mut recipe = EditRecipe::default();
    recipe.masks.push(LocalAdjustment {
        mask: MaskGeometry::Bitmap {
            path: "photo.mask-sky-1.png".to_string(),
        },
        ..Default::default()
    });

    let landed = logic::segmented::apply(
        &mut recipe,
        "sky",
        std::path::Path::new("photo.mask-sky-2.png"),
        "BiRefNet",
        Some("photo"),
    );

    assert_eq!(landed.selection, 0);
    assert_eq!(recipe.masks.len(), 1);
    assert_eq!(
        autoshade::render::geometry_raster_path(&recipe.masks[0].mask),
        Some("photo.mask-sky-2.png")
    );
}

#[test]
fn a_fallback_segmentation_backend_raises_a_visible_error() {
    let mut recipe = EditRecipe::default();

    let landed = logic::segmented::apply(
        &mut recipe,
        "subject",
        std::path::Path::new("mask-subject.png"),
        "U2NET FALLBACK",
        None,
    );

    assert!(
        landed
            .effects
            .iter()
            .any(|effect| matches!(effect, Effect::ToastError(_)))
    );
}

#[test]
fn a_recorded_snapshot_id_takes_precedence_over_kind_and_position() {
    let meta = autoshade::store::VersionMetaEntry {
        n: 1,
        name: None,
        from_kind: Some("original".to_string()),
        from_id: Some("card-a".to_string()),
        origin: None,
    };

    assert!(!logic::version::is_from(
        Some(&meta),
        "card-b",
        Some("original")
    ));
    assert!(logic::version::is_from(Some(&meta), "", Some("original")));
}

#[test]
fn an_unsaved_background_card_counts_as_quit_dirty() {
    let variants = vec![
        Variant {
            kind: VariantKind::Original,
            id: "original".to_string(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        },
        Variant {
            kind: VariantKind::Generated,
            id: "generated".to_string(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        },
        Variant {
            kind: VariantKind::Generated,
            id: "generated-2".to_string(),
            name: None,
            recipe: EditRecipe::default(),
            base: None,
            origin: None,
            thumb: None,
        },
    ];

    assert_eq!(logic::strip::open_dirty_count(&variants, 0, None), 2);
}

#[test]
fn a_reference_look_uses_the_library_and_shows_reference_images() {
    let prefs = logic::look::LookPrefs {
        guide: autoshade::pipeline::Guide::Direction,
        style: 0.0,
        send_reference_image: false,
        strength: autoshade::recipe::GradeStrength::default(),
        think: false,
        adherence: autoshade::recipe::DirectionAdherence::default(),
        embed: autoshade::library::EmbeddingSwitch::OFF,
    };
    let started = logic::look::plan(
        &prefs,
        logic::look::LookIntent::of(logic::look::LookKind::OneClick(
            logic::look::OneClick::References,
        )),
        Some(std::path::Path::new("photo.dng")),
        false,
        false,
        9,
    )
    .unwrap();

    assert_eq!(started.job.req.guide, autoshade::pipeline::Guide::Library);
    assert_eq!(started.job.req.style, logic::look::REFERENCE_STYLE);
    assert!(started.job.req.send_reference_image);
    assert_eq!(started.job.library, autoshade::library::NORMAL_NAME);
}

#[test]
fn a_stale_look_result_does_not_claim_the_pending_job() {
    let mut state = logic::look::LookState {
        pending: Some(logic::look::PendingLook {
            kind: logic::look::LookKind::Analyze,
            photo: PathBuf::from("photo.dng"),
            epoch: 5,
            keep: logic::look::Keep::None,
        }),
    };
    let verdict = Verdict {
        decision: Decision::Accept,
        reasons: Vec::new(),
        revised_hint: None,
    };
    let result = Ok((recipe(1.0), verdict, Vec::new()));

    let claim = logic::look::claim(&mut state, 5, 4, &result);

    assert!(claim.is_none());
    assert!(state.pending.is_none());
}

#[test]
fn an_empty_folder_scan_uses_the_empty_state_wording() {
    let words = logic::folder::words(0, 0);

    assert_eq!(words.status.key, K_LOGIC_NO_PHOTOS_FOUND_IN_THIS_FOLDER);
    assert!(words.skipped.is_none());
}

#[test]
fn model_catalogue_lists_image_models_only_for_an_image_role() {
    let models = vec![
        autoshade::openai_models::ModelInfo::from("gpt-5"),
        autoshade::openai_models::ModelInfo::from("gpt-image-1"),
    ];

    let analysis = logic::models::catalogue(&models, false);
    let generation = logic::models::catalogue(&models, true);

    assert!(analysis.image.is_empty());
    assert_eq!(generation.image, vec!["gpt-image-1"]);
}

#[test]
fn mask_losses_interrupt_a_save_but_stamped_calibration_does_not() {
    let mask_loss = autoshade::xmp::MaskLoss {
        name: "mask".to_string(),
        reason: autoshade::xmp::MaskLossReason::Bitmap,
    };

    assert!(logic::xmp_disclosure::loss_interrupts(&[mask_loss], &[]));
    assert!(!logic::xmp_disclosure::loss_interrupts(
        &[],
        &["base_curve"]
    ));
    assert!(logic::xmp_disclosure::loss_interrupts(
        &[],
        &["color_field"]
    ));
}

#[test]
fn rotation_disclosures_deduplicate_and_cap_the_named_angles() {
    let rotations =
        logic::xmp_disclosure::rotation_list([0, 5, 5, -3, 7, 9, 11].into_iter()).unwrap();

    assert_eq!(rotations.shown, vec![5, -3, 7, 9]);
    assert_eq!(rotations.more, 1);
}

#[test]
fn reimagine_divergence_above_the_global_limit_is_a_warning() {
    let warning = logic::retouch::reimagine_fidelity(autoshade::fit::DIVERGENCE_GLOBAL + 0.1);
    let faithful = logic::retouch::reimagine_fidelity(autoshade::fit::DIVERGENCE_GLOBAL - 0.1);

    assert_eq!(
        warning.key,
        K_LOGIC_STRUCTURE_DIVERGED_FROM_THE_ORIGINAL_D_A_REVERSE
    );
    assert_eq!(
        faithful.key,
        K_LOGIC_FAITHFUL_TO_THE_FRAME_STRUCTURAL_DIVERGENCE_D
    );
}
