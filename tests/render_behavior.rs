use autoshade::recipe::{
    ColorGrade, CurvePoint, EditRecipe, LocalAdjustment, MaskGeometry,
};
use image::{DynamicImage, Rgb, RgbImage};

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
