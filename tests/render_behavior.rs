use autoshade::recipe::{
    BrushStroke, ColorGrade, CreativeLook, CurvePoint, EditRecipe, LensProfile, LocalAdjustment,
    MaskGeometry,
};
use autoshade::retouch::{RetouchArea, RetouchShape, SpotOrigin};
use image::{DynamicImage, Rgb, RgbImage};
use rawler::decoders::Orientation;

mod support;

fn render(source: &RgbImage, recipe: &EditRecipe) -> RgbImage {
    autoshade::render::develop_preview(
        &DynamicImage::ImageRgb8(source.clone()),
        recipe,
    )
    .to_rgb8()
}

fn luma(pixel: &Rgb<u8>) -> f64 {
    0.299 * f64::from(pixel[0])
        + 0.587 * f64::from(pixel[1])
        + 0.114 * f64::from(pixel[2])
}

fn mean_chroma(image: &RgbImage) -> f64 {
    image
        .pixels()
        .map(|pixel| {
            let min = pixel[0].min(pixel[1]).min(pixel[2]);
            let max = pixel[0].max(pixel[1]).max(pixel[2]);
            f64::from(max - min)
        })
        .sum::<f64>()
        / f64::from(image.width() * image.height())
}

fn luma_spread(image: &RgbImage) -> f64 {
    let mut values = image.pixels().map(luma).collect::<Vec<_>>();
    values.sort_by(f64::total_cmp);
    values[values.len() * 9 / 10] - values[values.len() / 10]
}

