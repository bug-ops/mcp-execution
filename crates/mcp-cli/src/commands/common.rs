//! Common utilities shared across CLI commands.
//!
//! Provides shared functionality for building server configurations from CLI arguments
//! and loading MCP server definitions from `~/.claude/mcp.json`.

use anyhow::{Context, Result, bail};
use mcp_execution_core::{Error as CoreError, ServerConfig, ServerConfigBuilder, ServerId};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tracing::warn;
use url::Url;

/// Fallback slug used when a URL sanitizes down to nothing (e.g. no host).
const FALLBACK_SERVER_ID_SLUG: &str = "http-server";

/// MCP configuration file structure (`~/.claude/mcp.json`).
///
/// The `mcp_servers` field defaults to an empty map so that an absent file or
/// a file containing only `{}` does not produce a deserialization error.
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct McpConfig {
    /// Map of server name → server configuration entry.
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerEntry>,
}

/// Canonical in-crate representation of an MCP server's transport.
///
/// This is the single source of truth for "stdio vs http vs sse", shared by
/// both the `mcp.json` config path ([`McpServerEntry`]) and the CLI-flag path
/// ([`TransportArgs`] converts into this via `TryFrom`).
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::common::McpTransport;
/// use std::collections::HashMap;
///
/// let transport = McpTransport::Http {
///     url: "https://api.example.com/mcp".to_string(),
///     headers: HashMap::new(),
/// };
/// assert!(matches!(transport, McpTransport::Http { .. }));
/// ```
#[derive(Debug, Clone)]
pub enum McpTransport {
    /// Stdio transport: spawn a subprocess and speak MCP over stdin/stdout.
    Stdio {
        /// Command to execute (binary name or absolute path).
        command: String,
        /// Arguments to pass to the command.
        args: Vec<String>,
        /// Environment variables for the server process.
        env: HashMap<String, String>,
        /// Working directory for the server process.
        cwd: Option<PathBuf>,
    },
    /// Streamable HTTP transport.
    Http {
        /// Server endpoint URL.
        url: String,
        /// HTTP headers sent with every request (e.g. `Authorization`).
        headers: HashMap<String, String>,
    },
    /// Server-Sent Events transport.
    Sse {
        /// Server endpoint URL.
        url: String,
        /// HTTP headers sent with every request (e.g. `Authorization`).
        headers: HashMap<String, String>,
    },
}

/// Individual MCP server configuration entry from `mcp.json`.
#[derive(Debug, Clone)]
pub struct McpServerEntry {
    /// The server's transport and its transport-specific settings.
    pub transport: McpTransport,
    /// Connection (handshake) timeout in seconds, overriding the 30-second
    /// default when set. JSON key: `connectTimeoutSecs`.
    pub connect_timeout_secs: Option<u64>,
    /// Tool discovery timeout in seconds, overriding the 30-second default
    /// when set. JSON key: `discoverTimeoutSecs`.
    pub discover_timeout_secs: Option<u64>,
}

/// Discriminant for the optional `"type"` field in an `mcp.json` server entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
enum TransportTag {
    Stdio,
    Http,
    Sse,
}

impl TransportTag {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Stdio => "stdio",
            Self::Http => "http",
            Self::Sse => "sse",
        }
    }
}

/// Flat, all-optional serde landing zone for a raw `mcp.json` server entry.
///
/// Every field is optional so that stdio, http, and sse shapes can share one
/// deserialization pass; [`McpServerEntry`]'s manual `Deserialize` converts
/// this via `TryFrom` and raises precise, field-naming errors for
/// cross-field violations that a derived `Deserialize` can't express (e.g.
/// "http entries must not set `command`"). Unknown keys land in `extra`
/// rather than hard-failing, since `~/.claude/mcp.json` is shared with other
/// MCP clients that store keys this project doesn't model (`disabled`,
/// `alwaysAllow`, ...).
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase", rename = "McpServerEntry")]
struct RawMcpServerEntry {
    #[serde(rename = "type")]
    transport_type: Option<TransportTag>,
    command: Option<String>,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
    cwd: Option<String>,
    url: Option<String>,
    #[serde(default)]
    headers: HashMap<String, String>,
    connect_timeout_secs: Option<u64>,
    discover_timeout_secs: Option<u64>,
    #[serde(flatten)]
    extra: HashMap<String, serde_json::Value>,
}

/// Resolves the `url`/`command` pair for an http-like (`http` or `sse`)
/// transport tag, rejecting a `command` field and requiring `url`.
fn http_like_transport(
    tag_name: &str,
    command: Option<&str>,
    url: Option<String>,
    headers: HashMap<String, String>,
) -> Result<(String, HashMap<String, String>), String> {
    if command.is_some() {
        return Err(format!("{tag_name} server entry must not set \"command\""));
    }
    let url = url.ok_or_else(|| format!("{tag_name} server entry requires \"url\""))?;
    Ok((url, headers))
}

impl TryFrom<RawMcpServerEntry> for McpServerEntry {
    type Error = String;

    fn try_from(raw: RawMcpServerEntry) -> Result<Self, Self::Error> {
        if !raw.extra.is_empty() {
            let mut keys: Vec<&str> = raw.extra.keys().map(String::as_str).collect();
            keys.sort_unstable();
            warn!(
                "mcp.json server entry has unrecognized field(s), ignoring: {}",
                keys.join(", ")
            );
        }

        let tag = match raw.transport_type {
            Some(tag) => tag,
            None if raw.command.is_some() => TransportTag::Stdio,
            None if raw.url.is_some() => TransportTag::Http,
            None => {
                return Err(
                    "server entry must set either \"command\" (stdio) or \"type\" and \"url\" \
                     (http/sse)"
                        .to_string(),
                );
            }
        };

        let transport = match tag {
            TransportTag::Stdio => {
                if raw.url.is_some() {
                    return Err("stdio server entry must not set \"url\"".to_string());
                }
                let command = raw
                    .command
                    .ok_or_else(|| "stdio server entry requires \"command\"".to_string())?;
                McpTransport::Stdio {
                    command,
                    args: raw.args,
                    env: raw.env,
                    cwd: raw.cwd.map(PathBuf::from),
                }
            }
            TransportTag::Http => {
                let (url, headers) = http_like_transport(
                    tag.as_str(),
                    raw.command.as_deref(),
                    raw.url,
                    raw.headers,
                )?;
                McpTransport::Http { url, headers }
            }
            TransportTag::Sse => {
                let (url, headers) = http_like_transport(
                    tag.as_str(),
                    raw.command.as_deref(),
                    raw.url,
                    raw.headers,
                )?;
                McpTransport::Sse { url, headers }
            }
        };

        Ok(Self {
            transport,
            connect_timeout_secs: raw.connect_timeout_secs,
            discover_timeout_secs: raw.discover_timeout_secs,
        })
    }
}

