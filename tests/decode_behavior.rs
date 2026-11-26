use std::path::Path;

use autoshade::recipe::EditRecipe;
use autoshade::render::ExportColorSpace;
use image::GenericImageView;
use rawler::formats::tiff::SRational;
use rawler::decoders::Orientation;

mod support;

const IDENTITY_MATRIX: [f32; 9] = [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0];
/// The inverse of the sRGB→XYZ(D65) matrix, so a synthetic camera's RGB IS
/// sRGB and the calibration stage is the identity on colour.
const SRGB_CAMERA_MATRIX: [f32; 9] = [
    3.2406, -1.5372, -0.4986, -0.9689, 1.8758, 0.0415, 0.0557, -0.2040, 1.0570,
];

fn write_raw(tree: &support::TempTree, name: &str, bytes: Vec<u8>) -> std::path::PathBuf {
    let path = tree.path().join(name);
    std::fs::write(&path, bytes).unwrap();
    path
}

fn render_error(path: &Path) -> (anyhow::Error, std::path::PathBuf) {
    let output = path.with_extension("out.png");
    let error = autoshade::render::render_to_file(
        path,
        &EditRecipe::default(),
        &output,
        None,
        None,
        autoshade::diag::silent(),
    )
    .unwrap_err();
    (error, output)
}

