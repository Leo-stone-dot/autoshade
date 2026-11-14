// One part of the GUI's tests (src/bin/gui/tests.rs includes it): the
// architecture seam between the view, the headless logic and the runtime.
//
// These are SOURCE tests on purpose: the drift they guard against is a line
// that goes missing or an import that creeps back, and neither has a runtime
// signature until the next feature pays for it. The pair pins the deal made in
// `logic/mod.rs` and `panels/lens_blur.rs`:
//
//  * logic is headless — no widget code, no rendered text, no worker spawn,
//    no dialog, no message enum;
//  * a migrated panel does not mutate the recipe, the app's own state or the
//    filesystem: it draws and emits `logic::Action`s.

    fn collect_rs(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        for entry in std::fs::read_dir(dir).expect("the source tree is listable") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                collect_rs(&path, out);
            } else if path.extension().is_some_and(|e| e == "rs") {
                out.push(path);
            }
        }
    }

    /// The logic layer is headless: it may read and write data, but reaching
    /// for the view or the runtime is what turned one policy change into
    /// hundreds of edits.
    ///
    /// MUTATION: call `egui::Context` (or a worker, or `tr`) from a file under
    /// `logic/` and this names the file and the needle.
    #[test]
    fn the_logic_layer_is_headless() {
        let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("bin")
            .join("gui")
            .join("logic");
        let mut files = Vec::new();
        collect_rs(&dir, &mut files);
        assert!(!files.is_empty(), "no logic sources found under {}", dir.display());
        for file in files {
            let src = std::fs::read_to_string(&file).expect("logic source readable");
            // Spellings the Rust-blind scanners (this one's own source included)
            // would otherwise read as real call sites are built at compile time:
            // a quoted call spelling reads as a call to the i18n audit.
            let calls = [concat!("tr", "("), concat!("trf", "("), concat!("spawn_worker", "(")];
            let plain = ["egui::", "AutoShadeApp", "rfd::", "Msg::", "ToastKind", "std::thread"];
            for needle in calls.iter().chain(plain.iter()) {
                assert!(
                    !src.contains(needle),
                    "{} reaches into the view/runtime with `{needle}` — logic returns an Effect instead",
                    file.display()
                );
            }
        }
    }

    /// The pilot panel is view-only: it reads the recipe and the app's flags,
    /// draws, and returns actions. A mutation, a worker, a dialog or a store
    /// write here would re-fork the business logic this migration removed.
    ///
    /// MUTATION: put a recipe `as_mut` call or a worker spawn back in the panel
    /// and this fails with the needle.
    #[test]
    fn the_migrated_panel_stays_behind_the_seam() {
        let file = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src")
            .join("bin")
            .join("gui")
            .join("panels")
            .join("lens_blur.rs");
        let src = std::fs::read_to_string(&file).expect("the panel source is readable");
        let spawn = concat!("spawn_worker", "(");
        for needle in [
            spawn,
            "rfd::",
            "std::fs::",
            "autoshade::store::",
            "Msg::",
            "ToastKind",
            "on_depth_estimated",
            "start_depth",
            ".as_mut()",
            "self.status =",
            "self.toast(",
            "self.busy =",
            "self.dirty =",
        ] {
            assert!(
                !src.contains(needle),
                "panels/lens_blur.rs reaches past the seam with `{needle}` — emit a logic::Action instead"
            );
        }
    }
