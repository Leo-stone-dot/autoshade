use autoshade::fit::{FitBudget, fit_recipe, fit_recipe_from};
use autoshade::fit_zoned::semantic::{ClassPlane, resolve_regions};
use autoshade::recipe::{EditRecipe, GradeStrength};
use image::{DynamicImage, GrayImage, Luma, Rgb, RgbImage};

#[ignore = "slow fit solve: run with cargo test --test fit_behavior -- --ignored"]
#[test]
fn a_degenerate_pair_refuses_to_fit_and_discloses_the_refusal() {
    let black = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([0, 0, 0])));

    let report = fit_recipe(&black, &black);

    assert_eq!(report.recipe.exposure_ev, 0.0);
    assert!(report.recipe.tone_curve.is_empty());
    assert!(
        report
            .notes
            .iter()
            .any(|note| note.key == autoshade::rationale::keys::FIT_DEGENERATE)
    );
}

#[ignore = "slow fit solve: run with cargo test --test fit_behavior -- --ignored"]
#[test]
fn a_degenerate_pair_keeps_the_calibration_base_in_the_refused_recipe() {
    let black = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([0, 0, 0])));
    let base = EditRecipe {
        as_shot_k: Some(4800.0),
        base_curve: vec![[0.0, 0.05], [1.0, 0.95]],
        ..EditRecipe::default()
    };

    let report = fit_recipe_from(&black, &black, &base);

    assert_eq!(report.recipe.as_shot_k, base.as_shot_k);
    assert_eq!(report.recipe.base_curve, base.base_curve);
    assert!(
        report
            .notes
            .iter()
            .any(|note| note.key == autoshade::rationale::keys::FIT_DEGENERATE)
    );
}

#[test]
fn fit_budget_scales_monotonically_with_strength() {
    let low = FitBudget::for_strength(GradeStrength::new(0.0));
    let mid = FitBudget::for_strength(GradeStrength::new(0.65));
    let high = FitBudget::for_strength(GradeStrength::new(1.0));

    assert!(low.ev < mid.ev && mid.ev < high.ev);
    assert!(low.sat < mid.sat && mid.sat < high.sat);
    assert!(low.cast_ratio < mid.cast_ratio && mid.cast_ratio < high.cast_ratio);
}

#[test]
fn joint_reading_matches_by_value_and_abstains_without_evidence() {
    let a = (0..4096)
        .map(|i| {
            let l = 0.05 + 0.9 * (i % 64) as f32 / 63.0;
            if (i / 64) % 2 == 0 {
                [l, l, l]
            } else {
                [(l + 0.15).min(1.0), l * 0.7, l * 0.35]
            }
        })
        .collect::<Vec<_>>();
    let mut b = a.iter().step_by(2).copied().collect::<Vec<_>>();
    b.reverse();

    let same = autoshade::fit_zoned::joint_reading(&a, &b).expect("same values have a reading");
    let empty = autoshade::fit_zoned::joint_reading(&[], &[]);

    assert!(same.weighted < 0.01);
    assert!(same.buckets >= 4);
    assert!(empty.is_none());
    assert!(autoshade::fit_zoned::joint_buckets(&[], &a).is_empty());
}

#[test]
fn joint_reading_reports_a_material_colour_mismatch_as_nonzero() {
    let source = vec![[0.6f32, 0.3, 0.3]; 4096];
    let target = vec![[0.3f32, 0.3, 0.6]; 4096];

    let reading = autoshade::fit_zoned::joint_reading(&source, &target)
        .expect("a populated colour change has a joint reading");

    assert!(reading.weighted > 0.05, "the mismatch must be visible: {reading:?}");
}

#[test]
fn a_semantic_class_missing_from_the_target_is_not_a_region() {
    let source = vec![ClassPlane {
        class_id: 1,
        label: "sky".to_string(),
        mean_confidence: 0.9,
        mask: GrayImage::from_pixel(2, 2, Luma([255])),
    }];
    let target = vec![ClassPlane {
        class_id: 2,
        label: "land".to_string(),
        mean_confidence: 0.9,
        mask: GrayImage::from_pixel(2, 2, Luma([255])),
    }];

    assert!(resolve_regions(&source, &target, 2).is_empty());
}

#[test]
fn overlapping_semantic_classes_are_resolved_to_one_disjoint_region() {
    let full = || GrayImage::from_pixel(2, 2, Luma([255]));
    let source = vec![
        ClassPlane { class_id: 1, label: "broad".to_string(), mean_confidence: 0.5, mask: full() },
        ClassPlane { class_id: 2, label: "specific".to_string(), mean_confidence: 0.9, mask: full() },
    ];
    let target = vec![
        ClassPlane { class_id: 1, label: "broad".to_string(), mean_confidence: 0.5, mask: full() },
        ClassPlane { class_id: 2, label: "specific".to_string(), mean_confidence: 0.9, mask: full() },
    ];

    let regions = resolve_regions(&source, &target, 2);

    assert_eq!(regions.len(), 1);
    assert_eq!(regions[0].class_id, 2);
    assert_eq!(regions[0].source.get_pixel(0, 0).0[0], 255);
}

