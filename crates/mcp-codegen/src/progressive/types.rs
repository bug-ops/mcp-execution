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
///     category: Some("issues".to_string()),
///     keywords: Some("create,issue,new,bug".to_string()),
///     short_description: Some("Create a new issue".to_string()),
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
    /// Optional category for tool grouping
    pub category: Option<String>,
    /// Optional keywords for discovery via grep/search
    pub keywords: Option<String>,
    /// Optional short description for header comment
    pub short_description: Option<String>,
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
///     categories: None,
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
    /// Tools grouped by category (optional, for categorized generation)
    #[serde(skip_serializing_if = "Option::is_none")]
    pub categories: Option<Vec<CategoryInfo>>,
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
///     category: Some("issues".to_string()),
///     keywords: Some("create,issue,new".to_string()),
///     short_description: Some("Create a new issue".to_string()),
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
    /// Optional category for tool grouping
    pub category: Option<String>,
    /// Optional keywords for discovery via grep/search
    pub keywords: Option<String>,
    /// Optional short description for header comment
    pub short_description: Option<String>,
}

/// Categorization metadata for a single tool.
///
/// Contains all categorization data from Claude's analysis.
///
/// # Examples
///
/// ```
/// use mcp_codegen::progressive::ToolCategorization;
///
/// let cat = ToolCategorization {
///     category: "issues".to_string(),
///     keywords: "create,issue,new,bug".to_string(),
///     short_description: "Create a new issue in a repository".to_string(),
/// };
///
/// assert_eq!(cat.category, "issues");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCategorization {
    /// Category for tool grouping
    pub category: String,
    /// Comma-separated keywords for discovery
    pub keywords: String,
    /// Concise description for header comment
    pub short_description: String,
}

/// Category information for grouped tool display in index.
///
/// Groups tools by category for organized documentation.
///
/// # Examples
///
/// ```
/// use mcp_codegen::progressive::{CategoryInfo, ToolSummary};
///
/// let category = CategoryInfo {
///     name: "issues".to_string(),
///     tools: vec![
///         ToolSummary {
///             typescript_name: "createIssue".to_string(),
///             description: "Creates a new issue".to_string(),
///             category: Some("issues".to_string()),
///             keywords: Some("create,issue".to_string()),
///             short_description: Some("Create issue".to_string()),
///         },
///     ],
/// };
///
/// assert_eq!(category.name, "issues");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryInfo {
    /// Category name
    pub name: String,
    /// Tools in this category
    pub tools: Vec<ToolSummary>,
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
            category: Some("issues".to_string()),
            keywords: Some("create,issue,new".to_string()),
            short_description: Some("Create a new issue".to_string()),
        };

        assert_eq!(context.server_id, "github");
        assert_eq!(context.name, "create_issue");
        assert_eq!(context.typescript_name, "createIssue");
        assert_eq!(context.category, Some("issues".to_string()));
        assert_eq!(context.keywords, Some("create,issue,new".to_string()));
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
            categories: None,
        };

        assert_eq!(context.server_name, "GitHub");
        assert_eq!(context.tool_count, 5);
        assert!(context.categories.is_none());
    }

    #[test]
    fn test_tool_summary() {
        let summary = ToolSummary {
            typescript_name: "createIssue".to_string(),
            description: "Creates an issue".to_string(),
            category: Some("issues".to_string()),
            keywords: Some("create,issue".to_string()),
            short_description: Some("Create issue".to_string()),
        };

        assert_eq!(summary.typescript_name, "createIssue");
        assert_eq!(summary.category, Some("issues".to_string()));
        assert_eq!(summary.keywords, Some("create,issue".to_string()));
    }

    #[test]
    fn test_bridge_context_default() {
        let context = BridgeContext::default();
        // Just verify it can be constructed
        let _serialized = serde_json::to_string(&context).unwrap();
    }
}
