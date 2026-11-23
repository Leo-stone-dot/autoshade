#[cfg(unix)]
mod http_analyze;
#[cfg(unix)]
mod http_fill;
mod http_limits;
mod http_public;
mod http_runtime;
mod http_settings;
mod http_workflows;
mod support;

use std::io::{BufRead, BufReader, Cursor, Read, Write};
use std::net::TcpStream;
use std::path::{Path, PathBuf};
use std::process::{Child, Stdio};
use std::sync::{Barrier, mpsc};
use std::time::Duration;

use image::{GenericImageView, Rgb, RgbImage};
use serde_json::{Value, json};
use support::TempTree;

struct RunningServer {
    child: Child,
    group: Option<autoshade::KillGroup>,
    output: Option<std::thread::JoinHandle<()>>,
}

impl Drop for RunningServer {
    fn drop(&mut self) {
        if let Some(group) = &self.group {
            group.kill_tree();
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(output) = self.output.take() {
            output.join().unwrap();
        }
    }
}

fn start_server(tree: &TempTree, dir: &Path) -> (RunningServer, u16) {
    start_server_with_env(tree, dir, &[])
}

fn start_server_with_env(
    tree: &TempTree,
    dir: &Path,
    environment: &[(&str, &std::ffi::OsStr)],
) -> (RunningServer, u16) {
    let temporary = tree.path().join("tmp");
    std::fs::create_dir_all(&temporary).unwrap();
    let stderr = tree.path().join("server.stderr");
    let mut command = autoshade::sidecar_command(env!("CARGO_BIN_EXE_autoshade"));
    command
        .arg("serve")
        .arg(dir)
        .arg("--port")
        .arg("0")
        .current_dir(tree.path())
        .env_clear()
        .env("HOME", tree.path())
        .env("TMPDIR", &temporary)
        .env("TMP", &temporary)
        .env("TEMP", &temporary)
        .env("AUTOSHADE_DATA_DIR", tree.path().join("data"))
        .env("AUTOSHADE_OUT_DIR", tree.path().join("out"))
        .stdout(Stdio::piped())
        .stderr(Stdio::from(std::fs::File::create(&stderr).unwrap()));
    for (key, value) in environment {
        command.env(key, value);
    }
    for key in ["SYSTEMROOT", "LLVM_PROFILE_FILE"] {
        if let Some(value) = std::env::var_os(key) {
            command.env(key, value);
        }
    }
    let mut child = autoshade::spawn_child(&mut command).unwrap();
    let group = autoshade::assign_kill_group(&child);
    let stdout = child.stdout.take().unwrap();
    let (tx, rx) = mpsc::channel();
    let output = std::thread::Builder::new()
        .spawn(move || {
            for line in BufReader::new(stdout).lines() {
                let Ok(line) = line else { break };
                for word in line.split_whitespace() {
                    if let Some(port) = word
                        .strip_prefix("http://127.0.0.1:")
                        .and_then(|port| port.parse::<u16>().ok())
                    {
                        let _ = tx.send(port);
                    }
                }
            }
        })
        .unwrap();
    let server = RunningServer {
        child,
        group,
        output: Some(output),
    };
    let port = rx
        .recv_timeout(Duration::from_secs(10))
        .unwrap_or_else(|error| {
            panic!(
                "server did not publish its listening URL: {error}; {}",
                std::fs::read_to_string(stderr).unwrap()
            );
        });
    (server, port)
}

fn request(port: u16, method: &str, path: &str) -> ureq::Request {
    ureq::AgentBuilder::new()
        .timeout(Duration::from_secs(10))
        .build()
        .request(method, &format!("http://127.0.0.1:{port}{path}"))
}

fn response(result: Result<ureq::Response, ureq::Error>) -> ureq::Response {
    match result {
        Ok(response) | Err(ureq::Error::Status(_, response)) => response,
        Err(error) => panic!("HTTP transport failed: {error}"),
    }
}

fn session_token(port: u16) -> String {
    let body = request(port, "GET", "/")
        .call()
        .unwrap()
        .into_string()
        .unwrap();
    let marker = "/api/thumb?token=";
    let start = body.find(marker).unwrap() + marker.len();
    let end = body[start..].find('&').unwrap() + start;
    body[start..end].to_string()
}

fn listing_generation(port: u16) -> u64 {
    let list: Value = request(port, "GET", "/api/list")
        .call()
        .unwrap()
        .into_json()
        .unwrap();
    list["gen"].as_u64().unwrap()
}

fn post(port: u16, path: &str, token: &str, generation: u64, body: &str) -> ureq::Response {
    request(port, "POST", path)
        .set("Content-Type", "application/json")
        .set("X-AutoShade-Token", token)
        .set("X-AutoShade-Gen", &generation.to_string())
        .send_string(body)
        .unwrap()
}

fn fixture(tree: &TempTree) -> PathBuf {
    let dir = tree.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let photo = dir.join("photo.png");
    RgbImage::from_pixel(16, 12, Rgb([48, 96, 144]))
        .save(&photo)
        .unwrap();
    photo
}

#[test]
fn a_http_export_publishes_one_complete_registered_file() {
    let tree = TempTree::new("serve-export");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);

