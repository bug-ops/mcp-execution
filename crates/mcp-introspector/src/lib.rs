//! MCP server introspection using rmcp official SDK.
//!
//! This crate provides functionality to discover MCP server capabilities, tools,
//! resources, and prompts using the official rmcp SDK. It enables automatic
//! extraction of tool schemas for code generation.
//!
//! # Architecture
//!
//! The introspector connects to MCP servers via stdio (subprocess) or
//! Streamable HTTP transport (used for both `Transport::Http` and
//! `Transport::Sse`) and uses rmcp's `ServiceExt` trait to query server
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
//! let server_id = ServerId::new("github").unwrap();
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

use futures_util::StreamExt;
use futures_util::stream::{self, Stream};
use http::{HeaderName, HeaderValue};
use mcp_execution_core::{
    Error, ResourceKind, Result, ServerConfig, ServerId, ToolName, Transport,
    validate_server_config,
};
use rmcp::RoleClient;
use rmcp::ServiceExt;
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::StreamableHttpClientTransport;
use rmcp::transport::async_rw::{JsonRpcMessageCodec, JsonRpcMessageCodecError};
use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::future::Future;
use std::process::Stdio;
use std::task::Poll;
use tokio::io::AsyncRead;
use tokio::process::Child;
use tokio_util::bytes::BytesMut;
use tokio_util::codec::{Decoder, FramedRead, FramedWrite};

/// Maximum number of tools accepted from a single MCP server's `list_all_tools` response
/// (denial-of-service protection, CWE-400).
///
/// An MCP server is untrusted input: without this cap, a malicious or misbehaving server
/// could return an unbounded tool list, which downstream codegen turns into one `.ts` file
/// per tool. 1000 is generous headroom over any real-world server's tool count while still
/// bounding the worst case.
///
/// # Examples
///
/// ```
/// use mcp_execution_introspector::MAX_TOOL_COUNT;
///
/// assert_eq!(MAX_TOOL_COUNT, 1000);
/// ```
pub const MAX_TOOL_COUNT: usize = 1000;

/// Maximum byte length for a single tool's `name`, as reported by the server.
///
/// # Examples
///
/// ```
/// use mcp_execution_introspector::MAX_TOOL_NAME_LEN;
///
/// assert!(MAX_TOOL_NAME_LEN > 0);
/// ```
pub const MAX_TOOL_NAME_LEN: usize = 256;

/// Maximum byte length for a single tool's `description`, as reported by the server.
///
/// # Examples
///
/// ```
/// use mcp_execution_introspector::MAX_TOOL_DESCRIPTION_LEN;
///
/// assert!(MAX_TOOL_DESCRIPTION_LEN > 0);
/// ```
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
///
/// # Examples
///
/// ```
/// use mcp_execution_introspector::MAX_SCHEMA_SIZE_BYTES;
///
/// assert!(MAX_SCHEMA_SIZE_BYTES > 0);
/// ```
pub const MAX_SCHEMA_SIZE_BYTES: usize = 64 * 1024;

/// Maximum size, in bytes, of a single newline-delimited JSON-RPC response line read from a
/// stdio MCP server's stdout during introspection (issue #225). Lines exceeding this are
/// discarded rather than buffered without bound.
///
/// `rmcp`'s default transport for a raw `(ChildStdout, ChildStdin)` pair (`AsyncRwTransport`)
/// reads lines via an unbounded `read_until`, bypassing [`JsonRpcMessageCodec`]'s `max_length`
/// entirely — see [`bounded_response_stream`], which wires stdout through the codec directly
/// instead. No shared line-size constant exists elsewhere in the workspace to reuse, so this
/// matches `mcp-execution-server`'s `MAX_REQUEST_LINE_SIZE` value for consistency — though not
/// its derivation: that constant was sized from measured real payloads on a different trust
/// boundary (inbound requests from an already-trusted local client). This one bounds an
/// untrusted MCP server's responses and is deliberately *not* sized to the crate's own
/// worst-case accepted `tools/list` response: [`MAX_TOOL_COUNT`] (1000) tools at up to
/// [`MAX_SCHEMA_SIZE_BYTES`] (64 KiB) each for input and output schema, plus
/// [`MAX_TOOL_DESCRIPTION_LEN`] (8 KiB), allows a single unpaginated response over 100 MB —
/// setting the cap that high would defeat the point of bounding memory here. A conforming
/// server is expected to paginate a tool list that large; one that instead sends it as a
/// single line over this cap has that line dropped by [`bounded_response_stream`] (logged, not
/// silently discarded), and the request it was answering then runs out its
/// [`ServerConfig::discover_timeout`] with no reply, surfacing to the caller as
/// [`Error::Timeout`] rather than a distinct size-limit error — there is no request id to
/// correlate a dropped, unparsed line back to. `tokio_util`'s internal read buffer also grows
/// by doubling and is only checked against this bound after a read fills whatever capacity it
/// already reserved, so an attacker's oversized line can push peak buffer capacity to roughly
/// 4x this value before it is rejected — still strictly bounded, just not 1:1 with the cap
/// (mirrors `mcp-execution-server`'s own documented behavior for the same codec).
const MAX_RESPONSE_LINE_SIZE: usize = 4 * 1024 * 1024;

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
///     id: ServerId::new("example").unwrap(),
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
///     name: ToolName::new("send_message").unwrap(),
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
/// let server1 = ServerId::new("server1").unwrap();
/// let config1 = ServerConfig::builder()
///     .command("server1-cmd".to_string())
///     .build()?;
/// introspector.discover_server(server1.clone(), &config1).await?;
///
/// let server2 = ServerId::new("server2").unwrap();
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
    /// # Security
    ///
    /// The stdio transport ([`Transport::Stdio`], handled by the private
    /// `discover_via_stdio`) bounds every response line read from the server's stdout to a
    /// fixed maximum size (issue #225). A line over that bound is dropped rather than
    /// erroring loudly: since it was never fully parsed, there is no request id to
    /// attribute a distinct error to, so the request it was answering instead runs out its
    /// configured timeout below and surfaces as [`Error::Timeout`].
    ///
    /// The HTTP/SSE transport ([`Transport::Http`], [`Transport::Sse`], handled by
    /// the private `discover_via_http`) has **no** such bound (issue #226): `rmcp` 2.2.0's Streamable
    /// HTTP client transport buffers each JSON response body and each SSE event fully in
    /// memory before this crate's own [`MAX_TOOL_COUNT`]/[`MAX_SCHEMA_SIZE_BYTES`] checks
    /// ever run, with no size-limit config knob to bound that buffering. This is a known
    /// upstream gap, not something fixable from this crate without re-implementing a large
    /// part of `rmcp`'s HTTP transport. The only mitigation in place is
    /// [`ServerConfig::discover_timeout`], which bounds how long an unbounded response can
    /// be read for, not how large it can grow. rmcp 3.0.0-beta.2 adds the missing
    /// `max_sse_event_size` config knob (still only for SSE events, not JSON response
    /// bodies); revisit this gap once rmcp ships a 3.0.0 stable release with that fix.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_introspector::Introspector;
    /// use mcp_execution_core::{ServerId, ServerConfig};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    /// let server_id = ServerId::new("github").unwrap();
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

        // Defense in depth: since #313, `ServerConfig` can only be obtained already validated
        // (`ServerConfigBuilder::build` or its `Deserialize` impl both run
        // `validate_server_config` internally, and its fields are private). Re-validating here
        // anyway keeps this method self-defending against a future construction path that
        // forgets to, rather than relying solely on that invariant holding elsewhere.
        validate_server_config(config)?;

        let discovery = match config.transport() {
            Transport::Stdio { command, .. } => {
                discover_via_stdio_process(&server_id, command, config).await?
            }
            Transport::Http { .. } | Transport::Sse { .. } => {
                discover_via_http(&server_id, config).await?
            }
        };

        let info = build_server_info(&server_id, discovery.peer_meta, discovery.tools)?;

        // Keyed by `info.id` (not the `server_id` parameter above) so the map key is
        // structurally derived from the value's own identity — the two cannot drift apart,
        // rather than merely happening to agree because both were built from the same
        // `server_id` local.
        self.servers.insert(info.id.clone(), info.clone());

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
    /// let server_id = ServerId::new("test").unwrap();
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
    /// let server_id = ServerId::new("test").unwrap();
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
    /// introspector.discover_server(ServerId::new("s1").unwrap(), &config1).await?;
    /// introspector.discover_server(ServerId::new("s2").unwrap(), &config2).await?;
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
fn spawn_introspection_child(
    server_id: &ServerId,
    command: &str,
    config: &ServerConfig,
) -> Result<Child> {
    // `command` comes from the caller's `Transport::Stdio { command, .. }` destructure, so a
    // missing command is unrepresentable here — no `config.command().unwrap_or_default()`
    // fallback (and its empty-string sentinel) needed.
    let mut command = tokio::process::Command::new(command);
    command.args(config.args());
    command.envs(config.env());
    if let Some(cwd) = config.cwd() {
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
    command: &str,
    config: &ServerConfig,
) -> Result<DiscoveryResult> {
    let mut child = spawn_introspection_child(server_id, command, config)?;
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
            resource: ResourceKind::ToolCount {
                server_id: server_id.clone(),
            },
            actual,
            limit: MAX_TOOL_COUNT,
        },
    }
}