fn oversized_declared_raw_plane() -> Vec<u8> {
    fn entry(bytes: &mut Vec<u8>, tag: u16, kind: u16, value: u32) {
        bytes.extend_from_slice(&tag.to_le_bytes());
        bytes.extend_from_slice(&kind.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        if kind == 3 {
            bytes.extend_from_slice(&(value as u16).to_le_bytes());
            bytes.extend_from_slice(&0_u16.to_le_bytes());
        } else {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    let mut bytes = Vec::new();
    bytes.extend_from_slice(b"II");
    bytes.extend_from_slice(&42_u16.to_le_bytes());
    bytes.extend_from_slice(&8_u32.to_le_bytes());
    bytes.extend_from_slice(&4_u16.to_le_bytes());
    entry(&mut bytes, 0x0100, 4, 50_001);
    entry(&mut bytes, 0x0101, 4, 1);
    entry(&mut bytes, 0x0106, 3, 32_803);
    entry(&mut bytes, 0x0115, 3, 1);
    bytes.extend_from_slice(&0_u32.to_le_bytes());
    bytes
}

#[test]
fn an_oversized_declared_raw_plane_is_refused_before_the_decoder_can_allocate_it() {
    let tree = support::TempTree::new("raw-plane-ceiling");
    let path = tree.path().join("oversized.dng");
    std::fs::write(&path, oversized_declared_raw_plane()).unwrap();

    let error = match autoshade::decode::decode_raw(&path) {
        Ok(_) => panic!("the oversized sensor plane was accepted"),
        Err(error) => error.to_string(),
    };

    assert!(error.contains("50001x1 sensor plane"), "{error}");
    assert!(error.contains("past its own 50000-per-side"), "{error}");
}

#[test]
fn a_raw_without_an_embedded_preview_honors_the_requested_quarter_turn() {
    let tree = support::TempTree::new("raw-no-preview-turn");
    let path = tree.path().join("synthetic.dng");
    std::fs::write(
        &path,
        support::synthetic_dng(16, 8, Orientation::Normal, None, false),
    )
    .unwrap();

    let decoded = autoshade::decode::decode_raw_turned(&path, 1).unwrap();

    assert_eq!(decoded.preview.dimensions(), (8, 16));
    assert_eq!((decoded.meta.width, decoded.meta.height), (8, 16));
}

#[test]
fn as_shot_wb_reads_finite_temperature_and_tint_from_dng_metadata() {
    let tree = support::TempTree::new("raw-as-shot-wb");
    let path = tree.path().join("white-balance.dng");
    let neutral = [SRational::new(1, 2), SRational::new(1, 1), SRational::new(1, 4)];
    std::fs::write(
        &path,
        support::synthetic_dng(16, 8, Orientation::Normal, Some(&neutral), true),
    )
    .unwrap();

    let (temperature, tint) = autoshade::render::as_shot_wb(&path)
        .expect("a DNG with AsShotNeutral and a colour matrix has an as-shot reading");

    assert!(temperature.is_finite() && tint.is_finite());
    assert!((1667.0..=15000.0).contains(&temperature), "temperature={temperature}");
    assert!(tint.abs() <= 150.0, "tint={tint}");
    assert!((temperature - 5500.0).abs() > 1.0, "metadata must not collapse to the default anchor");
}

#[test]
fn as_shot_wb_returns_none_when_the_dng_has_no_colour_matrix() {
    let tree = support::TempTree::new("raw-as-shot-wb-no-matrix");
    let path = tree.path().join("no-matrix.dng");
    let neutral = [SRational::new(1, 1), SRational::new(1, 1), SRational::new(1, 1)];
    std::fs::write(
        &path,
        support::synthetic_dng(16, 8, Orientation::Normal, Some(&neutral), false),
    )
    .unwrap();

    assert!(autoshade::render::as_shot_wb(&path).is_none());
}

#[test]
fn as_shot_wb_returns_none_for_a_zero_camera_neutral() {
    let tree = support::TempTree::new("raw-as-shot-wb-zero-neutral");
    let path = tree.path().join("zero-neutral.dng");
    let neutral = [SRational::new(0, 1), SRational::new(1, 1), SRational::new(1, 1)];
    std::fs::write(
        &path,
        support::synthetic_dng(16, 8, Orientation::Normal, Some(&neutral), true),
    )
    .unwrap();

    assert!(autoshade::render::as_shot_wb(&path).is_none());
}

#[test]
fn a_wide_gamut_render_of_a_matrix_less_raw_refuses_and_names_the_matrix() {
    let tree = support::TempTree::new("raw-no-matrix-wide");
    let path = write_raw(
        &tree,
        "no-matrix.dng",
        support::synthetic_dng(16, 8, Orientation::Normal, None, false),
    );

    let error = autoshade::render::render_to_image_in(
        &path,
        &EditRecipe::default(),
        None,
        None,
        ExportColorSpace::DisplayP3,
        autoshade::diag::silent(),
    )
    .unwrap_err();

    assert!(
        format!("{error:#}").contains("no camera colour matrix"),
        "{error:#}"
    );
}

#[test]
fn a_singular_camera_matrix_is_refused_before_any_output_is_written() {
    let tree = support::TempTree::new("raw-singular-matrix");
    let path = write_raw(
        &tree,
        "singular.dng",
        support::synthetic_dng_with(
            16,
            8,
            Orientation::Normal,
            None,
            Some([1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]),
            0,
            None,
            None,
            None,
        ),
    );

    let (error, output) = render_error(&path);
    let text = format!("{error:#}");

    assert!(text.contains("determinant"), "{text}");
    assert!(text.contains("singular.dng"), "{text}");
    assert!(!output.exists());
}

#[test]
fn a_degenerate_camera_matrix_row_is_refused_by_name() {
    let tree = support::TempTree::new("raw-degenerate-row");
    let path = write_raw(
        &tree,
        "degenerate.dng",
        support::synthetic_dng_with(
            16,
            8,
            Orientation::Normal,
            None,
            Some([0.5, -1.0, 0.5, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]),
            0,
            None,
            None,
            None,
        ),
    );

    let (error, output) = render_error(&path);

    assert!(format!("{error:#}").contains("degenerate row"), "{error:#}");
    assert!(!output.exists());
}

#[test]
fn a_zero_or_negative_as_shot_neutral_is_refused_by_name() {
    let tree = support::TempTree::new("raw-corrupt-neutral");
    for (name, neutral) in [
        (
            "zero.dng",
            [
                SRational::new(0, 1),
                SRational::new(1, 1),
                SRational::new(1, 1),
            ],
        ),
        (
            "negative.dng",
            [
                SRational::new(-1, 1),
                SRational::new(1, 1),
                SRational::new(1, 1),
            ],
        ),
    ] {
        let path = write_raw(
            &tree,
            name,
            support::synthetic_dng_with(
                16,
                8,
                Orientation::Normal,
                Some(&neutral),
                Some(IDENTITY_MATRIX),
                0,
                None,
                None,
                None,
            ),
        );

        let (error, output) = render_error(&path);

        assert!(
            format!("{error:#}").contains("AsShotNeutral"),
            "{name}: {error:#}"
        );
        assert!(!output.exists(), "{name}");
    }
}

#[test]
fn a_corrupt_fourth_wb_coefficient_is_refused_and_a_valid_one_renders() {
    let tree = support::TempTree::new("raw-fourth-wb");
    let corrupt = write_raw(
        &tree,
        "corrupt.dng",
        support::synthetic_dng_with(
            16,
            8,
            Orientation::Normal,
            Some(&[
                SRational::new(1, 1),
                SRational::new(1, 1),
                SRational::new(1, 1),
                SRational::new(0, 1),
            ]),
            Some(IDENTITY_MATRIX),
            0,
            None,
            None,
            None,
        ),
    );
    let (error, output) = render_error(&corrupt);
    assert!(
        format!("{error:#}").contains("fourth WB coefficient"),
        "{error:#}"
    );
    assert!(!output.exists());

    let valid = write_raw(
        &tree,
        "valid.dng",
        support::synthetic_dng_with(
            16,
            8,
            Orientation::Normal,
            Some(&[
                SRational::new(1, 1),
                SRational::new(1, 1),
                SRational::new(1, 1),
                SRational::new(6, 5),
            ]),
            Some(IDENTITY_MATRIX),
            0,
            None,
            None,
            None,
        ),
    );
    let rendered = autoshade::render::render_to_image(&valid, &EditRecipe::default(), None, None)
        .unwrap();
    assert_eq!(rendered.dimensions(), (16, 8));
}

#[test]
fn a_raw_without_as_shot_neutral_renders_with_the_unknown_convention() {
    let tree = support::TempTree::new("raw-unknown-neutral");
    let path = write_raw(
        &tree,
        "unknown.dng",
        support::synthetic_dng_with(
            16,
            8,
            Orientation::Normal,
            None,
            Some(IDENTITY_MATRIX),
            0,
            None,
            None,
            None,
        ),
    );

    let rendered = autoshade::render::render_to_image(&path, &EditRecipe::default(), None, None)
        .unwrap();

    assert_eq!(rendered.dimensions(), (16, 8));
}

/// Sub-black noise reaches the demosaic instead of being clamped to the black
/// level first: a mosaic straddling the black renders differently from the
/// same mosaic with its below-black sites clamped. The linear-domain "mean
/// of zero" the engine's own buffer shows is not observable through the
/// encoded public output, so this asserts the sensitivity, not a direction.
#[test]
fn sub_black_noise_reaches_the_demosaic_instead_of_being_clamped() {
    let tree = support::TempTree::new("raw-signed-black");
    let (width, height) = (16usize, 16usize);
    let cell = |i: usize| ((i % width) / 2 + (i / width) / 2).is_multiple_of(2);
    let straddling: Vec<u16> = (0..width * height)
        .map(|i| if cell(i) { 552 } else { 472 })
        .collect();
    let clamped: Vec<u16> = (0..width * height)
        .map(|i| if cell(i) { 552 } else { 512 })
        .collect();
    let mean = |name: &str, mosaic: Vec<u16>| {
        let path = write_raw(
            &tree,
            name,
            support::synthetic_dng_with(
                width,
                height,
                Orientation::Normal,
                None,
                Some(IDENTITY_MATRIX),
                512,
                Some(mosaic),
                None,
                None,
            ),
        );
        let rendered =
            autoshade::render::render_to_image(&path, &EditRecipe::default(), None, None)
                .unwrap()
                .to_rgb16();
        rendered
            .pixels()
            .map(|pixel| f64::from(pixel[0]) + f64::from(pixel[1]) + f64::from(pixel[2]))
            .sum::<f64>()
            / (3.0 * (width * height) as f64)
    };

    let straddling_mean = mean("straddling.dng", straddling);
    let clamped_mean = mean("clamped.dng", clamped);

    assert!(
        (straddling_mean / clamped_mean - 1.0).abs() > 0.05,
        "sub-black noise was clamped before the demosaic: {straddling_mean} vs {clamped_mean}"
    );
}

const XTRANS_XS10: &str = "GGRGGBGGBGGRBRGRBGGGBGGRGGRGGBRBGBRG";

fn unsharpened_recipe() -> EditRecipe {
    EditRecipe {
        sharpening: 0.0,
        explicit_zero: vec!["sharpening".to_string()],
        ..EditRecipe::default()
    }
}

fn xtrans_dng(
    tree: &support::TempTree,
    name: &str,
    width: usize,
    height: usize,
    truth: impl Fn(usize, usize) -> [f32; 3],
) -> std::path::PathBuf {
    let cfa = rawler::cfa::CFA::new(XTRANS_XS10);
    let mosaic = (0..width * height)
        .map(|i| {
            let (row, col) = (i / width, i % width);
            (truth(row, col)[cfa.color_at(row, col)] * 4095.0).round() as u16
        })
        .collect();
    write_raw(
        tree,
        name,
        support::synthetic_dng_with(
            width,
            height,
            Orientation::Normal,
            None,
            Some(SRGB_CAMERA_MATRIX),
            0,
            Some(mosaic),
            Some(XTRANS_XS10),
            None,
        ),
    )
}

#[test]
fn a_flat_x_trans_mosaic_renders_flat_at_every_cfa_phase() {
    let tree = support::TempTree::new("xtrans-flat");
    // 70 is not a whole number of 6-photo-site repeats, so the border ring
    // folds by the largest whole span inside the frame.
    let path = xtrans_dng(&tree, "flat.dng", 70, 70, |_, _| [0.2, 0.5, 0.3]);

    let rendered =
        autoshade::render::render_to_image(&path, &unsharpened_recipe(), None, None).unwrap();
    assert_eq!(rendered.dimensions(), (70, 70));
    let first = rendered.to_rgb16().get_pixel(0, 0).0;
    let worst = rendered
        .to_rgb16()
        .pixels()
        .flat_map(|pixel| pixel.0)
        .zip(first.iter().cycle())
        .map(|(value, reference)| value.abs_diff(*reference))
        .max()
        .unwrap();
    assert!(worst <= 2, "a flat X-Trans mosaic is not flat at every phase: {worst} codes");
}

fn srgb_encode(linear: f32) -> f32 {
    if linear <= 0.003_130_8 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    }
}

#[test]
fn a_four_colour_sensor_is_refused_by_name_and_renders_nothing() {
    let tree = support::TempTree::new("four-colour");
    let path = write_raw(
        &tree,
        "rgbe.dng",
        support::synthetic_dng_with(
            8,
            8,
            Orientation::Normal,
            None,
            Some(SRGB_CAMERA_MATRIX),
            0,
            None,
            Some("RGBE"),
            None,
        ),
    );

    let (error, output) = render_error(&path);
    let text = format!("{error:#}");
    assert!(text.contains("a 4-colour RGBC sensor"), "{text}");
    assert!(!output.exists(), "a refused render wrote {}", output.display());
}

#[test]
fn a_linear_gradient_on_x_trans_renders_without_tile_period_chroma() {
    let tree = support::TempTree::new("xtrans-gradient");
    let truth = |row: usize, col: usize| {
        let t = 0.1 + 0.6 * col as f32 / 71.0 + 0.2 * row as f32 / 71.0;
        [0.4 * t, t, 0.6 * t]
    };
    let path = xtrans_dng(&tree, "gradient.dng", 72, 72, truth);

    let rendered =
        autoshade::render::render_to_image(&path, &unsharpened_recipe(), None, None).unwrap();
    assert_eq!(rendered.dimensions(), (72, 72));
    let rgb = rendered.to_rgb16();
    let mut worst = 0u32;
    for (index, pixel) in rgb.pixels().enumerate() {
        let (row, col) = (index / 72, index % 72);
        if row < 8 || col < 8 || row + 8 >= 72 || col + 8 >= 72 {
            continue;
        }
        for channel in 0..3 {
            let expected = (srgb_encode(truth(row, col)[channel]) * 65535.0).round() as i32;
            worst = worst.max(i32::from(pixel.0[channel]).abs_diff(expected));
        }
    }
    assert!(worst <= 60, "the X-Trans gradient drifted {worst} codes from the truth at some interior pixel");
}

#[test]
fn an_x_trans_edge_does_not_ring() {
    let tree = support::TempTree::new("xtrans-edge");
    let path = xtrans_dng(&tree, "edge.dng", 72, 72, |_, col| {
            let t = if col < 36 { 0.3 } else { 0.7 };
            [0.4 * t, t, 0.6 * t]
        });

    let rendered =
        autoshade::render::render_to_image(&path, &unsharpened_recipe(), None, None).unwrap();
    let rgb = rendered.to_rgb16();
    let flat = |x: u32| rgb.get_pixel(x, 30).0;
    let (low, high) = (flat(5), flat(54));
    for pixel in rgb.pixels() {
        for channel in 0..3 {
            let value = i32::from(pixel.0[channel]);
            assert!(
                value <= i32::from(high[channel]) + 2 && value + 2 >= i32::from(low[channel]),
                "edge rang outside [{:?}, {:?}]: {:?}",
                low,
                high,
                pixel.0
            );
        }
    }
}

#[test]
#[ignore = "requires AUTOSHADE_FIT_CALIBRATION_DIR with p36-p42 Sony ARW files"]
fn real_sony_corpus_as_shot_wb_matches_camera_raws_grid() {
    let root = std::env::var_os("AUTOSHADE_FIT_CALIBRATION_DIR")
        .map(std::path::PathBuf::from)
        .expect("set AUTOSHADE_FIT_CALIBRATION_DIR to run the Sony as-shot gate");
    for (name, expected) in [
        ("p36", (4100.0, 4.0)),
        ("p37", (5550.0, 13.0)),
        ("p38", (5950.0, 13.0)),
        ("p39", (5400.0, 14.0)),
        ("p40", (7650.0, 16.0)),
        ("p41", (3800.0, -3.0)),
        ("p42", (6450.0, 11.0)),
    ] {
        let raw = root.join(format!("{name}.arw"));
        assert!(raw.is_file(), "{} is missing", raw.display());
        assert_eq!(
            autoshade::render::as_shot_wb(&raw),
            Some(expected),
            "{name}"
        );
    }
}
