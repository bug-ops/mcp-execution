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
use clap::{Parser, Subcommand};
use clap_complete::Shell;
use mcp_core::cli::{ExitCode, OutputFormat};
use std::path::PathBuf;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

mod actions;
mod commands;
pub mod formatters;

use actions::ServerAction;

/// MCP Code Execution - Secure WASM-based MCP tool execution.
///
/// This CLI provides secure execution of MCP tools in a WebAssembly sandbox,
/// achieving 90-98% token savings through progressive tool loading.
#[derive(Parser, Debug)]
#[command(name = "mcp-cli")]
#[command(version, about, long_about = None)]
#[command(author = "MCP Execution Team")]
pub struct Cli {
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
pub enum Commands {
    /// Introspect an MCP server and display its capabilities.
    ///
    /// Connects to an MCP server, discovers its tools, and displays
    /// detailed information about available capabilities.
    ///
    /// # Examples
    ///
    /// ```bash
    /// # Simple binary
    /// mcp-cli introspect github-mcp-server
    ///
    /// # With arguments
    /// mcp-cli introspect github-mcp-server --arg=stdio
    ///
    /// # Docker container
    /// mcp-cli introspect docker --arg=run --arg=-i --arg=--rm \
    ///     --arg=ghcr.io/github/github-mcp-server \
    ///     --env=GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx
    ///
    /// # HTTP transport
    /// mcp-cli introspect --http https://api.githubcopilot.com/mcp/ \
    ///     --header "Authorization=Bearer ghp_xxx"
    /// ```
    Introspect {
        /// Server command (binary name or path)
        ///
        /// For stdio transport: command to execute (e.g., "docker", "npx", "github-mcp-server")
        /// Not required when using --http or --sse
        #[arg(required_unless_present_any = ["http", "sse"])]
        server: Option<String>,

        /// Arguments to pass to the server command
        #[arg(short, long = "arg", num_args = 1)]
        args: Vec<String>,

        /// Environment variables in KEY=VALUE format
        #[arg(short, long = "env", num_args = 1)]
        env: Vec<String>,

        /// Working directory for the server process
        #[arg(long)]
        cwd: Option<String>,

        /// Use HTTP transport with specified URL
        #[arg(long, conflicts_with = "sse")]
        http: Option<String>,

        /// Use SSE transport with specified URL
        #[arg(long, conflicts_with = "http")]
        sse: Option<String>,

        /// HTTP headers in KEY=VALUE format (for HTTP/SSE transport)
        #[arg(long = "header", num_args = 1)]
        headers: Vec<String>,

        /// Show detailed tool schemas
        #[arg(short, long)]
        detailed: bool,
    },

    /// Generate progressive loading code from MCP server.
    ///
    /// Introspects an MCP server and generates TypeScript files
    /// for progressive tool loading.
    ///
    /// # Configuration Modes
    ///
    /// 1. Load from ~/.claude/mcp.json (recommended):
    ///    ```bash
    ///    mcp-cli generate --from-config github
    ///    ```
    ///
    /// 2. Manual configuration:
    ///    ```bash
    ///    mcp-cli generate docker --arg=run --arg=-i --arg=--rm \
    ///        --arg=ghcr.io/github/github-mcp-server \
    ///        --env=GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx \
    ///        --name=github
    ///    ```
    ///
    /// # Examples
    ///
    /// ```bash
    /// # Load GitHub server config from mcp.json
    /// mcp-cli generate --from-config github
    ///
    /// # Manual Docker container
    /// mcp-cli generate docker --arg=run --arg=-i --arg=--rm \
    ///     --arg=-e --arg=GITHUB_PERSONAL_ACCESS_TOKEN \
    ///     --arg=ghcr.io/github/github-mcp-server \
    ///     --env=GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx
    /// ```
    Generate {
        /// Load server configuration from ~/.claude/mcp.json by name
        ///
        /// When specified, all other server configuration options are ignored.
        /// The server must be defined in ~/.claude/mcp.json with matching name.
        ///
        /// Example mcp.json:
        /// ```json
        /// {
        ///   "mcpServers": {
        ///     "github": {
        ///       "command": "docker",
        ///       "args": ["run", "-i", "--rm", "..."],
        ///       "env": {"GITHUB_PERSONAL_ACCESS_TOKEN": "..."}
        ///     }
        ///   }
        /// }
        /// ```
        #[arg(long = "from-config", conflicts_with_all = ["server", "server_args", "server_env", "server_cwd", "http_url", "sse_url"])]
        from_config: Option<String>,

        /// Server command (binary name or path)
        ///
        /// For stdio transport: command to execute (e.g., "docker", "npx", "github-mcp-server")
        /// Not required when using --from-config, --http, or --sse
        #[arg(required_unless_present_any = ["from_config", "http_url", "sse_url"])]
        server: Option<String>,

        /// Arguments to pass to the server command
        #[arg(long = "arg", num_args = 1)]
        server_args: Vec<String>,

        /// Environment variables in KEY=VALUE format
        #[arg(long = "env", num_args = 1)]
        server_env: Vec<String>,

        /// Working directory for the server process
        #[arg(long = "cwd")]
        server_cwd: Option<String>,

        /// Use HTTP transport with specified URL
        #[arg(long = "http", conflicts_with = "sse_url")]
        http_url: Option<String>,

        /// Use SSE transport with specified URL
        #[arg(long = "sse", conflicts_with = "http_url")]
        sse_url: Option<String>,

        /// HTTP headers in KEY=VALUE format (for HTTP/SSE transport)
        #[arg(long = "header", num_args = 1)]
        server_headers: Vec<String>,

        /// Custom server name for directory (e.g., 'github' instead of 'docker')
        /// (default: uses server command name)
        #[arg(long)]
        name: Option<String>,

        /// Custom output directory for progressive loading files
        /// (default: ~/.claude/servers/)
        #[arg(long)]
        progressive_output: Option<PathBuf>,
    },

