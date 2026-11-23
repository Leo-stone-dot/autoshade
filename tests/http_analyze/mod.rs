use super::*;

#[test]
#[ignore = "fixture worker invoked by HTTP Analyze tests; not a standalone product case"]
fn print_develop_directory_fixture() {
    let photo = std::env::var_os("AUTOSHADE_FIXTURE_PHOTO").expect("fixture photo path");
    println!(
        "DEVELOP={}",
        autoshade::store::develop_dir(Path::new(&photo)).display()
    );
}

fn develop_dir_for(tree: &TempTree, photo: &Path) -> PathBuf {
    let mut command = autoshade::sidecar_command(std::env::current_exe().unwrap());
    command
        .args([
            "--ignored",
            "--exact",
            "http_analyze::print_develop_directory_fixture",
            "--nocapture",
        ])
        .current_dir(tree.path())
        .env_clear()
        .env("HOME", tree.path())
        .env("AUTOSHADE_DATA_DIR", tree.path().join("data"))
        .env("AUTOSHADE_FIXTURE_PHOTO", photo)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let output = autoshade::spawn_child(&mut command)
        .unwrap()
        .wait_with_output()
        .unwrap();
    assert!(
        output.status.success(),
        "develop-dir fixture failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .find_map(|line| line.strip_prefix("DEVELOP=").map(PathBuf::from))
        .expect("the fixture prints the public develop directory")
}

struct HarnessFixture {
    node: PathBuf,
    harness: PathBuf,
    cli: PathBuf,
    recipe: PathBuf,
    fail: PathBuf,
    fail_after_commit: PathBuf,
    develop: PathBuf,
}

impl HarnessFixture {
    fn new(tree: &TempTree, photo: &Path, exposure: f32) -> Self {
        #[cfg(unix)]
        use std::os::unix::fs::PermissionsExt;
        let node = tree.path().join("fake-node");
        std::fs::write(&node, include_str!("../fixtures/analyze-harness.sh")).unwrap();
        #[cfg(unix)]
        std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
        let harness = tree.path().join("analyze.mjs");
        std::fs::write(&harness, b"synthetic HTTP harness").unwrap();
        let cli = tree.path().join("autoshade-cli");
        std::fs::write(&cli, b"synthetic CLI placeholder").unwrap();
        let recipe = tree.path().join("harness-recipe.json");
        std::fs::write(
            &recipe,
            serde_json::to_vec_pretty(&autoshade::recipe::EditRecipe {
                exposure_ev: exposure,
                contrast: 18.0,
                rationale: "synthetic HTTP harness result".to_string(),
                ..Default::default()
            })
            .unwrap(),
        )
        .unwrap();
        Self {
            node,
            harness,
            cli,
            recipe,
            fail: tree.path().join("fail-harness"),
            fail_after_commit: tree.path().join("fail-harness-after-commit"),
            develop: develop_dir_for(tree, photo),
        }
    }

    fn environment(&self) -> [(&str, &std::ffi::OsStr); 9] {
        [
            ("AUTOSHADE_PIPELINE", std::ffi::OsStr::new("harness")),
            ("AUTOSHADE_NODE", self.node.as_os_str()),
            ("AUTOSHADE_PI_HARNESS", self.harness.as_os_str()),
            ("AUTOSHADE_MCP_EXE", self.cli.as_os_str()),
            ("AUTOSHADE_TEST_RECIPE", self.recipe.as_os_str()),
            ("AUTOSHADE_TEST_DEVELOP", self.develop.as_os_str()),
            ("AUTOSHADE_TEST_HARNESS_FAIL", self.fail.as_os_str()),
            (
                "AUTOSHADE_TEST_HARNESS_FAIL_AFTER_COMMIT",
                self.fail_after_commit.as_os_str(),
            ),
            ("DEEPSEEK_API_KEY", std::ffi::OsStr::new("synthetic-key")),
        ]
    }

    fn fail(&self) {
        std::fs::write(&self.fail, b"").unwrap();
    }

    fn fail_after_commit(&self) {
        std::fs::write(&self.fail_after_commit, b"").unwrap();
    }
}

fn analyze(port: u16) -> ureq::Response {
    response(
        request(port, "POST", "/api/analyze")
            .set("X-AutoShade-Token", &session_token(port))
            .set("X-AutoShade-Gen", &listing_generation(port).to_string())
            .send_json(json!({"id": 0})),
    )
}

#[test]
fn a_successful_http_harness_analysis_returns_and_reopens_its_committed_recipe() {
    let tree = TempTree::new("http-analyze-success");
    let photo = fixture(&tree);
    let source = std::fs::read(&photo).unwrap();
    let harness = HarnessFixture::new(&tree, &photo, 0.75);
    let (server, port) =
        start_server_with_env(&tree, photo.parent().unwrap(), &harness.environment());

    let result = analyze(port);
    assert_eq!(result.status(), 200);
    let body: Value = result.into_json().unwrap();
    assert_eq!(body["recipe"]["exposure_ev"], 0.75);
    assert_eq!(body["recipe"]["contrast"], 18.0);
    assert_eq!(body["verdict"]["decision"], "accept");
    assert_eq!(body["saved"], true);
    let revision = body["revision"]
        .as_str()
        .expect("a committed analysis returns its revision")
        .to_string();
    assert_eq!(saved(port).0["exposure_ev"], 0.75);
    assert_eq!(saved(port).1, format!("\"{revision}\""));
    assert_eq!(std::fs::read(&photo).unwrap(), source);
    drop(server);

    let (_server, port) =
        start_server_with_env(&tree, photo.parent().unwrap(), &harness.environment());
    assert_eq!(saved(port).0["exposure_ev"], 0.75);
}

#[test]
fn a_harness_failure_before_commit_leaves_the_previous_save_available() {
    let tree = TempTree::new("http-analyze-failure");
    let photo = fixture(&tree);
    let source = std::fs::read(&photo).unwrap();
    let harness = HarnessFixture::new(&tree, &photo, 1.5);
    let (_server, port) =
        start_server_with_env(&tree, photo.parent().unwrap(), &harness.environment());
    let old_revision = save(port, &session_token(port), listing_generation(port), 0.5);
    let before = saved(port);
    harness.fail();

    let result = analyze(port);

    assert_eq!(result.status(), 500);
    assert!(result.into_string().unwrap().contains("harness failed"));
    assert_eq!(saved(port), before);
    assert_eq!(saved(port).1, old_revision);
    assert_eq!(std::fs::read(photo).unwrap(), source);
}

#[test]
#[ignore = "known harness partial-commit defect; run with cargo test --release --test serve_behavior http_analyze::a_harness_error_cannot_report_failure_after_replacing_the_previous_save -- --ignored --exact"]
fn a_harness_error_cannot_report_failure_after_replacing_the_previous_save() {
    let tree = TempTree::new("http-analyze-partial-commit");
    let photo = fixture(&tree);
    let source = std::fs::read(&photo).unwrap();
    let harness = HarnessFixture::new(&tree, &photo, 1.5);
    let (_server, port) =
        start_server_with_env(&tree, photo.parent().unwrap(), &harness.environment());
    save(port, &session_token(port), listing_generation(port), 0.5);
    let before = saved(port);
    harness.fail_after_commit();

    let result = analyze(port);
    let status = result.status();
    let body = result.into_string().unwrap();
    let after = saved(port);

    if status == 200 {
        assert_eq!(after.0["exposure_ev"], 1.5);
        let body: Value = serde_json::from_str(&body).unwrap();
        assert_eq!(
            body["saved"], true,
            "a committed result must be reported as saved"
        );
    } else {
        assert_eq!(status, 500, "unexpected Analyze status: {status}: {body}");
        assert_eq!(
            after, before,
            "Analyze reported failure after replacing the previous save"
        );
    }
    assert_eq!(std::fs::read(photo).unwrap(), source);
}
