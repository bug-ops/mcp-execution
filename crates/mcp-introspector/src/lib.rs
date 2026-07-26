//! MCP server introspection using rmcp official SDK.
//!
//! This crate provides functionality to discover MCP server capabilities, tools,
//! resources, and prompts using the official rmcp SDK. It enables automatic
//! extraction of tool schemas for code generation.
//!
//! # Architecture
//!
//! The introspector connects to MCP servers via stdio (subprocess) or
//! Streamable HTTP transport (used for both `TransportType::Http` and
//! `TransportType::Sse`) and uses rmcp's `ServiceExt` trait to query server
//! capabilities. Discovered information is stored locally for subsequent
//! code generation phases.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_execution_introspector::Introspector;
//! use mcp_execution_core::{ServerId, ServerConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut introspector = Introspector::new();
//!
//! // Connect to github server
//! let server_id = ServerId::new("github");
//! let config = ServerConfig::builder()
//!     .command("github-server".to_string())
//!     .build()?;
//!
//! let info = introspector
//!     .discover_server(server_id, &config)
//!     .await?;
//!
//! println!("Server: {} v{}", info.name, info.version);
//! println!("Tools found: {}", info.tools.len());
//!
//! for tool in &info.tools {
//!     println!("  - {}: {}", tool.name, tool.description);
//! }
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

use http::{HeaderName, HeaderValue};
use mcp_execution_core::{
    Error, Result, ServerConfig, ServerId, ToolName, TransportType, validate_server_config,
};
use rmcp::ServiceExt;
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::process::Stdio;
use tokio::process::Child;

/// Maximum number of tools accepted from a single MCP server's `list_all_tools` response
/// (denial-of-service protection, CWE-400).
///
/// An MCP server is untrusted input: without this cap, a malicious or misbehaving server
/// could return an unbounded tool list, which downstream codegen turns into one `.ts` file
/// per tool. 1000 is generous headroom over any real-world server's tool count while still
/// bounding the worst case.
pub const MAX_TOOL_COUNT: usize = 1000;

/// Maximum byte length for a single tool's `name`, as reported by the server.
pub const MAX_TOOL_NAME_LEN: usize = 256;

/// Maximum byte length for a single tool's `description`, as reported by the server.
pub const MAX_TOOL_DESCRIPTION_LEN: usize = 8 * 1024;

/// Maximum serialized JSON byte size for a single tool's `input_schema`, as reported by the
/// server.
///
/// Kept small deliberately (~10x any real MCP tool schema observed in practice) since this is
/// the dominant term in every derived downstream budget (`mcp_execution_codegen`'s
/// `MAX_GENERATED_BYTES`, `mcp_execution_files`'s `MAX_EXPORT_BYTES`, and
/// `mcp_execution_server::state`'s `MAX_TOTAL_PENDING_BYTES`, per issue #198's M1 fix) — a
/// single generous byte-size input here multiplies through every layer that derives its own
/// budget from `MAX_TOOL_COUNT * MAX_SCHEMA_SIZE_BYTES`. Previously 256KB, which propagated
/// out to a ~1GB pending-session budget and a ~541MB generate/export budget; shrinking this one
/// source constant 4x shrinks all of those proportionally without any structural change.
pub const MAX_SCHEMA_SIZE_BYTES: usize = 64 * 1024;

/// Information about an MCP server.
///
/// Contains metadata about the server including its name, version,
/// available tools, and supported capabilities.
///
/// # Examples
///
/// ```
/// use mcp_execution_introspector::{ServerInfo, ServerCapabilities};
/// use mcp_execution_core::ServerId;
///
/// let info = ServerInfo {
///     id: ServerId::new("example"),
///     name: "Example Server".to_string(),
///     version: "1.0.0".to_string(),
///     tools: vec![],
///     capabilities: ServerCapabilities {
///         supports_tools: true,
///         supports_resources: false,
///         supports_prompts: false,
///     },
/// };
///
/// assert_eq!(info.name, "Example Server");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Unique server identifier
    pub id: ServerId,
    /// Human-readable server name
    pub name: String,
    /// Server version string
    pub version: String,
    /// List of available tools
    pub tools: Vec<ToolInfo>,
    /// Server capabilities
    pub capabilities: ServerCapabilities,
}

/// Information about an MCP tool.
///
/// Contains the tool's name, description, and JSON schema for input validation.
///
/// # Examples
///
/// ```
/// use mcp_execution_introspector::ToolInfo;
/// use mcp_execution_core::ToolName;
/// use serde_json::json;
///
/// let tool = ToolInfo {
///     name: ToolName::new("send_message"),
///     description: "Sends a message to a chat".to_string(),
///     input_schema: json!({
///         "type": "object",
///         "properties": {
///             "chat_id": {"type": "string"},
///             "text": {"type": "string"}
///         },
///         "required": ["chat_id", "text"]
///     }),
///     output_schema: None,
/// };
///
/// assert_eq!(tool.name.as_str(), "send_message");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Tool name
    pub name: ToolName,
    /// Human-readable description of what the tool does
    pub description: String,
    /// JSON Schema for tool input parameters
    pub input_schema: serde_json::Value,
    /// Optional JSON Schema for tool output (if provided by server)
    pub output_schema: Option<serde_json::Value>,
}

