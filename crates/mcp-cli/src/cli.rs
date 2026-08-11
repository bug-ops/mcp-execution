//! CLI argument definitions and parsing.
//!
//! Defines the command-line interface structure using clap:
//! - `Cli` - Main CLI entry point
//! - `Commands` - Available subcommands

use clap::builder::{PossibleValuesParser, TypedValueParser as _};
use clap::{ArgGroup, Args, Parser, Subcommand};
use clap_complete::Shell;
use std::fmt;
use std::path::{Path, PathBuf};
use std::str::FromStr;

use crate::actions::ServerAction;
use crate::commands::common::{ServerSource, TransportArgs};
use mcp_execution_core::cli::{LogFormat, OutputFormat};
use mcp_execution_core::{Error as CoreError, RedactedItems, RedactedUrl, sanitize_path_for_error};

/// MCP Code Execution - Secure WASM-based MCP tool execution.
///
/// This CLI provides secure execution of MCP tools in a WebAssembly sandbox,
/// achieving 90-98% token savings through progressive tool loading.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::cli::Cli;
/// use clap::Parser;
///
/// // Parse command-line arguments into a Cli struct
/// let args = Cli::parse();
/// println!("Verbose: {}", args.verbose);
/// println!("Format: {:?}", args.format);
/// ```
#[derive(Parser)]
#[command(version, about, long_about = None)]
#[command(author = "MCP Execution Team")]
pub struct Cli {
    /// Subcommand to execute
    #[command(subcommand)]
    pub command: Commands,

    /// Enable verbose logging (debug level)
    #[arg(short, long, global = true)]
    pub verbose: bool,

    /// Output format
    #[arg(
        long = "format",
        global = true,
        default_value = "pretty",
        ignore_case = true,
        value_parser = PossibleValuesParser::new(["json", "text", "pretty"])
            .map(|s| OutputFormat::from_str(&s).expect("possible values are OutputFormat variants"))
    )]
    pub format: OutputFormat,

    /// Diagnostic log format: `text` (default) or `json`.
    ///
    /// Independent of `--format`, which controls command *result* output, not diagnostic
    /// logging. When unset, falls back to the `MCP_EXECUTION_LOG_FORMAT` environment variable;
    /// when that is also unset or invalid, defaults to `text`.
    #[arg(
        long = "log-format",
        global = true,
        ignore_case = true,
        value_parser = PossibleValuesParser::new(["text", "json"])
            .map(|s| LogFormat::from_str(&s).expect("possible values are LogFormat variants"))
    )]
    pub log_format: Option<LogFormat>,
}

// Hand-written to redact `Commands::Introspect`'s `env`/`headers`/`http`/`sse`
// and `Commands::Generate`'s `server_env`/`server_headers`/`http_url`/
// `sse_url` — these carry raw, unparsed `KEY=VALUE` secrets and URLs (which
// may embed credentials, e.g. `https://user:token@host/mcp`) straight from
// argv, before `TransportArgs`/`McpTransport` ever get a chance to redact
// them. Mirrors `commands::common::TransportArgs`'s `Debug` impl and reuses
// `mcp_execution_core::RedactedItems`/`RedactedUrl` rather than duplicating
// the redaction logic.
impl fmt::Debug for Cli {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Destructuring (rather than `&self.field`) turns a field added to
        // `Cli` without a matching arm here into a compile error instead of
        // relying solely on `clippy::missing_fields_in_debug` to catch it.
        let Self {
            command,
            verbose,
            format,
            log_format,
        } = self;
        f.debug_struct("Cli")
            .field("command", command)
            .field("verbose", verbose)
            .field("format", format)
            .field("log_format", log_format)
            .finish()
    }
}

