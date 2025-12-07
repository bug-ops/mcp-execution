//! Types for Claude Agent SDK code generation.
//!
//! Defines data structures used during Claude Agent SDK code generation,
//! where each tool is generated with Zod schemas for the Claude Agent SDK.

use serde::{Deserialize, Serialize};

/// Context for rendering a single tool template.
///
/// Contains all data needed to generate one tool file for the
/// Claude Agent SDK format.
///
/// # Examples
///
/// ```
/// use mcp_codegen::claude_agent::ToolContext;
///
/// let context = ToolContext {
///     name: "create_issue".to_string(),
///     typescript_name: "createIssue".to_string(),
///     pascal_name: "CreateIssue".to_string(),
///     description: "Creates a new issue".to_string(),
///     properties: vec![],
/// };
///
/// assert_eq!(context.typescript_name, "createIssue");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    /// Original tool name (snake_case)
    pub name: String,
    /// TypeScript-friendly name (camelCase)
    pub typescript_name: String,
    /// PascalCase name for type definitions
    pub pascal_name: String,
    /// Human-readable description
    pub description: String,
    /// Extracted properties with Zod types
    pub properties: Vec<PropertyInfo>,
}

/// Information about a single parameter property with Zod type.
///
/// Used in Handlebars templates to render Zod schema definitions.
///
/// # Examples
///
/// ```
/// use mcp_codegen::claude_agent::PropertyInfo;
///
/// let prop = PropertyInfo {
///     name: "title".to_string(),
///     zod_type: "string".to_string(),
///     zod_modifiers: vec![],
///     description: Some("Issue title".to_string()),
///     required: true,
/// };
///
/// assert_eq!(prop.zod_type, "string");
/// assert!(prop.required);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyInfo {
    /// Property name
    pub name: String,
    /// Zod type (e.g., "string", "number", "boolean")
    pub zod_type: String,
    /// Additional Zod modifiers (e.g., ".int()", ".email()")
    pub zod_modifiers: Vec<String>,
    /// Optional description from schema
    pub description: Option<String>,
    /// Whether the property is required
    pub required: bool,
}

impl PropertyInfo {
    /// Returns the full Zod type expression with modifiers.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::claude_agent::PropertyInfo;
    ///
    /// let prop = PropertyInfo {
    ///     name: "email".to_string(),
    ///     zod_type: "string".to_string(),
    ///     zod_modifiers: vec![".email()".to_string()],
    ///     description: Some("User email".to_string()),
    ///     required: true,
    /// };
    ///
    /// assert_eq!(prop.full_zod_type(), "string().email()");
    /// ```
    #[must_use]
    pub fn full_zod_type(&self) -> String {
        let mut result = format!("{}()", self.zod_type);
        for modifier in &self.zod_modifiers {
            result.push_str(modifier);
        }
        result
    }
}

/// Context for rendering the server.ts template.
///
/// Contains server-level metadata and list of all tools.
///
/// # Examples
///
/// ```
/// use mcp_codegen::claude_agent::ServerContext;
///
/// let context = ServerContext {
///     server_name: "GitHub".to_string(),
///     server_variable_name: "github".to_string(),
///     server_version: "1.0.0".to_string(),
///     tool_count: 30,
///     tools: vec![],
/// };
///
/// assert_eq!(context.server_name, "GitHub");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerContext {
    /// Server name for documentation
    pub server_name: String,
    /// Variable name for server (camelCase)
    pub server_variable_name: String,
    /// Server version
    pub server_version: String,
    /// Total number of tools
    pub tool_count: usize,
    /// List of tool summaries
    pub tools: Vec<ToolSummary>,
}

/// Summary of a tool for server file generation.
///
/// Lighter-weight than full `ToolContext`, used only for
/// imports and tool array in server.ts.
///
/// # Examples
///
/// ```
/// use mcp_codegen::claude_agent::ToolSummary;
///
/// let summary = ToolSummary {
///     typescript_name: "createIssue".to_string(),
/// };
///
/// assert_eq!(summary.typescript_name, "createIssue");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSummary {
    /// TypeScript-friendly name (camelCase)
    pub typescript_name: String,
}

/// Context for rendering the index.ts template.
///
/// Contains exports and server information.
///
/// # Examples
///
/// ```
/// use mcp_codegen::claude_agent::IndexContext;
///
/// let context = IndexContext {
///     server_name: "GitHub".to_string(),
///     server_variable_name: "github".to_string(),
///     server_version: "1.0.0".to_string(),
///     tool_count: 30,
///     tools: vec![],
/// };
///
/// assert_eq!(context.tool_count, 30);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IndexContext {
    /// Server name for documentation
    pub server_name: String,
    /// Variable name for server (camelCase)
    pub server_variable_name: String,
    /// Server version
    pub server_version: String,
    /// Total number of tools
    pub tool_count: usize,
    /// List of tool summaries
    pub tools: Vec<ToolSummary>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tool_context() {
        let context = ToolContext {
            name: "create_issue".to_string(),
            typescript_name: "createIssue".to_string(),
            pascal_name: "CreateIssue".to_string(),
            description: "Creates an issue".to_string(),
            properties: vec![],
        };

        assert_eq!(context.name, "create_issue");
        assert_eq!(context.typescript_name, "createIssue");
        assert_eq!(context.pascal_name, "CreateIssue");
    }

    #[test]
    fn test_property_info() {
        let prop = PropertyInfo {
            name: "title".to_string(),
            zod_type: "string".to_string(),
            zod_modifiers: vec![],
            description: Some("Issue title".to_string()),
            required: true,
        };

        assert_eq!(prop.name, "title");
        assert_eq!(prop.zod_type, "string");
        assert!(prop.required);
        assert_eq!(prop.full_zod_type(), "string()");
    }

    #[test]
    fn test_property_info_with_modifiers() {
        let prop = PropertyInfo {
            name: "email".to_string(),
            zod_type: "string".to_string(),
            zod_modifiers: vec![".email()".to_string()],
            description: None,
            required: true,
        };

        assert_eq!(prop.full_zod_type(), "string().email()");
    }

    #[test]
    fn test_property_info_multiple_modifiers() {
        let prop = PropertyInfo {
            name: "count".to_string(),
            zod_type: "number".to_string(),
            zod_modifiers: vec![".int()".to_string(), ".min(0)".to_string()],
            description: None,
            required: false,
        };

        assert_eq!(prop.full_zod_type(), "number().int().min(0)");
    }

    #[test]
    fn test_server_context() {
        let context = ServerContext {
            server_name: "GitHub".to_string(),
            server_variable_name: "github".to_string(),
            server_version: "1.0.0".to_string(),
            tool_count: 30,
            tools: vec![],
        };

        assert_eq!(context.server_name, "GitHub");
        assert_eq!(context.server_variable_name, "github");
        assert_eq!(context.tool_count, 30);
    }

    #[test]
    fn test_index_context() {
        let context = IndexContext {
            server_name: "GitHub".to_string(),
            server_variable_name: "github".to_string(),
            server_version: "1.0.0".to_string(),
            tool_count: 5,
            tools: vec![],
        };

        assert_eq!(context.server_name, "GitHub");
        assert_eq!(context.tool_count, 5);
    }

    #[test]
    fn test_tool_summary() {
        let summary = ToolSummary {
            typescript_name: "createIssue".to_string(),
        };

        assert_eq!(summary.typescript_name, "createIssue");
    }
}
