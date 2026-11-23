use autoshade::stack::merge::{MergeOptions, StackKind, merge};

fn grain(seed: u32, index: usize) -> f32 {
    let mut value = (index as u32)
        .wrapping_mul(2_654_435_761)
        .wrapping_add(seed.wrapping_mul(40_503));
    value ^= value >> 13;
    value = value.wrapping_mul(1_274_126_177);
    value ^= value >> 16;
    value as f32 / u32::MAX as f32 - 0.5
}

fn linear_to_srgb(value: f32) -> f32 {
    if value <= 0.003_130_8 {
        12.92 * value
    } else {
        1.055 * value.powf(1.0 / 2.4) - 0.055
    }
}

fn srgb_to_linear(value: f32) -> f32 {
    if value <= 0.040_45 {
        value / 12.92
    } else {
        ((value + 0.055) / 1.055).powf(2.4)
    }
}

fn textured(width: usize, height: usize) -> Vec<[f32; 3]> {
    (0..width * height)
        .map(|index| {
            let (x, y) = ((index % width) as f32, (index / width) as f32);
            let value = 0.45 + 0.2 * (x * 1.1).sin() * (y * 0.9).cos() + 0.1 * (x * 0.13).sin();
            [value, value * 0.98, value * 1.02]
        })
        .collect()
}

fn gaussian_blur(plane: &[f32], width: usize, height: usize, sigma: f32) -> Vec<f32> {
    let radius = (3.0 * sigma).ceil() as isize;
    let mut kernel = (-radius..=radius)
        .map(|offset| (-(offset as f32).powi(2) / (2.0 * sigma * sigma)).exp())
        .collect::<Vec<_>>();
    let sum = kernel.iter().sum::<f32>();
    for weight in &mut kernel {
        *weight /= sum;
    }
    let sample = |values: &[f32], x: isize, y: isize| {
        let x = x.clamp(0, width as isize - 1) as usize;
        let y = y.clamp(0, height as isize - 1) as usize;
        values[y * width + x]
    };
    let mut horizontal = vec![0.0; width * height];
    for y in 0..height {
        for x in 0..width {
            horizontal[y * width + x] = (-radius..=radius)
                .zip(&kernel)
                .map(|(offset, weight)| weight * sample(plane, x as isize + offset, y as isize))
                .sum();
        }
    }
    let mut vertical = vec![0.0; width * height];
    for y in 0..height {
        for x in 0..width {
            vertical[y * width + x] = (-radius..=radius)
                .zip(&kernel)
                .map(|(offset, weight)| {
                    weight * sample(&horizontal, x as isize, y as isize + offset)
                })
                .sum();
        }
    }
    vertical
}

fn detail_energy(frame: &[[f32; 3]], width: usize, height: usize, left: bool) -> f32 {
    let grey = frame
        .iter()
        .map(|pixel| (pixel[0] + pixel[1] + pixel[2]) / 3.0)
        .collect::<Vec<_>>();
    (0..width * height)
        .filter(|index| (index % width < width / 2) == left)
        .map(|index| {
            let x = index % width;
            let y = index / width;
            let sample = |x: usize, y: usize| grey[y * width + x];
            let left = sample(x.saturating_sub(1), y);
            let right = sample((x + 1).min(width - 1), y);
            let up = sample(x, y.saturating_sub(1));
            let down = sample(x, (y + 1).min(height - 1));
            (left + right + up + down - 4.0 * grey[index]).abs()
        })
        .sum()
}

fn mean_interior(
    frame: &[[f32; 3]],
    width: usize,
    height: usize,
    pick: impl Fn(&[f32; 3]) -> f32,
) -> f32 {
    let values = (0..width * height)
        .filter(|index| {
            (8..width - 8).contains(&(index % width)) && (8..height - 8).contains(&(index / width))
        })
        .map(|index| pick(&frame[index]))
        .collect::<Vec<_>>();
    values.iter().sum::<f32>() / values.len() as f32
}

fn saturation(pixel: &[f32; 3]) -> f32 {
    let mean = pixel.iter().sum::<f32>() / 3.0;
    (pixel
        .iter()
        .map(|value| (value - mean).powi(2))
        .sum::<f32>()
        / 3.0)
        .sqrt()
}

