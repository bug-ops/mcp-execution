//! Introspect command implementation.
//!
//! Connects to an MCP server and displays its capabilities, tools, and metadata.

use super::common::{build_server_config, load_server_from_config};
use anyhow::{Context, Result};
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_introspector::{Introspector, ServerInfo, ToolInfo};
use serde::Serialize;
use tracing::{debug, info};

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
/// 1. Builds `ServerConfig` from CLI arguments or loads from ~/.claude/mcp.json
/// 2. Creates an introspector and connects to the server
/// 3. Discovers server capabilities and tools
/// 4. Formats the output according to the specified format
/// 5. Displays the results to stdout
///
/// # Arguments
///
/// * `from_config` - Load server config from ~/.claude/mcp.json by name
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
///     None,
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
    from_config: Option<String>,
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
    // Build server config: either from mcp.json or from CLI arguments
    let (server_id, config) = if let Some(config_name) = from_config {
        debug!(
            "Loading server configuration from ~/.claude/mcp.json: {}",
            config_name
        );
        load_server_from_config(&config_name)?
    } else {
        build_server_config(server, args, env, cwd, http, sse, headers)?
    };

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
    use mcp_core::{ServerId, ToolName};
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
            None,
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

    // Note: build_server_config tests are in common.rs

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

    #[tokio::test]
    async fn test_run_with_text_format() {
        // Test that Text format output works correctly (compact JSON)
        let result = run(
            None,
            Some("nonexistent-server".to_string()),
            vec![],
            vec![],
            None,
            None,
            None,
            vec![],
            false,
            OutputFormat::Text,
        )
        .await;

        // Connection should fail but format handling should not panic
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_with_pretty_format() {
        // Test that Pretty format output works correctly (colorized)
        let result = run(
            None,
            Some("nonexistent-server".to_string()),
            vec![],
            vec![],
            None,
            None,
            None,
            vec![],
            false,
            OutputFormat::Pretty,
        )
        .await;

        // Connection should fail but format handling should not panic
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_with_detailed_mode() {
        // Test that detailed mode doesn't cause crashes even with connection failure
        let result = run(
            None,
            Some("nonexistent-server".to_string()),
            vec![],
            vec![],
            None,
            None,
            None,
            vec![],
            true, // detailed mode
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_http_transport() {
        // Test HTTP transport with invalid URL
        let result = run(
            None,
            None,
            vec![],
            vec![],
            None,
            Some("https://localhost:99999/invalid".to_string()),
            None,
            vec!["Authorization=Bearer test".to_string()],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("failed to connect to server"));
    }

    #[tokio::test]
    async fn test_run_sse_transport() {
        // Test SSE transport with invalid URL
        let result = run(
            None,
            None,
            vec![],
            vec![],
            None,
            None,
            Some("https://localhost:99999/sse".to_string()),
            vec!["X-API-Key=test-key".to_string()],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("failed to connect to server"));
    }

    #[tokio::test]
    async fn test_run_all_output_formats() {
        // Test all output formats don't cause panics
        for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
            let result = run(
                None,
                Some("nonexistent".to_string()),
                vec![],
                vec![],
                None,
                None,
                None,
                vec![],
                false,
                format,
            )
            .await;

            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn test_run_detailed_with_all_formats() {
        // Test detailed mode with all output formats
        for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
            let result = run(
                None,
                Some("nonexistent".to_string()),
                vec![],
                vec![],
                None,
                None,
                None,
                vec![],
                true, // detailed
                format,
            )
            .await;

            assert!(result.is_err());
        }
    }

    #[test]
    fn test_build_result_empty_tools() {
        let server_info = ServerInfo {
            id: ServerId::new("empty"),
            name: "Empty Server".to_string(),
            version: "0.1.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: false,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let result = build_result(&server_info, false);

        assert_eq!(result.server.name, "Empty Server");
        assert_eq!(result.tools.len(), 0);
        assert!(!result.server.supports_tools);
    }

    #[test]
    fn test_build_result_many_tools() {
        // Test with many tools to ensure no performance issues
        let tools: Vec<ToolInfo> = (0..100)
            .map(|i| ToolInfo {
                name: ToolName::new(&format!("tool_{i}")),
                description: format!("Tool number {i}"),
                input_schema: json!({"type": "object"}),
                output_schema: Some(json!({"type": "string"})),
            })
            .collect();

        let server_info = ServerInfo {
            id: ServerId::new("many-tools"),
            name: "Server with many tools".to_string(),
            version: "1.0.0".to_string(),
            tools,
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: true,
                supports_prompts: true,
            },
        };

        let result = build_result(&server_info, true);

        assert_eq!(result.tools.len(), 100);
        assert_eq!(result.tools[0].name, "tool_0");
        assert_eq!(result.tools[99].name, "tool_99");
        // In detailed mode, schemas should be present
        assert!(result.tools[0].input_schema.is_some());
        assert!(result.tools[0].output_schema.is_some());
    }

    #[test]
    fn test_build_tool_metadata_complex_schema() {
        let tool_info = ToolInfo {
            name: ToolName::new("complex_tool"),
            description: "Tool with complex schema".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "name": {"type": "string", "minLength": 1},
                    "age": {"type": "integer", "minimum": 0},
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                },
                "required": ["name"]
            }),
            output_schema: Some(json!({
                "type": "object",
                "properties": {
                    "success": {"type": "boolean"},
                    "message": {"type": "string"}
                }
            })),
        };

        let metadata = build_tool_metadata(&tool_info, true);

        assert_eq!(metadata.name, "complex_tool");
        assert!(metadata.input_schema.is_some());
        assert!(metadata.output_schema.is_some());

        let input = metadata.input_schema.as_ref().unwrap();
        assert_eq!(input["type"], "object");
        assert!(input["properties"]["name"].is_object());
        assert!(input["properties"]["tags"]["items"].is_object());
    }

    #[test]
    fn test_introspection_result_clone() {
        let result = IntrospectionResult {
            server: ServerMetadata {
                id: "test".to_string(),
                name: "Test".to_string(),
                version: "1.0.0".to_string(),
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![],
        };

        // Test Clone implementation
        let cloned = result.clone();
        assert_eq!(cloned.server.id, result.server.id);
        assert_eq!(cloned.server.name, result.server.name);
    }

    #[test]
    fn test_server_metadata_serialization_all_fields() {
        let metadata = ServerMetadata {
            id: "test-id".to_string(),
            name: "Test Server".to_string(),
            version: "2.1.0".to_string(),
            supports_tools: true,
            supports_resources: true,
            supports_prompts: true,
        };

        let json = serde_json::to_value(&metadata).unwrap();

        assert_eq!(json["id"], "test-id");
        assert_eq!(json["name"], "Test Server");
        assert_eq!(json["version"], "2.1.0");
        assert_eq!(json["supports_tools"], true);
        assert_eq!(json["supports_resources"], true);
        assert_eq!(json["supports_prompts"], true);
    }

    #[test]
    fn test_tool_metadata_serialization_without_schemas() {
        let metadata = ToolMetadata {
            name: "simple_tool".to_string(),
            description: "A simple tool".to_string(),
            input_schema: None,
            output_schema: None,
        };

        let json = serde_json::to_string(&metadata).unwrap();

        // Fields with None should not be serialized (skip_serializing_if)
        assert!(!json.contains("input_schema"));
        assert!(!json.contains("output_schema"));
        assert!(json.contains("simple_tool"));
        assert!(json.contains("A simple tool"));
    }

    #[test]
    fn test_tool_metadata_long_description() {
        let long_description = "A".repeat(1000);
        let metadata = ToolMetadata {
            name: "tool".to_string(),
            description: long_description.clone(),
            input_schema: None,
            output_schema: None,
        };

        // Should handle long descriptions without issues
        assert_eq!(metadata.description.len(), 1000);
        let json = serde_json::to_string(&metadata).unwrap();
        assert!(json.contains(&long_description));
    }

    #[test]
    fn test_build_result_mixed_capabilities() {
        let server_info = ServerInfo {
            id: ServerId::new("mixed"),
            name: "Mixed Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: ToolName::new("tool1"),
                description: "First".to_string(),
                input_schema: json!({}),
                output_schema: None,
            }],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: true,
                supports_prompts: false, // Mixed capabilities
            },
        };

        let result = build_result(&server_info, false);

        assert!(result.server.supports_tools);
        assert!(result.server.supports_resources);
        assert!(!result.server.supports_prompts);
    }

    #[tokio::test]
    async fn test_run_from_config_not_found() {
        let result = run(
            Some("nonexistent-server-xyz".to_string()),
            None,
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
        assert!(
            err_msg.contains("not found in MCP config")
                || err_msg.contains("failed to read MCP config"),
            "Expected config-related error, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_run_from_config_takes_priority() {
        // When from_config is Some, it should be used for config loading
        // (server arg should be None due to clap conflicts, but we test the logic)
        let result = run(
            Some("test-server".to_string()),
            None, // server is None when from_config is used
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

        // Should fail because config doesn't exist, not because of server
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // Should try to load from config, not use manual server
        assert!(
            err_msg.contains("MCP config") || err_msg.contains("test-server"),
            "Should attempt config loading: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_run_manual_mode_backward_compatible() {
        // Existing behavior: from_config = None, use server arg
        let result = run(
            None, // from_config
            Some("test-server-direct".to_string()),
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
        // Should fail with connection error, not config error
        assert!(
            err_msg.contains("failed to connect") || err_msg.contains("test-server-direct"),
            "Should try direct connection: {err_msg}"
        );
    }
}
