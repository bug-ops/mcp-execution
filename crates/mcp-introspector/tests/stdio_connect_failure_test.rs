//! Integration test proving `connect_and_list_tools`'s non-timeout
//! `Error::ConnectionFailed` mapping fires via the stdio path (issue #332).
//!
//! `discover_via_http`'s analogous branch is covered by
//! `test_discover_server_http_reserved_header_collision_is_legible` in
//! `tests/http_transport_test.rs`. On the stdio side,
//! `test_discover_server_nonexistent_command` in `tests/integration_test.rs`
//! only covers process *spawn* failure (the command never resolves on
//! `PATH`), which fails before `connect_and_list_tools` runs at all. This
//! test spawns a real, resolvable executable that exits immediately, closing
//! its stdout pipe before ever answering the `initialize` handshake, so the
//! failure instead surfaces from the handshake read itself — exercising the
//! `connect` future's `Err` arm inside `connect_and_list_tools`.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`, which
//! is how CI and the project's preferred `cargo nextest` invocation run
//! tests) so that `fixture-immediate-exit-server` is built.

#![cfg(feature = "test-fixtures")]

use mcp_execution_core::{Error, ServerConfig, ServerId};
use mcp_execution_introspector::Introspector;
use std::time::Duration;

/// Absolute path to the `fixture-immediate-exit-server` binary built
/// alongside this test target.
const FIXTURE_BIN: &str = env!("CARGO_BIN_EXE_fixture-immediate-exit-server");

#[tokio::test]
async fn test_discover_server_stdio_handshake_failure_is_connection_failed_not_timeout() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("stdio-immediate-exit").unwrap();

    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .connect_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let result = introspector.discover_server(server_id, &config).await;

    match result {
        Err(Error::ConnectionFailed { server, source }) => {
            assert_eq!(server, "stdio-immediate-exit");
            // Pins the failure to the handshake step inside `connect_and_list_tools`
            // (`lib.rs:991`), not to `spawn_introspection_child`'s spawn-failure or
            // pipe-capture `ConnectionFailed` sites (`lib.rs:628`/`646`/`650`) —
            // neither of which ever mentions "initialize". The fixture exits so
            // fast there's a genuine race between two legitimate outcomes at the
            // *same* intended site: the client's `initialize` write can fail first
            // (`"... Broken pipe ..., when send initialize request"`) or its read of
            // the response can fail first (`ServiceError::ConnectionClosed("initialize
            // response")`) — both are correct, so the check must accept either.
            let msg = source.to_string().to_ascii_lowercase();
            assert!(
                msg.contains("initialize"),
                "expected a handshake-failure error mentioning \"initialize\" (either the \
                 request write or the response read), got: {msg}"
            );
        }
        other => panic!("expected Error::ConnectionFailed, got {other:?}"),
    }
}
