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
//!
//! An optional third CLI arg gives a file path; if present, the fixture
//! writes its own OS process id to that file immediately on startup (before
//! the connect delay), letting tests confirm - via the written pid - that
//! the process was actually terminated after a timeout, rather than merely
//! asserting on the returned error.
//!
//! An optional fourth CLI arg gives another file path; if present, the
//! fixture writes a two-line report to it immediately on startup (before the
//! connect delay): the value of the [`ENV_MARKER_VAR`] environment variable
//! (empty if unset) on the first line, and the process's current working
//! directory on the second. This lets `tests/timeout_test.rs` assert
//! that `ServerConfig::env`/`ServerConfig::cwd` actually reach the spawned
//! child process, rather than only being covered at the config-object level.

use rmcp::model::{ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerInfo};
use rmcp::service::RequestContext;
use rmcp::transport::stdio;
use rmcp::{ErrorData as McpError, RoleServer, ServerHandler, ServiceExt};
use std::time::Duration;

/// Name of the environment variable the fixture echoes into the env/cwd
/// report file (see the fourth CLI arg above). Must match the constant of
/// the same name used by `tests/timeout_test.rs`.
const ENV_MARKER_VAR: &str = "MCP_EXECUTION_TEST_ENV_MARKER";

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

    if let Some(pid_file) = std::env::args().nth(3) {
        std::fs::write(pid_file, std::process::id().to_string()).expect("failed to write pid file");
    }

    if let Some(report_file) = std::env::args().nth(4) {
        let observed_env = std::env::var(ENV_MARKER_VAR).unwrap_or_default();
        let observed_cwd = std::env::current_dir().expect("failed to read current directory");
        std::fs::write(
            report_file,
            format!("{observed_env}\n{}", observed_cwd.display()),
        )
        .expect("failed to write env/cwd report file");
    }

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