/// Shared server-selection, transport, and timeout flags for `introspect` and `generate`.
///
/// Fields are private: the only way to obtain a value of this type is via clap parsing
/// (`#[command(flatten)]` on `Commands::Introspect`/`Commands::Generate`), which — via the
/// `server_source` argument group below — guarantees exactly one of `--from-config`, the
/// positional `server`, `--http`, or `--sse` is set before [`TryFrom<ServerFlags> for
/// ServerSource`](ServerSource) ever runs. This makes the illegal states (zero or multiple
/// selectors) unconstructible outside this module rather than merely checked at runtime.
///
/// # Examples
///
/// ```
/// use clap::Parser;
/// use mcp_execution_cli::cli::{Cli, Commands};
///
/// // The positional `server` and `--from-config`/`--http`/`--sse` are
/// // alternative selectors accepted by the same `server_source` group.
/// let cli = Cli::parse_from(["mcp-execution-cli", "introspect", "github-mcp-server"]);
/// assert!(matches!(cli.command, Commands::Introspect { .. }));
///
/// let cli = Cli::parse_from([
///     "mcp-execution-cli",
///     "introspect",
///     "--http",
///     "https://api.example.com/mcp",
/// ]);
/// assert!(matches!(cli.command, Commands::Introspect { .. }));
///
/// // Exactly one selector is required: none set is a parse error.
/// assert!(Cli::try_parse_from(["mcp-execution-cli", "introspect"]).is_err());
/// ```
#[derive(Args)]
#[command(group(
    ArgGroup::new("server_source")
        .required(true)
        .args(["from_config", "server", "http", "sse"])
))]
pub struct ServerFlags {
    /// Load server configuration from ~/.claude/mcp.json by name
    ///
    /// When specified, all other server configuration options are ignored.
    /// The server must be defined in ~/.claude/mcp.json with matching name.
    ///
    /// Example mcp.json (stdio and http entries can be mixed freely):
    /// ```json
    /// {
    ///   "mcpServers": {
    ///     "github": {
    ///       "command": "docker",
    ///       "args": ["run", "-i", "--rm", "..."],
    ///       "env": {"GITHUB_PERSONAL_ACCESS_TOKEN": "..."}
    ///     },
    ///     "remote": {
    ///       "type": "http",
    ///       "url": "https://api.example.com/mcp",
    ///       "headers": {"Authorization": "Bearer ..."}
    ///     }
    ///   }
    /// }
    /// ```
    #[arg(long = "from-config", conflicts_with_all = ["server", "args", "env", "cwd", "http", "sse", "connect_timeout_secs", "discover_timeout_secs"])]
    from_config: Option<String>,

    /// Server command (binary name or path)
    ///
    /// For stdio transport: command to execute (e.g., "docker", "npx", "github-mcp-server")
    /// Not required when using --from-config, --http, or --sse
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

    /// Override the connection (handshake) timeout, in seconds.
    ///
    /// Same field/units as `mcp.json`'s `connectTimeoutSecs`. Must be
    /// greater than zero and at most 600 seconds (10 minutes); there is
    /// no infinite-timeout option, since an unbounded wait would let a
    /// hung server block this command forever.
    ///
    /// Conflicts with `--from-config`: to override the timeout for a
    /// server defined in `mcp.json`, either edit its `connectTimeoutSecs`
    /// field, or re-run this command without `--from-config` using the
    /// server's command/args/env directly.
    #[arg(long = "connect-timeout-secs")]
    connect_timeout_secs: Option<u64>,

    /// Override the tool discovery timeout, in seconds.
    ///
    /// Same field/units as `mcp.json`'s `discoverTimeoutSecs`. Same
    /// bounds and `--from-config` conflict as `--connect-timeout-secs`.
    #[arg(long = "discover-timeout-secs")]
    discover_timeout_secs: Option<u64>,
}

// Hand-written to redact `server`/`env`/`http`/`sse`/`headers` — these carry
// raw, unparsed `KEY=VALUE` secrets and URLs (which may embed credentials,
// e.g. `https://user:token@host/mcp`) straight from argv, before
// `TransportArgs`/`McpTransport` ever get a chance to redact them. `args` is
// deliberately left unredacted here, matching the pre-existing
// `Commands::Debug` invariant (see `test_commands_debug_does_not_redact_args`):
// it is positional, not secret-shaped, and stays visible. This is an
// intentional asymmetry with `TransportArgs::Stdio`/`McpTransport::Stdio`,
// whose own `Debug` impls *do* wrap `args` in `RedactedItems` — a caller can
// still smuggle a secret through `--arg` (e.g. `docker run -e TOKEN=...`
// style), but by the time a `ServerSource`/`ServerConfig` value (the type
// that actually flows through the app and can end up in an error's
// `anyhow::Context`) is built from these flags, that later layer's
// redaction applies. `ServerFlags::Debug` itself is not on that path today
// (nothing prints a bare `Commands`/`ServerFlags` value), so this only
// matters if a future caller adds one.

