//! Type definitions for MCP server tools.
//!
//! This module defines all parameter and result types for the three main tools:
//! - `introspect_server`: Connect to and introspect an MCP server
//! - `save_categorized_tools`: Generate TypeScript files with categorization
//! - `list_generated_servers`: List all servers with generated files

use chrono::{DateTime, Utc};
use mcp_execution_core::{ServerConfig, ServerId};
use mcp_execution_introspector::ServerInfo;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

// ============================================================================
// introspect_server types
// ============================================================================

/// Parameters for introspecting an MCP server.
///
/// # Examples
///
/// ```
/// use mcp_execution_server::types::IntrospectServerParams;
/// use std::collections::HashMap;
///
/// let params = IntrospectServerParams {
///     server_id: "github".to_string(),
///     command: "npx".to_string(),
///     args: vec!["-y".to_string(), "@anthropic/mcp-server-github".to_string()],
///     env: HashMap::new(),
///     output_dir: None,
/// };
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IntrospectServerParams {
    /// Unique identifier for the server (e.g., "github", "filesystem")
    pub server_id: String,

    /// Command to start the server (e.g., "npx", "docker")
    pub command: String,

    /// Arguments to pass to the command
    #[serde(default)]
    pub args: Vec<String>,

    /// Environment variables for the server process
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Custom output directory (default: `~/.claude/servers/{server_id}`)
    pub output_dir: Option<PathBuf>,
}

/// Result from introspecting an MCP server.
///
/// Contains tool metadata for Claude to categorize and a session ID
/// for use with `save_categorized_tools`.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IntrospectServerResult {
    /// Server identifier
    pub server_id: String,

    /// Human-readable server name
    pub server_name: String,

    /// Number of tools discovered
    pub tools_found: usize,

    /// List of tools for categorization
    pub tools: Vec<ToolMetadata>,

    /// Session ID for `save_categorized_tools` call
    pub session_id: Uuid,

    /// Session expiration time (ISO 8601)
    pub expires_at: DateTime<Utc>,
}

/// Metadata about a tool for categorization by Claude.
///
/// Includes the tool name, description, and parameter names to help
/// Claude understand the tool's purpose and assign appropriate categories.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolMetadata {
    /// Original tool name
    pub name: String,

    /// Tool description from server
    pub description: String,

    /// Parameter names for context
    pub parameters: Vec<String>,
}

// ============================================================================
// save_categorized_tools types
// ============================================================================

/// Parameters for saving categorized tools.
///
/// # Examples
///
/// ```
/// use mcp_execution_server::types::{SaveCategorizedToolsParams, CategorizedTool};
/// use uuid::Uuid;
///
/// let params = SaveCategorizedToolsParams {
///     session_id: Uuid::new_v4(),
///     categorized_tools: vec![
///         CategorizedTool {
///             name: "create_issue".to_string(),
///             category: "issues".to_string(),
///             keywords: "create,issue,new,bug,feature".to_string(),
///             short_description: "Create a new issue in a repository".to_string(),
///         },
///     ],
/// };
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SaveCategorizedToolsParams {
    /// Session ID from `introspect_server` call
    pub session_id: Uuid,

    /// Tools with Claude's categorization
    pub categorized_tools: Vec<CategorizedTool>,
}

/// A tool with categorization metadata from Claude.
///
/// Claude analyzes the tool's purpose and provides:
/// - A category for grouping related tools
/// - Keywords for discovery via grep/search
/// - A concise description for file headers
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct CategorizedTool {
    /// Original tool name (must match introspected tool)
    pub name: String,

    /// Category assigned by Claude (e.g., "issues", "repos", "users")
    pub category: String,

    /// Comma-separated keywords for discovery
    pub keywords: String,

    /// Concise description (max 80 chars) for header comment
    pub short_description: String,
}

/// Result from saving categorized tools.
///
/// Reports success status, number of files generated, and any errors
/// that occurred during generation.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct SaveCategorizedToolsResult {
    /// Whether generation succeeded
    pub success: bool,

    /// Number of TypeScript files created
    pub files_generated: usize,

    /// Directory where files were written
    pub output_dir: String,

    /// Count of tools per category
    pub categories: HashMap<String, usize>,

    /// Any tools that failed to generate
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub errors: Vec<ToolGenerationError>,
}

/// Error that occurred while generating a specific tool.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct ToolGenerationError {
    /// Name of the tool that failed
    pub tool_name: String,

    /// Error message
    pub error: String,
}

