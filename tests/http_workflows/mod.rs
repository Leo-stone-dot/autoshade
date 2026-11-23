use super::*;
use base64::Engine as _;
use image::{DynamicImage, Rgba, RgbaImage};

const BACKGROUND: [u8; 3] = [80, 100, 120];
const DOTS: [(u32, u32); 2] = [(28, 40), (68, 56)];

fn dotted_photo(tree: &TempTree) -> PathBuf {
    let dir = tree.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("first.png");
    let mut image = RgbImage::from_pixel(96, 96, Rgb(BACKGROUND));
    for (cx, cy) in DOTS {
        for y in cy - 1..=cy + 1 {
            for x in cx - 1..=cx + 1 {
                image.put_pixel(x, y, Rgb([250, 250, 250]));
            }
        }
    }
    image.save(&path).unwrap();
    path
}

fn paint_mask(index: usize) -> RgbaImage {
    let mut mask = RgbaImage::from_pixel(96, 96, Rgba([0, 0, 0, 255]));
    let (cx, cy) = DOTS[index];
    for y in cy - 2..=cy + 2 {
        for x in cx - 2..=cx + 2 {
            mask.put_pixel(x, y, Rgba([0, 0, 0, 0]));
        }
    }
    mask
}

fn painted_dot(index: usize) -> String {
    let mut bytes = Cursor::new(Vec::new());
    paint_mask(index)
        .write_to(&mut bytes, image::ImageFormat::Png)
        .unwrap();
    base64::engine::general_purpose::STANDARD.encode(bytes.into_inner())
}

pub(super) fn decode_header(value: &str) -> String {
    let mut decoded = Vec::new();
    let mut bytes = value.bytes();
    while let Some(byte) = bytes.next() {
        if byte == b'%' {
            let hex = [
                bytes.next().expect("complete escape"),
                bytes.next().expect("complete escape"),
            ];
            decoded.push(u8::from_str_radix(std::str::from_utf8(&hex).unwrap(), 16).unwrap());
        } else {
            decoded.push(byte);
        }
    }
    String::from_utf8(decoded).unwrap()
}

pub(super) fn master_path(response: &ureq::Response) -> PathBuf {
    assert_eq!(response.status(), 200);
    PathBuf::from(decode_header(
        response
            .header("X-Output-Path")
            .expect("a delivered pixel master"),
    ))
}

fn photo_id(port: u16, stem: &str) -> u64 {
    gallery(port)["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["stem"] == stem)
        .expect("fixture is in the gallery")["id"]
        .as_u64()
        .unwrap()
}

fn photo_post(port: u16, path: &str, body: Value) -> ureq::Response {
    response(
        request(port, "POST", path)
            .set("X-AutoShade-Token", &session_token(port))
            .set("X-AutoShade-Gen", &listing_generation(port).to_string())
            .send_json(body),
    )
}

fn heal_dot(port: u16, index: usize, master: Option<&Path>) -> ureq::Response {
    let result = photo_post(
        port,
        "/api/heal",
        json!({
            "id": photo_id(port, "first"), "auto": false, "mask": painted_dot(index), "master": master,
        }),
    );
    if result.status() != 200 {
        panic!(
            "heal returned {}: {}",
            result.status(),
            result.into_string().unwrap()
        );
    }
    result
}

fn assert_repaired(image: &RgbImage, index: usize) {
    let (x, y) = DOTS[index];
    for (actual, expected) in image.get_pixel(x, y).0.into_iter().zip(BACKGROUND) {
        assert!(
            (i16::from(actual) - i16::from(expected)).abs() <= 8,
            "painted defect {index} was not replaced from its surroundings: {actual} vs {expected}"
        );
    }
}

fn response_image(response: ureq::Response) -> DynamicImage {
    assert_eq!(response.status(), 200);
    let mut bytes = Vec::new();
    response.into_reader().read_to_end(&mut bytes).unwrap();
    image::load_from_memory(&bytes).unwrap()
}

#[test]
fn a_painted_heal_repairs_only_the_selected_defect_and_leaves_the_source_untouched() {
    let tree = TempTree::new("http-heal-selection");
    let photo = dotted_photo(&tree);
    let original = std::fs::read(&photo).unwrap();
    let before = autoshade::decode::load_image(&photo).unwrap().to_rgb8();
    assert_eq!(before.get_pixel(DOTS[0].0, DOTS[0].1).0, [250, 250, 250]);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());

    let result = heal_dot(port, 0, None);
    assert_eq!(result.header("X-Heal-Spots"), Some("1"));
    let image = autoshade::decode::load_image(&master_path(&result))
        .unwrap()
        .to_rgb8();

    assert_eq!(image.dimensions(), (96, 96));
    assert_repaired(&image, 0);
    assert_eq!(image.get_pixel(DOTS[1].0, DOTS[1].1).0, [250, 250, 250]);
    let mask = paint_mask(0);
    for (x, y, previous) in before.enumerate_pixels() {
        if mask.get_pixel(x, y).0[3] == 255 {
            for (actual, expected) in image.get_pixel(x, y).0.into_iter().zip(previous.0) {
                assert!(
                    (i16::from(actual) - i16::from(expected)).abs() <= 1,
                    "unpainted pixel ({x}, {y}) changed from {expected} to {actual}"
                );
            }
        }
    }
    assert_eq!(std::fs::read(photo).unwrap(), original);
}

