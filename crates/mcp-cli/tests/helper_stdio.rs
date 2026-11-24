//! Tests of the hidden `helper` command (#575): the helper protocol over stdio against the
//! fixture server, without Node.js. Covers pooling (one initialize for concurrent first calls),
//! responses matched by id out of order, the result-size bound, and server shutdown at end of
//! input.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::io::{BufRead as _, BufReader, Write as _};
use std::process::{ChildStdin, Stdio};
use std::sync::mpsc::{self, Receiver};
use std::time::Duration;

use common::{KillOnDrop, Sandbox};
use serde_json::{Value, json};

struct Client {
    child: KillOnDrop,
    stdin: Option<ChildStdin>,
    lines: Receiver<Value>,
}

impl Client {
    fn start(sb: &Sandbox) -> Self {
        let mut child = sb
            .command()
            .arg("helper")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn helper");
        let stdin = child.stdin.take();
        let stdout = child.stdout.take().expect("stdout piped");
        let (sender, lines) = mpsc::channel();
        std::thread::spawn(move || {
            for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                if sender.send(serde_json::from_str(&line).unwrap()).is_err() {
                    break;
                }
            }
        });
        Self {
            child: KillOnDrop(child),
            stdin,
            lines,
        }
    }

    fn send(&mut self, line: &Value) {
        let stdin = self.stdin.as_mut().expect("stdin open");
        writeln!(stdin, "{line}").unwrap();
        stdin.flush().unwrap();
    }

    fn next(&self, within: Duration) -> Value {
        self.lines.recv_timeout(within).expect("a protocol line")
    }

    fn hello(&mut self) {
        self.send(&json!({"v": 1}));
        assert_eq!(self.next(Duration::from_secs(10)), json!({"v": 1}));
    }

    fn call(&mut self, id: u64, server: &str, tool: &str, arguments: &Value) {
        self.send(&json!({
            "id": id, "server": server, "tool": tool, "arguments": arguments, "result": "content"
        }));
    }

    fn close_input(&mut self) {
        drop(self.stdin.take());
    }
}

#[test]
fn a_call_returns_its_value_by_id() {
    let sb = Sandbox::new();
    let mut client = Client::start(&sb);
    client.hello();

    client.call(41, "fixture", "echo", &json!({"a": 1}));

    assert_eq!(
        client.next(Duration::from_secs(20)),
        json!({"id": 41, "value": {"a": 1}})
    );
}

#[test]
fn an_empty_success_and_a_tool_error_use_their_own_shapes() {
    let sb = Sandbox::new();
    let mut client = Client::start(&sb);
    client.hello();

    client.call(1, "fixture", "empty", &json!({}));
    client.call(2, "fixture", "error", &json!({}));

    let mut by_id = std::collections::BTreeMap::new();
    for _ in 0..2 {
        let line = client.next(Duration::from_secs(20));
        by_id.insert(line["id"].as_u64().unwrap(), line);
    }
    assert_eq!(by_id[&1], json!({"id": 1, "empty": true}));
    assert_eq!(by_id[&2]["error"]["kind"], "tool_error");
    assert_eq!(by_id[&2]["error"]["server_message"], "fixture tool error");
    assert_eq!(by_id[&2]["error"]["server_message_truncated"], false);
}

#[test]
fn concurrent_first_calls_open_one_connection() {
    let sb = Sandbox::new();
    let mut client = Client::start(&sb);
    client.hello();

    client.call(1, "fixture", "echo", &json!({"n": 1}));
    client.call(2, "fixture", "echo", &json!({"n": 2}));
    let first = client.next(Duration::from_secs(20));
    let second = client.next(Duration::from_secs(20));

    let mut ids = [
        first["id"].as_u64().unwrap(),
        second["id"].as_u64().unwrap(),
    ];
    ids.sort_unstable();
    assert_eq!(ids, [1, 2]);
    let log = sb.fixture_log();
    assert_eq!(
        log.iter().filter(|l| *l == "initialize").count(),
        1,
        "{log:?}"
    );
    assert_eq!(
        log.iter().filter(|l| *l == "call echo").count(),
        2,
        "{log:?} {first} {second}"
    );
}

