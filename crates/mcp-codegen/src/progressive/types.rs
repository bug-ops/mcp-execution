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
/// use mcp_execution_codegen::progressive::ToolContext;
/// use serde_json::json;
///
/// let context = ToolContext {
///     server_id: "github".to_string(),
///     name: "create_issue".to_string(),
///     name_literal: "create_issue".to_string(),
///     server_id_literal: "github".to_string(),
///     typescript_name: "createIssue".to_string(),
///     description: "Creates a new issue".to_string(),
///     input_schema: json!({"type": "object"}),
///     properties: vec![],
///     category: Some("issues".to_string()),
///     keywords: Some("create,issue,new,bug".to_string()),
///     short_description: "Create a new issue".to_string(),
/// };
///
/// assert_eq!(context.server_id, "github");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    /// MCP server identifier, sanitized for safe embedding in a `JSDoc` comment
    pub server_id: String,
    /// Original tool name (`snake_case`), sanitized for safe embedding in a `JSDoc` comment
    pub name: String,
    /// Original tool name escaped for safe embedding in a single-quoted TS string literal
    pub name_literal: String,
    /// Server identifier escaped for safe embedding in a single-quoted TS string literal
    pub server_id_literal: String,
    /// TypeScript-friendly name (camelCase), sanitized to a safe identifier
    pub typescript_name: String,
    /// Human-readable description
    pub description: String,
    /// JSON Schema for input parameters, with `description` fields sanitized
    /// for safe interpolation into `JSDoc` block comments (see issue #102).
    pub input_schema: serde_json::Value,
    /// Extracted properties for template rendering
    pub properties: Vec<PropertyInfo>,
    /// Optional category for tool grouping
    pub category: Option<String>,
    /// Optional keywords for discovery via grep/search
    pub keywords: Option<String>,
    /// Short description for header comment. Always populated: `ProgressiveGenerator`'s only
    /// constructor falls back to `description` when no categorization short description is
    /// available, so `None` is not a state this type can represent.
    pub short_description: String,
}

/// Information about a single parameter property.
///
/// Used in Handlebars templates to render parameter type definitions.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::progressive::PropertyInfo;
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
/// use mcp_execution_codegen::progressive::IndexContext;
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
/// use mcp_execution_codegen::progressive::ToolSummary;
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
/// use mcp_execution_codegen::progressive::ToolCategorization;
///
/// let cat = ToolCategorization {
///     category: "issues".to_string(),
///     keywords: vec!["create".to_string(), "issue".to_string(), "new".to_string(), "bug".to_string()],
///     short_description: "Create a new issue in a repository".to_string(),
/// };
///
/// assert_eq!(cat.category, "issues");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCategorization {
    /// Category for tool grouping
    pub category: String,
    /// Keywords for discovery via grep/search
    pub keywords: Vec<String>,
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
/// use mcp_execution_codegen::progressive::{CategoryInfo, ToolSummary};
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
/// The forbidden-char/forbidden-env-name fields are rendered directly from
/// `mcp_execution_core`'s canonical lists so the generated bridge's copies structurally
/// cannot drift from the Rust source of truth — see [`BridgeContext::default`], the only way
/// to construct one, which populates them from `mcp_execution_core::forbidden_chars`/
/// `forbidden_env_names`/`forbidden_env_prefix` rather than leaving them empty. This
/// deliberately does *not* derive `Default`: an empty `forbidden_chars` would render a bridge
/// whose `validateCommandString` accepts every shell metacharacter (fail-open on exactly the
/// check this exists to enforce), so `Default` is hand-written to make "always populated" a
/// property of the type rather than a convention callers must remember to uphold.
///
/// The three fields are private with read-only accessors for the same reason: `pub` fields
/// would let `BridgeContext { forbidden_chars: vec![], .. }` bypass the invariant entirely and
/// still compile, silently reintroducing the fail-open state `Default` exists to prevent.
/// `Deserialize` is intentionally not derived — nothing in this codebase deserializes a
/// `BridgeContext` from external input, and doing so would need to re-validate non-emptiness
/// rather than trust the wire data.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::progressive::BridgeContext;
///
/// let context = BridgeContext::default();
/// assert!(!context.forbidden_chars().is_empty());
/// assert!(context.forbidden_chars().contains(&";".to_string()));
/// assert!(!context.forbidden_env_prefix().is_empty());
/// ```
#[expect(
    clippy::struct_field_names,
    reason = "The shared `forbidden_` prefix mirrors the three distinct mcp_execution_core \
              accessors (forbidden_chars/forbidden_env_names/forbidden_env_prefix) these fields \
              are populated from; dropping it would obscure that correspondence for no clarity \
              gain."
)]
#[derive(Debug, Clone, Serialize)]
pub struct BridgeContext {
    /// Shell metacharacters forbidden in a command or argument string, each pre-escaped for
    /// safe embedding inside a single-quoted TypeScript string literal.
    forbidden_chars: Vec<String>,
    /// Forbidden environment variable names (exact match).
    forbidden_env_names: Vec<String>,
    /// Environment-variable-name prefix rejected regardless of exact match (e.g. `DYLD_`).
    forbidden_env_prefix: String,
}

