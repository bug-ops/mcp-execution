//! Acceptance tests of `Introspector::connect` and `Session::call_tool` (#574, FR-039): the
//! single connect step shares discovery's validation and errors, and a call classifies the reply
//! with the FR-030 value semantics, over Streamable HTTP and over the SSE transport selection.
//!
//! The stdio side (a spawned server process, `close` leaving no child) lives in
//! `crates/mcp-cli/tests/acceptance_session_stdio.rs`, next to the fixture binary.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::assert_matches;
use std::sync::Arc;
use std::time::Duration;

use axum::Router;
use mcp_execution_core::{Error, ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{
    CallTimeout, Introspector, ResultSource, ServerText, Session, ToolCall, ToolCallOutcome,
};
use rmcp::model::{
    CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
    InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities, Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{Map, Value, json};
use tokio_util::sync::CancellationToken;

/// A server whose tools exercise every reply class of `ToolCallOutcome`.
#[derive(Clone)]
struct Replies;

fn text_result(text: &str) -> CallToolResult {
    CallToolResult::success(vec![ContentBlock::text(text)])
}

impl ServerHandler for Replies {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("replies", "1.0.0"))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> {
        let tools = [
            "diverge",
            "json_text",
            "plain_text",
            "broken_json",
            "empty",
            "fail",
            "long_fail",
            "rpc_fail",
            "slow",
            "fast",
        ]
        .map(|name| Tool::new(name, name, Map::new()));
        std::future::ready(Ok(ListToolsResult::with_all_items(tools.to_vec())))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let result = match request.name.as_ref() {
            "diverge" => {
                let mut result = text_result(r#"{"from":"content"}"#);
                result.structured_content = Some(json!({"from": "structured"}));
                result
            }
            "json_text" => text_result(r#"{"k":1,"list":[1,2]}"#),
            "plain_text" => text_result("plain words"),
            "broken_json" => text_result("{not json"),
            "empty" => CallToolResult::success(Vec::new()),
            "fail" => CallToolResult::error(vec![ContentBlock::text("boom")]),
            "long_fail" => CallToolResult::error(vec![ContentBlock::text("e".repeat(5000))]),
            "rpc_fail" => return Err(McpError::invalid_params("bad params from server", None)),
            "slow" => {
                tokio::time::sleep(Duration::from_secs(4)).await;
                text_result("late")
            }
            "fast" => text_result("quick"),
            _ => return Err(McpError::invalid_params("unknown tool", None)),
        };
        Ok(result.into())
    }
}

async fn spawn_server() -> (String, CancellationToken) {
    let ct = CancellationToken::new();
    let config = StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token());
    let service: StreamableHttpService<Replies, LocalSessionManager> = StreamableHttpService::new(
        || Ok(Replies),
        Arc::new(LocalSessionManager::default()),
        config,
    );
    let router = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let shutdown = ct.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
            .await;
    });
    (format!("http://{addr}/mcp"), ct)
}

async fn open(url: &str) -> Session {
    let config = ServerConfig::builder()
        .http_transport(url.to_string())
        .build()
        .unwrap();
    Introspector::new()
        .connect(ServerId::new("replies").unwrap(), &config)
        .await
        .expect("connect over HTTP")
}

async fn call(session: &Session, tool: &str, source: ResultSource) -> ToolCallOutcome {
    call_within(session, tool, source, Duration::from_secs(10))
        .await
        .expect("call completes")
}

async fn call_within(
    session: &Session,
    tool: &str,
    source: ResultSource,
    limit: Duration,
) -> mcp_execution_core::Result<ToolCallOutcome> {
    let tool = ToolName::new(tool).unwrap();
    session
        .call_tool(
            ToolCall {
                tool: &tool,
                arguments: Map::new(),
                result_source: source,
            },
            CallTimeout::new(limit).unwrap(),
        )
        .await
}

#[tokio::test]
async fn connect_fails_like_discovery_for_a_server_that_cannot_start() {
    let config = ServerConfig::builder()
        .command("definitely-not-a-real-mcp-server-binary".to_string())
        .build()
        .unwrap();
    let id = ServerId::new("missing").unwrap();
    let introspector = Introspector::new();

    let discovered = introspector.discover_server(id.clone(), &config).await;
    let connected = introspector.connect(id, &config).await;

    let (Err(discovery_error), Err(connect_error)) = (discovered, connected) else {
        panic!("both must fail for a missing command");
    };
    assert_eq!(connect_error.to_string(), discovery_error.to_string());
}

