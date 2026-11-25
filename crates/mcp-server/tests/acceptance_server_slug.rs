//! Acceptance tests of `ServerSlug` on the server surface (#677): the JSON schema keeps its
//! shape, and a bad `server_id` in a request is rejected during argument deserialization with
//! a normal tool result (`isError: true`, the slug rule in the text), not a JSON-RPC error.
//!
//! The wire test drives the real binary over stdio, since the tool handlers are private.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use mcp_execution_core::ServerSlug;
use serde_json::{Value, json};
use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"acceptance","version":"0.0.0"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;
const SLUG_RULE: &str = "lowercase letters, digits, and hyphens";

#[test]
fn server_slug_schema_is_a_bounded_lowercase_pattern_string() {
    let schema = serde_json::to_value(schemars::schema_for!(ServerSlug)).unwrap();
    assert_eq!(schema["type"], "string");
    assert_eq!(schema["maxLength"], 64);
    assert_eq!(schema["pattern"], "^[a-z0-9-]+$");
}

fn tool_response(tool: &str, arguments: &Value) -> Value {
    let home = tempfile::tempdir().unwrap();
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-execution"))
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn mcp-execution");

    let mut stdin = child.stdin.take().expect("stdin");
    let stdout = child.stdout.take().expect("stdout");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Ok(message) = serde_json::from_str::<Value>(&line)
                && message["id"] == 2
            {
                let _ = tx.send(message);
                return;
            }
        }
    });

    let call = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/call",
        "params": {
            "name": tool,
            "arguments": arguments,
        },
    })
    .to_string();
    for line in [INITIALIZE, INITIALIZED, &call] {
        writeln!(stdin, "{line}").expect("write request");
    }
    stdin.flush().expect("flush");

    let response = rx.recv_timeout(Duration::from_secs(30));
    let _ = child.kill();
    let _ = child.wait();
    response.expect("a response to the tool call")
}

fn assert_rejected_as_tool_error(tool: &str, arguments: &Value, raw_id: &str) {
    let response = tool_response(tool, arguments);
    assert!(
        response.get("error").is_none(),
        "{tool} {raw_id:?}: {response}"
    );
    let result = &response["result"];
    assert_eq!(result["isError"], true, "{tool} {raw_id:?}: {response}");
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("server_id"), "{tool} {raw_id:?}: {text}");
    if !raw_id.is_empty() {
        assert!(text.contains(SLUG_RULE), "{tool} {raw_id:?}: {text}");
    }
    if raw_id.contains('<') {
        assert!(!text.contains(raw_id), "{tool}: raw id echoed: {text}");
    }
}

const BAD_IDS: [&str; 4] = ["Bad_Id", "a.b", "", "Evil<tag>"];

#[test]
fn introspect_server_rejects_a_bad_server_id_as_a_tool_error() {
    for bad in BAD_IDS {
        let arguments = json!({ "server_id": bad, "command": "true" });
        assert_rejected_as_tool_error("introspect_server", &arguments, bad);
    }
}

#[test]
fn generate_skill_rejects_a_bad_server_id_as_a_tool_error() {
    for bad in BAD_IDS {
        let arguments = json!({ "server_id": bad });
        assert_rejected_as_tool_error("generate_skill", &arguments, bad);
    }
}

#[test]
fn save_skill_rejects_a_bad_server_id_as_a_tool_error() {
    for bad in BAD_IDS {
        let arguments = json!({ "server_id": bad, "content": "x" });
        assert_rejected_as_tool_error("save_skill", &arguments, bad);
    }
}
