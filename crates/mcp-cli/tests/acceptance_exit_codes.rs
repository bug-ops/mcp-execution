//! Exit-code gaps of `call` and `run` (FR-036.9, FR-039.11): connect failure is 3, a connect
//! timeout is 4, an unknown `--from-config` name is a classified input or runtime error, SSE
//! works like HTTP, and a `run` that cannot resolve a server import fails as a script (6) or as a
//! classified runner error, never as a crash.
//!
//! There is no per-call timeout flag for `call`, so the timeout case uses the connect timeout.
//!
//! Requires the `test-fixtures` feature and Node.js 22.18+ for the `run` cases.

#![cfg(feature = "test-fixtures")]

mod common;

use std::assert_matches;
use std::net::TcpListener;
use std::time::Duration;

use common::{Sandbox, anchored, require_node, spawn_http_fixture};
use serde_json::{Value, json};

#[test]
fn call_to_a_server_that_cannot_start_exits_3() {
    let sb = Sandbox::new();

    let out = sb.cli(&[
        "call",
        "definitely-not-a-real-mcp-server-binary",
        "--",
        "echo",
    ]);

    assert_eq!(out.code, Some(3), "stderr: {}", out.stderr);
    assert!(out.stdout.trim().is_empty(), "stdout: {}", out.stdout);
}

#[test]
fn call_to_a_server_that_never_answers_exits_4_at_the_connect_timeout() {
    let sb = Sandbox::new();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/mcp", listener.local_addr().unwrap());
    let holder = std::thread::spawn(move || {
        let mut held = Vec::new();
        while let Ok((stream, _)) = listener.accept() {
            held.push(stream);
            if held.len() >= 4 {
                break;
            }
        }
        std::thread::sleep(Duration::from_secs(8));
    });

    let out = sb.cli(&[
        "call",
        "--http",
        &url,
        "--connect-timeout-secs",
        "1",
        "--",
        "echo",
    ]);

    assert_eq!(out.code, Some(4), "stderr: {}", out.stderr);
    assert!(
        out.elapsed < Duration::from_secs(20),
        "took {:?}",
        out.elapsed
    );
    drop(holder);
}

#[test]
fn call_with_an_unknown_config_name_fails_before_any_connection() {
    let sb = Sandbox::new();

    let out = sb.cli(&["call", "--from-config", "no-such-server", "--", "echo"]);

    assert_matches!(
        out.code,
        Some(1 | 2),
        "expected a classified error, got {:?}",
        out.code
    );
    assert!(
        out.stderr.contains("no-such-server"),
        "stderr: {}",
        out.stderr
    );
    assert!(sb.fixture_log().is_empty(), "no server may be started");
}

#[test]
fn call_over_the_sse_selection_prints_the_same_result_as_http() {
    let sb = Sandbox::new();
    let (_server, url) = spawn_http_fixture(None);

    let out = sb.cli(&["call", "--sse", &url, "--", "echo", r#"{"via":"sse"}"#]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let printed: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(printed, json!({"via": "sse"}));
}

#[test]
fn run_of_a_script_importing_an_unknown_server_fails_as_a_script() {
    if !require_node("run_of_a_script_importing_an_unknown_server_fails_as_a_script") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script(
        "unknown-import.ts",
        "import * as s from 'mcp:no-such-server';\nconsole.log(Object.keys(s));\n",
    );

    let out = sb.run_script(&script, &[]);

    assert_eq!(out.code, Some(6), "stderr: {}", out.stderr);
    assert!(anchored(&out.stdout).trailer().contains("exit 1"));
    assert!(!out.stderr.contains("panicked"), "stderr: {}", out.stderr);
}

#[test]
fn run_with_a_servers_dir_that_does_not_exist_never_crashes() {
    if !require_node("run_with_a_servers_dir_that_does_not_exist_never_crashes") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script(
        "needs-server.ts",
        "import * as s from 'mcp:fixture';\nconsole.log(Object.keys(s));\n",
    );
    let nowhere = sb.root.join("no-such-servers-dir").display().to_string();
    let script_arg = script.display().to_string();

    let out = sb.cli(&["run", &script_arg, "--servers-dir", &nowhere]);

    assert_matches!(
        out.code,
        Some(1 | 2 | 6),
        "expected a classified failure, got {:?} (stdout: {} stderr: {})",
        out.code,
        out.stdout,
        out.stderr
    );
    assert!(!out.stderr.contains("panicked"), "stderr: {}", out.stderr);
    assert!(sb.fixture_log().is_empty(), "no server may be started");
}
