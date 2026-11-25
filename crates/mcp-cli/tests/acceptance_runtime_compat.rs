//! Acceptance tests of generated tool files under Bun and Deno (#605): a tool file run directly
//! with `bun <tool>.ts '<json>'` or `deno run -A <tool>.ts '<json>'` reaches the fixture server
//! through `mcp-execution-cli helper` (found via `MCP_EXECUTION_CLI`) and prints the result.
//!
//! A runtime that is not installed skips its test unless `MCP_EXECUTION_REQUIRE_ALT_RUNTIMES=1`
//! (see `common::require_alt_runtime`); the `node-latest` CI job sets it.
//!
//! Requires the `test-fixtures` feature.

#![cfg(feature = "test-fixtures")]

mod common;

use std::path::Path;

use common::{ALT_RUNTIME_SKIP_MARKER, Sandbox, WATCHDOG, require_alt_runtime, run_with_watchdog};
use serde_json::{Value, json};

/// Generates the fixture package and runs its `echo.ts` with `{"a":1}` through `command`.
fn echo_through(program: &str, leading_args: &[&str]) {
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    let tool = sb.servers.join("fixture").join("echo.ts");
    let mut cmd = sb.command_for(program);
    cmd.args(leading_args).arg(&tool).arg(r#"{"a":1}"#);

    let out = run_with_watchdog(&mut cmd, None, WATCHDOG);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let printed: Value = serde_json::from_str(out.stdout.trim())
        .unwrap_or_else(|e| panic!("stdout is not JSON ({e}): {:?}", out.stdout));
    assert_eq!(printed, json!({"a": 1}));
    assert_eq!(sb.fixture_log(), ["initialize", "call echo"]);
}

#[test]
fn the_ci_ran_not_skipped_check_greps_the_skip_marker() {
    let workflow = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.github/workflows/ci.yml");
    let text = std::fs::read_to_string(workflow).unwrap();

    let grep = format!("grep -q '{ALT_RUNTIME_SKIP_MARKER}' compat.log");
    assert!(text.contains(&grep), "ci.yml must run `{grep}`");
}

#[test]
fn a_generated_tool_runs_under_bun() {
    if !require_alt_runtime("bun", "a_generated_tool_runs_under_bun") {
        return;
    }
    echo_through("bun", &[]);
}

#[test]
fn a_generated_tool_runs_under_deno() {
    if !require_alt_runtime("deno", "a_generated_tool_runs_under_deno") {
        return;
    }
    echo_through("deno", &["run", "-A"]);
}