/// Server capabilities.
///
/// Indicates which MCP features the server supports.
///
/// # Examples
///
/// ```
/// use mcp_execution_introspector::ServerCapabilities;
///
/// let caps = ServerCapabilities {
///     supports_tools: true,
///     supports_resources: true,
///     supports_prompts: false,
/// };
///
/// assert!(caps.supports_tools);
/// assert!(!caps.supports_prompts);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Server supports tool execution
    pub supports_tools: bool,
    /// Server supports resource access
    pub supports_resources: bool,
    /// Server supports prompts
    pub supports_prompts: bool,
}

/// MCP server introspector.
///
/// Discovers and caches information about MCP servers using the official
/// rmcp SDK. Multiple servers can be discovered and their information
/// retrieved later for code generation.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, allowing it to be used across thread
/// boundaries safely.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_introspector::Introspector;
/// use mcp_execution_core::{ServerId, ServerConfig};
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut introspector = Introspector::new();
///
/// // Discover multiple servers
/// let server1 = ServerId::new("server1");
/// let config1 = ServerConfig::builder()
///     .command("server1-cmd".to_string())
///     .build()?;
/// introspector.discover_server(server1.clone(), &config1).await?;
///
/// let server2 = ServerId::new("server2");
/// let config2 = ServerConfig::builder()
///     .command("server2-cmd".to_string())
///     .build()?;
/// introspector.discover_server(server2.clone(), &config2).await?;
///
/// // Retrieve information
/// if let Some(info) = introspector.get_server(&server1) {
///     println!("Server 1 has {} tools", info.tools.len());
/// }
///
/// // List all servers
/// let all_servers = introspector.list_servers();
/// println!("Total servers discovered: {}", all_servers.len());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Introspector {
    servers: HashMap<ServerId, ServerInfo>,
}

impl Introspector {
    /// Creates a new introspector.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_introspector::Introspector;
    ///
    /// let introspector = Introspector::new();
    /// assert_eq!(introspector.list_servers().len(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    /// Connects to an MCP server via stdio and discovers its capabilities.
    ///
    /// This method:
    /// 1. Re-validates the server configuration for security (defense in depth)
    /// 2. Spawns the server process using stdio transport
    /// 3. Connects via rmcp client
    /// 4. Queries server information using `ServiceExt::list_all_tools`
    /// 5. Extracts tools and capabilities
    /// 6. Caches the information for later retrieval
    ///
    /// `config` should normally come from
    /// [`ServerConfigBuilder::build`](mcp_execution_core::ServerConfigBuilder::build), which
    /// already runs full security validation — but `ServerConfig`'s fields are all `pub` and
    /// the type derives `Deserialize`, so a caller can still hand this method an unvalidated
    /// config obtained by other means (a struct literal, or deserializing untrusted JSON
    /// directly). This method re-validates via [`validate_server_config`] as defense in
    /// depth, so it never spawns a process or opens a connection for a config that fails
    /// that check, even when the caller bypassed the builder.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - `config` fails [`validate_server_config`]
    /// - The server process cannot be spawned
    /// - Connection to the server fails
    /// - Server does not respond to capability queries
    /// - Server response is malformed
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_introspector::Introspector;
    /// use mcp_execution_core::{ServerId, ServerConfig};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    /// let server_id = ServerId::new("github");
    /// let config = ServerConfig::builder()
    ///     .command("github-server".to_string())
    ///     .build()?;
    ///
    /// let info = introspector
    ///     .discover_server(server_id, &config)
    ///     .await?;
    ///
    /// println!("Found {} tools", info.tools.len());
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip_all, fields(server_id = %server_id))]
    pub async fn discover_server(
        &mut self,
        server_id: ServerId,
        config: &ServerConfig,
    ) -> Result<ServerInfo> {
        tracing::info!("Discovering MCP server: {}", server_id);

        // Defense in depth: `config` is normally pre-validated by `ServerConfigBuilder::build`,
        // but every field is `pub` and `ServerConfig` derives `Deserialize`, so a caller can
        // still construct an unvalidated config directly. Re-validating here (rather than
        // trusting the builder alone) means this method never spawns a process or opens a
        // connection for a config that fails security validation, regardless of how it was
        // constructed.
        validate_server_config(config)?;

        let discovery = match config.transport() {
            TransportType::Stdio => discover_via_stdio_process(&server_id, config).await?,
            TransportType::Http | TransportType::Sse => {
                discover_via_http(&server_id, config).await?
            }
        };

        let info = build_server_info(&server_id, discovery.peer_meta, discovery.tools)?;

        self.servers.insert(server_id, info.clone());

        tracing::info!("Successfully discovered {} tools", info.tools.len());

        Ok(info)
    }

    /// Gets information about a previously discovered server.
    ///
    /// Returns `None` if the server has not been discovered yet.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_introspector::Introspector;
    /// use mcp_execution_core::{ServerId, ServerConfig};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    /// let server_id = ServerId::new("test");
    ///
    /// // Not discovered yet
    /// assert!(introspector.get_server(&server_id).is_none());
    ///
    /// // Discover it
    /// let config = ServerConfig::builder()
    ///     .command("test-cmd".to_string())
    ///     .build()?;
    /// introspector.discover_server(server_id.clone(), &config).await?;
    ///
    /// // Now available
    /// assert!(introspector.get_server(&server_id).is_some());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn get_server(&self, server_id: &ServerId) -> Option<&ServerInfo> {
        self.servers.get(server_id)
    }

    /// Lists all discovered servers.
    ///
    /// Returns a vector of references to server information in no
    /// particular order.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_introspector::Introspector;
    ///
    /// let introspector = Introspector::new();
    /// let servers = introspector.list_servers();
    /// assert_eq!(servers.len(), 0);
    /// ```
    #[must_use]
    pub fn list_servers(&self) -> Vec<&ServerInfo> {
        self.servers.values().collect()
    }

    /// Returns the number of discovered servers.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_introspector::Introspector;
    ///
    /// let introspector = Introspector::new();
    /// assert_eq!(introspector.server_count(), 0);
    /// ```
    #[must_use]
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    /// Removes a server from the cache.
    ///
    /// Returns `true` if the server was present and removed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_introspector::Introspector;
    /// use mcp_execution_core::{ServerId, ServerConfig};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    /// let server_id = ServerId::new("test");
    /// let config = ServerConfig::builder()
    ///     .command("test-cmd".to_string())
    ///     .build()?;
    ///
    /// introspector.discover_server(server_id.clone(), &config).await?;
    /// assert_eq!(introspector.server_count(), 1);
    ///
    /// let removed = introspector.remove_server(&server_id);
    /// assert!(removed);
    /// assert_eq!(introspector.server_count(), 0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn remove_server(&mut self, server_id: &ServerId) -> bool {
        self.servers.remove(server_id).is_some()
    }

    /// Clears all discovered servers from the cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_introspector::Introspector;
    /// use mcp_execution_core::{ServerId, ServerConfig};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    ///
    /// let config1 = ServerConfig::builder().command("cmd1".to_string()).build()?;
    /// let config2 = ServerConfig::builder().command("cmd2".to_string()).build()?;
    ///
    /// introspector.discover_server(ServerId::new("s1"), &config1).await?;
    /// introspector.discover_server(ServerId::new("s2"), &config2).await?;
    /// assert_eq!(introspector.server_count(), 2);
    ///
    /// introspector.clear();
    /// assert_eq!(introspector.server_count(), 0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn clear(&mut self) {
        self.servers.clear();
    }
}

