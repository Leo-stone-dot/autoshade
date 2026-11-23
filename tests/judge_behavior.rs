mod support;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

use autoshade::advisor::{Decision, JudgeImages, JudgeTask, judge_pair};
use support::{ScopedEnv, TempTree};

#[cfg(unix)]
fn fake_judge(tree: &TempTree, result: &str) -> (std::path::PathBuf, std::path::PathBuf) {
    let node = tree.path().join("fake-node");
    let script = tree.path().join("complete.mjs");
    let fixture = tree.path().join("judge-result.json");
    std::fs::write(&fixture, result).unwrap();
    std::fs::write(
        &node,
        b"#!/bin/sh\nset -eu\ncp \"$AUTOSHADE_TEST_JUDGE_RESULT\" \"$AUTOSHADE_MODEL_RESULT\"\n",
    )
    .unwrap();
    std::fs::set_permissions(&node, std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(&script, b"fake complete").unwrap();
    (node, script)
}

#[cfg(unix)]
#[test]
fn a_successful_judge_result_is_parsed_through_the_pi_harness_boundary() {
    let tree = TempTree::new("judge-success");
    let (node, script) = fake_judge(
        &tree,
        r#"{"ok":true,"text":"{\"score\":91,\"decision\":\"accept\",\"critique\":\"clean colour and framing\",\"hint\":null}"}"#,
    );
    let fixture = tree.path().join("judge-result.json");
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_NODE", &node);
    env.set("AUTOSHADE_PI_HARNESS", &script);
    env.set("AUTOSHADE_TEST_JUDGE_RESULT", &fixture);

    let judgement = judge_pair(
        JudgeImages { reference: b"reference", candidate: b"candidate" },
        JudgeTask::Develop,
        None,
        None,
    )
    .unwrap();

    assert_eq!(judgement.score, 91.0);
    assert_eq!(judgement.decision, Decision::Accept);
    assert_eq!(judgement.critique, "clean colour and framing");
    assert!(judgement.hint.is_none());
}

#[cfg(unix)]
#[test]
fn a_judge_failure_is_typed_and_does_not_leak_the_analysis_key() {
    let tree = TempTree::new("judge-failure");
    let (node, script) = fake_judge(&tree, r#"{"ok":false,"error":"synthetic judge failure"}"#);
    let fixture = tree.path().join("judge-result.json");
    let mut env = ScopedEnv::new();
    env.set("AUTOSHADE_DATA_DIR", tree.path());
    env.set("AUTOSHADE_NODE", &node);
    env.set("AUTOSHADE_PI_HARNESS", &script);
    env.set("AUTOSHADE_TEST_JUDGE_RESULT", &fixture);
    env.set("DEEPSEEK_API_KEY", "synthetic-secret-key");

    let error = judge_pair(
        JudgeImages { reference: b"reference", candidate: b"candidate" },
        JudgeTask::Develop,
        None,
        None,
    )
    .unwrap_err();
    let text = error.to_string();

    assert!(text.contains("synthetic judge failure"), "error={text}");
    assert!(!text.contains("synthetic-secret-key"), "error leaked the key: {text}");
}
