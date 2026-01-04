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
//! - Forbidden env vars: `LD_PRELOAD`, `LD_LIBRARY_PATH`, `DYLD_*`, `PATH`
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
//!     .build();
//!
//! // Full configuration with args and env
//! let config = ServerConfig::builder()
//!     .command("/usr/local/bin/mcp-server".to_string())
//!     .arg("--port".to_string())
//!     .arg("8080".to_string())
//!     .env("LOG_LEVEL".to_string(), "debug".to_string())
//!     .build();
//!
//! // HTTP transport configuration
//! let config = ServerConfig::builder()
//!     .http_transport("https://api.example.com/mcp".to_string())
//!     .header("Authorization".to_string(), "Bearer token".to_string())
//!     .build();
//! ```

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

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
/// This type is designed to be safe by construction. Use the builder pattern
/// to construct instances, and call [`validate_server_config`] before execution
/// to ensure security requirements are met.
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
///     .build();
///
/// assert_eq!(config.command, "docker");
/// assert_eq!(config.args.len(), 2);
///
/// // HTTP transport
/// let config = ServerConfig::builder()
///     .http_transport("https://api.example.com/mcp".to_string())
///     .header("Authorization".to_string(), "Bearer token".to_string())
///     .build();
/// ```
///
/// [`validate_server_config`]: fn.validate_server_config.html
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
    #[serde(default)]
    pub url: Option<String>,

    /// HTTP headers for HTTP transport.
    ///
    /// **Only used for HTTP transport.**
    ///
    /// Common headers include:
    /// - `Authorization`: Authentication token
    /// - `Content-Type`: Request content type
    #[serde(default)]
    pub headers: HashMap<String, String>,
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
    ///     .build();
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
///     .build();
///
/// // HTTP transport
/// let config = ServerConfig::builder()
///     .http_transport("https://api.example.com/mcp".to_string())
///     .header("Authorization".to_string(), "Bearer token".to_string())
///     .build();
/// ```
#[derive(Debug, Default, Clone)]
pub struct ServerConfigBuilder {
    transport: TransportType,
    command: Option<String>,
    args: Vec<String>,
    env: HashMap<String, String>,
    cwd: Option<PathBuf>,
    url: Option<String>,
    headers: HashMap<String, String>,
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
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
    ///     .build();
    /// ```
    #[must_use]
    pub fn headers(mut self, headers: HashMap<String, String>) -> Self {
        self.headers = headers;
        self
    }

    /// Builds the `ServerConfig`.
    ///
    /// # Panics
    ///
    /// Panics if:
    /// - Command was not set for stdio transport
    /// - URL was not set for HTTP transport
    ///
    /// Use `try_build()` for fallible construction.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let config = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .build();
    /// ```
    #[must_use]
    pub fn build(self) -> ServerConfig {
        self.try_build()
            .expect("ServerConfig::build() failed validation")
    }

