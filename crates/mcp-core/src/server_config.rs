//! MCP server configuration with command, arguments, and environment.
//!
//! This module provides type-safe server configuration for launching MCP servers
//! with security validation of commands, arguments, and environment variables.
//!
//! # Transport Types
//!
//! Supports two transport types:
//! - Stdio: Subprocess communication via stdin/stdout (default)
//! - HTTP: Communication via HTTP/HTTPS API
//!
//! # Security
//!
//! The configuration enforces:
//! - Command validation (absolute path or binary name)
//! - Argument sanitization (no shell metacharacters)
//! - Environment variable validation (block dangerous names)
//! - Forbidden characters: `;`, `|`, `&`, `>`, `<`, `` ` ``, `$`, `(`, `)`, `\n`, `\r`
//! - Forbidden env vars: dynamic-linker (`LD_PRELOAD`, `LD_LIBRARY_PATH`,
//!   `LD_AUDIT`, `DYLD_*`), `PATH`, and interpreter hijack vectors
//!   (`NODE_OPTIONS`, `PYTHONPATH`, `PYTHONSTARTUP`, `RUBYOPT`, `PERL5OPT`,
//!   `JAVA_TOOL_OPTIONS`) — see `command::FORBIDDEN_ENV_NAMES` for the full
//!   list and its documented threat model
//!
//! # Examples
//!
//! ```
//! use mcp_execution_core::ServerConfig;
//! use std::collections::HashMap;
//!
//! // Simple configuration with just command
//! let config = ServerConfig::builder()
//!     .command("docker".to_string())
//!     .build().unwrap();
//!
//! // Full configuration with args and env
//! let config = ServerConfig::builder()
//!     .command("mcp-server".to_string())
//!     .arg("--port".to_string())
//!     .arg("8080".to_string())
//!     .env("LOG_LEVEL".to_string(), "debug".to_string())
//!     .build().unwrap();
//!
//! // HTTP transport configuration
//! let config = ServerConfig::builder()
//!     .http_transport("https://api.example.com/mcp".to_string())
//!     .header("Authorization".to_string(), "Bearer token".to_string())
//!     .build().unwrap();
//! ```

use crate::path::sanitize_path_for_error;
use crate::redact::{RedactedItems, RedactedMapValues, RedactedUrl};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Default timeout for establishing an MCP server connection (handshake).
const DEFAULT_CONNECT_TIMEOUT: Duration = Duration::from_secs(30);

/// Default timeout for the `list_all_tools` discovery call after connecting.
const DEFAULT_DISCOVER_TIMEOUT: Duration = Duration::from_secs(30);

const fn default_connect_timeout() -> Duration {
    DEFAULT_CONNECT_TIMEOUT
}

const fn default_discover_timeout() -> Duration {
    DEFAULT_DISCOVER_TIMEOUT
}

/// Transport type for MCP server communication.
///
/// Defines how the client communicates with the MCP server.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::TransportType;
///
/// // Default is stdio
/// let transport = TransportType::default();
/// assert_eq!(transport, TransportType::Stdio);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum TransportType {
    /// Stdio transport: subprocess communication via stdin/stdout.
    #[default]
    Stdio,
    /// HTTP transport: communication via HTTP/HTTPS API.
    Http,
    /// SSE transport: Server-Sent Events for streaming communication.
    Sse,
}

