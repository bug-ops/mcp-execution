//! MCP server entry point for progressive loading generation.
//!
//! This binary provides an MCP server that helps generate progressive loading
//! TypeScript files for other MCP servers. Claude provides categorization
//! intelligence through natural language understanding.
//!
//! # Usage
//!
//! Run the server via stdio transport:
//!
//! ```bash
//! mcp-execution-server
//! ```
//!
//! Or configure in `~/.config/claude/mcp.json`:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "mcp-execution": {
//!       "command": "mcp-execution-server"
//!     }
//!   }
//! }
//! ```

use anyhow::Result;
use mcp_execution_server::service::GeneratorService;
use rmcp::ServiceExt;
use rmcp::transport::stdio;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr (stdout is for MCP protocol)
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,mcp_execution_server=debug")),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true),
        )
        .init();

    tracing::info!(
        "Starting mcp-execution-server v{}",
        env!("CARGO_PKG_VERSION")
    );

    // Create and run the service with stdio transport
    let service = GeneratorService::new().serve(stdio()).await?;
    service.waiting().await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}