impl fmt::Debug for ServerFlags {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            from_config,
            server,
            args,
            env,
            cwd,
            http,
            sse,
            headers,
            connect_timeout_secs,
            discover_timeout_secs,
        } = self;
        f.debug_struct("ServerFlags")
            .field("from_config", from_config)
            .field(
                "server",
                &server
                    .as_deref()
                    .map(|s| sanitize_path_for_error(Path::new(s))),
            )
            .field("args", args)
            .field("env", &RedactedItems(env))
            .field(
                "cwd",
                &cwd.as_deref()
                    .map(|cwd| sanitize_path_for_error(Path::new(cwd))),
            )
            .field("http", &http.as_deref().map(RedactedUrl))
            .field("sse", &sse.as_deref().map(RedactedUrl))
            .field("headers", &RedactedItems(headers))
            .field("connect_timeout_secs", connect_timeout_secs)
            .field("discover_timeout_secs", discover_timeout_secs)
            .finish()
    }
}

/// Converts clap's parsed [`ServerFlags`] landing zone into the closed
/// [`ServerSource`] domain enum.
///
/// # Errors
///
/// Returns [`CoreError::InvalidArgument`] if none or more than one of
/// `from_config`/`server`/`http`/`sse` is set. Unreachable when `flags` came
/// from real CLI parsing — the `server_source` argument group on
/// [`ServerFlags`] already enforces exactly one — but reachable from a
/// directly-constructed `ServerFlags` value (e.g. in tests, which can build
/// one since they are a child module of this one).
impl TryFrom<ServerFlags> for ServerSource {
    type Error = CoreError;

    fn try_from(flags: ServerFlags) -> Result<Self, Self::Error> {
        let ServerFlags {
            from_config,
            server,
            args,
            env,
            cwd,
            http,
            sse,
            headers,
            connect_timeout_secs,
            discover_timeout_secs,
        } = flags;

        match (from_config, server, http, sse) {
            (Some(name), None, None, None) => Ok(Self::Config { name }),
            (None, Some(command), None, None) => Ok(Self::Flags {
                transport: TransportArgs::Stdio {
                    command,
                    args,
                    env,
                    cwd,
                },
                connect_timeout_secs,
                discover_timeout_secs,
            }),
            (None, None, Some(url), None) => Ok(Self::Flags {
                transport: TransportArgs::Http { url, headers },
                connect_timeout_secs,
                discover_timeout_secs,
            }),
            (None, None, None, Some(url)) => Ok(Self::Flags {
                transport: TransportArgs::Sse { url, headers },
                connect_timeout_secs,
                discover_timeout_secs,
            }),
            _ => Err(CoreError::InvalidArgument(
                "exactly one of --from-config, a server command, --http, or --sse must be set"
                    .to_string(),
            )),
        }
    }
}

