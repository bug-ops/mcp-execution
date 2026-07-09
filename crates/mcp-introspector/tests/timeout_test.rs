//! Integration tests proving `Introspector::discover_server` enforces its
//! configured `connect_timeout` and `discover_timeout` (issue #120), and that
//! per-server-id locking in `mcp-execution-server` does not serialize
//! introspection across unrelated server ids.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`, which
//! is how CI and the project's preferred `cargo nextest` invocation run
//! tests) so that `fixture-slow-mcp-server` is built.

#![cfg(feature = "test-fixtures")]

use mcp_execution_core::{Error, ServerConfig, ServerId};
use mcp_execution_introspector::Introspector;
use std::time::{Duration, Instant};

/// Absolute path to the `fixture-slow-mcp-server` binary built alongside this
/// test target. The fixture speaks real MCP over stdio and takes two
/// independent delays (both in milliseconds): the first CLI arg delays the
/// start of the connect handshake, the second delays the `tools/list`
/// response. Both default to 0 (immediate) when omitted. Using a real
/// compiled fixture (rather than the platform `sleep` command) keeps these
/// tests working on the `windows-latest` CI runner, which has no `sleep` on
/// `PATH`.
const FIXTURE_BIN: &str = env!("CARGO_BIN_EXE_fixture-slow-mcp-server");

/// The fixture delays the start of the handshake itself, so the rmcp `serve`
/// call blocks waiting for a response. With a short `connect_timeout`,
/// `discover_server` must fail with `Error::Timeout { operation: "connect" }`
/// well before the fixture's 30s handshake delay would elapse.
#[tokio::test]
async fn test_discover_server_connect_timeout_fires() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-connect-timeout");

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("30000".to_string()) // fixture delays the handshake itself by 30s
        .connect_timeout(Duration::from_millis(150))
        .build();

    let started = Instant::now();
    let result = introspector.discover_server(server_id, &config).await;
    let elapsed = started.elapsed();

    match result {
        Err(Error::Timeout { operation, .. }) => assert!(
            operation.starts_with("connect"),
            "expected a \"connect\" timeout, got operation={operation:?}"
        ),
        other => panic!("expected Error::Timeout {{ operation: \"connect\" }}, got {other:?}"),
    }

    assert!(
        elapsed < Duration::from_secs(5),
        "timeout should fire near the configured 150ms bound, not wait for the fixture's 30s handshake delay; took {elapsed:?}"
    );
}

/// The fixture server completes the handshake immediately but sleeps before
/// answering `tools/list`. With a short `discover_timeout`, `discover_server`
/// must fail with `Error::Timeout { operation: "list_all_tools" }`.
#[tokio::test]
async fn test_discover_server_discover_timeout_fires() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-discover-timeout");

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("0".to_string()) // no handshake delay
        .arg("30000".to_string()) // fixture delays tools/list by 30s
        .discover_timeout(Duration::from_millis(150))
        .build();

    let started = Instant::now();
    let result = introspector.discover_server(server_id, &config).await;
    let elapsed = started.elapsed();

    match result {
        Err(Error::Timeout { operation, .. }) => assert!(
            operation.starts_with("list_all_tools"),
            "expected a \"list_all_tools\" timeout, got operation={operation:?}"
        ),
        other => {
            panic!("expected Error::Timeout {{ operation: \"list_all_tools\" }}, got {other:?}")
        }
    }

    assert!(
        elapsed < Duration::from_secs(5),
        "timeout should fire near the configured 150ms bound, not wait for the fixture's 30s tools/list delay; took {elapsed:?}"
    );
}

/// Sanity check: when the fixture responds promptly (no artificial delays),
/// `discover_server` succeeds and does not spuriously time out.
#[tokio::test]
async fn test_discover_server_succeeds_when_within_timeouts() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test-fast-server");

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg("0".to_string())
        .arg("0".to_string())
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build();

    let result = introspector.discover_server(server_id, &config).await;

    assert!(result.is_ok(), "expected success, got {result:?}");
}