#[test]
fn a_focus_stack_reproduces_the_sharp_frame_on_both_sides() {
    let (width, height) = (128, 96);
    let sharp = (0..width * height)
        .map(|index| {
            let on = (index % width) % 24 < 4 || (index / width) % 32 < 4;
            let value = if on { 0.72 } else { 0.32 };
            [value, value * 0.98, value * 1.02]
        })
        .collect::<Vec<_>>();
    let planes = (0..3)
        .map(|channel| sharp.iter().map(|pixel| pixel[channel]).collect::<Vec<_>>())
        .collect::<Vec<_>>();
    let soft = planes
        .iter()
        .map(|plane| gaussian_blur(plane, width, height, 3.0))
        .collect::<Vec<_>>();
    let mix = |left_sharp: bool| {
        (0..width * height)
            .map(|index| {
                let use_sharp = (index % width < width / 2) == left_sharp;
                std::array::from_fn(|channel| {
                    if use_sharp {
                        sharp[index][channel]
                    } else {
                        soft[channel][index]
                    }
                })
            })
            .collect::<Vec<_>>()
    };
    let frames = vec![mix(true), mix(false)];

    for (index, sharp_left) in [(0, true), (1, false)] {
        assert!(
            detail_energy(&frames[index], width, height, sharp_left)
                > 3.0 * detail_energy(&frames[index], width, height, !sharp_left)
        );
    }
    let merged = merge(
        &frames,
        width,
        height,
        &MergeOptions {
            kind: StackKind::Focus,
            align: None,
        },
    )
    .unwrap();

    for left in [true, false] {
        assert!(
            detail_energy(&merged.pixels, width, height, left)
                > 0.99 * detail_energy(&sharp, width, height, left)
        );
    }
    let seam = width / 2 - 12..width / 2 + 12;
    let worst = (0..width * height)
        .filter(|index| {
            (8..width - 8).contains(&(index % width))
                && (8..height - 8).contains(&(index / width))
                && !seam.contains(&(index % width))
        })
        .map(|index| (merged.pixels[index][1] - sharp[index][1]).abs())
        .fold(0.0_f32, f32::max);
    assert!(worst < 0.02, "worst={worst}");
}

#[test]
fn exposure_fusion_prefers_colour_when_detail_is_equal() {
    let (width, height) = (96, 72);
    let texture = |index: usize| {
        let (x, y) = ((index % width) as f32, (index / width) as f32);
        0.07 * (x * 1.1).sin() * (y * 0.9).cos()
    };
    let coloured = (0..width * height)
        .map(|index| {
            let value = 0.5 + texture(index);
            [value + 0.15, value, value - 0.15]
        })
        .collect::<Vec<_>>();
    let grey = (0..width * height)
        .map(|index| {
            let value = 0.5 + texture(index);
            [value; 3]
        })
        .collect::<Vec<_>>();

    let merged = merge(
        &[coloured.clone(), grey],
        width,
        height,
        &MergeOptions {
            kind: StackKind::Fuse,
            align: None,
        },
    )
    .unwrap();
    let got = mean_interior(&merged.pixels, width, height, saturation);
    let expected = mean_interior(&coloured, width, height, saturation);

    assert!(got > 0.7 * expected, "got={got} expected={expected}");
}

#[test]
fn exposure_fusion_prefers_midtones_when_detail_is_equal() {
    let (width, height) = (96, 72);
    let texture = |index: usize| {
        let (x, y) = ((index % width) as f32, (index / width) as f32);
        0.07 * (x * 1.1).sin() * (y * 0.9).cos()
    };
    let middle = (0..width * height)
        .map(|index| {
            let value = 0.5 + texture(index);
            [value; 3]
        })
        .collect::<Vec<_>>();
    let pale = (0..width * height)
        .map(|index| {
            let value = 0.88 + texture(index);
            [value; 3]
        })
        .collect::<Vec<_>>();

    let merged = merge(
        &[middle, pale],
        width,
        height,
        &MergeOptions {
            kind: StackKind::Fuse,
            align: None,
        },
    )
    .unwrap();
    let level = mean_interior(&merged.pixels, width, height, |pixel| pixel[1]);

    assert!(level < 0.6, "level={level}");
}