#[test]
fn semantic_region_resolution_returns_at_most_the_requested_limit() {
    let left = GrayImage::from_fn(2, 2, |x, _| if x == 0 { Luma([255]) } else { Luma([0]) });
    let right = GrayImage::from_fn(2, 2, |x, _| if x == 1 { Luma([255]) } else { Luma([0]) });
    let source = vec![
        ClassPlane { class_id: 1, label: "left".to_string(), mean_confidence: 0.9, mask: left.clone() },
        ClassPlane { class_id: 2, label: "right".to_string(), mean_confidence: 0.9, mask: right.clone() },
    ];
    let target = vec![
        ClassPlane { class_id: 1, label: "left".to_string(), mean_confidence: 0.9, mask: left },
        ClassPlane { class_id: 2, label: "right".to_string(), mean_confidence: 0.9, mask: right },
    ];

    assert_eq!(resolve_regions(&source, &target, 1).len(), 1);
}

#[test]
fn the_semantic_bitmap_budget_refuses_an_over_cap_request() {
    assert!(autoshade::fit_zoned::semantic::bitmap_budget_allows(0, 1, 1, 1));
    assert!(autoshade::fit_zoned::semantic::bitmap_budget_allows(
        0,
        8192,
        8192,
        1
    ));
    assert!(!autoshade::fit_zoned::semantic::bitmap_budget_allows(
        1,
        8192,
        8192,
        1
    ));
    assert!(!autoshade::fit_zoned::semantic::bitmap_budget_allows(
        usize::MAX,
        1,
        1,
        1
    ));
}

#[test]
fn semantic_region_resolution_with_a_zero_limit_returns_no_regions() {
    let plane = ClassPlane {
        class_id: 1,
        label: "sky".to_string(),
        mean_confidence: 0.9,
        mask: GrayImage::from_pixel(2, 2, Luma([255])),
    };
    let planes = [plane];

    assert!(resolve_regions(&planes, &planes, 0).is_empty());
}

#[test]
fn semantic_region_resolution_rejects_mismatched_geometry() {
    let source = ClassPlane {
        class_id: 1,
        label: "sky".to_string(),
        mean_confidence: 0.9,
        mask: GrayImage::from_pixel(2, 2, Luma([255])),
    };
    let target = ClassPlane {
        class_id: 1,
        label: "sky".to_string(),
        mean_confidence: 0.9,
        mask: GrayImage::from_pixel(3, 3, Luma([255])),
    };

    assert!(resolve_regions(&[source], &[target], 2).is_empty());
}

#[test]
fn semantic_region_resolution_rejects_an_invalid_confidence() {
    let source = ClassPlane {
        class_id: 1,
        label: "sky".to_string(),
        mean_confidence: 1.5,
        mask: GrayImage::from_pixel(2, 2, Luma([255])),
    };
    let target = ClassPlane {
        class_id: 1,
        label: "sky".to_string(),
        mean_confidence: 1.5,
        mask: GrayImage::from_pixel(2, 2, Luma([255])),
    };

    assert!(resolve_regions(&[source], &[target], 2).is_empty());
}

#[test]
fn semantic_region_resolution_rejects_a_class_below_the_share_floor() {
    let mut tiny = GrayImage::from_pixel(10, 10, Luma([0]));
    tiny.put_pixel(0, 0, Luma([255]));
    let source = ClassPlane {
        class_id: 1,
        label: "tiny".to_string(),
        mean_confidence: 0.9,
        mask: tiny.clone(),
    };
    let target = ClassPlane {
        class_id: 1,
        label: "tiny".to_string(),
        mean_confidence: 0.9,
        mask: tiny,
    };

    assert!(resolve_regions(&[source], &[target], 2).is_empty());
}

#[test]
fn semantic_region_resolution_rejects_a_target_below_the_share_floor() {
    let source = ClassPlane {
        class_id: 1,
        label: "sky".to_string(),
        mean_confidence: 0.9,
        mask: GrayImage::from_pixel(10, 10, Luma([255])),
    };
    let mut tiny = GrayImage::from_pixel(10, 10, Luma([0]));
    tiny.put_pixel(0, 0, Luma([255]));
    let target = ClassPlane {
        class_id: 1,
        label: "sky".to_string(),
        mean_confidence: 0.9,
        mask: tiny,
    };

    assert!(resolve_regions(&[source], &[target], 2).is_empty());
}

#[test]
fn joint_buckets_partition_the_shared_value_range() {
    let pixels = (0..4096)
        .map(|i| {
            let l = 0.05 + 0.9 * (i % 64) as f32 / 63.0;
            if (i / 64) % 2 == 0 {
                [l, l, l]
            } else {
                [(l + 0.15).min(1.0), l * 0.7, l * 0.35]
            }
        })
        .collect::<Vec<_>>();

    let buckets = autoshade::fit_zoned::joint_buckets(&pixels, &pixels);

    assert!(!buckets.is_empty());
    assert!(buckets.iter().all(|bucket| (0.0..=1.0).contains(&bucket.share)));
    assert!(buckets.iter().map(|bucket| bucket.share).sum::<f32>() <= 1.0 + 1e-5);
}
