use std::ffi::{OsStr, OsString};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use rawler::dng::{
    CropMode, DNG_VERSION_V1_4, DngCompression, DngPhotometricConversion, writer::DngWriter,
};
use rawler::formats::tiff::SRational;
use rawler::imgop::xyz::Illuminant;
use rawler::pixarray::PixU16;
use rawler::rawimage::{BlackLevel, RawPhotometricInterpretation, WhiteLevel};
use rawler::tags::{DngTag, ExifTag};
use rawler::{CFA, RawImage, decoders::Camera, rawimage::CFAConfig};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
#[allow(dead_code)]
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// A serialized process-environment scope for integration tests.
///
/// The mutex is per test binary and the previous values are restored when the
/// scope is dropped. Test cases should use this helper instead of writing the
/// process environment directly.
#[allow(dead_code)]
pub struct ScopedEnv {
    previous: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

#[allow(dead_code)]
impl ScopedEnv {
    #[allow(clippy::disallowed_methods)]
    pub fn new() -> Self {
        Self {
            previous: Vec::new(),
            _guard: ENV_LOCK.lock().unwrap_or_else(|poisoned| poisoned.into_inner()),
        }
    }

    #[allow(clippy::disallowed_methods)]
    pub fn set(&mut self, key: &'static str, value: impl AsRef<OsStr>) {
        if !self.previous.iter().any(|(previous_key, _)| *previous_key == key) {
            self.previous.push((key, std::env::var_os(key)));
        }
        // SAFETY: ScopedEnv holds the per-binary environment lock for its
        // entire lifetime, and Drop restores every value it changed.
        unsafe { std::env::set_var(key, value) };
    }
}

impl Drop for ScopedEnv {
    #[allow(clippy::disallowed_methods)]
    fn drop(&mut self) {
        // SAFETY: Drop runs while the environment lock is still held.
        for (key, previous) in self.previous.drain(..).rev() {
            match previous {
                Some(value) => unsafe { std::env::set_var(key, value) },
                None => unsafe { std::env::remove_var(key) },
            }
        }
    }
}

/// A per-process, per-case temporary tree.
///
/// Integration-test binaries run in separate processes. The atomic suffix keeps
/// cases inside one binary independent even when cargo runs them in parallel;
/// the tag only makes a leftover directory readable during diagnosis.
pub struct TempTree(PathBuf);

impl TempTree {
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "autoshade-test-{}-{}-{tag}",
            std::process::id(),
            NEXT_TEMP.fetch_add(1, Ordering::Relaxed),
        ));
        std::fs::create_dir_all(&path).expect("temporary test tree is creatable");
        Self(path)
    }

    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempTree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A stable, synthetic camera RAW with no embedded preview.
///
/// Tests use this when the behaviour depends on real RAW metadata but not on a
/// private photographic fixture. The writer's own failure is surfaced rather
/// than allowing an empty file to turn the case into a false pass.
#[allow(dead_code)]
pub fn synthetic_dng(
    width: usize,
    height: usize,
    orientation: rawler::decoders::Orientation,
    as_shot_neutral: Option<&[SRational]>,
    with_color_matrix: bool,
) -> Vec<u8> {
    synthetic_dng_with(
        width,
        height,
        orientation,
        as_shot_neutral,
        with_color_matrix.then_some([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]),
        0,
        None,
        None,
        None,
    )
}

/// [`synthetic_dng`] with the calibration a case needs to vary: the D65
/// colour matrix (row-major xyz→cam), the black level every CFA site shares,
/// the mosaic itself, the CFA repeat pattern (`None` = RGGB) and an active
/// area offset from the frame origin. A supplied mosaic must fill the frame; a
/// short one panics rather than turning the case into a false pass.
#[allow(dead_code)]
#[allow(clippy::too_many_arguments)] // one field per DNG tag the fixture can vary
pub fn synthetic_dng_with(
    width: usize,
    height: usize,
    orientation: rawler::decoders::Orientation,
    as_shot_neutral: Option<&[SRational]>,
    color_matrix: Option<[f32; 9]>,
    black_level: u16,
    pixels: Option<Vec<u16>>,
    cfa: Option<&str>,
    active_area: Option<rawler::imgop::Rect>,
) -> Vec<u8> {
    let mut camera = Camera::new();
    camera.cfa = CFA::new(cfa.unwrap_or("RGGB"));
    camera.clean_make = "Synthetic".to_string();
    camera.clean_model = "NoPreview".to_string();
    if let Some(matrix) = color_matrix {
        camera.color_matrix.insert(
            Illuminant::D65,
            matrix.to_vec(),
        );
    }
    let pixels = pixels.unwrap_or_else(|| {
        (0..width * height)
            .map(|i| ((i * 37) % 4096) as u16)
            .collect()
    });
    assert_eq!(
        pixels.len(),
        width * height,
        "the synthetic mosaic must fill the declared frame"
    );
    let photometric = RawPhotometricInterpretation::Cfa(CFAConfig::new_from_camera(&camera));
    let mut raw = RawImage::new(
        camera,
        PixU16::new_with(pixels, width, height),
        1,
        [1.0, 1.0, 1.0, f32::NAN],
        photometric,
        Some(BlackLevel::new(&[u32::from(black_level); 4], 2, 2, 1)),
        Some(WhiteLevel::new_bits(12, 1)),
        false,
    );
    raw.active_area = active_area;
    let mut buffer = Cursor::new(Vec::new());
    let mut dng = DngWriter::new(&mut buffer, DNG_VERSION_V1_4).unwrap();
    dng.load_base_tags(&raw).unwrap();
    let mut frame = dng.subframe(0);
    frame
        .raw_image(
            &raw,
            if active_area.is_some() {
                CropMode::ActiveArea
            } else {
                CropMode::None
            },
            DngCompression::Uncompressed,
            DngPhotometricConversion::Original,
            1,
        )
        .unwrap();
    frame.finalize().unwrap();
    if let Some(neutral) = as_shot_neutral {
        dng.root_ifd_mut().add_tag(DngTag::AsShotNeutral, neutral);
    }
    dng.root_ifd_mut()
        .add_tag(ExifTag::Orientation, orientation.to_u16());
    dng.close().unwrap();
    buffer.into_inner()
}
