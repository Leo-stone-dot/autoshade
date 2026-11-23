use super::http_workflows::master_path;
use super::*;
use base64::Engine as _;
use image::{DynamicImage, Rgba, RgbaImage};
use std::ffi::{CString, OsStr};
use std::fs::{File, OpenOptions};
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::time::Instant;

const BG: [u8; 3] = [48, 64, 80];
const RED: [u8; 3] = [210, 75, 35];
const BLUE: [u8; 3] = [35, 100, 220];
const CENTERS: [(u32, u32); 2] = [(26, 34), (70, 62)];

struct Backend {
    root: PathBuf,
    cli: PathBuf,
    workflow: PathBuf,
}

impl Backend {
    fn new(tree: &TempTree) -> Self {
        let root = tree.path().join("model");
        std::fs::create_dir_all(&root).unwrap();
        let cli = root.join("comfy");
        std::fs::write(&cli, include_str!("../fixtures/comfy-cli.sh")).unwrap();
        std::fs::set_permissions(&cli, std::fs::Permissions::from_mode(0o755)).unwrap();
        let workflow = root.join("configured-workflow.json");
        std::fs::write(&workflow, serde_json::to_vec(&json!({
            "1": {"class_type": "LoadImage", "inputs": {"image": "{{IMAGE}}"}},
            "2": {"class_type": "LoadImageMask", "inputs": {"image": "{{MASK}}", "channel": "red"}},
            "3": {"class_type": "SaveImage", "inputs": {"images": ["1", 0], "filename_prefix": "{{PROMPT}}"}},
        })).unwrap()).unwrap();
        Self {
            root,
            cli,
            workflow,
        }
    }

    fn answer(&self, colors: &[(usize, [u8; 3])]) {
        let mut img = RgbImage::from_pixel(96, 96, Rgb(BG));
        for &(region, color) in colors {
            let (cx, cy) = CENTERS[region];
            for y in cy - 10..cy + 10 {
                for x in cx - 10..cx + 10 {
                    img.put_pixel(x, y, Rgb(color));
                }
            }
        }
        img.save(self.root.join("answer.png")).unwrap();
    }

    fn start(&self, tree: &TempTree, directory: &Path) -> (RunningServer, u16) {
        start_server_with_env(
            tree,
            directory,
            &[
                ("AUTOSHADE_COMFY_CLI", self.cli.as_os_str()),
                ("AUTOSHADE_COMFY_WORKSPACE", self.root.as_os_str()),
                ("AUTOSHADE_COMFY_FILL_WORKFLOW", self.workflow.as_os_str()),
                ("AUTOSHADE_TEST_COMFY_ROOT", self.root.as_os_str()),
                ("AUTOSHADE_GENERATIVE", OsStr::new("comfy")),
            ],
        )
    }

