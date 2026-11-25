//! Stdio side of the `Session` acceptance tests (#574, FR-039.9/10): a call over a spawned server
//! process, a timeout that leaves the session usable, and `close` or `Drop` leaving no child.
//!
//! Lives in this crate because the fixture binary does; the in-process HTTP and SSE tests are in
//! `crates/mcp-introspector/tests/acceptance_session.rs`.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::assert_matches;
use std::time::Duration;

use common::FIXTURE_BIN;
use mcp_execution_core::helper_protocol::ResultSource;
use mcp_execution_core::{Error, ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{
    CallTimeout, Introspector, ServerText, Session, ToolCall, ToolCallOutcome,
};
use serde_json::{Value, json};

async fn connect(marker: &str) -> Session {
    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg(marker.to_string())
        .build()
        .unwrap();
    Introspector::new()
        .connect(ServerId::new("fixture").unwrap(), &config)
        .await
        .expect("connect over stdio")
}

async fn call(
    session: &Session,
    tool: &str,
    arguments: Value,
    limit: Duration,
) -> mcp_execution_core::Result<ToolCallOutcome> {
    let tool = ToolName::new(tool).unwrap();
    let Value::Object(arguments) = arguments else {
        panic!("arguments must be an object");
    };
    session
        .call_tool(
            ToolCall {
                tool: &tool,
                arguments,
                result_source: ResultSource::Structured,
            },
            CallTimeout::new(limit).unwrap(),
        )
        .await
}

fn marker(test: &str) -> String {
    format!("--session-test-marker={test}-{}", std::process::id())
}

#[tokio::test]
async fn stdio_call_tool_classifies_value_empty_tool_error_and_rpc_error() {
    let session = connect(&marker("classify")).await;
    let limit = Duration::from_secs(10);

    let value = call(&session, "echo", json!({"a": 1}), limit)
        .await
        .unwrap();
    let structured = call(&session, "structured", json!({}), limit)
        .await
        .unwrap();
    let empty = call(&session, "empty", json!({}), limit).await.unwrap();
    let tool_error = call(&session, "error", json!({}), limit).await.unwrap();
    let rpc_error = call(&session, "no_such_tool", json!({}), limit)
        .await
        .unwrap();

    assert_eq!(value, ToolCallOutcome::Value(json!({"a": 1})));
    assert_eq!(
        structured,
        ToolCallOutcome::Value(json!({"ok": true, "items": [1, 2, 3]}))
    );
    assert_eq!(empty, ToolCallOutcome::Empty);
    assert_eq!(
        tool_error,
        ToolCallOutcome::ToolError {
            server_text: Some(ServerText {
                text: "fixture tool error".into(),
                truncated: false
            })
        }
    );
    assert_matches!(rpc_error, ToolCallOutcome::RpcError { .. }, "{rpc_error:?}");
    session.close().await;
}

#[tokio::test]
async fn stdio_call_timeout_leaves_the_session_usable() {
    let session = connect(&marker("timeout")).await;

    let slow = call(
        &session,
        "slow",
        json!({"ms": 5000}),
        Duration::from_secs(1),
    )
    .await;
    let after = call(&session, "echo", json!({"n": 2}), Duration::from_secs(10)).await;

    assert_matches!(slow, Err(Error::Timeout { .. }), "{slow:?}");
    assert_eq!(after.unwrap(), ToolCallOutcome::Value(json!({"n": 2})));
    session.close().await;
}

#[cfg(unix)]
#[tokio::test]
async fn close_kills_the_server_process() {
    let marker = marker("close");
    let session = connect(&marker).await;
    assert!(
        call(
            &session,
            "echo",
            Value::Object(serde_json::Map::new()),
            Duration::from_secs(10)
        )
        .await
        .is_ok()
    );
    assert_eq!(
        common::processes_matching(&marker),
        1,
        "the server must be running"
    );

    session.close().await;

    assert_eq!(
        common::wait_no_processes(&marker, Duration::from_secs(10)),
        0
    );
}

#[cfg(unix)]
#[tokio::test]
async fn dropping_a_session_without_close_still_reaps_the_server_process() {
    let marker = marker("drop");
    let session = connect(&marker).await;
    assert_eq!(
        common::processes_matching(&marker),
        1,
        "the server must be running"
    );

    drop(session);

    assert_eq!(
        common::wait_no_processes(&marker, Duration::from_secs(10)),
        0
    );
}
