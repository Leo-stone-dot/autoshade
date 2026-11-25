use autoshade::advisor::catalogue::{global_value, Shape, RECIPE_CONTROLS};
use autoshade::jobs::plan_with_peak;
use autoshade::recipe::{EditRecipe, LensProfile};
use autoshade::render::{develop_preview, geometry_profile, CropPolicy, FilmScale};
use image::{DynamicImage, Rgb};

/// Every registry row whose tier renders nothing must move no pixel through
/// the develop, the composed lens geometry or the finishing pass. Derived from
/// the public registry so a row that changes tier without gaining an engine
/// stage fails here; `probed >= 1` keeps the sieve from passing over nothing.
#[test]
fn every_control_whose_tier_renders_nothing_moves_no_pixel() {
    let source = DynamicImage::ImageRgb16(image::ImageBuffer::<Rgb<u16>, Vec<u16>>::from_fn(64, 48, |x, y| {
        Rgb([(x * 900) as u16, (y * 1200) as u16, ((x + y) * 500) as u16])
    }));
    let neutral_recipe = EditRecipe::default();
    let neutral = develop_preview(&source, &neutral_recipe).to_rgb16();
    let tail = |recipe: &EditRecipe| {
        autoshade::render::frame_and_finish(
            source.clone(),
            recipe,
            &geometry_profile(recipe),
            FilmScale::NATIVE,
            CropPolicy::Cut,
        )
        .to_rgb16()
    };
    let neutral_tail = tail(&neutral_recipe);
    let mut probed = 0usize;

    for control in RECIPE_CONTROLS
        .iter()
        .filter(|control| control.tier.is_some_and(|tier| !tier.renders()))
    {
        let mut json = serde_json::to_value(&neutral_recipe).unwrap();
        json[control.name] = match control.shape {
            Shape::Bool => serde_json::json!(true),
            Shape::EngineCarrier => serde_json::json!({ "PerspectiveVertical": "-35" }),
            _ => serde_json::json!(control.range.map(|(_, hi)| hi).unwrap_or(1.0)),
        };
        let mut recipe: EditRecipe = serde_json::from_value(json)
            .unwrap_or_else(|error| panic!("{}: probe rejected: {error}", control.name));
        recipe.clamp();
        assert_ne!(
            global_value(&recipe, control.name),
            global_value(&neutral_recipe, control.name),
            "{}: the probe must move the control",
            control.name
        );

        let developed = develop_preview(&source, &recipe).to_rgb16();
        assert!(
            developed.pixels().zip(neutral.pixels()).all(|(p, q)| p == q),
            "{}: a non-rendering tier moved a develop pixel",
            control.name
        );
        assert_eq!(
            *geometry_profile(&recipe),
            *geometry_profile(&neutral_recipe),
            "{}: a non-rendering tier changed the composed lens geometry",
            control.name
        );
        let finished = tail(&recipe);
        assert!(
            finished.pixels().zip(neutral_tail.pixels()).all(|(p, q)| p == q),
            "{}: a non-rendering tier moved a finishing pixel",
            control.name
        );
        probed += 1;
    }

    assert!(probed >= 1, "the registry has no non-rendering row to sieve");
}