/// Available CLI subcommands.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::cli::{Cli, Commands};
/// use clap::Parser;
///
/// let args = Cli::parse();
/// match args.command {
///     Commands::Introspect { .. } => println!("Introspect command"),
///     Commands::Generate { .. } => println!("Generate command"),
///     Commands::Server { .. } => println!("Server command"),
///     Commands::Skill { .. } => println!("Skill command"),
///     Commands::Setup => println!("Setup command"),
///     Commands::Completions { .. } => println!("Completions command"),
/// }
/// ```
#[derive(Subcommand)]
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
    ///    mcp-execution-cli introspect --from-config github
    ///    ```
    ///
    /// 2. Manual configuration:
    ///    ```bash
    ///    mcp-execution-cli introspect github-mcp-server --arg=stdio
    ///    ```
    ///
    /// # Examples
    ///
    /// ```bash
    /// # Load GitHub server config from mcp.json
    /// mcp-execution-cli introspect --from-config github
    ///
    /// # Load with detailed schemas
    /// mcp-execution-cli introspect --from-config github --detailed
    ///
    /// # Manual: Simple binary
    /// mcp-execution-cli introspect github-mcp-server
    ///
    /// # Manual: With arguments
    /// mcp-execution-cli introspect github-mcp-server --arg=stdio
    ///
    /// # Manual: Docker container
    /// mcp-execution-cli introspect docker --arg=run --arg=-i --arg=--rm \
    ///     --arg=ghcr.io/github/github-mcp-server \
    ///     --env=GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx
    ///
    /// # HTTP transport
    /// mcp-execution-cli introspect --http https://api.githubcopilot.com/mcp/ \
    ///     --header "Authorization=Bearer ghp_xxx"
    /// ```
    Introspect {
        /// Server selection, transport, and timeout flags (shared with `generate`)
        #[command(flatten)]
        flags: ServerFlags,

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
    /// mcp-execution-cli skill --server github
    ///
    /// # With custom output path
    /// mcp-execution-cli skill --server github --output ~/.claude/skills/github/SKILL.md
    ///
    /// # With use case hints
    /// mcp-execution-cli skill --server github \
    ///     --hint "managing pull requests" \
    ///     --hint "reviewing code changes"
    ///
    /// # Overwrite existing skill
    /// mcp-execution-cli skill --server github --overwrite
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
        /// Multiple hints can be provided to generate more relevant documentation. Each hint is
        /// rendered as a bullet in the generated SKILL.md's "Use Cases" section.
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
    ///    mcp-execution-cli generate --from-config github
    ///    ```
    ///
    /// 2. Manual configuration:
    ///    ```bash
    ///    mcp-execution-cli generate docker --arg=run --arg=-i --arg=--rm \
    ///        --arg=ghcr.io/github/github-mcp-server \
    ///        --env=GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx \
    ///        --name=github
    ///    ```
    ///
    /// # Examples
    ///
    /// ```bash
    /// # Load GitHub server config from mcp.json
    /// mcp-execution-cli generate --from-config github
    ///
    /// # Manual Docker container
    /// mcp-execution-cli generate docker --arg=run --arg=-i --arg=--rm \
    ///     --arg=-e --arg=GITHUB_PERSONAL_ACCESS_TOKEN \
    ///     --arg=ghcr.io/github/github-mcp-server \
    ///     --env=GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx
    /// ```
    Generate {
        /// Server selection, transport, and timeout flags (shared with `introspect`)
        #[command(flatten)]
        flags: ServerFlags,

        /// Custom server name for directory (e.g., 'github' instead of 'docker')
        /// (default: uses server command name)
        #[arg(long)]
        name: Option<String>,

        /// Custom output directory for progressive loading files
        /// (default: ~/.claude/servers/)
        #[arg(long)]
        progressive_output: Option<PathBuf>,

        /// Preview files that would be generated without writing to disk
        #[arg(long)]
        dry_run: bool,
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

impl fmt::Debug for Commands {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Introspect { flags, detailed } => f
                .debug_struct("Introspect")
                .field("flags", flags)
                .field("detailed", detailed)
                .finish(),
            Self::Skill {
                server,
                servers_dir,
                output,
                skill_name,
                hints,
                overwrite,
            } => f
                .debug_struct("Skill")
                .field("server", server)
                .field("servers_dir", servers_dir)
                .field("output", output)
                .field("skill_name", skill_name)
                .field("hints", hints)
                .field("overwrite", overwrite)
                .finish(),
            Self::Generate {
                flags,
                name,
                progressive_output,
                dry_run,
            } => f
                .debug_struct("Generate")
                .field("flags", flags)
                .field("name", name)
                .field("progressive_output", progressive_output)
                .field("dry_run", dry_run)
                .finish(),
            Self::Server { action } => f.debug_struct("Server").field("action", action).finish(),
            Self::Setup => write!(f, "Setup"),
            Self::Completions { shell } => {
                f.debug_struct("Completions").field("shell", shell).finish()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::CommandFactory;

    #[test]
    fn test_cli_help_examples_use_published_binary_name() {
        let mut command = Cli::command();

        for subcommand in ["introspect", "generate", "skill"] {
            let help = command
                .find_subcommand_mut(subcommand)
                .expect("subcommand should exist")
                .render_long_help()
                .to_string();

            assert!(
                help.contains(&format!("mcp-execution-cli {subcommand}")),
                "{subcommand} help should include examples with the published binary name"
            );
            assert!(
                !help.contains(&format!("mcp-cli {subcommand}")),
                "{subcommand} help should not reference the old binary name"
            );
        }
    }

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
        if let Commands::Introspect { flags, .. } = cli.command {
            assert_eq!(flags.server, Some("docker".to_string()));
            assert_eq!(
                flags.args,
                vec!["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"]
            );
            assert_eq!(flags.env, vec!["GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx"]);
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
        if let Commands::Introspect { flags, .. } = cli.command {
            assert_eq!(flags.server, None);
            assert_eq!(
                flags.http,
                Some("https://api.githubcopilot.com/mcp/".to_string())
            );
            assert_eq!(flags.headers, vec!["Authorization=Bearer token"]);
        } else {
            panic!("Expected Introspect command");
        }
    }

    #[test]
    fn test_cli_parsing_introspect_timeout_overrides() {
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "docker",
            "--connect-timeout-secs",
            "5",
            "--discover-timeout-secs",
            "90",
        ]);
        if let Commands::Introspect { flags, .. } = cli.command {
            assert_eq!(flags.connect_timeout_secs, Some(5));
            assert_eq!(flags.discover_timeout_secs, Some(90));
        } else {
            panic!("Expected Introspect command");
        }
    }

    #[test]
    fn test_cli_parsing_introspect_timeout_conflicts_with_from_config() {
        let result = Cli::try_parse_from([
            "mcp-cli",
            "introspect",
            "--from-config",
            "github",
            "--connect-timeout-secs",
            "5",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing_generate_timeout_conflicts_with_from_config() {
        let result = Cli::try_parse_from([
            "mcp-cli",
            "generate",
            "--from-config",
            "github",
            "--discover-timeout-secs",
            "90",
        ]);
        assert!(result.is_err());
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
    fn test_cli_parsing_generate_timeout_overrides() {
        let cli = Cli::parse_from([
            "mcp-cli",
            "generate",
            "docker",
            "--connect-timeout-secs",
            "5",
            "--discover-timeout-secs",
            "90",
        ]);
        if let Commands::Generate { flags, .. } = cli.command {
            assert_eq!(flags.connect_timeout_secs, Some(5));
            assert_eq!(flags.discover_timeout_secs, Some(90));
        } else {
            panic!("Expected Generate command");
        }
    }

    #[test]
    fn test_cli_parsing_generate_dry_run() {
        let cli = Cli::parse_from(["mcp-cli", "generate", "server", "--dry-run"]);
        if let Commands::Generate { dry_run, .. } = cli.command {
            assert!(dry_run);
        } else {
            panic!("Expected Generate command");
        }
    }

    #[test]
    fn test_cli_parsing_generate_dry_run_default_false() {
        let cli = Cli::parse_from(["mcp-cli", "generate", "server"]);
        if let Commands::Generate { dry_run, .. } = cli.command {
            assert!(!dry_run);
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
        assert_eq!(cli.format, OutputFormat::Pretty);
    }

    #[test]
    fn test_cli_output_format_custom() {
        let cli = Cli::parse_from(["mcp-cli", "--format", "json", "introspect", "github"]);
        assert_eq!(cli.format, OutputFormat::Json);
    }

    #[test]
    fn test_cli_output_format_invalid_rejected_by_clap() {
        let result = Cli::try_parse_from(["mcp-cli", "--format", "xml", "introspect", "github"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_output_format_possible_values_parse_via_from_str() {
        // Guards the `--format` value parser's `expect()` against the
        // `PossibleValuesParser` list drifting from `OutputFormat`'s actual
        // variants: reads the possible values off the real `clap::Command`
        // rather than hardcoding a third independent copy of the list, so an
        // entry added to one side without the other fails here instead of
        // panicking inside clap's argument parsing.
        let cmd = Cli::command();
        let arg = cmd
            .get_arguments()
            .find(|a| a.get_id() == "format")
            .expect("--format argument must exist");
        let values = arg.get_possible_values();
        assert!(!values.is_empty(), "--format must declare possible values");
        for possible_value in values {
            let name = possible_value.get_name();
            assert!(
                OutputFormat::from_str(name).is_ok(),
                "{name} must parse via OutputFormat::from_str to match the --format value parser"
            );
        }
    }

    #[test]
    fn test_cli_output_format_case_insensitive() {
        let cli = Cli::parse_from(["mcp-cli", "--format", "JSON", "introspect", "github"]);
        assert_eq!(cli.format, OutputFormat::Json);

        let cli = Cli::parse_from(["mcp-cli", "--format", "PRETTY", "introspect", "github"]);
        assert_eq!(cli.format, OutputFormat::Pretty);
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
    fn test_cli_log_format_default_unset() {
        let cli = Cli::parse_from(["mcp-cli", "introspect", "github"]);
        assert_eq!(cli.log_format, None);
    }

    #[test]
    fn test_cli_log_format_json() {
        let cli = Cli::parse_from(["mcp-cli", "--log-format", "json", "introspect", "github"]);
        assert_eq!(cli.log_format, Some(LogFormat::Json));
    }

    #[test]
    fn test_cli_log_format_case_insensitive() {
        let cli = Cli::parse_from(["mcp-cli", "--log-format", "JSON", "introspect", "github"]);
        assert_eq!(cli.log_format, Some(LogFormat::Json));
    }

    #[test]
    fn test_cli_log_format_invalid_rejected_by_clap() {
        let result =
            Cli::try_parse_from(["mcp-cli", "--log-format", "xml", "introspect", "github"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_log_format_global_flag_accepted_after_subcommand() {
        let cli = Cli::parse_from(["mcp-cli", "introspect", "github", "--log-format", "json"]);
        assert_eq!(cli.log_format, Some(LogFormat::Json));
    }

    #[test]
    fn test_cli_log_format_possible_values_parse_via_from_str() {
        let cmd = Cli::command();
        let arg = cmd
            .get_arguments()
            .find(|a| a.get_id() == "log_format")
            .expect("--log-format argument must exist");
        let values = arg.get_possible_values();
        assert!(
            !values.is_empty(),
            "--log-format must declare possible values"
        );
        for possible_value in values {
            let name = possible_value.get_name();
            assert!(
                LogFormat::from_str(name).is_ok(),
                "{name} must parse via LogFormat::from_str to match the --log-format value parser"
            );
        }
    }

    #[test]
    fn test_cli_log_format_help_documents_env_var() {
        let mut command = Cli::command();
        let help = command.render_long_help().to_string();
        assert!(
            help.contains("MCP_EXECUTION_LOG_FORMAT"),
            "--help must document the MCP_EXECUTION_LOG_FORMAT environment variable per FR-004"
        );
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

    #[test]
    fn test_commands_debug_redacts_introspect_env_and_headers() {
        let secret_body = "sk-verySECRETtoken1234567890";
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "docker",
            "--env",
            &format!("GITHUB_TOKEN={secret_body}"),
            "--header",
            &format!("Authorization=Bearer {secret_body}"),
        ]);

        let debug_output = format!("{:?}", cli.command);
        assert!(debug_output.contains("<redacted>"));
        assert!(!debug_output.contains(secret_body));
    }

    #[test]
    fn test_commands_debug_redacts_generate_env_and_headers() {
        let secret_body = "sk-verySECRETtoken1234567890";
        let cli = Cli::parse_from([
            "mcp-cli",
            "generate",
            "docker",
            "--env",
            &format!("GITHUB_TOKEN={secret_body}"),
            "--header",
            &format!("Authorization=Bearer {secret_body}"),
        ]);

        let debug_output = format!("{:?}", cli.command);
        assert!(debug_output.contains("<redacted>"));
        assert!(!debug_output.contains(secret_body));
    }

    #[test]
    fn test_commands_debug_does_not_redact_args() {
        // `args`/`server_args` are positional, not secret-shaped, and must
        // remain visible in Debug output.
        let cli = Cli::parse_from(["mcp-cli", "introspect", "docker", "--arg=stdio"]);
        let debug_output = format!("{:?}", cli.command);
        assert!(debug_output.contains("stdio"));
    }

    #[test]
    fn test_commands_debug_redacts_introspect_http_url() {
        let secret = "sk-verySECRETtoken1234567890";
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "--http",
            &format!("https://user:{secret}@host.example.com/mcp?token={secret}"),
        ]);

        let debug_output = format!("{:?}", cli.command);
        assert!(!debug_output.contains(secret));
        assert!(debug_output.contains("host.example.com/mcp"));
    }

    #[test]
    fn test_commands_debug_redacts_introspect_sse_url() {
        let secret = "sk-verySECRETtoken1234567890";
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "--sse",
            &format!("https://user:{secret}@host.example.com/mcp?token={secret}"),
        ]);

        let debug_output = format!("{:?}", cli.command);
        assert!(!debug_output.contains(secret));
        assert!(debug_output.contains("host.example.com/mcp"));
    }

    #[test]
    fn test_commands_debug_redacts_generate_http_url() {
        let secret = "sk-verySECRETtoken1234567890";
        let cli = Cli::parse_from([
            "mcp-cli",
            "generate",
            "--http",
            &format!("https://user:{secret}@host.example.com/mcp?token={secret}"),
        ]);

        let debug_output = format!("{:?}", cli.command);
        assert!(!debug_output.contains(secret));
        assert!(debug_output.contains("host.example.com/mcp"));
    }

    #[test]
    fn test_commands_debug_redacts_generate_sse_url() {
        let secret = "sk-verySECRETtoken1234567890";
        let cli = Cli::parse_from([
            "mcp-cli",
            "generate",
            "--sse",
            &format!("https://user:{secret}@host.example.com/mcp?token={secret}"),
        ]);

        let debug_output = format!("{:?}", cli.command);
        assert!(!debug_output.contains(secret));
        assert!(debug_output.contains("host.example.com/mcp"));
    }

    #[test]
    fn test_server_flags_debug_redacts_secret_shaped_fields() {
        // Migrated from the former `RawServerArgs` regression test: `server`'s
        // home-relative path must be tilde-sanitized, not just the URL/env/header
        // fields already covered above.
        let secret = "sk-live-secret";
        let home = dirs::home_dir().expect("home directory must be resolvable in test environment");
        let server_path = home.join("tools").join("mcp-server");

        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            &server_path.display().to_string(),
            "--env",
            &format!("GITHUB_TOKEN={secret}"),
            "--connect-timeout-secs",
            "30",
        ]);

        let debug_output = format!("{:?}", cli.command);
        assert!(!debug_output.contains(secret));
        assert!(!debug_output.contains(&home.display().to_string()));
        assert!(debug_output.contains('~'));
        assert!(debug_output.contains("connect_timeout_secs: Some(30)"));
    }

    // ── #314: `server_source` argument group makes the transport-selector
    // exclusivity a clap-level guarantee instead of a runtime check ──

    #[test]
    fn test_cli_parsing_introspect_positional_with_http_errors() {
        // Approved behavior change: previously the positional command was
        // silently discarded by `TransportArgs::from_flags` in favor of
        // `--http`. Now the `server_source` group rejects both being set.
        let result = Cli::try_parse_from([
            "mcp-cli",
            "introspect",
            "docker",
            "--http",
            "https://api.example.com",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing_generate_positional_with_http_errors() {
        let result = Cli::try_parse_from([
            "mcp-cli",
            "generate",
            "docker",
            "--http",
            "https://api.example.com",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing_introspect_no_selector_errors() {
        let result = Cli::try_parse_from(["mcp-cli", "introspect"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing_generate_no_selector_errors() {
        let result = Cli::try_parse_from(["mcp-cli", "generate"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing_introspect_http_and_sse_together_errors() {
        let result = Cli::try_parse_from([
            "mcp-cli",
            "introspect",
            "--http",
            "https://api.example.com",
            "--sse",
            "https://api.example.com/sse",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing_generate_http_and_sse_together_errors() {
        let result = Cli::try_parse_from([
            "mcp-cli",
            "generate",
            "--http",
            "https://api.example.com",
            "--sse",
            "https://api.example.com/sse",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing_introspect_from_config_and_http_together_errors() {
        let result = Cli::try_parse_from([
            "mcp-cli",
            "introspect",
            "--from-config",
            "github",
            "--http",
            "https://api.example.com",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_cli_parsing_generate_from_config_and_http_together_errors() {
        let result = Cli::try_parse_from([
            "mcp-cli",
            "generate",
            "--from-config",
            "github",
            "--http",
            "https://api.example.com",
        ]);
        assert!(result.is_err());
    }

    #[test]
    fn test_server_source_try_from_server_flags_catch_all_errors() {
        // Legal only because this test module is a child of `cli`, so it can
        // see `ServerFlags`'s private fields. Unreachable via real CLI
        // parsing: the `server_source` argument group already guarantees
        // exactly one selector is set.
        let flags = ServerFlags {
            from_config: None,
            server: None,
            args: vec![],
            env: vec![],
            cwd: None,
            http: None,
            sse: None,
            headers: vec![],
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        let result = ServerSource::try_from(flags);
        assert!(result.is_err());
    }

    // ── S1: round-trip parse -> `TryFrom<ServerFlags>` -> assert variant and
    // payload for each of the four `Ok` arms. Without these, transposing the
    // `Http`/`Sse` arms or the `args`/`env` fields inside `Stdio` (all
    // same-typed) would compile and pass the rest of the suite — exactly
    // issue #286's bug class, previously guarded by the now-deleted
    // `test_transport_args_from_flags_{stdio,http,sse}` tests. ──

    fn introspect_flags(cli: Cli) -> ServerFlags {
        match cli.command {
            Commands::Introspect { flags, .. } => flags,
            other => panic!("expected Introspect command, got {other:?}"),
        }
    }

    #[test]
    fn test_server_source_try_from_config_arm() {
        let cli = Cli::parse_from(["mcp-cli", "introspect", "--from-config", "github"]);
        let source = ServerSource::try_from(introspect_flags(cli)).unwrap();

        assert!(matches!(source, ServerSource::Config { name } if name == "github"));
    }

    #[test]
    fn test_server_source_try_from_stdio_arm_does_not_transpose_args_and_env() {
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "docker",
            "--arg=run",
            "--env=TOKEN=abc",
            "--cwd=/tmp/work",
        ]);
        let source = ServerSource::try_from(introspect_flags(cli)).unwrap();

        match source {
            ServerSource::Flags {
                transport:
                    TransportArgs::Stdio {
                        command,
                        args,
                        env,
                        cwd,
                    },
                ..
            } => {
                assert_eq!(command, "docker");
                assert_eq!(args, vec!["run".to_string()]);
                assert_eq!(env, vec!["TOKEN=abc".to_string()]);
                assert_eq!(cwd, Some("/tmp/work".to_string()));
            }
            other => panic!("expected Flags{{Stdio}}, got {other:?}"),
        }
    }

    #[test]
    fn test_server_source_try_from_http_arm_does_not_swap_with_sse() {
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "--http",
            "https://api.example.com/mcp",
            "--header=Authorization=Bearer x",
        ]);
        let source = ServerSource::try_from(introspect_flags(cli)).unwrap();

        match source {
            ServerSource::Flags {
                transport: TransportArgs::Http { url, headers },
                ..
            } => {
                assert_eq!(url, "https://api.example.com/mcp");
                assert_eq!(headers, vec!["Authorization=Bearer x".to_string()]);
            }
            other => panic!("expected Flags{{Http}}, got {other:?}"),
        }
    }

    #[test]
    fn test_server_source_try_from_sse_arm_does_not_swap_with_http() {
        let cli = Cli::parse_from([
            "mcp-cli",
            "introspect",
            "--sse",
            "https://api.example.com/sse",
            "--header=X-API-Key=secret",
        ]);
        let source = ServerSource::try_from(introspect_flags(cli)).unwrap();

        match source {
            ServerSource::Flags {
                transport: TransportArgs::Sse { url, headers },
                ..
            } => {
                assert_eq!(url, "https://api.example.com/sse");
                assert_eq!(headers, vec!["X-API-Key=secret".to_string()]);
            }
            other => panic!("expected Flags{{Sse}}, got {other:?}"),
        }
    }
}