fn srgb_to_linear(channel: f32) -> f32 {
    if channel <= 0.04045 {
        channel / 12.92
    } else {
        ((channel + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(channel: f32) -> f32 {
    if channel <= 0.003_130_8 {
        channel * 12.92
    } else {
        1.055 * channel.powf(1.0 / 2.4) - 0.055
    }
}

fn hazy_ramp(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        let light = x as f32 / (width - 1) as f32;
        let clear = match y * 4 / height {
            0 => [light, light, light],
            1 => [light, light * 0.6, light * 0.2],
            2 => [light * 0.2, light * 0.7, light],
            _ => [light * 0.3, light, light * 0.4],
        };
        let hazy = clear.map(|channel| {
            linear_to_srgb(srgb_to_linear(channel) * 0.55 + 0.9 * 0.45)
        });
        Rgb(hazy.map(|channel| (channel * 255.0).round() as u8))
    })
}

#[test]
fn a_neutral_recipe_preserves_the_delivered_pixels() {
    let source = RgbImage::from_fn(32, 24, |x, y| {
        Rgb([
            ((x * 7 + y * 3) % 256) as u8,
            ((x * 5 + y * 11) % 256) as u8,
            ((x * 13 + y * 2) % 256) as u8,
        ])
    });

    let delivered = render(&source, &EditRecipe::default());
    let max_delta = source
        .pixels()
        .zip(delivered.pixels())
        .flat_map(|(before, after)| {
            before
                .0
                .into_iter()
                .zip(after.0)
                .map(|(before, after)| before.abs_diff(after))
        })
        .max()
        .unwrap();

    assert!(
        max_delta <= 1,
        "a neutral develop moved a channel by {max_delta} codes"
    );
}

#[test]
fn opposing_vignette_stages_cancel_before_highlights_are_clamped() {
    let source = RgbImage::from_pixel(9, 9, Rgb([230, 230, 230]));
    let neutral = render(&source, &EditRecipe::default());
    let profile_only = render(
        &source,
        &EditRecipe {
            lens_profile: LensProfile {
                vignette: vec![2.0; 4],
                vignette_on: true,
                ..LensProfile::default()
            },
            ..EditRecipe::default()
        },
    );
    let combined = render(
        &source,
        &EditRecipe {
            lens_vignette: -50.0,
            lens_profile: LensProfile {
                vignette: vec![2.0; 4],
                vignette_on: true,
                ..LensProfile::default()
            },
            ..EditRecipe::default()
        },
    );

    assert!(profile_only.get_pixel(0, 0)[0] > neutral.get_pixel(0, 0)[0]);
    assert!(
        combined.get_pixel(0, 0)[0].abs_diff(neutral.get_pixel(0, 0)[0]) <= 1,
        "inverse vignette gains lost a bright corner: {:?} -> {:?}",
        neutral.get_pixel(0, 0),
        combined.get_pixel(0, 0)
    );
}

#[test]
fn a_base_curve_changes_pixels_without_user_tone_controls() {
    let source = RgbImage::from_pixel(8, 8, Rgb([100, 100, 100]));
    let neutral = render(&source, &EditRecipe::default());
    let lifted = render(
        &source,
        &EditRecipe {
            base_curve: vec![[0.0, 0.0], [0.4, 0.62], [1.0, 1.0]],
            ..EditRecipe::default()
        },
    );

    assert_eq!(neutral.get_pixel(0, 0)[0], 100);
    assert!(lifted.get_pixel(0, 0)[0] > 130);
}

#[test]
fn user_exposure_composes_after_the_base_curve() {
    let source = RgbImage::from_fn(32, 8, |x, _| {
        let value = (32 + x * 5) as u8;
        Rgb([value, value, value])
    });
    let curve = vec![[0.0, 0.0], [0.4, 0.62], [1.0, 1.0]];
    let based = render(
        &source,
        &EditRecipe {
            base_curve: curve.clone(),
            ..EditRecipe::default()
        },
    );
    let sequential = render(
        &based,
        &EditRecipe {
            exposure_ev: 1.0,
            ..EditRecipe::default()
        },
    );
    let combined = render(
        &source,
        &EditRecipe {
            base_curve: curve,
            exposure_ev: 1.0,
            ..EditRecipe::default()
        },
    );
    let worst = sequential
        .pixels()
        .zip(combined.pixels())
        .flat_map(|(expected, actual)| {
            expected
                .0
                .into_iter()
                .zip(actual.0)
                .map(|(expected, actual)| expected.abs_diff(actual))
        })
        .max()
        .unwrap();

    assert!(worst <= 2, "base then exposure differed by {worst} codes");
}

#[test]
fn a_base_curve_keeps_black_pinned() {
    let source = RgbImage::from_pixel(4, 4, Rgb([0, 0, 0]));
    let delivered = render(
        &source,
        &EditRecipe {
            base_curve: vec![[0.0, 0.0], [0.4, 0.62], [1.0, 1.0]],
            ..EditRecipe::default()
        },
    );

    assert_eq!(delivered.get_pixel(0, 0), &Rgb([0, 0, 0]));
}

#[test]
fn naming_a_camera_profile_does_not_replace_the_recipe_base_curve() {
    let source = RgbImage::from_fn(32, 8, |x, _| {
        let value = (32 + x * 5) as u8;
        Rgb([value; 3])
    });
    let curve = vec![[0.0, 0.0], [0.25, 0.4], [0.5, 0.7], [1.0, 1.0]];
    let unprofiled = render(
        &source,
        &EditRecipe {
            base_curve: curve.clone(),
            contrast: 20.0,
            ..EditRecipe::default()
        },
    );
    let profiled = render(
        &source,
        &EditRecipe {
            base_curve: curve,
            camera_profile: "Adobe Standard".to_string(),
            contrast: 20.0,
            ..EditRecipe::default()
        },
    );
    let without_base = render(
        &source,
        &EditRecipe {
            camera_profile: "Adobe Standard".to_string(),
            contrast: 20.0,
            ..EditRecipe::default()
        },
    );

    assert!(profiled.as_raw() == unprofiled.as_raw());
    assert!(profiled.as_raw() != without_base.as_raw());
}

#[test]
fn a_creative_looks_baked_tone_controls_do_not_reapply_to_pixels() {
    let point = |input, output| CurvePoint { input, output };
    let baked = CreativeLook {
        name: "Adobe Color".to_string(),
        amount: 1.0,
        tone_curve: vec![
            point(0, 0),
            point(22, 16),
            point(40, 35),
            point(127, 127),
            point(224, 230),
            point(255, 255),
        ],
        red_curve: vec![point(0, 0), point(128, 64), point(255, 255)],
        shadows: -40.0,
        highlights: -12.0,
        ..CreativeLook::default()
    };
    let source = RgbImage::from_fn(16, 8, |x, y| {
        Rgb([(x * 16) as u8, (y * 30) as u8, (255 - x * 12) as u8])
    });
    let own = EditRecipe {
        shadows: -40.0,
        contrast: 15.0,
        ..EditRecipe::default()
    };
    let with_look = EditRecipe {
        look: Some(baked),
        ..own.clone()
    };

    let actual = render(&source, &with_look);
    let expected = render(&source, &own);
    assert!(
        actual.as_raw() == expected.as_raw(),
        "creative Look tone controls were applied twice"
    );
}

#[test]
fn a_monochrome_creative_look_delivers_greyscale_pixels() {
    let source = RgbImage::from_pixel(8, 8, Rgb([200, 60, 40]));
    let recipe = EditRecipe {
        look: Some(CreativeLook {
            name: "Adobe Monochrome".to_string(),
            amount: 1.0,
            grayscale: true,
            ..CreativeLook::default()
        }),
        ..EditRecipe::default()
    };

    assert!(!recipe.convert_to_grayscale);
    let delivered = render(&source, &recipe);
    let pixel = delivered.get_pixel(0, 0);
    let (low, high) = (
        pixel[0].min(pixel[1]).min(pixel[2]),
        pixel[0].max(pixel[1]).max(pixel[2]),
    );
    assert!(high - low <= 1, "monochrome Look delivered {pixel:?}");
}

fn retouch_object_fixture() -> RgbImage {
    let mut source = RgbImage::from_pixel(160, 120, Rgb([140; 3]));
    for y in 0..120 {
        for x in 0..160 {
            let (dx, dy) = (x as f32 - 40.0, y as f32 - 60.0);
            if dx * dx + dy * dy <= 64.0 {
                source.put_pixel(x, y, Rgb([0, 0, 0]));
            }
        }
    }
    source
}

fn darkest_retouch_object(image: &RgbImage) -> u8 {
    let mut darkest = 255;
    for y in 51..70 {
        for x in 31..50 {
            let (dx, dy) = (x as f32 - 40.0, y as f32 - 60.0);
            if dx * dx + dy * dy <= 64.0 {
                darkest = darkest.min(image.get_pixel(x, y)[0]);
            }
        }
    }
    darkest
}

fn assert_retouch_removes_dark_object(shape: RetouchShape) {
    let source = retouch_object_fixture();
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            retouch: vec![RetouchArea {
                origin: SpotOrigin::LightroomContentAware,
                feather: 0.0,
                donor: None,
                shape,
            }],
            ..EditRecipe::default()
        },
    );
    let far = delivered.get_pixel(130, 25)[0];

    assert!(darkest_retouch_object(&neutral) + 80 < neutral.get_pixel(130, 25)[0]);
    assert!(
        darkest_retouch_object(&delivered) + 20 > far,
        "retouch left the dark object visible"
    );
}

