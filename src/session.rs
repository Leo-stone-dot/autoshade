//! Reading a pi-harness session transcript (`<store>/harness/sessions/*.json`).
//!
//! The transcript is the durable record of every AI decision: per-message
//! token usage/cost, the critic verdicts, the tool loop, and the commit. The
//! CLI (`autoshade session show|replay`) and the tests share these readers so
//! the numbers cannot drift between the printer and the replay.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde_json::Value;

/// Token/cost totals over one transcript.
#[derive(Debug, Default, Clone, Copy, PartialEq)]
pub struct Usage {
    pub input: u64,
    pub output: u64,
    pub cache_read: u64,
    pub total_tokens: u64,
    pub cost_usd: f64,
}

/// Sum the assistant messages' `usage` blocks.
pub fn usage_of(value: &Value) -> Usage {
    let mut u = Usage::default();
    let Some(messages) = value.get("messages").and_then(Value::as_array) else {
        return u;
    };
    for m in messages {
        if m.get("role").and_then(Value::as_str) != Some("assistant") {
            continue;
        }
        let Some(usage) = m.get("usage") else { continue };
        u.input += usage.get("input").and_then(Value::as_u64).unwrap_or(0);
        u.output += usage.get("output").and_then(Value::as_u64).unwrap_or(0);
        u.cache_read += usage.get("cacheRead").and_then(Value::as_u64).unwrap_or(0);
        u.total_tokens += usage.get("totalTokens").and_then(Value::as_u64).unwrap_or(0);
        u.cost_usd += usage
            .pointer("/cost/total")
            .and_then(Value::as_f64)
            .unwrap_or(0.0);
    }
    u
}

/// Assistant turns in the transcript.
pub fn assistant_turns(value: &Value) -> usize {
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|msgs| {
            msgs.iter()
                .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
                .count()
        })
        .unwrap_or(0)
}

/// Tool calls the model issued (`content[]` blocks of type `toolCall`).
pub fn tool_calls(value: &Value) -> usize {
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|msgs| {
            msgs.iter()
                .filter(|m| m.get("role").and_then(Value::as_str) == Some("assistant"))
                .filter_map(|m| m.get("content").and_then(Value::as_array))
                .flatten()
                .filter(|b| b.get("type").and_then(Value::as_str) == Some("toolCall"))
                .count()
        })
        .unwrap_or(0)
}

/// Tool results the harness fed back.
pub fn tool_results(value: &Value) -> usize {
    value
        .get("messages")
        .and_then(Value::as_array)
        .map(|msgs| {
            msgs.iter()
                .filter(|m| m.get("role").and_then(Value::as_str) == Some("toolResult"))
                .count()
        })
        .unwrap_or(0)
}

/// `["revise 80", "revise 68", …]` in critic order.
pub fn verdict_lines(value: &Value) -> Vec<String> {
    value
        .get("verdicts")
        .and_then(Value::as_array)
        .map(|a| {
            a.iter()
                .map(|v| {
                    format!(
                        "{} {}",
                        v.get("decision").and_then(Value::as_str).unwrap_or("?"),
                        v.get("score").and_then(Value::as_i64).unwrap_or(-1)
                    )
                })
                .collect()
        })
        .unwrap_or_default()
}

/// `(develop, xmp, note)` from `commit`, accepting both the object the harness
/// records today and the JSON string older transcripts kept. Empty strings mean
/// "not committed".
pub fn commit_paths(value: &Value) -> (String, String, String) {
    let owned = |v: &Value, key: &str| {
        v.get(key)
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string()
    };
    match value.get("commit") {
        Some(c) if c.is_object() => (owned(c, "develop"), owned(c, "xmp"), owned(c, "note")),
        Some(c) if c.is_string() => {
            let parsed: Value =
                serde_json::from_str(c.as_str().unwrap_or("")).unwrap_or(Value::Null);
            (owned(&parsed, "develop"), owned(&parsed, "xmp"), owned(&parsed, "note"))
        }
        _ => (String::new(), String::new(), String::new()),
    }
}

/// The failure class of a transcript, when it failed. New transcripts carry
/// `failure` (classified by the harness); older ones derive it from `error`.
pub fn failure_kind(value: &Value) -> Option<String> {
    if let Some(f) = value.get("failure").and_then(Value::as_str) {
        return Some(f.to_string());
    }
    let error = value.get("error").and_then(Value::as_str)?;
    Some(
        if error.starts_with("budget_exhausted") {
            "budget_exhausted"
        } else if error.starts_with("model_error") {
            "model_error"
        } else {
            "error"
        }
        .to_string(),
    )
}

/// Did the run stop on the turn budget? A budget stop may still have a
/// committed best candidate, so this is a failure CLASS, not "no output".
pub fn budget_exhausted(value: &Value) -> bool {
    value
        .get("budget_exhausted")
        .and_then(Value::as_bool)
        .unwrap_or(false)
        || failure_kind(value).as_deref() == Some("budget_exhausted")
}

/// Resolve a session id (`analyze-…`), a bare filename, or a JSON path against
/// the harness sessions directory. Ids win over files? No: an explicit existing
/// path is used as-is, then `<dir>/<id>.json`, then `<dir>/<arg>`.
pub fn resolve_session(arg: &str, sessions_dir: &Path) -> Result<PathBuf> {
    let direct = PathBuf::from(arg);
    if direct.is_file() {
        return Ok(direct);
    }
    for candidate in [
        sessions_dir.join(format!("{arg}.json")),
        sessions_dir.join(arg),
    ] {
        if candidate.is_file() {
            return Ok(candidate);
        }
    }
    anyhow::bail!(
        "no harness session {arg:?} — pass an id (analyze-…) or the JSON path; looked in {}",
        sessions_dir.display()
    )
}

/// Parse a session file.
pub fn load(path: &Path) -> Result<Value> {
    let text = std::fs::read_to_string(path)
        .with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}
