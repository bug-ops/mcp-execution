//! Server command implementation.
//!
//! Manages MCP server listing, inspection, and validation using
//! `~/.claude/mcp.json` as the single source of truth for server definitions.

use crate::actions::ServerAction;
use crate::commands::common::{
    McpServerEntry, McpTransport, build_core_config, get_mcp_server_entry, list_mcp_servers,
};
use crate::formatters::escape_error_text;
use anyhow::{Context, Result};
use mcp_execution_core::ServerConfig;
use mcp_execution_core::ServerId;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_introspector::Introspector;
use serde::Serialize;
use std::time::Duration;
use tracing::{info, warn};
use url::Url;

/// Maximum time `server list` waits for a single http/sse availability
/// check, independent of (and shorter than) the entry's own configured
/// `connect_timeout_secs`/`discover_timeout_secs`.
///
/// `list` enumerates every configured server and users expect it to stay
/// responsive — especially with several servers configured — even though
/// checks already run concurrently (see [`list_servers`]). Three seconds is
/// generous enough for a typical cross-network MCP handshake while keeping a
/// single slow or firewalled entry from making the whole command visibly
/// hang. `server validate <name>`/`server info <name>` do not use this
/// bound: they are explicit, single-target commands where a user consciously
/// waits for a definitive answer using the entry's full configured timeout.
const LIST_AVAILABILITY_TIMEOUT: Duration = Duration::from_secs(3);

/// Status of a configured server.
///
/// The precise check behind this depends on the call site: `server list`
/// uses [`transport_available`] (PATH lookup for stdio; URL well-formedness
/// plus a bounded MCP introspection attempt for http/sse — see
/// `LIST_AVAILABILITY_TIMEOUT`). `server info`/`server validate` instead
/// reflect whether a full MCP introspection handshake succeeded, waiting out
/// the entry's full configured `connect_timeout_secs`/`discover_timeout_secs`.
///
/// For **http/sse**, `list` and `info`/`validate` share the exact same
/// connection path (`Introspector::discover_server`), so they can no longer
/// disagree about *how* a transport is reached (proxying, IPv6) — only about
/// *how long* the check is allowed to run. `list`'s bounded check is a
/// time-boxed, best-effort signal across every configured server; `server
/// validate <name>`/`server info <name>` are the authoritative check for one
/// specific server. A server that is merely slow (past `list`'s short bound
/// but within its own configured timeout) can therefore show `unavailable`
/// in `list` and `available` in `validate`/`info`. This is an intentional,
/// documented trade-off — distinct from #280, which was an unconditional
/// *wrong* answer, not a bounded, best-effort one.
///
/// For **stdio**, this equivalence does not hold: `list` still performs only
/// a PATH lookup while `info`/`validate` perform a full handshake, so the
/// two can disagree about more than timing (pre-existing behavior, unrelated
/// to #280's http/sse scope).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerStatus {
    /// `list`: command in PATH / URL well-formed and reachable. `info`:
    /// introspection succeeded.
    Available,
    /// `list`: command missing / URL malformed or unreachable. `info`:
    /// introspection failed.
    Unavailable,
}

impl ServerStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Represents a configured server entry for output.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::server::ServerEntry;
///
/// let entry = ServerEntry {
///     id: "github".to_string(),
///     command: "github-mcp-server".to_string(),
///     status: "available".to_string(),
/// };
///
/// assert_eq!(entry.id, "github");
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerEntry {
    /// Server identifier.
    pub id: String,
    /// Command used to start the server.
    pub command: String,
    /// Current server status (`"available"` or `"unavailable"`). For stdio,
    /// a PATH lookup. For http/sse, a well-formedness pre-check followed by
    /// the same MCP introspection handshake `server info`/`server validate`
    /// use — but bounded to `LIST_AVAILABILITY_TIMEOUT` rather than the
    /// entry's full configured timeout, so this is a time-bounded,
    /// best-effort signal. Run `server validate <name>` for an authoritative
    /// answer on one specific server.
    pub status: String,
}

/// List of configured servers.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::server::{ServerEntry, ServerList};
///
/// let list = ServerList {
///     servers: vec![
///         ServerEntry {
///             id: "github".to_string(),
///             command: "github-mcp-server".to_string(),
///             status: "available".to_string(),
///         }
///     ],
/// };
///
/// assert_eq!(list.servers.len(), 1);
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerList {
    /// All configured servers.
    pub servers: Vec<ServerEntry>,
}

