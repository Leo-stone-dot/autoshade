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

fn textured(width: usize, height: usize) -> Vec<[f32; 3]> {
    (0..width * height)
        .map(|index| {
            let (x, y) = ((index % width) as f32, (index / width) as f32);
            let value = 0.45 + 0.2 * (x * 1.1).sin() * (y * 0.9).cos() + 0.1 * (x * 0.13).sin();
            [value, value * 0.98, value * 1.02]
        })
        .collect()
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