impl<'de> Deserialize<'de> for McpServerEntry {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let raw = RawMcpServerEntry::deserialize(deserializer)?;
        raw.try_into().map_err(serde::de::Error::custom)
    }
}

/// Loads MCP configuration from the given path.
///
/// This is the primary, testable entry point. [`load_mcp_config`] is a thin
/// wrapper that resolves the default `~/.claude/mcp.json` location.
///
/// # Errors
///
/// Returns an error if the file cannot be read or the JSON is malformed.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::common::load_mcp_config_from;
/// use std::path::Path;
///
/// let config = load_mcp_config_from(Path::new("/tmp/mcp.json")).unwrap();
/// println!("{} servers configured", config.mcp_servers.len());
/// ```
pub fn load_mcp_config_from(path: &Path) -> Result<McpConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read MCP config from {}", path.display()))?;

    serde_json::from_str(&content).context("failed to parse MCP config JSON")
}

/// Loads MCP configuration from `~/.claude/mcp.json`.
///
/// Delegates to [`load_mcp_config_from`] after resolving the default path.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined, the file
/// cannot be read, or the JSON is malformed.
pub fn load_mcp_config() -> Result<McpConfig> {
    let home = dirs::home_dir().context("failed to get home directory")?;
    load_mcp_config_from(&home.join(".claude").join("mcp.json"))
}

/// Lists all servers defined in the given `mcp.json` file.
///
/// Returns an empty list when the file does not exist — the primary testable
/// entry point for the "fresh machine" code path (no config file yet).
///
/// # Errors
///
/// Returns an error if the file exists but cannot be read or parsed.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::common::list_mcp_servers_from;
/// use std::path::Path;
///
/// let servers = list_mcp_servers_from(Path::new("/tmp/mcp.json")).unwrap();
/// println!("{} servers", servers.len());
/// ```
pub fn list_mcp_servers_from(path: &Path) -> Result<Vec<(String, McpServerEntry)>> {
    if !path.exists() {
        return Ok(Vec::new());
    }
    let config = load_mcp_config_from(path)?;
    Ok(config.mcp_servers.into_iter().collect())
}

/// Lists all servers defined in `~/.claude/mcp.json`.
///
/// Returns an empty list when the config file does not exist so that
/// `server list` shows a clear empty result rather than hard-failing.
///
/// Delegates to [`list_mcp_servers_from`] after resolving the default path.
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined, or the config
/// file exists but cannot be read or parsed.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::common::list_mcp_servers;
///
/// for (name, entry) in list_mcp_servers().unwrap() {
///     println!("{}: {:?}", name, entry.transport);
/// }
/// ```
pub fn list_mcp_servers() -> Result<Vec<(String, McpServerEntry)>> {
    let home = dirs::home_dir().context("failed to get home directory")?;
    list_mcp_servers_from(&home.join(".claude").join("mcp.json"))
}

/// Retrieves a named server from `~/.claude/mcp.json`.
///
/// # Arguments
///
/// * `name` - Server name as defined under `mcpServers` in `mcp.json`
///
/// # Returns
///
/// A tuple of `(ServerId, ServerConfig, McpServerEntry)`:
/// - [`ServerId`] — typed server identifier
/// - [`ServerConfig`] — ready-to-use connection config for `Introspector`
/// - [`McpServerEntry`] — raw entry for display purposes
///
/// # Errors
///
/// Returns an error if the config file is missing, malformed, or the named
/// server is not present.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::common::get_mcp_server;
///
/// let (id, _config, _entry) = get_mcp_server("github").unwrap();
/// assert_eq!(id.as_str(), "github");
/// ```
pub fn get_mcp_server(name: &str) -> Result<(ServerId, ServerConfig, McpServerEntry)> {
    let config = load_mcp_config()?;

    let entry = config
        .mcp_servers
        .get(name)
        .with_context(|| {
            format!(
                "server '{name}' not found in ~/.claude/mcp.json\n\
                 Hint: ensure the server is defined in ~/.claude/mcp.json under \"mcpServers\""
            )
        })?
        .clone();

    let server_config = build_core_config(&entry);
    Ok((ServerId::new(name), server_config, entry))
}

/// Loads server configuration from `~/.claude/mcp.json` by server name.
///
/// Convenience wrapper around [`get_mcp_server`] that drops the raw entry.
///
/// # Arguments
///
/// * `name` - Server name from `mcp.json` (e.g., `"github"`)
///
/// # Errors
///
/// Returns an error if the config file is missing, malformed, or the server
/// name is not present.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::common::load_server_from_config;
///
/// let (id, config) = load_server_from_config("github").unwrap();
/// assert_eq!(id.as_str(), "github");
/// ```
pub fn load_server_from_config(name: &str) -> Result<(ServerId, ServerConfig)> {
    let (id, config, _) = get_mcp_server(name)?;
    Ok((id, config))
}

/// Applies transport-specific settings onto a fresh [`ServerConfig`] builder.
///
/// The single place where [`ServerConfig::builder()`] is invoked; both the
/// `mcp.json` path ([`build_core_config`]) and the CLI-flag path
/// ([`build_server_config`]) funnel through this.
fn builder_for_transport(transport: McpTransport) -> ServerConfigBuilder {
    match transport {
        McpTransport::Stdio {
            command,
            args,
            env,
            cwd,
        } => {
            let mut builder = ServerConfig::builder().command(command);
            if !args.is_empty() {
                builder = builder.args(args);
            }
            for (key, value) in env {
                builder = builder.env(key, value);
            }
            if let Some(dir) = cwd {
                builder = builder.cwd(dir);
            }
            builder
        }
        McpTransport::Http { url, headers } => {
            let mut builder = ServerConfig::builder().http_transport(url);
            for (key, value) in headers {
                builder = builder.header(key, value);
            }
            builder
        }
        McpTransport::Sse { url, headers } => {
            let mut builder = ServerConfig::builder().sse_transport(url);
            for (key, value) in headers {
                builder = builder.header(key, value);
            }
            builder
        }
    }
}

/// Builds a core [`ServerConfig`] from an [`McpServerEntry`].
fn build_core_config(entry: &McpServerEntry) -> ServerConfig {
    let mut builder = builder_for_transport(entry.transport.clone());

    if let Some(secs) = entry.connect_timeout_secs {
        builder = builder.connect_timeout(Duration::from_secs(secs));
    }

    if let Some(secs) = entry.discover_timeout_secs {
        builder = builder.discover_timeout(Duration::from_secs(secs));
    }

    builder.build()
}

