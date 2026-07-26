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
use clap::Parser;
use mcp_execution_cli::cli::Cli;
use mcp_execution_cli::runner;
use mcp_execution_core::cli::OutputFormat;

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();

    runner::init_logging(cli.verbose)?;

    // `OutputFormat::from_str`'s `Err` is a `mcp_execution_core::Error`
    // (e.g. `--format xml`) — routed through the same
    // `report_and_classify` used for command-handler failures so an invalid
    // CLI argument here also exits `ExitCode::INVALID_INPUT`, not a bare 1.
    let exit_code = match cli.format.parse::<OutputFormat>() {
        Ok(output_format) => runner::execute_command(cli.command, output_format).await?,
        Err(err) => runner::report_and_classify(&anyhow::Error::from(err)),
    };

    std::process::exit(exit_code.as_i32());
}
