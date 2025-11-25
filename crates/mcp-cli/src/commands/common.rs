//! Common utilities shared across CLI commands.
//!
//! Provides shared functionality for building server configurations from CLI arguments.

use anyhow::{Result, bail};
use mcp_core::{ServerConfig, ServerConfigBuilder, ServerId};
use std::path::PathBuf;

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
        Ok((parts[0].to_string(), parts[1].to_string()))
    };

    // Build config based on transport type
    let (server_id, config) = if let Some(url) = http {
        // HTTP transport
        let mut builder = ServerConfig::builder().http_transport(url.clone());

        for header in headers {
            let (key, value) = parse_key_value(&header, "header")?;
            builder = builder.header(key, value);
        }

        let id = ServerId::new(&url);
        (id, builder.build())
    } else if let Some(url) = sse {
        // SSE transport
        let mut builder = ServerConfig::builder().sse_transport(url.clone());

        for header in headers {
            let (key, value) = parse_key_value(&header, "header")?;
            builder = builder.header(key, value);
        }

        let id = ServerId::new(&url);
        (id, builder.build())
    } else {
        // Stdio transport (default)
        let command = server.expect("server is required for stdio transport");
        let mut builder: ServerConfigBuilder = ServerConfig::builder().command(command.clone());

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

        let id = ServerId::new(&command);
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
}
