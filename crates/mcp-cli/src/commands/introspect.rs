//! Introspect command implementation.
//!
//! Connects to an MCP server and displays its capabilities, tools, and metadata.

use anyhow::{Context, Result, bail};
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_core::{ServerConfig, ServerConfigBuilder, ServerId};
use mcp_introspector::{Introspector, ServerInfo, ToolInfo};
use serde::Serialize;
use std::path::PathBuf;
use tracing::info;

/// Result of server introspection.
///
/// Contains server information and list of available tools,
/// formatted for display to the user.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::introspect::{IntrospectionResult, ServerMetadata};
///
/// let result = IntrospectionResult {
///     server: ServerMetadata {
///         id: "github".to_string(),
///         name: "github".to_string(),
///         version: "1.0.0".to_string(),
///         supports_tools: true,
///         supports_resources: false,
///         supports_prompts: false,
///     },
///     tools: vec![],
/// };
///
/// assert_eq!(result.server.name, "github");
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct IntrospectionResult {
    /// Server metadata
    pub server: ServerMetadata,
    /// List of available tools
    pub tools: Vec<ToolMetadata>,
}

/// Server metadata for display.
///
/// Simplified representation of server information optimized
/// for CLI output formatting.
#[derive(Debug, Clone, Serialize)]
pub struct ServerMetadata {
    /// Server identifier
    pub id: String,
    /// Server name
    pub name: String,
    /// Server version
    pub version: String,
    /// Whether server supports tools
    pub supports_tools: bool,
    /// Whether server supports resources
    pub supports_resources: bool,
    /// Whether server supports prompts
    pub supports_prompts: bool,
}

/// Tool metadata for display.
///
/// Contains tool information with optional schema details
/// when detailed output is requested.
#[derive(Debug, Clone, Serialize)]
pub struct ToolMetadata {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
    /// Input schema (only included when detailed mode is enabled)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<serde_json::Value>,
    /// Output schema (only included when detailed mode is enabled and available)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<serde_json::Value>,
}

/// Runs the introspect command.
///
/// Connects to the specified server, discovers its tools, and displays
/// information according to the output format.
///
/// # Process
///
/// 1. Builds `ServerConfig` from CLI arguments
/// 2. Creates an introspector and connects to the server
/// 3. Discovers server capabilities and tools
/// 4. Formats the output according to the specified format
/// 5. Displays the results to stdout
///
/// # Arguments
///
/// * `server` - Server command (binary name or path), None for HTTP/SSE
/// * `args` - Arguments to pass to the server command
/// * `env` - Environment variables in KEY=VALUE format
/// * `cwd` - Working directory for the server process
/// * `http` - HTTP transport URL
/// * `sse` - SSE transport URL
/// * `headers` - HTTP headers in KEY=VALUE format
/// * `detailed` - Whether to show detailed tool schemas
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if:
/// - Server configuration is invalid
/// - Server connection fails
/// - Server introspection fails
/// - Output formatting fails
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::introspect;
/// use mcp_core::cli::OutputFormat;
///
/// # async fn example() -> anyhow::Result<()> {
/// // Simple server
/// let exit_code = introspect::run(
///     Some("github-mcp-server".to_string()),
///     vec!["stdio".to_string()],
///     vec![],
///     None,
///     None,
///     None,
///     vec![],
///     false,
///     OutputFormat::Json
/// ).await?;
///
/// // HTTP transport
/// let exit_code = introspect::run(
///     None,
///     vec![],
///     vec![],
///     None,
///     Some("https://api.githubcopilot.com/mcp/".to_string()),
///     None,
///     vec!["Authorization=Bearer token".to_string()],
///     false,
///     OutputFormat::Json
/// ).await?;
/// # Ok(())
/// # }
/// ```
#[allow(clippy::too_many_arguments)]
pub async fn run(
    server: Option<String>,
    args: Vec<String>,
    env: Vec<String>,
    cwd: Option<String>,
    http: Option<String>,
    sse: Option<String>,
    headers: Vec<String>,
    detailed: bool,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    // Build ServerConfig from CLI arguments
    let (server_id, config) = build_server_config(server, args, env, cwd, http, sse, headers)?;

    info!("Introspecting server: {}", server_id);
    info!("Transport: {:?}", config.transport());
    info!("Detailed: {}", detailed);
    info!("Output format: {}", output_format);

    // Create introspector
    let mut introspector = Introspector::new();

    // Discover server
    let server_info = introspector
        .discover_server(server_id.clone(), &config)
        .await
        .with_context(|| {
            format!(
                "failed to connect to server '{server_id}' - ensure the server is installed and accessible"
            )
        })?;

    info!(
        "Successfully discovered {} tools from server",
        server_info.tools.len()
    );

    // Build result
    let result = build_result(&server_info, detailed);

    // Format and display output
    let formatted = crate::formatters::format_output(&result, output_format)
        .context("failed to format introspection results")?;

    println!("{formatted}");

    Ok(ExitCode::SUCCESS)
}