    fn hold(&self) -> ModelQueue {
        let ready = self.root.join("ready");
        let name = CString::new(ready.as_os_str().as_bytes()).unwrap();
        // SAFETY: this creates a FIFO at a fixture-owned path, with owner-only permissions.
        assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        let reader = OpenOptions::new()
            .read(true)
            .write(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(ready)
            .unwrap();
        std::fs::write(self.root.join("hold"), b"").unwrap();
        ModelQueue {
            root: self.root.clone(),
            reader,
            pending: Vec::new(),
        }
    }
}

struct ModelQueue {
    root: PathBuf,
    reader: File,
    pending: Vec<u8>,
}

impl ModelQueue {
    fn next(&mut self) -> Release {
        let deadline = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(end) = self.pending.iter().position(|&b| b == b'\n') {
                let ticket = String::from_utf8(self.pending.drain(..=end).collect()).unwrap();
                let path = self.root.join(ticket.trim()).join("release");
                let writer = OpenOptions::new()
                    .read(true)
                    .write(true)
                    .custom_flags(libc::O_NONBLOCK)
                    .open(path)
                    .unwrap();
                return Release(Some(writer));
            }
            let remaining = deadline
                .checked_duration_since(Instant::now())
                .expect("the external model did not receive the request before the deadline");
            let mut poll = libc::pollfd {
                fd: self.reader.as_raw_fd(),
                events: libc::POLLIN,
                revents: 0,
            };
            // SAFETY: poll receives one live fixture FD and a finite wait.
            let ready =
                unsafe { libc::poll(&mut poll, 1, remaining.as_millis().min(15000) as i32) };
            if ready < 0
                && std::io::Error::last_os_error().kind() == std::io::ErrorKind::Interrupted
            {
                continue;
            }
            assert!(
                ready > 0,
                "the external model did not receive the request before the deadline"
            );
            let mut bytes = [0u8; 1024];
            match self.reader.read(&mut bytes) {
                Ok(n) if n > 0 => self.pending.extend_from_slice(&bytes[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => {}
                other => panic!("cannot read model readiness: {other:?}"),
            }
        }
    }
}

struct Release(Option<File>);

impl Release {
    fn send(&mut self) {
        if let Some(mut writer) = self.0.take() {
            writer.write_all(b"continue\n").unwrap();
        }
    }
}

impl Drop for Release {
    fn drop(&mut self) {
        if let Some(mut writer) = self.0.take() {
            let _ = writer.write_all(b"continue\n");
        }
    }
}

fn source(tree: &TempTree) -> PathBuf {
    let dir = tree.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("photo.png");
    RgbImage::from_pixel(96, 96, Rgb(BG)).save(&path).unwrap();
    path
}

fn mask(region: usize) -> String {
    let mut image = RgbaImage::from_pixel(96, 96, Rgba([0, 0, 0, 255]));
    let (cx, cy) = CENTERS[region];
    for y in cy - 10..cy + 10 {
        for x in cx - 10..cx + 10 {
            image.put_pixel(x, y, Rgba([0, 0, 0, 0]));
        }
    }
    let mut bytes = Cursor::new(Vec::new());
    image.write_to(&mut bytes, image::ImageFormat::Png).unwrap();
    base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
}

fn fill(port: u16, region: usize, master: Option<&Path>) -> ureq::Response {
    response(
        request(port, "POST", "/api/retouch")
            .timeout(Duration::from_secs(60))
            .set("X-AutoShade-Token", &session_token(port))
            .set("X-AutoShade-Gen", &listing_generation(port).to_string())
            .send_json(json!({
                "id": 0, "mask": mask(region), "prompt": "  synthetic fill  ", "master": master,
            })),
    )
}

fn successful_fill(response: ureq::Response, backend: &Backend) -> ureq::Response {
    if response.status() != 200 {
        let diagnostics: Vec<_> = std::fs::read_dir(&backend.root)
            .unwrap()
            .map(Result::unwrap)
            .filter(|entry| entry.file_name().to_string_lossy().ends_with(".error"))
            .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
            .filter(|text| !text.is_empty())
            .collect();
        panic!(
            "fill returned {}: {}; provider diagnostics: {diagnostics:?}",
            response.status(),
            response.into_string().unwrap()
        );
    }
    response
}

fn image(response: ureq::Response) -> DynamicImage {
    assert_eq!(response.status(), 200);
    let mut bytes = Vec::new();
    response.into_reader().read_to_end(&mut bytes).unwrap();
    image::load_from_memory(&bytes).unwrap()
}

fn has_color(image: &RgbImage, region: usize, channel: usize) {
    let (cx, cy) = CENTERS[region];
    let px = image
        .get_pixel(cx * image.width() / 96, cy * image.height() / 96)
        .0;
    assert!(
        px[channel] > 140 && i16::from(px[channel]) > i16::from(px[(channel + 1) % 3]) + 35,
        "selected region {region} did not receive the expected colour: {px:?}"
    );
}

fn action(port: u16, path: &str, body: Value) -> ureq::Response {
    response(
        request(port, "POST", path)
            .set("X-AutoShade-Token", &session_token(port))
            .set("X-AutoShade-Gen", &listing_generation(port).to_string())
            .send_json(body),
    )
}

#[test]
fn a_masked_fill_keeps_distant_pixels_and_delivers_the_changed_area_in_its_preview() {
    let tree = TempTree::new("fill-selection");
    let photo = source(&tree);
    let bytes = std::fs::read(&photo).unwrap();
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED), (1, BLUE)]);
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());

    let result = fill(port, 0, None);
    let master = autoshade::decode::load_image(&master_path(&result))
        .unwrap()
        .to_rgb8();
    let preview = image(result).to_rgb8();

    assert_eq!(master.dimensions(), (96, 96));
    has_color(&master, 0, 0);
    has_color(&preview, 0, 0);
    let (cx, cy) = CENTERS[0];
    // A distant region must remain unchanged; the seam itself may be feathered.
    for (x, y, pixel) in master.enumerate_pixels() {
        if x.abs_diff(cx) > 18 || y.abs_diff(cy) > 18 {
            assert_eq!(pixel.0, BG, "unselected distant pixel ({x}, {y}) changed");
        }
    }
    assert_eq!(std::fs::read(photo).unwrap(), bytes);
}

