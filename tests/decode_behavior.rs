use image::GenericImageView;
use rawler::formats::tiff::Rational;
use rawler::decoders::Orientation;

mod support;

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
    let neutral = [Rational::new(1, 2), Rational::new(1, 1), Rational::new(1, 4)];
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
    let neutral = [Rational::new(1, 1), Rational::new(1, 1), Rational::new(1, 1)];
    std::fs::write(
        &path,
        support::synthetic_dng(16, 8, Orientation::Normal, Some(&neutral), false),
    )
    .unwrap();

    assert!(autoshade::render::as_shot_wb(&path).is_none());
}