/// MCP server configuration with command, arguments, and environment.
///
/// Represents the configuration needed to communicate with an MCP server,
/// supporting both stdio (subprocess) and HTTP transports.
///
/// # Transport Types
///
/// - **Stdio**: Launches a subprocess and communicates via stdin/stdout
/// - **HTTP**: Connects to an HTTP/HTTPS API endpoint
///
/// # Security
///
/// [`ServerConfigBuilder::build`] runs
/// [`validate_server_config`] internally, so a `ServerConfig` built through the builder
/// cannot be constructed without having already passed security validation. This is *not*
/// a type-level guarantee, though: every field is `pub` and the type derives `Deserialize`,
/// so a caller can still assemble an unvalidated `ServerConfig` directly (a struct literal,
/// or deserializing a hand-edited `mcp.json`) — see the `deserialize_http_config_missing_url`
/// pattern in `command.rs`'s tests for exactly that construction. Callers that obtain a
/// `ServerConfig` from anywhere other than the builder should still call
/// [`validate_server_config`] themselves before using it to spawn a process or open a
/// connection; `mcp_execution_introspector::Introspector::discover_server` does this as
/// defense-in-depth even though its own callers always go through the builder.
///
/// `headers`, `env`, `args`, and `url` routinely carry secrets (e.g. an
/// `Authorization: Bearer <token>` header, a `GITHUB_PERSONAL_ACCESS_TOKEN`
/// environment variable, an `--api-key sk-...`-style argument, or a
/// `?token=`-style query string), so this type's [`Debug`] implementation is
/// hand-written to redact them:
///
/// - `headers`/`env`: keys stay visible, values are replaced — a legitimately
///   configured key (a chosen header or env var name, e.g. `"Authorization"`)
///   is not itself a secret and remains useful for debugging. This mirrors
///   the discipline already applied to header values in `command.rs`'s
///   `validate_header_value_string`, which never echoes a header value into
///   an error message.
/// - `args`: every entry is replaced wholesale (via [`crate::RedactedItems`])
///   since an argument has no key/value split to preserve half of.
/// - `url`: userinfo credentials and any query string are stripped (via
///   [`crate::RedactedUrl`]); scheme, host, and path stay readable.
/// - `command`/`cwd`: passed through [`crate::sanitize_path_for_error`] —
///   not a secret, but an absolute path leaks the OS username, and the
///   program name itself (`docker`, `npx`) is worth keeping readable for
///   telling server entries apart in a log.
///
/// This is a narrower guarantee than `command.rs`'s header-*name* validation
/// errors, which redact the name too: those fire on input that has not yet
/// been confirmed well-formed (e.g. a `Name=Value` CLI argument mis-split on
/// the wrong separator can leave a secret value sitting in the name
/// position), so any name reaching that error path must be treated as
/// secret-shaped. Once [`validate_server_config`] has accepted a config,
/// its `headers`/`env` keys are the caller's own identifiers rather than
/// unvalidated split output, so trusting them for `Debug` output is not in
/// tension with distrusting an unvalidated name in an error message.
///
/// [`ServerConfigBuilder`] carries the same [`Debug`] treatment for
/// consistency, but that guarantee does not extend to it: the builder is
/// populated *before* [`validate_server_config`] runs, so a caller that
/// feeds it unvalidated input (e.g. a mis-split CLI argument) can still end
/// up with a secret-shaped key in `format!("{builder:?}")`. Keys are shown
/// there deliberately regardless — redacting them would defeat the point of
/// a debug impl for a type whose purpose is to be inspected before
/// `build()`.
///
/// `Serialize`/`Deserialize` are deliberately left unredacted: config
/// persistence and the wire format must still round-trip real values.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::ServerConfig;
///
/// // Stdio transport
/// let config = ServerConfig::builder()
///     .command("docker".to_string())
///     .arg("run".to_string())
///     .arg("mcp-server".to_string())
///     .build().unwrap();
///
/// assert_eq!(config.command, "docker");
/// assert_eq!(config.args.len(), 2);
///
/// // HTTP transport
/// let config = ServerConfig::builder()
///     .http_transport("https://api.example.com/mcp".to_string())
///     .header("Authorization".to_string(), "Bearer token".to_string())
///     .build().unwrap();
/// ```
///
/// Debug output redacts header/env values but keeps keys:
///
/// ```
/// use mcp_execution_core::ServerConfig;
///
/// let config = ServerConfig::builder()
///     .http_transport("https://api.example.com/mcp".to_string())
///     .header("Authorization".to_string(), "Bearer sk-secret-value".to_string())
///     .build();
///
/// let debug_output = format!("{config:?}");
/// assert!(debug_output.contains("Authorization"));
/// assert!(!debug_output.contains("sk-secret-value"));
/// ```
///
/// Debug output redacts `args` wholesale and strips URL userinfo/query,
/// while keeping `command` and the URL host/path readable:
///
/// ```
/// use mcp_execution_core::ServerConfig;
///
/// let config = ServerConfig::builder()
///     .command("docker".to_string())
///     .arg("--api-key".to_string())
///     .arg("sk-secret-arg".to_string())
///     .build();
///
/// let debug_output = format!("{config:?}");
/// assert!(debug_output.contains("docker"));
/// assert!(!debug_output.contains("sk-secret-arg"));
///
/// let config = ServerConfig::builder()
///     .http_transport("https://user:sk-secret@api.example.com/mcp?token=sk-secret".to_string())
///     .build();
///
/// let debug_output = format!("{config:?}");
/// assert!(debug_output.contains("api.example.com/mcp"));
/// assert!(!debug_output.contains("sk-secret"));
/// ```
///
/// [`validate_server_config`]: fn.validate_server_config.html
#[derive(Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ServerConfig {
    /// Transport type (stdio or http).
    ///
    /// Determines how the client communicates with the MCP server.
    #[serde(default)]
    pub transport: TransportType,

    /// Command to execute (binary name or absolute path).
    ///
    /// **Only used for stdio transport.**
    ///
    /// Can be either:
    /// - Binary name (e.g., "docker", "python") - resolved via PATH
    /// - Absolute path (e.g., "/usr/local/bin/mcp-server")
    #[serde(default)]
    pub command: String,

    /// Arguments to pass to command.
    ///
    /// **Only used for stdio transport.**
    ///
    /// Each argument is passed separately to avoid shell interpretation.
    /// Do not include the command itself in arguments.
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables to set for the subprocess.
    ///
    /// **Only used for stdio transport.**
    ///
    /// These are added to (or override) the parent process environment.
    /// Security validation blocks dangerous variables like `LD_PRELOAD`.
    /// Values routinely hold secrets (e.g. `GITHUB_PERSONAL_ACCESS_TOKEN`); see
    /// the redaction note on [`ServerConfig`]'s own doc comment.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Working directory for the subprocess (optional).
    ///
    /// **Only used for stdio transport.**
    ///
    /// If None, inherits the parent process working directory.
    #[serde(default)]
    pub cwd: Option<PathBuf>,

    /// URL for HTTP transport.
    ///
    /// **Only used for HTTP transport.**
    ///
    /// Example: `https://api.example.com/mcp`
    ///
    /// This crate does not apply SSRF allowlisting to this URL — it is
    /// treated like a `curl` target, appropriate for a local CLI tool.
    /// Embedders that expose this config in a multi-tenant or server context
    /// should apply their own URL allowlisting before connecting.
    #[serde(default)]
    pub url: Option<String>,

    /// HTTP headers for HTTP transport.
    ///
    /// **Only used for HTTP transport.**
    ///
    /// Common headers include:
    /// - `Authorization`: Authentication token
    /// - `Content-Type`: Request content type
    ///
    /// Values routinely hold secrets (e.g. a bearer token); see the redaction
    /// note on [`ServerConfig`]'s own doc comment.
    #[serde(default)]
    pub headers: HashMap<String, String>,

    /// Timeout for establishing a connection to the server (handshake).
    ///
    /// Bounds how long `Introspector::discover_server` waits for the initial
    /// rmcp `serve` handshake before giving up. Defaults to 30 seconds.
    #[serde(default = "default_connect_timeout")]
    pub connect_timeout: Duration,

    /// Timeout for the tool discovery call after a connection is established.
    ///
    /// Bounds how long `Introspector::discover_server` waits for
    /// `list_all_tools` to respond. Defaults to 30 seconds.
    #[serde(default = "default_discover_timeout")]
    pub discover_timeout: Duration,
}

