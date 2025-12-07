//! Type definitions for MCP server tools.
//!
//! This module defines all parameter and result types for the three main tools:
//! - `introspect_server`: Connect to and introspect an MCP server
//! - `save_categorized_tools`: Generate TypeScript files with categorization
//! - `list_generated_servers`: List all servers with generated files

use chrono::{DateTime, Utc};
use mcp_core::{ServerConfig, ServerId};
use mcp_introspector::ServerInfo;
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
/// use mcp_server::types::IntrospectServerParams;
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
/// use mcp_server::types::{SaveCategorizedToolsParams, CategorizedTool};
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
// generate_skill types
// ============================================================================

/// Parameters for generating a skill.
///
/// # Examples
///
/// ```
/// use mcp_server::types::GenerateSkillParams;
///
/// let params = GenerateSkillParams {
///     server_id: "github".to_string(),
///     servers_dir: None,
///     skill_name: None,
///     use_case_hints: None,
/// };
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct GenerateSkillParams {
    /// Server identifier (e.g., "github").
    ///
    /// Must contain only lowercase letters, digits, and hyphens.
    pub server_id: String,

    /// Base directory for generated servers.
    ///
    /// Default: `~/.claude/servers`
    pub servers_dir: Option<PathBuf>,

    /// Custom skill name.
    ///
    /// Default: `{server_id}-progressive`
    pub skill_name: Option<String>,

    /// Additional context about intended use cases.
    ///
    /// Helps generate more relevant documentation.
    pub use_case_hints: Option<Vec<String>>,
}

/// Result from `generate_skill` tool.
///
/// Contains all context Claude needs to generate optimal SKILL.md content.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct GenerateSkillResult {
    /// Server identifier.
    pub server_id: String,

    /// Suggested skill name.
    pub skill_name: String,

    /// Server description (inferred from tools).
    pub server_description: Option<String>,

    /// Tools grouped by category.
    pub categories: Vec<SkillCategory>,

    /// Total tool count.
    pub tool_count: usize,

    /// Example tool usages (for documentation).
    pub example_tools: Vec<ToolExample>,

    /// Prompt template for skill generation.
    ///
    /// Claude uses this prompt to generate SKILL.md content.
    pub generation_prompt: String,

    /// Output path for the skill file.
    pub output_path: String,
}

/// A category of tools for the skill.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillCategory {
    /// Category name (e.g., "issues", "repositories").
    pub name: String,

    /// Human-readable display name.
    pub display_name: String,

    /// Tools in this category.
    pub tools: Vec<SkillTool>,
}

/// Tool information for skill generation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillTool {
    /// Original tool name.
    pub name: String,

    /// TypeScript function name.
    pub typescript_name: String,

    /// Short description.
    pub description: String,

    /// Keywords for discovery.
    pub keywords: Vec<String>,

    /// Required parameters.
    pub required_params: Vec<String>,

    /// Optional parameters.
    pub optional_params: Vec<String>,
}

/// Example tool usage for documentation.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct ToolExample {
    /// Tool name.
    pub tool_name: String,

    /// Natural language description of what this does.
    pub description: String,

    /// Example CLI command.
    pub cli_command: String,

    /// Example parameters as JSON.
    pub params_json: String,
}

// ============================================================================
// save_skill types
// ============================================================================

/// Parameters for saving a skill.
///
/// # Examples
///
/// ```
/// use mcp_server::types::SaveSkillParams;
///
/// let params = SaveSkillParams {
///     server_id: "github".to_string(),
///     content: "---\nname: github\n---\n# GitHub".to_string(),
///     output_path: None,
///     overwrite: false,
/// };
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct SaveSkillParams {
    /// Server identifier.
    pub server_id: String,

    /// SKILL.md content (markdown with YAML frontmatter).
    pub content: String,

    /// Custom output path.
    ///
    /// Default: `~/.claude/skills/{server_id}/SKILL.md`
    pub output_path: Option<PathBuf>,

    /// Overwrite if exists.
    #[serde(default)]
    pub overwrite: bool,
}

/// Result from saving a skill.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SaveSkillResult {
    /// Whether save was successful.
    pub success: bool,

    /// Path where skill was saved.
    pub output_path: String,

    /// Whether an existing file was overwritten.
    pub overwritten: bool,

    /// Skill metadata extracted from content.
    pub metadata: SkillMetadata,
}

/// Metadata extracted from saved skill.
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
pub struct SkillMetadata {
    /// Skill name from frontmatter.
    pub name: String,

    /// Description from frontmatter.
    pub description: String,

    /// Section count (H2 headers).
    pub section_count: usize,

    /// Approximate word count.
    pub word_count: usize,
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
    /// use mcp_server::types::PendingGeneration;
    /// use mcp_core::{ServerId, ServerConfig};
    /// use mcp_introspector::ServerInfo;
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
    /// use mcp_server::types::PendingGeneration;
    /// # use mcp_core::{ServerId, ServerConfig};
    /// # use mcp_introspector::ServerInfo;
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
        use mcp_core::ToolName;
        use mcp_introspector::{ServerCapabilities, ToolInfo};

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
