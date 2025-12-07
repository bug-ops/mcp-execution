//! MCP server implementation for progressive loading generation.
//!
//! The `GeneratorService` provides three main tools:
//! 1. `introspect_server` - Connect to and introspect an MCP server
//! 2. `save_categorized_tools` - Generate TypeScript files with categorization
//! 3. `list_generated_servers` - List all servers with generated files

use crate::state::StateManager;
use crate::types::{
    CategorizedTool, GeneratedServerInfo, IntrospectServerParams, IntrospectServerResult,
    ListGeneratedServersParams, ListGeneratedServersResult, PendingGeneration,
    SaveCategorizedToolsParams, SaveCategorizedToolsResult, ToolMetadata,
};
use mcp_codegen::progressive::ProgressiveGenerator;
use mcp_core::{ServerConfig, ServerId};
use mcp_files::FilesBuilder;
use mcp_introspector::Introspector;
use rmcp::handler::server::ServerHandler;
use rmcp::handler::server::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{
    CallToolResult, Content, Implementation, ProtocolVersion, ServerCapabilities, ServerInfo,
};
use rmcp::{ErrorData as McpError, tool, tool_handler, tool_router};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex;

/// MCP server for progressive loading generation.
///
/// This service helps generate progressive loading TypeScript files for other
/// MCP servers. Claude provides the categorization intelligence through natural
/// language understanding - no separate LLM API needed.
///
/// # Workflow
///
/// 1. Call `introspect_server` to discover tools from a target MCP server
/// 2. Claude analyzes the tools and assigns categories, keywords, descriptions
/// 3. Call `save_categorized_tools` to generate TypeScript files
/// 4. Use `list_generated_servers` to see all generated servers
///
/// # Examples
///
/// ```no_run
/// use mcp_server::service::GeneratorService;
/// use rmcp::transport::stdio;
///
/// # async fn example() {
/// let service = GeneratorService::new();
/// // Service implements rmcp ServerHandler trait
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct GeneratorService {
    /// State manager for pending generations
    state: Arc<StateManager>,

    /// MCP server introspector
    introspector: Arc<Mutex<Introspector>>,

    /// Tool router for MCP protocol
    tool_router: ToolRouter<Self>,
}

impl GeneratorService {
    /// Creates a new generator service.
    #[must_use]
    pub fn new() -> Self {
        Self {
            state: Arc::new(StateManager::new()),
            introspector: Arc::new(Mutex::new(Introspector::new())),
            tool_router: Self::tool_router(),
        }
    }
}

