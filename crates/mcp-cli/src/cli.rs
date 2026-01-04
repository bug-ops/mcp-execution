//! CLI argument definitions and parsing.
//!
//! Defines the command-line interface structure using clap:
//! - `Cli` - Main CLI entry point
//! - `Commands` - Available subcommands

use clap::{Parser, Subcommand};
use clap_complete::Shell;
use std::path::PathBuf;

use crate::actions::ServerAction;

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
    pub command: Commands,

    /// Enable verbose logging (debug level)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output format (json, text, pretty)
    #[arg(long = "format", global = true, default_value = "pretty")]
    pub format: String,
}

/// Available CLI subcommands.
#[derive(Subcommand, Debug)]
pub enum Commands {
    /// Introspect an MCP server and display its capabilities.
    ///
    /// Connects to an MCP server, discovers its tools, and displays
    /// detailed information about available capabilities.
    ///
    /// # Configuration Modes
    ///
    /// 1. Load from ~/.claude/mcp.json (recommended):
    ///    ```bash
    ///    mcp-cli introspect --from-config github
    ///    ```
    ///
    /// 2. Manual configuration:
    ///    ```bash
    ///    mcp-cli introspect github-mcp-server --arg=stdio
    ///    ```
    ///
    /// # Examples
    ///
    /// ```bash
    /// # Load GitHub server config from mcp.json
    /// mcp-cli introspect --from-config github
    ///
    /// # Load with detailed schemas
    /// mcp-cli introspect --from-config github --detailed
    ///
    /// # Manual: Simple binary
    /// mcp-cli introspect github-mcp-server
    ///
    /// # Manual: With arguments
    /// mcp-cli introspect github-mcp-server --arg=stdio
    ///
    /// # Manual: Docker container
    /// mcp-cli introspect docker --arg=run --arg=-i --arg=--rm \
    ///     --arg=ghcr.io/github/github-mcp-server \
    ///     --env=GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx
    ///
    /// # HTTP transport
    /// mcp-cli introspect --http https://api.githubcopilot.com/mcp/ \
    ///     --header "Authorization=Bearer ghp_xxx"
    /// ```
    Introspect {
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
        #[arg(long = "from-config", conflicts_with_all = ["server", "args", "env", "cwd", "http", "sse"])]
        from_config: Option<String>,

        /// Server command (binary name or path)
        ///
        /// For stdio transport: command to execute (e.g., "docker", "npx", "github-mcp-server")
        /// Not required when using --http or --sse
        #[arg(required_unless_present_any = ["from_config", "http", "sse"])]
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

    /// Generate Claude Code skill file from progressive loading tools.
    ///
    /// Scans generated progressive loading TypeScript files and creates
    /// an instruction skill (SKILL.md) for Claude Code integration.
    ///
    /// # Note
    ///
    /// For optimal results, prefer using the MCP server (`mcp-server`) for skill generation.
    /// The MCP server can leverage LLM capabilities to summarize tool descriptions and reduce
    /// context size, resulting in more concise and effective skill files.
    ///
    /// # Examples
    ///
    /// ```bash
    /// # Generate skill for GitHub server
    /// mcp-cli skill --server github
    ///
    /// # With custom output path
    /// mcp-cli skill --server github --output ~/.claude/skills/github/SKILL.md
    ///
    /// # With use case hints
    /// mcp-cli skill --server github \
    ///     --hint "managing pull requests" \
    ///     --hint "reviewing code changes"
    ///
    /// # Overwrite existing skill
    /// mcp-cli skill --server github --overwrite
    /// ```
    Skill {
        /// Server identifier (e.g., "github")
        ///
        /// Must match a directory in `servers_dir` containing generated TypeScript files.
        #[arg(short, long)]
        server: String,

        /// Base directory for generated servers
        ///
        /// Default: ~/.claude/servers
        #[arg(long)]
        servers_dir: Option<PathBuf>,

        /// Custom output path for SKILL.md file
        ///
        /// Default: ~/.claude/skills/{server}/SKILL.md
        #[arg(short, long)]
        output: Option<PathBuf>,

        /// Custom skill name
        ///
        /// Default: {server}-progressive
        #[arg(long)]
        skill_name: Option<String>,

        /// Use case hints for skill generation
        ///
        /// Multiple hints can be provided to generate more relevant documentation.
        /// Examples: "managing pull requests", "code review", "CI/CD automation"
        #[arg(long = "hint", num_args = 1)]
        hints: Vec<String>,

        /// Overwrite existing SKILL.md file
        #[arg(long)]
        overwrite: bool,
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
        use mcp_execution_core::cli::OutputFormat;

        let format: OutputFormat = "json".parse().unwrap();
        assert_eq!(format, OutputFormat::Json);

        let format: OutputFormat = "text".parse().unwrap();
        assert_eq!(format, OutputFormat::Text);

        let format: OutputFormat = "pretty".parse().unwrap();
        assert_eq!(format, OutputFormat::Pretty);
    }

    #[test]
    fn test_output_format_parsing_invalid() {
        use mcp_execution_core::cli::OutputFormat;
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

    #[test]
    fn test_cli_parsing_skill_basic() {
        let cli = Cli::parse_from(["mcp-cli", "skill", "--server", "github"]);
        if let Commands::Skill {
            server,
            servers_dir,
            output,
            skill_name,
            hints,
            overwrite,
        } = cli.command
        {
            assert_eq!(server, "github");
            assert!(servers_dir.is_none());
            assert!(output.is_none());
            assert!(skill_name.is_none());
            assert!(hints.is_empty());
            assert!(!overwrite);
        } else {
            panic!("Expected Skill command");
        }
    }

    #[test]
    fn test_cli_parsing_skill_all_options() {
        let cli = Cli::parse_from([
            "mcp-cli",
            "skill",
            "--server",
            "github",
            "--servers-dir",
            "/custom/servers",
            "--output",
            "/custom/skills/github.md",
            "--skill-name",
            "github-advanced",
            "--hint",
            "pull requests",
            "--hint",
            "code review",
            "--overwrite",
        ]);
        if let Commands::Skill {
            server,
            servers_dir,
            output,
            skill_name,
            hints,
            overwrite,
        } = cli.command
        {
            assert_eq!(server, "github");
            assert_eq!(servers_dir, Some(PathBuf::from("/custom/servers")));
            assert_eq!(output, Some(PathBuf::from("/custom/skills/github.md")));
            assert_eq!(skill_name, Some("github-advanced".to_string()));
            assert_eq!(
                hints,
                vec!["pull requests".to_string(), "code review".to_string()]
            );
            assert!(overwrite);
        } else {
            panic!("Expected Skill command");
        }
    }

    #[test]
    fn test_cli_parsing_skill_short_flags() {
        let cli = Cli::parse_from(["mcp-cli", "skill", "-s", "github", "-o", "/tmp/skill.md"]);
        if let Commands::Skill { server, output, .. } = cli.command {
            assert_eq!(server, "github");
            assert_eq!(output, Some(PathBuf::from("/tmp/skill.md")));
        } else {
            panic!("Expected Skill command");
        }
    }

    #[test]
    fn test_cli_parsing_skill_multiple_hints() {
        let cli = Cli::parse_from([
            "mcp-cli",
            "skill",
            "--server",
            "github",
            "--hint",
            "managing pull requests",
            "--hint",
            "code review",
            "--hint",
            "CI/CD automation",
        ]);
        if let Commands::Skill { hints, .. } = cli.command {
            assert_eq!(hints.len(), 3);
            assert_eq!(hints[0], "managing pull requests");
            assert_eq!(hints[1], "code review");
            assert_eq!(hints[2], "CI/CD automation");
        } else {
            panic!("Expected Skill command");
        }
    }

    #[test]
    fn test_cli_parsing_skill_overwrite() {
        let cli = Cli::parse_from(["mcp-cli", "skill", "--server", "test", "--overwrite"]);
        if let Commands::Skill { overwrite, .. } = cli.command {
            assert!(overwrite);
        } else {
            panic!("Expected Skill command");
        }
    }
}
