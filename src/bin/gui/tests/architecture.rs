// One part of the GUI's tests (src/bin/gui/tests.rs includes it): the
// headless-logic canary for what a lint cannot see.
//
// The widget half is a clippy restriction lint now: `clippy.toml` lists the
// egui/eframe types, the crate root allows `disallowed_types`, and `mod logic`
// denies it — so `egui::Context` (and `rfd::FileDialog`) inside `logic/` is a
// compile error, not a grep hit. What remains here is the part clippy's type and method lints
// cannot express: crate-local runtime names (`Msg`, `ToastKind`,
// `AutoShadeApp`), free functions (`tr`/`trf`, `spawn_worker`) and thread
// spawning. A crate boundary would make all of them unreachable.

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
    /// MUTATION: call a worker, open a dialog or render text from a file under
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
            let plain = ["AutoShadeApp", "Msg::", "ToastKind", "std::thread"];
            for needle in calls.iter().chain(plain.iter()) {
                assert!(
                    !src.contains(needle),
                    "{} reaches into the view/runtime with `{needle}` — logic returns an Effect instead",
                    file.display()
                );
            }
        }
    }
