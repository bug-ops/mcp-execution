//! Types for progressive loading code generation.
//!
//! Defines data structures used during progressive code generation,
//! where each tool is generated as a separate file.

use crate::package::observed::{
    OBSERVED_SHAPE_MAX_DEPTH, OBSERVED_SHAPE_MAX_KEY_LEN, OBSERVED_SHAPE_MAX_KEYS,
    OBSERVED_SHAPE_MAX_LINE_BYTES,
};
use mcp_execution_core::helper_protocol::{
    CLI_PATH_ENV, HELPER_CHANNEL_KEY, HELPER_ENV, HELPER_PROTOCOL_VERSION, HELPER_TOKEN_ENV,
    MAX_HELPER_LINE_BYTES, RUN_MARKER_ENV, ResultSource,
};
use mcp_execution_core::untrusted::BARE_IDENTIFIER_PATTERN;
use serde::{Deserialize, Serialize};

/// Context for rendering a single tool template.
///
/// Contains all data needed to generate one tool file in the
/// progressive loading pattern.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::package::ToolContext;
/// use mcp_execution_core::helper_protocol::ResultSource;
///
/// let context = ToolContext {
///     server_id: "github".to_string(),
///     name: "create_issue".to_string(),
///     name_literal: "create_issue".to_string(),
///     server_id_literal: "github".to_string(),
///     typescript_name: "createIssue".to_string(),
///     description: "Creates a new issue".to_string(),
///     params_description: None,
///     params_type: "{ title: string }".to_string(),
///     result_type: "unknown | undefined".to_string(),
///     result_source: ResultSource::Content,
///     observed_hint: None,
///     category: Some("issues".to_string()),
///     keywords: Some("create,issue,new,bug".to_string()),
///     short_description: "Create a new issue".to_string(),
///     annotation_tags: vec![],
/// };
///
/// assert_eq!(context.server_id, "github");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[doc(hidden)]
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
    /// Description of the whole input schema, sanitized for a `JSDoc` comment
    pub params_description: Option<String>,
    /// TypeScript type of the parameters, in the multi-line declaration form
    pub params_type: String,
    /// TypeScript type of the result, ending in `| undefined` for an empty success
    pub result_type: String,
    /// Which part of the tool result the generated function asks the bridge for
    pub result_source: ResultSource,
    /// `JSDoc` text describing a result shape observed in a past call, for tools that declare
    /// no output schema
    pub observed_hint: Option<String>,
    /// Optional category for tool grouping
    pub category: Option<String>,
    /// Optional keywords for discovery via grep/search
    pub keywords: Option<String>,
    /// Short description for header comment. Always populated: `PackageGenerator`'s only
    /// constructor falls back to `description` when no categorization short description is
    /// available, so `None` is not a state this type can represent.
    pub short_description: String,
    /// `JSDoc` tag lines for the annotations the server stated, empty when none were
    pub annotation_tags: Vec<String>,
}

/// Context for rendering the index.ts template.
///
/// Contains server-level metadata and list of all tools.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::package::IndexContext;
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
#[doc(hidden)]
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
/// use mcp_execution_codegen::package::ToolSummary;
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
/// use mcp_execution_codegen::package::ToolCategorization;
///
/// let cat = ToolCategorization::new("issues".to_string(), vec!["create".to_string(), "issue".to_string(), "new".to_string(), "bug".to_string()], "Create a new issue in a repository".to_string());
///
/// assert_eq!(cat.category, "issues");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
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
/// use mcp_execution_codegen::package::{CategoryInfo, ToolSummary};
///
/// let category = CategoryInfo::new("issues".to_string(), vec![
///         ToolSummary {
///             typescript_name: "createIssue".to_string(),
///             description: "Creates a new issue".to_string(),
///             category: Some("issues".to_string()),
///             keywords: Some("create,issue".to_string()),
///             short_description: Some("Create issue".to_string()),
///         },
///     ]);
///
/// assert_eq!(category.name, "issues");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct CategoryInfo {
    /// Category name
    pub name: String,
    /// Tools in this category
    pub tools: Vec<ToolSummary>,
}

impl ToolCategorization {
    /// Creates a categorization from its category, keywords and short description.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::package::ToolCategorization;
    ///
    /// let categorization = ToolCategorization::new(
    ///     "issues".to_string(),
    ///     vec!["create".to_string()],
    ///     "Create an issue".to_string(),
    /// );
    /// assert_eq!(categorization.category, "issues");
    /// ```
    #[must_use]
    pub const fn new(category: String, keywords: Vec<String>, short_description: String) -> Self {
        Self {
            category,
            keywords,
            short_description,
        }
    }
}

impl CategoryInfo {
    /// Creates a category from its name and tools.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::package::CategoryInfo;
    ///
    /// let category = CategoryInfo::new("issues".to_string(), vec![]);
    /// assert!(category.tools.is_empty());
    /// ```
    #[must_use]
    pub const fn new(name: String, tools: Vec<ToolSummary>) -> Self {
        Self { name, tools }
    }
}