#[test]
fn worker_plan_stays_within_the_requested_and_work_limits() {
    for requested in [0, 1, 2, 4, 16] {
        for work in [0, 1, 2, 5, 100] {
            for headroom in [None, Some(0), Some(1), Some(100), Some(10_000)] {
                for peak in [0, 1, 1_800, 10_000] {
                    let plan = plan_with_peak(requested, work, headroom, peak);
                    let limit = requested.max(1).min(work.max(1));

                    assert!(
                        (1..=limit).contains(&plan.jobs),
                        "requested={requested} work={work} headroom={headroom:?} peak={peak}: {plan:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn worker_plan_discloses_a_memory_limit_when_it_reduces_the_worker_count() {
    for requested in [0, 1, 2, 4, 16] {
        for work in [0, 1, 2, 5, 100] {
            for headroom in [None, Some(0), Some(1), Some(100), Some(10_000)] {
                for peak in [0, 1, 1_800, 10_000] {
                    let plan = plan_with_peak(requested, work, headroom, peak);
                    let limit = requested.max(1).min(work.max(1));

                    assert_eq!(
                        plan.jobs < limit,
                        plan.note.is_some(),
                        "a memory note must exist exactly when the count is reduced: {plan:?}"
                    );
                }
            }
        }
    }
}

#[test]
fn manual_distortion_and_its_inverse_round_trip_interior_points() {
    for dims in [(100.0, 50.0), (50.0, 100.0), (4000.0, 3000.0)] {
        for amount in [-80.0, -40.0, -10.0, 10.0, 40.0, 80.0] {
            for (nx, ny) in [(0.2, 0.2), (0.4, 0.3), (0.5, 0.5), (0.7, 0.6), (0.8, 0.8)] {
                let corrected = autoshade::render::distort_norm(nx, ny, dims, amount);
                let restored = autoshade::render::undistort_norm(
                    corrected.0,
                    corrected.1,
                    dims,
                    amount,
                );

                assert!(
                    (restored.0 - nx).abs() < 1e-3 && (restored.1 - ny).abs() < 1e-3,
                    "dims={dims:?} amount={amount} point=({nx},{ny}) restored={restored:?}"
                );
            }
        }
    }
}

#[test]
fn profile_distortion_strength_scales_the_profile_map_without_dropping_it() {
    let recipe = EditRecipe {
        lens_profile: LensProfile {
            distortion: vec![1.0, 1.1],
            distortion_on: true,
            ..LensProfile::default()
        },
        lens_profile_distortion_scale: 50.0,
        ..EditRecipe::default()
    };

    let profile = autoshade::render::geometry_profile(&recipe);

    assert_eq!(profile.distortion, vec![1.0, 1.05]);
}

#[test]
fn manual_ca_folds_into_the_profile_ca_without_dropping_other_profile_fields() {
    let recipe = EditRecipe {
        lens_profile: LensProfile {
            distortion: vec![1.0, 1.1],
            distortion_on: true,
            ca_r: vec![1.0],
            ca_b: vec![1.0],
            ca_on: true,
            ..LensProfile::default()
        },
        lens_profile_distortion_scale: 50.0,
        ca_r: 100.0,
        ca_b: -100.0,
        ..EditRecipe::default()
    };

    let profile = autoshade::render::geometry_profile(&recipe);

    assert_eq!(profile.distortion, vec![1.0, 1.05]);
    assert!(profile.ca_on);
    assert!((profile.ca_r[0] - 1.002).abs() < 1e-6);
    assert!((profile.ca_b[0] - 0.998).abs() < 1e-6);
}

#[test]
fn manual_ca_alone_marks_the_profile_ca_active() {
    let recipe = EditRecipe {
        lens_profile: LensProfile {
            ca_r: vec![1.0],
            ca_b: vec![1.0],
            ca_on: false,
            ..LensProfile::default()
        },
        ca_r: 100.0,
        ca_b: -100.0,
        ..EditRecipe::default()
    };

    let profile = autoshade::render::geometry_profile(&recipe);

    assert!(profile.ca_on);
    assert_eq!(profile.ca_r, vec![1.002]);
    assert_eq!(profile.ca_b, vec![0.998]);
}

#[test]
fn lightroom_mask_warp_and_its_inverse_round_trip_interior_points() {
    let profiles = [
        LensProfile { mask_warp: vec![1.02, 1.01, 1.0], ..LensProfile::default() },
        LensProfile { mask_warp: vec![0.98, 1.0, 1.01], ..LensProfile::default() },
    ];
    for profile in profiles {
        for dims in [(100.0, 50.0), (50.0, 100.0), (4000.0, 3000.0)] {
            for (nx, ny) in [(0.1, 0.2), (0.25, 0.75), (0.5, 0.5), (0.75, 0.25), (0.9, 0.8)] {
                let warped =
                    autoshade::render::lr_mask_warp_norm(nx, ny, dims, &profile);
                let restored = autoshade::render::lr_mask_unwarp_norm(
                    warped.0,
                    warped.1,
                    dims,
                    &profile,
                );

                if (nx - 0.5).abs() > 0.1 || (ny - 0.5).abs() > 0.1 {
                    assert!(
                        (warped.0 - nx).abs() > 1e-4 || (warped.1 - ny).abs() > 1e-4,
                        "a non-identity mask warp must move an off-centre point: dims={dims:?} point=({nx},{ny})"
                    );
                }
                assert!(
                    (restored.0 - nx).abs() < 2e-3 && (restored.1 - ny).abs() < 2e-3,
                    "dims={dims:?} point=({nx},{ny}) restored={restored:?}"
                );
            }
        }
    }
}

#[test]
fn recipe_clamping_is_idempotent() {
    for exposure in [-100.0, -1.0, 0.0, 1.0, 100.0] {
        for contrast in [-500.0, -1.0, 0.0, 1.0, 500.0] {
            let mut recipe = EditRecipe {
                exposure_ev: exposure,
                contrast,
                highlights: f32::MAX,
                shadows: f32::MIN,
                ..EditRecipe::default()
            };
            recipe.clamp();
            let once = recipe.clone();

            let second = recipe.clamp();

            assert_eq!(recipe, once, "clamp changed an already-clamped recipe");
            assert!(second.is_empty(), "a second clamp reported new losses: {second:?}");
        }
    }
}

#[test]
fn xmp_round_trip_preserves_scalar_develop_controls() {
    for exposure in [-1.0, -0.25, 0.0, 0.75, 2.0] {
        for contrast in [-50.0, 0.0, 50.0] {
            for saturation in [-50.0, 0.0, 50.0] {
                let recipe = EditRecipe {
                    exposure_ev: exposure,
                    contrast,
                    saturation,
                    temperature_k: Some(5500.0),
                    tint: 0.0,
                    ..EditRecipe::default()
                };

                let restored = autoshade::xmp::xmp_to_recipe(&autoshade::xmp::recipe_to_xmp(&recipe));

                assert!(
                    (restored.exposure_ev - exposure).abs() < 0.011,
                    "exposure={exposure} restored={}",
                    restored.exposure_ev
                );
                assert_eq!(restored.contrast, contrast);
                assert_eq!(restored.saturation, saturation);
            }
        }
    }
}
