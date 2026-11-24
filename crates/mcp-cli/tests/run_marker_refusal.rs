//! `call` and `helper` refuse to run inside `mcp-execution-cli run` (the run marker is set):
//! a nested CLI would reach servers without the run's journal and dry-run policy.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Out, Sandbox, WATCHDOG, run_with_watchdog};
use mcp_execution_core::helper_protocol::RUN_MARKER_ENV;

fn inside_run(sb: &Sandbox, args: &[&str]) -> Out {
    let mut cmd = sb.command();
    cmd.args(args).env(RUN_MARKER_ENV, "1");
    run_with_watchdog(&mut cmd, None, WATCHDOG)
}

#[test]
fn call_inside_run_exits_2_without_connecting() {
    let sb = Sandbox::new();

    let out = inside_run(
        &sb,
        &["call", "--from-config", "fixture", "--", "echo", "{}"],
    );

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(
        out.stderr
            .contains("not supported inside `mcp-execution-cli run`")
    );
    assert!(sb.fixture_log().is_empty(), "no server may be started");
}

#[test]
fn helper_inside_run_exits_2_without_serving() {
    let sb = Sandbox::new();

    let out = inside_run(&sb, &["helper"]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(
        out.stderr
            .contains("not supported inside `mcp-execution-cli run`")
    );
    assert!(
        out.stdout.is_empty(),
        "the helper must not speak the protocol"
    );
}

#[test]
#[cfg_attr(
    windows,
    ignore = "mcp.json cannot be redirected on Windows; tracked in #604"
)]
fn call_outside_run_is_unaffected() {
    let sb = Sandbox::new();

    let out = sb.cli(&["call", "--from-config", "fixture", "--", "echo", "{}"]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
}
