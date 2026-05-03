//! Common utilities shared across CLI commands.
//!
//! Provides shared functionality for building server configurations from CLI arguments
//! and loading MCP server definitions from `~/.claude/mcp.json`.

use anyhow::{Context, Result, bail};
use mcp_execution_core::{ServerConfig, ServerConfigBuilder, ServerId};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

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

/// Individual MCP server configuration entry from `mcp.json`.
#[derive(Debug, Clone, Deserialize)]
pub struct McpServerEntry {
    /// Command to execute (binary name or absolute path).
    pub command: String,
    /// Arguments to pass to the command.
    #[serde(default)]
    pub args: Vec<String>,
    /// Environment variables for the server process.
    #[serde(default)]
    pub env: HashMap<String, String>,
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
///     println!("{}: {} {:?}", name, entry.command, entry.args);
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
/// - [`McpServerEntry`] — raw entry for display purposes (command, args, env)
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
/// let (id, _config, entry) = get_mcp_server("github").unwrap();
/// assert_eq!(id.as_str(), "github");
/// println!("command: {}", entry.command);
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

/// Builds a core [`ServerConfig`] from an [`McpServerEntry`].
fn build_core_config(entry: &McpServerEntry) -> ServerConfig {
    let mut builder = ServerConfig::builder().command(entry.command.clone());

    if !entry.args.is_empty() {
        builder = builder.args(entry.args.clone());
    }

    for (key, value) in &entry.env {
        builder = builder.env(key.clone(), value.clone());
    }

    builder.build()
}

/// Builds `ServerConfig` from CLI arguments.
///
/// Parses CLI arguments into a `ServerConfig` for connecting to an MCP server.
///
/// # Arguments
///
/// * `server` - Server command (binary name or path)
/// * `args` - Arguments to pass to the server command
/// * `env` - Environment variables in KEY=VALUE format
/// * `cwd` - Working directory for the server process
/// * `http` - HTTP transport URL
/// * `sse` - SSE transport URL
/// * `headers` - HTTP headers in KEY=VALUE format
///
/// # Errors
///
/// Returns an error if environment variables or headers are not in KEY=VALUE format.
///
/// # Panics
///
/// Panics if `server` is `None` when using stdio transport (i.e., when neither
/// `http` nor `sse` is provided). This is enforced by CLI argument validation.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::common::build_server_config;
///
/// // Stdio transport
/// let (id, config) = build_server_config(
///     Some("github-mcp-server".to_string()),
///     vec!["stdio".to_string()],
///     vec!["TOKEN=abc".to_string()],
///     None,
///     None,
///     None,
///     vec![],
/// ).unwrap();
///
/// assert_eq!(id.as_str(), "github-mcp-server");
/// assert_eq!(config.args(), &["stdio"]);
/// ```
pub fn build_server_config(
    server: Option<String>,
    args: Vec<String>,
    env: Vec<String>,
    cwd: Option<String>,
    http: Option<String>,
    sse: Option<String>,
    headers: Vec<String>,
) -> Result<(ServerId, ServerConfig)> {
    // Parse environment variables / headers in KEY=VALUE format
    let parse_key_value = |s: &str, kind: &str| -> Result<(String, String)> {
        let parts: Vec<&str> = s.splitn(2, '=').collect();
        if parts.len() != 2 {
            bail!("invalid {kind} format: '{s}' (expected KEY=VALUE)");
        }
        if parts[0].is_empty() {
            bail!("invalid {kind} format: '{s}' (key cannot be empty)");
        }
        Ok((parts[0].to_string(), parts[1].to_string()))
    };

    // Build config based on transport type
    let (server_id, config) = if let Some(url) = http {
        // HTTP transport
        let id = ServerId::new(&url);
        let mut builder = ServerConfig::builder().http_transport(url);

        for header in headers {
            let (key, value) = parse_key_value(&header, "header")?;
            builder = builder.header(key, value);
        }

        (id, builder.build())
    } else if let Some(url) = sse {
        // SSE transport
        let id = ServerId::new(&url);
        let mut builder = ServerConfig::builder().sse_transport(url);

        for header in headers {
            let (key, value) = parse_key_value(&header, "header")?;
            builder = builder.header(key, value);
        }

        (id, builder.build())
    } else {
        // Stdio transport (default)
        let command = server.expect("server is required for stdio transport");
        let id = ServerId::new(&command);
        let mut builder: ServerConfigBuilder = ServerConfig::builder().command(command);

        if !args.is_empty() {
            builder = builder.args(args);
        }

        for env_var in env {
            let (key, value) = parse_key_value(&env_var, "environment variable")?;
            builder = builder.env(key, value);
        }

        if let Some(dir) = cwd {
            builder = builder.cwd(PathBuf::from(dir));
        }

        (id, builder.build())
    };

    Ok((server_id, config))
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
        // Server with only command (args and env should default)
        let json = r#"{"mcpServers": {"minimal": {"command": "python"}}}"#;
        let file = create_test_config(json);

        let config = load_mcp_config_from(file.path()).unwrap();
        let entry = &config.mcp_servers["minimal"];
        assert_eq!(entry.command, "python");
        assert!(entry.args.is_empty());
        assert!(entry.env.is_empty());
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

    #[test]
    fn test_build_server_config_stdio() {
        let (id, config) = build_server_config(
            Some("github-mcp-server".to_string()),
            vec!["stdio".to_string()],
            vec!["TOKEN=abc123".to_string()],
            None,
            None,
            None,
            vec![],
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
            Some("docker".to_string()),
            vec![
                "run".to_string(),
                "-i".to_string(),
                "--rm".to_string(),
                "ghcr.io/github/github-mcp-server".to_string(),
            ],
            vec!["GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxx".to_string()],
            None,
            None,
            None,
            vec![],
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
            None,
            vec![],
            vec![],
            None,
            Some("https://api.githubcopilot.com/mcp/".to_string()),
            None,
            vec!["Authorization=Bearer token123".to_string()],
        )
        .unwrap();

        assert_eq!(id.as_str(), "https://api.githubcopilot.com/mcp/");
        assert_eq!(config.url(), Some("https://api.githubcopilot.com/mcp/"));
        assert_eq!(
            config.headers().get("Authorization"),
            Some(&"Bearer token123".to_string())
        );
    }

    #[test]
    fn test_build_server_config_sse() {
        let (id, config) = build_server_config(
            None,
            vec![],
            vec![],
            None,
            None,
            Some("https://example.com/sse".to_string()),
            vec!["X-API-Key=secret".to_string()],
        )
        .unwrap();

        assert_eq!(id.as_str(), "https://example.com/sse");
        assert_eq!(config.url(), Some("https://example.com/sse"));
        assert_eq!(
            config.headers().get("X-API-Key"),
            Some(&"secret".to_string())
        );
    }

    #[test]
    fn test_build_server_config_with_cwd() {
        let (_, config) = build_server_config(
            Some("server".to_string()),
            vec![],
            vec![],
            Some("/tmp/workdir".to_string()),
            None,
            None,
            vec![],
        )
        .unwrap();

        assert_eq!(config.cwd(), Some(PathBuf::from("/tmp/workdir")).as_ref());
    }

    #[test]
    fn test_build_server_config_invalid_env() {
        let result = build_server_config(
            Some("server".to_string()),
            vec![],
            vec!["INVALID_FORMAT".to_string()],
            None,
            None,
            None,
            vec![],
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("expected KEY=VALUE")
        );
    }

    #[test]
    fn test_build_server_config_invalid_header() {
        let result = build_server_config(
            None,
            vec![],
            vec![],
            None,
            Some("https://example.com".to_string()),
            None,
            vec!["InvalidHeader".to_string()],
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("expected KEY=VALUE")
        );
    }

    #[test]
    fn test_build_server_config_multiple_env_vars() {
        let (_, config) = build_server_config(
            Some("server".to_string()),
            vec![],
            vec![
                "TOKEN=abc123".to_string(),
                "API_KEY=secret456".to_string(),
                "DEBUG=true".to_string(),
            ],
            None,
            None,
            None,
            vec![],
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
            Some("server".to_string()),
            vec![],
            vec![
                "TOKEN=abc=def=123".to_string(),
                "URL=https://example.com?key=value".to_string(),
                "ENCODED=a=b=c=d".to_string(),
            ],
            None,
            None,
            None,
            vec![],
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
            Some("simple-server".to_string()),
            vec![],
            vec![],
            None,
            None,
            None,
            vec![],
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
            None,
            vec![],
            vec![],
            None,
            Some("https://api.example.com".to_string()),
            None,
            vec![
                "Authorization=Bearer token123".to_string(),
                "X-API-Key=secret".to_string(),
                "Content-Type=application/json".to_string(),
            ],
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
            None,
            vec![],
            vec![],
            None,
            Some("https://api.example.com".to_string()),
            None,
            vec![
                "X-Custom=value=with=equals".to_string(),
                "X-Query=a=b&c=d".to_string(),
            ],
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
            None,
            vec![],
            vec![],
            None,
            None,
            Some("https://sse.example.com/events".to_string()),
            vec!["Authorization=Bearer xyz".to_string()],
        )
        .unwrap();

        assert_eq!(id.as_str(), "https://sse.example.com/events");
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
            Some("server".to_string()),
            vec![],
            vec!["EMPTY=".to_string()],
            None,
            None,
            None,
            vec![],
        )
        .unwrap();

        assert_eq!(config.env().get("EMPTY"), Some(&String::new()));
    }