#[test]
fn responses_may_arrive_out_of_order() {
    let sb = Sandbox::new();
    let mut client = Client::start(&sb);
    client.hello();

    client.call(1, "fixture", "slow", &json!({"ms": 1500}));
    client.call(2, "fixture", "echo", &json!({"tag": "fast"}));

    assert_eq!(
        client.next(Duration::from_secs(20)),
        json!({"id": 2, "value": {"tag": "fast"}})
    );
    assert_eq!(
        client.next(Duration::from_secs(20)),
        json!({"id": 1, "value": "done"})
    );
}

#[test]
fn a_result_over_the_line_bound_becomes_call_failed() {
    let sb = Sandbox::new();
    let (_server, url) = common::spawn_http_fixture(None);
    sb.add_http_server("fixture-http", &url);
    let mut client = Client::start(&sb);
    client.hello();

    client.call(
        3,
        "fixture-http",
        "large",
        &json!({"bytes": 5 * 1024 * 1024}),
    );

    let line = client.next(Duration::from_secs(30));
    assert_eq!(line["id"], 3);
    assert_eq!(line["error"]["kind"], "call_failed");
    assert!(
        line["error"]["message"]
            .as_str()
            .unwrap()
            .contains("exceeds")
    );
}

#[test]
fn a_stdio_response_line_over_the_cap_fails_the_call_at_once() {
    let sb = Sandbox::new();
    let mut client = Client::start(&sb);
    client.hello();
    let started = std::time::Instant::now();

    client.call(4, "fixture", "large", &json!({"bytes": 5 * 1024 * 1024}));

    let line = client.next(Duration::from_secs(20));
    assert_eq!(line["id"], 4);
    assert_eq!(line["error"]["kind"], "call_failed");
    assert!(
        line["error"]["message"]
            .as_str()
            .unwrap()
            .contains("exceeds")
    );
    assert!(
        started.elapsed() < Duration::from_secs(15),
        "the call must not wait for its timeout"
    );

    client.call(5, "fixture", "echo", &json!({"again": 1}));
    assert_eq!(
        client.next(Duration::from_secs(20)),
        json!({"id": 5, "value": {"again": 1}}),
        "the poisoned session must be replaced by a new connection"
    );
}

#[test]
fn request_id_zero_is_reserved_and_closes_the_channel() {
    let sb = Sandbox::new();
    let mut client = Client::start(&sb);
    client.hello();

    client.call(0, "fixture", "echo", &json!({}));

    let line = client.next(Duration::from_secs(10));
    assert_eq!(line["id"], 0);
    assert_eq!(line["error"]["kind"], "call_failed");
    assert!(client.lines.recv_timeout(Duration::from_secs(10)).is_err());
}

#[test]
fn an_unknown_server_is_a_call_failure_not_a_crash() {
    let sb = Sandbox::new();
    let mut client = Client::start(&sb);
    client.hello();

    client.call(9, "no-such-server", "echo", &json!({}));
    client.call(10, "fixture", "echo", &json!({"still": "alive"}));

    let mut kinds = std::collections::BTreeMap::new();
    for _ in 0..2 {
        let line = client.next(Duration::from_secs(20));
        kinds.insert(line["id"].as_u64().unwrap(), line);
    }
    assert_eq!(kinds[&9]["error"]["kind"], "call_failed");
    assert_eq!(kinds[&10]["value"], json!({"still": "alive"}));
}

#[cfg(unix)]
#[test]
fn closing_stdin_ends_the_helper_and_its_servers() {
    let sb = Sandbox::new();
    let mut client = Client::start(&sb);
    client.hello();
    client.call(1, "fixture", "echo", &json!({}));
    client.next(Duration::from_secs(20));
    assert_eq!(common::processes_matching(&sb.marker()), 1);

    client.close_input();

    let status = client.child.0.wait().unwrap();
    assert!(status.success(), "{status:?}");
    assert_eq!(
        common::wait_no_processes(&sb.marker(), Duration::from_secs(10)),
        0
    );
}