// ============================================================================
// list_generated_servers types
// ============================================================================

/// Parameters for listing generated servers.
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct ListGeneratedServersParams {
    /// Base directory to scan (default: `~/.claude/servers`)
    pub base_dir: Option<String>,
}

/// Result from listing generated servers.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ListGeneratedServersResult {
    /// List of servers with generated files
    pub servers: Vec<GeneratedServerInfo>,

    /// Total number of servers found
    pub total_servers: usize,
}

/// Information about a server with generated progressive loading files.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct GeneratedServerInfo {
    /// Server identifier
    pub id: String,

    /// Number of tool files (excluding runtime)
    pub tool_count: usize,

    /// Last generation timestamp
    pub generated_at: Option<DateTime<Utc>>,

    /// Directory path
    pub output_dir: String,
}

// ============================================================================
// State management types
// ============================================================================

/// Pending generation session.
///
/// Stores introspection data between `introspect_server` and
/// `save_categorized_tools` calls.
#[derive(Debug, Clone)]
pub struct PendingGeneration {
    /// Server identifier
    pub server_id: ServerId,

    /// Full server introspection data
    pub server_info: ServerInfo,

    /// Server configuration for regeneration if needed
    pub config: ServerConfig,

    /// Output directory
    pub output_dir: PathBuf,

    /// Session creation time
    pub created_at: DateTime<Utc>,

    /// Session expiration time (30 minutes default)
    pub expires_at: DateTime<Utc>,
}

impl PendingGeneration {
    /// Default session timeout: 30 minutes.
    pub const DEFAULT_TIMEOUT_MINUTES: i64 = 30;

    /// Creates a new pending generation session.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::types::PendingGeneration;
    /// use mcp_execution_core::{ServerId, ServerConfig};
    /// use mcp_execution_introspector::ServerInfo;
    /// use std::path::PathBuf;
    ///
    /// # fn example(server_info: ServerInfo) {
    /// let server_id = ServerId::new("github");
    /// let config = ServerConfig::builder()
    ///     .command("npx".to_string())
    ///     .arg("-y".to_string())
    ///     .arg("@anthropic/mcp-server-github".to_string())
    ///     .build();
    /// let output_dir = PathBuf::from("/tmp/output");
    ///
    /// let pending = PendingGeneration::new(
    ///     server_id,
    ///     server_info,
    ///     config,
    ///     output_dir,
    /// );
    /// # }
    /// ```
    #[must_use]
    pub fn new(
        server_id: ServerId,
        server_info: ServerInfo,
        config: ServerConfig,
        output_dir: PathBuf,
    ) -> Self {
        let now = Utc::now();
        Self {
            server_id,
            server_info,
            config,
            output_dir,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(Self::DEFAULT_TIMEOUT_MINUTES),
        }
    }

    /// Checks if this session has expired.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::types::PendingGeneration;
    /// # use mcp_execution_core::{ServerId, ServerConfig};
    /// # use mcp_execution_introspector::ServerInfo;
    /// # use std::path::PathBuf;
    ///
    /// # fn example(server_info: ServerInfo) {
    /// let pending = PendingGeneration::new(
    ///     ServerId::new("test"),
    ///     server_info,
    ///     ServerConfig::builder().command("echo".to_string()).build(),
    ///     PathBuf::from("/tmp"),
    /// );
    ///
    /// assert!(!pending.is_expired());
    /// # }
    /// ```
    #[must_use]
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pending_generation_not_expired() {
        let pending = create_test_pending();
        assert!(!pending.is_expired());
    }

    #[test]
    fn test_categorized_tool_serialization() {
        let tool = CategorizedTool {
            name: "create_issue".to_string(),
            category: "issues".to_string(),
            keywords: "create,issue,new".to_string(),
            short_description: "Create a new issue".to_string(),
        };

        let json = serde_json::to_string(&tool).unwrap();
        let _deserialized: CategorizedTool = serde_json::from_str(&json).unwrap();
    }

    // Test helper
    fn create_test_pending() -> PendingGeneration {
        use mcp_execution_core::ToolName;
        use mcp_execution_introspector::{ServerCapabilities, ToolInfo};

        let server_id = ServerId::new("test");
        let server_info = ServerInfo {
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
                input_schema: serde_json::json!({}),
                output_schema: None,
            }],
        };
        let config = ServerConfig::builder().command("echo".to_string()).build();
        let output_dir = PathBuf::from("/tmp/test");

        PendingGeneration::new(server_id, server_info, config, output_dir)
    }
}
