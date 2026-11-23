use super::*;

const ANALYSIS_SECRET: &str = "synthetic-analysis-secret-7931";
const IMAGE_SECRET: &str = "synthetic-image-secret-4682";
const ANALYSIS_BASE_A: &str = "https://analysis-a.invalid/v1";
const ANALYSIS_BASE_B: &str = "https://analysis-b.invalid/v1";
const IMAGE_BASE_A: &str = "https://image-a.invalid/v1";
const IMAGE_BASE_B: &str = "https://image-b.invalid/v1";

fn settings_server(tag: &str) -> (TempTree, RunningServer, u16) {
    let tree = TempTree::new(tag);
    let photo = fixture(&tree);
    let (server, port) = start_server(&tree, photo.parent().unwrap());
    (tree, server, port)
}

fn settings_get(port: u16) -> (String, Value) {
    let text = request(port, "GET", "/api/settings")
        .call()
        .unwrap()
        .into_string()
        .unwrap();
    let value = serde_json::from_str(&text).unwrap();
    (text, value)
}

fn settings_post_result(port: u16, token: &str, body: Value) -> ureq::Response {
    response(
        request(port, "POST", "/api/settings")
            .set("X-AutoShade-Token", token)
            .send_json(body),
    )
}

fn settings_post(port: u16, token: &str, body: Value) -> Value {
    let response = settings_post_result(port, token, body);
    if response.status() != 200 {
        panic!(
            "settings POST returned {}: {}",
            response.status(),
            response.into_string().unwrap()
        );
    }
    response.into_json().unwrap()
}

fn seed_secrets(port: u16, token: &str) -> PathBuf {
    let saved = settings_post(
        port,
        token,
        json!({
            "analysis_provider": "api",
            "analysis_model": "analysis-initial",
            "analysis_base_url": ANALYSIS_BASE_A,
            "analysis_api_key": ANALYSIS_SECRET,
            "image_model": "image-initial",
            "image_base_url": IMAGE_BASE_A,
            "image_api_key": IMAGE_SECRET,
        }),
    );
    assert_eq!(saved["ok"], true);
    PathBuf::from(saved["saved"].as_str().unwrap())
}

fn settings_file(path: &Path) -> Value {
    serde_json::from_slice(&std::fs::read(path).unwrap()).unwrap()
}

#[test]
fn reading_settings_reports_key_presence_without_returning_the_saved_secrets() {
    let (_tree, _server, port) = settings_server("http-settings-secrets");
    let token = session_token(port);
    seed_secrets(port, &token);

    let (body, value) = settings_get(port);

    assert_eq!(value["analysis"]["key_present"], true);
    assert_eq!(value["image"]["key_present"], true);
    assert!(!body.contains(ANALYSIS_SECRET));
    assert!(!body.contains(IMAGE_SECRET));
    assert!(value["analysis"].get("api_key").is_none());
    assert!(value["image"].get("api_key").is_none());
}

#[test]
fn blank_key_fields_preserve_saved_credentials_while_other_settings_change() {
    let (_tree, _server, port) = settings_server("http-settings-blank-key");
    let token = session_token(port);
    let path = seed_secrets(port, &token);

    settings_post(
        port,
        &token,
        json!({
            "analysis_model": "analysis-updated",
            "analysis_api_key": "",
            "image_model": "image-updated",
            "image_api_key": "   ",
        }),
    );

    let (_, current) = settings_get(port);
    assert_eq!(current["analysis"]["model"], "analysis-updated");
    assert_eq!(current["image"]["model"], "image-updated");
    assert_eq!(current["analysis"]["key_present"], true);
    assert_eq!(current["image"]["key_present"], true);
    let persisted = settings_file(&path);
    assert_eq!(persisted["analysis_api_key"], ANALYSIS_SECRET);
    assert_eq!(persisted["image_api_key"], IMAGE_SECRET);
    assert_eq!(persisted["analysis_api_key_base"], ANALYSIS_BASE_A);
    assert_eq!(persisted["image_api_key_base"], IMAGE_BASE_A);
}

