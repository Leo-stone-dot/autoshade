use super::*;

#[test]
fn fresh_reset_calibration_matches_the_initial_open_and_ignores_saved_edits() {
    let tree = TempTree::new("http-fresh-base");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());

    let initial = response(request(port, "GET", "/api/recipe?id=0").call());
    assert_eq!(initial.status(), 404);
    let initial: Value = initial.into_json().unwrap();
    let fresh_before: Value = request(port, "GET", "/api/fresh-base?id=0")
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(fresh_before, initial);

    save(port, &session_token(port), listing_generation(port), 1.25);
    assert_eq!(saved(port).0["exposure_ev"], 1.25);
    let fresh_after: Value = request(port, "GET", "/api/fresh-base?id=0")
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    assert_eq!(fresh_after, fresh_before);
}

fn control<'a>(controls: &'a Value, name: &str) -> &'a Value {
    controls["controls"]
        .as_array()
        .unwrap()
        .iter()
        .find(|entry| entry["name"] == name)
        .unwrap_or_else(|| panic!("the public control catalogue has no {name}"))
}

#[test]
fn web_control_ranges_are_unique_and_contain_their_public_defaults() {
    let tree = TempTree::new("http-controls");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());

    let controls: Value = request(port, "GET", "/api/controls")
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    let rows = controls["controls"].as_array().unwrap();
    assert!(!rows.is_empty());
    let mut names = std::collections::HashSet::new();
    for row in rows {
        assert!(names.insert(row["name"].as_str().unwrap()));
        let lo = row["range"][0].as_f64().unwrap();
        let hi = row["range"][1].as_f64().unwrap();
        assert!(lo < hi);
        if let Some(default) = row["default"].as_f64() {
            assert!((lo..=hi).contains(&default));
        }
    }
    assert_eq!(
        control(&controls, "exposure_ev")["range"],
        json!([-5.0, 5.0])
    );
    assert_eq!(control(&controls, "exposure_ev")["default"], 0.0);
    assert_eq!(
        control(&controls, "temperature_k")["range"],
        json!([2000.0, 50000.0])
    );
    assert!(control(&controls, "temperature_k")["default"].is_null());
}

#[cfg(unix)]
struct LibraryFixture {
    photos: PathBuf,
    script: PathBuf,
    vector: PathBuf,
    placeholder: PathBuf,
}

#[cfg(unix)]
impl LibraryFixture {
    fn new(tree: &TempTree) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let photos = tree.path().join("finished");
        std::fs::create_dir_all(&photos).unwrap();
        for (name, color) in [("first.png", [40, 80, 120]), ("second.png", [120, 80, 40])] {
            RgbImage::from_pixel(16, 12, Rgb(color))
                .save(photos.join(name))
                .unwrap();
        }
        let mut values = vec![0.0f32; autoshade::embed::EMBED_DIM];
        values[0] = 1.0;
        let vector = tree.path().join("vector.json");
        std::fs::write(&vector, serde_json::to_vec(&values).unwrap()).unwrap();
        let placeholder = tree.path().join("embed.py");
        std::fs::write(&placeholder, b"fixture").unwrap();
        let script = tree.path().join("embed-fixture");
        std::fs::write(
            &script,
            "#!/bin/sh\nset -eu\nshift\nmanifest=\nout=\nwhile [ $# -gt 0 ]; do\ncase \"$1\" in\n--manifest-jsonl) manifest=$2; shift 2;;\n--output) out=$2; shift 2;;\n*) shift;;\nesac\ndone\nvec=$(cat \"$AUTOSHADE_TEST_VECTOR\")\n: > \"$out\"\nwhile IFS= read -r line; do\npath=$(printf '%s\\n' \"$line\" | sed -n 's/.*\"path\":\"\\([^\"]*\\)\".*/\\1/p')\n[ -n \"$path\" ] || continue\nprintf '{\"path\":\"%s\",\"vector\":%s}\\n' \"$path\" \"$vec\" >> \"$out\"\ndone < \"$manifest\"\n",
        )
        .unwrap();
        std::fs::set_permissions(&script, std::fs::Permissions::from_mode(0o755)).unwrap();
        Self {
            photos,
            script,
            vector,
            placeholder,
        }
    }

    fn environment(&self) -> [(&str, &std::ffi::OsStr); 3] {
        [
            ("AUTOSHADE_PYTHON", self.script.as_os_str()),
            ("AUTOSHADE_EMBED_SCRIPT", self.placeholder.as_os_str()),
            ("AUTOSHADE_TEST_VECTOR", self.vector.as_os_str()),
        ]
    }

    fn fail(&self) {
        std::fs::write(&self.script, "#!/bin/sh\nexit 7\n").unwrap();
    }
}