impl fmt::Debug for ServerConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfig")
            .field("transport", &self.transport)
            .field(
                "command",
                &sanitize_path_for_error(Path::new(&self.command)),
            )
            .field("args", &RedactedItems(&self.args))
            .field("env", &RedactedMapValues(&self.env))
            .field("cwd", &self.cwd.as_deref().map(sanitize_path_for_error))
            .field("url", &self.url.as_deref().map(RedactedUrl))
            .field("headers", &RedactedMapValues(&self.headers))
            .field("connect_timeout", &self.connect_timeout)
            .field("discover_timeout", &self.discover_timeout)
            .finish()
    }
}

impl ServerConfig {
    /// Creates a new builder for `ServerConfig`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn builder() -> ServerConfigBuilder {
        ServerConfigBuilder::default()
    }

    /// Returns the transport type.
    #[must_use]
    pub const fn transport(&self) -> &TransportType {
        &self.transport
    }

    /// Returns the command as a string slice.
    #[must_use]
    pub fn command(&self) -> &str {
        &self.command
    }

    /// Returns a slice of arguments.
    #[must_use]
    pub fn args(&self) -> &[String] {
        &self.args
    }

    /// Returns a reference to the environment variables map.
    #[must_use]
    pub const fn env(&self) -> &HashMap<String, String> {
        &self.env
    }

    /// Returns the working directory, if set.
    #[must_use]
    pub const fn cwd(&self) -> Option<&PathBuf> {
        self.cwd.as_ref()
    }

    /// Returns the URL for HTTP transport, if set.
    #[must_use]
    pub fn url(&self) -> Option<&str> {
        self.url.as_deref()
    }

    /// Returns a reference to the HTTP headers map.
    #[must_use]
    pub const fn headers(&self) -> &HashMap<String, String> {
        &self.headers
    }

    /// Returns the connection (handshake) timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    /// use std::time::Duration;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .build().unwrap();
    ///
    /// assert_eq!(config.connect_timeout(), Duration::from_secs(30));
    /// ```
    #[must_use]
    pub const fn connect_timeout(&self) -> Duration {
        self.connect_timeout
    }

    /// Returns the tool discovery timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    /// use std::time::Duration;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .build().unwrap();
    ///
    /// assert_eq!(config.discover_timeout(), Duration::from_secs(30));
    /// ```
    #[must_use]
    pub const fn discover_timeout(&self) -> Duration {
        self.discover_timeout
    }
}

