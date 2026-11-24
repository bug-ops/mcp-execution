//! Protocol-level tests of the per-run helper channel on the Unix carrier (#575): the script's
//! file descriptor 0 is a socket to an in-process `HelperServer`, driven by a scripted Node
//! client.
//!
//! Requires the `test-fixtures` feature and Node.js.

#![cfg(all(unix, feature = "test-fixtures"))]
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::process::{Command, Stdio};
use std::time::Duration;

use common::require_node;
use mcp_execution_cli::runtime::helper::{Carrier, DryRunMode, HelperConfig, HelperServer};
use mcp_execution_core::helper_protocol::MAX_HELPER_LINE_BYTES;
use mcp_execution_introspector::CallTimeout;
use serde_json::{Value, json};

/// Sends every `H_LINES` entry (a string as is, a number as that many `x` bytes) over fd 0,
/// prints each received line and a `closed` event, and exits after `H_WAIT_MS`.
const CLIENT_JS: &str = r"
const net = require('node:net');
const sock = new net.Socket({ fd: 0, readable: true, writable: true });
const emit = (o) => process.stdout.write(JSON.stringify(o) + '\n');
let buf = '';
sock.setEncoding('utf8');
sock.on('data', (d) => {
  buf += d;
  let i;
  while ((i = buf.indexOf('\n')) >= 0) { emit({ recv: buf.slice(0, i) }); buf = buf.slice(i + 1); }
});
sock.on('close', () => { emit({ closed: true }); process.exit(0); });
sock.on('error', () => {});
for (const line of JSON.parse(process.env.H_LINES)) {
  sock.write((typeof line === 'number' ? 'x'.repeat(line) : line) + '\n');
}
setTimeout(() => process.exit(0), Number(process.env.H_WAIT_MS));
";

struct Exchange {
    received: Vec<Value>,
    closed: bool,
}

async fn exchange(lines: Vec<Value>, wait: Duration) -> Exchange {
    let dir = tempfile::tempdir().unwrap();
    let script = dir.path().join("client.js");
    std::fs::write(&script, CLIENT_JS).unwrap();
    let (handle, mut endpoint) = HelperServer::bind(
        Carrier::native(),
        HelperConfig {
            servers_dir: dir.path().to_path_buf(),
            call_timeout: CallTimeout::new(Duration::from_secs(30)).unwrap(),
            journal: None,
            dry_run: DryRunMode::Off,
        },
    )
    .expect("bind the helper");
    let stdin = endpoint.take_stdin().expect("fd 0 carrier has a stdin");
    let output = tokio::task::spawn_blocking(move || {
        Command::new("node")
            .arg(&script)
            .env("H_LINES", Value::Array(lines).to_string())
            .env("H_WAIT_MS", wait.as_millis().to_string())
            .stdin(stdin)
            .stdout(Stdio::piped())
            .output()
            .unwrap()
    })
    .await
    .unwrap();
    handle.finish().await;

    let events: Vec<Value> = String::from_utf8(output.stdout)
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    Exchange {
        closed: events.iter().any(|e| e["closed"] == true),
        received: events
            .iter()
            .filter_map(|e| e["recv"].as_str())
            .map(|l| serde_json::from_str(l).unwrap())
            .collect(),
    }
}

fn hello(version: u32) -> Value {
    Value::String(json!({ "v": version }).to_string())
}

fn request(id: u64, server: &str) -> Value {
    Value::String(
        json!({"id": id, "server": server, "tool": "echo", "arguments": {}, "result": "content"})
            .to_string(),
    )
}

#[tokio::test]
async fn matching_hello_is_answered_and_the_channel_stays_open() {
    if !require_node("matching_hello_is_answered_and_the_channel_stays_open") {
        return;
    }

    let got = exchange(vec![hello(1)], Duration::from_millis(600)).await;

    assert_eq!(got.received, [json!({"v": 1})]);
    assert!(!got.closed);
}

#[tokio::test]
async fn a_version_mismatch_closes_the_channel_without_a_reply() {
    if !require_node("a_version_mismatch_closes_the_channel_without_a_reply") {
        return;
    }

    let got = exchange(vec![hello(999)], Duration::from_secs(4)).await;

    assert!(got.received.is_empty(), "{:?}", got.received);
    assert!(got.closed);
}

#[tokio::test]
async fn an_oversized_request_gets_call_failed_and_closes_the_channel() {
    if !require_node("an_oversized_request_gets_call_failed_and_closes_the_channel") {
        return;
    }
    let oversized = json!(MAX_HELPER_LINE_BYTES + 1024);

    let got = exchange(vec![hello(1), oversized], Duration::from_secs(4)).await;

    assert!(
        got.received
            .iter()
            .any(|r| r["error"]["kind"] == "call_failed"),
        "{:?}",
        got.received
    );
    assert!(got.closed);
}

#[tokio::test]
async fn a_malformed_request_is_rejected_with_its_id_and_closes_the_channel() {
    if !require_node("a_malformed_request_is_rejected_with_its_id_and_closes_the_channel") {
        return;
    }
    let bad = Value::String(json!({"id": 5, "server": "../x"}).to_string());

    let got = exchange(vec![hello(1), bad], Duration::from_secs(4)).await;

    let reply = &got.received[1];
    assert_eq!(reply["id"], 5);
    assert_eq!(reply["error"]["kind"], "call_failed");
    assert!(got.closed);
}

#[tokio::test]
async fn a_call_to_an_unknown_server_fails_by_id_and_the_channel_survives() {
    if !require_node("a_call_to_an_unknown_server_fails_by_id_and_the_channel_survives") {
        return;
    }

    let got = exchange(
        vec![hello(1), request(7, "no-such-server-for-helper-test")],
        Duration::from_millis(1500),
    )
    .await;

    let reply = &got.received[1];
    assert_eq!(reply["id"], 7);
    assert_eq!(reply["error"]["kind"], "call_failed");
    assert!(!got.closed);
}
