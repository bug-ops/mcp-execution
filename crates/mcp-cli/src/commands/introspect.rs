//! Introspect command implementation.
//!
//! Connects to an MCP server and displays its capabilities, tools, and metadata.

use super::common::{ServerSource, resolve_server_config};
use anyhow::{Context, Result};
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_introspector::{Introspector, ServerInfo, ToolInfo};
use serde::Serialize;
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
    pub tools: Vec<ToolDisplay>,
}

/// Server metadata for display.
///
/// Simplified representation of server information optimized
/// for CLI output formatting.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::introspect::ServerMetadata;
///
/// let metadata = ServerMetadata {
///     id: "github".to_string(),
///     name: "GitHub MCP".to_string(),
///     version: "1.0.0".to_string(),
///     supports_tools: true,
///     supports_resources: false,
///     supports_prompts: false,
/// };
///
/// assert_eq!(metadata.name, "GitHub MCP");
/// ```
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

/// Tool information formatted for CLI display.
///
/// Contains tool information with optional schema details
/// when detailed output is requested.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::introspect::ToolDisplay;
///
/// let tool = ToolDisplay {
///     name: "search".to_string(),
///     description: "Search repositories".to_string(),
///     input_schema: None,
///     output_schema: None,
/// };
///
/// assert_eq!(tool.name, "search");
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct ToolDisplay {
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
/// * `source` - Resolved server-selection source: either a `~/.claude/mcp.json`
///   name or CLI transport flags with timeout overrides. Timeout overrides
///   only exist on the `Flags` arm — a `Config` source always uses the
///   `mcp.json` entry's own `connectTimeoutSecs`/`discoverTimeoutSecs`, so
///   there is no "ignored override" state to document.
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
/// use mcp_execution_cli::commands::common::{ServerSource, TransportArgs};
/// use mcp_execution_cli::commands::introspect;
/// use mcp_execution_core::cli::OutputFormat;
///
/// # async fn example() -> anyhow::Result<()> {
/// // Simple server
/// let exit_code = introspect::run(
///     ServerSource::Flags {
///         transport: TransportArgs::Stdio {
///             command: "github-mcp-server".to_string(),
///             args: vec!["stdio".to_string()],
///             env: vec![],
///             cwd: None,
///         },
///         connect_timeout_secs: None,
///         discover_timeout_secs: None,
///     },
///     false,
///     OutputFormat::Json
/// ).await?;
///
/// // HTTP transport with a shorter connect timeout
/// let exit_code = introspect::run(
///     ServerSource::Flags {
///         transport: TransportArgs::Http {
///             url: "https://api.githubcopilot.com/mcp/".to_string(),
///             headers: vec!["Authorization=Bearer token".to_string()],
///         },
///         connect_timeout_secs: Some(5),
///         discover_timeout_secs: None,
///     },
///     false,
///     OutputFormat::Json
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run(
    source: ServerSource,
    detailed: bool,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    // Build server config: either from mcp.json or from CLI arguments
    let (server_id, config) = resolve_server_config(source)?;

    info!("Introspecting server: {}", server_id);
    info!("Server config: {config:?}");
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
    crate::formatters::emit(&result, output_format, ExitCode::SUCCESS)
        .context("failed to format introspection results")
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
/// use mcp_execution_introspector::{ServerInfo, ServerCapabilities};
/// use mcp_execution_core::ServerId;
///
/// let server_info = ServerInfo {
///     id: ServerId::new("test").unwrap(),
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
/// Transforms `ToolInfo` into `ToolDisplay` with optional schema details.
///
/// # Arguments
///
/// * `tool_info` - Tool information from introspector
/// * `detailed` - Whether to include input/output schemas
fn build_tool_metadata(tool_info: &ToolInfo, detailed: bool) -> ToolDisplay {
    ToolDisplay {
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
    use crate::commands::common::TransportArgs;
    use mcp_execution_core::{REDACTED_PLACEHOLDER, ServerId, ToolName};
    use mcp_execution_introspector::ServerCapabilities;
    use serde_json::json;

    fn stdio_source(command: &str) -> ServerSource {
        ServerSource::Flags {
            transport: TransportArgs::Stdio {
                command: command.to_string(),
                args: vec![],
                env: vec![],
                cwd: None,
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        }
    }

    fn http_source(url: &str, headers: Vec<&str>) -> ServerSource {
        ServerSource::Flags {
            transport: TransportArgs::Http {
                url: url.to_string(),
                headers: headers.into_iter().map(String::from).collect(),
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        }
    }

    fn sse_source(url: &str, headers: Vec<&str>) -> ServerSource {
        ServerSource::Flags {
            transport: TransportArgs::Sse {
                url: url.to_string(),
                headers: headers.into_iter().map(String::from).collect(),
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        }
    }

    fn config_source(name: &str) -> ServerSource {
        ServerSource::Config {
            name: name.to_string(),
        }
    }

    #[test]
    fn test_build_result_basic() {
        let server_info = ServerInfo {
            id: ServerId::new("test-server").unwrap(),
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
            id: ServerId::new("test").unwrap(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("tool1").unwrap(),
                    description: "First tool".to_string(),
                    input_schema: json!({"type": "object"}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("tool2").unwrap(),
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
            id: ServerId::new("test").unwrap(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("tool1").unwrap(),
                    description: "First tool".to_string(),
                    input_schema: json!({"type": "object", "properties": {"name": {"type": "string"}}}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("tool2").unwrap(),
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
            name: ToolName::new("send_message").unwrap(),
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
            name: ToolName::new("send_message").unwrap(),
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
            tools: vec![ToolDisplay {
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
            tools: vec![ToolDisplay {
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
        let source = stdio_source("nonexistent-server-xyz");
        let result = run(source, false, OutputFormat::Json).await;

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
        let metadata = ToolDisplay {
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
            id: ServerId::new("test").unwrap(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("alpha").unwrap(),
                    description: "A".to_string(),
                    input_schema: json!({}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("beta").unwrap(),
                    description: "B".to_string(),
                    input_schema: json!({}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("gamma").unwrap(),
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
        let source = stdio_source("nonexistent-server");
        let result = run(source, false, OutputFormat::Text).await;

        // Connection should fail but format handling should not panic
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_with_pretty_format() {
        // Test that Pretty format output works correctly (colorized)
        let source = stdio_source("nonexistent-server");
        let result = run(source, false, OutputFormat::Pretty).await;

        // Connection should fail but format handling should not panic
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_with_detailed_mode() {
        // Test that detailed mode doesn't cause crashes even with connection failure
        let source = stdio_source("nonexistent-server");
        let result = run(source, true, OutputFormat::Json).await; // detailed mode

        assert!(result.is_err());
    }

    /// Port 99999 exceeds `u16::MAX`, so this always fails at the transport
    /// layer (`InvalidPort`) rather than actually reaching a network peer —
    /// deterministic without a live server.
    #[tokio::test]
    async fn test_run_http_transport() {
        let source = http_source(
            "https://localhost:99999/invalid",
            vec!["Authorization=Bearer test"],
        );
        let result = run(source, false, OutputFormat::Json).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let chain_msg = err
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | ");

        // Regression guard for #180: before the fix, every Http/Sse config
        // failed validation with this misleading message before the
        // transport was ever consulted. Asserting its absence (not just that
        // *some* error occurred) is what gives this test signal.
        assert!(
            !chain_msg.contains("command cannot be empty"),
            "must not regress to the pre-#180 empty-command validation error: {chain_msg}"
        );
        assert!(
            chain_msg.contains("MCP server connection failed"),
            "expected a real connection-layer failure, got: {chain_msg}"
        );
    }

    /// See `test_run_http_transport` — same deterministic-failure rationale.
    #[tokio::test]
    async fn test_run_sse_transport() {
        let source = sse_source("https://localhost:99999/sse", vec!["X-API-Key=test-key"]);
        let result = run(source, false, OutputFormat::Json).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let chain_msg = err
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | ");

        assert!(
            !chain_msg.contains("command cannot be empty"),
            "must not regress to the pre-#180 empty-command validation error: {chain_msg}"
        );
        assert!(
            chain_msg.contains("MCP server connection failed"),
            "expected a real connection-layer failure, got: {chain_msg}"
        );
    }

    #[tokio::test]
    async fn test_run_all_output_formats() {
        // Test all output formats don't cause panics
        for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
            let source = stdio_source("nonexistent");
            let result = run(source, false, format).await;

            assert!(result.is_err());
        }
    }

    #[tokio::test]
    async fn test_run_detailed_with_all_formats() {
        // Test detailed mode with all output formats
        for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
            let source = stdio_source("nonexistent");
            let result = run(source, true, format).await; // detailed

            assert!(result.is_err());
        }
    }

    #[test]
    fn test_build_result_empty_tools() {
        let server_info = ServerInfo {
            id: ServerId::new("empty").unwrap(),
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
                name: ToolName::new(&format!("tool_{i}")).unwrap(),
                description: format!("Tool number {i}"),
                input_schema: json!({"type": "object"}),
                output_schema: Some(json!({"type": "string"})),
            })
            .collect();

        let server_info = ServerInfo {
            id: ServerId::new("many-tools").unwrap(),
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
            name: ToolName::new("complex_tool").unwrap(),
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
        let metadata = ToolDisplay {
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
        let metadata = ToolDisplay {
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
            id: ServerId::new("mixed").unwrap(),
            name: "Mixed Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: ToolName::new("tool1").unwrap(),
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
        let source = config_source("nonexistent-server-xyz");
        let result = run(source, false, OutputFormat::Json).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("not found in")
                || err_msg.contains("failed to read MCP config")
                || err_msg.contains("mcp.json"),
            "Expected config-related error, got: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_run_from_config_takes_priority() {
        // When from_config is Some, it should be used for config loading
        // (server arg is unrepresentable alongside it — enforced by
        // `ServerSource` being a closed enum rather than a runtime check).
        let source = config_source("test-server");
        let result = run(source, false, OutputFormat::Json).await;

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
        let source = stdio_source("test-server-direct");
        let result = run(source, false, OutputFormat::Json).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        // Should fail with connection error, not config error
        assert!(
            err_msg.contains("failed to connect") || err_msg.contains("test-server-direct"),
            "Should try direct connection: {err_msg}"
        );
    }

    #[tokio::test]
    async fn test_run_zero_connect_timeout_override_rejected_by_validation() {
        // A zero override must surface the same connect_timeout validation
        // error as the mcp.json path, not just a generic connection failure.
        let source = ServerSource::Flags {
            transport: TransportArgs::Stdio {
                command: "nonexistent-server-timeout-test".to_string(),
                args: vec![],
                env: vec![],
                cwd: None,
            },
            connect_timeout_secs: Some(0),
            discover_timeout_secs: None,
        };
        let result = run(source, false, OutputFormat::Json).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let chain_msg = err
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            chain_msg.contains("greater than zero"),
            "expected connect_timeout validation error in the error chain, got: {chain_msg}"
        );
    }

    /// Captures the formatted `message` text of every tracing event observed while
    /// installed as the default subscriber, so a test can assert on what an `info!`
    /// call actually emitted instead of re-deriving it via a bare `format!` call.
    ///
    /// Minimal by design (mirrors the `WarnCounter`/`CorrelationLayer` precedents in
    /// `mcp-introspector`/`mcp-server`'s test suites): no span bookkeeping beyond
    /// what `tracing::Subscriber` requires, since these tests only need event text.
    #[derive(Clone, Default)]
    struct MessageCapture(std::sync::Arc<std::sync::Mutex<Vec<String>>>);

    impl MessageCapture {
        fn joined(&self) -> String {
            self.0.lock().unwrap().join("\n")
        }
    }

    impl tracing::Subscriber for MessageCapture {
        fn enabled(&self, _metadata: &tracing::Metadata<'_>) -> bool {
            true
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            struct MessageVisitor(Option<String>);
            impl tracing::field::Visit for MessageVisitor {
                fn record_debug(
                    &mut self,
                    field: &tracing::field::Field,
                    value: &dyn std::fmt::Debug,
                ) {
                    if field.name() == "message" {
                        self.0 = Some(format!("{value:?}"));
                    }
                }
            }

            let mut visitor = MessageVisitor(None);
            event.record(&mut visitor);
            if let Some(message) = visitor.0 {
                self.0.lock().unwrap().push(message);
            }
        }

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    /// Regression test for #336: drives the real CLI-args -> `resolve_server_config`
    /// -> `run` path (not a bare `ServerConfig::builder()` + `format!` call) with an
    /// HTTP header carrying a secret, and captures the actual tracing output the
    /// `--verbose` log line emits. The server connection still fails (no real
    /// server listening), but the log line fires before that attempt, so the
    /// capture reflects exactly what a `--verbose` run would print to stderr.
    #[tokio::test]
    async fn test_run_verbose_log_redacts_http_header_secret() {
        let secret_body = "sk-verySECRETtoken1234567890";
        let header = format!("Authorization=Bearer {secret_body}");
        let capture = MessageCapture::default();
        let _guard = tracing::subscriber::set_default(capture.clone());

        let source = http_source("https://localhost:99999/invalid", vec![&header]);
        let _ = run(source, false, OutputFormat::Json).await;

        let logged = capture.joined();
        assert!(logged.contains("Authorization"));
        assert!(logged.contains(REDACTED_PLACEHOLDER));
        assert!(!logged.contains(secret_body));
    }

    /// Regression test for #336, stdio transport variant: env var values (e.g.
    /// `GITHUB_TOKEN`) must not leak into the same log line either, exercised
    /// through the same CLI-args -> `run` path as the HTTP case above.
    #[tokio::test]
    async fn test_run_verbose_log_redacts_stdio_env_secret() {
        let secret_body = "ghp_verySECRETtoken1234567890abcdef";
        let capture = MessageCapture::default();
        let _guard = tracing::subscriber::set_default(capture.clone());

        let source = ServerSource::Flags {
            transport: TransportArgs::Stdio {
                command: "nonexistent-server-336".to_string(),
                args: vec![],
                env: vec![format!("GITHUB_TOKEN={secret_body}")],
                cwd: None,
            },
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };
        let _ = run(source, false, OutputFormat::Json).await;

        let logged = capture.joined();
        assert!(logged.contains("GITHUB_TOKEN"));
        assert!(logged.contains(REDACTED_PLACEHOLDER));
        assert!(!logged.contains(secret_body));
    }

    #[tokio::test]
    async fn test_run_with_valid_timeout_overrides_reaches_connection_attempt() {
        // Valid overrides must not be rejected before the connection attempt.
        let source = ServerSource::Flags {
            transport: TransportArgs::Stdio {
                command: "nonexistent-server-timeout-test-2".to_string(),
                args: vec![],
                env: vec![],
                cwd: None,
            },
            connect_timeout_secs: Some(5),
            discover_timeout_secs: Some(90),
        };
        let result = run(source, false, OutputFormat::Json).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("failed to connect to server"));
    }
}