impl Default for GeneratorService {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl GeneratorService {
    /// Introspect an MCP server and prepare for categorization.
    ///
    /// Connects to the target MCP server, discovers its tools, and returns
    /// metadata for Claude to categorize. Returns a session ID for use with
    /// `save_categorized_tools`.
    #[tool(
        description = "Connect to an MCP server, discover its tools, and return metadata for categorization. Returns a session ID for use with save_categorized_tools."
    )]
    async fn introspect_server(
        &self,
        Parameters(params): Parameters<IntrospectServerParams>,
    ) -> Result<CallToolResult, McpError> {
        // Validate server_id format
        if !params
            .server_id
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(McpError::invalid_params(
                "server_id must contain only lowercase letters, digits, and hyphens",
                None,
            ));
        }

        // Extract server_id before consuming params
        let server_id_str = params.server_id;
        let server_id = ServerId::new(&server_id_str);

        // Determine output directory (needs server_id_str)
        let output_dir = params.output_dir.unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude")
                .join("servers")
                .join(&server_id_str)
        });

        // Build server config (consume args and env to avoid clones)
        let mut config_builder = ServerConfig::builder().command(params.command);

        for arg in params.args {
            config_builder = config_builder.arg(arg);
        }

        for (key, value) in params.env {
            config_builder = config_builder.env(key, value);
        }

        let config = config_builder.build();

        // Connect and introspect
        let server_info = {
            let mut introspector = self.introspector.lock().await;
            introspector
                .discover_server(server_id.clone(), &config)
                .await
                .map_err(|e| {
                    McpError::internal_error(format!("Failed to introspect server: {e}"), None)
                })?
        };

        // Extract tool metadata for Claude
        let tools: Vec<ToolMetadata> = server_info
            .tools
            .iter()
            .map(|tool| {
                let parameters = extract_parameter_names(&tool.input_schema);

                ToolMetadata {
                    name: tool.name.as_str().to_string(),
                    description: tool.description.clone(),
                    parameters,
                }
            })
            .collect();

        // Store pending generation
        let pending =
            PendingGeneration::new(server_id, server_info.clone(), config, output_dir.clone());

        let session_id = self.state.store(pending.clone()).await;

        // Build result
        let result = IntrospectServerResult {
            server_id: server_id_str,
            server_name: server_info.name,
            tools_found: tools.len(),
            tools,
            session_id,
            expires_at: pending.expires_at,
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize result: {e}"), None)
            })?,
        )]))
    }

    /// Save categorized tools as TypeScript files.
    ///
    /// Generates progressive loading TypeScript files using Claude's
    /// categorization. Requires `session_id` from a previous `introspect_server`
    /// call.
    #[tool(
        description = "Generate progressive loading TypeScript files using Claude's categorization. Requires session_id from a previous introspect_server call."
    )]
    async fn save_categorized_tools(
        &self,
        Parameters(params): Parameters<SaveCategorizedToolsParams>,
    ) -> Result<CallToolResult, McpError> {
        // Retrieve pending generation
        let pending = self.state.take(params.session_id).await.ok_or_else(|| {
            McpError::invalid_params(
                "Session not found or expired. Please run introspect_server again.",
                None,
            )
        })?;

        // Validate categorized tools match introspected tools
        let introspected_names: HashSet<_> = pending
            .server_info
            .tools
            .iter()
            .map(|t| t.name.as_str())
            .collect();

        for cat_tool in &params.categorized_tools {
            if !introspected_names.contains(cat_tool.name.as_str()) {
                return Err(McpError::invalid_params(
                    format!("Tool '{}' not found in introspected tools", cat_tool.name),
                    None,
                ));
            }
        }

        // Build categorization map and category stats in single pass (avoid double iteration)
        let tool_count = params.categorized_tools.len();
        let mut categorization: HashMap<String, &CategorizedTool> =
            HashMap::with_capacity(tool_count);
        let mut categories: HashMap<String, usize> = HashMap::with_capacity(tool_count);

        for tool in &params.categorized_tools {
            categorization.insert(tool.name.clone(), tool);
            *categories.entry(tool.category.clone()).or_default() += 1;
        }

        // Generate code with categorization
        let generator = ProgressiveGenerator::new().map_err(|e| {
            McpError::internal_error(format!("Failed to create generator: {e}"), None)
        })?;

        let code = generate_with_categorization(&generator, &pending.server_info, &categorization)
            .map_err(|e| McpError::internal_error(format!("Failed to generate code: {e}"), None))?;

        // Build virtual filesystem
        let vfs = FilesBuilder::from_generated_code(code, "/")
            .build()
            .map_err(|e| McpError::internal_error(format!("Failed to build VFS: {e}"), None))?;

        // Capture file count before moving vfs
        let files_generated = vfs.file_count();

        // Ensure output directory exists (async)
        tokio::fs::create_dir_all(&pending.output_dir)
            .await
            .map_err(|e| {
                McpError::internal_error(format!("Failed to create output directory: {e}"), None)
            })?;

        // Export to filesystem (blocking operation wrapped in spawn_blocking)
        let output_dir = pending.output_dir.clone();
        tokio::task::spawn_blocking(move || vfs.export_to_filesystem(&output_dir))
            .await
            .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?
            .map_err(|e| McpError::internal_error(format!("Failed to export files: {e}"), None))?;

        let result = SaveCategorizedToolsResult {
            success: true,
            files_generated,
            output_dir: pending.output_dir.display().to_string(),
            categories,
            errors: vec![],
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize result: {e}"), None)
            })?,
        )]))
    }

    /// List all servers with generated progressive loading files.
    ///
    /// Scans the output directory (default: `~/.claude/servers`) for servers
    /// that have generated TypeScript files.
    #[tool(
        description = "List all MCP servers that have generated progressive loading files in ~/.claude/servers/"
    )]
    async fn list_generated_servers(
        &self,
        Parameters(params): Parameters<ListGeneratedServersParams>,
    ) -> Result<CallToolResult, McpError> {
        let base_dir = params.base_dir.map_or_else(
            || {
                dirs::home_dir()
                    .unwrap_or_else(|| PathBuf::from("."))
                    .join(".claude")
                    .join("servers")
            },
            PathBuf::from,
        );

        // Scan directories (blocking operation wrapped in spawn_blocking)
        let servers = tokio::task::spawn_blocking(move || {
            let mut servers = Vec::new();

            if base_dir.exists()
                && base_dir.is_dir()
                && let Ok(entries) = std::fs::read_dir(&base_dir)
            {
                for entry in entries.flatten() {
                    if entry.path().is_dir() {
                        let id = entry.file_name().to_string_lossy().to_string();

                        // Count .ts files (excluding _runtime and starting with _)
                        let tool_count = std::fs::read_dir(entry.path())
                            .map(|e| {
                                e.flatten()
                                    .filter(|f| {
                                        let name = f.file_name();
                                        let name = name.to_string_lossy();
                                        name.ends_with(".ts") && !name.starts_with('_')
                                    })
                                    .count()
                            })
                            .unwrap_or(0);

                        // Get modification time
                        let generated_at = entry
                            .metadata()
                            .and_then(|m| m.modified())
                            .ok()
                            .map(chrono::DateTime::<chrono::Utc>::from);

                        servers.push(GeneratedServerInfo {
                            id,
                            tool_count,
                            generated_at,
                            output_dir: entry.path().display().to_string(),
                        });
                    }
                }
            }

            servers.sort_by(|a, b| a.id.cmp(&b.id));
            servers
        })
        .await
        .map_err(|e| McpError::internal_error(format!("Task join error: {e}"), None))?;

        let result = ListGeneratedServersResult {
            total_servers: servers.len(),
            servers,
        };

        Ok(CallToolResult::success(vec![Content::text(
            serde_json::to_string_pretty(&result).map_err(|e| {
                McpError::internal_error(format!("Failed to serialize result: {e}"), None)
            })?,
        )]))
    }
}