#[test]
fn an_elliptical_retouch_area_removes_a_dark_object() {
    assert_retouch_removes_dark_object(RetouchShape::Ellipse {
        cx: 40.0 / 160.0,
        cy: 60.0 / 120.0,
        size_x: 10.0 / 160.0,
        size_y: 10.0 / 160.0,
    });
}

#[test]
fn a_brush_retouch_area_removes_a_dark_object() {
    assert_retouch_removes_dark_object(RetouchShape::Brush(vec![BrushStroke {
        value: 1.0,
        radius: 10.0 / 160.0,
        flow: 1.0,
        center_weight: 1.0,
        sync_id: String::new(),
        dabs: "d 0.250000 0.500000".to_string(),
    }]));
}

#[test]
fn a_brush_retouch_plan_keeps_its_area_metadata_and_exact_shape() {
    let recipe = EditRecipe {
        retouch: vec![RetouchArea {
            origin: SpotOrigin::LightroomGenerative,
            feather: 0.25,
            donor: None,
            shape: RetouchShape::Brush(vec![BrushStroke {
                value: 1.0,
                radius: 0.05,
                flow: 1.0,
                center_weight: 1.0,
                sync_id: String::new(),
                dabs: "d 0.750000 0.500000".to_string(),
            }]),
        }],
        ..EditRecipe::default()
    };

    let spots = autoshade::render::retouch_spots(&recipe, 160, 120);

    assert!(!spots.is_empty());
    assert!(spots.iter().all(|spot| {
        spot.origin == SpotOrigin::LightroomGenerative
            && (spot.feather - 0.25).abs() < 1e-6
            && spot.coverage.is_some()
    }));
}

fn retouch_orientation_recipe() -> EditRecipe {
    let area = |shape| RetouchArea {
        origin: SpotOrigin::LightroomContentAware,
        feather: 0.5,
        donor: Some([0.6, 0.4]),
        shape,
    };
    EditRecipe {
        retouch: vec![
            area(RetouchShape::Ellipse {
                cx: 0.25,
                cy: 0.5,
                size_x: 0.05,
                size_y: 0.05,
            }),
            area(RetouchShape::Brush(vec![BrushStroke {
                value: 1.0,
                radius: 0.05,
                flow: 1.0,
                center_weight: 1.0,
                sync_id: String::new(),
                dabs: "d 0.250000 0.500000".to_string(),
            }])),
        ],
        ..EditRecipe::default()
    }
}

#[test]
fn retouch_geometry_moves_with_a_quarter_turn() {
    let mut recipe = retouch_orientation_recipe();
    let before = recipe.retouch.clone();

    assert!(autoshade::render::orient_recipe_coords(
        &mut recipe,
        Orientation::Rotate90,
        autoshade::render::CoordFrame::new(9504.0, 6336.0),
    ));

    let (
        RetouchShape::Ellipse {
            cx: before_cx,
            size_x: before_size,
            ..
        },
        RetouchShape::Ellipse {
            cx: after_cx,
            size_x: after_size,
            ..
        },
    ) = (&before[0].shape, &recipe.retouch[0].shape)
    else {
        panic!("ellipse shape changed kind")
    };
    assert!(before_cx != after_cx && before_size != after_size);
    let (RetouchShape::Brush(before_brush), RetouchShape::Brush(after_brush)) =
        (&before[1].shape, &recipe.retouch[1].shape)
    else {
        panic!("brush shape changed kind")
    };
    assert!(
        before_brush[0].dabs != after_brush[0].dabs
            && before_brush[0].radius != after_brush[0].radius
    );
    assert_ne!(before[0].donor, recipe.retouch[0].donor);
}