/// Detailed server information for output.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::server::{ServerInfo, ToolSummary};
///
/// let info = ServerInfo {
///     id: "github".to_string(),
///     name: "GitHub MCP".to_string(),
///     version: "1.0.0".to_string(),
///     command: "github-mcp-server".to_string(),
///     status: "available".to_string(),
///     tools: vec![],
///     capabilities: vec!["tools".to_string()],
/// };
///
/// assert_eq!(info.id, "github");
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerInfo {
    /// Server identifier.
    pub id: String,
    /// Server name from introspection.
    pub name: String,
    /// Server version.
    pub version: String,
    /// Command used to start the server.
    pub command: String,
    /// Current server status.
    pub status: String,
    /// Available tools.
    pub tools: Vec<ToolSummary>,
    /// Server capabilities.
    pub capabilities: Vec<String>,
}

/// Tool summary for output.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::server::ToolSummary;
///
/// let tool = ToolSummary {
///     name: "search".to_string(),
///     description: "Search repositories".to_string(),
/// };
///
/// assert_eq!(tool.name, "search");
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolSummary {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
}

/// Validation result for a server command.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::server::ValidationResult;
///
/// let result = ValidationResult {
///     command: "server".to_string(),
///     valid: true,
///     message: "Command is valid".to_string(),
/// };
///
/// assert!(result.valid);
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidationResult {
    /// The validated command.
    pub command: String,
    /// Whether the command is valid.
    pub valid: bool,
    /// Validation message.
    pub message: String,
}

/// Runs the server command.
///
/// Manages server listing, detailed info, and validation.
/// All server definitions are loaded from `~/.claude/mcp.json`.
///
/// # Arguments
///
/// * `action` - Server management action (List, Info, or Validate)
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if:
/// - The configuration file cannot be read or is malformed
/// - For the `Info` action, the named server is not found in the configuration
/// - Output formatting fails (serialization error)
///
/// Note: For the `Validate` action, an unknown server name is reported via
/// `ExitCode::ERROR` rather than returning `Err`. Server introspection failures
/// (for both `Info` and `Validate`) are also caught internally and reported via
/// `ExitCode::ERROR`. So is an entry that is present but fails security validation (e.g. an
/// invalid URL scheme, #305/#304) — only a genuinely absent entry propagates as `Err`.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::server;
/// use mcp_execution_core::cli::{ExitCode, OutputFormat};
///
/// # #[tokio::main]
/// # async fn main() {
/// let result = server::run(
///     mcp_execution_cli::ServerAction::List,
///     OutputFormat::Json
/// ).await;
/// assert!(result.is_ok());
/// # }
/// ```
pub async fn run(action: ServerAction, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Server action: {:?}", action);
    info!("Output format: {}", output_format);

    match action {
        ServerAction::List => list_servers(output_format).await,
        ServerAction::Info { server } => show_server_info(server, output_format).await,
        ServerAction::Validate { command } => validate_command(command, output_format).await,
    }
}

/// Lists all servers configured in `~/.claude/mcp.json`.
///
/// Returns an empty list (not an error) when the config file does not exist.
///
/// For every http/sse entry, this performs a real, bounded MCP handshake
/// against the remote server (see `LIST_AVAILABILITY_TIMEOUT`), not a purely
/// local check — this has real network cost and, per known
/// `mcp-execution-introspector` limitations, can leave an orphaned session
/// on the remote server per invocation.
async fn list_servers(output_format: OutputFormat) -> Result<ExitCode> {
    let servers = list_mcp_servers()
        .context("failed to read server configuration from ~/.claude/mcp.json")?;

    if servers.is_empty() {
        info!("No MCP servers configured in ~/.claude/mcp.json");
        let server_list = ServerList {
            servers: Vec::new(),
        };
        let formatted = crate::formatters::format_output(&server_list, output_format)?;
        println!("{formatted}");
        return Ok(ExitCode::SUCCESS);
    }

    // Each server's status check may include a full MCP introspection
    // attempt (see `transport_available`); run them concurrently so `list`'s
    // total latency is bounded by the slowest single check, not their sum.
    let checks = servers.into_iter().map(|(name, entry)| async move {
        let command = build_command_string(&entry);
        let status = if transport_available(&name, &entry).await {
            ServerStatus::Available
        } else {
            ServerStatus::Unavailable
        };

        ServerEntry {
            id: name,
            command,
            status: status.as_str().to_string(),
        }
    });
    let entries = futures_util::future::join_all(checks).await;

    let server_list = ServerList { servers: entries };
    let formatted = crate::formatters::format_output(&server_list, output_format)?;
    println!("{formatted}");

    Ok(ExitCode::SUCCESS)
}

