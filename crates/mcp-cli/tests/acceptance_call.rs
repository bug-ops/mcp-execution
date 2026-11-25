//! Acceptance tests of the one-shot `call` command (#574): `call <ServerFlags> -- TOOL [JSON]`.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]

mod common;

use common::{FIXTURE_BIN, Out, Sandbox, spawn_http_fixture};
use serde_json::{Value, json};

const SECRET: &str = "SECRET-9f3a-do-not-echo";

/// `call <fixture over stdio> -- <tool> [json]`.
fn call_stdio(sb: &Sandbox, tool: &str, json: Option<&str>) -> Out {
    let env = format!("MCP_FIXTURE_LOG={}", sb.log.display());
    let mut args = vec!["call", FIXTURE_BIN, "-e", env.as_str(), "--", tool];
    args.extend(json);
    sb.cli(&args)
}

fn stdout_json(out: &Out) -> Value {
    serde_json::from_str(out.stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {:?}", out.stdout))
}

#[test]
fn call_prints_the_parsed_result_of_a_content_tool() {
    let sb = Sandbox::new();

    let out = call_stdio(&sb, "echo", Some(r#"{"a":1}"#));

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(stdout_json(&out), json!({"a": 1}));
}

#[test]
fn call_prefers_structured_content_when_the_tool_declares_it() {
    let sb = Sandbox::new();

    let out = call_stdio(&sb, "structured", Some("{}"));

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(stdout_json(&out), json!({"ok": true, "items": [1, 2, 3]}));
}

#[test]
fn call_without_a_json_argument_sends_an_empty_object() {
    let sb = Sandbox::new();

    let out = call_stdio(&sb, "echo", None);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(stdout_json(&out), json!({}));
}

#[test]
fn call_empty_success_prints_nothing_and_exits_0() {
    let sb = Sandbox::new();

    let out = call_stdio(&sb, "empty", Some("{}"));

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.trim().is_empty(), "stdout: {:?}", out.stdout);
}

#[test]
fn call_tool_error_exits_3_with_a_one_line_stderr() {
    let sb = Sandbox::new();

    let out = call_stdio(&sb, "error", Some("{}"));

    assert_eq!(out.code, Some(3), "stdout: {}", out.stdout);
    assert_eq!(out.stderr_lines().len(), 1, "stderr: {}", out.stderr);
    assert_eq!(out.stdout.trim(), "");
}

#[test]
fn call_rejects_a_non_object_or_malformed_argument_without_echoing_it() {
    let sb = Sandbox::new();
    let not_an_object = format!(r#""{SECRET}""#);
    let malformed = format!(r#"{{"k":"{SECRET}""#);

    for argument in [not_an_object, malformed] {
        let out = call_stdio(&sb, "echo", Some(&argument));

        assert_eq!(out.code, Some(2), "argument {argument}");
        assert!(!out.stdout.contains(SECRET) && !out.stderr.contains(SECRET));
    }
    assert!(sb.fixture_log().is_empty(), "no server may be started");
}

#[test]
fn call_rejects_an_invalid_tool_name_before_connecting() {
    let sb = Sandbox::new();

    let out = call_stdio(&sb, "bad name!", Some("{}"));

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(sb.fixture_log().is_empty(), "no server may be started");
}

#[test]
fn call_spills_output_over_the_budget() {
    let sb = Sandbox::new();

    let out = call_stdio(&sb, "large", Some(r#"{"bytes":100000}"#));

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stdout.len() < 60_000,
        "stdout was {} bytes",
        out.stdout.len()
    );
    let spill = sb
        .state_files("spill")
        .into_iter()
        .find(|f| std::fs::metadata(f).is_ok_and(|m| m.len() == 100_002))
        .expect(
            "a spill file holds the full result in JSON form (100000 characters plus two quotes)",
        );
    assert!(out.stdout.contains(&spill.display().to_string()));
    assert!(
        out.stdout.contains("100002") || out.stdout.contains("100,002"),
        "the notice counts the compact JSON form (the string plus its two quotes): {}",
        out.stdout
    );
}

#[test]
fn call_reaches_a_server_over_http() {
    let sb = Sandbox::new();
    let (_server, url) = spawn_http_fixture(Some(&sb.log));

    let out = sb.cli(&["call", "--http", &url, "--", "echo", r#"{"over":"http"}"#]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(stdout_json(&out), json!({"over": "http"}));
}

#[test]
fn call_resolves_a_server_from_mcp_json() {
    let sb = Sandbox::new();

    let out = sb.cli(&[
        "call",
        "--from-config",
        "fixture",
        "--",
        "echo",
        r#"{"b":2}"#,
    ]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(stdout_json(&out), json!({"b": 2}));
}

#[cfg(unix)]
#[test]
fn call_leaves_no_server_process_behind() {
    let sb = Sandbox::new();

    let out = sb.cli(&["call", "--from-config", "fixture", "--", "echo", "{}"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert_eq!(
        sb.fixture_log(),
        ["initialize", "call echo"],
        "the call must have reached the server"
    );
    assert_eq!(
        common::wait_no_processes(&sb.marker(), std::time::Duration::from_secs(10)),
        0
    );
}