#[test]
fn a_fill_sends_the_current_image_selection_and_user_instruction_to_the_external_model() {
    let tree = TempTree::new("fill-provider-input");
    let photo = source(&tree);
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED)]);
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());
    assert_eq!(fill(port, 0, None).status(), 200);

    let captured: Vec<_> = std::fs::read_dir(&backend.root)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("workflow."))
        .collect();
    assert!(!captured.is_empty(), "the provider must receive a workflow");
    for entry in captured {
        let workflow: Value =
            serde_json::from_slice(&std::fs::read(entry.path()).unwrap()).unwrap();
        let uploaded = image::load_from_memory(
            &std::fs::read(
                backend
                    .root
                    .join(workflow["1"]["inputs"]["image"].as_str().unwrap()),
            )
            .unwrap(),
        )
        .unwrap()
        .to_rgb8();
        let selected = image::load_from_memory(
            &std::fs::read(
                backend
                    .root
                    .join(workflow["2"]["inputs"]["image"].as_str().unwrap()),
            )
            .unwrap(),
        )
        .unwrap()
        .to_luma8();
        assert_eq!(uploaded.dimensions(), selected.dimensions());
        assert_eq!(uploaded.get_pixel(0, 0).0, BG);
        let (cx, cy) = CENTERS[0];
        assert_eq!(
            selected
                .get_pixel(cx * selected.width() / 96, cy * selected.height() / 96)
                .0[0],
            255
        );
        assert_eq!(selected.get_pixel(0, 0).0[0], 0);
        assert_eq!(workflow["3"]["inputs"]["filename_prefix"], "synthetic fill");
    }
}

#[test]
fn a_second_fill_preserves_the_pixels_from_the_first_fill() {
    let tree = TempTree::new("fill-chain");
    let photo = source(&tree);
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED)]);
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());
    let first = master_path(&fill(port, 0, None));
    has_color(&image::open(&first).unwrap().to_rgb8(), 0, 0);
    backend.answer(&[(1, BLUE)]);

    let next = fill(port, 1, Some(&first));
    let master = image::open(master_path(&next)).unwrap().to_rgb8();

    has_color(&master, 0, 0);
    has_color(&master, 1, 2);
}

#[test]
fn a_saved_fill_reopens_with_its_generated_patch() {
    let tree = TempTree::new("fill-reopen");
    let photo = source(&tree);
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED)]);
    let (server, port) = backend.start(&tree, photo.parent().unwrap());
    let master = master_path(&fill(port, 0, None));
    assert_eq!(
        action(
            port,
            "/api/xmp",
            json!({"id":0, "recipe":{}, "master":master})
        )
        .status(),
        200
    );
    drop(server);
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());

    let preview = image(action(port, "/api/develop", json!({"id":0, "recipe":{}}))).to_rgb8();

    has_color(&preview, 0, 0);
}

