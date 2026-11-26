use autoshade::recipe::{
    BrushStroke, ColorGrade, CreativeLook, Crop, CurvePoint, EditRecipe, Hsl, LensProfile,
    LocalAdjustment, MaskGeometry,
};
use autoshade::render::LensBlur;
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

fn linear_to_oklab_standard(rgb: [f32; 3]) -> [f32; 3] {
    let l = (0.412_221_47 * rgb[0] + 0.536_332_55 * rgb[1] + 0.051_445_995 * rgb[2])
        .max(0.0)
        .cbrt();
    let m = (0.211_903_5 * rgb[0] + 0.680_699_5 * rgb[1] + 0.107_396_96 * rgb[2])
        .max(0.0)
        .cbrt();
    let s = (0.088_302_46 * rgb[0] + 0.281_718_85 * rgb[1] + 0.629_978_7 * rgb[2])
        .max(0.0)
        .cbrt();
    [
        0.210_454_26 * l + 0.793_617_8 * m - 0.004_072_047 * s,
        1.977_998_5 * l - 2.428_592_2 * m + 0.450_593_7 * s,
        0.025_904_037 * l + 0.782_771_77 * m - 0.808_675_77 * s,
    ]
}

fn pixel_oklab(pixel: &Rgb<u8>) -> [f32; 3] {
    linear_to_oklab_standard(pixel.0.map(|channel| {
        srgb_to_linear(f32::from(channel) / 255.0)
    }))
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

fn lens_blur_depth(
    tree: &support::TempTree,
    name: &str,
    width: u32,
    height: u32,
    edge: u32,
) -> std::path::PathBuf {
    let path = tree.path().join(name);
    image::ImageBuffer::from_fn(width, height, |x, _| {
        image::Luma([if x < edge { u16::MAX } else { 0 }])
    })
    .save(&path)
    .unwrap();
    path
}

fn lens_blur_recipe(depth: &std::path::Path, focus: [f32; 2]) -> EditRecipe {
    EditRecipe {
        lens_blur: Some(LensBlur {
            depth_map: depth.to_string_lossy().into_owned(),
            focus,
            amount: 100.0,
            range: 0.0,
            ..LensBlur::default()
        }),
        ..EditRecipe::default()
    }
}

fn checker_frame(width: u32, height: u32) -> RgbImage {
    RgbImage::from_fn(width, height, |x, y| {
        Rgb([if (x + y) % 2 == 0 { 204 } else { 51 }; 3])
    })
}

fn horizontal_texture(image: &RgbImage, columns: std::ops::Range<u32>) -> f64 {
    let mut total = 0.0;
    for y in 0..image.height() {
        for x in columns.clone() {
            total += f64::from(image.get_pixel(x + 1, y)[1].abs_diff(image.get_pixel(x, y)[1]));
        }
    }
    total / f64::from(image.height() * columns.len() as u32)
}

#[test]
fn lens_blur_follows_depth_distance_on_both_sides_of_focus() {
    let tree = support::TempTree::new("lens-blur-focus");
    let depth = lens_blur_depth(&tree, "depth.png", 128, 32, 64);
    let source = checker_frame(128, 32);
    let sharp = horizontal_texture(&source, 2..56);
    let focused_near = render(&source, &lens_blur_recipe(&depth, [0.25, 0.5]));
    let focused_far = render(&source, &lens_blur_recipe(&depth, [0.75, 0.5]));

    assert!((horizontal_texture(&focused_near, 2..56) - sharp).abs() < 1.0);
    assert!(horizontal_texture(&focused_near, 72..125) < sharp * 0.2);
    assert!(horizontal_texture(&focused_far, 2..56) < sharp * 0.2);
    assert!((horizontal_texture(&focused_far, 72..125) - sharp).abs() < 1.0);
}

#[test]
fn disabled_zero_or_unresolved_lens_blur_is_inert() {
    let tree = support::TempTree::new("lens-blur-inert");
    let depth = lens_blur_depth(&tree, "depth.png", 64, 32, 32);
    let source = checker_frame(64, 32);
    let neutral = render(&source, &EditRecipe::default());
    let active = LensBlur {
        depth_map: depth.to_string_lossy().into_owned(),
        amount: 100.0,
        ..LensBlur::default()
    };
    for lens_blur in [
        LensBlur {
            enabled: false,
            ..active.clone()
        },
        LensBlur {
            amount: 0.0,
            ..active.clone()
        },
        LensBlur {
            depth_map: String::new(),
            ..active
        },
    ] {
        let delivered = render(
            &source,
            &EditRecipe {
                lens_blur: Some(lens_blur),
                ..EditRecipe::default()
            },
        );
        assert!(delivered.as_raw() == neutral.as_raw());
    }
}

#[test]
fn focused_subject_does_not_bleed_into_the_blurred_background() {
    let tree = support::TempTree::new("lens-blur-halo");
    let depth = lens_blur_depth(&tree, "depth.png", 128, 16, 64);
    let source = RgbImage::from_fn(128, 16, |x, _| {
        Rgb([if x < 64 { 255 } else { 0 }; 3])
    });

    let delivered = render(&source, &lens_blur_recipe(&depth, [0.25, 0.5]));

    assert!(delivered.get_pixel(65, 8)[0] < 13);
    assert!(delivered.get_pixel(63, 8)[0] >= 250);
}

#[test]
fn lens_blur_scales_with_the_rendered_frame() {
    let tree = support::TempTree::new("lens-blur-scale");
    let depth = lens_blur_depth(&tree, "depth.png", 64, 32, 24);
    let frame = |width: u32, height: u32| {
        RgbImage::from_fn(width, height, |x, _| {
            Rgb([if (x * 16 / width).is_multiple_of(2) {
                179
            } else {
                77
            }; 3])
        })
    };
    let small = render(&frame(512, 256), &lens_blur_recipe(&depth, [0.1, 0.5]));
    let large = render(
        &frame(1024, 512),
        &lens_blur_recipe(&depth, [0.1, 0.5]),
    );
    let reduced = autoshade::render::thumbnail_exact(
        &DynamicImage::ImageRgb8(large),
        512,
        256,
    )
    .to_rgb8();
    let mean = small
        .as_raw()
        .iter()
        .zip(reduced.as_raw())
        .map(|(left, right)| f64::from(left.abs_diff(*right)))
        .sum::<f64>()
        / small.as_raw().len() as f64;

    assert!(mean < 2.0, "scaled lens blur differs by {mean} codes");
}

#[test]
fn develop_loads_depth_and_missing_depth_refuses_export_without_blurring_preview() {
    let tree = support::TempTree::new("lens-blur-depth-loading");
    let depth = lens_blur_depth(&tree, "depth.png", 64, 32, 32);
    let source = checker_frame(64, 32);
    let source_path = tree.path().join("source.png");
    source.save(&source_path).unwrap();
    let plain = render(&source, &EditRecipe::default());
    let active = lens_blur_recipe(&depth, [0.25, 0.5]);
    let blurred = render(&source, &active);

    assert_eq!(blurred.get_pixel(8, 8), plain.get_pixel(8, 8));
    assert_ne!(blurred.get_pixel(56, 8), plain.get_pixel(56, 8));

    let missing = lens_blur_recipe(&tree.path().join("missing.png"), [0.25, 0.5]);
    assert!(render(&source, &missing).as_raw() == plain.as_raw());
    let output = tree.path().join("output.png");
    let error = autoshade::render::render_to_file(
        &source_path,
        &missing,
        &output,
        None,
        None,
        autoshade::diag::silent(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("depth map"), "{error:#}");
    assert!(!output.exists());
}

fn lens_blur_protect(
    tree: &support::TempTree,
    name: &str,
    width: u32,
    height: u32,
    columns: std::ops::Range<u32>,
    level: u8,
) -> std::path::PathBuf {
    let path = tree.path().join(name);
    image::GrayImage::from_fn(width, height, |x, _| {
        image::Luma([if columns.contains(&x) { level } else { 0 }])
    })
    .save(&path)
    .unwrap();
    path
}

#[test]
fn a_black_lens_blur_protect_mask_matches_unprotected_delivery() {
    let tree = support::TempTree::new("lens-blur-black-protect");
    let depth = lens_blur_depth(&tree, "depth.png", 512, 128, 200);
    let black = lens_blur_protect(&tree, "black.png", 512, 128, 0..0, 255);
    let source = checker_frame(512, 128);
    let neutral = render(&source, &EditRecipe::default());
    let unprotected = render(&source, &lens_blur_recipe(&depth, [0.1, 0.5]));
    let mut protected_recipe = lens_blur_recipe(&depth, [0.1, 0.5]);
    protected_recipe.lens_blur.as_mut().unwrap().protect =
        Some(black.to_string_lossy().into_owned());
    let black_protected = render(&source, &protected_recipe);

    assert!(unprotected.as_raw() != neutral.as_raw());
    assert!(black_protected.as_raw() == unprotected.as_raw());
}

#[test]
fn white_and_half_lens_blur_protection_control_sharpness() {
    let tree = support::TempTree::new("lens-blur-protect-levels");
    let depth = lens_blur_depth(&tree, "depth.png", 512, 16, 256);
    let white = lens_blur_protect(&tree, "white.png", 512, 16, 256..512, 255);
    let half = lens_blur_protect(&tree, "half.png", 512, 16, 256..512, 128);
    let source = RgbImage::from_fn(512, 16, |x, _| {
        Rgb([if (x / 8) % 2 == 0 { 204 } else { 51 }; 3])
    });
    let unprotected = render(&source, &lens_blur_recipe(&depth, [0.25, 0.5]));
    let with = |protect: &std::path::Path| {
        let mut recipe = lens_blur_recipe(&depth, [0.25, 0.5]);
        recipe.lens_blur.as_mut().unwrap().protect =
            Some(protect.to_string_lossy().into_owned());
        render(&source, &recipe)
    };
    let half_protected = with(&half);
    let white_protected = with(&white);
    let sharp = horizontal_texture(&source, 330..440);
    let full = horizontal_texture(&unprotected, 330..440);
    let midway = horizontal_texture(&half_protected, 330..440);
    let kept = horizontal_texture(&white_protected, 330..440);

    assert!(full + 1.0 < midway && midway + 1.0 < sharp);
    assert!((kept - sharp).abs() < 1.0);
}

#[test]
fn develop_loads_lens_blur_protect_and_missing_protect_refuses_export() {
    let tree = support::TempTree::new("lens-blur-protect-loading");
    let depth = lens_blur_depth(&tree, "depth.png", 64, 32, 32);
    let protect = lens_blur_protect(&tree, "protect.png", 64, 32, 40..64, 255);
    let source = checker_frame(64, 32);
    let source_path = tree.path().join("source.png");
    source.save(&source_path).unwrap();
    let plain = render(&source, &EditRecipe::default());
    let unprotected = render(&source, &lens_blur_recipe(&depth, [0.25, 0.5]));
    let mut kept_recipe = lens_blur_recipe(&depth, [0.25, 0.5]);
    kept_recipe.lens_blur.as_mut().unwrap().protect =
        Some(protect.to_string_lossy().into_owned());
    let kept = render(&source, &kept_recipe);

    assert_ne!(unprotected.get_pixel(52, 8), plain.get_pixel(52, 8));
    assert_eq!(kept.get_pixel(52, 8), plain.get_pixel(52, 8));
    assert_eq!(kept.get_pixel(34, 8), unprotected.get_pixel(34, 8));

    let mut missing = lens_blur_recipe(&depth, [0.25, 0.5]);
    missing.lens_blur.as_mut().unwrap().protect =
        Some(tree.path().join("missing.png").to_string_lossy().into_owned());
    assert!(render(&source, &missing).as_raw() == plain.as_raw());
    let output = tree.path().join("output.png");
    let error = autoshade::render::render_to_file(
        &source_path,
        &missing,
        &output,
        None,
        None,
        autoshade::diag::silent(),
    )
    .unwrap_err();
    assert!(error.to_string().contains("protect mask"), "{error:#}");
    assert!(!output.exists());
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
fn strong_hsl_band_controls_leave_grey_pixels_unchanged() {
    let source = RgbImage::from_pixel(8, 8, Rgb([128, 128, 128]));
    let neutral = render(&source, &EditRecipe::default());
    let delivered = render(
        &source,
        &EditRecipe {
            hsl: Hsl {
                saturation: [100.0; 8],
                luminance: [100.0; 8],
                ..Hsl::default()
            },
            ..EditRecipe::default()
        },
    );
    let worst = neutral
        .as_raw()
        .iter()
        .zip(delivered.as_raw())
        .map(|(left, right)| left.abs_diff(*right))
        .max()
        .unwrap();

    assert!(worst <= 1, "grey moved by {worst} codes");
}

#[test]
fn hsl_band_controls_do_not_blotch_a_near_grey_sky() {
    let source = RgbImage::from_fn(64, 1, |x, _| {
        if x % 2 == 0 {
            Rgb([181, 182, 185])
        } else {
            Rgb([181, 185, 184])
        }
    });
    let delivered = render(
        &source,
        &EditRecipe {
            hsl: Hsl {
                luminance: [0.0, 0.0, 0.0, 0.0, 60.0, -80.0, 0.0, 0.0],
                ..Hsl::default()
            },
            ..EditRecipe::default()
        },
    );
    let lumas = delivered.pixels().map(luma).collect::<Vec<_>>();
    let spread = lumas.iter().copied().fold(f64::MIN, f64::max)
        - lumas.iter().copied().fold(f64::MAX, f64::min);

    assert!(spread < 10.2, "near-grey sky luma spread is {spread} codes");
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

fn left_half_mask() -> MaskGeometry {
    MaskGeometry::Linear {
        zero_x: 0.5,
        zero_y: 0.0,
        full_x: 0.0,
        full_y: 0.0,
    }
}

fn whole_frame_mask() -> MaskGeometry {
    MaskGeometry::Linear {
        zero_x: 2.0,
        zero_y: 0.0,
        full_x: 1.5,
        full_y: 0.0,
    }
}

#[test]
fn local_hue_rotates_both_directions_without_leaking_outside_the_mask() {
    let source = RgbImage::from_pixel(16, 4, Rgb([204, 64, 51]));
    let neutral = render(&source, &EditRecipe::default());
    let shifted = |hue| {
        render(
            &source,
            &EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: left_half_mask(),
                    amount: 1.0,
                    hue,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            },
        )
    };
    let positive = shifted(100.0);
    let negative = shifted(-100.0);

    assert!(positive.get_pixel(0, 0)[1] > neutral.get_pixel(0, 0)[1] + 10);
    assert!(negative.get_pixel(0, 0)[2] > neutral.get_pixel(0, 0)[2] + 5);
    assert_eq!(positive.get_pixel(15, 0), neutral.get_pixel(15, 0));
    assert_eq!(negative.get_pixel(15, 0), neutral.get_pixel(15, 0));
}

fn detail_edge_energy(image: &RgbImage, columns: std::ops::Range<u32>) -> f64 {
    let mut total = 0.0;
    for y in 0..image.height() {
        for x in columns.clone() {
            total += (luma(image.get_pixel(x + 1, y)) - luma(image.get_pixel(x, y))).abs();
        }
    }
    total
}

#[test]
fn local_sharpness_strengthens_and_softens_only_inside_the_mask() {
    let source = RgbImage::from_fn(64, 48, |x, y| {
        let coarse = if (x / 8 + y / 8) % 2 == 0 { 20 } else { -20 };
        let fine = if (x + y) % 2 == 0 { 8 } else { -8 };
        let value = (115 + coarse + fine) as u8;
        Rgb([value, value, value])
    });
    let neutral = render(&source, &EditRecipe::default());
    let shifted = |sharpness| {
        render(
            &source,
            &EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: left_half_mask(),
                    amount: 1.0,
                    sharpness,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            },
        )
    };
    let sharpened = shifted(100.0);
    let softened = shifted(-100.0);
    let base_left = detail_edge_energy(&neutral, 0..16);
    let base_right = detail_edge_energy(&neutral, 48..63);

    assert!(detail_edge_energy(&sharpened, 0..16) > base_left * 1.01);
    assert!(detail_edge_energy(&softened, 0..16) < base_left * 0.99);
    assert!((detail_edge_energy(&sharpened, 48..63) - base_right).abs() < 1.0);
    assert!((detail_edge_energy(&softened, 48..63) - base_right).abs() < 1.0);
}

#[test]
fn mask_tint_is_inert_at_zero_and_moves_only_covered_pixels() {
    let source = RgbImage::from_pixel(16, 3, Rgb([153, 153, 153]));
    let neutral = render(&source, &EditRecipe::default());
    let parked_adjustment = LocalAdjustment {
        mask: left_half_mask(),
        amount: 1.0,
        toning_hue: 231.0,
        toning_amount: 0.0,
        ..LocalAdjustment::default()
    };
    let active_adjustment = LocalAdjustment {
        toning_amount: 50.0,
        ..parked_adjustment.clone()
    };
    assert!(!autoshade::render::engine_active(&parked_adjustment));
    assert!(autoshade::render::engine_active(&active_adjustment));
    let tinted = |amount| {
        render(
            &source,
            &EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: left_half_mask(),
                    amount: 1.0,
                    toning_hue: 231.0,
                    toning_amount: amount,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            },
        )
    };
    let parked = tinted(0.0);
    let active = tinted(50.0);

    assert!(parked.as_raw() == neutral.as_raw());
    assert!(active.get_pixel(0, 0)[2] > active.get_pixel(0, 0)[0] + 7);
    assert_eq!(active.get_pixel(15, 0), neutral.get_pixel(15, 0));
}

#[test]
fn mask_tint_preserves_oklab_lightness_and_uses_a_midlight_window() {
    let source = RgbImage::from_raw(
        3,
        1,
        vec![5, 5, 5, 120, 120, 120, 250, 250, 250],
    )
    .unwrap();
    let delivered = render(
        &source,
        &EditRecipe {
            masks: vec![LocalAdjustment {
                mask: whole_frame_mask(),
                amount: 1.0,
                toning_hue: 231.0,
                toning_amount: 50.0,
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        },
    );
    let shifts = (0..3)
        .map(|x| {
            let before = pixel_oklab(source.get_pixel(x, 0));
            let after = pixel_oklab(delivered.get_pixel(x, 0));
            assert!((after[0] - before[0]).abs() < 0.01);
            (after[1] - before[1]).hypot(after[2] - before[2])
        })
        .collect::<Vec<_>>();

    assert!(shifts[1] > shifts[0] * 2.0);
    assert!(shifts[1] > shifts[2] * 2.0);
}

#[test]
fn mask_tint_follows_the_public_oklab_wheel() {
    for (hue, expected_angle) in [
        (0.0, 12.0),
        (60.0, 100.0),
        (120.0, 155.3),
        (180.0, 190.5),
        (240.0, 276.6),
        (300.0, 336.1),
    ] {
        let source = RgbImage::from_pixel(1, 1, Rgb([120; 3]));
        let delivered = render(
            &source,
            &EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: whole_frame_mask(),
                    amount: 1.0,
                    toning_hue: hue,
                    toning_amount: 50.0,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            },
        );
        let before = pixel_oklab(source.get_pixel(0, 0));
        let after = pixel_oklab(delivered.get_pixel(0, 0));
        let angle = (after[2] - before[2])
            .atan2(after[1] - before[1])
            .to_degrees()
            .rem_euclid(360.0);
        let error = (angle - expected_angle + 180.0).rem_euclid(360.0) - 180.0;

        assert!(error.abs() < 6.0, "hue {hue}: {angle} vs {expected_angle}");
    }
}

fn cielab(pixel: &Rgb<u8>) -> [f32; 3] {
    let [r, g, b] = pixel.0.map(|value| srgb_to_linear(f32::from(value) / 255.0));
    let f = |value: f32| {
        if value > 0.008_856 {
            value.cbrt()
        } else {
            7.787 * value + 16.0 / 116.0
        }
    };
    let x = f((0.412_456_4 * r + 0.357_576_1 * g + 0.180_437_5 * b) / 0.950_47);
    let y = f(0.212_672_9 * r + 0.715_152_2 * g + 0.072_175 * b);
    let z = f((0.019_333_9 * r + 0.119_192 * g + 0.950_304_1 * b) / 1.088_83);
    [116.0 * y - 16.0, 500.0 * (x - y), 200.0 * (y - z)]
}

fn mean_delta_e(left: &RgbImage, right: &RgbImage) -> f32 {
    left.pixels()
        .zip(right.pixels())
        .map(|(left, right)| {
            let (left, right) = (cielab(left), cielab(right));
            ((left[0] - right[0]).powi(2)
                + (left[1] - right[1]).powi(2)
                + (left[2] - right[2]).powi(2))
            .sqrt()
        })
        .sum::<f32>()
        / (left.width() * left.height()) as f32
}

#[test]
#[ignore = "requires AUTOSHADE_FIT_CALIBRATION_DIR/tint Lightroom and Camera Raw exports"]
fn real_mask_tint_matches_lightroom_and_camera_raw_exports() {
    let root = std::env::var_os("AUTOSHADE_FIT_CALIBRATION_DIR")
        .map(std::path::PathBuf::from)
        .expect("set AUTOSHADE_FIT_CALIBRATION_DIR to the approved calibration corpus");
    let directory = root.join("tint");
    let load = |name: &str| {
        let path = directory.join(name);
        assert!(path.is_file(), "{} is missing", path.display());
        let image = image::open(&path).unwrap_or_else(|error| panic!("{}: {error}", path.display()));
        let height = (image.height() as f32 * 1200.0 / image.width() as f32).round() as u32;
        image
            .resize_exact(1200, height, image::imageops::FilterType::Lanczos3)
            .to_rgb8()
    };
    let flat = load("p39-flat.jpg");
    let tinted = |base: &RgbImage, hue, amount| {
        render(
            base,
            &EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: whole_frame_mask(),
                    amount: 1.0,
                    toning_hue: hue,
                    toning_amount: amount,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            },
        )
    };
    for (name, amount, untinted_floor, ceiling) in [
        ("p39-tint-073.jpg", 78.2609, 10.0, 2.2),
        ("p39-tint-035.jpg", 39.1304, 5.0, 1.5),
    ] {
        let expected = load(name);
        assert_eq!(expected.dimensions(), flat.dimensions());
        assert!(mean_delta_e(&flat, &expected) >= untinted_floor);
        assert!(mean_delta_e(&tinted(&flat, 210.0, amount), &expected) <= ceiling);
    }

    let none = load("acr-none.jpg");
    for (name, hue, amount, ceiling) in [
        ("acr-hue000.jpg", 0.0, 50.0, 2.0),
        ("acr-hue060.jpg", 60.0, 50.0, 2.0),
        ("acr-hue120.jpg", 120.0, 50.0, 2.0),
        ("acr-hue180.jpg", 180.0, 50.0, 2.0),
        ("acr-hue240.jpg", 240.0, 50.0, 2.0),
        ("acr-hue300.jpg", 300.0, 50.0, 2.0),
        ("acr-sat025.jpg", 210.0, 25.0, 1.2),
        ("acr-sat100.jpg", 210.0, 100.0, 2.6),
    ] {
        let expected = load(name);
        assert_eq!(expected.dimensions(), none.dimensions());
        assert!(mean_delta_e(&none, &expected) >= 3.0);
        assert!(mean_delta_e(&tinted(&none, hue, amount), &expected) <= ceiling);
    }
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

fn brush_group(strokes: &[(f32, f32, f32, f32, &str)]) -> MaskGeometry {
    MaskGeometry::Brush {
        name: "Brush 1".to_string(),
        blend_mode: 0,
        value: 1.0,
        inverted: false,
        strokes: strokes
            .iter()
            .map(|&(value, radius, flow, center_weight, dabs)| BrushStroke {
                value,
                radius,
                flow,
                center_weight,
                sync_id: "synthetic".to_string(),
                dabs: dabs.to_string(),
            })
            .collect(),
    }
}

fn brushed_recipe(radius: f32, dabs: &str) -> EditRecipe {
    EditRecipe {
        masks: vec![LocalAdjustment {
            mask: brush_group(&[(1.0, radius, 1.0, 0.0, dabs)]),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    }
}

fn only_stream(recipe: &EditRecipe) -> &str {
    let MaskGeometry::Brush { strokes, .. } = &recipe.masks[0].mask else {
        panic!("a brush")
    };
    &strokes[0].dabs
}

fn only_radius(recipe: &EditRecipe) -> f32 {
    let MaskGeometry::Brush { strokes, .. } = &recipe.masks[0].mask else {
        panic!("a brush")
    };
    strokes[0].radius
}

fn probe_frame() -> Option<autoshade::render::CoordFrame> {
    autoshade::render::CoordFrame::new(9504.0, 6336.0)
}

fn brush_coverage(mask: &MaskGeometry, width: u32, height: u32) -> image::GrayImage {
    let reference = DynamicImage::ImageRgb8(RgbImage::from_pixel(width, height, Rgb([128; 3])));
    autoshade::render::mask_coverage(
        &LocalAdjustment {
            mask: mask.clone(),
            ..LocalAdjustment::default()
        },
        &reference,
        autoshade::render::MaskFrame::AsRendered,
    )
}

#[test]
fn a_quarter_turn_rewrites_the_dab_stream_and_rescales_its_radii() {
    const SEED: &str =
        "r 0.200000\nf 0.500000\nh 1.000000\nd 0.100000 0.800000\nd 0.300000 0.400000";
    for (orientation, want_stream, want_radius) in [
        (
            Orientation::Rotate90,
            "r 0.300000\nf 0.500000\nh 1.000000\nd 0.200000 0.100000\nd 0.600000 0.300000",
            0.375f32,
        ),
        (
            Orientation::Rotate180,
            "r 0.200000\nf 0.500000\nh 1.000000\nd 0.900000 0.200000\nd 0.700000 0.600000",
            0.25,
        ),
        (
            Orientation::Rotate270,
            "r 0.300000\nf 0.500000\nh 1.000000\nd 0.800000 0.900000\nd 0.400000 0.700000",
            0.375,
        ),
    ] {
        let mut recipe = brushed_recipe(0.25, SEED);
        assert!(autoshade::render::orient_recipe_coords(
            &mut recipe,
            orientation,
            probe_frame()
        ));
        assert_eq!(only_stream(&recipe), want_stream, "{orientation:?}");
        assert!((only_radius(&recipe) - want_radius).abs() < 1e-6, "{orientation:?}");
    }

    let mut recipe = brushed_recipe(0.25, SEED);
    let portrait = autoshade::render::CoordFrame::new(6336.0, 9504.0);
    for k in 0..4 {
        let frame = if k % 2 == 0 { probe_frame() } else { portrait };
        assert!(autoshade::render::orient_recipe_coords(
            &mut recipe,
            Orientation::Rotate90,
            frame
        ));
    }
    assert!((only_radius(&recipe) - 0.25).abs() < 1e-6);
    for (got, want) in only_stream(&recipe).split('\n').zip(SEED.split('\n')) {
        let numbers = |token: &str| {
            token
                .split_whitespace()
                .skip(1)
                .map(|value| value.parse::<f32>().unwrap())
                .collect::<Vec<_>>()
        };
        assert_eq!(got.split_whitespace().next(), want.split_whitespace().next());
        for (a, b) in numbers(got).into_iter().zip(numbers(want)) {
            assert!((a - b).abs() < 1e-5, "a full circle moved a token: {got} vs {want}");
        }
    }
}

#[test]
fn an_unturned_photo_keeps_every_dab_byte() {
    const RAW_FORM: &str = "r .5\nd 0 0\nd 1 1";
    for orientation in [Orientation::Normal, Orientation::Unknown] {
        let mut recipe = brushed_recipe(0.25, RAW_FORM);
        let before = recipe.clone();
        assert!(!autoshade::render::orient_recipe_coords(
            &mut recipe,
            orientation,
            probe_frame()
        ));
        assert_eq!(recipe, before, "{orientation:?} must not touch a dab byte");
    }

    let mut recipe = brushed_recipe(0.25, RAW_FORM);
    recipe.crop = Some(Crop {
        left: 0.1,
        top: 0.2,
        right: 0.8,
        bottom: 0.9,
    });
    assert!(autoshade::render::orient_recipe_coords(
        &mut recipe,
        Orientation::Rotate90,
        None
    ));
    assert_eq!(only_stream(&recipe), RAW_FORM, "no frame, no rewrite");
    assert!((only_radius(&recipe) - 0.25).abs() < 1e-9);
    assert!(recipe.crop.unwrap().left != 0.1);
}

#[test]
fn a_turned_dab_lands_where_the_turned_radial_beside_it_does() {
    let (px, py) = (0.30f32, 0.65f32);
    let radial = MaskGeometry::Radial {
        top: py - 0.05,
        left: px - 0.12,
        bottom: py + 0.05,
        right: px + 0.12,
        feather: 0.5,
        roundness: 0.0,
        flipped: false,
        angle: 0.0,
        midpoint: 50.0,
        mask_version: 2,
    };
    for orientation in [
        Orientation::Rotate90,
        Orientation::Rotate180,
        Orientation::Rotate270,
        Orientation::Transpose,
        Orientation::HorizontalFlip,
    ] {
        let mut recipe = EditRecipe {
            masks: vec![
                LocalAdjustment {
                    mask: brush_group(&[(1.0, 0.1, 1.0, 0.0, &format!("d {px} {py}"))]),
                    ..LocalAdjustment::default()
                },
                LocalAdjustment {
                    mask: radial.clone(),
                    ..LocalAdjustment::default()
                },
            ],
            ..EditRecipe::default()
        };
        assert!(autoshade::render::orient_recipe_coords(
            &mut recipe,
            orientation,
            probe_frame()
        ));
        let dab: Vec<f32> = only_stream(&recipe)
            .split_whitespace()
            .skip(1)
            .map(|value| value.parse::<f32>().unwrap())
            .collect();
        let MaskGeometry::Radial { top, left, bottom, right, .. } = recipe.masks[1].mask else {
            panic!("a radial")
        };
        let (cx, cy) = ((left + right) / 2.0, (top + bottom) / 2.0);
        assert!(
            (dab[0] - cx).abs() < 1e-6 && (dab[1] - cy).abs() < 1e-6,
            "{orientation:?}: the dab landed at ({}, {}) and the radial at ({cx}, {cy})",
            dab[0],
            dab[1]
        );
    }
}

#[test]
fn a_turned_dab_is_still_a_circle_in_pixels() {
    let (fw, fh) = (480u32, 320u32);
    let mut recipe = brushed_recipe(0.1, "h 1.000000\nd 0.500000 0.500000");
    let flat = |coverage: &image::GrayImage, w: u32, h: u32| {
        (
            f32::from(coverage.get_pixel(w / 2 + 40, h / 2)[0]) / 255.0,
            f32::from(coverage.get_pixel(w / 2, h / 2 + 40)[0]) / 255.0,
        )
    };
    let before = brush_coverage(&recipe.masks[0].mask, fw, fh);
    let (bx, by) = flat(&before, fw, fh);
    assert!(bx > 0.5 && (bx - by).abs() < 0.05, "premise: the dab starts round ({bx}, {by})");

    assert!(autoshade::render::orient_recipe_coords(
        &mut recipe,
        Orientation::Rotate90,
        autoshade::render::CoordFrame::new(480.0, 320.0)
    ));
    let after = brush_coverage(&recipe.masks[0].mask, fh, fw);
    let (ax, ay) = flat(&after, fh, fw);
    assert!(ax > 0.5 && (ax - ay).abs() < 0.05, "the turned dab must still be round ({ax}, {ay})");
    assert!(
        (ax - bx).abs() < 0.05 && (ay - by).abs() < 0.05,
        "and the same size: was ({bx}, {by}), now ({ax}, {ay})"
    );
}

#[test]
fn the_develop_hands_the_brush_arm_its_raster() {
    let (w, h) = (240u32, 160u32);
    let mask = LocalAdjustment {
        mask: brush_group(&[(1.0, 0.2, 1.0, 1.0, "d 0.5 0.5")]),
        exposure_ev: -3.0,
        ..LocalAdjustment::default()
    };
    let source = RgbImage::from_pixel(w, h, Rgb([128, 128, 128]));
    let delivered = render(
        &source,
        &EditRecipe {
            masks: vec![mask.clone()],
            ..EditRecipe::default()
        },
    );

    assert!(delivered.get_pixel(w / 2, h / 2)[1] < 51, "the dab centre must darken");
    assert_eq!(delivered.get_pixel(2, 2)[1], 128, "the far corner must not move");

    let coverage = brush_coverage(&mask.mask, w, h);
    assert!(coverage.get_pixel(w / 2, h / 2)[0] > 200, "the overlay must show the coverage");
    assert_eq!(coverage.get_pixel(2, 2)[0], 0, "and none where the render applied none");
}

/// The kernel against the measured nine-rung table (R29 Batch-6 §4.1), read
/// texel by texel through the public coverage overlay on a 1:1 raster. The
/// tolerances are the report's own: pooled rms 0.012, worst cell 0.035.
#[test]
fn brush_kernel_reproduces_the_measured_nine_rungs() {
    const HS: [f32; 9] = [0.0, 0.125, 0.25, 0.375, 0.5, 0.625, 0.75, 0.875, 1.0];
    const TABLE: [(f32, [f32; 9]); 11] = [
        (0.092, [0.9350, 0.9643, 0.9814, 0.9908, 0.9959, 0.9975, 0.9972, 0.9977, 0.9973]),
        (0.193, [0.7326, 0.8494, 0.9235, 0.9657, 0.9869, 0.9948, 0.9975, 0.9981, 0.9979]),
        (0.292, [0.4790, 0.6831, 0.8321, 0.9232, 0.9710, 0.9904, 0.9980, 0.9991, 0.9989]),
        (0.393, [0.2646, 0.5000, 0.7138, 0.8630, 0.9467, 0.9840, 0.9964, 0.9992, 0.9995]),
        (0.492, [0.1302, 0.3359, 0.5775, 0.7794, 0.9073, 0.9697, 0.9935, 0.9989, 0.9999]),
        (0.593, [0.0645, 0.2084, 0.4298, 0.6609, 0.8353, 0.9360, 0.9806, 0.9948, 0.9975]),
        (0.693, [0.0339, 0.1244, 0.2875, 0.5030, 0.7102, 0.8603, 0.9461, 0.9842, 0.9966]),
        (0.792, [0.0151, 0.0685, 0.1671, 0.3153, 0.4978, 0.6772, 0.8207, 0.9159, 0.9679]),
        (0.893, [0.0023, 0.0232, 0.0672, 0.1366, 0.2316, 0.3494, 0.4832, 0.6214, 0.7505]),
        (0.943, [0.0002, 0.0063, 0.0230, 0.0522, 0.0956, 0.1530, 0.2249, 0.3109, 0.4094]),
        (0.972, [-0.0007, 0.0007, 0.0046, 0.0126, 0.0246, 0.0424, 0.0656, 0.0954, 0.1318]),
    ];
    let (fw, fh) = (240u32, 160u32);
    let (mut sq, mut cells) = (0.0f64, 0usize);
    let mut measured_kernel = [[0.0f32; 9]; 11];
    // Each table row gets its own dab radius so the probed texel sits
    // exactly on that row's ρ: on the h = 1 edge the kernel falls ~0.28 per
    // 0.03 of ρ, so a half-texel miss would read as a 0.03 error on its own.
    let kernel_at = |rho: f32, hardness: f32| {
        let d = (rho * 60.0 - 0.5).round();
        let radius_texels = (d + 0.5) / rho;
        let coverage = brush_coverage(
            &brush_group(&[(1.0, radius_texels / 240.0, 1.0, hardness, "d 0.5 0.5")]),
            fw,
            fh,
        );
        f32::from(coverage.get_pixel(120 + d as u32, 80)[0]) / 255.0
    };
    for (row_index, (rho, row)) in TABLE.into_iter().enumerate() {
        for (h_index, hardness) in HS.into_iter().enumerate() {
            let got = kernel_at(rho, hardness);
            measured_kernel[row_index][h_index] = got;
            let delta = got - row[h_index];
            sq += f64::from(delta) * f64::from(delta);
            cells += 1;
            assert!(
                delta.abs() <= 0.035,
                "k({rho}, {hardness}) = {got}, measured {} (B6 §4.1)",
                row[h_index]
            );
        }
    }
    for hardness in HS {
        let core = brush_coverage(
            &brush_group(&[(1.0, 0.25, 1.0, hardness, "d 0.5 0.5")]),
            fw,
            fh,
        );
        assert_eq!(core.get_pixel(120, 80)[0], 255, "k(0) = 1 at h = {hardness}");
        for rho in [1.0f32, 1.002, 1.042] {
            assert_eq!(kernel_at(rho, hardness), 0.0, "k({rho}) = 0 at h = {hardness}");
        }
    }
    assert_eq!(cells, 99);
    let rms = (sq / cells as f64).sqrt();
    assert!(rms <= 0.012, "pooled rms {rms} against B6 §4.4's own 0.01020");

    for (row_index, (rho, _)) in TABLE.into_iter().enumerate() {
        for h_index in 1..9 {
            assert!(
                measured_kernel[row_index][h_index] >= measured_kernel[row_index][h_index - 1] - 1e-6,
                "k is not monotone in h at ρ = {rho}"
            );
        }
    }
}

/// The flow law against the measured deposit (R29 Batch-6 §5.3), read at the
/// dab core where the kernel is 1 and the alpha is the deposit itself.
#[test]
fn brush_flow_law_matches_the_measured_deposit() {
    const LADDER: [(f32, f32); 4] =
        [(0.10, 0.01307), (0.25, 0.03935), (0.50, 0.11182), (0.75, 0.27937)];
    let (mut odds_sq, mut lin_sq) = (0.0f64, 0.0f64);
    for (flow, measured) in LADDER {
        let coverage = brush_coverage(&brush_group(&[(1.0, 0.25, flow, 0.5, "d 0.5 0.5")]), 480, 320);
        let deposit = f32::from(coverage.get_pixel(240, 160)[0]) / 255.0;
        assert!(
            (deposit - measured).abs() <= 0.0025 + 0.5 / 255.0,
            "D({flow}) = {deposit}, measured {measured} (B6 §5.3)"
        );
        odds_sq += f64::from(deposit - measured).powi(2);
        lin_sq += f64::from(0.31252 * flow - measured).powi(2);
    }
    let at_core = |flow| {
        let coverage = brush_coverage(&brush_group(&[(1.0, 0.25, flow, 0.5, "d 0.5 0.5")]), 480, 320);
        f32::from(coverage.get_pixel(240, 160)[0]) / 255.0
    };
    assert!(at_core(1.0) >= 1.0 - 1.0 / 255.0, "D(1) must be 1");
    assert_eq!(at_core(0.0), 0.0, "a flow-0 dab deposits nothing");
    assert_eq!(at_core(-1.0), 0.0);
    assert!(at_core(7.0) >= 1.0 - 1.0 / 255.0);
    let (odds, linear) = ((odds_sq / 4.0).sqrt(), (lin_sq / 4.0).sqrt());
    assert!(linear > odds * 10.0, "odds {odds}, linear {linear}");
}

#[test]
fn brush_dabs_accumulate_by_screen_not_sum_or_max() {
    let (fw, fh) = (480u32, 320u32);
    let two = brush_coverage(
        &brush_group(&[(1.0, 0.25, 0.75, 0.5, "d 0.4 0.5\nd 0.6 0.5")]),
        fw,
        fh,
    );
    let one = brush_coverage(
        &brush_group(&[(1.0, 0.25, 0.75, 0.5, "d 0.5 0.5")]),
        fw,
        fh,
    );
    let got = f32::from(two.get_pixel(240, 160)[0]) / 255.0;
    let a = f32::from(one.get_pixel(192, 160)[0]) / 255.0;
    let screen = 1.0 - (1.0 - a) * (1.0 - a);
    let sum = (a + a).min(1.0);

    assert!((got - screen).abs() <= 0.006, "α = {got}, screen says {screen}");
    assert!((got - sum).abs() >= 0.05, "sum-clamp would say {sum}");
    assert!((got - a).abs() >= 0.15, "max would say {a}");
}

#[test]
fn a_brush_dab_has_a_hard_support_edge_and_is_round_in_pixels() {
    let (fw, fh) = (480u32, 320u32);
    let coverage = brush_coverage(&brush_group(&[(0.7, 0.25, 1.0, 0.5, "d 0.5 0.5")]), fw, fh);
    assert_eq!(coverage.dimensions(), (fw, fh));
    let texel = |x: u32, y: u32| f32::from(coverage.get_pixel(x, y)[0]) / 255.0;

    assert!(texel(359, 160) > 0.0, "texel 359 is inside the support");
    assert_eq!(texel(360, 160), 0.0, "support ends EXACTLY at ρ = 1 (B6 §4.1)");
    let horizontal = texel(347, 159);
    let vertical = texel(239, 267);
    assert_eq!(horizontal, vertical, "the dab is not round in pixels");
    assert!(horizontal > 0.0, "premise: both probes are inside the dab");

    let full = brush_coverage(&brush_group(&[(1.0, 0.25, 1.0, 0.5, "d 0.5 0.5")]), fw, fh);
    let ratio = texel(300, 160) / (f32::from(full.get_pixel(300, 160)[0]) / 255.0);
    assert!((ratio - 0.7).abs() < 0.01, "density must scale the alpha: {ratio}");
}

#[test]
fn a_brush_group_with_no_drawable_dab_is_inert() {
    let (fw, fh) = (240u32, 160u32);
    for (what, stream, radius, flow) in [
        ("state tokens only", "r 0.2\nf 1\nh 0.5", 0.2f32, 1.0f32),
        ("zero radius", "d 0.5 0.5", 0.0, 1.0),
        ("zero flow", "d 0.5 0.5", 0.2, 0.0),
        ("garbage", "wobble\nd\nd 0.5", 0.2, 1.0),
    ] {
        let group = brush_group(&[(1.0, radius, flow, 0.5, stream)]);
        let coverage = brush_coverage(&group, fw, fh);
        assert!(coverage.pixels().all(|pixel| pixel[0] == 0), "{what} must draw nothing");

        let MaskGeometry::Brush { name, blend_mode, value, strokes, .. } = group else {
            unreachable!()
        };
        let flipped = MaskGeometry::Brush {
            name,
            blend_mode,
            value,
            inverted: true,
            strokes,
        };
        let source = RgbImage::from_pixel(fw, fh, Rgb([128; 3]));
        let delivered = render(
            &source,
            &EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: flipped,
                    exposure_ev: -3.0,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            },
        );
        assert_eq!(delivered.get_pixel(2, 2)[1], 128, "{what}: an inverted empty group covered the frame");
    }
    assert!(brush_coverage(&brush_group(&[]), fw, fh)
        .pixels()
        .all(|pixel| pixel[0] == 0));
}

#[test]
fn a_brush_groups_own_inversion_is_rendered() {
    let (fw, fh) = (480u32, 320u32);
    let plain = brush_group(&[(1.0, 0.25, 1.0, 1.0, "d 0.5 0.5")]);
    let MaskGeometry::Brush { name, blend_mode, value, strokes, .. } = plain.clone() else {
        unreachable!()
    };
    let inverted = MaskGeometry::Brush {
        name,
        blend_mode,
        value,
        inverted: true,
        strokes,
    };
    let plain_coverage = brush_coverage(&plain, fw, fh);
    let inverted_coverage = brush_coverage(&inverted, fw, fh);
    assert_eq!(inverted_coverage.dimensions(), (fw, fh));
    for (x, y) in [(240u32, 160u32), (264, 160), (10, 6)] {
        let w = f32::from(plain_coverage.get_pixel(x, y)[0]) / 255.0;
        let f = f32::from(inverted_coverage.get_pixel(x, y)[0]) / 255.0;
        assert!((w + f - 1.0).abs() <= 1.0 / 255.0, "({x}, {y}): {w} + {f} != 1");
    }
}

fn unresolved_ai_mask(inverted: bool) -> LocalAdjustment {
    LocalAdjustment {
        name: "Sky".to_string(),
        inverted,
        exposure_ev: 3.0,
        mask: MaskGeometry::AiMask {
            name: "Sky 1".to_string(),
            subtype: 2,
            ref_x: 0.5,
            ref_y: 0.3,
            blend_mode: 0,
            value: 1.0,
            inverted: false,
            mask_version: 1,
            provenance: Vec::new(),
            gesture: Vec::new(),
            raster: None,
        },
        ..LocalAdjustment::default()
    }
}

fn without_recipe_payload(mut xmp: String) -> String {
    for name in ["asr:Recipe", "asr:RecipeCrc32"] {
        let marker = format!(" {name}=\"");
        while let Some(start) = xmp.find(&marker) {
            let value = start + marker.len();
            let end = value + xmp[value..].find('"').unwrap() + 1;
            xmp.replace_range(start..end, "");
        }
    }
    xmp
}

#[test]
fn export_depth_follows_the_option_not_the_extension() {
    let tree = support::TempTree::new("export-depth");
    let source = tree.path().join("in.png");
    RgbImage::from_fn(12, 8, |x, y| Rgb([(x * 20) as u8, (y * 30) as u8, 90]))
        .save(&source)
        .unwrap();
    for (name, eight_bit, want8) in [
        ("d16.png", false, false),
        ("d8.png", true, true),
        ("d16.tif", false, false),
        ("d8.tif", true, true),
    ] {
        let output = tree.path().join(name);
        autoshade::render::render_to_file(
            &source,
            &EditRecipe::default(),
            &output,
            None,
            Some(&autoshade::render::ExportOpts {
                eight_bit,
                ..autoshade::render::ExportOpts::default()
            }),
            autoshade::diag::silent(),
        )
        .unwrap();
        let delivered = image::open(&output).unwrap();
        let got8 = matches!(
            delivered.color(),
            image::ColorType::Rgb8 | image::ColorType::Rgba8
        );
        assert_eq!(got8, want8, "{name}: {:?}", delivered.color());
    }
}

#[test]
fn an_unresolved_ai_mask_skips_its_adjustment_instead_of_covering_the_frame() {
    let unresolved = unresolved_ai_mask(false);
    assert!(autoshade::render::is_raster_backed(&unresolved.mask));
    assert!(autoshade::render::geometry_raster_path(&unresolved.mask).is_none());

    let reference = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([100; 3])));
    let neutral = render(
        &RgbImage::from_pixel(8, 8, Rgb([100; 3])),
        &EditRecipe::default(),
    );
    for inverted in [false, true] {
        let coverage = autoshade::render::mask_coverage(
            &unresolved_ai_mask(inverted),
            &reference,
            autoshade::render::MaskFrame::AsRendered,
        );
        assert!(coverage.pixels().all(|pixel| pixel[0] == 0), "inverted={inverted}");
        let delivered = render(
            &RgbImage::from_pixel(8, 8, Rgb([100; 3])),
            &EditRecipe {
                masks: vec![unresolved_ai_mask(inverted)],
                ..EditRecipe::default()
            },
        );
        assert!(
            delivered.as_raw() == neutral.as_raw(),
            "inverted={inverted}: an unresolved AI mask moved the frame"
        );
    }

    let tree = support::TempTree::new("ai-mask-resolved");
    let alpha = tree.path().join("alpha.png");
    image::GrayImage::from_pixel(4, 4, image::Luma([255]))
        .save(&alpha)
        .unwrap();
    let mut resolved = unresolved_ai_mask(false);
    let MaskGeometry::AiMask { raster, .. } = &mut resolved.mask else {
        unreachable!()
    };
    *raster = Some(alpha.to_string_lossy().into_owned());
    assert_eq!(
        autoshade::render::geometry_raster_path(&resolved.mask),
        Some(alpha.to_string_lossy().as_ref())
    );
    let coverage = autoshade::render::mask_coverage(
        &resolved,
        &reference,
        autoshade::render::MaskFrame::AsRendered,
    );
    assert!(
        coverage.pixels().all(|pixel| pixel[0] == 255),
        "a resolved alpha must cover the frame"
    );
}

#[test]
fn a_zone_ai_mask_renders_exactly_like_the_bitmap_it_replaced() {
    let tree = support::TempTree::new("zone-ai-render");
    let alpha = tree.path().join("mask-zone-sky.png");
    image::GrayImage::from_fn(16, 16, |_, y| {
        image::Luma([match y {
            0..=5 => 255,
            6 => 160,
            7 => 64,
            _ => 0,
        }])
    })
    .save(&alpha)
    .unwrap();
    let path = alpha.to_string_lossy().into_owned();
    let source = RgbImage::from_fn(24, 16, |x, y| {
        Rgb([40 + (x * 7 % 180) as u8, 60 + (y * 5 % 160) as u8, 120])
    });
    let plain = render(&source, &EditRecipe::default());
    for (inverted, role) in [
        (false, autoshade::recipe::MaskRole::ZoneSky),
        (true, autoshade::recipe::MaskRole::ZoneLand),
    ] {
        let zone = |mask: MaskGeometry| {
            EditRecipe {
                masks: vec![LocalAdjustment {
                    mask,
                    role,
                    inverted,
                    exposure_ev: -0.7,
                    saturation: 18.0,
                    color_gains: Some([1.12, 0.98, 0.87]),
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            }
        };
        let was = render(&source, &zone(MaskGeometry::Bitmap { path: path.clone() }));
        let now = render(
            &source,
            &zone(MaskGeometry::select_sky(0.5, 0.2, false, path.clone())),
        );
        assert!(
            was.as_raw() == now.as_raw(),
            "inverted={inverted}: the Select Sky carrier must render the bitmap's pixels"
        );
        assert!(plain.as_raw() != now.as_raw(), "inverted={inverted}: the zone must change the render");
    }
    let arm = |inverted: bool| {
        render(
            &source,
            &EditRecipe {
                masks: vec![LocalAdjustment {
                    mask: MaskGeometry::select_sky(0.5, 0.2, false, path.clone()),
                    role: autoshade::recipe::MaskRole::ZoneSky,
                    inverted,
                    exposure_ev: -0.7,
                    ..LocalAdjustment::default()
                }],
                ..EditRecipe::default()
            },
        )
    };
    assert!(arm(false).as_raw() != arm(true).as_raw(), "the inversion must reach the pixels");
}

#[test]
fn ai_mask_import_preserves_authored_inversion_and_native_edits_win() {
    let tree = support::TempTree::new("ai-import-inversion");
    let alpha = tree.path().join("alpha.png");
    image::GrayImage::from_fn(8, 8, |x, _| image::Luma([(x * 32) as u8]))
        .save(&alpha)
        .unwrap();
    let path = alpha.to_string_lossy().into_owned();
    let recipe = |inverted: bool| {
        EditRecipe {
            masks: vec![LocalAdjustment {
                mask: MaskGeometry::select_sky(0.5, 0.25, false, path.clone()),
                role: autoshade::recipe::MaskRole::ZoneLand,
                inverted,
                exposure_ev: -0.5,
                ..LocalAdjustment::default()
            }],
            ..EditRecipe::default()
        }
    };
    let strip_intent = |mut text: String| {
        while let Some(start) = text.find(" ash:") {
            let value = start + text[start..].find("=\"").unwrap() + 2;
            let end = value + text[value..].find('"').unwrap() + 1;
            text.replace_range(start..end, "");
        }
        text
    };
    let exported = |inverted: bool| {
        strip_intent(without_recipe_payload(autoshade::xmp::recipe_to_xmp(
            &recipe(inverted),
        )))
    };
    let authored_inv = exported(true);
    let authored_up = exported(false);
    assert!(authored_inv.contains(r#"crs:MaskInverted="true""#), "{authored_inv}");
    let edited_up = authored_inv.replace(r#"crs:MaskInverted="true""#, r#"crs:MaskInverted="false""#);
    let edited_inv = authored_up.replace(r#"crs:MaskInverted="false""#, r#"crs:MaskInverted="true""#);
    let reference = DynamicImage::ImageRgb8(RgbImage::from_pixel(8, 8, Rgb([100; 3])));
    for (label, text, want_inverted) in [
        ("native inverted", &authored_inv, true),
        ("native upright", &authored_up, false),
        ("native edit upright", &edited_up, false),
        ("native edit inverted", &edited_inv, true),
    ] {
        let imported = autoshade::xmp::xmp_to_recipe(text);
        assert_eq!(imported.masks.len(), 1, "{label}");
        let mask = &imported.masks[0];
        assert_eq!(mask.net_inverted(), want_inverted, "{label}: the native net wins");
        let mut resolved = mask.clone();
        let MaskGeometry::AiMask { raster, .. } = &mut resolved.mask else {
            panic!("{label}: expected the Select Sky component back")
        };
        *raster = Some(path.clone());
        let coverage = autoshade::render::mask_coverage(
            &resolved,
            &reference,
            autoshade::render::MaskFrame::AsRendered,
        );
        let left = f32::from(coverage.get_pixel(1, 4)[0]) / 255.0;
        let right = f32::from(coverage.get_pixel(7, 4)[0]) / 255.0;
        assert!(
            if want_inverted { left > right } else { left < right },
            "{label}: coverage must be the {} alpha (left {left}, right {right})",
            if want_inverted { "complement of the" } else { "upright" }
        );
    }
}

#[test]
fn turning_the_frame_moves_the_ai_click_and_invalidates_its_cached_alpha() {
    let mut recipe = EditRecipe {
        coord_era: 0,
        masks: vec![LocalAdjustment {
            exposure_ev: 1.0,
            mask: MaskGeometry::AiMask {
                name: "Sky 1".to_string(),
                subtype: 2,
                ref_x: 0.25,
                ref_y: 0.10,
                blend_mode: 0,
                value: 1.0,
                inverted: false,
                mask_version: 1,
                provenance: Vec::new(),
                gesture: Vec::new(),
                raster: Some("stale.png".to_string()),
            },
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    assert!(autoshade::render::recipe_has_frame_coords(&recipe));
    assert!(autoshade::render::orient_recipe_coords(
        &mut recipe,
        Orientation::Rotate90,
        probe_frame()
    ));
    let MaskGeometry::AiMask { ref_x, ref_y, raster, .. } = &recipe.masks[0].mask else {
        panic!("the geometry must survive the migration")
    };
    assert_ne!((*ref_x, *ref_y), (0.25, 0.10), "the click moved with the frame");
    assert!(raster.is_none(), "the alpha from the OLD frame must not be reused");
}

#[test]
fn turning_the_frame_keeps_a_zone_masks_own_alpha_and_turns_it() {
    let mut recipe = EditRecipe {
        coord_era: 0,
        masks: vec![LocalAdjustment {
            exposure_ev: 1.0,
            role: autoshade::recipe::MaskRole::ZoneSky,
            mask: MaskGeometry::select_sky(0.25, 0.10, false, "mask-zone-sky.png".to_string()),
            ..LocalAdjustment::default()
        }],
        ..EditRecipe::default()
    };
    assert_eq!(
        recipe.masks[0]
            .turnable_raster_paths_mut()
            .into_iter()
            .map(|path| path.to_string())
            .collect::<Vec<_>>(),
        vec!["mask-zone-sky.png".to_string()]
    );
    assert!(autoshade::render::orient_recipe_coords(
        &mut recipe,
        Orientation::Rotate90,
        probe_frame()
    ));
    let MaskGeometry::AiMask { ref_x, ref_y, raster, .. } = &recipe.masks[0].mask else {
        panic!("the geometry must survive the migration")
    };
    assert_ne!((*ref_x, *ref_y), (0.25, 0.10), "the click still moves with the frame");
    assert_eq!(
        raster.as_deref(),
        Some("mask-zone-sky.png"),
        "the fit's own alpha is kept for phase 1 to turn, never dropped"
    );
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

fn ca_profile_recipe(ca_r: f32, ca_b: f32) -> EditRecipe {
    EditRecipe {
        lens_profile: LensProfile {
            ca_r: vec![ca_r; 16],
            ca_b: vec![ca_b; 16],
            ca_on: true,
            ..LensProfile::default()
        },
        ..EditRecipe::default()
    }
}

fn geometry_ramp(width: u32, height: u32) -> DynamicImage {
    DynamicImage::ImageRgb16(image::ImageBuffer::<image::Rgb<u16>, Vec<u16>>::from_fn(width, height, |x, y| {
        image::Rgb([
            (x * 300) as u16,
            (x * 200 + y * 7) as u16,
            (y * 500) as u16,
        ])
    }))
}

#[test]
fn profile_ca_moves_each_channel_by_its_own_knots_only() {
    let recipe = ca_profile_recipe(1.02, 0.98);
    let ramp = geometry_ramp(200, 100);
    let source = ramp.to_rgb16();
    let delivered = autoshade::render::apply_lens_geometry(
        &ramp,
        &autoshade::render::geometry_profile(&recipe),
        0.0,
    )
    .to_rgb16();

    assert!(
        delivered
            .pixels()
            .zip(source.pixels())
            .all(|(after, before)| after[1] == before[1]),
        "green moved under a CA-only profile"
    );
    let (centre, x) = (99.5f32, 180u32);
    let sample_x = centre + (x as f32 - centre) * 1.02;
    let index = sample_x.floor() as u32;
    let fraction = sample_x - sample_x.floor();
    let expected = f32::from(source.get_pixel(index, 50)[0]) * (1.0 - fraction)
        + f32::from(source.get_pixel(index + 1, 50)[0]) * fraction;
    let actual = f32::from(delivered.get_pixel(x, 50)[0]);
    assert!(
        (actual - expected).abs() <= 1.0,
        "red at x={x}: {actual}, expected the source at {sample_x} ({expected})"
    );

    let white = DynamicImage::ImageRgb8(RgbImage::from_pixel(201, 101, Rgb([255; 3])));
    let delivered_white = autoshade::render::apply_lens_geometry(
        &white,
        &autoshade::render::geometry_profile(&recipe),
        0.0,
    )
    .to_rgb16();
    let darkest = delivered_white
        .pixels()
        .flat_map(|pixel| pixel.0)
        .min()
        .unwrap();
    assert!(darkest >= 65_000, "unfilled edge pixels: min {darkest}");
}

#[test]
fn ca_never_moves_green_even_when_a_channel_overshoots() {
    let distortion: Vec<f32> = (0..16)
        .map(|i| 1.0008 - 0.02 * (i as f32 / 15.0).powi(2))
        .collect();
    let with_ca = EditRecipe {
        lens_profile: LensProfile {
            distortion: distortion.clone(),
            distortion_on: true,
            ca_r: vec![1.004; 16],
            ca_b: vec![0.999; 16],
            ca_on: true,
            ..LensProfile::default()
        },
        ..EditRecipe::default()
    };
    let without_ca = EditRecipe {
        lens_profile: LensProfile {
            distortion,
            distortion_on: true,
            ..LensProfile::default()
        },
        ..EditRecipe::default()
    };
    let ramp = geometry_ramp(160, 90);
    let a = autoshade::render::apply_lens_geometry(
        &ramp,
        &autoshade::render::geometry_profile(&with_ca),
        0.0,
    )
    .to_rgb16();
    let b = autoshade::render::apply_lens_geometry(
        &ramp,
        &autoshade::render::geometry_profile(&without_ca),
        0.0,
    )
    .to_rgb16();

    assert!(
        a.pixels().zip(b.pixels()).all(|(p, q)| p[1] == q[1]),
        "green must be byte-identical with and without an overshooting CA"
    );
    assert!(
        a.pixels().zip(b.pixels()).any(|(p, q)| p[0] != q[0]),
        "premise: the red knot really moved red"
    );
}

#[test]
fn a_rest_manual_ca_pair_renders_the_profile_byte_for_byte() {
    let profile = LensProfile {
        distortion: (0..16)
            .map(|i| 1.0008 - 0.02 * (i as f32 / 15.0).powi(2))
            .collect(),
        distortion_on: true,
        ca_r: vec![0.999; 16],
        ca_b: vec![1.001; 16],
        ca_on: true,
        ..LensProfile::default()
    };
    let recipe = EditRecipe {
        lens_profile: profile.clone(),
        ..EditRecipe::default()
    };
    let ramp = geometry_ramp(160, 90);
    let composed = autoshade::render::apply_lens_geometry(
        &ramp,
        &autoshade::render::geometry_profile(&recipe),
        0.0,
    )
    .to_rgb16();
    let direct = autoshade::render::apply_lens_geometry(&ramp, &profile, 0.0).to_rgb16();

    assert!(
        composed.pixels().zip(direct.pixels()).all(|(p, q)| p == q),
        "a rest manual CA pair changed the render"
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