#[test]
fn retouch_geometry_returns_after_the_inverse_turn() {
    let mut recipe = retouch_orientation_recipe();
    let before = recipe.retouch.clone();
    autoshade::render::orient_recipe_coords(
        &mut recipe,
        Orientation::Rotate90,
        autoshade::render::CoordFrame::new(9504.0, 6336.0),
    );

    autoshade::render::orient_recipe_coords(
        &mut recipe,
        Orientation::Rotate270,
        autoshade::render::CoordFrame::new(6336.0, 9504.0),
    );

    for (expected, actual) in before.iter().zip(&recipe.retouch) {
        match (&expected.shape, &actual.shape) {
            (
                RetouchShape::Ellipse {
                    cx: x0,
                    cy: y0,
                    size_x: sx0,
                    size_y: sy0,
                },
                RetouchShape::Ellipse {
                    cx: x1,
                    cy: y1,
                    size_x: sx1,
                    size_y: sy1,
                },
            ) => {
                assert!(
                    [(x0, x1), (y0, y1), (sx0, sx1), (sy0, sy1)]
                        .into_iter()
                        .all(|(left, right)| (left - right).abs() < 1e-6)
                );
            }
            (RetouchShape::Brush(before_brush), RetouchShape::Brush(after_brush)) => {
                assert_eq!(before_brush[0].dabs, after_brush[0].dabs);
                assert!((before_brush[0].radius - after_brush[0].radius).abs() < 1e-6);
            }
            _ => panic!("retouch shape changed kind"),
        }
        let (Some(expected_donor), Some(actual_donor)) = (expected.donor, actual.donor) else {
            panic!("retouch donor disappeared")
        };
        assert!(
            expected_donor
                .into_iter()
                .zip(actual_donor)
                .all(|(left, right)| (left - right).abs() < 1e-6)
        );
    }
}

fn sdr_ramp() -> RgbImage {
    RgbImage::from_fn(64, 256, |_, y| Rgb([y as u8; 3]))
}

#[test]
fn sdr_controls_are_inert_outside_hdr_edit_mode() {
    let source = sdr_ramp();
    let bare = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            sdr_brightness: 40.0,
            sdr_contrast: 30.0,
            sdr_highlights: -50.0,
            sdr_shadows: 25.0,
            sdr_whites: -20.0,
            sdr_clarity: 20.0,
            ..EditRecipe::default()
        },
    );

    assert!(
        delivered.as_raw() == bare.as_raw(),
        "SDR controls changed a non-HDR photograph"
    );
}

#[test]
fn sdr_brightness_lifts_midtones_inside_hdr_edit_mode() {
    let source = sdr_ramp();
    let bare = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            hdr_edit: true,
            sdr_brightness: 40.0,
            ..EditRecipe::default()
        },
    );

    assert!(delivered.get_pixel(32, 96)[0] > bare.get_pixel(32, 96)[0]);
}

#[test]
fn sdr_blend_scales_the_hdr_highlight_shoulder() {
    let source = sdr_ramp();
    let bare = render(&source, &EditRecipe::default());
    let one = render(
        &source,
        &EditRecipe {
            hdr_edit: true,
            hdr_max_ev: 1.0,
            ..EditRecipe::default()
        },
    );
    let off = render(
        &source,
        &EditRecipe {
            hdr_edit: true,
            hdr_max_ev: 1.0,
            sdr_blend: -100.0,
            ..EditRecipe::default()
        },
    );
    let twice = render(
        &source,
        &EditRecipe {
            hdr_edit: true,
            hdr_max_ev: 1.0,
            sdr_blend: 100.0,
            ..EditRecipe::default()
        },
    );

    assert!(one.get_pixel(32, 230)[0] < bare.get_pixel(32, 230)[0]);
    assert!(off.as_raw() == bare.as_raw());
    assert!(twice.get_pixel(32, 230)[0] < one.get_pixel(32, 230)[0]);
}

#[test]
fn hdr_headroom_keeps_growing_past_slider_limits() {
    let source = sdr_ramp();
    let top = |headroom| {
        render(
            &source,
            &EditRecipe {
                hdr_edit: true,
                hdr_max_ev: headroom,
                ..EditRecipe::default()
            },
        )
        .get_pixel(32, 230)[0] as i32
    };
    let bare = top(0.0);
    let (d1, d2, d4) = (bare - top(1.0), bare - top(2.0), bare - top(4.0));

    assert!(0 < d1 && d1 < d2 && d2 < d4);
    assert!(bare - top(2.30) >= 40);
}

