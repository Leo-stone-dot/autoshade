use autoshade::stack::align::{Affine, AlignParams, Warp, coverage, solve, warp_rgb};

fn scene(width: usize, height: usize) -> Vec<[f32; 3]> {
    (0..width * height)
        .map(|index| {
            let x = (index % width) as f32;
            let y = (index / width) as f32;
            let value = (0.5
                + 0.18 * (x * 0.21).sin() * (y * 0.13).cos()
                + 0.12 * (x * 0.07 + y * 0.05).sin()
                + 0.08 * ((x * 0.9).sin() * (y * 0.7).sin()))
            .clamp(0.04, 0.96);
            [value, value * 0.97, value * 1.02]
        })
        .collect()
}

fn bilinear(plane: &[f32], width: usize, height: usize, x: f32, y: f32) -> f32 {
    let x = x.clamp(0.0, (width - 1) as f32);
    let y = y.clamp(0.0, (height - 1) as f32);
    let x0 = x.floor() as usize;
    let y0 = y.floor() as usize;
    let x1 = (x0 + 1).min(width - 1);
    let y1 = (y0 + 1).min(height - 1);
    let tx = x - x0 as f32;
    let ty = y - y0 as f32;
    let top = plane[y0 * width + x0] * (1.0 - tx) + plane[y0 * width + x1] * tx;
    let bottom = plane[y1 * width + x0] * (1.0 - tx) + plane[y1 * width + x1] * tx;
    top * (1.0 - ty) + bottom * ty
}