impl Default for Introspector {
    fn default() -> Self {
        Self::new()
    }
}

/// Server name, version, and feature-support flags extracted from an MCP
/// handshake result.
///
/// Replaces a positional `(String, String, bool, bool)` tuple that was
/// previously threaded through [`extract_peer_meta`] and its callers, where a
/// transposition of the two same-typed trailing `bool` fields would type-check
/// silently and produce wrong [`ServerCapabilities`] values (issue #207).
#[derive(Debug)]
struct PeerMeta {
    /// Human-readable server name from the handshake (or a fallback).
    server_name: String,
    /// Server version string from the handshake (or `"unknown"`).
    server_version: String,
    /// Whether the server advertised resource support.
    has_resources: bool,
    /// Whether the server advertised prompt support.
    has_prompts: bool,
}

/// Tools and handshake metadata discovered from a single introspection
/// round-trip.
///
/// Replaces a positional `(Vec<Tool>, String, String, bool, bool)` tuple
/// previously returned by [`discover_via_stdio_process`], [`discover_via_stdio`],
/// and [`discover_via_http`] (issue #207).
#[derive(Debug)]
struct DiscoveryResult {
    /// Tools reported by the server.
    tools: Vec<rmcp::model::Tool>,
    /// Handshake-derived server metadata and capability flags.
    peer_meta: PeerMeta,
}

/// Spawns the MCP server subprocess used for a single introspection
/// round-trip, with piped stdin/stdout so it can be driven over stdio.
///
/// The caller owns the returned [`Child`] for its full lifetime and is
/// responsible for terminating it (e.g. via [`Child::kill`]) once
/// introspection completes; this crate does not keep the process alive for
/// later tool invocation.
///
/// `kill_on_drop(true)` is set as a backstop for the case where the
/// `discover_via_stdio_process` future itself is dropped before it reaches
/// its own explicit [`Child::kill`] call - e.g. a caller racing discovery
/// against a cancellation signal via `tokio::select!`, which drops the
/// losing future (and everything it owns, including `child`) without ever
/// running the rest of its body. Unlike rmcp's `TokioChildProcess` cleanup
/// (a `tokio::spawn`-ed background task that a short-lived runtime can starve,
/// per issue #132), tokio's own `kill_on_drop` sends the kill signal
/// synchronously inside `Drop`, so it fires reliably even then.
///
/// # Errors
///
/// Returns [`Error::ConnectionFailed`] if the process cannot be spawned
/// (e.g. the command does not exist or is not executable).
fn spawn_introspection_child(server_id: &ServerId, config: &ServerConfig) -> Result<Child> {
    let mut command = tokio::process::Command::new(&config.command);
    command.args(&config.args);
    command.envs(&config.env);
    if let Some(cwd) = &config.cwd {
        command.current_dir(cwd);
    }
    command.stdin(Stdio::piped());
    command.stdout(Stdio::piped());
    command.kill_on_drop(true);

    command.spawn().map_err(|e| Error::ConnectionFailed {
        server: server_id.to_string(),
        source: Box::new(e),
    })
}