#[test]
fn sdr_clarity_matches_basic_clarity_pixels() {
    let source = RgbImage::from_fn(512, 512, |x, y| {
        let value = if (x / 32 + y / 32) % 2 == 0 {
            90
        } else {
            165
        };
        Rgb([value; 3])
    });
    let bare = render(&source, &EditRecipe::default());
    let basic = render(
        &source,
        &EditRecipe {
            clarity: 60.0,
            ..EditRecipe::default()
        },
    );
    let sdr = render(
        &source,
        &EditRecipe {
            hdr_edit: true,
            sdr_clarity: 60.0,
            ..EditRecipe::default()
        },
    );

    assert!(basic.as_raw() != bare.as_raw());
    assert!(
        sdr.as_raw() == basic.as_raw(),
        "SDR and Basic clarity delivered different pixels"
    );
}

#[test]
fn positive_dehaze_restores_tonal_span_and_colour() {
    let source = hazy_ramp(64, 32);
    let delivered = render(
        &source,
        &EditRecipe {
            dehaze: 50.0,
            ..EditRecipe::default()
        },
    );

    assert!(
        luma_spread(&delivered) > luma_spread(&source) * 1.15,
        "dehaze did not restore the tonal span"
    );
    assert!(
        mean_chroma(&delivered) > mean_chroma(&source) * 1.10,
        "dehaze did not restore colour separation"
    );
}

#[test]
fn local_dehaze_leaves_uncovered_pixels_unchanged() {
    let source = hazy_ramp(64, 16);
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.0,
                    full_x: 0.0,
                    full_y: 0.0,
                },
                amount: 1.0,
                dehaze: 100.0,
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        },
    );

    assert!(
        delivered.get_pixel(0, 8) != neutral.get_pixel(0, 8),
        "the fully covered area did not receive local dehaze"
    );
    for y in 0..source.height() {
        for x in source.width() / 2..source.width() {
            assert_eq!(
                delivered.get_pixel(x, y),
                neutral.get_pixel(x, y),
                "local dehaze leaked into uncovered pixel ({x}, {y})"
            );
        }
    }
}

#[test]
fn hsl_saturation_changes_only_the_target_colour_family() {
    let source = RgbImage::from_raw(2, 1, vec![204, 26, 26, 26, 26, 204]).unwrap();
    let mut recipe = EditRecipe::default();
    recipe.hsl.saturation[0] = -100.0;

    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(&source, &recipe);
    let red = delivered.get_pixel(0, 0);
    let red_spread = red[0].max(red[1]).max(red[2]) - red[0].min(red[1]).min(red[2]);

    assert!(red_spread < 16, "the red family did not desaturate: {red:?}");
    for channel in 0..3 {
        assert!(
            delivered.get_pixel(1, 0)[channel].abs_diff(neutral.get_pixel(1, 0)[channel]) <= 2,
            "the red control moved the blue family: {:?} -> {:?}",
            neutral.get_pixel(1, 0),
            delivered.get_pixel(1, 0)
        );
    }
}

#[test]
fn local_temperature_warms_only_the_covered_region() {
    let source = RgbImage::from_pixel(1, 4, Rgb([128, 128, 128]));
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.125,
                    full_x: 0.5,
                    full_y: 0.875,
                },
                amount: 1.0,
                temperature: 100.0,
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        },
    );

    assert_eq!(delivered.get_pixel(0, 0), neutral.get_pixel(0, 0));
    let warm = delivered.get_pixel(0, 3);
    assert!(
        warm[0] > neutral.get_pixel(0, 3)[0]
            && warm[2] < neutral.get_pixel(0, 3)[2],
        "the covered region did not warm: {:?} -> {warm:?}",
        neutral.get_pixel(0, 3)
    );
}

#[test]
fn sharpening_strengthens_an_edge_without_moving_far_flat_pixels() {
    let source = RgbImage::from_fn(32, 8, |x, _| {
        if x < 16 {
            Rgb([77, 77, 77])
        } else {
            Rgb([179, 179, 179])
        }
    });
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            sharpening: 120.0,
            ..EditRecipe::default()
        },
    );
    let before = i16::from(neutral.get_pixel(16, 4)[0])
        - i16::from(neutral.get_pixel(15, 4)[0]);
    let after = i16::from(delivered.get_pixel(16, 4)[0])
        - i16::from(delivered.get_pixel(15, 4)[0]);

    assert!(after > before, "sharpening did not strengthen the edge");
    for x in [0, 1, 30, 31] {
        assert_eq!(
            delivered.get_pixel(x, 4),
            neutral.get_pixel(x, 4),
            "sharpening moved far flat pixel {x}"
        );
    }
}

