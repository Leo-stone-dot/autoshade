use std::ffi::{OsStr, OsString};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

use rawler::dng::{
    CropMode, DNG_VERSION_V1_4, DngCompression, DngPhotometricConversion, writer::DngWriter,
};
use rawler::formats::tiff::Rational;
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
    as_shot_neutral: Option<&[Rational]>,
    with_color_matrix: bool,
) -> Vec<u8> {
    let mut camera = Camera::new();
    camera.cfa = CFA::new("RGGB");
    camera.clean_make = "Synthetic".to_string();
    camera.clean_model = "NoPreview".to_string();
    if with_color_matrix {
        camera.color_matrix.insert(
            Illuminant::D65,
            vec![1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        );
    }
    let pixels = (0..width * height)
        .map(|i| ((i * 37) % 4096) as u16)
        .collect::<Vec<_>>();
    let photometric = RawPhotometricInterpretation::Cfa(CFAConfig::new_from_camera(&camera));
    let raw = RawImage::new(
        camera,
        PixU16::new_with(pixels, width, height),
        1,
        [1.0, 1.0, 1.0, f32::NAN],
        photometric,
        Some(BlackLevel::new(&[0_u32, 0, 0, 0], 2, 2, 1)),
        Some(WhiteLevel::new_bits(12, 1)),
        false,
    );
    let mut buffer = Cursor::new(Vec::new());
    let mut dng = DngWriter::new(&mut buffer, DNG_VERSION_V1_4).unwrap();
    dng.load_base_tags(&raw).unwrap();
    let mut frame = dng.subframe(0);
    frame
        .raw_image(
            &raw,
            CropMode::None,
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