    let response = post(
        port,
        "/api/export",
        &token,
        generation,
        r#"{"id":0,"recipe":{},"format":"jpg"}"#,
    );

    assert_eq!(response.status(), 200);
    let published = PathBuf::from(response.into_string().unwrap());
    let bytes = std::fs::read(&published).unwrap();
    assert_eq!(
        image::load_from_memory(&bytes).unwrap().dimensions(),
        (16, 12)
    );
    assert!(published.starts_with(tree.path().join("out")));
}

#[test]
fn a_http_download_streams_one_complete_image_without_a_persistent_delivery() {
    let tree = TempTree::new("serve-download");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);

    let response = post(
        port,
        "/api/download",
        &token,
        generation,
        r#"{"id":0,"recipe":{},"format":"jpg"}"#,
    );
    let mut bytes = Vec::new();
    response.into_reader().read_to_end(&mut bytes).unwrap();

    assert_eq!(
        image::load_from_memory(&bytes).unwrap().dimensions(),
        (16, 12)
    );
    assert!(
        !tree.path().join("out").exists()
            || std::fs::read_dir(tree.path().join("out"))
                .unwrap()
                .next()
                .is_none()
    );
}

#[test]
fn a_web_preview_keeps_the_full_frame_for_slider_feedback() {
    let tree = TempTree::new("serve-preview-keep");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);

    let response = post(
        port,
        "/api/develop",
        &token,
        generation,
        r#"{"id":0,"recipe":{"crop":{"left":0.25,"top":0.25,"right":0.75,"bottom":0.75}}}"#,
    );
    let mut bytes = Vec::new();
    response.into_reader().read_to_end(&mut bytes).unwrap();
    let preview = image::load_from_memory(&bytes).unwrap();

    assert_eq!(preview.dimensions(), (16, 12));
}

fn save_request(port: u16, token: &str, generation: u64) -> ureq::Request {
    request(port, "POST", "/api/xmp")
        .set("Content-Type", "application/json")
        .set("X-AutoShade-Token", token)
        .set("X-AutoShade-Gen", &generation.to_string())
}

fn save(port: u16, token: &str, generation: u64, exposure: f32) -> String {
    let result = save_request(port, token, generation)
        .send_json(json!({"id": 0, "recipe": {"exposure_ev": exposure}}))
        .unwrap();
    result
        .header("ETag")
        .expect("a saved develop has a revision")
        .to_string()
}

fn saved(port: u16) -> (Value, String) {
    let result = request(port, "GET", "/api/recipe?id=0").call().unwrap();
    let tag = result
        .header("ETag")
        .expect("a read develop has a revision")
        .to_string();
    (result.into_json().unwrap(), tag)
}

fn gallery(port: u16) -> Value {
    request(port, "GET", "/api/list")
        .call()
        .unwrap()
        .into_json()
        .unwrap()
}