#[test]
fn each_rgb_curve_controls_only_its_own_channel() {
    let source = RgbImage::from_pixel(1, 1, Rgb([0, 0, 0]));
    let lift = || {
        vec![
            CurvePoint {
                input: 0,
                output: 60,
            },
            CurvePoint {
                input: 255,
                output: 255,
            },
        ]
    };
    let recipes = [
        EditRecipe {
            red_curve: lift(),
            ..EditRecipe::default()
        },
        EditRecipe {
            green_curve: lift(),
            ..EditRecipe::default()
        },
        EditRecipe {
            blue_curve: lift(),
            ..EditRecipe::default()
        },
    ];

    for (expected_channel, recipe) in recipes.iter().enumerate() {
        let delivered = render(&source, recipe);
        let pixel = delivered.get_pixel(0, 0);
        assert!(
            pixel[expected_channel] > 38,
            "channel {expected_channel} was not lifted: {pixel:?}"
        );
        for channel in (0..3).filter(|channel| *channel != expected_channel) {
            assert!(
                pixel[channel] < 5,
                "curve for channel {expected_channel} moved channel {channel}: {pixel:?}"
            );
        }
    }
}

#[test]
fn extreme_tone_controls_keep_a_monotonic_tonal_ramp() {
    let source = RgbImage::from_fn(256, 1, |x, _| {
        let value = x as u8;
        Rgb([value, value, value])
    });
    let recipes = [
        EditRecipe {
            highlights: -100.0,
            shadows: 100.0,
            contrast: 100.0,
            ..EditRecipe::default()
        },
        EditRecipe {
            highlights: 100.0,
            shadows: -100.0,
            contrast: -100.0,
            ..EditRecipe::default()
        },
        EditRecipe {
            whites: -100.0,
            blacks: 100.0,
            ..EditRecipe::default()
        },
        EditRecipe {
            whites: 100.0,
            blacks: -100.0,
            ..EditRecipe::default()
        },
    ];

    for recipe in recipes {
        let delivered = render(&source, &recipe);
        let levels = delivered
            .pixels()
            .map(|pixel| pixel[0])
            .collect::<std::collections::BTreeSet<_>>();
        for x in 1..delivered.width() {
            assert!(
                delivered.get_pixel(x, 0)[0] >= delivered.get_pixel(x - 1, 0)[0],
                "tone controls reversed the ramp at {x}: {:?}",
                recipe
            );
        }
        assert!(
            levels.len() >= 32,
            "tone controls collapsed the ramp to {} levels: {:?}",
            levels.len(),
            recipe
        );
    }
}

#[test]
fn shadow_colour_grading_leaves_highlights_neutral() {
    let source = RgbImage::from_raw(2, 1, vec![38, 38, 38, 230, 230, 230]).unwrap();
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            color_grade: ColorGrade {
                shadow_hue: 240.0,
                shadow_sat: 100.0,
                blending: 100.0,
                ..ColorGrade::default()
            },
            ..EditRecipe::default()
        },
    );
    let shadow = delivered.get_pixel(0, 0);
    let highlight = delivered.get_pixel(1, 0);

    assert!(
        shadow[2] > shadow[0] && shadow[2] > shadow[1],
        "the shadow wheel did not tint the shadow blue: {shadow:?}"
    );
    for channel in 0..3 {
        assert!(
            highlight[channel].abs_diff(neutral.get_pixel(1, 0)[channel]) <= 2,
            "the shadow wheel moved the highlight: {:?} -> {highlight:?}",
            neutral.get_pixel(1, 0)
        );
    }
}

#[test]
fn point_colour_moves_its_sampled_colour_without_moving_a_distant_hue() {
    let source = RgbImage::from_raw(2, 1, vec![204, 26, 26, 26, 26, 204]).unwrap();
    let neutral = render(&source, &EditRecipe::default());
    let mut swatch =
        autoshade::render::point_color_at([0.8, 0.1, 0.1]).expect("red has a point-colour swatch");
    swatch.sat_scale = -1.0;
    let delivered = render(
        &source,
        &EditRecipe {
            point_colors: vec![swatch],
            ..EditRecipe::default()
        },
    );
    let red = delivered.get_pixel(0, 0);
    let red_spread = red[0].max(red[1]).max(red[2]) - red[0].min(red[1]).min(red[2]);

    assert!(red_spread < 32, "the sampled red did not desaturate: {red:?}");
    for channel in 0..3 {
        assert!(
            delivered.get_pixel(1, 0)[channel].abs_diff(neutral.get_pixel(1, 0)[channel]) <= 2,
            "the red swatch moved the distant blue hue: {:?} -> {:?}",
            neutral.get_pixel(1, 0),
            delivered.get_pixel(1, 0)
        );
    }
}