fn shifted(source: &[[f32; 3]], width: usize, height: usize, affine: Affine) -> Vec<[f32; 3]> {
    let planes = (0..3)
        .map(|channel| {
            source
                .iter()
                .map(|pixel| pixel[channel])
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    (0..width * height)
        .map(|index| {
            let (x, y) = affine.apply(
                (index % width) as f32,
                (index / width) as f32,
                width,
                height,
            );
            [
                bilinear(&planes[0], width, height, x, y),
                bilinear(&planes[1], width, height, x, y),
                bilinear(&planes[2], width, height, x, y),
            ]
        })
        .collect()
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

fn exposed(source: &[[f32; 3]], stops: f32) -> Vec<[f32; 3]> {
    let gain = 2.0_f32.powf(stops);
    source
        .iter()
        .map(|pixel| pixel.map(|value| linear_to_srgb((srgb_to_linear(value) * gain).min(1.0))))
        .collect()
}

struct ShiftedRoundTrip {
    reference: Vec<[f32; 3]>,
    moving: Vec<[f32; 3]>,
    restored: Vec<[f32; 3]>,
    covered: Vec<bool>,
    width: usize,
    height: usize,
}

fn shifted_round_trip() -> ShiftedRoundTrip {
    let (width, height) = (96, 72);
    let reference = scene(width, height);
    let truth = Affine([1.0, 0.0, 5.0, 0.0, 1.0, 3.0]);
    let moving = shifted(&reference, width, height, truth);
    let back = Warp {
        global: truth.inverse().unwrap(),
        local: [Vec::new(), Vec::new()],
        bx: 0,
        by: 0,
    };
    let restored = warp_rgb(&moving, width, height, &back);
    let covered = coverage(width, height, &back);
    ShiftedRoundTrip {
        reference,
        moving,
        restored,
        covered,
        width,
        height,
    }
}

#[test]
fn a_two_stop_bracket_recovers_its_translation_without_inventing_scale() {
    let (width, height) = (192, 144);
    let reference = scene(width, height);
    let truth = Affine([1.0, 0.0, 6.0, 0.0, 1.0, -4.0]);
    let moving = exposed(&shifted(&reference, width, height, truth), 2.0);
    let params = AlignParams {
        blocks: 0,
        ..AlignParams::default()
    };

    let solved = solve(&reference, &moving, width, height, &params).global.0;
    let expected = truth.inverse().unwrap().0;

    assert!(
        (solved[2] - expected[2]).abs() < 0.5 && (solved[5] - expected[5]).abs() < 0.5,
        "solved={solved:?} expected={expected:?}"
    );
    assert!(
        (solved[0] - 1.0).abs() < 0.01 && (solved[4] - 1.0).abs() < 0.01,
        "solved={solved:?}"
    );
}

#[test]
fn affine_alignment_recovers_roll_scale_and_translation() {
    let (width, height) = (192, 144);
    let reference = scene(width, height);
    let (cos, sin, scale) = (0.9998_f32, 0.0201_f32, 1.012_f32);
    let truth = Affine([
        cos * scale,
        -sin * scale,
        2.0,
        sin * scale,
        cos * scale,
        1.0,
    ]);
    let moving = shifted(&reference, width, height, truth);
    let params = AlignParams {
        blocks: 0,
        ..AlignParams::default()
    };

    let solved = solve(&reference, &moving, width, height, &params).global.0;
    let expected = truth.inverse().unwrap().0;

    for index in [0, 1, 3, 4] {
        assert!(
            (solved[index] - expected[index]).abs() < 0.004,
            "index={index} solved={solved:?} expected={expected:?}"
        );
    }
    assert!(
        (solved[2] - expected[2]).abs() < 0.5 && (solved[5] - expected[5]).abs() < 0.5,
        "solved={solved:?} expected={expected:?}"
    );
}

#[test]
fn warping_back_reproduces_every_covered_pixel() {
    let fixture = shifted_round_trip();
    let worst = fixture
        .covered
        .iter()
        .enumerate()
        .filter(|(_, covered)| **covered)
        .map(|(index, _)| (fixture.restored[index][1] - fixture.reference[index][1]).abs())
        .fold(0.0_f32, f32::max);

    assert!(worst < 0.02, "worst={worst}");
}

#[test]
fn coverage_names_the_wedge_a_shift_pushes_outside_the_source() {
    let fixture = shifted_round_trip();
    let uncovered = fixture.covered.iter().filter(|covered| !**covered).count();

    assert!(
        fixture.covered.iter().filter(|covered| **covered).count()
            > fixture.width * fixture.height / 2
    );
    assert!(uncovered > 0);
}

#[test]
fn an_uncovered_warp_pixel_keeps_the_value_it_had() {
    let fixture = shifted_round_trip();
    let worst = fixture
        .covered
        .iter()
        .enumerate()
        .filter(|(_, covered)| !**covered)
        .map(|(index, _)| (fixture.restored[index][1] - fixture.moving[index][1]).abs())
        .fold(0.0_f32, f32::max);

    assert_eq!(worst, 0.0);
}

#[test]
fn a_split_residual_field_is_sampled_from_its_matching_planes() {
    let (width, height, bx, by) = (40, 30, 4, 3);
    let dx = (0..bx * by)
        .map(|index| (index as f32 * 0.7).sin() * 3.0)
        .collect::<Vec<_>>();
    let dy = (0..bx * by)
        .map(|index| (index as f32 * 1.3).cos() * 2.0)
        .collect::<Vec<_>>();
    let warp = Warp {
        global: Affine([1.0, 0.02, 1.5, -0.01, 1.0, -0.5]),
        local: [dx.clone(), dy.clone()],
        bx,
        by,
    };
    let mut moved = 0;

    for y in 0..height {
        for x in 0..width {
            let (px, py) = (x as f32, y as f32);
            let (global_x, global_y) = warp.global.apply(px, py, width, height);
            let grid_x = px * bx as f32 / width as f32 - 0.5;
            let grid_y = py * by as f32 / height as f32 - 0.5;
            let expected = (
                global_x + bilinear(&dx, bx, by, grid_x, grid_y),
                global_y + bilinear(&dy, bx, by, grid_x, grid_y),
            );
            let actual = warp.apply(px, py, width, height);

            assert_eq!(actual.0.to_bits(), expected.0.to_bits(), "x={x} y={y}");
            assert_eq!(actual.1.to_bits(), expected.1.to_bits(), "x={x} y={y}");
            moved += usize::from(actual != (global_x, global_y));
        }
    }

    assert!(moved > width * height / 2);
}

#[test]
fn a_large_shift_is_recovered_even_with_a_fifth_of_the_frame_uncovered() {
    let (width, height) = (384, 288);
    let reference = scene(width, height);
    let truth = Affine([1.0, 0.0, 48.0, 0.0, 1.0, -32.0]);
    let moving = shifted(&reference, width, height, truth);
    let params = AlignParams {
        blocks: 0,
        ..AlignParams::default()
    };

    let warp = solve(&reference, &moving, width, height, &params);
    let solved = warp.global.0;
    let expected = truth.inverse().unwrap().0;
    let uncovered = coverage(width, height, &warp)
        .iter()
        .filter(|covered| !**covered)
        .count();

    assert!(
        (solved[2] - expected[2]).abs() < 0.5 && (solved[5] - expected[5]).abs() < 0.5,
        "solved={solved:?} expected={expected:?}"
    );
    assert!(uncovered > width * height / 6, "uncovered={uncovered}");
}