/// Parses a single `KEY=VALUE` CLI argument (used for `--env` and `--header`).
///
/// Security: `s` routinely carries secrets (tokens, API keys) in the value
/// portion, so it must never be echoed into an error message verbatim —
/// mirrors the discipline in `mcp_execution_core::command::validate_header_value_string`.
/// Every error here is a `CoreError::InvalidArgument` (rather than a bare
/// anyhow string) so it classifies as `ExitCode::INVALID_INPUT` downstream
/// in `runner::classify_exit_code`.
fn parse_key_value(s: &str, kind: &str) -> Result<(String, String)> {
    // No `=` at all: the whole string could itself be the secret with no
    // discernible key, so it is never echoed, not even its length (which
    // would narrow the secret's type/format for free in CI logs).
    let Some((key, value)) = s.split_once('=') else {
        return Err(CoreError::InvalidArgument(format!(
            "invalid {kind} format: no '=' separator found (expected KEY=VALUE)"
        ))
        .into());
    };
    if key.is_empty() {
        return Err(CoreError::InvalidArgument(format!(
            "invalid {kind} format: key cannot be empty (expected KEY=VALUE)"
        ))
        .into());
    }
    // A real header/env key never legitimately contains whitespace, `:`, or
    // control characters. Their presence is the signature of the `=` having
    // matched somewhere inside the value instead of acting as the separator
    // — e.g. a header written `Name: Value` by mistake, where the value
    // happens to contain `=` (base64 padding, a JWT). Reject without echoing
    // `key`, since in that scenario it *is* the secret.
    if key
        .chars()
        .any(|c| c.is_whitespace() || c == ':' || c.is_control())
    {
        return Err(CoreError::InvalidArgument(format!(
            "invalid {kind} format: text before '=' contains characters that are never valid \
             in a key, suggesting '=' matched inside a value rather than as the separator; \
             refusing to echo it since it may contain a secret (expected KEY=VALUE)"
        ))
        .into());
    }
    Ok((key.to_string(), value.to_string()))
}

/// CLI-flag mirror of [`McpTransport`], holding the raw, unvalidated
/// `Option`/`Vec<String>` values clap hands back.
///
/// [`TransportArgs::from_flags`] is the single place that enforces "exactly
/// one transport selected". `TryFrom<TransportArgs> for McpTransport` does
/// the `KEY=VALUE` parsing for environment variables and headers.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::common::TransportArgs;
///
/// let transport = TransportArgs::Stdio {
///     command: "github-mcp-server".to_string(),
///     args: vec!["stdio".to_string()],
///     env: vec![],
///     cwd: None,
/// };
/// assert!(matches!(transport, TransportArgs::Stdio { .. }));
/// ```
#[derive(Debug, Clone)]
pub enum TransportArgs {
    /// Stdio transport (default): raw CLI flags.
    Stdio {
        /// Command to execute (binary name or path).
        command: String,
        /// Arguments to pass to the command.
        args: Vec<String>,
        /// Environment variables in `KEY=VALUE` format.
        env: Vec<String>,
        /// Working directory for the server process.
        cwd: Option<String>,
    },
    /// HTTP transport: raw CLI flags.
    Http {
        /// Server endpoint URL.
        url: String,
        /// HTTP headers in `KEY=VALUE` format.
        headers: Vec<String>,
    },
    /// SSE transport: raw CLI flags.
    Sse {
        /// Server endpoint URL.
        url: String,
        /// HTTP headers in `KEY=VALUE` format.
        headers: Vec<String>,
    },
}

impl TransportArgs {
    /// Builds a [`TransportArgs`] from the CLI's flat `--http`/`--sse`/positional
    /// flag surface, enforcing that exactly one transport was selected.
    ///
    /// # Errors
    ///
    /// Returns an error if both `http` and `sse` are set, or if none of
    /// `server`, `http`, or `sse` is set. Clap's `conflicts_with` /
    /// `required_unless_present_any` already prevent both cases when parsing
    /// real CLI input; this is the safety net for callers that build
    /// `TransportArgs` directly (e.g. as a library).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::commands::common::TransportArgs;
    ///
    /// let transport = TransportArgs::from_flags(
    ///     Some("github-mcp-server".to_string()),
    ///     vec!["stdio".to_string()],
    ///     vec![],
    ///     None,
    ///     None,
    ///     None,
    ///     vec![],
    /// )
    /// .unwrap();
    /// assert!(matches!(transport, TransportArgs::Stdio { .. }));
    ///
    /// let err = TransportArgs::from_flags(None, vec![], vec![], None, None, None, vec![]);
    /// assert!(err.is_err());
    /// ```
    #[allow(clippy::too_many_arguments)] // mirrors the CLI's flat argument surface; grouping would add an abstraction for no behavioral benefit
    pub fn from_flags(
        server: Option<String>,
        args: Vec<String>,
        env: Vec<String>,
        cwd: Option<String>,
        http: Option<String>,
        sse: Option<String>,
        headers: Vec<String>,
    ) -> Result<Self> {
        match (http, sse) {
            (Some(_), Some(_)) => bail!("cannot use both --http and --sse transports"),
            (Some(url), None) => Ok(Self::Http { url, headers }),
            (None, Some(url)) => Ok(Self::Sse { url, headers }),
            (None, None) => {
                let command = server.context(
                    "server command is required for stdio transport (or use --http/--sse)",
                )?;
                Ok(Self::Stdio {
                    command,
                    args,
                    env,
                    cwd,
                })
            }
        }
    }
}

impl TryFrom<TransportArgs> for McpTransport {
    type Error = anyhow::Error;

    fn try_from(args: TransportArgs) -> Result<Self> {
        match args {
            TransportArgs::Stdio {
                command,
                args,
                env,
                cwd,
            } => {
                let env = env
                    .iter()
                    .map(|s| parse_key_value(s, "environment variable"))
                    .collect::<Result<HashMap<_, _>>>()?;
                Ok(Self::Stdio {
                    command,
                    args,
                    env,
                    cwd: cwd.map(PathBuf::from),
                })
            }
            TransportArgs::Http { url, headers } => {
                let headers = headers
                    .iter()
                    .map(|s| parse_key_value(s, "header"))
                    .collect::<Result<HashMap<_, _>>>()?;
                Ok(Self::Http { url, headers })
            }
            TransportArgs::Sse { url, headers } => {
                let headers = headers
                    .iter()
                    .map(|s| parse_key_value(s, "header"))
                    .collect::<Result<HashMap<_, _>>>()?;
                Ok(Self::Sse { url, headers })
            }
        }
    }
}

