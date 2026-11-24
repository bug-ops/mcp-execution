//! Acceptance tests of single-call mode through the spawned helper (#575): a generated tool file
//! run directly with `node <tool>.ts '<json>'` reaches its server through
//! `mcp-execution-cli helper` (found via `MCP_EXECUTION_CLI`), over stdio and HTTP.
//!
//! Requires the `test-fixtures` feature and Node.js 22.18+. Every test needs `mcp.json`, which
//! cannot be redirected on Windows (#604).

#![cfg(feature = "test-fixtures")]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::time::Duration;

use common::{Out, Sandbox, WATCHDOG, require_node, run_with_watchdog};
use serde_json::{Value, json};

const PROMPT: Duration = Duration::from_secs(25);

/// Runs `node <servers>/<server>/<file> <json>` in the sandbox environment.
fn run_tool(sb: &Sandbox, server: &str, file: &str, json: &str, cli: Option<&str>) -> Out {
    let tool = sb.servers.join(server).join(file);
    let mut cmd = sb.command_for("node");
    cmd.arg(&tool).arg(json);
    if let Some(cli) = cli {
        cmd.env(mcp_execution_core::helper_protocol::CLI_PATH_ENV, cli);
    }
    run_with_watchdog(&mut cmd, None, WATCHDOG)
}

/// A `MCP_EXECUTION_CLI` stand-in that records its arguments, then runs the real CLI. Returns the
/// wrapper path and the file it appends to.
#[cfg(unix)]
fn recording_cli(sb: &Sandbox) -> (std::path::PathBuf, std::path::PathBuf) {
    use std::os::unix::fs::PermissionsExt as _;
    let record = sb.root.join("helper-invocations.log");
    let wrapper = sb.root.join("recording-cli.sh");
    std::fs::write(
        &wrapper,
        format!(
            "#!/bin/sh\necho \"$@\" >> '{}'\nexec '{}' \"$@\"\n",
            record.display(),
            common::CLI_BIN
        ),
    )
    .unwrap();
    std::fs::set_permissions(&wrapper, std::fs::Permissions::from_mode(0o755)).unwrap();
    (wrapper, record)
}

#[cfg(unix)]
fn assert_helper_was_spawned(record: &std::path::Path) {
    let invoked = std::fs::read_to_string(record).unwrap_or_default();
    assert!(
        invoked.lines().any(|l| l.trim() == "helper"),
        "the bridge must spawn `mcp-execution-cli helper`; invocations: {invoked:?}"
    );
}

#[cfg(unix)]
#[test]
fn single_call_reaches_a_stdio_server_through_the_spawned_helper() {
    if !require_node("single_call_reaches_a_stdio_server_through_the_spawned_helper") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    let (wrapper, record) = recording_cli(&sb);

    let out = run_tool(
        &sb,
        "fixture",
        "echo.ts",
        r#"{"a":1}"#,
        Some(&wrapper.display().to_string()),
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let printed: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(printed, json!({"a": 1}));
    assert!(out.elapsed < PROMPT, "took {:?}", out.elapsed);
    assert_eq!(sb.fixture_log(), ["initialize", "call echo"]);
    assert_helper_was_spawned(&record);
}

#[test]
#[cfg_attr(
    windows,
    ignore = "mcp.json cannot be redirected on Windows; tracked in #604"
)]
fn single_call_reaches_an_http_server_through_the_spawned_helper() {
    if !require_node("single_call_reaches_an_http_server_through_the_spawned_helper") {
        return;
    }
    let sb = Sandbox::new();
    let (_server, url) = common::spawn_http_fixture(None);
    sb.add_http_server("fixture-http", &url);
    sb.generate_server("fixture-http");

    let out = run_tool(&sb, "fixture-http", "echo.ts", r#"{"over":"http"}"#, None);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let printed: Value = serde_json::from_str(out.stdout.trim()).unwrap();
    assert_eq!(printed, json!({"over": "http"}));
}

#[cfg(unix)]
#[test]
fn single_call_tool_error_keeps_the_structured_failure_line() {
    if !require_node("single_call_tool_error_keeps_the_structured_failure_line") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    let (wrapper, record) = recording_cli(&sb);

    let out = run_tool(
        &sb,
        "fixture",
        "error.ts",
        "{}",
        Some(&wrapper.display().to_string()),
    );

    assert_eq!(out.code, Some(1), "stdout: {}", out.stdout);
    assert!(out.stdout.is_empty(), "stdout: {:?}", out.stdout);
    let last = out.stderr_lines().last().copied().expect("a stderr line");
    let line: Value = serde_json::from_str(last).unwrap();
    assert_eq!(line["kind"], "tool_error");
    assert_eq!(line["server_message"], "fixture tool error");
    assert_helper_was_spawned(&record);
}

#[test]
#[cfg_attr(
    windows,
    ignore = "mcp.json cannot be redirected on Windows; tracked in #604"
)]
fn single_call_without_a_findable_binary_fails_fast_and_names_the_variable() {
    if !require_node("single_call_without_a_findable_binary_fails_fast_and_names_the_variable") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    let missing = sb.root.join("no-such-binary");

    let out = run_tool(
        &sb,
        "fixture",
        "echo.ts",
        "{}",
        Some(&missing.display().to_string()),
    );

    assert_ne!(out.code, Some(0));
    assert!(out.elapsed < PROMPT, "took {:?}", out.elapsed);
    assert!(
        out.stderr
            .contains(mcp_execution_core::helper_protocol::CLI_PATH_ENV),
        "stderr must say how to fix it: {}",
        out.stderr
    );
    assert!(sb.fixture_log().is_empty(), "no server may be started");
}