/// [`Decoder`] wrapper around [`JsonRpcMessageCodec`] that folds a recoverable per-line
/// error (oversized, malformed, or a skipped non-standard notification) into a decoded
/// `Item` instead of a `Decoder::Error` (issue #225).
///
/// This is required for correctness, not just style. `tokio_util`'s `FramedImpl` treats
/// *any* `Decoder::decode` `Err` as terminal for the current read: on the very next poll
/// it sets `is_readable = false` and returns `None` without ever calling `decode` again
/// on the buffer that error came from, instead going straight to another underlying
/// `poll_read`. The same happens on a bare `Ok(None)` — including the "skip non-standard
/// message" case inside [`JsonRpcMessageCodec::decode`], which still consumes the
/// skipped line from the buffer before returning it. If a well-formed message is already
/// sitting in the buffer right behind the bad or skipped line — realistic any time a
/// server flushes `"log line\n{...valid response...}\n"` in one write, exactly the case
/// the recoverable-error policy exists for — and the peer sends nothing further because
/// it is waiting on us, that next `poll_read` blocks forever: the buffered message is
/// never redelivered, and discovery stalls until [`Error::Timeout`] instead of the
/// bounded-but-tolerant behavior intended. The `AsyncRwTransport`/`BufReader::read_until`
/// this replaces did not have this failure mode, since it serves the next already-read
/// line from its own buffer without another I/O read.
///
/// Wrapping the codec avoids this: `decode`/`decode_eof` here only ever return
/// [`std::io::Error`] for a genuine I/O fault (never a plain `Ok(None)` after consuming
/// bytes, and never an `Err` for a recoverable case), so `FramedImpl` never leaves its
/// normal readable/framing state on a recoverable error — it keeps calling `decode`
/// against the residual buffer exactly as it does for any other successfully decoded
/// frame. The inner loop mirrors that: whenever [`JsonRpcMessageCodec::decode`] consumes
/// bytes without producing an `Item` (the skip case), this decoder calls it again
/// immediately rather than reporting "needs more data", since a further frame may
/// already be sitting in what is left of the buffer.
struct BoundedResponseDecoder {
    inner: JsonRpcMessageCodec<RxJsonRpcMessage<RoleClient>>,
    /// Resumable cursor for [`Self::peek_blank_line`]: how many leading bytes of the
    /// current buffer are already known to contain no newline. Mirrors the inner codec's
    /// own scan-resume behavior (its private `next_index`) so a long, non-blank line
    /// built up over many small reads is peeked once in total rather than re-scanned
    /// from the front on every call — see [`Self::peek_blank_line`] for why this state
    /// must be kept separately rather than driving `buf` from outside the codec.
    blank_scan_from: usize,
    /// Set once `decode_step` reports [`JsonRpcMessageCodecError::MaxLineLengthExceeded`]
    /// and cleared once it reports a message or a `Serde` error. While set, [`Self::drive`]
    /// ignores [`Self::peek_blank_line`]'s result: the inner codec may still be discarding
    /// the tail of that oversized line, so the bytes at the front of `buf` are that tail
    /// rather than a genuine next line, and treating them as blank would misattribute the
    /// following line's own parse error to blank-line suppression instead of logging it.
    assume_mid_discard: bool,
}

impl BoundedResponseDecoder {
    /// Read-only check for whether the *next* line the inner codec is about to consume
    /// is empty or whitespace-only. Returns `Some(true/false)` once a full line (a `\n`
    /// within `max_length + 1` bytes of the front of `buf`) is buffered; `None` if no
    /// newline is in reach yet, in which case `scan_from` is advanced to the bound
    /// already checked so the next call resumes instead of re-scanning.
    ///
    /// Deliberately never mutates `buf`: an earlier version of this fix called
    /// `buf.advance()` directly from outside [`JsonRpcMessageCodec`], which desynchronized
    /// the codec's own private `next_index`/`is_discarding` bookkeeping from `buf`'s real
    /// contents — causing an out-of-range slice panic when a whitespace-only line was
    /// split across two reads, and silently dropping the next valid response when the
    /// codec was mid-discard on an oversized line. Keeping all buffer consumption inside
    /// the codec (this function only peeks) makes that class of desync impossible: the
    /// codec's state can never disagree with `buf` because nothing else ever changes it.
    fn peek_blank_line(scan_from: &mut usize, max_length: usize, buf: &BytesMut) -> Option<bool> {
        let bound = std::cmp::min(max_length.saturating_add(1), buf.len());
        if *scan_from >= bound {
            return None;
        }
        if let Some(offset) = buf[*scan_from..bound]
            .iter()
            .position(|&byte| byte == b'\n')
        {
            let newline_at = *scan_from + offset;
            Some(buf[..newline_at].iter().all(u8::is_ascii_whitespace))
        } else {
            *scan_from = bound;
            None
        }
    }

