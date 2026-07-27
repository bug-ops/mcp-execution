//! Integration tests proving `Introspector::discover_server` works end-to-end
//! over Streamable HTTP (issue #180): connects to a real in-process rmcp
//! Streamable HTTP server, discovers its tools and handshake metadata, proves
//! `Transport::Sse` uses the same client path as `Transport::Http`,
//! confirms a custom header set via `ServerConfig::header` actually reaches
//! the server on the wire, and exercises the connect/discover timeout paths
//! the same way `tests/timeout_test.rs` does for stdio.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`, which
//! is how CI and the project's preferred `cargo nextest` invocation run
//! tests) so that `rmcp/transport-streamable-http-server` is enabled.

#![cfg(feature = "test-fixtures")]

use axum::Router;
use axum::extract::{Request, State};
use axum::middleware::{self, Next};
use axum::response::Response;
use mcp_execution_core::{Error, ServerConfig, ServerId};
use mcp_execution_introspector::Introspector;
use rmcp::model::{
    Implementation, InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

/// Header the fixture server watches for, to prove custom headers configured
/// via `ServerConfig::header` reach the server over the wire.
const TEST_HEADER_NAME: &str = "x-test-header";

/// Minimal MCP server exposing a single `echo` tool, used to exercise the
/// HTTP/SSE client path end-to-end. `list_tools_delay` lets discover-timeout
/// tests hang the `tools/list` response independently of the connect phase.
#[derive(Clone)]
struct FixtureHandler {
    list_tools_delay: Duration,
}

impl ServerHandler for FixtureHandler {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("fixture-http-server", "9.9.9"))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        if !self.list_tools_delay.is_zero() {
            tokio::time::sleep(self.list_tools_delay).await;
        }
        Ok(ListToolsResult::with_all_items(vec![Tool::new(
            "echo",
            "Echoes input back",
            serde_json::Map::new(),
        )]))
    }
}

/// Shared state for the header-capturing, optionally-delaying middleware
/// wrapped around the fixture's Streamable HTTP service.
#[derive(Clone)]
struct MiddlewareState {
    /// Delay applied to every request, simulating a hung connect phase.
    connect_delay: Duration,
    /// Records the last observed value of [`TEST_HEADER_NAME`].
    captured_header: Arc<Mutex<Option<String>>>,
}

async fn instrument_request(
    State(state): State<MiddlewareState>,
    req: Request,
    next: Next,
) -> Response {
    if let Some(value) = req.headers().get(TEST_HEADER_NAME) {
        *state.captured_header.lock().unwrap() =
            Some(value.to_str().unwrap_or_default().to_string());
    }
    if !state.connect_delay.is_zero() {
        tokio::time::sleep(state.connect_delay).await;
    }
    next.run(req).await
}

/// Spawns the fixture server on a loopback TCP port.
///
/// Returns the base URL to connect to (`http://127.0.0.1:PORT/mcp`), a
/// `CancellationToken` the caller must cancel to shut the server down, and
/// the header-capture handle described in [`MiddlewareState`].
async fn spawn_fixture_server(
    connect_delay: Duration,
    list_tools_delay: Duration,
) -> (String, CancellationToken, Arc<Mutex<Option<String>>>) {
    let ct = CancellationToken::new();
    let server_config =
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token());

    let handler = FixtureHandler { list_tools_delay };
    let service: StreamableHttpService<FixtureHandler, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(handler.clone()),
            Arc::new(LocalSessionManager::default()),
            server_config,
        );

    let captured_header = Arc::new(Mutex::new(None));
    let middleware_state = MiddlewareState {
        connect_delay,
        captured_header: captured_header.clone(),
    };

    let router: Router =
        Router::new()
            .nest_service("/mcp", service)
            .layer(middleware::from_fn_with_state(
                middleware_state,
                instrument_request,
            ));

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind fixture listener");
    let addr = listener
        .local_addr()
        .expect("fixture listener has a local addr");

    let shutdown_ct = ct.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown_ct.cancelled_owned().await })
            .await;
    });

    (format!("http://{addr}/mcp"), ct, captured_header)
}

#[tokio::test]
async fn test_discover_server_http_lists_tools_and_metadata() {
    let (url, ct, captured_header) = spawn_fixture_server(Duration::ZERO, Duration::ZERO).await;

    let mut introspector = Introspector::new();
    let config = ServerConfig::builder()
        .http_transport(url)
        .header(TEST_HEADER_NAME.to_string(), "propagated-value".to_string())
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let result = introspector
        .discover_server(ServerId::new("http-fixture").unwrap(), &config)
        .await;

    ct.cancel();

    let info = result.expect("discover_server should succeed against the HTTP fixture");
    assert_eq!(info.name, "fixture-http-server");
    assert_eq!(info.version, "9.9.9");
    assert_eq!(info.tools.len(), 1);
    assert_eq!(info.tools[0].name.as_str(), "echo");
    assert!(info.capabilities.supports_tools);

    assert_eq!(
        captured_header.lock().unwrap().as_deref(),
        Some("propagated-value"),
        "the custom header configured via ServerConfig::header must reach the server"
    );
}