#[tokio::test]
async fn call_tool_prefers_structured_content_when_asked_for_it() {
    let (url, ct) = spawn_server().await;
    let session = open(&url).await;

    let outcome = call(&session, "diverge", ResultSource::Structured).await;

    assert_eq!(
        outcome,
        ToolCallOutcome::Value(json!({"from": "structured"}))
    );
    ct.cancel();
}

#[tokio::test]
async fn call_tool_uses_the_first_content_block_when_asked_for_content() {
    let (url, ct) = spawn_server().await;
    let session = open(&url).await;

    let outcome = call(&session, "diverge", ResultSource::Content).await;

    assert_eq!(outcome, ToolCallOutcome::Value(json!({"from": "content"})));
    ct.cancel();
}

#[tokio::test]
async fn call_tool_parses_json_looking_text_and_keeps_other_text_raw() {
    let (url, ct) = spawn_server().await;
    let session = open(&url).await;

    let parsed = call(&session, "json_text", ResultSource::Content).await;
    let plain = call(&session, "plain_text", ResultSource::Content).await;
    let broken = call(&session, "broken_json", ResultSource::Content).await;

    assert_eq!(
        parsed,
        ToolCallOutcome::Value(json!({"k": 1, "list": [1, 2]}))
    );
    assert_eq!(
        plain,
        ToolCallOutcome::Value(Value::String("plain words".into()))
    );
    assert_eq!(
        broken,
        ToolCallOutcome::Value(Value::String("{not json".into()))
    );
    ct.cancel();
}

#[tokio::test]
async fn call_tool_reports_an_empty_success_as_empty() {
    let (url, ct) = spawn_server().await;
    let session = open(&url).await;

    let outcome = call(&session, "empty", ResultSource::Structured).await;

    assert_eq!(outcome, ToolCallOutcome::Empty);
    ct.cancel();
}

#[tokio::test]
async fn call_tool_reports_a_tool_error_with_bounded_server_text() {
    let (url, ct) = spawn_server().await;
    let session = open(&url).await;

    let short = call(&session, "fail", ResultSource::Content).await;
    let long = call(&session, "long_fail", ResultSource::Content).await;

    assert_eq!(
        short,
        ToolCallOutcome::ToolError {
            server_text: Some(ServerText {
                text: "boom".into(),
                truncated: false
            })
        }
    );
    let ToolCallOutcome::ToolError {
        server_text: Some(bounded),
    } = long
    else {
        panic!("expected a tool error with text, got {long:?}");
    };
    assert!(bounded.truncated);
    assert!(
        bounded.text.chars().count() <= 500,
        "{} chars",
        bounded.text.chars().count()
    );
    ct.cancel();
}

#[tokio::test]
async fn call_tool_reports_a_json_rpc_error_with_its_code() {
    let (url, ct) = spawn_server().await;
    let session = open(&url).await;

    let outcome = call(&session, "rpc_fail", ResultSource::Content).await;

    let ToolCallOutcome::RpcError { code, server_text } = outcome else {
        panic!("expected an rpc error, got {outcome:?}");
    };
    assert_eq!(code, Some(-32602));
    assert!(server_text.is_some_and(|t| t.text.contains("bad params")));
    ct.cancel();
}

#[tokio::test]
async fn call_tool_timeout_is_an_error_and_leaves_the_session_usable() {
    let (url, ct) = spawn_server().await;
    let session = open(&url).await;

    let timed_out = call_within(
        &session,
        "slow",
        ResultSource::Content,
        Duration::from_secs(1),
    )
    .await;
    let after = call(&session, "fast", ResultSource::Content).await;

    assert_matches!(
        timed_out,
        Err(Error::Timeout { .. }),
        "expected a timeout error, got {timed_out:?}"
    );
    assert_eq!(after, ToolCallOutcome::Value(Value::String("quick".into())));
    ct.cancel();
}

#[tokio::test]
async fn call_tool_over_the_sse_transport_selection_behaves_like_http() {
    let (url, ct) = spawn_server().await;
    let config = ServerConfig::builder().sse_transport(url).build().unwrap();
    let session = Introspector::new()
        .connect(ServerId::new("replies-sse").unwrap(), &config)
        .await
        .expect("connect over SSE selection");

    let outcome = call(&session, "json_text", ResultSource::Content).await;

    assert_eq!(
        outcome,
        ToolCallOutcome::Value(json!({"k": 1, "list": [1, 2]}))
    );
    ct.cancel();
}
