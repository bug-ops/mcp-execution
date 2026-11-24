//! Acceptance tests of the retired bridge (#575, FR-040.3/4): the generated
//! `_runtime/mcp-bridge.ts` is a helper-protocol client. It has no handshake pin, no `mcp.json`
//! read, no mirrored validation and no fd-0 access of its own, and it takes the protocol
//! constants from `mcp_execution_core::helper_protocol`.
//!
//! Not repeated here because they hold today and are covered by `runtime_bridge.rs` and the
//! generator's unit tests: the file count (tool count + 7) and that the generated package passes
//! `tsc`.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod acceptance_support;

use acceptance_support::{file, generate, server_info, tool};
use mcp_execution_core::helper_protocol::{
    CLI_PATH_ENV, HELPER_ENV, HELPER_TOKEN_ENV, MAX_HELPER_LINE_BYTES,
};
use serde_json::json;

const BRIDGE: &str = "_runtime/mcp-bridge.ts";

fn bridge() -> String {
    let server = server_info(vec![tool("get_thing", json!({"type": "object"}), None)]);
    file(&generate(&server), BRIDGE).to_string()
}

#[test]
fn bridge_has_no_protocol_handshake_pin() {
    assert!(!bridge().contains("2024-11-05"));
}

#[test]
fn bridge_does_not_read_mcp_json_or_mirror_the_rust_validation() {
    let bridge = bridge();

    for retired in [
        "mcp.json",
        "mcpServers",
        "LD_PRELOAD",
        "NODE_OPTIONS",
        "homedir",
    ] {
        assert!(
            !bridge.contains(retired),
            "the bridge must not carry {retired:?}: connection code and mirrored validation moved to the Rust helper"
        );
    }
}

#[test]
fn bridge_renders_the_helper_protocol_constants_from_core() {
    let bridge = bridge();

    for constant in [HELPER_ENV, HELPER_TOKEN_ENV, CLI_PATH_ENV] {
        assert!(bridge.contains(constant), "{constant} not rendered");
    }
    let line_bound = MAX_HELPER_LINE_BYTES.to_string();
    let grouped = format!(
        "{}_{}_{}",
        &line_bound[..1],
        &line_bound[1..4],
        &line_bound[4..]
    );
    assert!(
        bridge.contains(&line_bound) || bridge.contains(&grouped),
        "the line bound {line_bound} must come from core"
    );
}

#[test]
fn bridge_tells_the_user_to_regenerate_on_a_protocol_version_mismatch() {
    assert!(bridge().to_lowercase().contains("regenerate"));
}

#[test]
fn bridge_error_kinds_include_write_skipped_for_dry_runs() {
    assert!(bridge().contains("write_skipped"));
}

#[test]
fn bridge_uses_the_registered_channel_and_never_opens_fd_zero() {
    let bridge = bridge();

    assert!(
        bridge.contains("mcp-execution.helper-channel"),
        "the preload registers the channel under this symbol"
    );
    assert!(
        !bridge.contains("fd: 0") && !bridge.contains("fd:0"),
        "the bridge must never open fd 0 itself"
    );
}