#[test]
fn a_corrupt_model_reply_preserves_the_previous_master_and_allows_a_retry() {
    let tree = TempTree::new("fill-corrupt-reply");
    let photo = source(&tree);
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED)]);
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());
    let first = master_path(&fill(port, 0, None));
    let original = std::fs::read(&first).unwrap();
    std::fs::write(backend.root.join("answer.png"), b"incomplete model image").unwrap();

    let result = fill(port, 1, Some(&first));

    assert_eq!(result.status(), 500);
    assert!(result.header("X-Output-Path").is_none());
    assert!(result.into_string().unwrap().contains("retouch failed"));
    assert_eq!(std::fs::read(&first).unwrap(), original);
    backend.answer(&[(1, BLUE)]);
    let retry = image::open(master_path(&fill(port, 1, Some(&first))))
        .unwrap()
        .to_rgb8();
    has_color(&retry, 0, 0);
    has_color(&retry, 1, 2);
}

#[test]
fn a_failed_model_run_is_not_published_even_when_download_left_a_complete_image() {
    let tree = TempTree::new("fill-partial-provider");
    let photo = source(&tree);
    let before = std::fs::read(&photo).unwrap();
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED)]);
    std::fs::write(backend.root.join("fail-run"), b"").unwrap();
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());

    let result = fill(port, 0, None);

    assert_eq!(result.status(), 500);
    assert!(result.header("X-Output-Path").is_none());
    assert!(
        result
            .into_string()
            .unwrap()
            .contains("synthetic generation stopped")
    );
    assert_eq!(std::fs::read(photo).unwrap(), before);
}

#[test]
fn an_export_finishes_while_a_fill_is_waiting_for_the_external_model() {
    let tree = TempTree::new("fill-export-progress");
    let photo = source(&tree);
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED)]);
    let mut queue = backend.hold();
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());

    std::thread::scope(|scope| {
        let pending_fill = scope.spawn(|| fill(port, 0, None));
        let mut release = queue.next();
        let exported = request(port, "POST", "/api/export")
            .timeout(Duration::from_secs(5))
            .set("X-AutoShade-Token", &session_token(port))
            .set("X-AutoShade-Gen", &listing_generation(port).to_string())
            .send_json(json!({"id":0, "recipe":{}, "format":"jpg"}));
        // Always release the external fixture before examining the export verdict.
        release.send();
        let filled = pending_fill.join().unwrap();
        let exported = exported.expect("export must finish before the external model is released");
        let delivered = PathBuf::from(exported.into_string().unwrap());
        assert_eq!(image::open(delivered).unwrap().dimensions(), (96, 96));
        has_color(&image::open(master_path(&filled)).unwrap().to_rgb8(), 0, 0);
    });
}

#[test]
#[ignore = "known Comfy temporary-path collision; run with cargo test --release --test serve_behavior http_fill::simultaneous_fills_keep_both_deliveries_instead_of_overwriting_one_another -- --ignored --exact"]
fn simultaneous_fills_keep_both_deliveries_instead_of_overwriting_one_another() {
    let tree = TempTree::new("fill-concurrent");
    let photo = source(&tree);
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED), (1, BLUE)]);
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());
    let start = Barrier::new(2);

    let masters = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            start.wait();
            fill(port, 0, None)
        });
        let second = scope.spawn(|| {
            start.wait();
            fill(port, 1, None)
        });
        [
            master_path(&successful_fill(first.join().unwrap(), &backend)),
            master_path(&successful_fill(second.join().unwrap(), &backend)),
        ]
    });

    let a = image::open(&masters[0]).unwrap().to_rgb8();
    let b = image::open(&masters[1]).unwrap().to_rgb8();
    has_color(&a, 0, 0);
    assert_eq!(a.get_pixel(CENTERS[1].0, CENTERS[1].1).0, BG);
    has_color(&b, 1, 2);
    assert_eq!(b.get_pixel(CENTERS[0].0, CENTERS[0].1).0, BG);
}