/// Builds the introspection result from server info.
///
/// Transforms `ServerInfo` into `IntrospectionResult` suitable for CLI display.
///
/// # Arguments
///
/// * `server_info` - Server information from introspector
/// * `detailed` - Whether to include detailed tool schemas
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::introspect::build_result;
/// use mcp_introspector::{ServerInfo, ServerCapabilities};
/// use mcp_core::ServerId;
///
/// let server_info = ServerInfo {
///     id: ServerId::new("test"),
///     name: "Test Server".to_string(),
///     version: "1.0.0".to_string(),
///     tools: vec![],
///     capabilities: ServerCapabilities {
///         supports_tools: true,
///         supports_resources: false,
///         supports_prompts: false,
///     },
/// };
///
/// let result = build_result(&server_info, false);
/// assert_eq!(result.server.name, "Test Server");
/// assert_eq!(result.tools.len(), 0);
/// ```
#[must_use]
pub fn build_result(server_info: &ServerInfo, detailed: bool) -> IntrospectionResult {
    let server = ServerMetadata {
        id: server_info.id.as_str().to_string(),
        name: server_info.name.clone(),
        version: server_info.version.clone(),
        supports_tools: server_info.capabilities.supports_tools,
        supports_resources: server_info.capabilities.supports_resources,
        supports_prompts: server_info.capabilities.supports_prompts,
    };

    let tools = server_info
        .tools
        .iter()
        .map(|tool| build_tool_metadata(tool, detailed))
        .collect();

    IntrospectionResult { server, tools }
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
fn build_server_config(
    server: Option<String>,
    args: Vec<String>,
    env: Vec<String>,
    cwd: Option<String>,
    http: Option<String>,
    sse: Option<String>,
    headers: Vec<String>,
) -> Result<(ServerId, ServerConfig)> {
    // Parse environment variables
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

        // Add headers
        for header in headers {
            let (key, value) = parse_key_value(&header, "header")?;
            builder = builder.header(key, value);
        }

        let id = ServerId::new(&url);
        (id, builder.build())
    } else if let Some(url) = sse {
        // SSE transport
        let mut builder = ServerConfig::builder().sse_transport(url.clone());

        // Add headers
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

        // Add arguments
        if !args.is_empty() {
            builder = builder.args(args);
        }

        // Add environment variables
        for env_var in env {
            let (key, value) = parse_key_value(&env_var, "environment variable")?;
            builder = builder.env(key, value);
        }

        // Add working directory
        if let Some(dir) = cwd {
            builder = builder.cwd(PathBuf::from(dir));
        }

        let id = ServerId::new(&command);
        (id, builder.build())
    };

    Ok((server_id, config))
}