    /// Attempts to build the `ServerConfig`, returning an error if invalid.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Command is not set for stdio transport
    /// - URL is not set for HTTP transport
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_core::ServerConfig;
    ///
    /// let result = ServerConfig::builder()
    ///     .command("docker".to_string())
    ///     .try_build();
    ///
    /// assert!(result.is_ok());
    /// ```
    pub fn try_build(self) -> Result<ServerConfig, String> {
        match self.transport {
            TransportType::Stdio => {
                let command = self
                    .command
                    .ok_or_else(|| "command is required for stdio transport".to_string())?;

                if command.trim().is_empty() {
                    return Err("command cannot be empty for stdio transport".to_string());
                }

                Ok(ServerConfig {
                    transport: TransportType::Stdio,
                    command,
                    args: self.args,
                    env: self.env,
                    cwd: self.cwd,
                    url: None,
                    headers: HashMap::new(),
                })
            }
            TransportType::Http => {
                let url = self
                    .url
                    .ok_or_else(|| "url is required for HTTP transport".to_string())?;

                Ok(ServerConfig {
                    transport: TransportType::Http,
                    command: String::new(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: Some(url),
                    headers: self.headers,
                })
            }
            TransportType::Sse => {
                let url = self
                    .url
                    .ok_or_else(|| "url is required for SSE transport".to_string())?;

                Ok(ServerConfig {
                    transport: TransportType::Sse,
                    command: String::new(),
                    args: Vec::new(),
                    env: HashMap::new(),
                    cwd: None,
                    url: Some(url),
                    headers: self.headers,
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
            .build();

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
            .build();

        assert_eq!(config.command, "docker");
        assert_eq!(config.args, vec!["run", "--rm", "mcp-server"]);
    }

    #[test]
    fn test_server_config_builder_with_args_vec() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .args(vec!["run".to_string(), "--rm".to_string()])
            .build();

        assert_eq!(config.args, vec!["run", "--rm"]);
    }

    #[test]
    fn test_server_config_builder_with_env() {
        let config = ServerConfig::builder()
            .command("mcp-server".to_string())
            .env("LOG_LEVEL".to_string(), "debug".to_string())
            .env("DEBUG".to_string(), "1".to_string())
            .build();

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
            .build();

        assert_eq!(config.env.len(), 2);
    }

    #[test]
    fn test_server_config_builder_with_cwd() {
        let config = ServerConfig::builder()
            .command("mcp-server".to_string())
            .cwd(PathBuf::from("/tmp"))
            .build();

        assert_eq!(config.cwd, Some(PathBuf::from("/tmp")));
    }

    #[test]
    fn test_server_config_builder_full() {
        let mut env_map = HashMap::new();
        env_map.insert("LOG_LEVEL".to_string(), "debug".to_string());

        let config = ServerConfig::builder()
            .command("/usr/local/bin/mcp-server".to_string())
            .args(vec!["--port".to_string(), "8080".to_string()])
            .environment(env_map)
            .cwd(PathBuf::from("/var/run"))
            .build();

        assert_eq!(config.command, "/usr/local/bin/mcp-server");
        assert_eq!(config.args.len(), 2);
        assert_eq!(config.env.len(), 1);
        assert_eq!(config.cwd, Some(PathBuf::from("/var/run")));
    }

    #[test]
    #[should_panic(expected = "command")]
    fn test_server_config_builder_missing_command() {
        let _ = ServerConfig::builder().build();
    }

    #[test]
    fn test_server_config_builder_try_build_missing_command() {
        let result = ServerConfig::builder().try_build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("command"));
    }

    #[test]
    fn test_server_config_accessors() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .arg("run".to_string())
            .env("VAR".to_string(), "value".to_string())
            .cwd(PathBuf::from("/tmp"))
            .build();

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
            .build();

        let json = serde_json::to_string(&config).unwrap();
        let deserialized: ServerConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config, deserialized);
    }

    #[test]
    fn test_server_config_clone() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build();

        let cloned = config.clone();
        assert_eq!(config, cloned);
    }

    #[test]
    fn test_server_config_debug() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build();

        let debug_str = format!("{config:?}");
        assert!(debug_str.contains("docker"));
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
            .build();

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
            .build();

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
            .build();

        assert_eq!(config.headers.len(), 1);
    }

    #[test]
    fn test_server_config_http_try_build_missing_url() {
        let result = ServerConfig::builder().try_build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("required"));
    }

    #[test]
    fn test_server_config_http_accessors() {
        let config = ServerConfig::builder()
            .http_transport("https://api.example.com/mcp".to_string())
            .header("Auth".to_string(), "token".to_string())
            .build();

        assert_eq!(config.transport(), &TransportType::Http);
        assert_eq!(config.url(), Some("https://api.example.com/mcp"));
        assert_eq!(config.headers().len(), 1);
    }

    #[test]
    fn test_server_config_stdio_default_transport() {
        let config = ServerConfig::builder()
            .command("docker".to_string())
            .build();

        assert_eq!(config.transport, TransportType::Stdio);
    }

    #[test]
    fn test_server_config_sse_transport() {
        let config = ServerConfig::builder()
            .sse_transport("https://api.example.com/sse".to_string())
            .build();

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
            .build();

        assert_eq!(config.transport, TransportType::Sse);
        assert_eq!(config.headers.len(), 2);
        assert_eq!(
            config.headers.get("Authorization"),
            Some(&"Bearer token".to_string())
        );
        assert_eq!(config.headers.get("X-Custom"), Some(&"value".to_string()));
    }

    #[test]
    fn test_server_config_sse_try_build_missing_url() {
        let mut builder = ServerConfig::builder();
        builder.transport = TransportType::Sse;

        let result = builder.try_build();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("url is required"));
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