#[test]
fn hdr_merge_restores_structure_above_white_and_preserves_the_room() {
    let (width, height) = (96, 72);
    let window = |stops: f32| {
        let gain = 2.0_f32.powf(stops);
        (0..width * height)
            .map(|index| {
                let (x, y) = (index % width, index / width);
                let room =
                    0.06 + 0.04 * (x as f32 / width as f32) + 0.02 * (y as f32 / height as f32);
                let linear = if (width / 2..width).contains(&x)
                    && (height / 4..3 * height / 4).contains(&y)
                {
                    2.0 + 10.0 * ((x - width / 2) as f32 / (width / 2) as f32)
                } else {
                    room
                };
                let value = linear_to_srgb((linear * gain).min(1.0));
                [value, value * 0.98, value * 1.03]
            })
            .collect::<Vec<_>>()
    };
    let frames = vec![window(0.0), window(-3.0), window(-6.0)];
    let window_pixels = (0..width * height)
        .filter(|index| {
            (width / 2 + 4..width - 4).contains(&(index % width))
                && (height / 3..2 * height / 3).contains(&(index / width))
        })
        .collect::<Vec<_>>();
    let spread = |frame: &[[f32; 3]]| {
        let (low, high) = window_pixels
            .iter()
            .fold((1.0_f32, 0.0_f32), |(low, high), index| {
                (low.min(frame[*index][1]), high.max(frame[*index][1]))
            });
        high - low
    };

    assert!(spread(&frames[0]) < 0.001);
    let merged = merge(
        &frames,
        width,
        height,
        &MergeOptions {
            kind: StackKind::Hdr,
            align: None,
        },
    )
    .unwrap();
    let room = (0..width * height)
        .filter(|index| index % width < width / 2 - 4)
        .map(|index| (merged.pixels[index][1] - frames[0][index][1]).abs())
        .fold(0.0_f32, f32::max);

    assert!(spread(&merged.pixels) > 0.05);
    assert!(merged.headroom_ev > 1.5);
    assert!(room < 0.02, "room={room}");
}