fn library_build(port: u16, token: &str, directory: &Path) -> ureq::Response {
    response(
        request(port, "POST", "/api/library-build")
            .set("X-AutoShade-Token", token)
            .send_json(json!({"dir": directory})),
    )
}

fn library_info(port: u16) -> Value {
    request(port, "GET", "/api/library-info")
        .call()
        .unwrap()
        .into_json()
        .unwrap()
}

#[cfg(unix)]
#[test]
fn a_web_library_build_is_published_and_reported_by_library_info() {
    let tree = TempTree::new("http-library-build");
    let photo = fixture(&tree);
    let library = LibraryFixture::new(&tree);
    let (_server, port) =
        start_server_with_env(&tree, photo.parent().unwrap(), &library.environment());
    assert_eq!(library_info(port)["library"]["built"], false);

    let built: Value = library_build(port, &session_token(port), &library.photos)
        .into_json()
        .unwrap();
    let info = library_info(port);

    assert_eq!(built["ok"], true);
    assert_eq!(built["total"], 2);
    assert_eq!(info["library"]["built"], true);
    assert_eq!(info["library"]["total"], 2);
    assert_eq!(
        std::path::Path::new(info["library"]["source_dir"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        library.photos.canonicalize().unwrap()
    );
    let file = PathBuf::from(info["library"]["file"].as_str().unwrap());
    let document: Value = serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap();
    assert_eq!(document["entries"].as_array().unwrap().len(), 2);
}

#[cfg(unix)]
#[test]
fn a_failed_web_library_rebuild_keeps_the_previous_library_available() {
    let tree = TempTree::new("http-library-failure");
    let photo = fixture(&tree);
    let library = LibraryFixture::new(&tree);
    let (_server, port) =
        start_server_with_env(&tree, photo.parent().unwrap(), &library.environment());
    let token = session_token(port);
    assert_eq!(library_build(port, &token, &library.photos).status(), 200);
    let before = library_info(port);
    let file = PathBuf::from(before["library"]["file"].as_str().unwrap());
    let bytes = std::fs::read(&file).unwrap();
    RgbImage::from_pixel(16, 12, Rgb([10, 20, 30]))
        .save(library.photos.join("third.png"))
        .unwrap();
    library.fail();

    let failed = library_build(port, &token, &library.photos);

    assert_eq!(failed.status(), 400);
    assert_eq!(library_info(port), before);
    assert_eq!(std::fs::read(file).unwrap(), bytes);
}

#[test]
fn analyze_without_a_harness_key_refuses_before_decoding_or_saving() {
    let tree = TempTree::new("http-analyze-key");
    let dir = tree.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let photo = dir.join("broken.png");
    let source = b"not a decodable image";
    std::fs::write(&photo, source).unwrap();
    let (_server, port) = start_server_with_env(
        &tree,
        &dir,
        &[("AUTOSHADE_PIPELINE", std::ffi::OsStr::new("legacy"))],
    );

    let result = response(
        request(port, "POST", "/api/analyze")
            .set("X-AutoShade-Token", &session_token(port))
            .set("X-AutoShade-Gen", &listing_generation(port).to_string())
            .send_json(json!({"id": 0, "guidance": "make it brighter"})),
    );
    assert_eq!(result.status(), 500);
    let body = result.into_string().unwrap();

    assert!(body.contains("no key for the pi harness"), "{body}");
    assert!(!body.contains("decode image"), "{body}");
    assert_eq!(
        response(request(port, "GET", "/api/recipe?id=0").call()).status(),
        404
    );
    assert_eq!(std::fs::read(photo).unwrap(), source);
}

#[test]
fn library_guidance_refuses_a_typed_direction_without_modifying_the_photo() {
    let tree = TempTree::new("http-analyze-guide-conflict");
    let photo = fixture(&tree);
    let source = std::fs::read(&photo).unwrap();
    let (_server, port) = start_server(&tree, photo.parent().unwrap());

    let result = response(
        request(port, "POST", "/api/analyze")
            .set("X-AutoShade-Token", &session_token(port))
            .set("X-AutoShade-Gen", &listing_generation(port).to_string())
            .send_json(json!({
                "id": 0,
                "guide": "library",
                "guidance": "make it warmer",
            })),
    );

    assert_eq!(result.status(), 400);
    assert!(result.into_string().unwrap().contains("library"));
    assert_eq!(
        response(request(port, "GET", "/api/recipe?id=0").call()).status(),
        404
    );
    assert_eq!(std::fs::read(photo).unwrap(), source);
}
