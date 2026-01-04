//! MCP Code Execution CLI.
#![allow(clippy::format_push_string)]
// NOTE(mvp): Many async functions are stubs prepared for future expansion.
// These will be implemented as features are added beyond Phase 8.
#![allow(clippy::unused_async)]
#![allow(clippy::cast_possible_truncation)]
// u128->u64 for millis is safe in practice
// TODO(phase-7.3): Add comprehensive error documentation to all public CLI functions
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::needless_collect)]
#![allow(clippy::unnecessary_wraps)] // API design requires Result for consistency across commands
#![allow(clippy::unnecessary_literal_unwrap)]
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
use clap::Parser;
use mcp_execution_core::cli::OutputFormat;

mod actions;
mod cli;
mod commands;
pub mod formatters;
mod runner;

use cli::Cli;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    runner::init_logging(cli.verbose)?;

    let output_format = cli
        .format
        .parse::<OutputFormat>()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    let exit_code = runner::execute_command(cli.command, output_format).await?;

    std::process::exit(exit_code.as_i32());
}