/// Shows detailed information about a specific server.
///
/// Connects to the server and introspects its capabilities, tools, and status.
///
/// An entry whose `url` (or other field) fails [`build_core_config`]'s security validation is
/// reported the same way as an entry that is well-formed but unreachable — a structured
/// `"status": "unavailable"` [`ServerInfo`] through `output_format`, not a raw, unformatted error
/// (#305). Only a genuinely absent entry propagates as `Err` via [`get_mcp_server_entry`].
async fn show_server_info(server: String, output_format: OutputFormat) -> Result<ExitCode> {
    let (server_id, entry) = get_mcp_server_entry(&server)?;
    let command = build_command_string(&entry);

    let server_config = match build_core_config(&entry) {
        Ok(config) => config,
        Err(e) => {
            warn!(
                "Server '{}' has an invalid configuration: {}",
                server,
                escape_error_text(&e.to_string())
            );
            let formatted = crate::formatters::format_output(
                &unavailable_server_info(server, command),
                output_format,
            )?;
            println!("{formatted}");
            return Ok(ExitCode::ERROR);
        }
    };

    info!("Introspecting server '{}'...", server);

    let mut introspector = Introspector::new();
    match introspector
        .discover_server(server_id, &server_config)
        .await
    {
        Ok(introspected) => {
            let mut capabilities = Vec::new();
            if introspected.capabilities.supports_tools {
                capabilities.push("tools".to_string());
            }
            if introspected.capabilities.supports_resources {
                capabilities.push("resources".to_string());
            }
            if introspected.capabilities.supports_prompts {
                capabilities.push("prompts".to_string());
            }

            let tools = introspected
                .tools
                .iter()
                .map(|t| ToolSummary {
                    name: t.name.as_str().to_string(),
                    description: t.description.clone(),
                })
                .collect();

            let server_info = ServerInfo {
                id: server,
                name: introspected.name,
                version: introspected.version,
                command,
                status: ServerStatus::Available.as_str().to_string(),
                tools,
                capabilities,
            };

            let formatted = crate::formatters::format_output(&server_info, output_format)?;
            println!("{formatted}");

            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            warn!(
                "Failed to introspect server '{}': {}",
                server,
                escape_error_text(&e.to_string())
            );

            let formatted = crate::formatters::format_output(
                &unavailable_server_info(server, command),
                output_format,
            )?;
            println!("{formatted}");

            Ok(ExitCode::ERROR)
        }
    }
}

/// Builds the `"status": "unavailable"` [`ServerInfo`] shared by `show_server_info`'s two failure
/// branches — invalid configuration and failed introspection — so both report through the same
/// structured shape.
fn unavailable_server_info(server: String, command: String) -> ServerInfo {
    ServerInfo {
        id: server.clone(),
        name: server,
        version: "unknown".to_string(),
        command,
        status: ServerStatus::Unavailable.as_str().to_string(),
        tools: Vec::new(),
        capabilities: Vec::new(),
    }
}