/// Spawns the stdio introspection child, drives discovery over it, and tears
/// it down afterward.
///
/// # Errors
///
/// Returns [`Error::ConnectionFailed`] if the process cannot be spawned or its
/// stdio pipes are unavailable, or propagates errors from [`discover_via_stdio`].
async fn discover_via_stdio_process(
    server_id: &ServerId,
    config: &ServerConfig,
) -> Result<DiscoveryResult> {
    let mut child = spawn_introspection_child(server_id, config)?;
    let stdout = child.stdout.take().ok_or_else(|| Error::ConnectionFailed {
        server: server_id.to_string(),
        source: Box::new(std::io::Error::other("child stdout was not captured")),
    })?;
    let stdin = child.stdin.take().ok_or_else(|| Error::ConnectionFailed {
        server: server_id.to_string(),
        source: Box::new(std::io::Error::other("child stdin was not captured")),
    })?;

    let discovery = discover_via_stdio(server_id, config, (stdout, stdin)).await;

    // The child process is spawned solely for this discovery round-trip, so it
    // must be reaped here regardless of outcome. We deliberately kill it
    // ourselves rather than relying on rmcp's `TokioChildProcess`, whose cleanup
    // is a `tokio::spawn`-ed background task in `Drop`: under a short-lived
    // runtime (e.g. `#[tokio::test]`) that task can be starved before it ever
    // runs, leaking the process (issue #132).
    if let Err(kill_err) = child.kill().await {
        tracing::warn!(
            "failed to terminate introspection child process for {server_id}: {kill_err}"
        );
    }

    discovery
}

/// Outcome of a single [`list_tools_bounded`] page fetch that isn't a plain success.
enum ListToolsBoundedError {
    /// The underlying rmcp request failed (connection, protocol, or server-side error).
    Service(rmcp::ServiceError),
    /// The accumulated tool count across pages fetched so far exceeded [`MAX_TOOL_COUNT`].
    /// Carries the accumulated count purely for the error message.
    TooMany(usize),
}

impl From<rmcp::ServiceError> for ListToolsBoundedError {
    fn from(e: rmcp::ServiceError) -> Self {
        Self::Service(e)
    }
}

/// Fetches a server's tool list page by page via `list_tools`, bailing out as soon as the
/// accumulated count exceeds [`MAX_TOOL_COUNT`] instead of buffering every page first the way
/// `Peer::list_all_tools` does (issue #198 S4): a malicious or misbehaving server can return an
/// arbitrarily large tool list across arbitrarily many pages, so checking the bound only after
/// the whole (potentially huge) response has already been collected does not actually bound
/// peak memory during discovery — only what gets kept afterward. This bails after the page
/// that first pushes the running total over the limit, so at most one page's worth of tools
/// beyond [`MAX_TOOL_COUNT`] is ever held at once.
async fn list_tools_bounded(
    client: &rmcp::service::RunningService<rmcp::RoleClient, ()>,
) -> std::result::Result<Vec<rmcp::model::Tool>, ListToolsBoundedError> {
    let mut tools = Vec::new();
    let mut cursor = None;
    loop {
        let page = client
            .list_tools(Some(
                rmcp::model::PaginatedRequestParams::default().with_cursor(cursor),
            ))
            .await?;
        tools.extend(page.tools);

        if tools.len() > MAX_TOOL_COUNT {
            return Err(ListToolsBoundedError::TooMany(tools.len()));
        }

        cursor = page.next_cursor;
        if cursor.is_none() {
            break;
        }
    }
    Ok(tools)
}

/// Converts a [`ListToolsBoundedError`] (from a `tokio::time::timeout`-wrapped
/// [`list_tools_bounded`] call) into this crate's [`Error`], given the `server_id` the
/// discovery attempt was for.
fn map_list_tools_bounded_error(server_id: &ServerId, error: ListToolsBoundedError) -> Error {
    match error {
        ListToolsBoundedError::Service(e) => Error::ConnectionFailed {
            server: server_id.to_string(),
            source: Box::new(e),
        },
        ListToolsBoundedError::TooMany(actual) => Error::ResourceLimitExceeded {
            resource: format!("tool count from server '{server_id}'"),
            actual,
            limit: MAX_TOOL_COUNT,
        },
    }
}

/// Connects to an already-spawned MCP server over `transport` and lists its
/// tools, with each step bounded by `config`'s configured timeouts.
///
/// Returns the discovered tools alongside the handshake-derived server name,
/// version, and capability flags (resources / prompts support).
///
/// # Errors
///
/// Returns [`Error::Timeout`] if the connect or discovery step exceeds its
/// configured timeout, or [`Error::ConnectionFailed`] if the underlying rmcp
/// connection or request fails.
async fn discover_via_stdio(
    server_id: &ServerId,
    config: &ServerConfig,
    transport: (tokio::process::ChildStdout, tokio::process::ChildStdin),
) -> Result<DiscoveryResult> {
    // Create client using serve pattern, bounded by the connect timeout
    let client = tokio::time::timeout(config.connect_timeout(), ().serve(transport))
        .await
        .map_err(|_elapsed| Error::Timeout {
            operation: format!("connect to {server_id}"),
            duration_secs: config.connect_timeout().as_secs(),
        })?
        .map_err(|e| Error::ConnectionFailed {
            server: server_id.to_string(),
            source: Box::new(e),
        })?;

    // List tools page by page, bounded by the discover timeout overall and bailing out early
    // if the accumulated count exceeds MAX_TOOL_COUNT (see `list_tools_bounded`'s docs).
    let tool_list = tokio::time::timeout(config.discover_timeout(), list_tools_bounded(&client))
        .await
        .map_err(|_elapsed| Error::Timeout {
            operation: format!("list_all_tools for {server_id}"),
            duration_secs: config.discover_timeout().as_secs(),
        })?
        .map_err(|e| map_list_tools_bounded_error(server_id, e))?;

    // Extract name, version, and capabilities from the MCP handshake result.
    // Falls back to the command string / "unknown" if the server did not send peer info.
    let peer_meta = extract_peer_meta(config, client.peer_info().as_deref());

    Ok(DiscoveryResult {
        tools: tool_list,
        peer_meta,
    })
}