/// Builder for constructing `ServerConfig` instances.
///
/// Provides a fluent API for building server configurations with
/// optional arguments, environment variables, and HTTP settings.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::ServerConfig;
///
/// // Stdio transport
/// let config = ServerConfig::builder()
///     .command("mcp-server".to_string())
///     .arg("--verbose".to_string())
///     .env("DEBUG".to_string(), "1".to_string())
///     .build().unwrap();
///
/// // HTTP transport
/// let config = ServerConfig::builder()
///     .http_transport("https://api.example.com/mcp".to_string())
///     .header("Authorization".to_string(), "Bearer token".to_string())
///     .build().unwrap();
/// ```
///
/// Like [`ServerConfig`] itself, this builder accumulates `env`/`headers`
/// before secrets they may carry are known to be well-formed, so its
/// [`Debug`] impl redacts values the same way — see the redaction note on
/// [`ServerConfig`]'s doc comment.
#[derive(Clone)]
pub struct ServerConfigBuilder {
    transport: TransportType,
    command: Option<String>,
    args: Vec<String>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
    url: Option<String>,
    headers: HashMap<String, String>,
    connect_timeout: Duration,
    discover_timeout: Duration,
}

impl fmt::Debug for ServerConfigBuilder {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerConfigBuilder")
            .field("transport", &self.transport)
            .field(
                "command",
                &self
                    .command
                    .as_deref()
                    .map(|command| sanitize_path_for_error(Path::new(command))),
            )
            .field("args", &RedactedItems(&self.args))
            .field("env", &RedactedMapValues(&self.env))
            .field("cwd", &self.cwd.as_deref().map(sanitize_path_for_error))
            .field("url", &self.url.as_deref().map(RedactedUrl))
            .field("headers", &RedactedMapValues(&self.headers))
            .field("connect_timeout", &self.connect_timeout)
            .field("discover_timeout", &self.discover_timeout)
            .finish()
    }
}

impl Default for ServerConfigBuilder {
    fn default() -> Self {
        Self {
            transport: TransportType::default(),
            command: None,
            args: Vec::new(),
            env: HashMap::new(),
            cwd: None,
            url: None,
            headers: HashMap::new(),
            connect_timeout: DEFAULT_CONNECT_TIMEOUT,
            discover_timeout: DEFAULT_DISCOVER_TIMEOUT,
        }
    }
}