/// Builds `ServerConfig` from CLI transport arguments.
///
/// # Arguments
///
/// * `transport` - The selected transport and its raw CLI flags; build with
///   [`TransportArgs::from_flags`].
/// * `connect_timeout_secs` - Connection (handshake) timeout override, in
///   seconds. Same semantics as `mcp.json`'s `connectTimeoutSecs`: must be
///   greater than zero and at most 600 seconds, enforced by
///   [`validate_server_config`](mcp_execution_core::validate_server_config)
///   at connect time.
/// * `discover_timeout_secs` - Tool discovery timeout override, in seconds.
///   Same semantics as `mcp.json`'s `discoverTimeoutSecs`.
///
/// # Errors
///
/// Returns an error if environment variables or headers are not in
/// `KEY=VALUE` format.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::common::{TransportArgs, build_server_config};
///
/// let transport = TransportArgs::from_flags(
///     Some("github-mcp-server".to_string()),
///     vec!["stdio".to_string()],
///     vec!["TOKEN=abc".to_string()],
///     None,
///     None,
///     None,
///     vec![],
/// )
/// .unwrap();
///
/// let (id, config) = build_server_config(transport, None, None).unwrap();
///
/// assert_eq!(id.as_str(), "github-mcp-server");
/// assert_eq!(config.args(), &["stdio"]);
/// ```
pub fn build_server_config(
    transport: TransportArgs,
    connect_timeout_secs: Option<u64>,
    discover_timeout_secs: Option<u64>,
) -> Result<(ServerId, ServerConfig)> {
    let server_id = match &transport {
        TransportArgs::Stdio { command, .. } => ServerId::new(command),
        TransportArgs::Http { url, .. } | TransportArgs::Sse { url, .. } => {
            derive_server_id_from_url(url)
        }
    };

    let mut builder = builder_for_transport(McpTransport::try_from(transport)?);

    if let Some(secs) = connect_timeout_secs {
        builder = builder.connect_timeout(Duration::from_secs(secs));
    }

    if let Some(secs) = discover_timeout_secs {
        builder = builder.discover_timeout(Duration::from_secs(secs));
    }

    Ok((server_id, builder.build()))
}