#[test]
fn a_local_point_curve_leaves_uncovered_pixels_unchanged() {
    let source = RgbImage::from_pixel(16, 4, Rgb([153, 128, 102]));
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.0,
                    full_x: 0.0,
                    full_y: 0.0,
                },
                amount: 1.0,
                main_curve: vec![
                    CurvePoint {
                        input: 0,
                        output: 0,
                    },
                    CurvePoint {
                        input: 128,
                        output: 64,
                    },
                    CurvePoint {
                        input: 255,
                        output: 255,
                    },
                ],
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        },
    );

    assert!(
        delivered.get_pixel(0, 2)[0] + 10 < neutral.get_pixel(0, 2)[0],
        "the local point curve did not darken its covered area"
    );
    for y in 0..source.height() {
        for x in source.width() / 2..source.width() {
            assert_eq!(
                delivered.get_pixel(x, y),
                neutral.get_pixel(x, y),
                "the local point curve leaked into uncovered pixel ({x}, {y})"
            );
        }
    }
}

#[test]
fn a_linear_mask_delivers_a_smooth_monotonic_transition() {
    let source = RgbImage::from_pixel(1, 8, Rgb([153, 153, 153]));
    let delivered = render(
        &source,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Linear {
                    zero_x: 0.5,
                    zero_y: 0.0,
                    full_x: 0.5,
                    full_y: 1.0,
                },
                amount: 1.0,
                exposure_ev: -2.0,
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        },
    );
    let levels = (0..delivered.height())
        .map(|y| delivered.get_pixel(0, y)[0])
        .collect::<Vec<_>>();

    assert!(
        levels.windows(2).all(|pair| pair[0] > pair[1]),
        "the linear mask collapsed or reversed its transition: {levels:?}"
    );
}

#[test]
fn a_bitmap_mask_scales_the_effect_with_its_stored_alpha() {
    let tree = support::TempTree::new("render-bitmap-alpha");
    let mask_path = tree.path().join("mask.png");
    image::GrayImage::from_fn(8, 1, |x, _| image::Luma([(x * 255 / 7) as u8]))
        .save(&mask_path)
        .unwrap();
    let source = RgbImage::from_pixel(8, 1, Rgb([128, 128, 128]));
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Bitmap {
                    path: mask_path.to_string_lossy().into_owned(),
                },
                amount: 1.0,
                exposure_ev: 1.0,
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        },
    );
    let gains = (0..8)
        .map(|x| delivered.get_pixel(x, 0)[0] - neutral.get_pixel(x, 0)[0])
        .collect::<Vec<_>>();

    assert_eq!(gains[0], 0);
    assert!(
        gains.windows(2).all(|pair| pair[0] < pair[1]),
        "bitmap alpha did not scale the effect monotonically: {gains:?}"
    );
}

#[test]
fn a_missing_inverted_bitmap_mask_is_inert() {
    let tree = support::TempTree::new("render-missing-bitmap");
    let source = RgbImage::from_pixel(8, 8, Rgb([128, 128, 128]));
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::Bitmap {
                    path: tree.path().join("missing.png").to_string_lossy().into_owned(),
                },
                inverted: true,
                amount: 1.0,
                exposure_ev: 2.0,
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        },
    );

    assert_eq!(delivered, neutral);
}

#[test]
fn radial_feather_produces_intermediate_coverage() {
    let reference = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 64, Rgb([128, 128, 128])));
    let mask = LocalAdjustment {
        mask: MaskGeometry::Radial {
            top: 0.25,
            left: 0.25,
            bottom: 0.75,
            right: 0.75,
            feather: 75.0,
            roundness: 0.0,
            flipped: false,
            angle: 0.0,
            midpoint: 50.0,
            mask_version: 2,
        },
        ..LocalAdjustment::default()
    };

    let coverage =
        autoshade::render::mask_coverage(&mask, &reference, autoshade::render::MaskFrame::AsRendered);
    let values = coverage.pixels().map(|pixel| pixel[0]).collect::<Vec<_>>();

    assert!(values.contains(&0));
    assert!(values.contains(&255));
    assert!(
        values.iter().any(|value| (1..=254).contains(value)),
        "a feathered radial mask contained no transition coverage"
    );
}