    /// Runs `decode_step` in a loop, following [`JsonRpcMessageCodec`]'s "skip and keep
    /// scanning" behavior: an `Ok(None)` that consumed bytes may have a further frame
    /// sitting right behind it, so `decode_step` is called again immediately; an `Ok(None)`
    /// that made no progress genuinely means more input is needed, and a recoverable
    /// [`JsonRpcMessageCodecError`] is reported as an `Item` rather than looped on, so the
    /// caller can log it once per bad line instead of the underlying codec's own internal
    /// discard loop being observed as a single opaque error.
    ///
    /// A [`JsonRpcMessageCodecError::Serde`] for a line [`Self::peek_blank_line`] already
    /// identified as blank is looped on instead of reported, so blank lines never produce
    /// a logged `Item` (issue #275); `scan_from` is reset whenever `decode_step` actually
    /// consumes bytes, since a shorter buffer invalidates any previously-checked range.
    /// `assume_mid_discard` gates whether the peek is trusted at all — see its field doc.
    fn drive(
        buf: &mut BytesMut,
        max_length: usize,
        scan_from: &mut usize,
        assume_mid_discard: &mut bool,
        mut decode_step: impl FnMut(
            &mut BytesMut,
        ) -> std::result::Result<
            Option<RxJsonRpcMessage<RoleClient>>,
            JsonRpcMessageCodecError,
        >,
    ) -> std::io::Result<
        Option<std::result::Result<RxJsonRpcMessage<RoleClient>, JsonRpcMessageCodecError>>,
    > {
        loop {
            let is_blank = !*assume_mid_discard
                && Self::peek_blank_line(scan_from, max_length, buf).unwrap_or(false);
            let before = buf.len();
            let step_result = decode_step(buf);
            if buf.len() < before {
                *scan_from = 0;
            }
            match &step_result {
                Ok(Some(_)) | Err(JsonRpcMessageCodecError::Serde(_)) => {
                    *assume_mid_discard = false;
                }
                Err(JsonRpcMessageCodecError::MaxLineLengthExceeded) => {
                    *assume_mid_discard = true;
                }
                Ok(None) | Err(_) => {}
            }
            return match step_result {
                Ok(Some(message)) => Ok(Some(Ok(message))),
                Ok(None) => {
                    if buf.len() < before {
                        continue;
                    }
                    Ok(None)
                }
                Err(JsonRpcMessageCodecError::Io(error)) => Err(error),
                Err(JsonRpcMessageCodecError::Serde(_)) if is_blank => continue,
                Err(error) => Ok(Some(Err(error))),
            };
        }
    }
}

impl Decoder for BoundedResponseDecoder {
    type Item = std::result::Result<RxJsonRpcMessage<RoleClient>, JsonRpcMessageCodecError>;
    type Error = std::io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> std::io::Result<Option<Self::Item>> {
        let max_length = self.inner.max_length();
        let inner = &mut self.inner;
        Self::drive(
            buf,
            max_length,
            &mut self.blank_scan_from,
            &mut self.assume_mid_discard,
            |buf| inner.decode(buf),
        )
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> std::io::Result<Option<Self::Item>> {
        let max_length = self.inner.max_length();
        let inner = &mut self.inner;
        Self::drive(
            buf,
            max_length,
            &mut self.blank_scan_from,
            &mut self.assume_mid_discard,
            |buf| inner.decode_eof(buf),
        )
    }
}

/// Wraps a size-bounded [`FramedRead`] over an MCP server's stdout so one oversized,
/// malformed, or skipped response line is dropped without ending the introspection
/// session (via [`BoundedResponseDecoder`]), while a genuine I/O error still ends it
/// (issue #225).
///
/// Client-side counterpart to `mcp-execution-server`'s `bounded_request_stream`, but
/// deliberately simpler — it must not be shared with that server-side helper:
/// - No concurrency admission gate. `rmcp`'s `serve_inner` is role-generic and does spawn
///   an unbounded `tokio::spawn` per inbound request for [`RoleClient`] too, the same
///   mechanism the server-side gate defends against — so the omission here is not because
///   the client "has no inbound requests to admit". It is safe for different reasons: the
///   handler driving this stream is `()` (the unit `Service` impl), so each such spawned
///   task is a trivial method-not-found reply rather than the server's own handlers (which
///   can run for minutes and allocate hundreds of MB); the session as a whole is already
///   bounded by `config`'s `connect_timeout`/`discover_timeout`; and the child process is
///   killed unconditionally once discovery returns (see [`discover_via_stdio_process`]).
/// - [`JsonRpcMessageCodecError::Serde`] is recoverable here, not just
///   `MaxLineLengthExceeded`: the `AsyncRwTransport` this replaces already tolerated any
///   unparsable stdout line, and real MCP servers commonly log free-form text to stdout
///   alongside the protocol stream. Treating a parse failure as fatal here would regress
///   introspection against servers that work fine today.
fn bounded_response_stream<R>(
    reader: R,
    max_length: usize,
) -> impl Stream<Item = RxJsonRpcMessage<RoleClient>> + Send + Unpin + 'static
where
    R: AsyncRead + Send + Unpin + 'static,
{
    let mut framed = FramedRead::new(
        reader,
        BoundedResponseDecoder {
            inner: JsonRpcMessageCodec::new_with_max_length(max_length),
            blank_scan_from: 0,
            assume_mid_discard: false,
        },
    );
    stream::poll_fn(move |cx| {
        loop {
            return match framed.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(Ok(message)))) => Poll::Ready(Some(message)),
                Poll::Ready(Some(Ok(Err(error)))) => {
                    tracing::warn!(%error, "dropping oversized or malformed response line from MCP server");
                    continue;
                }
                Poll::Ready(Some(Err(error))) => {
                    tracing::error!(%error, "introspection stdout read failed; ending discovery session");
                    Poll::Ready(None)
                }
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            };
        }
    })
}