/// Validates a server by checking its command and attempting introspection.
///
/// The server must be configured in `~/.claude/mcp.json`. An entry that is present but fails
/// [`build_core_config`]'s security validation (e.g. an invalid URL scheme) is reported with a
/// message describing that specific problem, not the "not found" message reserved for a
/// genuinely absent entry (#304).
async fn validate_command(server_name: String, output_format: OutputFormat) -> Result<ExitCode> {
    let (server_id, entry) = match get_mcp_server_entry(&server_name) {
        Ok(result) => result,
        Err(e) => {
            let result = ValidationResult {
                command: server_name,
                valid: false,
                message: format!("Server not found in configuration: {e}"),
            };
            let formatted = crate::formatters::format_output(&result, output_format)?;
            println!("{formatted}");
            return Ok(ExitCode::ERROR);
        }
    };

    let command = build_command_string(&entry);
    info!("Validating server '{}'...", server_name);

    // Exhaustive over `McpTransport` with no `_` arm: adding a new transport
    // variant must fail to compile here rather than silently skip the
    // precheck (that asymmetry with the exhaustive match below is what let
    // #280 slip through).
    let precheck_failure = match &entry.transport {
        McpTransport::Stdio {
            command: bin_command,
            ..
        } => (!check_command_exists(bin_command))
            .then(|| format!("Command '{bin_command}' not found in PATH")),
        McpTransport::Http { url, .. } | McpTransport::Sse { url, .. } => (!url_well_formed(url))
            .then(|| {
                format!("URL '{url}' is not well-formed (expected http:// or https:// with a host)")
            }),
    };

    if let Some(message) = precheck_failure {
        let result = ValidationResult {
            command: command.clone(),
            valid: false,
            message,
        };
        let formatted = crate::formatters::format_output(&result, output_format)?;
        println!("{formatted}");
        return Ok(ExitCode::ERROR);
    }

    // The precheck above catches the common malformed-URL/missing-command cases, but
    // `build_core_config` runs additional security validation (e.g. header safety, timeout
    // bounds) the precheck does not duplicate. A failure here is still "entry present, invalid
    // configuration" rather than "entry not found", so it gets its own message rather than
    // falling through to `get_mcp_server_entry`'s not-found wrapping.
    let server_config = match build_core_config(&entry) {
        Ok(config) => config,
        Err(e) => {
            let result = ValidationResult {
                command,
                valid: false,
                message: format!("Server '{server_name}' has an invalid configuration: {e}"),
            };
            let formatted = crate::formatters::format_output(&result, output_format)?;
            println!("{formatted}");
            return Ok(ExitCode::ERROR);
        }
    };

    let mut introspector = Introspector::new();
    match introspector
        .discover_server(server_id, &server_config)
        .await
    {
        Ok(_) => {
            let result = ValidationResult {
                command,
                valid: true,
                message: format!(
                    "Server '{server_name}' is available and responds to MCP protocol"
                ),
            };
            let formatted = crate::formatters::format_output(&result, output_format)?;
            println!("{formatted}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            warn!(
                "Failed to introspect server '{}' during validation: {}",
                server_name,
                escape_error_text(&e.to_string())
            );
            let message = match &entry.transport {
                McpTransport::Stdio { .. } => format!(
                    "Server '{server_name}' command exists but failed to respond to MCP protocol"
                ),
                McpTransport::Http { .. } | McpTransport::Sse { .. } => {
                    format!("Server '{server_name}' endpoint failed to respond to MCP protocol")
                }
            };
            let result = ValidationResult {
                command,
                valid: false,
                message,
            };
            let formatted = crate::formatters::format_output(&result, output_format)?;
            println!("{formatted}");
            Ok(ExitCode::ERROR)
        }
    }
}

/// Builds a displayable command string from a server entry.
///
/// Stdio entries render as `command args…`; http/sse entries render as the
/// endpoint URL.
fn build_command_string(entry: &McpServerEntry) -> String {
    match &entry.transport {
        McpTransport::Stdio { command, args, .. } => {
            if args.is_empty() {
                command.clone()
            } else {
                format!("{} {}", command, args.join(" "))
            }
        }
        McpTransport::Http { url, .. } | McpTransport::Sse { url, .. } => url.clone(),
    }
}

/// Returns `true` if the given command binary is available in PATH.
fn check_command_exists(command: &str) -> bool {
    which::which(command).is_ok()
}

/// Returns `true` if `url` is a well-formed `http://`/`https://` URL with a host.
///
/// Delegates the scheme check to
/// [`mcp_execution_core::validate_url_scheme`] rather than re-deriving it
/// from `url::Url::parse` (which normalizes whitespace and other input
/// `validate_url_scheme` does not) — two disagreeing URL-validity checks
/// across `mcp-core` and `mcp-cli` for the same transport is the same defect
/// class as the transport-mismatch this module already guards against. The
/// combined check can therefore never accept a URL `validate_url_scheme`
/// (and, transitively, `server validate`/`generate`) would reject; the host
/// check on top is strictly additional and only makes this stricter, never
/// more permissive.
fn url_well_formed(url: &str) -> bool {
    mcp_execution_core::validate_url_scheme(url).is_ok()
        && Url::parse(url).is_ok_and(|parsed| parsed.host().is_some())
}

/// Returns `true` if the entry's transport is ready to attempt a connection.
///
/// Stdio checks PATH for the command. Http/Sse first checks that the URL is
/// well-formed, then attempts the same MCP introspection handshake `server
/// info`/`server validate` use via [`Introspector::discover_server`] — the
/// same connection path, so it automatically honors the entry's configured
/// `connect_timeout_secs`/`discover_timeout_secs`, IPv6 literals, and any
/// proxy handling the underlying transport applies, with nothing
/// re-implemented or kept in sync by hand here. Unlike `server info`/`server
/// validate`, this attempt is additionally bounded by the short
/// `LIST_AVAILABILITY_TIMEOUT`, since `list` checks every configured server
/// and must stay responsive: a server that is merely slower than that bound
/// (but still within its own configured timeout) is reported unavailable
/// here even though `validate`/`info` would eventually report it available.
///
/// `name` is only used to build the [`ServerId`] passed to the introspector
/// for the http/sse case; it plays no role in the stdio PATH check.
async fn transport_available(name: &str, entry: &McpServerEntry) -> bool {
    match &entry.transport {
        McpTransport::Stdio { command, .. } => check_command_exists(command),
        McpTransport::Http { url, .. } | McpTransport::Sse { url, .. } => {
            if !url_well_formed(url) {
                return false;
            }
            let Ok(server_config) = build_core_config(entry) else {
                return false;
            };
            discover_within(name, &server_config, LIST_AVAILABILITY_TIMEOUT).await
        }
    }
}

