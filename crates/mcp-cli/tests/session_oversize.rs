//! A stdio server answering with a line over `MAX_RESPONSE_LINE_BYTES` fails the call at once
//! with a limit error and poisons the session: the line cannot be attributed to a request, so
//! waiting would only run out the call timeout.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::time::{Duration, Instant};

use common::FIXTURE_BIN;
use mcp_execution_core::{Error, ResourceKind, ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{
    CallTimeout, Introspector, ResultSource, Session, ToolCall, ToolCallOutcome,
};
use serde_json::{Map, Value, json};

async fn call(
    session: &Session,
    tool: &str,
    arguments: Value,
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
                result_source: ResultSource::Content,
            },
            CallTimeout::new(Duration::from_secs(30)).unwrap(),
        )
        .await
}

const fn is_response_line_limit(result: &mcp_execution_core::Result<ToolCallOutcome>) -> bool {
    matches!(
        result,
        Err(Error::ResourceLimitExceeded {
            resource: ResourceKind::ResponseLine { .. },
            ..
        })
    )
}

#[tokio::test]
async fn an_oversized_stdio_line_fails_the_call_fast_and_poisons_the_session() {
    let config = ServerConfig::builder()
        .command(FIXTURE_BIN.to_string())
        .arg(format!("--oversize-test-marker={}", std::process::id()))
        .build()
        .unwrap();
    let session = Introspector::new()
        .connect(ServerId::new("fixture").unwrap(), &config)
        .await
        .expect("connect");
    assert!(!session.is_poisoned());
    let started = Instant::now();

    let first = call(&session, "large", json!({"bytes": 5 * 1024 * 1024})).await;

    assert!(is_response_line_limit(&first), "{first:?}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(session.is_poisoned());

    let again = Instant::now();
    let next = call(&session, "echo", Value::Object(Map::new())).await;

    assert!(is_response_line_limit(&next), "{next:?}");
    assert!(again.elapsed() < Duration::from_secs(1));
    session.close().await;
}