/// Derives a filesystem- and `validate_server_id`-safe [`ServerId`] slug from
/// an Http/Sse transport URL.
///
/// Using the raw URL as the id (the previous behavior) is unsafe once Http/Sse
/// configs can actually reach `generate`: the id flows into a directory name
/// under `~/.claude/servers/{id}/` and into generated `tool.ts` literals, so a
/// raw URL there breaks `mcp_execution_skill::validate_server_id`'s
/// lowercase/digit/hyphen requirement, can smuggle `..` path segments through
/// `PathBuf::join`, and — if the URL carries `user:token@host` userinfo —
/// leaks the credential into a directory name and generated source.
///
/// Only `host` and `path` are used (never `userinfo`, so credentials are
/// structurally excluded). The result is lowercased, every run of characters
/// outside `[a-z0-9-]` collapses to a single `-`, and leading/trailing `-` are
/// trimmed. Falls back to [`FALLBACK_SERVER_ID_SLUG`] if the URL fails to
/// parse or the result would otherwise be empty (e.g. a bare `https://` URL
/// with no host). The slug is truncated to fit `validate_server_id`'s length
/// limit.
fn derive_server_id_from_url(url: &str) -> ServerId {
    // Mirrors the private `mcp_execution_skill::types::MAX_SERVER_ID_LENGTH`;
    // duplicated here since that constant isn't exported, and enforced by
    // this module's tests calling `mcp_execution_skill::validate_server_id`
    // on the derived slug.
    const MAX_SERVER_ID_LENGTH: usize = 64;

    // On parse failure, fall through to the empty-slug case below rather than
    // sanitizing the raw string: a URL that failed to parse is about to be
    // rejected by `validate_url_scheme`/the connection attempt anyway, and
    // preserving any part of it here would defeat the credential-exclusion
    // guarantee above for inputs like `https://user:pass@evil.com:99999/x`
    // (a mistyped port is a realistic `Url::parse` failure, not just an
    // adversarial one).
    let host_and_path = Url::parse(url)
        .ok()
        .map(|parsed| format!("{}{}", parsed.host_str().unwrap_or_default(), parsed.path()))
        .unwrap_or_default();

    let mut slug = String::with_capacity(host_and_path.len());
    for ch in host_and_path.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_lowercase() || lower.is_ascii_digit() {
            slug.push(lower);
        } else if slug.chars().next_back().is_some_and(|last| last != '-') {
            slug.push('-');
        }
    }

    let slug = slug.trim_matches('-');
    let slug = &slug[..slug.len().min(MAX_SERVER_ID_LENGTH)];
    let slug = slug.trim_end_matches('-');

    ServerId::new(if slug.is_empty() {
        FALLBACK_SERVER_ID_SLUG
    } else {
        slug
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Creates a temporary mcp.json file for testing.
    fn create_test_config(content: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    fn stdio_transport(
        command: &str,
        args: Vec<&str>,
        env: Vec<&str>,
        cwd: Option<&str>,
    ) -> TransportArgs {
        TransportArgs::Stdio {
            command: command.to_string(),
            args: args.into_iter().map(String::from).collect(),
            env: env.into_iter().map(String::from).collect(),
            cwd: cwd.map(String::from),
        }
    }

    fn http_transport(url: &str, headers: Vec<&str>) -> TransportArgs {
        TransportArgs::Http {
            url: url.to_string(),
            headers: headers.into_iter().map(String::from).collect(),
        }
    }

    fn sse_transport(url: &str, headers: Vec<&str>) -> TransportArgs {
        TransportArgs::Sse {
            url: url.to_string(),
            headers: headers.into_iter().map(String::from).collect(),
        }
    }

    #[test]
    fn test_load_mcp_config_from_valid() {
        let json = r#"{"mcpServers": {"github": {"command": "node", "args": ["server.js"]}}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        assert!(config.mcp_servers.contains_key("github"));
    }

    #[test]
    fn test_load_mcp_config_from_empty_servers() {
        // mcp_servers defaults to empty map when key is absent
        let json = r"{}";
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        assert!(config.mcp_servers.is_empty());
    }

    #[test]
    fn test_load_mcp_config_from_minimal_server() {
        // Server with only command (args and env should default), no "type" key
        let json = r#"{"mcpServers": {"minimal": {"command": "python"}}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        let entry = &config.mcp_servers["minimal"];
        match &entry.transport {
            McpTransport::Stdio {
                command, args, env, ..
            } => {
                assert_eq!(command, "python");
                assert!(args.is_empty());
                assert!(env.is_empty());
            }
            other => panic!("expected Stdio transport, got {other:?}"),
        }
    }

    #[test]
    fn test_load_mcp_config_from_multiple_servers() {
        let json = r#"{
            "mcpServers": {
                "server1": {"command": "node", "args": ["s1.js"]},
                "server2": {"command": "python", "args": ["s2.py"]}
            }
        }"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        assert_eq!(config.mcp_servers.len(), 2);
        assert!(config.mcp_servers.contains_key("server1"));
        assert!(config.mcp_servers.contains_key("server2"));
    }

    #[test]
    fn test_load_mcp_config_from_not_found() {
        let result = load_mcp_config_from(Path::new("/nonexistent/path/mcp.json"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("failed to read"));
    }

    #[test]
    fn test_load_mcp_config_from_malformed_json() {
        let file = create_test_config("not valid json");
        let result = load_mcp_config_from(file.path());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("parse MCP config"));
    }

    // ── mixed stdio/http/sse configs (#210) ──

    #[test]
    fn test_load_mcp_config_mixed_stdio_http_sse() {
        let json = r#"{
            "mcpServers": {
                "local": {"command": "node", "args": ["server.js"]},
                "remote-http": {"type": "http", "url": "https://api.example.com/mcp", "headers": {"Authorization": "Bearer x"}},
                "remote-sse": {"type": "sse", "url": "https://example.com/sse"}
            }
        }"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        assert_eq!(config.mcp_servers.len(), 3);

        assert!(matches!(
            config.mcp_servers["local"].transport,
            McpTransport::Stdio { .. }
        ));
        assert!(matches!(
            config.mcp_servers["remote-http"].transport,
            McpTransport::Http { .. }
        ));
        assert!(matches!(
            config.mcp_servers["remote-sse"].transport,
            McpTransport::Sse { .. }
        ));
    }

    #[test]
    fn test_load_mcp_config_http_entry_type_absent_but_url_present() {
        // "type" is optional: a bare `url` key alone resolves to Http.
        let json = r#"{"mcpServers": {"remote": {"url": "https://api.example.com/mcp"}}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        assert!(matches!(
            config.mcp_servers["remote"].transport,
            McpTransport::Http { .. }
        ));
    }

    #[test]
    fn test_load_mcp_config_http_entry_missing_url_errors_naming_url() {
        let json = r#"{"mcpServers": {"remote": {"type": "http"}}}"#;
        let file = create_test_config(json);

        let result = load_mcp_config_from(file.path());
        assert!(result.is_err());
        // anyhow's `Display` only prints the outermost context; the field
        // name lives in the wrapped serde_json error, so inspect the chain.
        assert!(format!("{:#}", result.unwrap_err()).contains("url"));
    }

    #[test]
    fn test_load_mcp_config_entry_with_neither_command_nor_type_errors() {
        let json = r#"{"mcpServers": {"broken": {}}}"#;
        let file = create_test_config(json);

        let result = load_mcp_config_from(file.path());
        assert!(result.is_err());
        let msg = format!("{:#}", result.unwrap_err());
        assert!(msg.contains("command"));
        assert!(msg.contains("url"));
    }

    #[test]
    fn test_load_mcp_config_http_entry_with_command_errors() {
        let json = r#"{"mcpServers": {"bad": {"type": "http", "url": "https://x.com", "command": "node"}}}"#;
        let file = create_test_config(json);

        let result = load_mcp_config_from(file.path());
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("command"));
    }

    #[test]
    fn test_load_mcp_config_stdio_entry_with_url_errors() {
        let json = r#"{"mcpServers": {"bad": {"command": "node", "url": "https://x.com"}}}"#;
        let file = create_test_config(json);

        let result = load_mcp_config_from(file.path());
        assert!(result.is_err());
        assert!(format!("{:#}", result.unwrap_err()).contains("url"));
    }

    #[test]
    fn test_load_mcp_config_unknown_field_still_parses() {
        // Unrecognized keys (owned by other MCP clients sharing the file,
        // e.g. Claude Code's "disabled") must warn, not fail the whole file.
        let json = r#"{"mcpServers": {"github": {"command": "node", "disabled": false, "description": "x"}}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        assert!(matches!(
            config.mcp_servers["github"].transport,
            McpTransport::Stdio { .. }
        ));
    }

    #[test]
    fn test_build_server_config_stdio() {
        let (id, config) = build_server_config(
            stdio_transport(
                "github-mcp-server",
                vec!["stdio"],
                vec!["TOKEN=abc123"],
                None,
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(id.as_str(), "github-mcp-server");
        assert_eq!(config.command(), "github-mcp-server");
        assert_eq!(config.args(), &["stdio"]);
        assert_eq!(config.env().get("TOKEN"), Some(&"abc123".to_string()));
    }

    #[test]
    fn test_build_server_config_docker() {
        let (id, config) = build_server_config(
            stdio_transport(
                "docker",
                vec!["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"],
                vec!["GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx"],
                None,
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(id.as_str(), "docker");
        assert_eq!(config.command(), "docker");
        assert_eq!(
            config.args(),
            &["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"]
        );
        assert_eq!(
            config.env().get("GITHUB_PERSONAL_ACCESS_TOKEN"),
            Some(&"ghp_xxx".to_string())
        );
    }

    #[test]
    fn test_build_server_config_http() {
        let (id, config) = build_server_config(
            http_transport(
                "https://api.githubcopilot.com/mcp/",
                vec!["Authorization=Bearer token123"],
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(id.as_str(), "api-githubcopilot-com-mcp");
        assert_eq!(config.url(), Some("https://api.githubcopilot.com/mcp/"));
        assert_eq!(
            config.headers().get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
    }

    #[test]
    fn test_build_server_config_sse() {
        let (id, config) = build_server_config(
            sse_transport("https://example.com/sse", vec!["X-API-Key=secret"]),
            None,
            None,
        )
        .unwrap();

        assert_eq!(id.as_str(), "example-com-sse");
        assert_eq!(config.url(), Some("https://example.com/sse"));
        assert_eq!(
            config.headers().get("X-API-Key"),
            Some(&"secret".to_string())
        );
    }

    #[test]
    fn test_build_server_config_with_cwd() {
        let (_, config) = build_server_config(
            stdio_transport("server", vec![], vec![], Some("/tmp/workdir")),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.cwd(), Some(PathBuf::from("/tmp/workdir")).as_ref());
    }

    #[test]
    fn test_build_server_config_invalid_env() {
        // Regression test for #190: a malformed `--env` value with no `=` is
        // itself indistinguishable from a raw secret and must never be
        // echoed — checked against the `{:?}` chain, since that's what
        // `runner::execute_command` actually prints to stderr.
        let secret = "ghp_verySECRETtoken1234567890abcdef";
        let result = build_server_config(
            stdio_transport("server", vec![], vec![secret], None),
            None,
            None,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{err:?}").contains("expected KEY=VALUE"));
        assert!(
            !format!("{err:?}").contains(secret),
            "error chain leaked the raw secret: {err:?}"
        );
    }

    #[test]
    fn test_build_server_config_invalid_header() {
        // Regression test for #190: same guarantee for `--header` values,
        // which routinely carry bearer tokens / API keys.
        let secret = "Bearer sk-live-supersecretvalue1234567890";
        let result = build_server_config(
            http_transport("https://example.com", vec![secret]),
            None,
            None,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{err:?}").contains("expected KEY=VALUE"));
        assert!(
            !format!("{err:?}").contains(secret),
            "error chain leaked the raw secret: {err:?}"
        );
    }

    #[test]
    fn test_build_server_config_header_name_value_typo_does_not_leak_secret() {
        // Regression test for #190/S1: a header written with the conventional
        // `Name: Value` syntax (colon) instead of `Name=Value`, where the
        // value contains `=` (e.g. base64 padding), previously put the whole
        // secret into the "key" slot. That key then reached
        // `mcp_execution_core::command::validate_header_name_string`, whose
        // error message assumes header names are never secret and echoes
        // them verbatim — leaking the credential one function downstream of
        // the original fix.
        let secret = "c2VjcmV0dG9rZW4=";
        let header = format!("Authorization: Bearer {secret}");
        let result = build_server_config(
            http_transport("https://example.com", vec![&header]),
            None,
            None,
        );

        let err = result.unwrap_err();
        assert!(
            !format!("{err:?}").contains(secret),
            "error chain leaked the raw secret: {err:?}"
        );
        assert!(
            !format!("{err:?}").contains(&header),
            "error chain leaked the raw header argument: {err:?}"
        );
    }

    #[test]
    fn test_build_server_config_invalid_env_classifies_as_invalid_argument() {
        // Regression test for #195/S3: malformed `--env`/`--header` values are
        // the most common invalid-input path for `introspect`/`generate`. The
        // error must carry a `CoreError::InvalidArgument` so
        // `runner::classify_exit_code` maps it to `ExitCode::INVALID_INPUT`
        // instead of silently falling through to the generic `ExitCode::ERROR`.
        let result = build_server_config(
            stdio_transport("server", vec![], vec!["INVALID_FORMAT"], None),
            None,
            None,
        );

        let err = result.unwrap_err();
        assert!(matches!(
            err.downcast_ref::<CoreError>(),
            Some(CoreError::InvalidArgument(_))
        ));
    }

    #[test]
    fn test_build_server_config_multiple_env_vars() {
        let (_, config) = build_server_config(
            stdio_transport(
                "server",
                vec![],
                vec!["TOKEN=abc123", "API_KEY=secret456", "DEBUG=true"],
                None,
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.env().get("TOKEN"), Some(&"abc123".to_string()));
        assert_eq!(config.env().get("API_KEY"), Some(&"secret456".to_string()));
        assert_eq!(config.env().get("DEBUG"), Some(&"true".to_string()));
        assert_eq!(config.env().len(), 3);
    }

    #[test]
    fn test_build_server_config_env_with_special_chars() {
        // Test environment variable values containing equals signs
        let (_, config) = build_server_config(
            stdio_transport(
                "server",
                vec![],
                vec![
                    "TOKEN=abc=def=123",
                    "URL=https://example.com?key=value",
                    "ENCODED=a=b=c=d",
                ],
                None,
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.env().get("TOKEN"), Some(&"abc=def=123".to_string()));
        assert_eq!(
            config.env().get("URL"),
            Some(&"https://example.com?key=value".to_string())
        );
        assert_eq!(config.env().get("ENCODED"), Some(&"a=b=c=d".to_string()));
    }

    #[test]
    fn test_build_server_config_empty_args_stdio() {
        let (id, config) = build_server_config(
            stdio_transport("simple-server", vec![], vec![], None),
            None,
            None,
        )
        .unwrap();

        assert_eq!(id.as_str(), "simple-server");
        assert_eq!(config.command(), "simple-server");
        assert!(config.args().is_empty());
        assert!(config.env().is_empty());
    }

    #[test]
    fn test_build_server_config_http_multiple_headers() {
        let (_, config) = build_server_config(
            http_transport(
                "https://api.example.com",
                vec![
                    "Authorization=Bearer token123",
                    "X-API-Key=secret",
                    "Content-Type=application/json",
                ],
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            config.headers().get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
        assert_eq!(
            config.headers().get("X-API-Key"),
            Some(&"secret".to_string())
        );
        assert_eq!(
            config.headers().get("Content-Type"),
            Some(&"application/json".to_string())
        );
        assert_eq!(config.headers().len(), 3);
    }

    #[test]
    fn test_build_server_config_header_with_special_chars() {
        // Test header values containing equals signs
        let (_, config) = build_server_config(
            http_transport(
                "https://api.example.com",
                vec!["X-Custom=value=with=equals", "X-Query=a=b&c=d"],
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(
            config.headers().get("X-Custom"),
            Some(&"value=with=equals".to_string())
        );
        assert_eq!(
            config.headers().get("X-Query"),
            Some(&"a=b&c=d".to_string())
        );
    }

    #[test]
    fn test_build_server_config_sse_with_headers() {
        let (id, config) = build_server_config(
            sse_transport(
                "https://sse.example.com/events",
                vec!["Authorization=Bearer xyz"],
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(id.as_str(), "sse-example-com-events");
        assert_eq!(config.url(), Some("https://sse.example.com/events"));
        assert_eq!(
            config.headers().get("Authorization"),
            Some(&"Bearer xyz".to_string())
        );
    }

    #[test]
    fn test_build_server_config_empty_value_in_env() {
        // Test environment variable with empty value after equals
        let (_, config) = build_server_config(
            stdio_transport("server", vec![], vec!["EMPTY="], None),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.env().get("EMPTY"), Some(&String::new()));
    }

    #[test]
    fn test_build_server_config_empty_value_in_header() {
        // Test header with empty value after equals
        let (_, config) = build_server_config(
            http_transport("https://example.com", vec!["X-Empty="]),
            None,
            None,
        )
        .unwrap();

        assert_eq!(config.headers().get("X-Empty"), Some(&String::new()));
    }

    #[test]
    fn test_build_server_config_complex_docker_scenario() {
        let (id, config) = build_server_config(
            stdio_transport(
                "docker",
                vec!["run", "-i", "--rm", "--network=host", "my-image:latest"],
                vec!["API_TOKEN=secret123", "LOG_LEVEL=debug"],
                Some("/app/workdir"),
            ),
            None,
            None,
        )
        .unwrap();

        assert_eq!(id.as_str(), "docker");
        assert_eq!(config.command(), "docker");
        assert_eq!(
            config.args(),
            &["run", "-i", "--rm", "--network=host", "my-image:latest"]
        );
        assert_eq!(
            config.env().get("API_TOKEN"),
            Some(&"secret123".to_string())
        );
        assert_eq!(config.env().get("LOG_LEVEL"), Some(&"debug".to_string()));
        assert_eq!(config.cwd(), Some(PathBuf::from("/app/workdir")).as_ref());
    }

    #[test]
    fn test_build_server_config_empty_key_in_env() {
        // Regression test for #190: the pre-fix message echoed the raw `s`
        // (e.g. "=secretvalue"), leaking the value even though the key was
        // reported empty.
        let secret = "topsecretvalue";
        let env_arg = format!("={secret}");
        let result = build_server_config(
            stdio_transport("server", vec![], vec![&env_arg], None),
            None,
            None,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{err:?}").contains("key cannot be empty"));
        assert!(
            !format!("{err:?}").contains(secret),
            "error chain leaked the raw secret: {err:?}"
        );
    }

    #[test]
    fn test_build_server_config_empty_key_in_header() {
        let secret = "topsecretheadervalue";
        let header_arg = format!("={secret}");
        let result = build_server_config(
            http_transport("https://example.com", vec![&header_arg]),
            None,
            None,
        );

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(format!("{err:?}").contains("key cannot be empty"));
        assert!(
            !format!("{err:?}").contains(secret),
            "error chain leaked the raw secret: {err:?}"
        );
    }

    #[test]
    fn test_build_server_config_timeout_override_reaches_core_validation() {
        // The manual CLI-flag path must fail identically to the mcp.json path:
        // both end up calling the same `validate_server_config`, so a zero
        // override must trip the same `connect_timeout` ValidationError.
        let (_, config) = build_server_config(
            stdio_transport("docker", vec![], vec![], None),
            Some(0),
            None,
        )
        .unwrap();

        let result = mcp_execution_core::validate_server_config(&config);
        assert!(result.is_err());
        if let Err(mcp_execution_core::Error::ValidationError { field, reason }) = result {
            assert_eq!(field, "connect_timeout");
            assert!(reason.contains("greater than zero"));
        } else {
            panic!("expected ValidationError for connect_timeout");
        }
    }

    #[test]
    fn test_build_server_config_timeout_overrides() {
        let (_, config) = build_server_config(
            stdio_transport("server", vec![], vec![], None),
            Some(5),
            Some(90),
        )
        .unwrap();

        assert_eq!(config.connect_timeout(), Duration::from_secs(5));
        assert_eq!(config.discover_timeout(), Duration::from_secs(90));
    }

    #[test]
    fn test_build_server_config_default_timeouts_without_overrides() {
        let (_, config) =
            build_server_config(stdio_transport("server", vec![], vec![], None), None, None)
                .unwrap();

        assert_eq!(config.connect_timeout(), Duration::from_secs(30));
        assert_eq!(config.discover_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn test_load_server_from_config_not_found() {
        // Should fail because either config doesn't exist or server not in it
        let result = load_server_from_config("nonexistent");
        assert!(result.is_err());
    }

    #[test]
    fn test_load_mcp_config_no_file() {
        // Should fail gracefully when config file doesn't exist
        let result = load_mcp_config_from(Path::new("/nonexistent/mcp.json"));

        if let Err(error) = result {
            let error = error.to_string();
            assert!(
                error.contains("failed to read MCP config")
                    || error.contains("failed to get home directory"),
                "Expected config read error or home dir error, got: {error}"
            );
        }
    }

    #[test]
    fn test_list_mcp_servers_from_missing_file_returns_empty() {
        // GAP-1: the primary UX fix for #81 — missing config → empty list, not error.
        let result = list_mcp_servers_from(Path::new("/nonexistent/path/mcp.json"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_empty());
    }

    #[test]
    fn test_list_mcp_servers_from_valid_file() {
        let json = r#"{"mcpServers": {"github": {"command": "node"}}}"#;
        let file = create_test_config(json);

        let servers = list_mcp_servers_from(file.path()).unwrap();
        assert_eq!(servers.len(), 1);
        assert_eq!(servers[0].0, "github");
        assert!(matches!(
            servers[0].1.transport,
            McpTransport::Stdio { ref command, .. } if command == "node"
        ));
    }

    #[test]
    fn test_list_mcp_servers_from_empty_servers_key() {
        let json = r#"{"mcpServers": {}}"#;
        let file = create_test_config(json);

        let servers = list_mcp_servers_from(file.path()).unwrap();
        assert!(servers.is_empty());
    }

    #[test]
    fn test_load_mcp_config_without_timeout_keys_uses_defaults() {
        let json = r#"{"mcpServers": {"github": {"command": "node"}}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        let entry = &config.mcp_servers["github"];
        assert_eq!(entry.connect_timeout_secs, None);
        assert_eq!(entry.discover_timeout_secs, None);

        let server_config = build_core_config(entry);
        assert_eq!(server_config.connect_timeout(), Duration::from_secs(30));
        assert_eq!(server_config.discover_timeout(), Duration::from_secs(30));
    }

    #[test]
    fn test_load_mcp_config_with_timeout_keys_reaches_server_config() {
        let json = r#"{"mcpServers": {"github": {
            "command": "node",
            "connectTimeoutSecs": 5,
            "discoverTimeoutSecs": 90
        }}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        let entry = &config.mcp_servers["github"];
        assert_eq!(entry.connect_timeout_secs, Some(5));
        assert_eq!(entry.discover_timeout_secs, Some(90));

        let server_config = build_core_config(entry);
        assert_eq!(server_config.connect_timeout(), Duration::from_secs(5));
        assert_eq!(server_config.discover_timeout(), Duration::from_secs(90));
    }

    #[test]
    fn test_build_core_config_http_entry_reaches_server_config() {
        // The mcp.json -> ServerConfig path (what #210 is literally about),
        // as opposed to the CLI-flag path already covered by
        // `test_build_server_config_http`.
        let json = r#"{"mcpServers": {"remote": {"type": "http", "url": "https://api.example.com/mcp", "headers": {"Authorization": "Bearer x"}}}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        let entry = &config.mcp_servers["remote"];

        let server_config = build_core_config(entry);
        assert_eq!(server_config.url(), Some("https://api.example.com/mcp"));
        assert_eq!(
            server_config.headers().get("Authorization"),
            Some(&"Bearer x".to_string())
        );
    }

    #[test]
    fn test_build_core_config_stdio_cwd_reaches_server_config() {
        let json = r#"{"mcpServers": {"local": {"command": "node", "cwd": "/tmp/workdir"}}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        let entry = &config.mcp_servers["local"];

        let server_config = build_core_config(entry);
        assert_eq!(server_config.cwd(), Some(&PathBuf::from("/tmp/workdir")));
    }

    #[test]
    fn test_load_mcp_config_serde_default_on_missing_mcp_servers() {
        // When mcp.json has no mcpServers key, should deserialize to empty map
        let json = r#"{"someOtherKey": "value"}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        assert!(
            config.mcp_servers.is_empty(),
            "missing mcpServers key must produce empty map, not error"
        );
    }

    // ── TransportArgs::from_flags ──

    #[test]
    fn test_transport_args_from_flags_stdio() {
        let transport = TransportArgs::from_flags(
            Some("server".to_string()),
            vec![],
            vec![],
            None,
            None,
            None,
            vec![],
        )
        .unwrap();
        assert!(matches!(transport, TransportArgs::Stdio { .. }));
    }

    #[test]
    fn test_transport_args_from_flags_http() {
        let transport = TransportArgs::from_flags(
            None,
            vec![],
            vec![],
            None,
            Some("https://example.com".to_string()),
            None,
            vec![],
        )
        .unwrap();
        assert!(matches!(transport, TransportArgs::Http { .. }));
    }

    #[test]
    fn test_transport_args_from_flags_sse() {
        let transport = TransportArgs::from_flags(
            None,
            vec![],
            vec![],
            None,
            None,
            Some("https://example.com/sse".to_string()),
            vec![],
        )
        .unwrap();
        assert!(matches!(transport, TransportArgs::Sse { .. }));
    }

    #[test]
    fn test_transport_args_from_flags_none_set_errors() {
        // Regression test: this used to be an `.expect()` panic in
        // `build_server_config`'s stdio branch.
        let result = TransportArgs::from_flags(None, vec![], vec![], None, None, None, vec![]);
        assert!(result.is_err());
    }

    #[test]
    fn test_transport_args_from_flags_both_http_and_sse_errors() {
        let result = TransportArgs::from_flags(
            None,
            vec![],
            vec![],
            None,
            Some("https://example.com".to_string()),
            Some("https://example.com/sse".to_string()),
            vec![],
        );
        assert!(result.is_err());
    }

    // ── derive_server_id_from_url (review S1: raw-URL server ids are unsafe) ──

    #[test]
    fn test_derive_server_id_from_url_basic() {
        assert_eq!(
            derive_server_id_from_url("https://api.githubcopilot.com/mcp/").as_str(),
            "api-githubcopilot-com-mcp"
        );
        assert_eq!(
            derive_server_id_from_url("https://example.com/sse").as_str(),
            "example-com-sse"
        );
    }

    #[test]
    fn test_derive_server_id_from_url_strips_credentials() {
        // Userinfo (credentials) must never end up in the derived id: it flows
        // into a directory name and generated tool.ts source.
        let id = derive_server_id_from_url("https://user:sekrit-token@api.example.com/mcp");
        assert!(!id.as_str().contains("sekrit"));
        assert!(!id.as_str().contains("user"));
        assert_eq!(id.as_str(), "api-example-com-mcp");
    }

    #[test]
    fn test_derive_server_id_from_url_rejects_path_traversal_chars() {
        // `..` segments must not survive into the id (which is later joined
        // into a filesystem path via PathBuf::join).
        let id = derive_server_id_from_url("https://api.example.com/../../etc/passwd");
        assert!(!id.as_str().contains(".."));
        assert!(mcp_execution_skill::validate_server_id(id.as_str()).is_ok());
    }

    #[test]
    fn test_derive_server_id_from_url_join_never_escapes_base_dir() {
        // Literal reproduction of how `generate.rs` uses the id: joined onto
        // a base directory. Since the sanitized slug can only ever contain
        // `[a-z0-9-]`, `PathBuf::join` can never interpret a component of it
        // as `..` or an absolute-path override, regardless of what path
        // segments were present in the original URL.
        let base_dir = PathBuf::from("/home/user/.claude/servers");
        let malicious_urls = [
            "https://api.example.com/../../../../etc/passwd",
            "https://api.example.com/..%2f..%2fescape",
            "https://api.example.com/./././escape",
        ];

        for url in malicious_urls {
            let id = derive_server_id_from_url(url);
            let joined = base_dir.join(id.as_str());
            assert!(
                joined.starts_with(&base_dir),
                "joining derived id {:?} (from {url:?}) onto {base_dir:?} escaped it: {joined:?}",
                id.as_str()
            );
        }
    }

    #[test]
    fn test_derive_server_id_from_url_normalizes_case() {
        assert_eq!(
            derive_server_id_from_url("https://API.Example.COM/MCP").as_str(),
            "api-example-com-mcp"
        );
    }

    #[test]
    fn test_derive_server_id_from_url_truncates_to_length_limit() {
        let long_path = "a".repeat(200);
        let id = derive_server_id_from_url(&format!("https://example.com/{long_path}"));
        assert!(mcp_execution_skill::validate_server_id(id.as_str()).is_ok());
    }

    #[test]
    fn test_derive_server_id_from_url_falls_back_when_empty() {
        // `Url::parse` accepts "..." as a (degenerate but valid) host, so
        // this genuinely exercises the "parsed OK, but sanitizes to nothing"
        // path, not the parse-failure path covered by the test below.
        let id = derive_server_id_from_url("https://...");
        assert_eq!(id.as_str(), FALLBACK_SERVER_ID_SLUG);
        assert!(mcp_execution_skill::validate_server_id(id.as_str()).is_ok());
    }

    #[test]
    fn test_derive_server_id_from_url_falls_back_on_unparseable_url() {
        // On a `Url::parse` failure the raw input is discarded entirely
        // (never sanitized-and-reused) — every unparseable URL maps to the
        // same fixed fallback slug, regardless of its content.
        for unparseable in ["not a url at all", "", "://", "!!!"] {
            let id = derive_server_id_from_url(unparseable);
            assert_eq!(
                id.as_str(),
                FALLBACK_SERVER_ID_SLUG,
                "input {unparseable:?} should fall back to the default slug"
            );
        }
    }

    /// Regression test for the credential leak the second review round found:
    /// a URL with a mistyped port (a realistic user typo, not an attack) is a
    /// `Url::parse` failure. Before the fix, the fallback sanitized the raw
    /// string instead of discarding it, so `user`/`pass` survived into the id
    /// — which is logged via `info!("Introspecting server: {}", ..)` before
    /// `validate_server_config` ever gets a chance to reject the URL.
    #[test]
    fn test_derive_server_id_from_url_unparseable_credential_bearing_url_leaks_nothing() {
        let id = derive_server_id_from_url("https://user:pass@evil.com:99999/x");
        assert_eq!(id.as_str(), FALLBACK_SERVER_ID_SLUG);
        assert!(!id.as_str().contains("user"));
        assert!(!id.as_str().contains("pass"));
        assert!(!id.as_str().contains("evil"));
    }

    #[test]
    fn test_derive_server_id_from_url_always_passes_validate_server_id() {
        let urls = [
            "https://api.githubcopilot.com/mcp/",
            "https://example.com/sse",
            "https://user:token@host.example.com/mcp?query=1#frag",
            "https://HOST.EXAMPLE.COM/Path/With/Mixed_Case",
            "https://127.0.0.1:8443/mcp",
            "https://example.com/../../escape",
            "https://",
            "not-a-url",
        ];
        for url in urls {
            let id = derive_server_id_from_url(url);
            assert!(
                mcp_execution_skill::validate_server_id(id.as_str()).is_ok(),
                "derived id {:?} from url {url:?} must satisfy validate_server_id",
                id.as_str()
            );
        }
    }

    #[test]
    fn test_build_server_config_http_id_passes_validate_server_id() {
        let (id, _config) = build_server_config(
            http_transport("https://user:token@api.example.com/mcp/../secret", vec![]),
            None,
            None,
        )
        .unwrap();

        assert!(mcp_execution_skill::validate_server_id(id.as_str()).is_ok());
        assert!(!id.as_str().contains("token"));
    }
}
