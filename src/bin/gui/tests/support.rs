use std::ffi::{OsStr, OsString};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, MutexGuard};

static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);
static ENV_LOCK: Mutex<()> = Mutex::new(());

/// The real eframe app in a stable, windowless desktop environment.
pub fn headless_app() -> egui_kittest::Harness<'static, impl eframe::App> {
    egui_kittest::Harness::builder()
        .with_size([1400.0, 880.0])
        .with_pixels_per_point(1.0)
        .with_theme(egui::Theme::Dark)
        .with_os(egui::os::OperatingSystem::Nix)
        .with_max_steps(500)
        .with_step_dt(0.01)
        .build_eframe(|_| crate::AutoShadeApp::default())
}

pub fn drop_file<State: eframe::App>(
    harness: &mut egui_kittest::Harness<'_, State>,
    path: &Path,
) {
    harness.input_mut().dropped_files.push(egui::DroppedFile {
        path: Some(path.to_path_buf()),
        name: path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("photo")
            .to_string(),
        mime: String::new(),
        ..egui::DroppedFile::default()
    });
    harness.step();
    harness
        .try_run_realtime()
        .expect("the bounded headless app must settle after opening a tiny photo");
}

/// A serialized process-environment scope for the GUI unit-test binary.
pub struct ScopedEnv {
    previous: Vec<(&'static str, Option<OsString>)>,
    _guard: MutexGuard<'static, ()>,
}

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
pub struct TempTree(PathBuf);

impl TempTree {
    pub fn new(tag: &str) -> Self {
        let path = std::env::temp_dir().join(format!(
            "autoshade-gui-test-{}-{}-{tag}",
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
