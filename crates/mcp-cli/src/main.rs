//! MCP Code Execution CLI binary entry point.
//!
//! Command-line interface for executing code in MCP sandbox,
//! inspecting servers, and generating virtual filesystems.
//!
//! # Architecture
//!
//! The CLI is organized around subcommands:
//! - `introspect` - Analyze MCP servers and display capabilities
//! - `generate` - Generate progressive loading TypeScript files
//! - `server` - Manage MCP server connections
//! - `completions` - Generate shell completions
//!
//! # Examples
//!
//! ```bash
//! # Introspect a server
//! mcp-execution-cli introspect github-mcp-server
//!
//! # Generate progressive loading files
//! mcp-execution-cli generate github-mcp-server --env GITHUB_TOKEN=ghp_xxx
//! ```

use anyhow::Result;
use clap::Parser as _;
use mcp_execution_cli::{Cli, run};

#[tokio::main]
async fn main() -> Result<()> {
    // `--format` is typed as `OutputFormat` directly (a `PossibleValuesParser`-based value
    // parser, see `cli.rs`), so an invalid value (e.g. `--format xml`) is already rejected by
    // clap itself before this point.
    let exit_code = run(Cli::parse()).await?;

    std::process::exit(exit_code.as_i32());
}