#[test]
#[ignore = "fixture worker invoked by generated_origin_survives_fill_save_and_restart; not a standalone product case"]
fn record_generated_pixel_source_fixture() {
    let photo = std::env::var_os("AUTOSHADE_FIXTURE_PHOTO").expect("fixture photo path");
    let master =
        std::env::var_os("AUTOSHADE_FIXTURE_MASTER").expect("fixture generated master path");
    autoshade::store::write_pixel_source(Path::new(&photo), Path::new(&master), true).unwrap();
}

#[test]
fn generated_origin_survives_fill_save_and_restart_without_reapplying_the_base_curve() {
    let tree = TempTree::new("fill-generated-origin");
    let photo = source(&tree);
    let generated = tree.path().join("generated.png");
    RgbImage::from_pixel(96, 96, Rgb([95, 100, 105]))
        .save(&generated)
        .unwrap();
    let mut seed = autoshade::sidecar_command(std::env::current_exe().unwrap());
    seed.args([
        "--ignored",
        "--exact",
        "http_fill::record_generated_pixel_source_fixture",
    ])
    .current_dir(tree.path())
    .env_clear()
    .env("HOME", tree.path())
    .env("AUTOSHADE_DATA_DIR", tree.path().join("data"))
    .env("AUTOSHADE_FIXTURE_PHOTO", &photo)
    .env("AUTOSHADE_FIXTURE_MASTER", &generated)
    .stdout(Stdio::null())
    .stderr(Stdio::inherit());
    let mut child = autoshade::spawn_child(&mut seed).unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(
                status.success(),
                "could not create the generated-pixel premise"
            );
            break;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("generated-pixel fixture did not finish");
        }
        std::thread::yield_now();
    }
    let backend = Backend::new(&tree);
    backend.answer(&[(0, RED)]);
    let (server, port) = backend.start(&tree, photo.parent().unwrap());
    let master = master_path(&fill(port, 0, None));
    let curve = serde_json::to_value(autoshade::recipe::EditRecipe {
        base_curve: vec![[0.0, 0.0], [0.5, 0.15], [1.0, 1.0]],
        ..Default::default()
    })
    .unwrap();
    let imported: Value = action(port, "/api/import", json!({"path": generated}))
        .into_json()
        .unwrap();
    let control_id = imported["first_id"].as_u64().unwrap();
    let control = image(action(
        port,
        "/api/develop",
        json!({"id":control_id,"recipe":{}}),
    ))
    .to_rgb8();
    let control_with_curve = image(action(
        port,
        "/api/develop",
        json!({"id":control_id,"recipe":curve}),
    ))
    .to_rgb8();
    assert!(
        control_with_curve != control,
        "the calibration premise must change an ordinary photo"
    );
    let neutral = image(action(
        port,
        "/api/develop",
        json!({"id":0,"recipe":{},"master":master}),
    ))
    .to_rgb8();
    let with_curve = image(action(
        port,
        "/api/develop",
        json!({"id":0,"recipe":curve,"master":master}),
    ))
    .to_rgb8();
    assert!(
        with_curve == neutral,
        "a generated master must not be calibrated twice"
    );
    assert_eq!(
        action(
            port,
            "/api/xmp",
            json!({"id":0,"recipe":curve,"master":master})
        )
        .status(),
        200
    );
    drop(server);
    let (_server, port) = backend.start(&tree, photo.parent().unwrap());

    let reopened = image(action(port, "/api/develop", json!({"id":0,"recipe":curve}))).to_rgb8();

    assert!(
        reopened == neutral,
        "saving must preserve the generated origin"
    );
}