#[test]
fn a_saved_web_edit_reopens_with_its_revision_without_changing_the_source() {
    let tree = TempTree::new("http-save");
    let photo = fixture(&tree);
    let original = std::fs::read(&photo).unwrap();
    let (server, port) = start_server(&tree, photo.parent().unwrap());
    let tag = save(port, &session_token(port), listing_generation(port), 0.75);
    drop(server);

    let (_reopened, port) = start_server(&tree, photo.parent().unwrap());
    let (recipe, reopened_tag) = saved(port);

    assert_eq!(recipe["exposure_ev"], 0.75);
    assert_eq!(reopened_tag, tag);
    assert_eq!(std::fs::read(photo).unwrap(), original);
}

#[test]
fn a_save_without_the_current_session_token_leaves_the_saved_edit_unchanged() {
    let tree = TempTree::new("http-token");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let generation = listing_generation(port);
    save(port, &session_token(port), generation, 0.75);
    let before = saved(port);

    for token in [None, Some("another-session")] {
        let mut req =
            request(port, "POST", "/api/xmp").set("X-AutoShade-Gen", &generation.to_string());
        if let Some(token) = token {
            req = req.set("X-AutoShade-Token", token);
        }
        let result = response(req.send_json(json!({"id": 0, "recipe": {"exposure_ev": 1.5}})));
        assert_eq!(result.status(), 403);
        assert!(result.into_string().unwrap().contains("session token"));
        assert_eq!(saved(port), before);
    }
}

#[test]
fn a_foreign_web_origin_cannot_modify_a_saved_photo_even_with_a_session_token() {
    let tree = TempTree::new("http-origin");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    save(port, &token, generation, 0.75);
    let before = saved(port);

    for (header, value) in [
        ("Origin", "http://example.invalid".to_string()),
        (
            "Origin",
            format!("http://127.0.0.1:{}", if port == 80 { 81 } else { 80 }),
        ),
        ("Host", format!("example.invalid:{port}")),
        ("Sec-Fetch-Site", "cross-site".to_string()),
    ] {
        let result = response(
            save_request(port, &token, generation)
                .set(header, &value)
                .send_json(json!({"id": 0, "recipe": {"exposure_ev": 1.5}})),
        );
        assert_eq!(result.status(), 403, "{header}: {value}");
        assert_eq!(saved(port), before);
    }
}

#[test]
fn photo_images_are_readable_only_with_the_current_image_capability() {
    let tree = TempTree::new("http-images");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);

    for endpoint in ["/api/thumb", "/api/preview"] {
        for suffix in ["", "&token=another-session"] {
            let result = response(request(port, "GET", &format!("{endpoint}?id=0{suffix}")).call());
            assert_eq!(result.status(), 403);
        }
        let result = request(port, "GET", &format!("{endpoint}?id=0&token={token}"))
            .call()
            .unwrap();
        let mut bytes = Vec::new();
        result.into_reader().read_to_end(&mut bytes).unwrap();
        let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
        let pixel = image.get_pixel(image.width() / 2, image.height() / 2);
        for (actual, expected) in pixel.0.into_iter().zip([48i16, 96, 144]) {
            assert!((i16::from(actual) - expected).abs() <= 3);
        }
    }
}

fn switch_folder(tree: &TempTree, port: u16, token: &str) -> Value {
    let dir = tree.path().join("other-photos");
    std::fs::create_dir_all(&dir).unwrap();
    RgbImage::from_pixel(16, 12, Rgb([180, 30, 60]))
        .save(dir.join("other.png"))
        .unwrap();
    request(port, "POST", "/api/setdir")
        .set("X-AutoShade-Token", token)
        .send_json(json!({"path": dir}))
        .unwrap()
        .into_json()
        .unwrap()
}

