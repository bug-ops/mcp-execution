//! MCP Code Execution CLI.
#![allow(clippy::format_push_string)]
#![allow(clippy::unused_async)] // MVP: Many functions are async stubs
#![allow(clippy::cast_possible_truncation)] // u128->u64 for millis is safe
#![allow(clippy::missing_errors_doc)] // MVP: Will add comprehensive docs in Phase 7.3
#![allow(clippy::needless_collect)]
#![allow(clippy::unnecessary_wraps)] // API design requires Result for consistency
#![allow(clippy::unnecessary_literal_unwrap)]
//!
//! Command-line interface for executing code in MCP sandbox,
//! inspecting servers, and generating virtual filesystems.
//!
//! # Architecture
//!
//! The CLI is organized around subcommands:
//! - `introspect` - Analyze MCP servers and display capabilities
//! - `generate` - Generate code from MCP server tools
//! - `execute` - Execute WASM modules in sandbox
//! - `server` - Manage MCP server connections
//! - `stats` - Display runtime statistics
//! - `debug` - Debug utilities and diagnostics
//! - `config` - Configuration management
//!
//! # Examples
//!
//! ```bash
//! # Introspect a server
//! mcp-cli introspect vkteams-bot
//!
//! # Generate code
//! mcp-cli generate vkteams-bot --output ./generated
//!
//! # Execute WASM module
//! mcp-cli execute module.wasm --entry main
//! ```

use anyhow::Result;
use clap::{Parser, Subcommand};
use mcp_core::cli::{ExitCode, OutputFormat};
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod actions;
mod commands;
pub mod formatters;

use actions::{ConfigAction, DebugAction, ServerAction};

/// MCP Code Execution - Secure WASM-based MCP tool execution.
///
/// This CLI provides secure execution of MCP tools in a WebAssembly sandbox,
/// achieving 90-98% token savings through progressive tool loading.
#[derive(Parser, Debug)]
#[command(name = "mcp-cli")]
#[command(version, about, long_about = None)]
#[command(author = "MCP Execution Team")]
struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    command: Commands,

    /// Enable verbose logging (debug level)
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Output format (json, text, pretty)
    #[arg(long = "format", global = true, default_value = "pretty")]
    format: String,
}

/// Available CLI subcommands.
#[derive(Subcommand, Debug)]
enum Commands {
    /// Introspect an MCP server and display its capabilities.
    ///
    /// Connects to an MCP server, discovers its tools, and displays
    /// detailed information about available capabilities.
    Introspect {
        /// Server connection string or command
        ///
        /// Can be a server name like "vkteams-bot" or a full command
        /// like "node server.js"
        server: String,

        /// Show detailed tool schemas
        #[arg(short, long)]
        detailed: bool,
    },

    /// Generate code from MCP server tools.
    ///
    /// Introspects a server and generates TypeScript or Rust code
    /// for tool execution, optionally compiling to WASM.
    Generate {
        /// Server connection string or command
        server: String,

        /// Output directory for generated code
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Code generation feature mode (wasm, skills)
        #[arg(short, long, default_value = "wasm")]
        feature: String,

        /// Overwrite existing output directory without prompting
        #[arg(short = 'F', long)]
        force: bool,

        /// Save generated code as a plugin
        #[arg(long)]
        save_plugin: bool,

        /// Plugin directory for save/load operations
        #[arg(long, default_value = "./plugins")]
        plugin_dir: PathBuf,
    },

    /// Execute a WASM module in the secure sandbox.
    ///
    /// Runs a WebAssembly module with security policies and resource limits.
    Execute {
        /// Path to WASM module file
        module: PathBuf,

        /// Entry point function name
        #[arg(short, long, default_value = "main")]
        entry: String,

        /// Memory limit in MB
        #[arg(short, long)]
        memory_limit: Option<u64>,

        /// Execution timeout in seconds
        #[arg(short, long)]
        timeout: Option<u64>,
    },

    /// Manage MCP server connections.
    ///
    /// List, validate, and manage configured MCP servers.
    Server {
        /// Server management action
        #[command(subcommand)]
        action: ServerAction,
    },

    /// Show runtime statistics.
    ///
    /// Display cache statistics, execution metrics, and performance data.
    Stats {
        /// Statistics category (cache, runtime, all)
        #[arg(default_value = "all")]
        category: String,
    },

    /// Debug utilities and diagnostics.
    ///
    /// Display system information, runtime metrics, and debugging data.
    Debug {
        /// Debug command
        #[command(subcommand)]
        action: DebugAction,
    },

