//! Common utilities shared across CLI commands.
//!
//! Provides shared functionality for building server configurations from CLI arguments.

use anyhow::{Context, Result, bail};
use mcp_core::{ServerConfig, ServerConfigBuilder, ServerId};
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

/// MCP configuration file structure (~/.claude/mcp.json)
#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct McpConfig {
    mcp_servers: HashMap<String, McpServerConfig>,
}

/// Individual MCP server configuration
#[derive(Debug, Deserialize)]
struct McpServerConfig {
    command: String,
    #[serde(default)]
    args: Vec<String>,
    #[serde(default)]
    env: HashMap<String, String>,
}

/// Loads MCP configuration from ~/.claude/mcp.json
///
/// # Errors
///
/// Returns error if:
/// - Home directory cannot be determined
/// - Config file cannot be read
/// - JSON is malformed
fn load_mcp_config() -> Result<McpConfig> {
    let home = dirs::home_dir().context("failed to get home directory")?;
    let config_path = home.join(".claude").join("mcp.json");

    let content = std::fs::read_to_string(&config_path)
        .with_context(|| "failed to read MCP config from ~/.claude/mcp.json")?;

    let config: McpConfig =
        serde_json::from_str(&content).context("failed to parse MCP config JSON")?;

    Ok(config)
}

/// Loads server configuration from ~/.claude/mcp.json by server name.
///
/// # Arguments
///
/// * `name` - Server name from mcp.json (e.g., "github")
///
/// # Returns
///
/// Returns `(ServerId, ServerConfig)` if server is found in config.
///
/// # Errors
///
/// Returns error if:
/// - Config file doesn't exist or is malformed
/// - Server name not found in config
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
    let config = load_mcp_config()?;

    let server_config = config.mcp_servers.get(name).with_context(|| {
        format!(
            "server '{name}' not found in MCP config at ~/.claude/mcp.json\n\
             Hint: Use 'mcp-execution-cli server list' to see available servers"
        )
    })?;

    let id = ServerId::new(name);
    let mut builder = ServerConfig::builder().command(server_config.command.clone());

    if !server_config.args.is_empty() {
        builder = builder.args(server_config.args.clone());
    }

    for (key, value) in &server_config.env {
        builder = builder.env(key.clone(), value.clone());
    }

    Ok((id, builder.build()))
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
        // Test with non-existent server name
        let result = load_server_from_config("nonexistent");

        // Should fail because either config doesn't exist or server not in it
        assert!(result.is_err());
    }

    #[test]
    fn test_load_mcp_config_no_file() {
        // Should fail gracefully when config file doesn't exist
        let result = load_mcp_config();

        // Can fail either because home dir not found or config file missing
        // Both are acceptable error states
        if let Err(error) = result {
            let error = error.to_string();
            assert!(
                error.contains("failed to read MCP config")
                    || error.contains("failed to get home directory"),
                "Expected config read error or home dir error, got: {error}"
            );
        }
    }
}