#[test]
fn a_second_heal_keeps_the_repair_in_the_supplied_session_master() {
    let tree = TempTree::new("http-heal-chain");
    let photo = dotted_photo(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let first = master_path(&heal_dot(port, 0, None));
    assert_repaired(&image::open(&first).unwrap().to_rgb8(), 0);

    let result = heal_dot(port, 1, Some(&first));
    let image = image::open(master_path(&result)).unwrap().to_rgb8();

    assert_repaired(&image, 0);
    assert_repaired(&image, 1);
}

#[test]
fn a_saved_heal_with_neutral_sliders_survives_a_server_restart() {
    let tree = TempTree::new("http-heal-reopen");
    let photo = dotted_photo(&tree);
    let (server, port) = start_server(&tree, photo.parent().unwrap());
    let master = master_path(&heal_dot(port, 0, None));
    let result = photo_post(
        port,
        "/api/xmp",
        json!({
            "id": photo_id(port, "first"), "recipe": {}, "master": master,
        }),
    );
    assert_eq!(result.status(), 200);
    drop(server);

    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let result = photo_post(
        port,
        "/api/develop",
        json!({"id": photo_id(port, "first"), "recipe": {}}),
    );
    let preview = response_image(result).to_rgb8();

    assert_eq!(preview.dimensions(), (96, 96));
    assert_repaired(&preview, 0);
}

#[test]
fn a_master_issued_for_one_photo_cannot_be_used_to_heal_another() {
    let tree = TempTree::new("http-master-owner");
    let photo = dotted_photo(&tree);
    let other = photo.parent().unwrap().join("second.png");
    RgbImage::from_pixel(96, 96, Rgb([40, 60, 80]))
        .save(&other)
        .unwrap();
    let original = std::fs::read(&other).unwrap();
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let master = master_path(&heal_dot(port, 0, None));

    let result = photo_post(
        port,
        "/api/heal",
        json!({
            "id": photo_id(port, "second"), "auto": false, "mask": painted_dot(0), "master": master,
        }),
    );

    assert_eq!(result.status(), 400);
    assert!(
        result
            .into_string()
            .unwrap()
            .contains("not issued for this photo")
    );
    assert_eq!(std::fs::read(other).unwrap(), original);
}

#[test]
fn a_missing_detector_runtime_still_heals_the_painted_area_and_discloses_the_failure() {
    let tree = TempTree::new("http-heal-detector-failure");
    let photo = dotted_photo(&tree);
    let script = tree.path().join("complete.mjs");
    // Only the external model runtime is unavailable; healing uses the real engine.
    std::fs::write(
        &script,
        "throw new Error('synthetic detector unavailable');",
    )
    .unwrap();
    let absent_runtime = tree.path().join("absent-node-runtime");
    let (_server, port) = start_server_with_env(
        &tree,
        photo.parent().unwrap(),
        &[
            ("AUTOSHADE_PI_HARNESS", script.as_os_str()),
            ("AUTOSHADE_NODE", absent_runtime.as_os_str()),
        ],
    );

    let result = photo_post(
        port,
        "/api/heal",
        json!({
            "id": photo_id(port, "first"), "auto": true, "mask": painted_dot(0),
        }),
    );
    assert_eq!(result.status(), 200);
    let warning = decode_header(
        result
            .header("X-Heal-Rationale")
            .expect("partial outcome is disclosed"),
    );

    assert_eq!(result.header("X-Heal-Spots"), Some("1"));
    assert!(
        warning.contains("failed") && warning.contains("painted"),
        "{warning}"
    );
    assert_repaired(&image::open(master_path(&result)).unwrap().to_rgb8(), 0);
}

#[test]
fn a_noise_stack_delivers_both_frames_at_full_size_without_overwriting_them() {
    let tree = TempTree::new("http-stack-delivery");
    let dir = tree.path().join("photos");
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.png");
    let second = dir.join("second.png");
    RgbImage::from_pixel(32, 24, Rgb([70, 90, 110]))
        .save(&first)
        .unwrap();
    RgbImage::from_pixel(32, 24, Rgb([110, 130, 150]))
        .save(&second)
        .unwrap();
    let before = [
        std::fs::read(&first).unwrap(),
        std::fs::read(&second).unwrap(),
    ];
    let (_server, port) = start_server(&tree, &dir);

    let result = photo_post(
        port,
        "/api/stack",
        json!({
            "ids": [photo_id(port, "first"), photo_id(port, "second")], "kind": "noise", "tripod": true,
        }),
    );
    let image = image::open(master_path(&result)).unwrap();

    assert_eq!(result.header("X-Stack-Kind"), Some("noise"));
    assert_eq!(result.header("X-Stack-Frames"), Some("2"));
    assert_eq!(image.dimensions(), (32, 24));
    assert_eq!(image.color().bits_per_pixel(), 48);
    for ((value, low), high) in image
        .to_rgb8()
        .get_pixel(16, 12)
        .0
        .into_iter()
        .zip([70, 90, 110])
        .zip([110, 130, 150])
    {
        assert!(
            low < value && value < high,
            "{value} did not blend {low} with {high}"
        );
    }
    assert_eq!(
        [
            std::fs::read(first).unwrap(),
            std::fs::read(second).unwrap()
        ],
        before
    );
}

#[test]
fn stacking_after_a_heal_uses_the_repaired_reference_frame() {
    let tree = TempTree::new("http-stack-after-heal");
    let photo = dotted_photo(&tree);
    RgbImage::from_pixel(96, 96, Rgb(BACKGROUND))
        .save(photo.parent().unwrap().join("second.png"))
        .unwrap();
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let master = master_path(&heal_dot(port, 0, None));

    let result = photo_post(
        port,
        "/api/stack",
        json!({
            "ids": [photo_id(port, "first"), photo_id(port, "second")],
            "kind": "noise", "tripod": true, "master": master,
        }),
    );

    assert_repaired(&image::open(master_path(&result)).unwrap().to_rgb8(), 0);
}

#[test]
fn stacking_different_frame_sizes_is_refused_without_changing_the_inputs() {
    let tree = TempTree::new("http-stack-frame-error");
    let photo = fixture(&tree);
    let other = photo.parent().unwrap().join("second.png");
    RgbImage::from_pixel(8, 8, Rgb([100, 120, 140]))
        .save(&other)
        .unwrap();
    let before = [
        std::fs::read(&photo).unwrap(),
        std::fs::read(&other).unwrap(),
    ];
    let (_server, port) = start_server(&tree, photo.parent().unwrap());

    let result = photo_post(
        port,
        "/api/stack",
        json!({
            "ids": [photo_id(port, "photo"), photo_id(port, "second")], "kind": "noise", "tripod": true,
        }),
    );

    assert_eq!(result.status(), 500);
    assert!(result.into_string().unwrap().contains("one framing"));
    assert_eq!(
        [std::fs::read(photo).unwrap(), std::fs::read(other).unwrap()],
        before
    );
}

#[test]
#[ignore = "999 failed real HTTP operations: cargo test --release --test serve_behavior http_workflows::failed_heals_do_not_exhaust_the_capacity_for_a_later_valid_heal -- --ignored --exact"]
fn failed_heals_do_not_exhaust_the_capacity_for_a_later_valid_heal() {
    let tree = TempTree::new("http-heal-failure-capacity");
    let photo = dotted_photo(&tree);
    let (_server, port) = start_server(&tree, photo.parent().unwrap());
    let id = photo_id(port, "first");
    for _ in 0..999 {
        let result = photo_post(port, "/api/heal", json!({"id": id, "auto": false}));
        assert_eq!(result.status(), 500);
        assert!(result.into_string().unwrap().contains("nothing to heal"));
    }

    let result = heal_dot(port, 0, None);

    assert_eq!(result.header("X-Heal-Spots"), Some("1"));
    assert_repaired(&image::open(master_path(&result)).unwrap().to_rgb8(), 0);
}