    /// Configuration management.
    ///
    /// Initialize, view, and modify CLI configuration.
    Config {
        /// Configuration action
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Manage saved plugins.
    ///
    /// Save, load, list, and manage plugins stored on disk.
    Plugin {
        /// Plugin management action
        #[command(subcommand)]
        action: commands::plugin::PluginAction,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    // Parse CLI arguments
    let cli = Cli::parse();

    // Initialize logging
    init_logging(cli.verbose)?;

    // Parse output format
    let output_format = cli
        .format
        .parse::<OutputFormat>()
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // Execute command and get exit code
    let exit_code = execute_command(cli.command, output_format).await?;

    // Exit with appropriate code
    std::process::exit(exit_code.as_i32());
}

/// Initializes logging infrastructure.
///
/// Sets up tracing with appropriate log levels based on verbosity flag.
///
/// # Errors
///
/// Returns an error if logging initialization fails.
fn init_logging(verbose: bool) -> Result<()> {
    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    Ok(())
}

/// Executes the specified CLI command.
///
/// Routes commands to their respective handlers and returns an exit code.
///
/// # Errors
///
/// Returns an error if command execution fails.
async fn execute_command(command: Commands, output_format: OutputFormat) -> Result<ExitCode> {
    match command {
        Commands::Introspect { server, detailed } => {
            commands::introspect::run(server, detailed, output_format).await
        }
        Commands::Generate {
            server,
            output,
            feature,
            force,
            save_plugin,
            plugin_dir,
        } => {
            commands::generate::run(
                server,
                output,
                feature,
                force,
                save_plugin,
                plugin_dir,
                output_format,
            )
            .await
        }
        Commands::Execute {
            module,
            entry,
            memory_limit,
            timeout,
        } => commands::execute::run(module, entry, memory_limit, timeout, output_format).await,
        Commands::Server { action } => commands::server::run(action, output_format).await,
        Commands::Stats { category } => commands::stats::run(category, output_format).await,
        Commands::Debug { action } => commands::debug::run(action, output_format).await,
        Commands::Config { action } => commands::config::run(action, output_format).await,
        Commands::Plugin { action } => commands::plugin::run(action, output_format).await,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_introspect() {
        let cli = Cli::parse_from(["mcp-cli", "introspect", "vkteams-bot"]);
        assert!(matches!(cli.command, Commands::Introspect { .. }));
    }

    #[test]
    fn test_cli_parsing_generate() {
        let cli = Cli::parse_from(["mcp-cli", "generate", "server"]);
        assert!(matches!(cli.command, Commands::Generate { .. }));

        // Test with output directory
        let cli = Cli::parse_from(["mcp-cli", "generate", "server", "--output", "/tmp"]);
        if let Commands::Generate { output, .. } = cli.command {
            assert_eq!(output, Some(PathBuf::from("/tmp")));
        } else {
            panic!("Expected Generate command");
        }

        // Test with force flag
        let cli = Cli::parse_from(["mcp-cli", "generate", "server", "--force"]);
        if let Commands::Generate { force, .. } = cli.command {
            assert!(force);
        } else {
            panic!("Expected Generate command");
        }
    }

    #[test]
    fn test_cli_parsing_execute() {
        let cli = Cli::parse_from(["mcp-cli", "execute", "module.wasm"]);
        assert!(matches!(cli.command, Commands::Execute { .. }));
    }

    #[test]
    fn test_cli_parsing_server_list() {
        let cli = Cli::parse_from(["mcp-cli", "server", "list"]);
        assert!(matches!(cli.command, Commands::Server { .. }));
    }

    #[test]
    fn test_cli_parsing_stats() {
        let cli = Cli::parse_from(["mcp-cli", "stats"]);
        assert!(matches!(cli.command, Commands::Stats { .. }));
    }

    #[test]
    fn test_cli_parsing_debug_info() {
        let cli = Cli::parse_from(["mcp-cli", "debug", "info"]);
        assert!(matches!(cli.command, Commands::Debug { .. }));
    }

    #[test]
    fn test_cli_parsing_config_init() {
        let cli = Cli::parse_from(["mcp-cli", "config", "init"]);
        assert!(matches!(cli.command, Commands::Config { .. }));
    }

    #[test]
    fn test_cli_verbose_flag() {
        let cli = Cli::parse_from(["mcp-cli", "--verbose", "stats"]);
        assert!(cli.verbose);
    }

    #[test]
    fn test_cli_output_format_default() {
        let cli = Cli::parse_from(["mcp-cli", "stats"]);
        assert_eq!(cli.format, "pretty");
    }

    #[test]
    fn test_cli_output_format_custom() {
        let cli = Cli::parse_from(["mcp-cli", "--format", "json", "stats"]);
        assert_eq!(cli.format, "json");
    }

    #[test]
    fn test_output_format_parsing_valid() {
        let format: OutputFormat = "json".parse().unwrap();
        assert_eq!(format, OutputFormat::Json);

        let format: OutputFormat = "text".parse().unwrap();
        assert_eq!(format, OutputFormat::Text);

        let format: OutputFormat = "pretty".parse().unwrap();
        assert_eq!(format, OutputFormat::Pretty);
    }

    #[test]
    fn test_output_format_parsing_invalid() {
        assert!("invalid".parse::<OutputFormat>().is_err());
    }
}