/// Builds tool metadata from tool info.
///
/// Transforms `ToolInfo` into `ToolMetadata` with optional schema details.
///
/// # Arguments
///
/// * `tool_info` - Tool information from introspector
/// * `detailed` - Whether to include input/output schemas
fn build_tool_metadata(tool_info: &ToolInfo, detailed: bool) -> ToolMetadata {
    ToolMetadata {
        name: tool_info.name.as_str().to_string(),
        description: tool_info.description.clone(),
        input_schema: if detailed {
            Some(tool_info.input_schema.clone())
        } else {
            None
        },
        output_schema: if detailed {
            tool_info.output_schema.clone()
        } else {
            None
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::ToolName;
    use mcp_introspector::ServerCapabilities;
    use serde_json::json;

    #[test]
    fn test_build_result_basic() {
        let server_info = ServerInfo {
            id: ServerId::new("test-server"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let result = build_result(&server_info, false);

        assert_eq!(result.server.id, "test-server");
        assert_eq!(result.server.name, "Test Server");
        assert_eq!(result.server.version, "1.0.0");
        assert!(result.server.supports_tools);
        assert!(!result.server.supports_resources);
        assert!(!result.server.supports_prompts);
        assert_eq!(result.tools.len(), 0);
    }

    #[test]
    fn test_build_result_with_tools_not_detailed() {
        let server_info = ServerInfo {
            id: ServerId::new("test"),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("tool1"),
                    description: "First tool".to_string(),
                    input_schema: json!({"type": "object"}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("tool2"),
                    description: "Second tool".to_string(),
                    input_schema: json!({"type": "string"}),
                    output_schema: Some(json!({"type": "boolean"})),
                },
            ],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: true,
                supports_prompts: true,
            },
        };

        let result = build_result(&server_info, false);

        assert_eq!(result.tools.len(), 2);
        assert_eq!(result.tools[0].name, "tool1");
        assert_eq!(result.tools[0].description, "First tool");
        assert!(result.tools[0].input_schema.is_none());
        assert!(result.tools[0].output_schema.is_none());

        assert_eq!(result.tools[1].name, "tool2");
        assert_eq!(result.tools[1].description, "Second tool");
        assert!(result.tools[1].input_schema.is_none());
        assert!(result.tools[1].output_schema.is_none());
    }

    #[test]
    fn test_build_result_with_tools_detailed() {
        let server_info = ServerInfo {
            id: ServerId::new("test"),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("tool1"),
                    description: "First tool".to_string(),
                    input_schema: json!({"type": "object", "properties": {"name": {"type": "string"}}}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("tool2"),
                    description: "Second tool".to_string(),
                    input_schema: json!({"type": "string"}),
                    output_schema: Some(json!({"type": "boolean"})),
                },
            ],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let result = build_result(&server_info, true);

        assert_eq!(result.tools.len(), 2);

        // First tool - has input schema but no output schema
        assert_eq!(result.tools[0].name, "tool1");
        assert!(result.tools[0].input_schema.is_some());
        assert_eq!(
            result.tools[0].input_schema.as_ref().unwrap()["type"],
            "object"
        );
        assert!(result.tools[0].output_schema.is_none());

        // Second tool - has both input and output schemas
        assert_eq!(result.tools[1].name, "tool2");
        assert!(result.tools[1].input_schema.is_some());
        assert_eq!(
            result.tools[1].input_schema.as_ref().unwrap()["type"],
            "string"
        );
        assert!(result.tools[1].output_schema.is_some());
        assert_eq!(
            result.tools[1].output_schema.as_ref().unwrap()["type"],
            "boolean"
        );
    }

    #[test]
    fn test_build_tool_metadata_not_detailed() {
        let tool_info = ToolInfo {
            name: ToolName::new("send_message"),
            description: "Sends a message".to_string(),
            input_schema: json!({"type": "object"}),
            output_schema: Some(json!({"type": "string"})),
        };

        let metadata = build_tool_metadata(&tool_info, false);

        assert_eq!(metadata.name, "send_message");
        assert_eq!(metadata.description, "Sends a message");
        assert!(metadata.input_schema.is_none());
        assert!(metadata.output_schema.is_none());
    }

    #[test]
    fn test_build_tool_metadata_detailed() {
        let tool_info = ToolInfo {
            name: ToolName::new("send_message"),
            description: "Sends a message".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "chat_id": {"type": "string"},
                    "text": {"type": "string"}
                }
            }),
            output_schema: Some(json!({"type": "string"})),
        };

        let metadata = build_tool_metadata(&tool_info, true);

        assert_eq!(metadata.name, "send_message");
        assert_eq!(metadata.description, "Sends a message");
        assert!(metadata.input_schema.is_some());
        assert_eq!(metadata.input_schema.as_ref().unwrap()["type"], "object");
        assert!(metadata.output_schema.is_some());
        assert_eq!(metadata.output_schema.as_ref().unwrap()["type"], "string");
    }

    #[test]
    fn test_introspection_result_serialization() {
        let result = IntrospectionResult {
            server: ServerMetadata {
                id: "test".to_string(),
                name: "Test Server".to_string(),
                version: "1.0.0".to_string(),
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolMetadata {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                input_schema: None,
                output_schema: None,
            }],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("Test Server"));
        assert!(json.contains("test_tool"));

        // Schemas should not be in JSON when None
        assert!(!json.contains("input_schema"));
        assert!(!json.contains("output_schema"));
    }

    #[test]
    fn test_introspection_result_serialization_with_schemas() {
        let result = IntrospectionResult {
            server: ServerMetadata {
                id: "test".to_string(),
                name: "Test Server".to_string(),
                version: "1.0.0".to_string(),
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolMetadata {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                input_schema: Some(json!({"type": "object"})),
                output_schema: Some(json!({"type": "string"})),
            }],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("input_schema"));
        assert!(json.contains("output_schema"));
        assert!(json.contains("\"type\":\"object\""));
        assert!(json.contains("\"type\":\"string\""));
    }

    #[tokio::test]
    async fn test_run_server_connection_failure() {
        let result = run(
            Some("nonexistent-server-xyz".to_string()),
            vec![],
            vec![],
            None,
            None,
            None,
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("failed to connect to server"));
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
    fn test_server_metadata_all_capabilities() {
        let metadata = ServerMetadata {
            id: "test".to_string(),
            name: "Test".to_string(),
            version: "2.0.0".to_string(),
            supports_tools: true,
            supports_resources: true,
            supports_prompts: true,
        };

        assert!(metadata.supports_tools);
        assert!(metadata.supports_resources);
        assert!(metadata.supports_prompts);
    }

    #[test]
    fn test_server_metadata_no_capabilities() {
        let metadata = ServerMetadata {
            id: "test".to_string(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            supports_tools: false,
            supports_resources: false,
            supports_prompts: false,
        };

        assert!(!metadata.supports_tools);
        assert!(!metadata.supports_resources);
        assert!(!metadata.supports_prompts);
    }

    #[test]
    fn test_tool_metadata_empty_description() {
        let metadata = ToolMetadata {
            name: "tool".to_string(),
            description: String::new(),
            input_schema: None,
            output_schema: None,
        };

        assert_eq!(metadata.description, "");
    }

    #[test]
    fn test_build_result_preserves_tool_order() {
        let server_info = ServerInfo {
            id: ServerId::new("test"),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("alpha"),
                    description: "A".to_string(),
                    input_schema: json!({}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("beta"),
                    description: "B".to_string(),
                    input_schema: json!({}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("gamma"),
                    description: "C".to_string(),
                    input_schema: json!({}),
                    output_schema: None,
                },
            ],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let result = build_result(&server_info, false);

        assert_eq!(result.tools.len(), 3);
        assert_eq!(result.tools[0].name, "alpha");
        assert_eq!(result.tools[1].name, "beta");
        assert_eq!(result.tools[2].name, "gamma");
    }
}
