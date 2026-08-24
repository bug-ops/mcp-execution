//! Test fixture: an MCP server over stdio whose `tools/list` handler serves
//! tools [`PAGE_SIZE`] at a time and never signals completion (every page
//! carries a `next_cursor`).
//!
//! Mirrors `PaginatedFixtureHandler` in `tests/tool_count_bound_test.rs`
//! (issue #198 S4), which proves `list_tools_bounded`'s early-bailout works
//! against a real `rmcp` transport over Streamable HTTP. This fixture closes
//! the same coverage gap for the stdio path (issue #332): without it,
//! `map_list_tools_bounded_error`'s `Error::ResourceLimitExceeded` mapping was
//! only ever exercised via HTTP.

use rmcp::model::{ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo, Tool};
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use std::sync::atomic::{AtomicUsize, Ordering};

/// Number of tools served per `list_tools` page. Deliberately smaller than
/// `MAX_TOOL_COUNT` so a single discovery run spans multiple pages.
const PAGE_SIZE: usize = 300;

#[derive(Default)]
struct PaginatedServer {
    /// Number of `list_tools` calls served so far; incremented on every call.
    call_count: AtomicUsize,
}

impl ServerHandler for PaginatedServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl std::future::Future<Output = Result<ListToolsResult, McpError>> {
        let page_index = self.call_count.fetch_add(1, Ordering::SeqCst);
        let start = page_index * PAGE_SIZE;

        let tools = (0..PAGE_SIZE)
            .map(|i| Tool::new(format!("tool{}", start + i), "d", serde_json::Map::new()))
            .collect();

        let mut result = ListToolsResult::with_all_items(tools);
        result.next_cursor = Some("more".to_string());
        std::future::ready(Ok(result))
    }
}

#[tokio::main]
async fn main() {
    let service = PaginatedServer::default()
        .serve(stdio())
        .await
        .expect("fixture server failed to start");

    let _ = service.waiting().await;
}