/// Connects to an MCP server over Streamable HTTP and lists its tools, with
/// each step bounded by `config`'s configured timeouts.
///
/// Used for both [`TransportType::Http`] and [`TransportType::Sse`]: rmcp 2.2
/// has a single client transport for network MCP servers ("Streamable
/// HTTP"), which superseded the standalone SSE transport in the 2025-03-26
/// MCP spec revision. There is no legacy SSE-only client to fall back to.
///
/// # Errors
///
/// Returns [`Error::Timeout`] if the connect or discovery step exceeds its
/// configured timeout, or [`Error::ConnectionFailed`] if a header cannot be
/// constructed, or the underlying rmcp connection or request fails (this
/// includes a reserved-header collision, e.g. a caller-supplied `Accept`
/// header, which rmcp rejects as `StreamableHttpError::ReservedHeaderConflict`).
async fn discover_via_http(server_id: &ServerId, config: &ServerConfig) -> Result<DiscoveryResult> {
    // `ServerConfigBuilder::build` already guarantees `url` is `Some` for
    // Http/Sse transports — no `ServerConfig` can exist otherwise.
    let url = config
        .url()
        .expect("url validated as present by ServerConfigBuilder::build");

    let mut custom_headers = HashMap::new();
    for (name, value) in config.headers() {
        let header_name =
            HeaderName::try_from(name.as_str()).map_err(|e| Error::ConnectionFailed {
                server: server_id.to_string(),
                source: Box::new(e),
            })?;
        let header_value =
            HeaderValue::try_from(value.as_str()).map_err(|e| Error::ConnectionFailed {
                server: server_id.to_string(),
                source: Box::new(e),
            })?;
        custom_headers.insert(header_name, header_value);
    }

    // Type parameter inferred as `reqwest::Client`: `StreamableHttpClientTransportConfig`'s
    // `from_config` is an inherent method defined only on that one specialization of
    // `StreamableHttpClientTransport<C>`, so this crate does not need to depend on `reqwest`
    // directly or name `reqwest::Client` to select it.
    let transport = StreamableHttpClientTransport::from_config(
        StreamableHttpClientTransportConfig::with_uri(url).custom_headers(custom_headers),
    );

    let client = tokio::time::timeout(config.connect_timeout(), ().serve(transport))
        .await
        .map_err(|_elapsed| Error::Timeout {
            operation: format!("connect to {server_id}"),
            duration_secs: config.connect_timeout().as_secs(),
        })?
        .map_err(|e| Error::ConnectionFailed {
            server: server_id.to_string(),
            source: Box::new(e),
        })?;

    let tool_list = tokio::time::timeout(config.discover_timeout(), list_tools_bounded(&client))
        .await
        .map_err(|_elapsed| Error::Timeout {
            operation: format!("list_all_tools for {server_id}"),
            duration_secs: config.discover_timeout().as_secs(),
        })?
        .map_err(|e| map_list_tools_bounded_error(server_id, e))?;

    // Extract name, version, and capabilities from the MCP handshake result.
    // Dropping `client` here triggers `WorkerTransport`'s drop guard, which
    // cancels the worker task; a cancelled worker cannot itself issue a
    // session-DELETE request, so no explicit disconnect happens on this path
    // — the server-side session simply expires on its own timeout instead.
    let peer_meta = extract_peer_meta(config, client.peer_info().as_deref());

    Ok(DiscoveryResult {
        tools: tool_list,
        peer_meta,
    })
}

/// Assembles a [`ServerInfo`] from the raw tool list and handshake metadata
/// returned by [`discover_server`](Introspector::discover_server)'s stdio or
/// HTTP/SSE discovery path.
///
/// # Errors
///
/// Returns [`Error::ResourceLimitExceeded`] if `tool_list` exceeds [`MAX_TOOL_COUNT`], or if
/// any individual tool's name, description, or input schema exceeds its own configured bound
/// (see [`build_tool_info`]) — defense against a malicious or misbehaving MCP server
/// returning an unbounded response (CWE-400).
fn build_server_info(
    server_id: &ServerId,
    peer_meta: PeerMeta,
    tool_list: Vec<rmcp::model::Tool>,
) -> Result<ServerInfo> {
    tracing::debug!(
        "Server {} responded with {} tools",
        server_id,
        tool_list.len()
    );

    if tool_list.len() > MAX_TOOL_COUNT {
        return Err(Error::ResourceLimitExceeded {
            resource: format!("tool count from server '{server_id}'"),
            actual: tool_list.len(),
            limit: MAX_TOOL_COUNT,
        });
    }

    let tools = tool_list
        .into_iter()
        .map(build_tool_info)
        .collect::<Result<Vec<_>>>()?;

    let capabilities = ServerCapabilities {
        supports_tools: !tools.is_empty(),
        supports_resources: peer_meta.has_resources,
        supports_prompts: peer_meta.has_prompts,
    };

    Ok(ServerInfo {
        id: server_id.clone(),
        name: peer_meta.server_name,
        version: peer_meta.server_version,
        tools,
        capabilities,
    })
}

