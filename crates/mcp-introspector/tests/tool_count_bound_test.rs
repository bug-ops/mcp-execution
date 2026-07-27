//! Integration tests proving `Introspector::discover_server`'s early-bailout pagination logic
//! (`list_tools_bounded`, issue #198 S4) actually bails mid-pagination against a real `rmcp`
//! transport, rather than only being covered indirectly through `build_server_info`'s
//! post-collection check (issue #198 N2 — that check is now unreachable on the real discovery
//! path, since `list_tools_bounded` bails before ever handing it an over-limit `Vec`).
//!
//! Reuses the in-process Streamable HTTP fixture pattern from `tests/http_transport_test.rs`,
//! with a `ServerHandler` that serves `tools/list` page by page instead of all at once.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`, which is how CI and the
//! project's preferred `cargo nextest` invocation run tests) so that
//! `rmcp/transport-streamable-http-server` is enabled.

#![cfg(feature = "test-fixtures")]

use axum::Router;
use mcp_execution_core::{Error, ResourceKind, ServerConfig, ServerId};
use mcp_execution_introspector::{Introspector, MAX_TOOL_COUNT};
use rmcp::model::{
    Implementation, InitializeResult, ListToolsResult, PaginatedRequestParams, ServerCapabilities,
    Tool,
};
use rmcp::service::RequestContext;
use rmcp::transport::streamable_http_server::{
    StreamableHttpServerConfig, StreamableHttpService, session::local::LocalSessionManager,
};
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;
use tokio_util::sync::CancellationToken;

/// Number of tools served per `list_tools` page by [`PaginatedFixtureHandler`]. Deliberately
/// smaller than [`MAX_TOOL_COUNT`] so a single discovery run spans multiple pages.
const PAGE_SIZE: usize = 300;

/// A fixture MCP server whose `tools/list` handler serves tools [`PAGE_SIZE`] at a time,
/// tracking how many pages were actually requested.
#[derive(Clone)]
struct PaginatedFixtureHandler {
    /// Number of `list_tools` calls served so far; incremented on every call.
    call_count: Arc<AtomicUsize>,
    /// Total number of tools this fixture will ever serve before signaling completion via
    /// `next_cursor: None`.
    ///
    /// `None` means the fixture never completes on its own — every page sets `next_cursor`
    /// regardless of how many have already been served — so a test using it proves the client
    /// bails out on its own initiative rather than merely reacting to the fixture running out
    /// of data. `list_tools_bounded` is expected to stop pulling pages as soon as its running
    /// total crosses `MAX_TOOL_COUNT`, which happens well before this fixture would ever
    /// signal completion.
    total_tools: Option<usize>,
}

impl ServerHandler for PaginatedFixtureHandler {
    fn get_info(&self) -> InitializeResult {
        InitializeResult::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("paginated-fixture-server", "1.0.0"))
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        let page_index = self.call_count.fetch_add(1, Ordering::SeqCst);
        let start = page_index * PAGE_SIZE;

        let (page_len, next_cursor) = self.total_tools.map_or_else(
            || (PAGE_SIZE, Some("more".to_string())),
            |total| {
                let remaining = total.saturating_sub(start);
                let page_len = remaining.min(PAGE_SIZE);
                let next_cursor = if start + page_len >= total {
                    None
                } else {
                    Some("more".to_string())
                };
                (page_len, next_cursor)
            },
        );

        let tools = (0..page_len)
            .map(|i| Tool::new(format!("tool{}", start + i), "d", serde_json::Map::new()))
            .collect();

        Ok(ListToolsResult {
            meta: None,
            next_cursor,
            tools,
        })
    }
}