impl BridgeContext {
    /// Shell metacharacters forbidden in a command or argument string, each pre-escaped for
    /// safe embedding inside a single-quoted TypeScript string literal. Never empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::progressive::BridgeContext;
    ///
    /// assert!(!BridgeContext::default().forbidden_chars().is_empty());
    /// ```
    #[must_use]
    pub fn forbidden_chars(&self) -> &[String] {
        &self.forbidden_chars
    }

    /// Forbidden environment variable names (exact match). Never empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::progressive::BridgeContext;
    ///
    /// assert!(!BridgeContext::default().forbidden_env_names().is_empty());
    /// ```
    #[must_use]
    pub fn forbidden_env_names(&self) -> &[String] {
        &self.forbidden_env_names
    }

    /// Environment-variable-name prefix rejected regardless of exact match (e.g. `DYLD_`).
    /// Never empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::progressive::BridgeContext;
    ///
    /// assert!(!BridgeContext::default().forbidden_env_prefix().is_empty());
    /// ```
    #[must_use]
    pub fn forbidden_env_prefix(&self) -> &str {
        &self.forbidden_env_prefix
    }
}

impl Default for BridgeContext {
    /// Populates the forbidden-char/forbidden-env-name fields directly from
    /// `mcp_execution_core`'s canonical lists, so `BridgeContext::default()` can never render
    /// a bridge with an empty (fail-open) `FORBIDDEN_CHARS`. Each character is passed through
    /// `sanitize_ts_string_literal` (this crate's TS-string-literal escaper) so it renders as
    /// a syntactically valid single-quoted TypeScript string literal regardless of what the
    /// Rust list contains.
    fn default() -> Self {
        Self {
            forbidden_chars: mcp_execution_core::forbidden_chars()
                .iter()
                .map(|c| crate::progressive::generator::sanitize_ts_string_literal(&c.to_string()))
                .collect(),
            forbidden_env_names: mcp_execution_core::forbidden_env_names()
                .iter()
                .map(|&s| s.to_string())
                .collect(),
            forbidden_env_prefix: mcp_execution_core::forbidden_env_prefix().to_string(),
        }
    }
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
            name_literal: "create_issue".to_string(),
            server_id_literal: "github".to_string(),
            typescript_name: "createIssue".to_string(),
            description: "Creates an issue".to_string(),
            input_schema: json!({"type": "object"}),
            properties: vec![],
            category: Some("issues".to_string()),
            keywords: Some("create,issue,new".to_string()),
            short_description: "Create a new issue".to_string(),
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
        let _serialized = serde_json::to_string(&context).unwrap();

        // #221 critique S2: `Default` must never render a fail-open (empty) forbidden-char
        // list, since an empty `FORBIDDEN_CHARS` in the rendered bridge would make
        // `validateCommandString` accept every shell metacharacter.
        assert!(!context.forbidden_chars().is_empty());
        assert!(!context.forbidden_env_names().is_empty());
        assert!(!context.forbidden_env_prefix().is_empty());
    }
}