/// Drives the connect / list-tools / peer-meta pipeline shared by
/// [`discover_via_stdio`] and [`discover_via_http`] (issue #294): awaits
/// `connect` bounded by `config.connect_timeout()`, then
/// [`list_tools_bounded`] bounded by `config.discover_timeout()`, then
/// extracts [`PeerMeta`] from the resulting client's handshake info. The two
/// callers differ only in how `connect` builds its transport — both produce
/// the same `RunningService<RoleClient, ()>` client type from that point on.
///
/// # Errors
///
/// Returns [`Error::Timeout`] if the connect or discovery step exceeds its
/// configured timeout, [`Error::ConnectionFailed`] if `connect` or the
/// tool-listing request fails, or [`Error::ResourceLimitExceeded`] if the
/// accumulated tool count exceeds [`MAX_TOOL_COUNT`] (see
/// [`list_tools_bounded`]).
async fn connect_and_list_tools<F, T>(
    server_id: &ServerId,
    config: &ServerConfig,
    connect: F,
) -> Result<DiscoveryResult>
where
    F: Future<Output = std::result::Result<rmcp::service::RunningService<RoleClient, ()>, T>>,
    T: std::error::Error + Send + Sync + 'static,
{
    // Bounded by the connect timeout.
    let client = tokio::time::timeout(config.connect_timeout(), connect)
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
    let peer_meta = extract_peer_meta(config, client.peer_info().as_deref());

    Ok(DiscoveryResult {
        tools: tool_list,
        peer_meta,
    })
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
/// configured timeout, [`Error::ConnectionFailed`] if the underlying rmcp
/// connection or request fails, or [`Error::ResourceLimitExceeded`] if the
/// accumulated tool count exceeds [`MAX_TOOL_COUNT`].
async fn discover_via_stdio(
    server_id: &ServerId,
    config: &ServerConfig,
    transport: (tokio::process::ChildStdout, tokio::process::ChildStdin),
) -> Result<DiscoveryResult> {
    // The default `(ChildStdout, ChildStdin)` transport (`AsyncRwTransport`) reads
    // lines via an unbounded `read_until`, bypassing `JsonRpcMessageCodec`'s
    // `max_length` entirely (issue #225). Building the sink/stream pair explicitly
    // routes stdout through `bounded_response_stream` instead.
    let (stdout, stdin) = transport;
    let sink = FramedWrite::new(
        stdin,
        JsonRpcMessageCodec::<TxJsonRpcMessage<RoleClient>>::new(),
    );
    let stream = bounded_response_stream(stdout, MAX_RESPONSE_LINE_SIZE);

    connect_and_list_tools(server_id, config, ().serve((sink, stream))).await
}

/// Connects to an MCP server over Streamable HTTP and lists its tools, with
/// each step bounded by `config`'s configured timeouts.
///
/// Used for both [`Transport::Http`] and [`Transport::Sse`]: rmcp 2.2
/// has a single client transport for network MCP servers ("Streamable
/// HTTP"), which superseded the standalone SSE transport in the 2025-03-26
/// MCP spec revision. There is no legacy SSE-only client to fall back to.
///
/// Unlike stdio discovery, this path has no response-size bound (issue #226): `rmcp`
/// 2.2.0's Streamable HTTP client transport buffers each JSON response body and SSE
/// event fully in memory with no config knob to cap it, and this crate has no
/// injection point to add one without re-implementing a large part of that transport.
/// See [`Introspector::discover_server`]'s `# Security` docs for the full rationale and
/// the upstream condition to revisit this under.
///
/// # Errors
///
/// Returns [`Error::Timeout`] if the connect or discovery step exceeds its
/// configured timeout, [`Error::ConnectionFailed`] if a header cannot be
/// constructed, or the underlying rmcp connection or request fails (this
/// includes a reserved-header collision, e.g. a caller-supplied `Accept`
/// header, which rmcp rejects as `StreamableHttpError::ReservedHeaderConflict`),
/// or [`Error::ResourceLimitExceeded`] if the accumulated tool count exceeds
/// [`MAX_TOOL_COUNT`].
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

    // The client `connect_and_list_tools` builds internally is dropped when it returns, which
    // triggers `WorkerTransport`'s drop guard, cancelling the worker task; a cancelled worker
    // cannot itself issue a session-DELETE request, so no explicit disconnect happens on this path —
    // the server-side session simply expires on its own timeout instead.
    connect_and_list_tools(server_id, config, ().serve(transport)).await
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
            resource: ResourceKind::ToolCount {
                server_id: server_id.clone(),
            },
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
/// `description`, and serialized `input_schema`/`output_schema` size (denial-of-service
/// protection, CWE-400) against a malicious or misbehaving MCP server.
///
/// `(*tool.input_schema).clone()` below, and both `serde_json::to_vec` calls, walk the
/// schema's full tree recursively with no depth limit of their own (`Value`'s `Clone` impl and
/// its `Serialize` implementation are both unconditionally recursive) — the same class of
/// unguarded recursion `mcp-execution-codegen`'s `MAX_SCHEMA_RECURSION_DEPTH` defends against
/// (issue #303). That cap is not duplicated here because it doesn't need to be: by the time a
/// `Tool` reaches this function, `tool.input_schema`/`tool.output_schema` were already produced
/// by deserializing this server's `tools/list` response (via this crate's own JSON-RPC decoder
/// for the stdio transport, or `rmcp`'s HTTP transport for the HTTP/SSE case), and `serde_json`
/// enforces its own default recursion limit (128) while deserializing — nothing in this
/// workspace's dependency tree raises or disables it (no `disable_recursion_limit`/
/// `unbounded_depth`). A schema nested deep enough to threaten a recursive `clone`/serialize
/// here would already have failed to deserialize into a `Tool` in the first place, surfacing as
/// a recoverable parse error rather than reaching this function. See
/// `mcp_execution_codegen::common::typescript::MAX_SCHEMA_RECURSION_DEPTH`'s docs for the
/// measured reachable-depth ceiling this reasoning is based on.
///
/// # Errors
///
/// Returns [`Error::ResourceLimitExceeded`] if the tool's name exceeds [`MAX_TOOL_NAME_LEN`],
/// its description exceeds [`MAX_TOOL_DESCRIPTION_LEN`], or its serialized input or output
/// schema exceeds [`MAX_SCHEMA_SIZE_BYTES`]. Returns [`Error::ValidationError`] if the tool's
/// name fails [`ToolName::new`]'s invariant (e.g. it is empty or contains a path separator) —
/// like every other check in this function, this hard-fails the caller's whole
/// [`build_server_info`] call (via `?`-propagation through
/// `.collect::<Result<Vec<_>>>()`) rather than skipping just the one malformed tool with a
/// warning: this is deliberate, matching how an oversized name/description/schema is already
/// handled, since a tool this project can't safely name (it's used to derive an output file
/// name downstream) shouldn't produce a partial, silently-incomplete result from an untrusted
/// server (#287).
fn build_tool_info(tool: rmcp::model::Tool) -> Result<ToolInfo> {
    let name = tool.name.to_string();

    if name.len() > MAX_TOOL_NAME_LEN {
        return Err(Error::ResourceLimitExceeded {
            resource: ResourceKind::ToolNameLength,
            actual: name.len(),
            limit: MAX_TOOL_NAME_LEN,
        });
    }

    // Logged only after the length check above, and through the shared untrusted-metadata
    // sanitizer (control characters replaced with a space) rather than the raw MCP-supplied
    // name: this project's `fmt::layer()` subscriber (`mcp-execution-server`'s `main.rs`)
    // renders trace fields verbatim, so an unsanitized name would let a malicious server plant
    // an ANSI escape sequence (or similar) directly on the operator's terminal under
    // `RUST_LOG=trace`.
    tracing::trace!(
        tool.name = %mcp_execution_core::untrusted::sanitize_untrusted_text(
            &name,
            mcp_execution_core::untrusted::MAX_UNTRUSTED_FIELD_LEN,
        ),
        "Found tool"
    );

    let description = tool.description.unwrap_or_default().to_string();
    if description.len() > MAX_TOOL_DESCRIPTION_LEN {
        return Err(Error::ResourceLimitExceeded {
            resource: ResourceKind::DescriptionLength { tool_name: name },
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
            resource: ResourceKind::InputSchemaSize { tool_name: name },
            actual: schema_size,
            limit: MAX_SCHEMA_SIZE_BYTES,
        });
    }

    let output_schema = tool
        .output_schema
        .map(|schema| serde_json::Value::Object((*schema).clone()));
    if let Some(ref schema) = output_schema {
        // Same pathological-serialization treatment as input_schema above.
        let schema_size = serde_json::to_vec(schema).map_or(usize::MAX, |bytes| bytes.len());
        if schema_size > MAX_SCHEMA_SIZE_BYTES {
            return Err(Error::ResourceLimitExceeded {
                resource: ResourceKind::OutputSchemaSize { tool_name: name },
                actual: schema_size,
                limit: MAX_SCHEMA_SIZE_BYTES,
            });
        }
    }

    let name = ToolName::new(name).map_err(|err| Error::ValidationError {
        field: "tool name".to_string(),
        reason: err.to_string(),
    })?;

    Ok(ToolInfo {
        name,
        description,
        input_schema,
        output_schema,
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
    peer_info: Option<&rmcp::model::ServerPeerInfo>,
) -> PeerMeta {
    peer_info.map_or_else(
        || PeerMeta {
            server_name: fallback_server_name(config),
            server_version: "unknown".to_string(),
            has_resources: false,
            has_prompts: false,
        },
        |info| PeerMeta {
            // `ServerPeerInfo::server_info` is optional (unlike `InitializeResult`'s), since
            // a server is not required to send its implementation identity on handshake.
            server_name: info
                .server_info
                .as_ref()
                .map_or_else(|| fallback_server_name(config), |si| si.name.clone()),
            server_version: info
                .server_info
                .as_ref()
                .map_or_else(|| "unknown".to_string(), |si| si.version.clone()),
            has_resources: info.capabilities.resources.is_some(),
            has_prompts: info.capabilities.prompts.is_some(),
        },
    )
}

/// Picks a display name for a server that sent no `peer_info` on handshake.
///
/// Prefers `config.command()` (stdio transport); falls back to `config.url()`
/// since Http/Sse transports never have a `command`.
fn fallback_server_name(config: &ServerConfig) -> String {
    config
        .command()
        .or_else(|| config.url())
        .unwrap_or_default()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── bounded_response_stream (issue #225) ─────────────────────────────────

    mod bounded_response_stream_tests {
        use super::*;
        use std::io;
        use std::pin::Pin;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicUsize, Ordering};
        use std::task::Context;
        use tokio::io::ReadBuf;
        use tokio_util::bytes::Buf;

        const TEST_MAX: usize = 64;

        fn oversized_line() -> Vec<u8> {
            let mut line = vec![b'x'; TEST_MAX * 3];
            line.push(b'\n');
            line
        }

        fn valid_notification_line() -> Vec<u8> {
            br#"{"jsonrpc":"2.0","method":"notifications/tools/list_changed"}"#
                .iter()
                .copied()
                .chain(std::iter::once(b'\n'))
                .collect()
        }

        fn malformed_json_line() -> Vec<u8> {
            b"{not valid json\n".to_vec()
        }

        /// `AsyncRead` that yields a fixed script of chunks in order, then a clean EOF.
        struct Script {
            chunks: Vec<Vec<u8>>,
            idx: usize,
        }

        impl AsyncRead for Script {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                buf: &mut ReadBuf<'_>,
            ) -> Poll<io::Result<()>> {
                if self.idx < self.chunks.len() {
                    let chunk = self.chunks[self.idx].clone();
                    self.idx += 1;
                    debug_assert!(
                        chunk.len() <= buf.remaining(),
                        "test fixture chunk exceeds the reader's spare buffer capacity"
                    );
                    buf.put_slice(&chunk);
                    return Poll::Ready(Ok(()));
                }
                Poll::Ready(Ok(())) // 0 bytes read signals EOF
            }
        }

        /// Minimal `tracing::Subscriber` that counts WARN-level events on the calling
        /// thread, so a test can assert exactly how many warnings `bounded_response_stream`
        /// logged without pulling in a tracing-capture crate for one assertion. Install via
        /// `tracing::subscriber::set_default`, which is thread-local; `#[tokio::test]` uses
        /// a current-thread runtime by default, so the whole test body — including every
        /// `.await` — runs on the thread the guard was set on.
        struct WarnCounter(Arc<AtomicUsize>);

        impl tracing::Subscriber for WarnCounter {
            fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
                *metadata.level() == tracing::Level::WARN
            }

            fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
                tracing::span::Id::from_u64(1)
            }

            fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

            fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {
            }

            fn event(&self, event: &tracing::Event<'_>) {
                if *event.metadata().level() == tracing::Level::WARN {
                    self.0.fetch_add(1, Ordering::SeqCst);
                }
            }

            fn enter(&self, _span: &tracing::span::Id) {}

            fn exit(&self, _span: &tracing::span::Id) {}
        }

        /// Number of times `ErrAfter` returns a real error before giving up and signaling
        /// EOF. Bounds the fixture itself so that if a regression ever makes
        /// `bounded_response_stream` treat an I/O error as recoverable again, the resulting
        /// test fails its assertion instead of spinning forever with no `Poll::Pending`.
        const MAX_ERR_AFTER_POLLS: usize = 8;

        /// `AsyncRead` that yields a fixed script, then a persistent I/O error up to
        /// `MAX_ERR_AFTER_POLLS` times, then a clean EOF; counts how many errors it returned.
        struct ErrAfter {
            chunks: Vec<Vec<u8>>,
            idx: usize,
            error_polls: Arc<AtomicUsize>,
        }

        impl AsyncRead for ErrAfter {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                buf: &mut ReadBuf<'_>,
            ) -> Poll<io::Result<()>> {
                if self.idx < self.chunks.len() {
                    let chunk = self.chunks[self.idx].clone();
                    self.idx += 1;
                    debug_assert!(
                        chunk.len() <= buf.remaining(),
                        "test fixture chunk exceeds the reader's spare buffer capacity"
                    );
                    buf.put_slice(&chunk);
                    return Poll::Ready(Ok(()));
                }
                if self.error_polls.fetch_add(1, Ordering::SeqCst) >= MAX_ERR_AFTER_POLLS {
                    return Poll::Ready(Ok(())); // give up: signal EOF so a regression fails loudly
                }
                Poll::Ready(Err(io::Error::other("persistent read failure")))
            }
        }

        #[tokio::test]
        async fn recovers_from_oversized_lines_and_keeps_reading() {
            let script = Script {
                chunks: vec![
                    oversized_line(),
                    oversized_line(),
                    valid_notification_line(),
                ],
                idx: 0,
            };
            let mut stream = bounded_response_stream(script, TEST_MAX);

            assert!(
                stream.next().await.is_some(),
                "the trailing valid line must still decode after two oversized lines"
            );
            assert!(
                stream.next().await.is_none(),
                "stream ends cleanly at EOF after the valid message"
            );
        }

        #[tokio::test]
        async fn recovers_from_malformed_json_and_keeps_reading() {
            // Diverges from mcp-server's request-side helper's threat model: a noisy
            // third-party MCP server logging free-form text to stdout must not abort
            // discovery, since `AsyncRwTransport` (the path this replaces) tolerated it.
            let script = Script {
                chunks: vec![malformed_json_line(), valid_notification_line()],
                idx: 0,
            };
            let mut stream = bounded_response_stream(script, TEST_MAX);

            assert!(
                stream.next().await.is_some(),
                "the trailing valid line must still decode after a malformed line"
            );
            assert!(stream.next().await.is_none());
        }

        #[tokio::test]
        async fn skips_blank_lines_silently_and_keeps_reading() {
            // Bare newlines, a whitespace-only line, and a CRLF blank line must all be
            // dropped without ever reaching `JsonRpcMessageCodec` (issue #275) — unlike
            // a malformed line, they must not surface as a decoded `Err` item at all.
            let script = Script {
                chunks: vec![
                    b"\n".to_vec(),
                    b"   \n".to_vec(),
                    b"\r\n".to_vec(),
                    valid_notification_line(),
                ],
                idx: 0,
            };
            let mut stream = bounded_response_stream(script, TEST_MAX);

            assert!(
                stream.next().await.is_some(),
                "the valid line must decode with no item emitted for the blank lines before it"
            );
            assert!(stream.next().await.is_none());
        }

        #[test]
        fn peek_blank_line_leaves_buf_untouched_for_non_blank_lines() {
            let buf = BytesMut::from(&b"{not valid json\nnext"[..]);
            let mut scan_from = 0;
            assert_eq!(
                BoundedResponseDecoder::peek_blank_line(&mut scan_from, TEST_MAX, &buf),
                Some(false)
            );
            assert_eq!(
                &buf[..],
                b"{not valid json\nnext",
                "peek must never mutate buf"
            );
            assert_eq!(
                scan_from, 0,
                "a found newline must not perturb the cursor — only the 'no newline yet' \
                 branch advances it"
            );
        }

        #[test]
        fn peek_blank_line_returns_none_without_a_complete_line() {
            let buf = BytesMut::from(&b"   "[..]);
            let mut scan_from = 0;
            assert_eq!(
                BoundedResponseDecoder::peek_blank_line(&mut scan_from, TEST_MAX, &buf),
                None
            );
            assert_eq!(&buf[..], b"   ");
            assert_eq!(
                scan_from,
                buf.len(),
                "the cursor must advance to the bound already checked, so a later call \
                 resumes instead of re-scanning these bytes"
            );
        }

        #[test]
        fn peek_blank_line_resumes_from_scan_from_instead_of_rescanning() {
            // A prior call already established there's no newline in the first 5 bytes;
            // a second call must not redo that work even though it's still true of the
            // now-longer buffer, and must find the newline that arrived after it.
            let buf = BytesMut::from(&b"     \n"[..]);
            let mut scan_from = 5;
            assert_eq!(
                BoundedResponseDecoder::peek_blank_line(&mut scan_from, TEST_MAX, &buf),
                Some(true)
            );
        }

        #[test]
        fn peek_blank_line_trusts_the_resumed_cursor_instead_of_rescanning_from_zero() {
            // `scan_from = 1` asserts "buf[0..1] has no newline" even though buf[0] is
            // one — a precondition a real caller never violates, but exploiting it here
            // is what makes this test discriminating: a regression that silently starts
            // every scan at 0 (ignoring the passed-in cursor) would "discover" that
            // leading newline and answer `Some(true)`. Only a real caller of `drive`
            // could trigger the regression this guards, since `drive` is what threads
            // `scan_from` across calls and would be the one to stop honoring it; this
            // unit test exercises `peek_blank_line` directly because it is the cheapest
            // way to pin the cursor's exact meaning (see M2, critic handoff
            // 2026-07-26T22-33-10).
            let buf = BytesMut::from(&b"\n    "[..]);
            let mut scan_from = 1;
            assert_eq!(
                BoundedResponseDecoder::peek_blank_line(&mut scan_from, TEST_MAX, &buf),
                None,
                "search must start at scan_from, not re-scan from the front of buf"
            );
            assert_eq!(
                scan_from,
                buf.len(),
                "cursor must advance to the bound reached by the resumed search"
            );
        }

        #[tokio::test]
        async fn splits_whitespace_only_line_across_reads_without_panicking() {
            // Regression for a desync bug in an earlier version of this fix: calling
            // `buf.advance()` from outside `JsonRpcMessageCodec` while a whitespace-only
            // line was still incomplete left the codec's private scan cursor pointing
            // past the (now shorter) buffer, panicking on the next decode. The blank-line
            // fast path must only ever peek, never mutate `buf` itself.
            let script = Script {
                chunks: vec![
                    b"          ".to_vec(), // 10 spaces, no newline yet
                    b"\n".to_vec(),
                    valid_notification_line(),
                ],
                idx: 0,
            };
            let mut stream = bounded_response_stream(script, TEST_MAX);

            assert!(
                stream.next().await.is_some(),
                "the valid line must still decode after a blank line split across two reads"
            );
            assert!(stream.next().await.is_none());
        }

        #[tokio::test]
        async fn recovers_two_valid_responses_after_oversized_whitespace_run() {
            // Regression: an earlier version of this fix let its own blank-line skip
            // consume the newline that ends an oversized, all-whitespace run while the
            // inner codec was still mid-discard for that same run, desynchronizing the
            // codec's `is_discarding` state from `buf` and silently dropping the next
            // valid response. The run's terminating newline must be in a *separate* read
            // from the run itself — putting them in one chunk (as an earlier version of
            // this test did) never leaves the codec mid-discard before the blank-line
            // logic runs, so it can't catch the desync at all (tester handoff
            // 2026-07-26T22-28-30). A single trailing response isn't enough either: with
            // only one, the old bug degenerates into the split-blank-line panic that
            // `splits_whitespace_only_line_across_reads_without_panicking` already
            // covers, rather than demonstrating the silent-loss variant this test targets
            // — two responses are what let the buggy code stay panic-free while still
            // dropping the second one.
            let warn_count = Arc::new(AtomicUsize::new(0));
            let _tracing_guard =
                tracing::subscriber::set_default(WarnCounter(Arc::clone(&warn_count)));

            let oversized_whitespace_run = vec![b' '; TEST_MAX * 3]; // no trailing newline
            let mut rest = vec![b'\n'];
            rest.extend(valid_notification_line());
            rest.extend(valid_notification_line());
            let script = Script {
                chunks: vec![oversized_whitespace_run, rest],
                idx: 0,
            };
            let mut stream = bounded_response_stream(script, TEST_MAX);

            assert!(
                stream.next().await.is_some(),
                "the first response after the oversized whitespace run must decode"
            );
            assert!(
                stream.next().await.is_some(),
                "the second response after the oversized whitespace run must not be \
                 silently dropped"
            );
            assert!(stream.next().await.is_none());
            assert_eq!(
                warn_count.load(Ordering::SeqCst),
                1,
                "exactly one warning for the oversized run itself — no blank-line noise, \
                 and no swallowed diagnostic either"
            );
        }

        #[tokio::test]
        async fn warns_for_malformed_line_immediately_after_oversized_discard() {
            // Regression for critic finding M3 (handoff 2026-07-26T22-33-10): right after
            // the codec finishes discarding an oversized line, the bytes now at the front
            // of `buf` are the *next* line, but a stale `is_blank` computed before
            // `decode_step` ran could be mis-attributed to that next line's own parse
            // error, swallowing its warning even though no message was lost. Both the
            // oversized-run warning and the malformed-line warning must be reported.
            let oversized_whitespace_run = vec![b' '; TEST_MAX * 3]; // no trailing newline
            let mut rest = b"\nnot json at all\n".to_vec();
            rest.extend(valid_notification_line());
            let script = Script {
                chunks: vec![oversized_whitespace_run, rest],
                idx: 0,
            };

            let warn_count = Arc::new(AtomicUsize::new(0));
            let _tracing_guard =
                tracing::subscriber::set_default(WarnCounter(Arc::clone(&warn_count)));
            let mut stream = bounded_response_stream(script, TEST_MAX);

            assert!(
                stream.next().await.is_some(),
                "the valid response after the malformed line must still decode"
            );
            assert!(stream.next().await.is_none());
            assert_eq!(
                warn_count.load(Ordering::SeqCst),
                2,
                "one warning for the oversized run and one for the malformed line that \
                 follows it — neither is blank, so neither may be suppressed"
            );
        }

        #[tokio::test]
        async fn ends_cleanly_on_unterminated_oversized_line_then_eof() {
            let script = Script {
                chunks: vec![vec![b'y'; TEST_MAX * 3]], // no trailing newline
                idx: 0,
            };
            let mut stream = bounded_response_stream(script, TEST_MAX);
            assert!(stream.next().await.is_none());
        }

        #[tokio::test]
        async fn ends_session_on_persistent_io_error_without_spinning() {
            let error_polls = Arc::new(AtomicUsize::new(0));
            let reader = ErrAfter {
                chunks: vec![valid_notification_line()],
                idx: 0,
                error_polls: error_polls.clone(),
            };
            let mut stream = bounded_response_stream::<ErrAfter>(reader, TEST_MAX);

            assert!(
                stream.next().await.is_some(),
                "the valid line decodes before the reader starts failing"
            );
            assert!(
                stream.next().await.is_none(),
                "a persistent I/O error must end the session rather than recover"
            );
            assert_eq!(
                error_polls.load(Ordering::SeqCst),
                1,
                "the I/O error must be surfaced on the first failing poll, not retried in a hot loop"
            );
        }

        #[tokio::test]
        async fn accepts_line_at_exact_cap_and_rejects_one_byte_over() {
            // The codec's length check runs on raw byte count before any JSON parsing, so
            // the rejected case doesn't need to be valid JSON — only the accepted case does.
            let mut content = valid_notification_line();
            assert_eq!(content.pop(), Some(b'\n'), "fixture must end in a newline");
            let boundary_max = content.len();

            let mut at_cap = content.clone();
            at_cap.push(b'\n');
            let mut stream = bounded_response_stream(
                Script {
                    chunks: vec![at_cap],
                    idx: 0,
                },
                boundary_max,
            );
            assert!(
                stream.next().await.is_some(),
                "a line whose content is exactly max_length bytes must be accepted"
            );
            assert!(stream.next().await.is_none());

            let mut one_over = content;
            one_over.push(b' ');
            one_over.push(b'\n');
            let mut stream = bounded_response_stream(
                Script {
                    chunks: vec![one_over],
                    idx: 0,
                },
                boundary_max,
            );
            assert!(
                stream.next().await.is_none(),
                "a line one byte over max_length must be dropped, not accepted"
            );
        }

        // ── C1 regression: a message already buffered behind a bad line must not
        // strand until another underlying read arrives (see `BoundedResponseDecoder`'s
        // doc comment). `Script` above delivers each scripted chunk on its own
        // `poll_read`, which never exercises this: it always gives `FramedRead` a
        // fresh underlying read to recover on, masking the bug. `Idle` below instead
        // delivers everything in one `poll_read` and then returns `Poll::Pending`
        // forever — modeling a peer that flushed once and is now waiting on us, the
        // scenario that actually stalled before this fix.

        /// `AsyncRead` that delivers a fixed script of chunks, then returns
        /// `Poll::Pending` forever — never EOF — modeling a peer that flushed once and
        /// is now idle awaiting our next request.
        struct Idle {
            chunks: Vec<Vec<u8>>,
            idx: usize,
        }

        impl AsyncRead for Idle {
            fn poll_read(
                mut self: Pin<&mut Self>,
                _cx: &mut Context<'_>,
                buf: &mut ReadBuf<'_>,
            ) -> Poll<io::Result<()>> {
                if self.idx < self.chunks.len() {
                    let chunk = self.chunks[self.idx].clone();
                    self.idx += 1;
                    debug_assert!(
                        chunk.len() <= buf.remaining(),
                        "test fixture chunk exceeds the reader's spare buffer capacity"
                    );
                    buf.put_slice(&chunk);
                    return Poll::Ready(Ok(()));
                }
                Poll::Pending
            }
        }

        /// Polls `stream.next()` with a bounded timeout so a regression that strands a
        /// buffered message (issue #225 C1) fails the test with a clear panic instead
        /// of hanging.
        async fn next_or_timeout<S>(stream: &mut S) -> Option<S::Item>
        where
            S: Stream + Unpin,
        {
            tokio::time::timeout(std::time::Duration::from_secs(2), stream.next())
                .await
                .expect("stream.next() must resolve within 2s instead of stalling")
        }

        #[tokio::test]
        async fn recovers_within_a_single_read_malformed_then_valid() {
            let mut chunk = malformed_json_line();
            chunk.extend(valid_notification_line());
            let mut stream = bounded_response_stream(
                Idle {
                    chunks: vec![chunk],
                    idx: 0,
                },
                TEST_MAX,
            );

            assert!(
                next_or_timeout(&mut stream).await.is_some(),
                "a valid message buffered right behind a malformed line in the same \
                 read must still be delivered, not stall until the peer sends more"
            );
        }

        #[tokio::test]
        async fn recovers_within_a_single_read_oversized_then_valid() {
            let mut chunk = oversized_line();
            chunk.extend(valid_notification_line());
            let mut stream = bounded_response_stream(
                Idle {
                    chunks: vec![chunk],
                    idx: 0,
                },
                TEST_MAX,
            );

            assert!(
                next_or_timeout(&mut stream).await.is_some(),
                "a valid message buffered right behind an oversized line in the same \
                 read must still be delivered, not stall until the peer sends more"
            );
        }

        // `JsonRpcMessageCodec`'s "skip non-standard message" path (`Ok(None)` after
        // consuming a line) is the third trigger for `BoundedResponseDecoder::drive`'s
        // internal retry loop, alongside the malformed/oversized cases above — but it
        // turns out to be unreachable through real JSON in this rmcp version: `rmcp` 2.2.0
        // added `CustomNotification`/`CustomRequest` catch-all variants to
        // `ServerNotification`/`ServerRequest` that structurally match any
        // notification/request-shaped JSON regardless of its `method` string, so any input
        // that would have hit the "skip" fallback in an older rmcp now just decodes
        // successfully as a `Custom*` message on the first attempt (verified directly
        // against `serde_json::from_str::<RxJsonRpcMessage<RoleClient>>`). `drive`'s retry
        // loop is exercised directly instead, independent of whether the underlying codec
        // can currently produce that shape — it is still part of `Decoder::decode`'s
        // documented contract and could reactivate for a future rmcp version or role.
        #[test]
        fn drive_retries_immediately_after_a_step_that_consumed_without_producing_an_item() {
            let message: RxJsonRpcMessage<RoleClient> = serde_json::from_slice(
                &valid_notification_line()[..valid_notification_line().len() - 1],
            )
            .expect("fixture notification must parse");
            let mut calls = 0u32;
            let mut buf = BytesMut::from(&b"stand-in bytes, contents are irrelevant"[..]);
            let mut scan_from = 0;
            let mut assume_mid_discard = false;

            let result = BoundedResponseDecoder::drive(
                &mut buf,
                TEST_MAX,
                &mut scan_from,
                &mut assume_mid_discard,
                |buf| {
                    calls += 1;
                    if calls == 1 {
                        // Mirrors the codec's own "skip" behavior: consumes bytes but
                        // produces no item.
                        let half = buf.len() / 2;
                        buf.advance(half);
                        Ok(None)
                    } else {
                        Ok(Some(message.clone()))
                    }
                },
            );

            assert!(
                matches!(result, Ok(Some(Ok(_)))),
                "a message must be delivered from the same `drive` call, not deferred to \
                 the next poll, when the prior decode step consumed bytes without \
                 producing an item"
            );
            assert_eq!(
                calls, 2,
                "decode_step must be retried immediately after it makes forward progress"
            );
        }

        #[test]
        fn drive_reports_needs_more_data_when_no_progress_is_made() {
            let mut buf = BytesMut::from(&b"an incomplete line with no delimiter yet"[..]);
            let mut scan_from = 0;
            let mut assume_mid_discard = false;
            let result = BoundedResponseDecoder::drive(
                &mut buf,
                TEST_MAX,
                &mut scan_from,
                &mut assume_mid_discard,
                |_buf| Ok(None),
            );

            assert!(
                matches!(result, Ok(None)),
                "drive must not loop forever when decode_step makes no progress at all"
            );
        }
    }

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
            id: ServerId::new("test").unwrap(),
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
            name: ToolName::new("test_tool").unwrap(),
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
        let server_id = ServerId::new("nonexistent").unwrap();
        assert!(introspector.get_server(&server_id).is_none());
    }

    #[test]
    fn test_clear() {
        let mut introspector = Introspector::new();

        // Add some fake server data
        let info = ServerInfo {
            id: ServerId::new("test").unwrap(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        introspector
            .servers
            .insert(ServerId::new("test").unwrap(), info);
        assert_eq!(introspector.server_count(), 1);

        introspector.clear();
        assert_eq!(introspector.server_count(), 0);
    }

    #[test]
    fn test_remove_server() {
        let mut introspector = Introspector::new();
        let server_id = ServerId::new("test").unwrap();

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
            id: ServerId::new("server1").unwrap(),
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
            id: ServerId::new("server2").unwrap(),
            name: "Server 2".to_string(),
            version: "2.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: false,
                supports_resources: true,
                supports_prompts: false,
            },
        };

        introspector
            .servers
            .insert(ServerId::new("server1").unwrap(), info1);
        introspector
            .servers
            .insert(ServerId::new("server2").unwrap(), info2);

        let servers = introspector.list_servers();
        assert_eq!(servers.len(), 2);
    }

    #[test]
    fn test_serialization() {
        let tool = ToolInfo {
            name: ToolName::new("test_tool").unwrap(),
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
    ) -> rmcp::model::ServerPeerInfo {
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
        serde_json::from_value(raw).expect("valid ServerPeerInfo JSON")
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

    /// Like [`make_raw_tool`], but carries the padded schema as `outputSchema` (with a fixed,
    /// minimal `inputSchema`) so output-schema bound tests can be built independently of the
    /// input-schema ones.
    fn make_raw_tool_with_output_schema(
        name: &str,
        description: &str,
        padding_len: usize,
    ) -> rmcp::model::Tool {
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
            "inputSchema": {"type": "object"},
            "outputSchema": schema_obj,
        }))
        .expect("valid rmcp::model::Tool JSON")
    }

    /// Serialized byte size of the `output_schema` produced by
    /// [`make_raw_tool_with_output_schema`] for a given `padding_len`, used to compute
    /// exact-boundary test inputs.
    fn output_schema_size_for_padding(padding_len: usize) -> usize {
        let tool = make_raw_tool_with_output_schema("tool", "d", padding_len);
        let schema = tool.output_schema.expect("output schema set");
        serde_json::to_vec(&serde_json::Value::Object((*schema).clone()))
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
            &ServerId::new("test").unwrap(),
            PeerMeta {
                server_name: "Test".to_string(),
                server_version: "1.0.0".to_string(),
                has_resources: false,
                has_prompts: false,
            },
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
            &ServerId::new("test").unwrap(),
            PeerMeta {
                server_name: "Test".to_string(),
                server_version: "1.0.0".to_string(),
                has_resources: false,
                has_prompts: false,
            },
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

    /// #287 — a tool name that isn't a valid `ToolName` (here, containing a path separator)
    /// hard-fails the whole `build_tool_info` call via `Error::ValidationError`, consistent
    /// with this function's existing hard-fail-on-first-violation handling of oversized
    /// names/descriptions/schemas (see `test_build_tool_info_rejects_oversized_name` above) —
    /// this is a deliberate decision, not a gap: a single malformed tool name is not silently
    /// skipped while the rest of the server's tools are returned.
    #[test]
    fn test_build_tool_info_rejects_tool_name_with_path_separator() {
        let tool = make_raw_tool("evil/tool", "d", 0);

        let result = build_tool_info(tool);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }

    /// Regression guard for a bypass found in review: `ServerInfo`/`ToolInfo` both derive
    /// `Deserialize` and hold a `ServerId`/`ToolName` field directly, so before
    /// `mcp_execution_core::{ServerId, ToolName}` routed `Deserialize` through their own
    /// `new()` invariant (via `#[serde(try_from = "String")]`), a hostile `id`/tool `name`
    /// (e.g. containing `..` or a path separator) survived deserialization unvalidated here —
    /// even though `ServerId::new`/`ToolName::new` themselves had already been made fallible.
    #[test]
    fn test_server_info_deserialize_rejects_invalid_server_id() {
        let json = serde_json::json!({
            "id": "../escape",
            "name": "Evil Server",
            "version": "1.0.0",
            "tools": [],
            "capabilities": { "supports_tools": true, "supports_resources": false, "supports_prompts": false },
        });
        let result: std::result::Result<ServerInfo, _> = serde_json::from_value(json);
        assert!(result.is_err());
    }

    #[test]
    fn test_tool_info_deserialize_rejects_invalid_tool_name() {
        let json = serde_json::json!({
            "name": "evil/tool",
            "description": "d",
            "input_schema": {},
            "output_schema": null,
        });
        let result: std::result::Result<ToolInfo, _> = serde_json::from_value(json);
        assert!(result.is_err());
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

    #[test]
    fn test_build_tool_info_rejects_output_schema_one_byte_over_max_size() {
        let overhead = output_schema_size_for_padding(0);
        let padding_len = MAX_SCHEMA_SIZE_BYTES - overhead + 1;
        let tool = make_raw_tool_with_output_schema("tool", "d", padding_len);

        let result = build_tool_info(tool);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_resource_limit_exceeded());
    }

    #[test]
    fn test_build_tool_info_accepts_output_schema_at_exact_max_size() {
        let overhead = output_schema_size_for_padding(0);
        let padding_len = MAX_SCHEMA_SIZE_BYTES - overhead;
        assert_eq!(
            output_schema_size_for_padding(padding_len),
            MAX_SCHEMA_SIZE_BYTES,
            "test setup should hit the cap exactly"
        );
        let tool = make_raw_tool_with_output_schema("tool", "d", padding_len);

        let result = build_tool_info(tool);
        assert!(result.is_ok());
    }

    // ── output_schema propagation (issue #254) ───────────────────────────────

    #[test]
    fn test_build_tool_info_propagates_output_schema() {
        let tool: rmcp::model::Tool = serde_json::from_value(serde_json::json!({
            "name": "tool",
            "description": "d",
            "inputSchema": {"type": "object"},
            "outputSchema": {"type": "string"},
        }))
        .expect("valid rmcp::model::Tool JSON");

        let result = build_tool_info(tool).expect("within resource limits");
        assert_eq!(
            result.output_schema,
            Some(serde_json::json!({"type": "string"}))
        );
    }

    #[test]
    fn test_build_tool_info_leaves_output_schema_none_when_absent() {
        let tool = make_raw_tool("tool", "d", 0);

        let result = build_tool_info(tool).expect("within resource limits");
        assert!(result.output_schema.is_none());
    }

    #[test]
    fn test_build_tool_info_propagates_empty_object_output_schema() {
        let tool: rmcp::model::Tool = serde_json::from_value(serde_json::json!({
            "name": "tool",
            "description": "d",
            "inputSchema": {"type": "object"},
            "outputSchema": {},
        }))
        .expect("valid rmcp::model::Tool JSON");

        let result = build_tool_info(tool).expect("within resource limits");
        assert_eq!(result.output_schema, Some(serde_json::json!({})));
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

    // ── ADR-369 §5 revisit gate (finding A) ──────────────────────────────────

    /// Fails the moment `rmcp` promotes `ProtocolVersion::LATEST` to
    /// `V_2026_07_28`, per ADR-369 §5 (`specs/decisions/ADR-369-rmcp-stateless-lifecycle-adoption.md`).
    ///
    /// A red assertion here is a trigger to **re-open** the deferred decision
    /// on adopting rmcp's SEP-2575 stateless discover lifecycle (finding A) —
    /// it does not authorize implementing it, and the benefit side of that
    /// decision (whether servers in the actual population answer
    /// `server/discover`) must still be re-assessed at that point, not
    /// assumed. Do not "fix" this assertion by updating the expected version;
    /// update it only after the ADR-369 discussion has been re-opened and a
    /// new decision recorded.
    #[test]
    fn test_adr_369_protocol_version_latest_gate() {
        assert_eq!(
            rmcp::model::ProtocolVersion::LATEST,
            rmcp::model::ProtocolVersion::V_2025_11_25,
            "rmcp promoted ProtocolVersion::LATEST — this is the ADR-369 §5 revisit gate for \
             finding A (specs/decisions/ADR-369-rmcp-stateless-lifecycle-adoption.md): re-open \
             the ADR-369 discussion for finding A, do NOT just bump the expected constant"
        );
    }
}