#[test]
fn a_save_for_the_previous_folder_cannot_edit_the_same_id_in_the_new_folder() {
    let tree = TempTree::new("http-stale-folder");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let old_generation = listing_generation(port);
    let new_listing = switch_folder(&tree, port, &token);
    assert_ne!(new_listing["gen"], old_generation);

    let result = response(
        save_request(port, &token, old_generation)
            .send_json(json!({"id": 0, "recipe": {"exposure_ev": 1.5}})),
    );

    assert_eq!(result.status(), 409);
    assert!(result.into_string().unwrap().contains("gallery moved"));
    assert_eq!(
        response(request(port, "GET", "/api/recipe?id=0").call()).status(),
        404
    );
}

fn read_http_head(reader: &mut impl BufRead) -> u16 {
    let mut line = String::new();
    reader.read_line(&mut line).unwrap();
    let status = line
        .split_whitespace()
        .nth(1)
        .unwrap_or_else(|| panic!("invalid HTTP status: {line}"))
        .parse()
        .unwrap();
    loop {
        line.clear();
        assert_ne!(
            reader.read_line(&mut line).unwrap(),
            0,
            "truncated HTTP headers"
        );
        if line == "\r\n" {
            return status;
        }
    }
}

#[test]
fn a_folder_switch_during_body_upload_cannot_redirect_the_pending_save() {
    let tree = TempTree::new("http-inflight-folder");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    let body = br#"{"id":0,"recipe":{"exposure_ev":1.5}}"#;
    let mut socket = TcpStream::connect(("127.0.0.1", port)).unwrap();
    socket
        .set_read_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    socket
        .set_write_timeout(Some(Duration::from_secs(10)))
        .unwrap();
    write!(
        socket,
        "POST /api/xmp HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\n\
        X-AutoShade-Token: {token}\r\nX-AutoShade-Gen: {generation}\r\n\
        Content-Length: {}\r\nExpect: 100-continue\r\nConnection: close\r\n\r\n",
        body.len()
    )
    .unwrap();
    let mut reader = BufReader::new(socket);
    // The protocol acknowledgement places the switch after dispatch but before body completion.
    assert_eq!(read_http_head(&mut reader), 100);
    let new_listing = switch_folder(&tree, port, &token);
    assert_ne!(new_listing["gen"], generation);
    reader.get_mut().write_all(body).unwrap();

    assert_eq!(read_http_head(&mut reader), 409);
    assert_eq!(
        response(request(port, "GET", "/api/recipe?id=0").call()).status(),
        404
    );
}

#[test]
fn an_outdated_revision_cannot_overwrite_a_newer_saved_edit() {
    let tree = TempTree::new("http-stale-save");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    let old_tag = save(port, &token, generation, 0.75);
    let current_tag = save(port, &token, generation, 1.25);
    assert_ne!(old_tag, current_tag);
    let before = saved(port);

    let result = response(
        save_request(port, &token, generation)
            .set("If-Match", &old_tag)
            .send_json(json!({"id": 0, "recipe": {"exposure_ev": -0.5}})),
    );

    assert_eq!(result.status(), 412);
    assert_eq!(result.header("ETag"), Some(current_tag.as_str()));
    assert!(
        result
            .into_string()
            .unwrap()
            .contains("nothing was overwritten")
    );
    assert_eq!(saved(port), before);
}

#[test]
fn an_outdated_reset_cannot_delete_a_newer_saved_edit() {
    let tree = TempTree::new("http-stale-clear");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    let old_tag = save(port, &token, generation, 0.75);
    save(port, &token, generation, 1.25);
    let before = saved(port);

    let result = response(
        save_request(port, &token, generation)
            .set("If-Match", &old_tag)
            .send_json(json!({"id": 0, "recipe": {}})),
    );

    assert_eq!(result.status(), 412);
    assert_eq!(saved(port), before);
}