    /// Manage MCP server connections.
    ///
    /// List, validate, and manage configured MCP servers.
    Server {
        /// Server management action
        #[command(subcommand)]
        action: ServerAction,
    },

    /// Validate runtime environment for MCP tool execution.
    ///
    /// Checks that the system is ready to execute generated MCP tools:
    /// - Verifies Node.js 18+ is installed
    /// - Checks MCP configuration exists
    /// - Makes TypeScript files executable (Unix only)
    ///
    /// # Examples
    ///
    /// ```bash
    /// # Validate environment
    /// mcp-execution-cli setup
    ///
    /// # Output:
    /// # ✓ Node.js v20.10.0 detected
    /// # ✓ MCP configuration found
    /// # ✓ Runtime setup complete
    /// ```
    Setup,

    /// Generate shell completions.
    ///
    /// Generates completion scripts for various shells that can be
    /// sourced or saved to enable tab completion for this CLI.
    Completions {
        /// Target shell for completion generation
        #[arg(value_enum)]
        shell: Shell,
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
        Commands::Introspect {
            server,
            args,
            env,
            cwd,
            http,
            sse,
            headers,
            detailed,
        } => {
            commands::introspect::run(
                server,
                args,
                env,
                cwd,
                http,
                sse,
                headers,
                detailed,
                output_format,
            )
            .await
        }
        Commands::Generate {
            from_config,
            server,
            server_args,
            server_env,
            server_cwd,
            http_url,
            sse_url,
            server_headers,
            name,
            progressive_output,
        } => {
            commands::generate::run(
                from_config,
                server,
                server_args,
                server_env,
                server_cwd,
                http_url,
                sse_url,
                server_headers,
                name,
                progressive_output,
                output_format,
            )
            .await
        }
        Commands::Server { action } => commands::server::run(action, output_format).await,
        Commands::Setup => commands::setup::run().await,
        Commands::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            commands::completions::run(shell, &mut cmd).await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cli_parsing_introspect() {
        let cli = Cli::parse_from(["mcp-cli", "introspect", "github"]);
        assert!(matches!(cli.command, Commands::Introspect { .. }));
    }

    #[test]
    fn test_cli_parsing_introspect_with_args() {
        // Use --arg=VALUE format for arguments that start with -
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "docker",
            "--arg=run",
            "--arg=-i",
            "--arg=--rm",
            "--arg=ghcr.io/github/github-mcp-server",
            "--env=GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx",
        ]);
        if let Commands::Introspect {
            server, args, env, ..
        } = cli.command
        {
            assert_eq!(server, Some("docker".to_string()));
            assert_eq!(
                args,
                vec!["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"]
            );
            assert_eq!(env, vec!["GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx"]);
        } else {
            panic!("Expected Introspect command");
        }
    }

    #[test]
    fn test_cli_parsing_introspect_http() {
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "--http",
            "https://api.githubcopilot.com/mcp/",
            "--header",
            "Authorization=Bearer token",
        ]);
        if let Commands::Introspect {
            server,
            http,
            headers,
            ..
        } = cli.command
        {
            assert_eq!(server, None);
            assert_eq!(http, Some("https://api.githubcopilot.com/mcp/".to_string()));
            assert_eq!(headers, vec!["Authorization=Bearer token"]);
        } else {
            panic!("Expected Introspect command");
        }
    }

    #[test]
    fn test_cli_parsing_generate() {
        let cli = Cli::parse_from(["mcp-cli", "generate", "server"]);
        assert!(matches!(cli.command, Commands::Generate { .. }));

        // Test with progressive output
        let cli = Cli::parse_from([
            "mcp-cli",
            "generate",
            "server",
            "--progressive-output",
            "/tmp/output",
        ]);
        if let Commands::Generate {
            progressive_output, ..
        } = cli.command
        {
            assert_eq!(progressive_output, Some(PathBuf::from("/tmp/output")));
        } else {
            panic!("Expected Generate command");
        }
    }

    #[test]
    fn test_cli_parsing_server_list() {
        let cli = Cli::parse_from(["mcp-cli", "server", "list"]);
        assert!(matches!(cli.command, Commands::Server { .. }));
    }

    #[test]
    fn test_cli_verbose_flag() {
        let cli = Cli::parse_from(["mcp-cli", "--verbose", "introspect", "github"]);
        assert!(cli.verbose);
    }

    #[test]
    fn test_cli_output_format_default() {
        let cli = Cli::parse_from(["mcp-cli", "introspect", "github"]);
        assert_eq!(cli.format, "pretty");
    }

    #[test]
    fn test_cli_output_format_custom() {
        let cli = Cli::parse_from(["mcp-cli", "--format", "json", "introspect", "github"]);
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

    #[test]
    fn test_cli_parsing_completions_bash() {
        let cli = Cli::parse_from(["mcp-cli", "completions", "bash"]);
        assert!(matches!(cli.command, Commands::Completions { .. }));
    }

    #[test]
    fn test_cli_parsing_completions_zsh() {
        let cli = Cli::parse_from(["mcp-cli", "completions", "zsh"]);
        if let Commands::Completions { shell } = cli.command {
            assert_eq!(shell, Shell::Zsh);
        } else {
            panic!("Expected Completions command");
        }
    }
}