/// Converts a single raw [`rmcp::model::Tool`] into a [`ToolInfo`], bounding its `name`,
/// `description`, and serialized `input_schema` size (denial-of-service protection, CWE-400)
/// against a malicious or misbehaving MCP server.
///
/// # Errors
///
/// Returns [`Error::ResourceLimitExceeded`] if the tool's name exceeds [`MAX_TOOL_NAME_LEN`],
/// its description exceeds [`MAX_TOOL_DESCRIPTION_LEN`], or its serialized input schema
/// exceeds [`MAX_SCHEMA_SIZE_BYTES`].
fn build_tool_info(tool: rmcp::model::Tool) -> Result<ToolInfo> {
    let name = tool.name.to_string();
    tracing::trace!("Found tool: {name}");

    if name.len() > MAX_TOOL_NAME_LEN {
        return Err(Error::ResourceLimitExceeded {
            resource: "tool name length".to_string(),
            actual: name.len(),
            limit: MAX_TOOL_NAME_LEN,
        });
    }

    let description = tool.description.unwrap_or_default().to_string();
    if description.len() > MAX_TOOL_DESCRIPTION_LEN {
        return Err(Error::ResourceLimitExceeded {
            resource: format!("description length for tool '{name}'"),
            actual: description.len(),
            limit: MAX_TOOL_DESCRIPTION_LEN,
        });
    }

    let input_schema = serde_json::Value::Object((*tool.input_schema).clone());
    // A `Value` built from a JSON schema can only fail to re-serialize in pathological cases
    // (e.g. a non-finite float somewhere in the tree); treat that as exceeding the bound
    // rather than let it panic or silently pass an unmeasured schema through.
    let schema_size = serde_json::to_vec(&input_schema).map_or(usize::MAX, |bytes| bytes.len());
    if schema_size > MAX_SCHEMA_SIZE_BYTES {
        return Err(Error::ResourceLimitExceeded {
            resource: format!("input_schema size for tool '{name}'"),
            actual: schema_size,
            limit: MAX_SCHEMA_SIZE_BYTES,
        });
    }

    Ok(ToolInfo {
        name: ToolName::new(name),
        description,
        input_schema,
        output_schema: None, // rmcp doesn't provide output schema
    })
}

/// Extracts server name, version, resource support, and prompt support from
/// the MCP handshake result (`peer_info`) into a [`PeerMeta`].
///
/// Falls back to `PeerMeta { server_name: fallback_server_name(config), server_version:
/// "unknown", has_resources: false, has_prompts: false }` when the server did not send
/// peer information (i.e. `peer_info` is `None`).
fn extract_peer_meta(
    config: &ServerConfig,
    peer_info: Option<&rmcp::model::InitializeResult>,
) -> PeerMeta {
    peer_info.map_or_else(
        || PeerMeta {
            server_name: fallback_server_name(config),
            server_version: "unknown".to_string(),
            has_resources: false,
            has_prompts: false,
        },
        |info| PeerMeta {
            server_name: info.server_info.name.clone(),
            server_version: info.server_info.version.clone(),
            has_resources: info.capabilities.resources.is_some(),
            has_prompts: info.capabilities.prompts.is_some(),
        },
    )
}