/// Attempts [`Introspector::discover_server`], bounding it to `timeout`.
///
/// Returns `false` on a timeout exactly as it would for any other discovery
/// error — `list` has no need to distinguish "too slow" from "refused" or
/// "unreachable", since [`transport_available`]'s doc already establishes
/// that a bounded `unavailable` here is not a definitive answer.
///
/// Extracted into its own function so tests can exercise the timeout branch
/// with a duration far shorter than [`LIST_AVAILABILITY_TIMEOUT`], without
/// waiting multiple real seconds.
async fn discover_within(name: &str, config: &ServerConfig, timeout: Duration) -> bool {
    tokio::time::timeout(
        timeout,
        Introspector::new().discover_server(ServerId::new(name), config),
    )
    .await
    .is_ok_and(|result| result.is_ok())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_server_status_as_str() {
        assert_eq!(ServerStatus::Available.as_str(), "available");
        assert_eq!(ServerStatus::Unavailable.as_str(), "unavailable");
    }

    #[test]
    fn test_build_command_string_no_args() {
        let entry = McpServerEntry {
            transport: McpTransport::Stdio {
                command: "node".to_string(),
                args: Vec::new(),
                env: HashMap::default(),
                cwd: None,
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };
        assert_eq!(build_command_string(&entry), "node");
    }

    #[test]
    fn test_build_command_string_with_args() {
        let entry = McpServerEntry {
            transport: McpTransport::Stdio {
                command: "node".to_string(),
                args: vec!["/path/to/server.js".to_string(), "--verbose".to_string()],
                env: HashMap::default(),
                cwd: None,
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };
        assert_eq!(
            build_command_string(&entry),
            "node /path/to/server.js --verbose"
        );
    }

    #[test]
    fn test_build_command_string_http() {
        let entry = McpServerEntry {
            transport: McpTransport::Http {
                url: "https://api.example.com/mcp".to_string(),
                headers: HashMap::default(),
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };
        assert_eq!(build_command_string(&entry), "https://api.example.com/mcp");
    }

    #[tokio::test]
    async fn test_transport_available_http_well_formed_but_unreachable_false() {
        // Regression test for #280 (S1): the issue's own repro was a
        // well-formed URL nothing listens on, and `list` reported it
        // "available" anyway. Well-formedness alone must no longer be
        // sufficient: `transport_available` now attempts real MCP
        // introspection, which fails to even connect here (nothing listens
        // on this port), so the entry must report unavailable.
        let entry = McpServerEntry {
            transport: McpTransport::Http {
                url: "http://127.0.0.1:1/mcp".to_string(),
                headers: HashMap::default(),
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };
        assert!(!transport_available("unreachable", &entry).await);
    }

    #[tokio::test]
    async fn test_discover_within_times_out_on_unresponsive_endpoint() {
        // Regression coverage for the `list`-latency fix: a single slow or
        // black-holed entry must not make `discover_within` (and therefore
        // `list`) wait out the entry's full configured connect/discover
        // timeout. 192.0.2.1 is TEST-NET-1 (RFC 5737), reserved for
        // documentation and guaranteed non-routable, so this never depends
        // on real network conditions: whether the attempt times out or fails
        // outright (e.g. "no route to host"), the result is `false` either
        // way — what this test actually pins down is that a short `timeout`
        // argument bounds the wait, so this never risks sleeping multiple
        // real seconds like the entry's own default timeouts would.
        let config = ServerConfig::builder()
            .http_transport("http://192.0.2.1:9/mcp".to_string())
            .build()
            .unwrap();

        let start = std::time::Instant::now();
        let available = discover_within("timeout-test", &config, Duration::from_millis(200)).await;
        let elapsed = start.elapsed();

        assert!(!available);
        assert!(
            elapsed < Duration::from_secs(2),
            "expected the short timeout to cut this attempt short, took {elapsed:?}"
        );
    }

    #[tokio::test]
    async fn test_transport_available_http_malformed_false() {
        let entry = McpServerEntry {
            transport: McpTransport::Http {
                url: "not-a-url".to_string(),
                headers: HashMap::default(),
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };
        assert!(!transport_available("badhttp", &entry).await);
    }

    #[tokio::test]
    async fn test_transport_available_sse_malformed_false() {
        let entry = McpServerEntry {
            transport: McpTransport::Sse {
                url: "ftp://example.com".to_string(),
                headers: HashMap::default(),
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };
        assert!(!transport_available("badsse", &entry).await);
    }

    #[test]
    fn test_url_well_formed_http_valid() {
        assert!(url_well_formed("http://example.com"));
    }

    #[test]
    fn test_url_well_formed_https_valid() {
        assert!(url_well_formed("https://example.com/mcp"));
    }

    #[test]
    fn test_url_well_formed_wrong_scheme_ftp() {
        assert!(!url_well_formed("ftp://example.com"));
    }

    #[test]
    fn test_url_well_formed_wrong_scheme_file() {
        // `file://` URLs parse without error but carry no host, so this is
        // also covered by the "no host" branch — kept as a separate case
        // since a wrong-scheme rejection and a missing-host rejection are
        // logically distinct failure modes that happen to coincide here.
        assert!(!url_well_formed("file:///etc/passwd"));
    }

    #[test]
    fn test_url_well_formed_rejects_leading_whitespace() {
        // Regression test for #280 (S2): the `url` crate strips leading
        // whitespace per WHATWG, but `mcp_execution_core::validate_url_scheme`
        // does not, so a whitespace-padded URL used to pass `list`'s check
        // while `server validate`/`generate` rejected the identical value
        // inside `build_core_config`. Delegating the scheme check to
        // `validate_url_scheme` closes that gap.
        assert!(!url_well_formed("  https://example.com/mcp"));
    }

    #[test]
    fn test_url_well_formed_rejects_trailing_control_chars() {
        // Same S2 divergence as the leading-whitespace case, but on the
        // trailing side and with a tab/newline rather than a plain space —
        // the exact combination the critic measured against `url` 2.5.8's
        // WHATWG-compliant trimming.
        assert!(!url_well_formed("\thttps://example.com/mcp\n"));
    }

    #[test]
    fn test_url_well_formed_no_host() {
        // Per the WHATWG URL spec, `http`/`https` require a non-empty
        // authority, so this fails to parse at all rather than parsing with
        // an empty host — `url_well_formed` must treat a parse failure the
        // same as a parsed-but-hostless URL.
        assert!(!url_well_formed("http://"));
    }

    #[test]
    fn test_url_well_formed_malformed_no_scheme() {
        assert!(!url_well_formed("not-a-url"));
    }

    #[test]
    fn test_url_well_formed_empty_string() {
        assert!(!url_well_formed(""));
    }

    #[test]
    fn test_check_command_exists() {
        assert!(check_command_exists("ls"));
        assert!(!check_command_exists(
            "this_command_definitely_does_not_exist_12345"
        ));
    }

    #[test]
    fn test_server_entry_serialization() {
        let entry = ServerEntry {
            id: "test".to_string(),
            command: "test-cmd".to_string(),
            status: "available".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("test-cmd"));
        assert!(json.contains("available"));
    }

    #[test]
    fn test_server_list_serialization() {
        let list = ServerList {
            servers: vec![ServerEntry {
                id: "test".to_string(),
                command: "test-cmd".to_string(),
                status: "available".to_string(),
            }],
        };

        let json = serde_json::to_string(&list).unwrap();
        assert!(json.contains("servers"));
        assert!(json.contains("test"));
    }

    #[test]
    fn test_server_info_serialization() {
        let info = ServerInfo {
            id: "test".to_string(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            command: "test-cmd".to_string(),
            status: "available".to_string(),
            tools: vec![ToolSummary {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
            }],
            capabilities: vec!["tools".to_string()],
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("Test Server"));
        assert!(json.contains("capabilities"));
        assert!(json.contains("tools"));
    }

    #[test]
    fn test_tool_summary_serialization() {
        let tool = ToolSummary {
            name: "send_message".to_string(),
            description: "Sends a message".to_string(),
        };

        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("send_message"));
        assert!(json.contains("Sends a message"));
    }

    /// Serializes tests that mutate the `HOME` env var so they cannot race
    /// each other when run in the same process (e.g. under plain `cargo
    /// test`, unlike `cargo nextest`, which isolates each test in its own
    /// process). An async-aware mutex, since the guard must stay held across
    /// the `.await` of the code under test.
    #[cfg(unix)]
    static HOME_ENV_LOCK: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    // Unix-only: redirects `dirs::home_dir()` by mutating `HOME`, which
    // `dirs` only consults on Unix. On Windows `dirs::home_dir()` resolves
    // via `SHGetKnownFolderPath(FOLDERID_Profile)`, a Win32 API that reads
    // the real OS user profile and ignores environment variables entirely
    // — no env var override can redirect it.
    #[cfg(unix)]
    #[tokio::test]
    async fn test_show_server_info_not_found_error_not_duplicated() {
        // Regression test for #164: the "not found" message from
        // `get_mcp_server` must propagate unwrapped, not get re-wrapped by
        // an equivalent, less complete `with_context` in `show_server_info`.
        //
        // Hermetic: HOME is pointed at a temp dir with a controlled
        // mcp.json that defines an unrelated server, so the "not found"
        // branch inside `get_mcp_server` is deterministically reached
        // regardless of the executing machine's real HOME. Without this, a
        // clean CI runner with no `~/.claude/mcp.json` at all would instead
        // fail earlier at the "read config file" step, and the regression
        // this test exists to catch would never actually be exercised.
        let _guard = HOME_ENV_LOCK.lock().await;

        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(
            claude_dir.join("mcp.json"),
            r#"{"mcpServers": {"unrelated": {"command": "node"}}}"#,
        )
        .unwrap();

        let original_home = std::env::var_os("HOME");
        // SAFETY: guarded by `HOME_ENV_LOCK`; no other test in this process
        // reads or writes `HOME` while the guard is held.
        unsafe {
            std::env::set_var("HOME", temp.path());
        }

        let result = run(
            ServerAction::Info {
                server: "nonexistent-server".to_string(),
            },
            OutputFormat::Json,
        )
        .await;

        // SAFETY: see above.
        unsafe {
            match &original_home {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
        }

        assert!(result.is_err());
        let message = format!("{:#}", result.unwrap_err());
        assert_eq!(
            message.matches("not found in ~/.claude/mcp.json").count(),
            1,
            "expected exactly one not-found message in the error chain, got: {message}"
        );
    }

    /// Points `dirs::home_dir()` at `home_dir` for the duration of the
    /// closure, serialized against other `HOME`-mutating tests via
    /// `HOME_ENV_LOCK`, and restores the original value afterwards even if
    /// the closure's future returns an error.
    #[cfg(unix)]
    async fn with_home_pointed_at<F, Fut, T>(home_dir: &std::path::Path, f: F) -> T
    where
        F: FnOnce() -> Fut,
        Fut: std::future::Future<Output = T>,
    {
        let _guard = HOME_ENV_LOCK.lock().await;

        let original_home = std::env::var_os("HOME");
        // SAFETY: guarded by `HOME_ENV_LOCK`; no other test in this process
        // reads or writes `HOME` while the guard is held.
        unsafe {
            std::env::set_var("HOME", home_dir);
        }

        let result = f().await;

        // SAFETY: see above.
        unsafe {
            match &original_home {
                Some(home) => std::env::set_var("HOME", home),
                None => std::env::remove_var("HOME"),
            }
        }

        result
    }

    /// Writes `mcp.json` with the given raw content under a fresh temp dir's
    /// `.claude/` subdirectory, returning the temp dir (kept alive by the
    /// caller for the test's duration).
    #[cfg(unix)]
    fn write_test_mcp_config(content: &str) -> tempfile::TempDir {
        let temp = tempfile::TempDir::new().unwrap();
        let claude_dir = temp.path().join(".claude");
        std::fs::create_dir_all(&claude_dir).unwrap();
        std::fs::write(claude_dir.join("mcp.json"), content).unwrap();
        temp
    }

    // Regression coverage for #280: `validate_command`'s pre-introspection
    // check used to unconditionally treat http/sse transports as available,
    // so a malformed URL would only be caught deep inside introspection (or
    // not at all). These exercise `run(ServerAction::Validate { .. })`
    // end-to-end against a real temp `mcp.json`, asserting on the returned
    // `ExitCode` since the human-readable message is only ever printed to
    // stdout, which this crate has no harness to capture (see handoff notes).

    #[cfg(unix)]
    #[tokio::test]
    async fn test_validate_command_malformed_http_url_early_exit() {
        let temp = write_test_mcp_config(
            r#"{"mcpServers": {"badhttp": {"type": "http", "url": "not-a-url"}}}"#,
        );

        let result = with_home_pointed_at(temp.path(), || {
            run(
                ServerAction::Validate {
                    command: "badhttp".to_string(),
                },
                OutputFormat::Json,
            )
        })
        .await;

        assert_eq!(result.unwrap(), ExitCode::ERROR);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_validate_command_malformed_sse_url_early_exit() {
        let temp = write_test_mcp_config(
            r#"{"mcpServers": {"badsse": {"type": "sse", "url": "ftp://example.com"}}}"#,
        );

        let result = with_home_pointed_at(temp.path(), || {
            run(
                ServerAction::Validate {
                    command: "badsse".to_string(),
                },
                OutputFormat::Json,
            )
        })
        .await;

        assert_eq!(result.unwrap(), ExitCode::ERROR);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_validate_command_stdio_missing_command_early_exit() {
        let temp = write_test_mcp_config(
            r#"{"mcpServers": {"badstdio": {"command": "this_command_definitely_does_not_exist_12345"}}}"#,
        );

        let result = with_home_pointed_at(temp.path(), || {
            run(
                ServerAction::Validate {
                    command: "badstdio".to_string(),
                },
                OutputFormat::Json,
            )
        })
        .await;

        assert_eq!(result.unwrap(), ExitCode::ERROR);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_validate_command_well_formed_but_unreachable_http_url_fails_post_introspection() {
        // Well-formed per `url_well_formed` (http scheme, present host), so
        // this skips the early-exit branch and reaches real introspection,
        // which fails because nothing listens on port 1 (a privileged port
        // no test server binds to) — proving the http/sse post-introspection
        // failure branch is reachable, not dead code.
        let temp = write_test_mcp_config(
            r#"{"mcpServers": {"unreachable": {"type": "http", "url": "http://127.0.0.1:1/mcp"}}}"#,
        );

        let result = with_home_pointed_at(temp.path(), || {
            run(
                ServerAction::Validate {
                    command: "unreachable".to_string(),
                },
                OutputFormat::Json,
            )
        })
        .await;

        assert_eq!(result.unwrap(), ExitCode::ERROR);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_validate_command_scheme_failure_does_not_reach_precheck_bypass() {
        // Regression test for #304: an entry that passes the `url_well_formed` precheck (a
        // syntactically fine https URL with a host) but fails `build_core_config`'s deeper
        // security validation (here: a zero connect timeout) must still resolve as
        // "entry present, invalid configuration" — not silently skip validation and proceed to
        // introspection, and not report a "not found" message either.
        let temp = write_test_mcp_config(
            r#"{"mcpServers": {"badtimeout": {"type": "http", "url": "https://example.com/mcp", "connectTimeoutSecs": 0}}}"#,
        );

        let result = with_home_pointed_at(temp.path(), || {
            run(
                ServerAction::Validate {
                    command: "badtimeout".to_string(),
                },
                OutputFormat::Json,
            )
        })
        .await;

        assert_eq!(result.unwrap(), ExitCode::ERROR);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn test_show_server_info_invalid_url_scheme_reports_structured_unavailable_not_raw_error()
    {
        // Regression test for #305: `server info` on an entry whose `url` fails scheme
        // validation must return the structured `"status": "unavailable"` `ServerInfo` output
        // through the normal `ExitCode` path, like the well-formed-but-unreachable case — not
        // propagate a raw, unformatted `anyhow` error via `?`.
        let temp = write_test_mcp_config(
            r#"{"mcpServers": {"http-malformed": {"type": "http", "url": "not-a-url"}}}"#,
        );

        let result = with_home_pointed_at(temp.path(), || {
            run(
                ServerAction::Info {
                    server: "http-malformed".to_string(),
                },
                OutputFormat::Json,
            )
        })
        .await;

        assert_eq!(
            result.expect("must return Ok(ExitCode::ERROR), not propagate a raw Err"),
            ExitCode::ERROR
        );
    }

    #[test]
    fn test_unavailable_server_info_reports_unavailable_status() {
        // Direct coverage for #305's structured-body claim: `show_server_info`'s invalid-config
        // and failed-introspection branches both build the reported `ServerInfo` through this
        // helper, so asserting its output here confirms the JSON body actually carries
        // `"status": "unavailable"` — the ExitCode-only end-to-end tests above cannot observe
        // this crate's `println!`-only output (see `with_home_pointed_at` test comments).
        let info = unavailable_server_info("http-malformed".to_string(), "curl".to_string());

        assert_eq!(info.status, ServerStatus::Unavailable.as_str());
        assert_eq!(info.id, "http-malformed");
        assert_eq!(info.name, "http-malformed");
        assert!(info.tools.is_empty());
        assert!(info.capabilities.is_empty());

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("\"status\":\"unavailable\""));
    }

    #[test]
    fn test_validation_result_serialization() {
        let result = ValidationResult {
            command: "test".to_string(),
            valid: true,
            message: "ok".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("command"));
        assert!(json.contains("valid"));
        assert!(json.contains("message"));
    }
}