/// rmcp 2.2 has a single client transport for both `Http` and `Sse`
/// (`Transport::Sse` is documented as an alias) — this proves that
/// end-to-end against a real server, not just at the type level.
#[tokio::test]
async fn test_discover_server_sse_transport_also_works() {
    let (url, ct, _captured_header) = spawn_fixture_server(Duration::ZERO, Duration::ZERO).await;

    let mut introspector = Introspector::new();
    let config = ServerConfig::builder()
        .sse_transport(url)
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let result = introspector
        .discover_server(ServerId::new("sse-fixture").unwrap(), &config)
        .await;

    ct.cancel();

    let info =
        result.expect("discover_server should succeed against the HTTP fixture over Sse transport");
    assert_eq!(info.tools.len(), 1);
    assert_eq!(info.tools[0].name.as_str(), "echo");
}

/// The fixture delays `tools/list`, so with a short `discover_timeout`
/// `discover_server` must fail with `Error::Timeout { operation: "list_all_tools" }`
/// — mirroring `test_discover_server_discover_timeout_fires` in `timeout_test.rs`
/// for the stdio path.
#[tokio::test]
async fn test_discover_server_http_discover_timeout_fires() {
    let (url, ct, _captured_header) =
        spawn_fixture_server(Duration::ZERO, Duration::from_secs(30)).await;

    let mut introspector = Introspector::new();
    let config = ServerConfig::builder()
        .http_transport(url)
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_millis(150))
        .build()
        .unwrap();

    let started = Instant::now();
    let result = introspector
        .discover_server(ServerId::new("http-discover-timeout").unwrap(), &config)
        .await;
    let elapsed = started.elapsed();

    ct.cancel();

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
        "timeout should fire near the configured 150ms bound, not wait for the fixture's 30s delay; took {elapsed:?}"
    );
}

/// The fixture delays every response (including the initial handshake), so
/// with a short `connect_timeout` `discover_server` must fail with
/// `Error::Timeout { operation: "connect" }` — mirroring
/// `test_discover_server_connect_timeout_fires` in `timeout_test.rs` for the
/// stdio path.
#[tokio::test]
async fn test_discover_server_http_connect_timeout_fires() {
    let (url, ct, _captured_header) =
        spawn_fixture_server(Duration::from_secs(30), Duration::ZERO).await;

    let mut introspector = Introspector::new();
    let config = ServerConfig::builder()
        .http_transport(url)
        .connect_timeout(Duration::from_millis(150))
        .build()
        .unwrap();

    let started = Instant::now();
    let result = introspector
        .discover_server(ServerId::new("http-connect-timeout").unwrap(), &config)
        .await;
    let elapsed = started.elapsed();

    ct.cancel();

    match result {
        Err(Error::Timeout { operation, .. }) => assert!(
            operation.starts_with("connect"),
            "expected a \"connect\" timeout, got operation={operation:?}"
        ),
        other => panic!("expected Error::Timeout {{ operation: \"connect\" }}, got {other:?}"),
    }
    assert!(
        elapsed < Duration::from_secs(5),
        "timeout should fire near the configured 150ms bound, not wait for the fixture's 30s delay; took {elapsed:?}"
    );
}

/// rmcp rejects a handful of reserved headers (`Accept`, `Mcp-Session-Id`,
/// `Last-Event-Id`) that its own transport already sets — `Authorization` is
/// not among them, so the documented PAT use case is unaffected. This proves
/// the resulting error is a legible message naming the header, not a raw
/// Debug dump, by reaching through `Error::ConnectionFailed`'s `source`.
///
/// No fixture server needed: rmcp validates custom headers while building the
/// request, before any socket I/O, so this is deterministic against an
/// address nothing listens on.
#[tokio::test]
async fn test_discover_server_http_reserved_header_collision_is_legible() {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind unused-port listener");
    let addr = listener.local_addr().expect("listener has a local addr");
    drop(listener); // nothing listens at `addr` from this point on

    let mut introspector = Introspector::new();
    let config = ServerConfig::builder()
        .http_transport(format!("http://{addr}/mcp"))
        .header("Accept".to_string(), "text/html".to_string())
        .connect_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let result = introspector
        .discover_server(ServerId::new("reserved-header-fixture").unwrap(), &config)
        .await;

    match result {
        Err(Error::ConnectionFailed { source, .. }) => {
            let msg = source.to_string();
            assert!(
                msg.to_ascii_lowercase().contains("reserved")
                    && msg.to_ascii_lowercase().contains("accept"),
                "expected a legible reserved-header-conflict message naming the header, got: {msg}"
            );
        }
        other => panic!("expected Error::ConnectionFailed, got {other:?}"),
    }
}