#[test]
fn hdr_merge_preserves_the_colour_of_a_partly_clipped_highlight() {
    let (width, height) = (64, 48);
    let tinted = |linear: f32, gain: f32| {
        let channel = |ratio: f32| linear_to_srgb((linear * ratio * gain).min(1.0));
        [channel(1.0), channel(0.30), channel(0.15)]
    };
    let frames = [0.0_f32, -2.0, -4.0]
        .iter()
        .map(|stops| {
            let gain = 2.0_f32.powf(*stops);
            (0..width * height)
                .map(|index| {
                    let linear = if index % width >= width / 2 {
                        1.5
                    } else {
                        0.10
                    };
                    tinted(linear, gain)
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    let probe = (height / 2) * width + width * 3 / 4;

    assert!(frames[0][probe][0] > 0.99);
    assert!(frames[0][probe][1] < 0.95);
    let merged = merge(
        &frames,
        width,
        height,
        &MergeOptions {
            kind: StackKind::Hdr,
            align: None,
        },
    )
    .unwrap();
    let linear = merged.pixels[probe].map(srgb_to_linear);
    let green_ratio = linear[1] / linear[0];
    let blue_ratio = linear[2] / linear[0];

    assert!(linear[0] > 1e-4);
    assert!(
        (green_ratio - 0.30).abs() < 0.04 && (blue_ratio - 0.15).abs() < 0.04,
        "ratios={green_ratio}:{blue_ratio}"
    );
}

#[test]
fn hdr_merge_gives_more_weight_to_the_frame_that_collected_more_light() {
    let (width, height) = (128, 96);
    let shot = |seed: u32, stops: f32| {
        let gain = 2.0_f32.powf(stops);
        (0..width * height)
            .map(|index| {
                let value = linear_to_srgb((0.10 * gain + 0.002 * grain(seed, index)).max(0.0));
                [value; 3]
            })
            .collect::<Vec<_>>()
    };
    let frames = vec![shot(1, 0.0), shot(2, -2.0)];
    let rms = |frame: &[[f32; 3]], scale: f32| {
        let squared = frame
            .iter()
            .map(|pixel| (srgb_to_linear(pixel[1]) * scale - 0.10).powi(2))
            .sum::<f32>();
        (squared / frame.len() as f32).sqrt()
    };
    let bright = rms(&frames[0], 1.0);
    let dark = rms(&frames[1], 4.0);

    assert!(dark > 3.0 * bright);
    let merged = merge(
        &frames,
        width,
        height,
        &MergeOptions {
            kind: StackKind::Hdr,
            align: None,
        },
    )
    .unwrap();

    assert!(rms(&merged.pixels, 1.0) < 1.4 * bright);
}

#[test]
fn hdr_shoulder_preserves_low_values_reaches_white_and_stays_monotonic() {
    let (width, height) = (257, 8);
    let radiance = (0..width * height)
        .map(|index| 4.0 * (index % width) as f32 / (width - 1) as f32)
        .collect::<Vec<_>>();
    let frames = [0.0_f32, -2.0, -4.0]
        .iter()
        .map(|stops| {
            let gain = 2.0_f32.powf(*stops);
            radiance
                .iter()
                .map(|value| [linear_to_srgb((value * gain).min(1.0)); 3])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let merged = merge(
        &frames,
        width,
        height,
        &MergeOptions {
            kind: StackKind::Hdr,
            align: None,
        },
    )
    .unwrap();
    let row = &merged.pixels[..width];
    let below_knee = width / 20;
    let reference = linear_to_srgb(radiance[below_knee]);

    assert!((merged.headroom_ev - 2.0).abs() < 0.1);
    assert!((row[below_knee][0] - reference).abs() < 0.01);
    assert!((row[width - 1][0] - 1.0).abs() < 1e-3);
    assert!(row.windows(2).all(|pair| pair[1][0] + 1e-6 >= pair[0][0]));
    assert!(row[width / 4][0] < 0.97);
}

#[test]
fn one_hot_pixel_does_not_set_the_hdr_headroom() {
    let (width, height) = (100, 100);
    let mut radiance = (0..width * height)
        .map(|index| {
            let (x, y) = ((index % width) as f32, (index / width) as f32);
            let texture = 0.01 * (x * 0.31).sin() * (y * 0.27).cos();
            if index % width < 3 * width / 4 {
                0.10 + texture
            } else {
                1.90 + texture
            }
        })
        .collect::<Vec<_>>();
    radiance[height / 2 * width + width * 7 / 8] = 8.0;
    let frames = [0.0_f32, -2.0, -4.0]
        .iter()
        .map(|stops| {
            let gain = 2.0_f32.powf(*stops);
            radiance
                .iter()
                .map(|value| [linear_to_srgb((value * gain).min(1.0)); 3])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();

    let merged = merge(
        &frames,
        width,
        height,
        &MergeOptions {
            kind: StackKind::Hdr,
            align: None,
        },
    )
    .unwrap();

    assert!(
        (merged.headroom_ev - 1.9_f32.log2()).abs() < 0.1,
        "headroom={} exposures={:?}",
        merged.headroom_ev,
        merged.exposures
    );
}

#[test]
fn exposure_fusion_recovers_contrast_in_both_halves_of_a_bracket() {
    let (width, height) = (128, 96);
    let base = (0..width * height)
        .map(|index| {
            let (x, y) = ((index % width) as f32, (index / width) as f32);
            let texture = 0.5 + 0.35 * (x * 0.9).sin() * (y * 0.7).cos();
            if index % width < width / 2 {
                0.012 * texture
            } else {
                0.75 * texture
            }
        })
        .collect::<Vec<_>>();
    let shot = |stops: f32| {
        let gain = 2.0_f32.powf(stops);
        base.iter()
            .map(|linear| {
                let value = linear_to_srgb((linear * gain).min(1.0));
                [value, value * 0.97, value * 1.03]
            })
            .collect::<Vec<_>>()
    };
    let frames = vec![shot(0.0), shot(4.0)];
    let contrast = |frame: &[[f32; 3]], left: bool| {
        let values = (0..width * height)
            .filter(|index| (index % width < width / 2) == left)
            .map(|index| frame[index][1])
            .collect::<Vec<_>>();
        values.iter().copied().fold(0.0_f32, f32::max)
            - values.iter().copied().fold(1.0_f32, f32::min)
    };

    assert!(contrast(&frames[0], true) < 0.1);
    assert!(contrast(&frames[1], false) < 0.1);
    let merged = merge(
        &frames,
        width,
        height,
        &MergeOptions {
            kind: StackKind::Fuse,
            align: None,
        },
    )
    .unwrap();

    assert!(contrast(&merged.pixels, true) > 0.12);
    assert!(contrast(&merged.pixels, false) > 0.12);
}

struct NoiseEvidence {
    one_frame_rms: f32,
    merged_rms: f32,
    worst_ghost: f32,
}

fn noise_evidence() -> &'static NoiseEvidence {
    static EVIDENCE: std::sync::OnceLock<NoiseEvidence> = std::sync::OnceLock::new();
    EVIDENCE.get_or_init(|| {
        let (width, height) = (96, 72);
        let truth = (0..width * height)
            .map(|index| 0.45 + 0.15 * ((index % width) as f32 * 0.05).sin())
            .collect::<Vec<_>>();
        let ghost = (0..width * height)
            .filter(|index| {
                (20..40).contains(&(index % width)) && (20..40).contains(&(index / width))
            })
            .collect::<Vec<_>>();
        let frames = (0..6_u32)
            .map(|seed| {
                (0..width * height)
                    .map(|index| {
                        let mut value = truth[index] + 0.02 * grain(seed, index);
                        if seed == 2 && ghost.binary_search(&index).is_ok() {
                            value = 0.95;
                        }
                        [value; 3]
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let rms = |frame: &[[f32; 3]]| {
            let squared = (0..width * height)
                .map(|index| (frame[index][1] - truth[index]).powi(2))
                .sum::<f32>();
            (squared / (width * height) as f32).sqrt()
        };
        let merged = merge(
            &frames,
            width,
            height,
            &MergeOptions {
                kind: StackKind::Noise,
                align: None,
            },
        )
        .unwrap();
        let worst_ghost = ghost
            .iter()
            .map(|index| (merged.pixels[*index][1] - truth[*index]).abs())
            .fold(0.0_f32, f32::max);
        NoiseEvidence {
            one_frame_rms: rms(&frames[0]),
            merged_rms: rms(&merged.pixels),
            worst_ghost,
        }
    })
}

#[test]
fn a_noise_stack_halves_the_grain() {
    let evidence = noise_evidence();

    assert!(evidence.one_frame_rms > 0.003);
    assert!(evidence.merged_rms < evidence.one_frame_rms / 2.0);
}

#[test]
fn a_noise_stack_removes_a_transient_ghost() {
    assert!(
        noise_evidence().worst_ghost < 0.03,
        "worst={}",
        noise_evidence().worst_ghost
    );
}

#[test]
fn every_stack_kind_returns_a_full_frame_and_one_exposure_per_input() {
    let (width, height) = (48, 32);
    let frames = vec![textured(width, height), textured(width, height)];

    for kind in StackKind::ALL {
        let merged = merge(&frames, width, height, &MergeOptions { kind, align: None })
            .unwrap_or_else(|error| panic!("{}: {error}", kind.store_str()));

        assert_eq!(merged.pixels.len(), width * height, "kind={kind:?}");
        assert_eq!(merged.exposures.len(), frames.len(), "kind={kind:?}");
    }
}

#[test]
fn a_stack_refuses_one_frame_and_mismatched_frame_sizes() {
    let (width, height) = (48, 32);
    let options = MergeOptions::new(StackKind::Hdr);

    assert!(merge(&[textured(width, height)], width, height, &options).is_err());
    assert!(
        merge(
            &[textured(width, height), textured(width, height - 1)],
            width,
            height,
            &options,
        )
        .is_err()
    );
}
