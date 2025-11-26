//! Types for progressive loading code generation.
//!
//! Defines data structures used during progressive code generation,
//! where each tool is generated as a separate file.

use serde::{Deserialize, Serialize};

/// Context for rendering a single tool template.
///
/// Contains all data needed to generate one tool file in the
/// progressive loading pattern.
///
/// # Examples
///
/// ```
/// use mcp_codegen::progressive::ToolContext;
/// use serde_json::json;
///
/// let context = ToolContext {
///     server_id: "github".to_string(),
///     name: "create_issue".to_string(),
///     typescript_name: "createIssue".to_string(),
///     description: "Creates a new issue".to_string(),
///     input_schema: json!({"type": "object"}),
///     properties: vec![],
/// };
///
/// assert_eq!(context.server_id, "github");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    /// MCP server identifier
    pub server_id: String,
    /// Original tool name (snake_case)
    pub name: String,
    /// TypeScript-friendly name (camelCase)
    pub typescript_name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters
    pub input_schema: serde_json::Value,
    /// Extracted properties for template rendering
    pub properties: Vec<PropertyInfo>,
}

/// Information about a single parameter property.
///
/// Used in Handlebars templates to render parameter type definitions.
///
/// # Examples
///
/// ```
/// use mcp_codegen::progressive::PropertyInfo;
///
/// let prop = PropertyInfo {
///     name: "title".to_string(),
///     typescript_type: "string".to_string(),
///     description: Some("Issue title".to_string()),
///     required: true,
/// };
///
/// assert_eq!(prop.name, "title");
/// assert!(prop.required);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PropertyInfo {
    /// Property name
    pub name: String,
    /// TypeScript type (e.g., "string", "number", "boolean")
    pub typescript_type: String,
    /// Optional description from schema
    pub description: Option<String>,
    /// Whether the property is required
    pub required: bool,
}

/// Context for rendering the index.ts template.
///
/// Contains server-level metadata and list of all tools.
///
/// # Examples
///
/// ```
/// use mcp_codegen::progressive::IndexContext;
///
/// let context = IndexContext {
///     server_name: "GitHub".to_string(),
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
    /// Server version
    pub server_version: String,
    /// Total number of tools
    pub tool_count: usize,
    /// List of tool summaries
    pub tools: Vec<ToolSummary>,
}

/// Summary of a tool for index file generation.
///
/// Lighter-weight than full `ToolContext`, used only for
/// re-exports and documentation in index.ts.
///
/// # Examples
///
/// ```
/// use mcp_codegen::progressive::ToolSummary;
///
/// let summary = ToolSummary {
///     typescript_name: "createIssue".to_string(),
///     description: "Creates a new issue".to_string(),
/// };
///
/// assert_eq!(summary.typescript_name, "createIssue");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSummary {
    /// TypeScript-friendly name (camelCase)
    pub typescript_name: String,
    /// Human-readable description
    pub description: String,
}

/// Context for rendering the runtime bridge template.
///
/// Currently minimal, but allows for future extension with
/// server-specific configuration or metadata.
///
/// # Examples
///
/// ```
/// use mcp_codegen::progressive::BridgeContext;
///
/// let context = BridgeContext::default();
/// // Currently no fields, but provides extensibility
/// ```
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BridgeContext {
    // Future: could include server-specific config, auth info, etc.
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_tool_context() {
        let context = ToolContext {
            server_id: "github".to_string(),
            name: "create_issue".to_string(),
            typescript_name: "createIssue".to_string(),
            description: "Creates an issue".to_string(),
            input_schema: json!({"type": "object"}),
            properties: vec![],
        };

        assert_eq!(context.server_id, "github");
        assert_eq!(context.name, "create_issue");
        assert_eq!(context.typescript_name, "createIssue");
    }

    #[test]
    fn test_property_info() {
        let prop = PropertyInfo {
            name: "title".to_string(),
            typescript_type: "string".to_string(),
            description: Some("Issue title".to_string()),
            required: true,
        };

        assert_eq!(prop.name, "title");
        assert_eq!(prop.typescript_type, "string");
        assert!(prop.required);
    }

    #[test]
    fn test_index_context() {
        let context = IndexContext {
            server_name: "GitHub".to_string(),
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
            description: "Creates an issue".to_string(),
        };

        assert_eq!(summary.typescript_name, "createIssue");
    }

    #[test]
    fn test_bridge_context_default() {
        let context = BridgeContext::default();
        // Just verify it can be constructed
        let _serialized = serde_json::to_string(&context).unwrap();
    }
}