/// Spawns [`PaginatedFixtureHandler`] on a loopback TCP port.
///
/// Returns the base URL to connect to, a `CancellationToken` the caller must cancel to shut
/// the server down, and a handle to the fixture's per-call counter.
async fn spawn_fixture_server(
    total_tools: Option<usize>,
) -> (String, CancellationToken, Arc<AtomicUsize>) {
    let ct = CancellationToken::new();
    let server_config =
        StreamableHttpServerConfig::default().with_cancellation_token(ct.child_token());

    let call_count = Arc::new(AtomicUsize::new(0));
    let handler = PaginatedFixtureHandler {
        call_count: call_count.clone(),
        total_tools,
    };
    let service: StreamableHttpService<PaginatedFixtureHandler, LocalSessionManager> =
        StreamableHttpService::new(
            move || Ok(handler.clone()),
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

    let shutdown_ct = ct.clone();
    tokio::spawn(async move {
        let _ = axum::serve(listener, router)
            .with_graceful_shutdown(async move { shutdown_ct.cancelled_owned().await })
            .await;
    });

    (format!("http://{addr}/mcp"), ct, call_count)
}

/// The fixture never signals completion (`total_tools: None`) — every page always carries a
/// `next_cursor` — so if `list_tools_bounded` did not bail out mid-pagination, this test would
/// either hang until `discover_timeout` fires or (if some later stage caught it) would have
/// fetched far more pages than the minimum needed to first cross `MAX_TOOL_COUNT`.
#[tokio::test]
async fn test_discover_server_bails_early_once_accumulated_tool_count_exceeds_max() {
    let (url, ct, call_count) = spawn_fixture_server(None).await;

    let mut introspector = Introspector::new();
    let config = ServerConfig::builder()
        .http_transport(url)
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let result = introspector
        .discover_server(ServerId::new("paginated-fixture-over").unwrap(), &config)
        .await;

    ct.cancel();

    let err = result.expect_err(
        "discover_server must reject once accumulated tool count exceeds MAX_TOOL_COUNT",
    );
    assert!(err.is_resource_limit_exceeded());

    // Smallest page count `k` such that `k * PAGE_SIZE > MAX_TOOL_COUNT` (integer division
    // floors, so `+ 1` lands on the first page that pushes the running total over the cap).
    let expected_pages = MAX_TOOL_COUNT / PAGE_SIZE + 1;
    assert_eq!(
        call_count.load(Ordering::SeqCst),
        expected_pages,
        "must bail on the page that first pushes the running total over MAX_TOOL_COUNT, not \
         keep draining the (fixture-side, effectively infinite) stream"
    );
}

/// Symmetric boundary case: exactly `MAX_TOOL_COUNT` tools spread across multiple pages must
/// be accepted in full, with pagination running to its natural completion.
#[tokio::test]
async fn test_discover_server_accepts_exactly_max_tool_count_across_pages() {
    let (url, ct, call_count) = spawn_fixture_server(Some(MAX_TOOL_COUNT)).await;

    let mut introspector = Introspector::new();
    let config = ServerConfig::builder()
        .http_transport(url)
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let result = introspector
        .discover_server(ServerId::new("paginated-fixture-exact").unwrap(), &config)
        .await;

    ct.cancel();

    let info = result.expect(
        "discover_server must accept exactly MAX_TOOL_COUNT tools spread across multiple pages",
    );
    assert_eq!(info.tools.len(), MAX_TOOL_COUNT);

    let expected_pages = MAX_TOOL_COUNT.div_ceil(PAGE_SIZE);
    assert_eq!(call_count.load(Ordering::SeqCst), expected_pages);
}

/// Absolute path to the `fixture-paginated-stdio-server` binary built
/// alongside this test target. Unlike [`PaginatedFixtureHandler`] above (HTTP
/// only, issue #226's rationale for why HTTP has no response-size bound), the
/// stdio path reads through `bounded_response_stream`, but pagination
/// early-bailout happens in `list_tools_bounded` before that bound is ever
/// relevant — this fixture proves the same early-bailout logic used by the
/// HTTP tests above also fires via stdio, closing the coverage gap noted in
/// issue #332.
const STDIO_FIXTURE_BIN: &str = env!("CARGO_BIN_EXE_fixture-paginated-stdio-server");

/// Stdio counterpart to
/// `test_discover_server_bails_early_once_accumulated_tool_count_exceeds_max`:
/// the fixture never signals pagination completion, so if
/// `list_tools_bounded` did not bail out mid-pagination over the stdio
/// transport, this test would hang until `discover_timeout` fires instead of
/// failing fast with `Error::ResourceLimitExceeded`.
#[tokio::test]
async fn test_discover_server_stdio_bails_early_once_accumulated_tool_count_exceeds_max() {
    let mut introspector = Introspector::new();
    let config = ServerConfig::builder()
        .command(STDIO_FIXTURE_BIN.to_string())
        .connect_timeout(Duration::from_secs(5))
        .discover_timeout(Duration::from_secs(5))
        .build()
        .unwrap();

    let result = introspector
        .discover_server(
            ServerId::new("paginated-stdio-fixture-over").unwrap(),
            &config,
        )
        .await;

    let err = result.expect_err(
        "discover_server must reject once accumulated tool count exceeds MAX_TOOL_COUNT",
    );

    // Smallest page count `k` such that `k * PAGE_SIZE > MAX_TOOL_COUNT` (see the HTTP sibling
    // test's identical calculation above) — the fixture's `PAGE_SIZE` matches this file's.
    let expected_pages = MAX_TOOL_COUNT / PAGE_SIZE + 1;
    let expected_actual = expected_pages * PAGE_SIZE;
    match err {
        Error::ResourceLimitExceeded {
            resource,
            actual,
            limit,
        } => {
            assert!(
                matches!(resource, ResourceKind::ToolCount { .. }),
                "expected a tool-count resource limit, got resource={resource:?}"
            );
            assert_eq!(
                actual, expected_actual,
                "must bail on the page that first pushes the running total over MAX_TOOL_COUNT"
            );
            assert_eq!(limit, MAX_TOOL_COUNT);
        }
        other => panic!("expected Error::ResourceLimitExceeded, got {other:?}"),
    }
}