impl ServerConfigBuilder {
    /// Sets the command to execute.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn command(mut self, command: String) -> Self {
        self.command = Some(command);
        self
    }

    /// Adds a single argument.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .arg("run".to_string())
    ///     .arg("--rm".to_string())
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn arg(mut self, arg: String) -> Self {
        self.args.push(arg);
        self
    }

    /// Sets all arguments at once, replacing any previously added.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .args(vec!["run".to_string(), "--rm".to_string()])
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn args(mut self, args: Vec<String>) -> Self {
        self.args = args;
        self
    }

    /// Adds a single environment variable.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("mcp-server".to_string())
    ///     .env("LOG_LEVEL".to_string(), "debug".to_string())
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn env(mut self, key: String, value: String) -> Self {
        self.env.insert(key, value);
        self
    }

    /// Sets all environment variables at once, replacing any previously added.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    /// use std::collections::HashMap;
    ///
    /// let mut env_map = HashMap::new();
    /// env_map.insert("DEBUG".to_string(), "1".to_string());
    ///
    /// let config = ServerConfig::builder()
    ///     .command("mcp-server".to_string())
    ///     .environment(env_map)
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn environment(mut self, env: HashMap<String, String>) -> Self {
        self.env = env;
        self
    }

    /// Sets the working directory for the subprocess.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    /// use std::path::PathBuf;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("mcp-server".to_string())
    ///     .cwd(PathBuf::from("/tmp"))
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn cwd(mut self, cwd: PathBuf) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// Configures HTTP transport with the given URL.
    ///
    /// This sets the transport type to HTTP and configures the endpoint URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .http_transport("https://api.example.com/mcp".to_string())
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn http_transport(mut self, url: String) -> Self {
        self.transport = TransportType::Http;
        self.url = Some(url);
        // Set a dummy command for HTTP transport so build() doesn't panic
        if self.command.is_none() {
            self.command = Some(String::new());
        }
        self
    }

    /// Configures SSE transport with the given URL.
    ///
    /// This sets the transport type to SSE (Server-Sent Events) and configures the endpoint URL.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .sse_transport("https://api.example.com/sse".to_string())
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn sse_transport(mut self, url: String) -> Self {
        self.transport = TransportType::Sse;
        self.url = Some(url);
        // Set a dummy command for SSE transport so build() doesn't panic
        if self.command.is_none() {
            self.command = Some(String::new());
        }
        self
    }

    /// Sets the URL for HTTP transport.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .http_transport("https://api.example.com/mcp".to_string())
    ///     .url("https://api.example.com/mcp/v2".to_string())
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn url(mut self, url: String) -> Self {
        self.url = Some(url);
        self
    }

    /// Adds a single HTTP header.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .http_transport("https://api.example.com/mcp".to_string())
    ///     .header("Authorization".to_string(), "Bearer token".to_string())
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn header(mut self, key: String, value: String) -> Self {
        self.headers.insert(key, value);
        self
    }

    /// Sets all HTTP headers at once, replacing any previously added.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    /// use std::collections::HashMap;
    ///
    /// let mut headers = HashMap::new();
    /// headers.insert("Authorization".to_string(), "Bearer token".to_string());
    ///
    /// let config = ServerConfig::builder()
    ///     .http_transport("https://api.example.com/mcp".to_string())
    ///     .headers(headers)
    ///     .build().unwrap();
    /// ```
    #[must_use]
    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Sets the connection (handshake) timeout, overriding the 30-second default.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    /// use std::time::Duration;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .connect_timeout(Duration::from_secs(5))
    ///     .build().unwrap();
    ///
    /// assert_eq!(config.connect_timeout(), Duration::from_secs(5));
    /// ```
    #[must_use]
    pub const fn connect_timeout(mut self, timeout: Duration) -> Self {
        self.connect_timeout = timeout;
        self
    }

    /// Sets the tool discovery timeout, overriding the 30-second default.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    /// use std::time::Duration;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .discover_timeout(Duration::from_secs(5))
    ///     .build().unwrap();
    ///
    /// assert_eq!(config.discover_timeout(), Duration::from_secs(5));
    /// ```
    #[must_use]
    pub const fn discover_timeout(mut self, timeout: Duration) -> Self {
        self.discover_timeout = timeout;
        self
    }

    /// Builds and validates the `ServerConfig`.
    ///
    /// This is the only way to obtain a [`ServerConfig`] through the builder: beyond the
    /// structural checks (command/url presence), it also runs the full security validation
    /// performed by [`validate_server_config`] — shell metacharacters, forbidden environment
    /// variables, URL scheme, and header safety — before returning. A `ServerConfig` built
    /// through this method therefore cannot exist without having passed security
    /// validation, closing the gap where a caller could construct a config via the builder
    /// and forget to validate it before spawning a process. This is a builder-level
    /// guarantee, not a type-level one: every field is `pub` and the type derives
    /// `Deserialize`, so a `ServerConfig` obtained by other means (a struct literal,
    /// `serde_json::from_str`) is not covered — see [`ServerConfig`]'s own "Security" section.
    ///
    /// # Errors
    ///
    /// Returns `Error::ValidationError` if:
    /// - Command is not set (or is empty) for stdio transport
    /// - URL is not set for HTTP/SSE transport
    ///
    /// Returns `Error::SecurityViolation` or `Error::ValidationError` for any
    /// reason documented on [`validate_server_config`] (forbidden shell
    /// metacharacters, forbidden environment variables, invalid URL scheme,
    /// unsafe headers, out-of-bounds timeouts, etc.).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .build()
    ///     .unwrap();
    ///
    /// // Rejected at construction time — no separate validation step needed.
    /// let err = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .arg("run; rm -rf /".to_string())
    ///     .build()
    ///     .unwrap_err();
    /// assert!(err.is_security_error());
    /// ```
    ///
    /// [`validate_server_config`]: fn.validate_server_config.html
    pub fn build(self) -> crate::Result<ServerConfig> {
        let config = self.build_structural()?;
        crate::validate_server_config(&config)?;
        Ok(config)
    }

    /// Checks structural completeness (command/url presence) and assembles
    /// the `ServerConfig`, without running security validation.
    ///
    /// Kept private and separate from [`Self::build`] so the two concerns —
    /// "is this config structurally complete" and "is this config safe to
    /// spawn" — stay independently testable, while the public API only ever
    /// hands out a fully validated [`ServerConfig`].
    fn build_structural(self) -> crate::Result<ServerConfig> {
        match self.transport {
            TransportType::Stdio => {
                let command = self.command.ok_or_else(|| crate::Error::ValidationError {
                    field: "command".to_string(),
                    reason: "command is required for stdio transport".to_string(),
                })?;

                if command.trim().is_empty() {
                    return Err(crate::Error::ValidationError {
                        field: "command".to_string(),
                        reason: "command cannot be empty for stdio transport".to_string(),
                    });
                }

                Ok(ServerConfig {
                    transport: TransportType::Stdio,
                    command,
                    args: self.args,
                    env: self.env,
                    cwd: self.cwd,
                    url: None,
                    headers: HashMap::new(),
                    connect_timeout: self.connect_timeout,
                    discover_timeout: self.discover_timeout,
                })
            }
            TransportType::Http => {
                let url = self.url.ok_or_else(|| crate::Error::ValidationError {
                    field: "url".to_string(),
                    reason: "url is required for HTTP transport".to_string(),
                })?;

                Ok(ServerConfig {
                    transport: TransportType::Http,
                    command: String::new(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: Some(url),
                    headers: self.headers,
                    connect_timeout: self.connect_timeout,
                    discover_timeout: self.discover_timeout,
                })
            }
            TransportType::Sse => {
                let url = self.url.ok_or_else(|| crate::Error::ValidationError {
                    field: "url".to_string(),
                    reason: "url is required for SSE transport".to_string(),
                })?;

                Ok(ServerConfig {
                    transport: TransportType::Sse,
                    command: String::new(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: Some(url),
                    headers: self.headers,
                    connect_timeout: self.connect_timeout,
                    discover_timeout: self.discover_timeout,
                })
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_config_builder_minimal() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build()
            .unwrap();

        assert_eq!(config.command, "docker");
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
        assert!(config.cwd.is_none());
    }

    #[test]
    fn test_server_config_builder_with_args() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run".to_string())
            .arg("--rm".to_string())
            .arg("mcp-server".to_string())
            .build()
            .unwrap();

        assert_eq!(config.command, "docker");
        assert_eq!(config.args, vec!["run", "--rm", "mcp-server"]);
    }

    #[test]
    fn test_server_config_builder_with_args_vec() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .args(vec!["run".to_string(), "--rm".to_string()])
            .build()
            .unwrap();

        assert_eq!(config.args, vec!["run", "--rm"]);
    }

    #[test]
    fn test_server_config_builder_with_env() {
        let config = ServerConfig::builder()
            .command("mcp-server".to_string())
            .env("LOG_LEVEL".to_string(), "debug".to_string())
            .env("DEBUG".to_string(), "1".to_string())
            .build()
            .unwrap();

        assert_eq!(config.env.len(), 2);
        assert_eq!(config.env.get("LOG_LEVEL"), Some(&"debug".to_string()));
        assert_eq!(config.env.get("DEBUG"), Some(&"1".to_string()));
    }

    #[test]
    fn test_server_config_builder_with_environment_map() {
        let mut env_map = HashMap::new();
        env_map.insert("VAR1".to_string(), "value1".to_string());
        env_map.insert("VAR2".to_string(), "value2".to_string());

        let config = ServerConfig::builder()
            .command("mcp-server".to_string())
            .environment(env_map)
            .build()
            .unwrap();

        assert_eq!(config.env.len(), 2);
    }

    #[test]
    fn test_server_config_builder_with_cwd() {
        let config = ServerConfig::builder()
            .command("mcp-server".to_string())
            .cwd(PathBuf::from("/tmp"))
            .build()
            .unwrap();

        assert_eq!(config.cwd, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn test_server_config_builder_full() {
        let mut env_map = HashMap::new();
        env_map.insert("LOG_LEVEL".to_string(), "debug".to_string());

        let config = ServerConfig::builder()
            .command("mcp-server".to_string())
            .args(vec!["--port".to_string(), "8080".to_string()])
            .environment(env_map)
            .cwd(PathBuf::from("/var/run"))
            .build()
            .unwrap();

        assert_eq!(config.command, "mcp-server");
        assert_eq!(config.args.len(), 2);
        assert_eq!(config.env.len(), 1);
        assert_eq!(config.cwd, Some(PathBuf::from("/var/run")));
    }

    #[test]
    #[should_panic(expected = "command")]
    fn test_server_config_builder_missing_command() {
        let _ = ServerConfig::builder().build().unwrap();
    }

    #[test]
    fn test_server_config_builder_build_missing_command() {
        let result = ServerConfig::builder().build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("command"));
    }

    /// #177 — security validation must be folded into construction: a
    /// `ServerConfig` carrying a shell metacharacter can no longer be built
    /// at all, rather than only being caught by a separate manual call to
    /// `validate_server_config` downstream.
    #[test]
    fn test_build_rejects_shell_metacharacters_at_construction() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run; rm -rf /".to_string())
            .build();

        assert!(result.is_err());
        assert!(result.unwrap_err().is_security_error());
    }

    /// #177 — same guarantee for forbidden environment variables.
    #[test]
    fn test_build_rejects_forbidden_env_var_at_construction() {
        let result = ServerConfig::builder()
            .command("docker".to_string())
            .env("LD_PRELOAD".to_string(), "/evil.so".to_string())
            .build();

        assert!(result.is_err());
        assert!(result.unwrap_err().is_security_error());
    }

    #[test]
    fn test_server_config_accessors() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run".to_string())
            .env("VAR".to_string(), "value".to_string())
            .cwd(PathBuf::from("/tmp"))
            .build()
            .unwrap();

        assert_eq!(config.command(), "docker");
        assert_eq!(config.args(), &["run".to_string()]);
        assert_eq!(config.env().len(), 1);
        assert_eq!(config.cwd(), Some(&PathBuf::from("/tmp")));
    }

    #[test]
    fn test_server_config_serialize_deserialize() {
        let config = ServerConfig::builder()
            .command("mcp-server".to_string())
            .arg("--verbose".to_string())
            .env("DEBUG".to_string(), "1".to_string())
            .build()
            .unwrap();

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ServerConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_server_config_clone() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build()
            .unwrap();

        let cloned = config.clone();
        assert_eq!(config, cloned);
    }

    #[test]
    fn test_server_config_debug() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build()
            .unwrap();

        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("docker"));
    }

    #[test]
    fn test_server_config_debug_redacts_header_values() {
        // headers are only populated for HTTP/SSE transport (see the
        // builder's `build`), so exercise them via `http_transport`.
        let config = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header(
                "Authorization".to_string(),
                "Bearer sk-secret-header-value".to_string(),
            )
            .build()
            .unwrap();

        let debug_str = format!("{config:?}");

        // The key is useful for debugging and is not secret.
        assert!(debug_str.contains("Authorization"));
        // The value must never appear.
        assert!(!debug_str.contains("sk-secret-header-value"));
    }

    #[test]
    fn test_server_config_debug_redacts_env_values() {
        // env is only populated for stdio transport (see the builder's
        // `build`), so exercise it via the default stdio transport.
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .env(
                "GITHUB_PERSONAL_ACCESS_TOKEN".to_string(),
                "ghp_supersecretvalue".to_string(),
            )
            .build()
            .unwrap();

        let debug_str = format!("{config:?}");

        // The key is useful for debugging and is not secret.
        assert!(debug_str.contains("GITHUB_PERSONAL_ACCESS_TOKEN"));
        // The value must never appear.
        assert!(!debug_str.contains("ghp_supersecretvalue"));
    }

    #[test]
    fn test_server_config_debug_redacts_args() {
        let secret = "sk-live-secret-arg-value";
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .arg("--api-key".to_string())
            .arg(secret.to_string())
            .build()
            .unwrap();

        let debug_str = format!("{config:?}");
        assert!(!debug_str.contains(secret));
    }

    #[test]
    fn test_server_config_debug_redacts_url_userinfo_and_query() {
        let secret = "hunter2";
        let config = ServerConfig::builder()
            .http_transport(format!(
                "https://user:{secret}@api.example.com/mcp?token={secret}"
            ))
            .build()
            .unwrap();

        let debug_str = format!("{config:?}");
        assert!(!debug_str.contains(secret));
        // Host/path stay readable.
        assert!(debug_str.contains("api.example.com/mcp"));
    }

    #[test]
    fn test_server_config_builder_debug_redacts_args() {
        let secret = "sk-live-secret-arg-value";
        let builder = ServerConfig::builder()
            .command("docker".to_string())
            .arg("--api-key".to_string())
            .arg(secret.to_string());

        let debug_str = format!("{builder:?}");
        assert!(!debug_str.contains(secret));
    }

    #[test]
    fn test_server_config_builder_debug_redacts_url_userinfo_and_query() {
        let secret = "hunter2";
        let builder = ServerConfig::builder().url(format!(
            "https://user:{secret}@api.example.com/mcp?token={secret}"
        ));

        let debug_str = format!("{builder:?}");
        assert!(!debug_str.contains(secret));
        assert!(debug_str.contains("api.example.com/mcp"));
    }

    #[test]
    fn test_server_config_builder_debug_redacts_env_and_header_values() {
        // Unlike `ServerConfig::build()`, the builder itself doesn't drop
        // env/headers based on transport, so both can be populated at once
        // and inspected via `{:?}` before `build()` is ever called.
        let builder = ServerConfig::builder()
            .command("docker".to_string())
            .env(
                "GITHUB_PERSONAL_ACCESS_TOKEN".to_string(),
                "ghp_supersecretvalue".to_string(),
            )
            .header(
                "Authorization".to_string(),
                "Bearer sk-secret-header-value".to_string(),
            );

        let debug_str = format!("{builder:?}");

        // Keys are useful for debugging and are not secret.
        assert!(debug_str.contains("GITHUB_PERSONAL_ACCESS_TOKEN"));
        assert!(debug_str.contains("Authorization"));
        // Values must never appear.
        assert!(!debug_str.contains("ghp_supersecretvalue"));
        assert!(!debug_str.contains("sk-secret-header-value"));
    }

    #[test]
    fn test_server_config_serialize_still_contains_real_secret_values() {
        // Serialize/Deserialize must round-trip real values for config
        // persistence; only Debug formatting is redacted. Headers and env
        // are exercised separately since `build()` drops whichever one
        // doesn't match the config's transport.
        let http_config = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header(
                "Authorization".to_string(),
                "Bearer sk-secret-header-value".to_string(),
            )
            .build()
            .unwrap();

        let http_json = serde_json::to_string(&http_config).unwrap();
        assert!(http_json.contains("sk-secret-header-value"));

        let http_deserialized: ServerConfig = serde_json::from_str(&http_json).unwrap();
        assert_eq!(http_config, http_deserialized);

        let stdio_config = ServerConfig::builder()
            .command("docker".to_string())
            .env(
                "GITHUB_PERSONAL_ACCESS_TOKEN".to_string(),
                "ghp_supersecretvalue".to_string(),
            )
            .build()
            .unwrap();

        let stdio_json = serde_json::to_string(&stdio_config).unwrap();
        assert!(stdio_json.contains("ghp_supersecretvalue"));

        let stdio_deserialized: ServerConfig = serde_json::from_str(&stdio_json).unwrap();
        assert_eq!(stdio_config, stdio_deserialized);
    }

    #[test]
    fn test_transport_type_default() {
        let transport = TransportType::default();
        assert_eq!(transport, TransportType::Stdio);
    }

    #[test]
    fn test_server_config_http_transport() {
        let config = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .build()
            .unwrap();

        assert_eq!(config.transport, TransportType::Http);
        assert_eq!(config.url(), Some("https://api.example.com/mcp"));
        assert!(config.headers.is_empty());
        assert!(config.command.is_empty());
    }

    #[test]
    fn test_server_config_http_with_headers() {
        let config = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Authorization".to_string(), "Bearer token".to_string())
            .header("Content-Type".to_string(), "application/json".to_string())
            .build()
            .unwrap();

        assert_eq!(config.transport, TransportType::Http);
        assert_eq!(config.headers.len(), 2);
        assert_eq!(
            config.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
        assert_eq!(
            config.headers.get("Content-Type"),
            Some(&"application/json".to_string())
        );
    }

    #[test]
    fn test_server_config_http_with_headers_map() {
        let mut headers = HashMap::new();
        headers.insert("Authorization".to_string(), "Bearer token".to_string());

        let config = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .headers(headers)
            .build()
            .unwrap();

        assert_eq!(config.headers.len(), 1);
    }

    #[test]
    fn test_server_config_http_build_missing_url() {
        let result = ServerConfig::builder().build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("required"));
    }

    #[test]
    fn test_server_config_http_accessors() {
        let config = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Auth".to_string(), "token".to_string())
            .build()
            .unwrap();

        assert_eq!(config.transport(), &TransportType::Http);
        assert_eq!(config.url(), Some("https://api.example.com/mcp"));
        assert_eq!(config.headers().len(), 1);
    }

    #[test]
    fn test_server_config_stdio_default_transport() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build()
            .unwrap();

        assert_eq!(config.transport, TransportType::Stdio);
    }

    #[test]
    fn test_server_config_sse_transport() {
        let config = ServerConfig::builder()
            .sse_transport("https://api.example.com/sse".to_string())
            .build()
            .unwrap();

        assert_eq!(config.transport, TransportType::Sse);
        assert_eq!(config.url(), Some("https://api.example.com/sse"));
        assert!(config.headers.is_empty());
        assert!(config.command.is_empty());
    }

    #[test]
    fn test_server_config_sse_with_headers() {
        let config = ServerConfig::builder()
            .sse_transport("https://api.example.com/sse".to_string())
            .header("Authorization".to_string(), "Bearer token".to_string())
            .header("X-Custom".to_string(), "value".to_string())
            .build()
            .unwrap();

        assert_eq!(config.transport, TransportType::Sse);
        assert_eq!(config.headers.len(), 2);
        assert_eq!(
            config.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
        assert_eq!(config.headers.get("X-Custom"), Some(&"value".to_string()));
    }

    #[test]
    fn test_server_config_sse_build_missing_url() {
        let mut builder = ServerConfig::builder();
        builder.transport = TransportType::Sse;

        let result = builder.build();
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("url is required"));
    }

    #[test]
    fn test_transport_type_serialization() {
        let stdio = TransportType::Stdio;
        let http = TransportType::Http;
        let sse = TransportType::Sse;

        assert_eq!(serde_json::to_string(&stdio).unwrap(), "\"stdio\"");
        assert_eq!(serde_json::to_string(&http).unwrap(), "\"http\"");
        assert_eq!(serde_json::to_string(&sse).unwrap(), "\"sse\"");
    }

    #[test]
    fn test_transport_type_deserialization() {
        let stdio: TransportType = serde_json::from_str("\"stdio\"").unwrap();
        let http: TransportType = serde_json::from_str("\"http\"").unwrap();
        let sse: TransportType = serde_json::from_str("\"sse\"").unwrap();

        assert_eq!(stdio, TransportType::Stdio);
        assert_eq!(http, TransportType::Http);
        assert_eq!(sse, TransportType::Sse);
    }
}