#[test]
fn brush_mask_paints_its_dabs_without_covering_far_corners() {
    let reference =
        DynamicImage::ImageRgb8(RgbImage::from_pixel(480, 320, Rgb([128, 128, 128])));
    let mask = LocalAdjustment {
        mask: MaskGeometry::Brush {
            name: "measured brush".to_string(),
            blend_mode: 0,
            value: 1.0,
            inverted: false,
            strokes: vec![BrushStroke {
                value: 0.439815,
                radius: 0.582157,
                flow: 1.0,
                center_weight: 0.0,
                sync_id: "synthetic".to_string(),
                dabs: "r 0.581835\nd 0.100684 0.840004\nr 0.581172\nd 0.213862 0.887261"
                    .to_string(),
            }],
        },
        ..LocalAdjustment::default()
    };

    let coverage = autoshade::render::mask_coverage(
        &mask,
        &reference,
        autoshade::render::MaskFrame::AsRendered,
    );
    for (x, y) in [(48, 268), (102, 283)] {
        assert!(
            coverage.get_pixel(x, y)[0] > 76,
            "brush did not paint its dab at ({x}, {y})"
        );
    }
    for (x, y) in [(0, 0), (475, 6)] {
        assert!(
            coverage.get_pixel(x, y)[0] < 6,
            "brush covered a far corner at ({x}, {y})"
        );
    }
}

#[test]
fn export_with_an_unreadable_active_mask_writes_nothing_and_names_the_mask() {
    let tree = support::TempTree::new("render-unreadable-mask-export");
    let source = tree.path().join("source.png");
    let output = tree.path().join("output.png");
    RgbImage::from_pixel(16, 12, Rgb([96, 112, 128]))
        .save(&source)
        .unwrap();
    let recipe = EditRecipe {
        masks: vec![LocalAdjustment {
            name: "sky".to_string(),
            mask: MaskGeometry::Bitmap {
                path: tree.path().join("missing-mask.png").to_string_lossy().into_owned(),
            },
            amount: 1.0,
            exposure_ev: 1.0,
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };

    let error = autoshade::render::render_to_file(
        &source,
        &recipe,
        &output,
        None,
        None,
        autoshade::diag::silent(),
    )
    .unwrap_err()
    .to_string();

    assert!(error.contains("sky"), "the refusal did not name the lost mask: {error}");
    assert!(!output.exists(), "a refused export left a delivery behind");
}

#[test]
fn profile_vignetting_lifts_corners_without_moving_the_center() {
    let source = RgbImage::from_pixel(120, 80, Rgb([100, 100, 100]));
    let recipe = EditRecipe {
        lens_profile: LensProfile {
            vignette: (0..16)
                .map(|index| 1.0 + 0.42 * (index as f32 / 15.0).powi(2))
                .collect(),
            vignette_on: true,
            ..LensProfile::default()
        },
        ..EditRecipe::default()
    };

    let delivered = render(&source, &recipe);

    assert_eq!(delivered.get_pixel(60, 40)[0], 100);
    assert!(
        delivered.get_pixel(0, 0)[0] > 110,
        "profile vignetting did not lift the corner: {:?}",
        delivered.get_pixel(0, 0)
    );
}

#[test]
fn manual_chromatic_aberration_moves_only_the_red_channel() {
    let source = DynamicImage::ImageRgb16(image::ImageBuffer::from_fn(161, 161, |x, y| {
        let dx = x as f32 - 80.0;
        let dy = y as f32 - 80.0;
        let radius = (dx * dx + dy * dy).sqrt();
        let value = (((radius * 0.7).sin() * 0.5 + 0.5) * 60_000.0) as u16;
        image::Rgb([value; 3])
    }));
    let neutral = EditRecipe::default();
    let shifted = EditRecipe {
        ca_r: -100.0,
        ..EditRecipe::default()
    };
    let before = autoshade::render::apply_lens_geometry(
        &source,
        &autoshade::render::geometry_profile(&neutral),
        0.0,
    )
    .to_rgb16();
    let after = autoshade::render::apply_lens_geometry(
        &source,
        &autoshade::render::geometry_profile(&shifted),
        0.0,
    )
    .to_rgb16();

    assert!(
        after
            .pixels()
            .zip(before.pixels())
            .any(|(after, before)| after[0] != before[0]),
        "manual CA did not move red"
    );
    assert!(
        after
            .pixels()
            .zip(before.pixels())
            .all(|(after, before)| after[1] == before[1] && after[2] == before[2]),
        "manual red CA moved green or blue"
    );
}

#[test]
fn positive_straighten_rotates_the_frame_clockwise() {
    let split = RgbImage::from_fn(100, 100, |x, _| {
        if x < 50 {
            Rgb([255, 0, 0])
        } else {
            Rgb([0, 0, 255])
        }
    });

    let delivered = autoshade::render::rotate_straighten(
        &DynamicImage::ImageRgb8(split),
        10.0,
    )
    .to_rgb8();
    let probe = delivered.get_pixel(delivered.width() / 2 + 3, 1);

    assert!(
        probe[0] > probe[2],
        "positive straighten did not rotate clockwise at the top edge: {probe:?}"
    );
}
