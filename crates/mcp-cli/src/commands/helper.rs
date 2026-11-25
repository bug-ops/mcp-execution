//! Hidden `helper`: serves the helper protocol on stdio for scripts run outside `run`.
//!
//! The generated bridge spawns this command (found through `MCP_EXECUTION_CLI` or `PATH`)
//! when no runner channel exists. It runs until stdin reaches end of file, then closes its
//! session pool.

use std::sync::Arc;

use anyhow::Result;
use mcp_execution_core::cli::ExitCode;
use mcp_execution_introspector::CallTimeout;

use super::common::refuse_inside_run;
use crate::runtime::helper::{CallHooks, DEFAULT_CALL_TIMEOUT, serve};
use crate::runtime::pool::SessionPool;

/// Serves the helper protocol on stdin and stdout until stdin closes.
///
/// # Errors
///
/// Returns an error when started inside `mcp-execution-cli run` (invalid input), or when the
/// hello is missing, late, malformed or of another protocol version.
pub async fn run() -> Result<ExitCode> {
    refuse_inside_run()?;
    let pool = Arc::new(SessionPool::new(CallTimeout::new(DEFAULT_CALL_TIMEOUT)?));
    let served = serve(
        tokio::io::stdin(),
        tokio::io::stdout(),
        Arc::clone(&pool),
        Arc::new(CallHooks::default()),
        None,
        std::future::pending(),
    )
    .await;
    pool.close().await;
    served.map(|()| ExitCode::SUCCESS)
}
