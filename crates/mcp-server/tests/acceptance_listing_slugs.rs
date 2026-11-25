//! Acceptance tests of server ids on the server surface: `list_generated_servers` lists only
//! directories whose name is a server slug (#687) and a Windows reserved device name is a tool
//! error (#696).
//!
//! The wire tests drive the real binary over stdio, since the tool handlers are private.
#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

use mcp_execution_server::types::GeneratedServerInfo;
use serde_json::{Value, json};
use std::io::{BufRead as _, BufReader, Write as _};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

const INITIALIZE: &str = r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"acceptance","version":"0.0.0"}}}"#;
const INITIALIZED: &str = r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#;

fn tool_response(home: &Path, tool: &str, arguments: &Value) -> Value {
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-execution"))
        .env("HOME", home)
        .env("USERPROFILE", home)
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
        "params": { "name": tool, "arguments": arguments },
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

#[test]
fn list_generated_servers_lists_only_directories_named_by_a_server_slug() {
    let home = tempfile::tempdir().unwrap();
    let servers = home.path().join(".claude").join("servers");
    for name in ["github", "My_Dir", ".cache", "a.b"] {
        std::fs::create_dir_all(servers.join(name)).unwrap();
        std::fs::write(servers.join(name).join("tool.ts"), "export {};\n").unwrap();
    }

    let response = tool_response(home.path(), "list_generated_servers", &json!({}));

    let text = response["result"]["content"][0]["text"]
        .as_str()
        .expect("text content");
    let listing: Value = serde_json::from_str(text).expect("listing JSON");
    let ids: Vec<&str> = listing["servers"]
        .as_array()
        .expect("servers array")
        .iter()
        .map(|server| server["id"].as_str().expect("id is a plain string"))
        .collect();
    assert_eq!(ids, ["github"], "{listing}");
    assert_eq!(listing["total_servers"], 1, "{listing}");
}

#[test]
fn generated_server_info_deserialize_rejects_a_non_slug_id() {
    let info = |id: &str| {
        serde_json::from_value::<GeneratedServerInfo>(json!({
            "id": id, "tool_count": 1, "generated_at": null, "output_dir": "d",
        }))
    };

    assert!(info("github").is_ok());
    for bad in ["My_Dir", ".cache", ""] {
        assert!(info(bad).is_err(), "{bad:?} was accepted");
    }
}

#[test]
fn introspect_server_rejects_a_reserved_server_id_as_a_tool_error() {
    let home = tempfile::tempdir().unwrap();

    let response = tool_response(
        home.path(),
        "introspect_server",
        &json!({ "server_id": "nul", "command": "true" }),
    );

    assert!(response.get("error").is_none(), "{response}");
    let result = &response["result"];
    assert_eq!(result["isError"], true, "{response}");
    let text = result["content"][0]["text"].as_str().unwrap_or_default();
    assert!(text.contains("reserved device name"), "{text}");
}