    #[test]
    fn test_build_server_config_empty_value_in_header() {
        // Test header with empty value after equals
        let (_, config) = build_server_config(
            None,
            vec![],
            vec![],
            None,
            Some("https://example.com".to_string()),
            None,
            vec!["X-Empty=".to_string()],
        )
        .unwrap();

        assert_eq!(config.headers().get("X-Empty"), Some(&String::new()));
    }

    #[test]
    fn test_build_server_config_complex_docker_scenario() {
        let (id, config) = build_server_config(
            Some("docker".to_string()),
            vec![
                "run".to_string(),
                "-i".to_string(),
                "--rm".to_string(),
                "--network=host".to_string(),
                "my-image:latest".to_string(),
            ],
            vec![
                "API_TOKEN=secret123".to_string(),
                "LOG_LEVEL=debug".to_string(),
            ],
            Some("/app/workdir".to_string()),
            None,
            None,
            vec![],
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
        let result = build_server_config(
            Some("server".to_string()),
            vec![],
            vec!["=value".to_string()],
            None,
            None,
            None,
            vec![],
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("key cannot be empty")
        );
    }

    #[test]
    fn test_build_server_config_empty_key_in_header() {
        let result = build_server_config(
            None,
            vec![],
            vec![],
            None,
            Some("https://example.com".to_string()),
            None,
            vec!["=value".to_string()],
        );

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("key cannot be empty")
        );
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
        assert_eq!(servers[0].1.command, "node");
    }

    #[test]
    fn test_list_mcp_servers_from_empty_servers_key() {
        let json = r#"{"mcpServers": {}}"#;
        let file = create_test_config(json);

        let servers = list_mcp_servers_from(file.path()).unwrap();
        assert!(servers.is_empty());
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
}
