//! Test fixture: a minimal MCP server with two independently controllable
//! delays - one before the connect handshake even starts, one before
//! answering `tools/list`.
//!
//! Used by `tests/timeout_test.rs` to prove that `Introspector::discover_server`
//! enforces both `connect_timeout` and `discover_timeout`:
//! - The connect-timeout test sets a long handshake delay (first CLI arg,
//!   milliseconds) so the rmcp `serve` handshake itself blocks, the same way
//!   a hung/malicious downstream server would - without depending on the
//!   platform-specific `sleep` command (unavailable on Windows CI runners).
//! - The discover-timeout test sets a long `tools/list` delay (second CLI
//!   arg, milliseconds) so the handshake completes instantly but discovery
//!   hangs.
//!
//! Both delays default to 0 (respond immediately) so the fixture can also
//! serve as a fast, successful server for sanity-check tests.

use rmcp::model::{ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo};
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use std::time::Duration;

struct SlowServer {
    list_tools_delay: Duration,
}

impl ServerHandler for SlowServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().build())
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        tokio::time::sleep(self.list_tools_delay).await;
        Ok(ListToolsResult::default())
    }
}

fn arg_millis(index: usize) -> u64 {
    std::env::args()
        .nth(index)
        .and_then(|arg| arg.parse().ok())
        .unwrap_or(0)
}

#[tokio::main]
async fn main() {
    let connect_delay = Duration::from_millis(arg_millis(1));
    let list_tools_delay = Duration::from_millis(arg_millis(2));

    // Delay before even starting the handshake, so the client's `serve()`
    // call blocks waiting for a response - simulating a hung connect phase
    // without relying on a platform-specific `sleep` binary.
    tokio::time::sleep(connect_delay).await;

    let service = SlowServer { list_tools_delay }
        .serve(stdio())
        .await
        .expect("fixture server failed to start");

    let _ = service.waiting().await;
}