/// Context for rendering the runtime bridge template.
///
/// The bridge is a thin client of the helper protocol, so everything it must agree on with the
/// Rust helper is rendered from [`mcp_execution_core::helper_protocol`]: the protocol version,
/// the line bound and the environment-variable names. The observed-shape recorder's limits come
/// from this crate's `observed` constants. Nothing is hand-copied into the template, so a limit
/// cannot drift between the two ends.
///
/// `identifier_pattern` is private with an accessor and [`Default`] is the only constructor:
/// an empty pattern would render `new RegExp('')`, which matches every key, so "always populated"
/// is a property of the type rather than a convention.
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::package::BridgeContext;
///
/// let context = BridgeContext::default();
/// assert_eq!(context.helper_protocol_version, 1);
/// assert!(context.max_helper_line_bytes > 0);
/// assert_ne!(context.identifier_pattern(), "");
/// ```
#[derive(Debug, Clone, Serialize)]
#[doc(hidden)]
pub struct BridgeContext {
    /// Helper protocol version the bridge speaks
    /// ([`mcp_execution_core::helper_protocol::HELPER_PROTOCOL_VERSION`]).
    pub helper_protocol_version: u32,
    /// Longest protocol line in either direction, in bytes
    /// ([`mcp_execution_core::helper_protocol::MAX_HELPER_LINE_BYTES`]).
    pub max_helper_line_bytes: usize,
    /// Environment variable that announces a runner channel
    /// ([`mcp_execution_core::helper_protocol::HELPER_ENV`]).
    pub helper_env: &'static str,
    /// Environment variable carrying the runner channel's token
    /// ([`mcp_execution_core::helper_protocol::HELPER_TOKEN_ENV`]).
    pub helper_token_env: &'static str,
    /// Non-secret marker inherited by every process under `run`
    /// ([`mcp_execution_core::helper_protocol::RUN_MARKER_ENV`]).
    pub run_marker_env: &'static str,
    /// Environment variable naming the binary spawned as the helper
    /// ([`mcp_execution_core::helper_protocol::CLI_PATH_ENV`]).
    pub cli_path_env: &'static str,
    /// Global key under which the `run` preload registers the helper channel
    /// ([`mcp_execution_core::helper_protocol::HELPER_CHANNEL_KEY`]).
    pub helper_channel_key: &'static str,
    /// Maximum nesting depth of a recorded observed shape.
    pub observed_shape_max_depth: usize,
    /// Maximum number of keys an object may have to be recorded key by key.
    pub observed_shape_max_keys: usize,
    /// Maximum length in characters of a recorded object key.
    pub observed_shape_max_key_len: usize,
    /// Maximum size in bytes of one recorded observed-shape line.
    pub observed_shape_max_line_bytes: usize,
    /// Anchored `RegExp` pattern source for the identifier keys a recorded shape may keep,
    /// pre-escaped for embedding inside a single-quoted TypeScript string literal.
    identifier_pattern: String,
}

impl BridgeContext {
    /// Anchored `RegExp` pattern source for the identifier keys a recorded observed shape may
    /// keep. Never empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::package::BridgeContext;
    ///
    /// assert!(!BridgeContext::default().identifier_pattern().is_empty());
    /// ```
    #[must_use]
    pub fn identifier_pattern(&self) -> &str {
        &self.identifier_pattern
    }
}

impl Default for BridgeContext {
    /// Populates every field from its canonical source constant.
    fn default() -> Self {
        Self {
            helper_protocol_version: HELPER_PROTOCOL_VERSION,
            max_helper_line_bytes: MAX_HELPER_LINE_BYTES,
            helper_env: HELPER_ENV,
            helper_token_env: HELPER_TOKEN_ENV,
            run_marker_env: RUN_MARKER_ENV,
            cli_path_env: CLI_PATH_ENV,
            helper_channel_key: HELPER_CHANNEL_KEY,
            observed_shape_max_depth: OBSERVED_SHAPE_MAX_DEPTH,
            observed_shape_max_keys: OBSERVED_SHAPE_MAX_KEYS,
            observed_shape_max_key_len: OBSERVED_SHAPE_MAX_KEY_LEN,
            observed_shape_max_line_bytes: OBSERVED_SHAPE_MAX_LINE_BYTES,
            identifier_pattern: crate::package::generator::sanitize_ts_string_literal(
                BARE_IDENTIFIER_PATTERN,
            ),
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
            params_description: None,
            params_type: "{}".to_string(),
            result_type: "unknown | undefined".to_string(),
            result_source: ResultSource::Content,
            observed_hint: None,
            category: Some("issues".to_string()),
            keywords: Some("create,issue,new".to_string()),
            short_description: "Create a new issue".to_string(),
            annotation_tags: vec![],
        };

        assert_eq!(context.server_id, "github");
        assert_eq!(context.name, "create_issue");
        assert_eq!(context.typescript_name, "createIssue");
        assert_eq!(context.category, Some("issues".to_string()));
        assert_eq!(context.keywords, Some("create,issue,new".to_string()));
    }

    #[test]
    fn test_result_source_serializes_as_template_literal() {
        assert_eq!(
            serde_json::to_value(ResultSource::Content).unwrap(),
            json!("content")
        );
        assert_eq!(
            serde_json::to_value(ResultSource::Structured).unwrap(),
            json!("structured")
        );
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

        assert_eq!(
            context.helper_protocol_version, HELPER_PROTOCOL_VERSION,
            "the protocol version is rendered from core"
        );
        assert_eq!(context.max_helper_line_bytes, MAX_HELPER_LINE_BYTES);
        assert_eq!(context.helper_env, HELPER_ENV);
        assert_eq!(context.helper_token_env, HELPER_TOKEN_ENV);
        assert_eq!(context.run_marker_env, RUN_MARKER_ENV);
        assert_eq!(context.cli_path_env, CLI_PATH_ENV);
        assert_eq!(context.helper_channel_key, HELPER_CHANNEL_KEY);
        // An empty pattern would render `new RegExp('')`, which matches every key.
        assert_ne!(context.identifier_pattern(), "");
    }
}