#[tool_handler]
impl ServerHandler for GeneratorService {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            protocol_version: ProtocolVersion::V_2024_11_05,
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            server_info: Implementation::from_build_env(),
            instructions: Some(
                "Generate progressive loading TypeScript files for MCP servers. \
                 Use introspect_server to discover tools, then save_categorized_tools \
                 with your categorization."
                    .to_string(),
            ),
        }
    }
}

// ============================================================================
// Helper functions
// ============================================================================

/// Extracts parameter names from a JSON Schema.
fn extract_parameter_names(schema: &serde_json::Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(|p| p.as_object())
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default()
}

/// Generates code with categorization metadata.
///
/// Converts the categorization map to the format expected by the generator
/// and calls `generate_with_categories`.
fn generate_with_categorization(
    generator: &ProgressiveGenerator,
    server_info: &mcp_introspector::ServerInfo,
    categorization: &HashMap<String, &CategorizedTool>,
) -> mcp_core::Result<mcp_codegen::GeneratedCode> {
    use mcp_codegen::progressive::ToolCategorization;

    // Convert CategorizedTool map to ToolCategorization map
    let categorizations: HashMap<String, ToolCategorization> = categorization
        .iter()
        .map(|(tool_name, cat_tool)| {
            (
                tool_name.clone(),
                ToolCategorization {
                    category: cat_tool.category.clone(),
                    keywords: cat_tool.keywords.clone(),
                    short_description: cat_tool.short_description.clone(),
                },
            )
        })
        .collect();

    generator.generate_with_categories(server_info, &categorizations)
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use mcp_core::ToolName;
    use mcp_introspector::{ServerCapabilities, ToolInfo};
    use rmcp::model::ErrorCode;
    use uuid::Uuid;

    // ========================================================================
    // Helper Functions Tests
    // ========================================================================

    #[test]
    fn test_extract_parameter_names() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "name": { "type": "string" },
                "age": { "type": "number" }
            }
        });

        let params = extract_parameter_names(&schema);
        assert_eq!(params.len(), 2);
        assert!(params.contains(&"name".to_string()));
        assert!(params.contains(&"age".to_string()));
    }

    #[test]
    fn test_extract_parameter_names_empty() {
        let schema = serde_json::json!({
            "type": "object"
        });

        let params = extract_parameter_names(&schema);
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_parameter_names_no_properties() {
        let schema = serde_json::json!({
            "type": "string"
        });

        let params = extract_parameter_names(&schema);
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_parameter_names_nested_object() {
        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "name": { "type": "string" }
                    }
                },
                "age": { "type": "number" }
            }
        });

        let params = extract_parameter_names(&schema);
        assert_eq!(params.len(), 2);
        assert!(params.contains(&"user".to_string()));
        assert!(params.contains(&"age".to_string()));
    }

    #[test]
    fn test_generate_with_categorization() {
        let generator = ProgressiveGenerator::new().unwrap();

        let server_id = ServerId::new("test");
        let server_info = mcp_introspector::ServerInfo {
            id: server_id.clone(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolInfo {
                name: ToolName::new("test_tool"),
                description: "Test tool description".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "param1": { "type": "string" }
                    }
                }),
                output_schema: None,
            }],
        };

        let categorized_tool = CategorizedTool {
            name: "test_tool".to_string(),
            category: "testing".to_string(),
            keywords: "test,tool".to_string(),
            short_description: "Test tool for testing".to_string(),
        };

        let mut categorization = HashMap::new();
        categorization.insert("test_tool".to_string(), &categorized_tool);

        let result = generate_with_categorization(&generator, &server_info, &categorization);
        assert!(result.is_ok());

        let code = result.unwrap();
        assert!(code.file_count() > 0, "Should generate at least one file");
    }

    #[test]
    fn test_generate_with_categorization_multiple_tools() {
        let generator = ProgressiveGenerator::new().unwrap();

        let server_id = ServerId::new("test");
        let server_info = mcp_introspector::ServerInfo {
            id: server_id.clone(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![
                ToolInfo {
                    name: ToolName::new("tool1"),
                    description: "First tool".to_string(),
                    input_schema: serde_json::json!({"type": "object"}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("tool2"),
                    description: "Second tool".to_string(),
                    input_schema: serde_json::json!({"type": "object"}),
                    output_schema: None,
                },
            ],
        };

        let tool1 = CategorizedTool {
            name: "tool1".to_string(),
            category: "category1".to_string(),
            keywords: "test".to_string(),
            short_description: "Tool 1".to_string(),
        };

        let tool2 = CategorizedTool {
            name: "tool2".to_string(),
            category: "category2".to_string(),
            keywords: "test".to_string(),
            short_description: "Tool 2".to_string(),
        };

        let mut categorization = HashMap::new();
        categorization.insert("tool1".to_string(), &tool1);
        categorization.insert("tool2".to_string(), &tool2);

        let result = generate_with_categorization(&generator, &server_info, &categorization);
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_with_categorization_empty_tools() {
        let generator = ProgressiveGenerator::new().unwrap();

        let server_id = ServerId::new("test");
        let server_info = mcp_introspector::ServerInfo {
            id: server_id,
            name: "Empty Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![],
        };

        let categorization = HashMap::new();

        let result = generate_with_categorization(&generator, &server_info, &categorization);
        assert!(result.is_ok());
    }

    // ========================================================================
    // Service Tests
    // ========================================================================

    #[test]
    fn test_generator_service_new() {
        let service = GeneratorService::new();
        assert!(service.introspector.try_lock().is_ok());
    }

    #[test]
    fn test_generator_service_default() {
        let service = GeneratorService::default();
        assert!(service.introspector.try_lock().is_ok());
    }

    #[test]
    fn test_get_info() {
        let service = GeneratorService::new();
        let info = service.get_info();

        assert_eq!(info.protocol_version, ProtocolVersion::V_2024_11_05);
        assert!(info.capabilities.tools.is_some());
        assert!(info.instructions.is_some());
    }

    // ========================================================================
    // Input Validation Tests
    // ========================================================================

    #[tokio::test]
    async fn test_introspect_server_invalid_server_id_uppercase() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "GitHub".to_string(), // Invalid: contains uppercase
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
        };

        let result = service.introspect_server(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS); // Invalid params error code
    }

    #[tokio::test]
    async fn test_introspect_server_invalid_server_id_underscore() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "git_hub".to_string(), // Invalid: contains underscore
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
        };

        let result = service.introspect_server(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    #[tokio::test]
    async fn test_introspect_server_invalid_server_id_special_chars() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "git@hub".to_string(), // Invalid: contains @
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
        };

        let result = service.introspect_server(Parameters(params)).await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_introspect_server_valid_server_id_with_hyphens() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "git-hub-server".to_string(), // Valid
            command: "echo".to_string(),
            args: vec!["test".to_string()],
            env: HashMap::new(),
            output_dir: None,
        };

        // This will fail because echo is not an MCP server, but validation should pass
        let result = service.introspect_server(Parameters(params)).await;

        // Should fail with internal error (connection), not invalid params
        if let Err(err) = result {
            assert_ne!(
                err.code,
                ErrorCode::INVALID_PARAMS,
                "Should not be invalid params error"
            );
        }
    }

    #[tokio::test]
    async fn test_introspect_server_valid_server_id_digits() {
        let service = GeneratorService::new();

        let params = IntrospectServerParams {
            server_id: "server123".to_string(), // Valid: lowercase + digits
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
        };

        let result = service.introspect_server(Parameters(params)).await;

        // Should fail with internal error (connection), not invalid params
        if let Err(err) = result {
            assert_ne!(err.code, ErrorCode::INVALID_PARAMS);
        }
    }

    // ========================================================================
    // save_categorized_tools Error Tests
    // ========================================================================

    #[tokio::test]
    async fn test_save_categorized_tools_invalid_session() {
        let service = GeneratorService::new();

        let params = SaveCategorizedToolsParams {
            session_id: Uuid::new_v4(), // Random UUID not in state
            categorized_tools: vec![],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS); // Invalid params
        assert!(err.message.contains("Session not found"));
    }

    #[tokio::test]
    async fn test_save_categorized_tools_tool_mismatch() {
        let service = GeneratorService::new();

        // Create a pending generation with tool1
        let server_id = ServerId::new("test");
        let server_info = mcp_introspector::ServerInfo {
            id: server_id.clone(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![ToolInfo {
                name: ToolName::new("tool1"),
                description: "Tool 1".to_string(),
                input_schema: serde_json::json!({"type": "object"}),
                output_schema: None,
            }],
        };

        let pending = PendingGeneration::new(
            server_id,
            server_info,
            ServerConfig::builder().command("echo".to_string()).build(),
            PathBuf::from("/tmp/test"),
        );

        let session_id = service.state.store(pending).await;

        // Try to save with tool2 (doesn't exist)
        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![CategorizedTool {
                name: "tool2".to_string(), // Mismatch!
                category: "test".to_string(),
                keywords: "test".to_string(),
                short_description: "Test".to_string(),
            }],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
        assert!(err.message.contains("not found in introspected tools"));
    }

    #[tokio::test]
    async fn test_save_categorized_tools_expired_session() {
        use chrono::Duration;

        let service = GeneratorService::new();

        // Create an expired pending generation
        let server_id = ServerId::new("test");
        let server_info = mcp_introspector::ServerInfo {
            id: server_id.clone(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools: vec![],
        };

        let mut pending = PendingGeneration::new(
            server_id,
            server_info,
            ServerConfig::builder().command("echo".to_string()).build(),
            PathBuf::from("/tmp/test"),
        );

        // Manually expire it
        pending.expires_at = Utc::now() - Duration::hours(1);

        let session_id = service.state.store(pending).await;

        let params = SaveCategorizedToolsParams {
            session_id,
            categorized_tools: vec![],
        };

        let result = service.save_categorized_tools(Parameters(params)).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        assert_eq!(err.code, ErrorCode::INVALID_PARAMS);
    }

    // ========================================================================
    // list_generated_servers Tests
    // ========================================================================

    #[tokio::test]
    async fn test_list_generated_servers_nonexistent_dir() {
        let service = GeneratorService::new();

        let params = ListGeneratedServersParams {
            base_dir: Some("/nonexistent/path/that/does/not/exist".to_string()),
        };

        let result = service.list_generated_servers(Parameters(params)).await;

        assert!(result.is_ok());
        let content = result.unwrap();
        let text_content = content.content[0].as_text().unwrap();
        let parsed: ListGeneratedServersResult = serde_json::from_str(&text_content.text).unwrap();

        assert_eq!(parsed.total_servers, 0);
        assert_eq!(parsed.servers.len(), 0);
    }

    #[tokio::test]
    async fn test_list_generated_servers_default_dir() {
        let service = GeneratorService::new();

        let params = ListGeneratedServersParams { base_dir: None };

        let result = service.list_generated_servers(Parameters(params)).await;

        // Should succeed even if directory doesn't exist
        assert!(result.is_ok());
    }
}