/// Picks a display name for a server that sent no `peer_info` on handshake.
///
/// Prefers `config.command` (stdio transport); falls back to `config.url()`
/// since Http/Sse transports always leave `command` empty.
fn fallback_server_name(config: &ServerConfig) -> String {
    if config.command.is_empty() {
        config.url().unwrap_or_default().to_string()
    } else {
        config.command.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_introspector_new() {
        let introspector = Introspector::new();
        assert_eq!(introspector.list_servers().len(), 0);
        assert_eq!(introspector.server_count(), 0);
    }

    #[test]
    fn test_introspector_default() {
        let introspector = Introspector::default();
        assert_eq!(introspector.server_count(), 0);
    }

    #[test]
    fn test_server_info_debug() {
        let info = ServerInfo {
            id: ServerId::new("test"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };
        let debug_str = format!("{info:?}");
        assert!(debug_str.contains("Test Server"));
        assert!(debug_str.contains("1.0.0"));
    }

    #[test]
    fn test_tool_info_creation() {
        let tool = ToolInfo {
            name: ToolName::new("test_tool"),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: None,
        };

        assert_eq!(tool.name.as_str(), "test_tool");
        assert_eq!(tool.description, "A test tool");
        assert!(tool.output_schema.is_none());
    }

    #[test]
    fn test_server_capabilities() {
        let caps = ServerCapabilities {
            supports_tools: true,
            supports_resources: true,
            supports_prompts: false,
        };

        assert!(caps.supports_tools);
        assert!(caps.supports_resources);
        assert!(!caps.supports_prompts);
    }

    #[test]
    fn test_get_server_not_found() {
        let introspector = Introspector::new();
        let server_id = ServerId::new("nonexistent");
        assert!(introspector.get_server(&server_id).is_none());
    }

    #[test]
    fn test_clear() {
        let mut introspector = Introspector::new();

        // Add some fake server data
        let info = ServerInfo {
            id: ServerId::new("test"),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        introspector.servers.insert(ServerId::new("test"), info);
        assert_eq!(introspector.server_count(), 1);

        introspector.clear();
        assert_eq!(introspector.server_count(), 0);
    }

    #[test]
    fn test_remove_server() {
        let mut introspector = Introspector::new();
        let server_id = ServerId::new("test");

        // Add fake server data
        let info = ServerInfo {
            id: server_id.clone(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        introspector.servers.insert(server_id.clone(), info);
        assert_eq!(introspector.server_count(), 1);

        // Remove existing server
        assert!(introspector.remove_server(&server_id));
        assert_eq!(introspector.server_count(), 0);

        // Remove non-existent server
        assert!(!introspector.remove_server(&server_id));
    }

    #[test]
    fn test_list_servers() {
        let mut introspector = Introspector::new();

        // Empty list
        assert_eq!(introspector.list_servers().len(), 0);

        // Add servers
        let info1 = ServerInfo {
            id: ServerId::new("server1"),
            name: "Server 1".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let info2 = ServerInfo {
            id: ServerId::new("server2"),
            name: "Server 2".to_string(),
            version: "2.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: false,
                supports_resources: true,
                supports_prompts: false,
            },
        };

        introspector.servers.insert(ServerId::new("server1"), info1);
        introspector.servers.insert(ServerId::new("server2"), info2);

        let servers = introspector.list_servers();
        assert_eq!(servers.len(), 2);
    }

    #[test]
    fn test_serialization() {
        let tool = ToolInfo {
            name: ToolName::new("test_tool"),
            description: "Test".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: Some(serde_json::json!({"type": "string"})),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("test_tool"));
        assert!(json.contains("Test"));

        // Deserialize back
        let tool2: ToolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(tool2.name.as_str(), "test_tool");
        assert_eq!(tool2.description, "Test");
    }

    // ── extract_peer_meta unit tests ─────────────────────────────────────────

    fn make_peer_info(
        name: &str,
        version: &str,
        has_resources: bool,
        has_prompts: bool,
    ) -> rmcp::model::InitializeResult {
        // rmcp structs are #[non_exhaustive]; construct via JSON deserialization.
        let mut capabilities = serde_json::json!({});
        if has_resources {
            capabilities["resources"] = serde_json::json!({});
        }
        if has_prompts {
            capabilities["prompts"] = serde_json::json!({});
        }

        let raw = serde_json::json!({
            "protocolVersion": "2024-11-05",
            "serverInfo": { "name": name, "version": version },
            "capabilities": capabilities
        });
        serde_json::from_value(raw).expect("valid InitializeResult JSON")
    }

    fn test_config(command: &str) -> ServerConfig {
        ServerConfig::builder()
            .command(command.to_string())
            .build()
            .unwrap()
    }

    /// #84 — name must come from the handshake, not `config.command`
    #[test]
    fn test_extract_peer_meta_name_from_handshake() {
        let config = test_config("my-server-binary");
        let peer = make_peer_info("Handshake Name", "0.0.0", false, false);

        let meta = extract_peer_meta(&config, Some(&peer));

        assert_eq!(meta.server_name, "Handshake Name");
        assert_ne!(meta.server_name, "my-server-binary");
    }

    /// #79 — version must come from the handshake, not the hardcoded "unknown"
    #[test]
    fn test_extract_peer_meta_version_from_handshake() {
        let config = test_config("cmd");
        let peer = make_peer_info("Server", "3.1.4", false, false);

        let meta = extract_peer_meta(&config, Some(&peer));

        assert_eq!(meta.server_version, "3.1.4");
        assert_ne!(meta.server_version, "unknown");
    }

    /// #80 — `supports_resources` reflects `capabilities.resources` being `Some`
    #[test]
    fn test_extract_peer_meta_supports_resources_true() {
        let config = test_config("cmd");
        let peer = make_peer_info("S", "1.0", true, false);

        let meta = extract_peer_meta(&config, Some(&peer));

        assert!(meta.has_resources);
    }

    /// #80 — `supports_resources` is false when `capabilities.resources` is `None`
    #[test]
    fn test_extract_peer_meta_supports_resources_false() {
        let config = test_config("cmd");
        let peer = make_peer_info("S", "1.0", false, false);

        let meta = extract_peer_meta(&config, Some(&peer));

        assert!(!meta.has_resources);
    }

    /// Bonus — `supports_prompts` reflects `capabilities.prompts` being `Some`
    #[test]
    fn test_extract_peer_meta_supports_prompts_true() {
        let config = test_config("cmd");
        let peer = make_peer_info("S", "1.0", false, true);

        let meta = extract_peer_meta(&config, Some(&peer));

        assert!(meta.has_prompts);
    }

    /// Bonus — `supports_prompts` is false when `capabilities.prompts` is `None`
    #[test]
    fn test_extract_peer_meta_supports_prompts_false() {
        let config = test_config("cmd");
        let peer = make_peer_info("S", "1.0", false, false);

        let meta = extract_peer_meta(&config, Some(&peer));

        assert!(!meta.has_prompts);
    }

    /// Fallback: `peer_info` is `None` — name falls back to `config.command`
    #[test]
    fn test_extract_peer_meta_fallback_name_is_command() {
        let config = test_config("fallback-binary");

        let meta = extract_peer_meta(&config, None);

        assert_eq!(meta.server_name, "fallback-binary");
    }

    /// Incidental fix: for Http/Sse configs `command` is always empty, so the
    /// fallback name must come from `url`, not silently be `""`.
    #[test]
    fn test_extract_peer_meta_fallback_name_is_url_for_http_transport() {
        let config = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .build()
            .unwrap();

        let meta = extract_peer_meta(&config, None);

        assert_eq!(meta.server_name, "https://api.example.com/mcp");
    }

    /// Fallback: `peer_info` is `None` — version falls back to `"unknown"`
    #[test]
    fn test_extract_peer_meta_fallback_version_is_unknown() {
        let config = test_config("cmd");

        let meta = extract_peer_meta(&config, None);

        assert_eq!(meta.server_version, "unknown");
    }

    /// Fallback: `peer_info` is `None` — capabilities are all false
    #[test]
    fn test_extract_peer_meta_fallback_capabilities_false() {
        let config = test_config("cmd");

        let meta = extract_peer_meta(&config, None);

        assert!(!meta.has_resources);
        assert!(!meta.has_prompts);
    }

    // ── Resource-exhaustion bounds (issue #198) ──────────────────────────────

    /// Builds a raw tool whose serialized `input_schema` grows linearly with `padding_len`:
    /// the `padding` property's string value always contributes exactly `padding_len` bytes,
    /// so tests can hit `MAX_SCHEMA_SIZE_BYTES` at an exact boundary by first measuring the
    /// fixed overhead at `padding_len == 0`.
    fn make_raw_tool(name: &str, description: &str, padding_len: usize) -> rmcp::model::Tool {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "padding": "a".repeat(padding_len),
            },
        });
        let serde_json::Value::Object(schema_obj) = schema else {
            unreachable!()
        };

        serde_json::from_value(serde_json::json!({
            "name": name,
            "description": description,
            "inputSchema": schema_obj,
        }))
        .expect("valid rmcp::model::Tool JSON")
    }

    /// Serialized byte size of the `input_schema` produced by [`make_raw_tool`] for a given
    /// `padding_len`, used to compute exact-boundary test inputs.
    fn schema_size_for_padding(padding_len: usize) -> usize {
        let tool = make_raw_tool("tool", "d", padding_len);
        serde_json::to_vec(&serde_json::Value::Object((*tool.input_schema).clone()))
            .expect("schema serializes")
            .len()
    }

    /// `build_server_info`'s own `tool_list.len() > MAX_TOOL_COUNT` check is no longer what
    /// protects `discover_server` in practice — `list_tools_bounded` (see the integration test
    /// `tests/tool_count_bound_test.rs`) now bails out mid-pagination before ever handing
    /// `build_server_info` an over-limit `Vec`, so this branch is unreachable on the real
    /// `discover_via_stdio`/`discover_via_http` paths. It remains real, reachable code via this
    /// direct call, though (`build_server_info` is `pub(crate)`-callable independent of the
    /// pagination helper), so this test still exercises a genuine defense-in-depth check —
    /// just not the primary one (issue #198 N2).
    #[test]
    fn test_build_server_info_rejects_too_many_tools_via_direct_call() {
        let tool_list: Vec<rmcp::model::Tool> = (0..=MAX_TOOL_COUNT)
            .map(|i| make_raw_tool(&format!("tool{i}"), "d", 0))
            .collect();

        let result = build_server_info(
            &ServerId::new("test"),
            "Test".to_string(),
            "1.0.0".to_string(),
            false,
            false,
            tool_list,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
    }

    #[test]
    fn test_build_server_info_accepts_max_tool_count() {
        let tool_list: Vec<rmcp::model::Tool> = (0..MAX_TOOL_COUNT)
            .map(|i| make_raw_tool(&format!("tool{i}"), "d", 0))
            .collect();

        let result = build_server_info(
            &ServerId::new("test"),
            "Test".to_string(),
            "1.0.0".to_string(),
            false,
            false,
            tool_list,
        );

        assert!(result.is_ok());
        assert_eq!(result.unwrap().tools.len(), MAX_TOOL_COUNT);
    }

    #[test]
    fn test_build_tool_info_rejects_oversized_name() {
        let long_name = "a".repeat(MAX_TOOL_NAME_LEN + 1);
        let tool = make_raw_tool(&long_name, "d", 0);

        let result = build_tool_info(tool);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
    }

    #[test]
    fn test_build_tool_info_accepts_name_at_max_len() {
        let name_at_cap = "a".repeat(MAX_TOOL_NAME_LEN);
        let tool = make_raw_tool(&name_at_cap, "d", 0);

        let result = build_tool_info(tool);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().name.as_str().len(), MAX_TOOL_NAME_LEN);
    }

    #[test]
    fn test_build_tool_info_rejects_oversized_description() {
        let long_description = "a".repeat(MAX_TOOL_DESCRIPTION_LEN + 1);
        let tool = make_raw_tool("tool", &long_description, 0);

        let result = build_tool_info(tool);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
    }

    #[test]
    fn test_build_tool_info_accepts_description_at_max_len() {
        let description_at_cap = "a".repeat(MAX_TOOL_DESCRIPTION_LEN);
        let tool = make_raw_tool("tool", &description_at_cap, 0);

        let result = build_tool_info(tool);
        assert!(result.is_ok());
        assert_eq!(result.unwrap().description.len(), MAX_TOOL_DESCRIPTION_LEN);
    }

    #[test]
    fn test_build_tool_info_rejects_schema_one_byte_over_max_size() {
        let overhead = schema_size_for_padding(0);
        let padding_len = MAX_SCHEMA_SIZE_BYTES - overhead + 1;
        let tool = make_raw_tool("tool", "d", padding_len);

        let result = build_tool_info(tool);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
    }

    #[test]
    fn test_build_tool_info_accepts_schema_at_exact_max_size() {
        let overhead = schema_size_for_padding(0);
        let padding_len = MAX_SCHEMA_SIZE_BYTES - overhead;
        assert_eq!(
            schema_size_for_padding(padding_len),
            MAX_SCHEMA_SIZE_BYTES,
            "test setup should hit the cap exactly"
        );
        let tool = make_raw_tool("tool", "d", padding_len);

        let result = build_tool_info(tool);
        assert!(result.is_ok());
    }

    /// All four values are correct simultaneously when `peer_info` is fully populated
    #[test]
    fn test_extract_peer_meta_all_fields_from_handshake() {
        let config = test_config("binary");
        let peer = make_peer_info("Full Server", "2.0.0", true, true);

        let meta = extract_peer_meta(&config, Some(&peer));

        assert_eq!(meta.server_name, "Full Server");
        assert_eq!(meta.server_version, "2.0.0");
        assert!(meta.has_resources);
        assert!(meta.has_prompts);
    }
}
