//! Claude Agent Skills format generation.
//!
//! This module provides types and rendering functions for generating
//! skill files in the Claude Agent Skills format.
//!
//! # Claude Format Specification
//!
//! Skills are generated in `.claude/skills/skill-name/` directory:
//! - `SKILL.md` - Main skill file with YAML frontmatter (REQUIRED)
//! - `REFERENCE.md` - Detailed API documentation (OPTIONAL)
//! - `examples/` - Usage examples (OPTIONAL)
//!
//! # Examples
//!
//! ```
//! use mcp_codegen::skills::claude::{SkillData, ToolData, ParameterData};
//!
//! let skill = SkillData {
//!     skill_name: "github".to_string(),
//!     skill_description: "Interact with VK Teams messenger".to_string(),
//!     server_name: "github".to_string(),
//!     server_version: "1.0.0".to_string(),
//!     server_description: "VK Teams MCP server".to_string(),
//!     protocol_version: "1.0".to_string(),
//!     generated_at: "2025-01-22T00:00:00Z".to_string(),
//!     tool_count: 1,
//!     tools: vec![],
//!     capabilities: vec!["tools".to_string()],
//! };
//! ```

use mcp_core::Result;
use serde::Serialize;

/// Data for rendering Claude skill templates.
///
/// This structure contains all information needed to render both
/// `SKILL.md` and `REFERENCE.md` templates for a Claude skill.
///
/// # Examples
///
/// ```
/// use mcp_codegen::skills::claude::SkillData;
///
/// let data = SkillData {
///     skill_name: "my-skill".to_string(),
///     skill_description: "My skill description".to_string(),
///     server_name: "my-server".to_string(),
///     server_version: "1.0.0".to_string(),
///     server_description: "Server description".to_string(),
///     protocol_version: "1.0".to_string(),
///     generated_at: chrono::Utc::now().to_rfc3339(),
///     tool_count: 0,
///     tools: vec![],
///     capabilities: vec![],
/// };
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct SkillData {
    /// Validated skill name (lowercase, alphanumeric, hyphens, underscores)
    pub skill_name: String,

    /// Validated skill description (max 1024 chars, no XML/templates)
    pub skill_description: String,

    /// MCP server name
    pub server_name: String,

    /// MCP server version
    pub server_version: String,

    /// MCP server description
    pub server_description: String,

    /// MCP protocol version
    pub protocol_version: String,

    /// Timestamp when skill was generated (ISO 8601 format)
    pub generated_at: String,

    /// Number of tools in this skill
    pub tool_count: usize,

    /// List of tool definitions
    pub tools: Vec<ToolData>,

    /// Server capabilities (e.g., "tools", "prompts", "resources")
    pub capabilities: Vec<String>,
}

impl SkillData {
    /// Creates a new `SkillData` with the current timestamp.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::skills::claude::SkillData;
    ///
    /// let data = SkillData::new(
    ///     "my-skill".to_string(),
    ///     "Description".to_string(),
    ///     "server".to_string(),
    ///     "1.0.0".to_string(),
    ///     "Server desc".to_string(),
    ///     "1.0".to_string(),
    ///     vec![],
    ///     vec![],
    /// );
    /// ```
    #[must_use]
    #[allow(clippy::too_many_arguments)] // Data structure constructor
    pub fn new(
        skill_name: String,
        skill_description: String,
        server_name: String,
        server_version: String,
        server_description: String,
        protocol_version: String,
        tools: Vec<ToolData>,
        capabilities: Vec<String>,
    ) -> Self {
        let tool_count = tools.len();
        let generated_at = chrono::Utc::now().to_rfc3339();

        Self {
            skill_name,
            skill_description,
            server_name,
            server_version,
            server_description,
            protocol_version,
            generated_at,
            tool_count,
            tools,
            capabilities,
        }
    }
}

/// Data for a single MCP tool in a Claude skill.
///
/// Contains all information needed to document a tool in the skill file.
///
/// # Examples
///
/// ```
/// use mcp_codegen::skills::claude::ToolData;
///
/// let tool = ToolData {
///     name: "send_message".to_string(),
///     description: "Sends a message to a chat".to_string(),
///     parameters: vec![],
///     input_schema_json: r#"{"type": "object"}"#.to_string(),
/// };
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct ToolData {
    /// Tool name as defined in MCP server
    pub name: String,

    /// Human-readable tool description
    pub description: String,

    /// List of tool parameters
    pub parameters: Vec<ParameterData>,

    /// JSON Schema for input parameters (pretty-printed JSON string)
    pub input_schema_json: String,
}