#[test]
fn changing_an_endpoint_withholds_its_saved_key_without_destroying_it() {
    let (_tree, _server, port) = settings_server("http-settings-key-home");
    let token = session_token(port);
    let path = seed_secrets(port, &token);

    settings_post(
        port,
        &token,
        json!({
            "analysis_base_url": ANALYSIS_BASE_B,
            "analysis_api_key": "",
            "image_base_url": IMAGE_BASE_B,
            "image_api_key": "",
        }),
    );
    let (_, changed) = settings_get(port);
    assert_eq!(changed["analysis"]["key_present"], false);
    assert_eq!(changed["image"]["key_present"], false);
    let persisted = settings_file(&path);
    assert_eq!(persisted["analysis_api_key"], ANALYSIS_SECRET);
    assert_eq!(persisted["image_api_key"], IMAGE_SECRET);
    assert_eq!(persisted["analysis_api_key_base"], ANALYSIS_BASE_A);
    assert_eq!(persisted["image_api_key_base"], IMAGE_BASE_A);

    settings_post(
        port,
        &token,
        json!({
            "analysis_base_url": ANALYSIS_BASE_A,
            "analysis_api_key": "",
            "image_base_url": IMAGE_BASE_A,
            "image_api_key": "",
        }),
    );
    let (_, restored) = settings_get(port);
    assert_eq!(restored["analysis"]["key_present"], true);
    assert_eq!(restored["image"]["key_present"], true);
}

#[test]
fn concurrent_partial_setting_updates_preserve_both_results() {
    let (_tree, _server, port) = settings_server("http-settings-concurrent");
    let token = session_token(port);
    let start = Barrier::new(2);

    let statuses = std::thread::scope(|scope| {
        let first = scope.spawn(|| {
            start.wait();
            settings_post_result(
                port,
                &token,
                json!({"analysis_model": "analysis-concurrent"}),
            )
            .status()
        });
        let second = scope.spawn(|| {
            start.wait();
            settings_post_result(port, &token, json!({"image_model": "image-concurrent"})).status()
        });
        [first.join().unwrap(), second.join().unwrap()]
    });

    assert_eq!(statuses, [200, 200]);
    let (_, current) = settings_get(port);
    assert_eq!(current["analysis"]["model"], "analysis-concurrent");
    assert_eq!(current["image"]["model"], "image-concurrent");
    let persisted = settings_file(Path::new(current["settings_file"].as_str().unwrap()));
    assert_eq!(persisted["analysis_model"], "analysis-concurrent");
    assert_eq!(persisted["image_model"], "image-concurrent");
}

#[cfg(unix)]
struct RestoreMode {
    path: PathBuf,
    mode: u32,
}

#[cfg(unix)]
impl RestoreMode {
    fn read_only(path: &Path) -> Self {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(path).unwrap().permissions().mode();
        std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o500)).unwrap();
        Self {
            path: path.to_path_buf(),
            mode,
        }
    }
}

#[cfg(unix)]
impl Drop for RestoreMode {
    fn drop(&mut self) {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&self.path, std::fs::Permissions::from_mode(self.mode)).unwrap();
    }
}

#[cfg(unix)]
#[test]
fn a_failed_settings_publish_leaves_the_previous_configuration_available() {
    let (_tree, _server, port) = settings_server("http-settings-failure");
    let token = session_token(port);
    let path = seed_secrets(port, &token);
    let before_bytes = std::fs::read(&path).unwrap();
    let before = settings_get(port);
    let _restore = RestoreMode::read_only(path.parent().unwrap());

    let result = settings_post_result(port, &token, json!({"analysis_model": "must-not-land"}));

    assert_eq!(result.status(), 500);
    assert!(result.into_string().unwrap().contains("write settings"));
    assert_eq!(std::fs::read(path).unwrap(), before_bytes);
    assert_eq!(settings_get(port), before);
}
