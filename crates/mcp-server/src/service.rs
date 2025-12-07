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

        let server_id = ServerId::new(&params.server_id);

        // Build server config
        let mut config_builder = ServerConfig::builder().command(params.command);

        for arg in &params.args {
            config_builder = config_builder.arg(arg.clone());
        }

        for (key, value) in &params.env {
            config_builder = config_builder.env(key.clone(), value.clone());
        }

        let config = config_builder.build();

        // Determine output directory
        let output_dir = params.output_dir.unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".claude")
                .join("servers")
                .join(&params.server_id)
        });

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
            server_id: params.server_id,
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

        // Create categorization map
        let categorization: HashMap<String, &CategorizedTool> = params
            .categorized_tools
            .iter()
            .map(|t| (t.name.clone(), t))
            .collect();

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

        // Ensure output directory exists
        std::fs::create_dir_all(&pending.output_dir).map_err(|e| {
            McpError::internal_error(format!("Failed to create output directory: {e}"), None)
        })?;

        // Export to filesystem
        vfs.export_to_filesystem(&pending.output_dir)
            .map_err(|e| McpError::internal_error(format!("Failed to export files: {e}"), None))?;

        // Build result with category stats
        let mut categories: HashMap<String, usize> = HashMap::new();
        for cat_tool in &params.categorized_tools {
            *categories.entry(cat_tool.category.clone()).or_default() += 1;
        }

        let result = SaveCategorizedToolsResult {
            success: true,
            files_generated: vfs.file_count(),
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
    // Convert CategorizedTool map to simple tool_name -> category map
    let categories: HashMap<String, String> = categorization
        .iter()
        .map(|(tool_name, cat_tool)| (tool_name.clone(), cat_tool.category.clone()))
        .collect();

    generator.generate_with_categories(server_info, &categories)
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