/// Data for a single tool parameter.
///
/// Describes a parameter for an MCP tool, including its type,
/// whether it's required, and an example value.
///
/// # Examples
///
/// ```
/// use mcp_codegen::skills::claude::ParameterData;
///
/// let param = ParameterData {
///     name: "chat_id".to_string(),
///     type_name: "string".to_string(),
///     required: true,
///     description: "Chat identifier".to_string(),
///     example_value: r#""123456""#.to_string(),
/// };
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct ParameterData {
    /// Parameter name
    pub name: String,

    /// TypeScript/JSON Schema type name (e.g., "string", "number", "boolean")
    pub type_name: String,

    /// Whether this parameter is required
    pub required: bool,

    /// Parameter description
    pub description: String,

    /// Example value as JSON string (e.g., `"hello"`, `123`, `true`)
    pub example_value: String,
}

/// Renders a `SKILL.md` file from template data.
///
/// Uses the Handlebars template at `templates/claude/skill.md.hbs`
/// to generate a skill file with YAML frontmatter.
///
/// # Errors
///
/// Returns an error if:
/// - Template rendering fails
/// - Template is not registered
///
/// # Examples
///
/// ```no_run
/// use mcp_codegen::skills::claude::{render_skill_md, SkillData};
/// use mcp_codegen::TemplateEngine;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let engine = TemplateEngine::new()?;
/// let data = SkillData::new(
///     "test-skill".to_string(),
///     "Test skill description".to_string(),
///     "test-server".to_string(),
///     "1.0.0".to_string(),
///     "Test server".to_string(),
///     "1.0".to_string(),
///     vec![],
///     vec![],
/// );
///
/// let rendered = render_skill_md(&engine, &data)?;
/// assert!(rendered.starts_with("---\n"));
/// # Ok(())
/// # }
/// ```
pub fn render_skill_md(engine: &crate::TemplateEngine<'_>, data: &SkillData) -> Result<String> {
    engine.render("claude_skill", data)
}

/// Renders a `REFERENCE.md` file from template data.
///
/// Uses the Handlebars template at `templates/claude/reference.md.hbs`
/// to generate detailed API documentation.
///
/// # Errors
///
/// Returns an error if:
/// - Template rendering fails
/// - Template is not registered
///
/// # Examples
///
/// ```no_run
/// use mcp_codegen::skills::claude::{render_reference_md, SkillData};
/// use mcp_codegen::TemplateEngine;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let engine = TemplateEngine::new()?;
/// let data = SkillData::new(
///     "test-skill".to_string(),
///     "Test skill description".to_string(),
///     "test-server".to_string(),
///     "1.0.0".to_string(),
///     "Test server".to_string(),
///     "1.0".to_string(),
///     vec![],
///     vec![],
/// );
///
/// let rendered = render_reference_md(&engine, &data)?;
/// assert!(rendered.contains("# test-server MCP Server Reference"));
/// # Ok(())
/// # }
/// ```
pub fn render_reference_md(engine: &crate::TemplateEngine<'_>, data: &SkillData) -> Result<String> {
    engine.render("claude_reference", data)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_data() -> SkillData {
        SkillData::new(
            "test-skill".to_string(),
            "Test skill for unit testing".to_string(),
            "test-server".to_string(),
            "1.0.0".to_string(),
            "A test MCP server".to_string(),
            "1.0".to_string(),
            vec![ToolData {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
                parameters: vec![ParameterData {
                    name: "param1".to_string(),
                    type_name: "string".to_string(),
                    required: true,
                    description: "Test parameter".to_string(),
                    example_value: r#""test""#.to_string(),
                }],
                input_schema_json: r#"{"type": "object"}"#.to_string(),
            }],
            vec!["tools".to_string()],
        )
    }

    #[test]
    fn test_skill_data_creation() {
        let data = create_test_data();
        assert_eq!(data.skill_name, "test-skill");
        assert_eq!(data.tool_count, 1);
        assert_eq!(data.tools.len(), 1);
    }

    #[test]
    fn test_skill_data_timestamp() {
        let data = create_test_data();
        // Should be valid RFC3339 timestamp
        assert!(chrono::DateTime::parse_from_rfc3339(&data.generated_at).is_ok());
    }

    #[test]
    fn test_tool_data() {
        let tool = ToolData {
            name: "send_message".to_string(),
            description: "Sends a message".to_string(),
            parameters: vec![],
            input_schema_json: r#"{"type": "object"}"#.to_string(),
        };

        assert_eq!(tool.name, "send_message");
        assert_eq!(tool.parameters.len(), 0);
    }

    #[test]
    fn test_parameter_data() {
        let param = ParameterData {
            name: "chat_id".to_string(),
            type_name: "string".to_string(),
            required: true,
            description: "Chat ID".to_string(),
            example_value: r#""123""#.to_string(),
        };

        assert_eq!(param.name, "chat_id");
        assert!(param.required);
    }
}
