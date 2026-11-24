//! Acceptance tests for #568: annotation and title capture during discovery.
//!
//! An in-process Streamable HTTP server advertises one tool per capture rule and a single
//! `discover_server` call reads them all back.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]

use axum::Router;
use mcp_execution_core::annotations::{Hint, ToolAnnotations};
use mcp_execution_core::untrusted::MAX_UNTRUSTED_FIELD_LEN;
use mcp_execution_core::{ServerConfig, ServerId};
use mcp_execution_introspector::{Introspector, ServerInfo};
use rmcp::model::{
    Implementation, InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    Tool, ToolAnnotations as WireAnnotations,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;

#[derive(Clone)]
struct AnnotatedHandler;

fn tool(name: &'static str) -> Tool {
    Tool::new(name, "d", serde_json::Map::new())
}

fn wire(configure: impl FnOnce(&mut WireAnnotations)) -> WireAnnotations {
    let mut annotations = WireAnnotations::new();
    configure(&mut annotations);
    annotations
}

fn advertised_tools() -> Vec<Tool> {
    let mut falsy = tool("falsy");
    falsy.annotations = Some(wire(|a| a.read_only_hint = Some(false)));

    let mut top_title_wins = tool("top_title_wins");
    top_title_wins.title = Some("Top Title".to_string());
    top_title_wins.annotations = Some(wire(|a| a.title = Some("Annotation Title".to_string())));

    let mut annotation_title_only = tool("annotation_title_only");
    annotation_title_only.annotations =
        Some(wire(|a| a.title = Some("Annotation Title".to_string())));

    let mut title_only = tool("title_only");
    title_only.title = Some("Only Title".to_string());

    let mut contradictory = tool("contradictory");
    contradictory.annotations = Some(wire(|a| {
        a.read_only_hint = Some(true);
        a.destructive_hint = Some(true);
    }));

    let mut hostile_title = tool("hostile_title");
    hostile_title.title = Some(format!(
        "bad\u{1b}[31m{}",
        "x".repeat(2 * MAX_UNTRUSTED_FIELD_LEN)
    ));

    vec![
        tool("plain"),
        falsy,
        top_title_wins,
        annotation_title_only,
        title_only,
        contradictory,
        hostile_title,
    ]
}

impl ServerHandler for AnnotatedHandler {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("annotated-fixture-server", "1.0.0"))
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> {
        std::future::ready(Ok(ListToolsResult::with_all_items(advertised_tools())))
    }
}

async fn discover() -> ServerInfo {
    let ct = CancellationToken::new();
    let server_config =
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token());
    let service: StreamableHttpService<AnnotatedHandler, LocalSessionManager> =
        StreamableHttpService::new(
            || Ok(AnnotatedHandler),
            Arc::new(LocalSessionManager::default()),
            server_config,
        );
    let router: Router = Router::new().nest_service("/mcp", service);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture listener");
    let addr = listener
        .local_addr()
        .expect("fixture listener has a local addr");
    let shutdown = ct.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown.cancelled_owned().await })
            .await;
    });

    let config = ServerConfig::builder()
        .http_transport(format!("http://{addr}/mcp"))
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build()
        .expect("valid http config");
    let result = Introspector::new()
        .discover_server(ServerId::new("annotated-fixture").unwrap(), &config)
        .await;
    ct.cancel();
    result.expect("discovery against the in-process fixture succeeds")
}

fn captured(info: &ServerInfo, name: &str) -> Option<ToolAnnotations> {
    info.tools
        .iter()
        .find(|t| t.name.as_str() == name)
        .unwrap_or_else(|| panic!("tool {name} must be discovered"))
        .annotations
        .clone()
}

fn title_of(info: &ServerInfo, name: &str) -> Option<String> {
    captured(info, name)
        .and_then(|a| a.title)
        .map(|t| t.as_str().to_string())
}

#[tokio::test]
async fn discover_server_keeps_stated_false_distinct_from_not_stated_and_none() {
    let info = discover().await;
    assert_eq!(
        captured(&info, "plain"),
        None,
        "no annotations and no title captures None"
    );
    let annotations = captured(&info, "falsy").expect("annotations were advertised");

    assert_eq!(annotations.read_only, Hint::StatedFalse);
    assert_eq!(annotations.destructive, Hint::NotStated);
    assert_eq!(annotations.idempotent, Hint::NotStated);
    assert_eq!(annotations.open_world, Hint::NotStated);
}

#[tokio::test]
async fn discover_server_prefers_tool_title_over_annotation_title() {
    let info = discover().await;
    assert_eq!(
        title_of(&info, "top_title_wins").as_deref(),
        Some("Top Title")
    );
}

#[tokio::test]
async fn discover_server_falls_back_to_annotation_title() {
    let info = discover().await;
    assert_eq!(
        title_of(&info, "annotation_title_only").as_deref(),
        Some("Annotation Title")
    );
}

#[tokio::test]
async fn discover_server_captures_a_title_without_annotations() {
    let info = discover().await;
    let annotations = captured(&info, "title_only").expect("a title alone is captured");

    assert_eq!(
        annotations
            .title
            .as_ref()
            .map(mcp_execution_core::annotations::AnnotationTitle::as_str),
        Some("Only Title")
    );
    assert_eq!(annotations.read_only, Hint::NotStated);
    assert_eq!(annotations.destructive, Hint::NotStated);
}

#[tokio::test]
async fn discover_server_keeps_read_only_plus_destructive_as_advertised() {
    let info = discover().await;
    let annotations = captured(&info, "contradictory").expect("annotations were advertised");

    assert_eq!(annotations.read_only, Hint::StatedTrue);
    assert_eq!(annotations.destructive, Hint::StatedTrue);
}

#[tokio::test]
async fn discover_server_sanitizes_and_caps_the_title() {
    let info = discover().await;
    let title = title_of(&info, "hostile_title").expect("title captured");

    assert!(!title.contains('\u{1b}'));
    assert!(title.chars().count() <= MAX_UNTRUSTED_FIELD_LEN);
}