#[test]
fn concurrent_saves_from_one_revision_accept_exactly_one_edit() {
    let tree = TempTree::new("http-concurrent-save");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    let tag = save(port, &token, generation, 0.75);
    let barrier = Barrier::new(2);

    let results = std::thread::scope(|scope| {
        let handles = [1.25, -0.5].map(|exposure| {
            let (token, tag, barrier) = (&token, &tag, &barrier);
            scope.spawn(move || {
                barrier.wait();
                let result = response(
                    save_request(port, token, generation)
                        .set("If-Match", tag)
                        .send_json(json!({"id": 0, "recipe": {"exposure_ev": exposure}})),
                );
                (
                    exposure,
                    result.status(),
                    result.header("ETag").unwrap().to_string(),
                )
            })
        });
        handles.map(|h| h.join().unwrap())
    });
    let mut statuses = results.each_ref().map(|r| r.1);
    statuses.sort();
    assert_eq!(statuses, [200, 412]);
    let winner = results.iter().find(|r| r.1 == 200).unwrap();
    let (recipe, tag) = saved(port);
    assert_eq!(recipe["exposure_ev"], winner.0);
    assert_eq!(tag, winner.2);
}

#[test]
fn an_unissued_pixel_master_is_refused_without_committing_the_edit() {
    let tree = TempTree::new("http-master");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    save(port, &token, generation, 0.75);
    let before = saved(port);

    let result = response(
        save_request(port, &token, generation)
            .send_json(json!({"id": 0, "recipe": {"exposure_ev": 1.5}, "master": photo})),
    );

    assert_eq!(result.status(), 400);
    assert!(
        result
            .into_string()
            .unwrap()
            .contains("not issued for this photo")
    );
    assert_eq!(saved(port), before);
}

#[test]
fn malformed_save_bodies_report_client_errors_without_changing_the_edit() {
    let tree = TempTree::new("http-json");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let generation = listing_generation(port);
    save(port, &token, generation, 0.75);
    let before = saved(port);

    for (body, message) in [
        (b"{".as_slice(), "parse request JSON"),
        (&[0xff], "not UTF-8"),
    ] {
        let result = response(save_request(port, &token, generation).send_bytes(body));
        assert_eq!(result.status(), 400);
        assert!(result.into_string().unwrap().contains(message));
        assert_eq!(saved(port), before);
    }
}

#[test]
fn an_empty_upload_is_refused_without_adding_a_gallery_item() {
    let tree = TempTree::new("http-empty-upload");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let before = gallery(port);

    let result = response(
        request(port, "POST", "/api/upload")
            .set("X-AutoShade-Token", &session_token(port))
            .set("X-Filename", "empty.png")
            .send_bytes(&[]),
    );

    assert_eq!(result.status(), 400);
    assert!(result.into_string().unwrap().contains("empty"));
    assert_eq!(gallery(port), before);
}

