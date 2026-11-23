mod support;

use std::io::Write;
use std::process::Stdio;

use image::{Rgb, RgbImage};
use serde_json::{Value, json};
use support::TempTree;

fn run_mcp(root: &std::path::Path, data: &std::path::Path, requests: &[Value]) -> Vec<Value> {
    let mut command = autoshade::sidecar_command(env!("CARGO_BIN_EXE_autoshade"));
    command
        .arg("mcp")
        .arg("--root")
        .arg(root)
        .arg("--data-dir")
        .arg(data)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = autoshade::spawn_child(&mut command).unwrap();
    {
        let mut stdin = child.stdin.take().unwrap();
        for request in requests {
            writeln!(stdin, "{request}").unwrap();
        }
    }
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success(), "MCP server failed: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

fn request(id: u64, method: &str, params: Value) -> Value {
    json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params })
}

fn photo(tree: &TempTree) -> std::path::PathBuf {
    let path = tree.path().join("photo.png");
    RgbImage::from_pixel(16, 12, Rgb([32, 64, 96])).save(&path).unwrap();
    path
}

#[test]
fn an_mcp_client_can_list_tools_and_read_a_photo_under_its_root() {
    let tree = TempTree::new("mcp-list-read");
    photo(&tree);
    let replies = run_mcp(
        tree.path(),
        &tree.path().join("data"),
        &[
            request(1, "initialize", json!({ "protocolVersion": "2025-06-18" })),
            request(2, "tools/list", json!({})),
            request(
                3,
                "tools/call",
                json!({ "name": "frame.read", "arguments": { "photo": "photo.png" } }),
            ),
        ],
    );

    assert_eq!(replies.len(), 3);
    assert_eq!(replies[1]["result"]["tools"][0]["name"], "frame.read");
    assert_eq!(replies[2]["result"]["isError"], false);
    assert!(replies[2]["result"]["content"][1]["text"].as_str().unwrap().contains("\"width\":16"));
}

#[test]
fn an_mcp_frame_read_refuses_a_path_outside_its_root() {
    let tree = TempTree::new("mcp-root-escape");
    let root = tree.path().join("root");
    std::fs::create_dir_all(&root).unwrap();
    RgbImage::from_pixel(16, 12, Rgb([32, 64, 96])).save(root.join("photo.png")).unwrap();
    let outside = tree.path().join("outside.png");
    RgbImage::from_pixel(2, 2, Rgb([255, 0, 0])).save(&outside).unwrap();
    let replies = run_mcp(
        &root,
        &tree.path().join("data"),
        &[request(
            1,
            "tools/call",
            json!({ "name": "frame.read", "arguments": { "photo": "../outside.png" } }),
        )],
    );

    assert_eq!(replies[0]["result"]["isError"], true);
    assert!(replies[0]["result"]["content"][0]["text"]
        .as_str()
        .unwrap()
        .contains("not a file under a configured --root"));
}
