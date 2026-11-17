// One part of the GUI's tests (src/bin/gui/tests.rs includes it): the
// headless-logic canary.
//
// The panel seam is enforced by the projection types (`PanelView` and
// `Commands`), so there is no source scan for it here. The one rule a type
// cannot express is the module graph: `logic/` may not reach for the view or
// the runtime, and this scan is the only place that says so.

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