#[test]
fn concurrent_same_named_uploads_preserve_both_photos() {
    let tree = TempTree::new("http-upload-collision");
    let photo = fixture(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let barrier = Barrier::new(2);
    let images = [[220, 30, 30], [30, 220, 30]].map(|color| {
        let mut bytes = Cursor::new(Vec::new());
        RgbImage::from_pixel(16, 12, Rgb(color))
            .write_to(&mut bytes, image::ImageFormat::Png)
            .unwrap();
        bytes.into_inner()
    });

    let results = std::thread::scope(|scope| {
        let handles = images.each_ref().map(|bytes| {
            let (token, barrier) = (&token, &barrier);
            scope.spawn(move || {
                barrier.wait();
                request(port, "POST", "/api/upload")
                    .set("X-AutoShade-Token", token)
                    .set("X-Filename", "same.png")
                    .send_bytes(bytes)
                    .unwrap()
                    .into_json::<Value>()
                    .unwrap()
            })
        });
        handles.map(|h| h.join().unwrap())
    });

    assert_ne!(results[0]["id"], results[1]["id"]);
    for (result, color) in results.iter().zip([[220i16, 30, 30], [30, 220, 30]]) {
        let url = format!("/api/preview?id={}&token={token}", result["id"]);
        let mut bytes = Vec::new();
        request(port, "GET", &url)
            .call()
            .unwrap()
            .into_reader()
            .read_to_end(&mut bytes)
            .unwrap();
        let image = image::load_from_memory(&bytes).unwrap().to_rgb8();
        assert_eq!(image.dimensions(), (16, 12));
        for (actual, expected) in image.get_pixel(8, 6).0.into_iter().zip(color) {
            assert!(
                (i16::from(actual) - expected).abs() <= 3,
                "the uploaded photo was replaced"
            );
        }
    }
}

#[test]
fn importing_the_same_photo_twice_adds_only_one_gallery_item() {
    let tree = TempTree::new("http-import");
    let photo = fixture(&tree);
    let other = tree.path().join("extra.png");
    RgbImage::from_pixel(16, 12, Rgb([180, 30, 60]))
        .save(&other)
        .unwrap();
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    let before = gallery(port);
    let import = || {
        request(port, "POST", "/api/import")
            .set("X-AutoShade-Token", &token)
            .send_json(json!({"path": other}))
            .unwrap()
            .into_json::<Value>()
            .unwrap()
    };

    let first = import();
    let second = import();

    assert_eq!(first["added"], 1);
    assert_eq!(second["added"], 0);
    let after = gallery(port);
    assert_eq!(
        after["total"].as_u64().unwrap(),
        before["total"].as_u64().unwrap() + 1
    );
    let id = first["first_id"].as_u64().unwrap();
    assert_eq!(after["items"][id as usize]["stem"], "extra");
}

#[test]
fn a_failed_import_keeps_the_photos_already_in_the_gallery() {
    let tree = TempTree::new("http-import-failure");
    let photo = fixture(&tree);
    let other = tree.path().join("extra.png");
    RgbImage::from_pixel(16, 12, Rgb([180, 30, 60]))
        .save(&other)
        .unwrap();
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let token = session_token(port);
    request(port, "POST", "/api/import")
        .set("X-AutoShade-Token", &token)
        .send_json(json!({"path": other}))
        .unwrap();
    let before = gallery(port);

    let result = response(
        request(port, "POST", "/api/import")
            .set("X-AutoShade-Token", &token)
            .send_json(json!({"path": tree.path().join("missing.png")})),
    );

    assert_eq!(result.status(), 400);
    assert_eq!(gallery(port), before);
    for id in [0, 1] {
        let mut bytes = Vec::new();
        request(port, "GET", &format!("/api/preview?id={id}&token={token}"))
            .call()
            .unwrap()
            .into_reader()
            .read_to_end(&mut bytes)
            .unwrap();
        assert_eq!(
            image::load_from_memory(&bytes).unwrap().dimensions(),
            (16, 12)
        );
    }
}

#[test]
fn a_raw_save_discloses_merge_loss_and_keeps_the_original_sidecar_untouched() {
    let tree = TempTree::new("http-merge-loss");
    let dir = tree.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let photo = dir.join("photo.dng");
    std::fs::write(
        &photo,
        support::synthetic_dng(16, 16, rawler::decoders::Orientation::Normal, None, true),
    )
    .unwrap();
    let sidecar = photo.with_extension("xmp");
    let original = b"<not-an-xmp-packet>unrecoverable Lightroom properties</not-an-xmp-packet>";
    std::fs::write(&sidecar, original).unwrap();
    let (_server, port) = start_server(&tree, &dir);

    let result = save_request(port, &session_token(port), listing_generation(port))
        .send_json(json!({"id": 0, "recipe": {"exposure_ev": 0.75}}))
        .unwrap();
    let body = result.into_string().unwrap();

    assert!(body.contains("could not be merged"), "{body}");
    assert!(
        body.contains("regenerated without the properties"),
        "{body}"
    );
    assert_eq!(std::fs::read(&sidecar).unwrap(), original);
    assert_eq!(saved(port).0["exposure_ev"], 0.75);
    let projection = std::fs::read_to_string(body.lines().next().unwrap()).unwrap();
    assert!((autoshade::xmp::xmp_to_recipe(&projection).exposure_ev - 0.75).abs() < 0.011);
}
