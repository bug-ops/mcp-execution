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
/// The forbidden-char/forbidden-env-name/charset-pattern fields are rendered directly from
/// `mcp_execution_core`'s canonical lists so the generated bridge's copies structurally
/// cannot drift from the Rust source of truth — see [`BridgeContext::default`], the only way
/// to construct one, which populates them from `mcp_execution_core::forbidden_chars`/
/// `forbidden_env_names`/`forbidden_env_prefix`/`env_name_charset_pattern` rather than leaving
/// them empty. This deliberately does *not* derive `Default`: an empty `forbidden_chars` would
/// render a bridge whose `validateCommandString` accepts every shell metacharacter, and an
/// empty `env_name_charset_pattern` would render `new RegExp('')`, which matches every string
/// (fail-open on exactly the checks these exist to enforce) — so `Default` is hand-written to
/// make "always populated" a property of the type rather than a convention callers must
/// remember to uphold.
///
/// Those four fields are private with read-only accessors for the same reason: `pub` fields
/// would let `BridgeContext { forbidden_chars: vec![], .. }` bypass the invariant entirely and
/// still compile, silently reintroducing the fail-open state `Default` exists to prevent.
/// `Deserialize` is intentionally not derived — nothing in this codebase deserializes a
/// `BridgeContext` from external input, and doing so would need to re-validate non-emptiness
/// rather than trust the wire data.
///
/// The remaining fields — the denial-of-service size/count ceilings
/// (`mcp_execution_core::MAX_ARG_COUNT` and siblings) and `env_name_charset_desc` (the
/// human-readable charset description used only in a rejection message's text, not in the
/// enforcement regex above) — are plain `pub` fields: unlike an emptied list or pattern, a
/// wrong value here cannot fail open — at worst it makes the rendered bridge reject configs it
/// should accept (a wrong `MAX_*`), or emit a confusing-but-still-rejecting error message (a
/// wrong `env_name_charset_desc`), never silently accept something it shouldn't — so the extra
/// accessor/invariant machinery above would be pure ceremony here.
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
/// assert!(!context.env_name_charset_pattern().is_empty());
/// assert!(!context.env_name_charset_desc.is_empty());
/// assert!(context.max_arg_count > 0);
/// ```
#[derive(Debug, Clone, Serialize)]
pub struct BridgeContext {
    /// Shell metacharacters forbidden in a command or argument string, each pre-escaped for
    /// safe embedding inside a single-quoted TypeScript string literal.
    forbidden_chars: Vec<String>,
    /// Forbidden environment variable names (exact match).
    forbidden_env_names: Vec<String>,
    /// Environment-variable-name prefix rejected regardless of exact match (e.g. `DYLD_`).
    forbidden_env_prefix: String,
    /// POSIX/Windows environment-variable-name identifier charset, as an anchored JavaScript
    /// `RegExp`-compatible pattern source (see `mcp_execution_core::env_name_charset_pattern`),
    /// pre-escaped for safe embedding inside a single-quoted TypeScript string literal — same
    /// treatment as `forbidden_chars`, and for the same reason: an unescaped pattern containing
    /// a `'` or `\` would either break the generated `new RegExp('...')` call or silently change
    /// what it matches.
    env_name_charset_pattern: String,
    /// Human-readable description of the charset above (`mcp_execution_core::env_name_charset_desc`,
    /// e.g. `"[A-Za-z_][A-Za-z0-9_]*"`), pre-escaped like `env_name_charset_pattern` and
    /// rendered into the bridge's own rejection message so that text isn't a second
    /// hand-copied literal alongside the pattern.
    pub env_name_charset_desc: String,
    /// Maximum number of positional arguments (`mcp_execution_core::MAX_ARG_COUNT`).
    pub max_arg_count: usize,
    /// Maximum byte length for a command, argument, env-var name, or header name
    /// (`mcp_execution_core::MAX_ARG_LEN`).
    pub max_arg_len: usize,
    /// Maximum number of environment variables (`mcp_execution_core::MAX_ENV_COUNT`).
    pub max_env_count: usize,
    /// Maximum byte length for a single environment variable value
    /// (`mcp_execution_core::MAX_ENV_VALUE_LEN`).
    pub max_env_value_len: usize,
    /// Maximum byte length for the Http/Sse transport `url`
    /// (`mcp_execution_core::MAX_URL_LEN`).
    pub max_url_len: usize,
    /// Maximum number of HTTP headers (`mcp_execution_core::MAX_HEADER_COUNT`).
    pub max_header_count: usize,
    /// Maximum byte length for a single HTTP header value
    /// (`mcp_execution_core::MAX_HEADER_VALUE_LEN`).
    pub max_header_value_len: usize,
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

    /// POSIX/Windows environment-variable-name identifier charset, as an anchored JavaScript
    /// `RegExp`-compatible pattern source. Never empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_codegen::progressive::BridgeContext;
    ///
    /// assert!(!BridgeContext::default().env_name_charset_pattern().is_empty());
    /// ```
    #[must_use]
    pub fn env_name_charset_pattern(&self) -> &str {
        &self.env_name_charset_pattern
    }
}

impl Default for BridgeContext {
    /// Populates the forbidden-char/forbidden-env-name/charset-pattern fields directly from
    /// `mcp_execution_core`'s canonical lists/constants, so `BridgeContext::default()` can
    /// never render a bridge with an empty (fail-open) `FORBIDDEN_CHARS` or
    /// `ENV_NAME_CHARSET_REGEX`. Each `forbidden_chars` entry and `env_name_charset_pattern`
    /// itself are passed through `sanitize_ts_string_literal` (this crate's TS-string-literal
    /// escaper) so they render as syntactically valid single-quoted TypeScript string literals
    /// regardless of what the Rust source contains — critique #471/#467 S2: without this, a
    /// future edit introducing a `'`/`\` into the Rust pattern would either break the generated
    /// `new RegExp('...')` call or silently change what it matches.
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
            env_name_charset_pattern: crate::progressive::generator::sanitize_ts_string_literal(
                mcp_execution_core::env_name_charset_pattern(),
            ),
            env_name_charset_desc: crate::progressive::generator::sanitize_ts_string_literal(
                mcp_execution_core::env_name_charset_desc(),
            ),
            max_arg_count: mcp_execution_core::MAX_ARG_COUNT,
            max_arg_len: mcp_execution_core::MAX_ARG_LEN,
            max_env_count: mcp_execution_core::MAX_ENV_COUNT,
            max_env_value_len: mcp_execution_core::MAX_ENV_VALUE_LEN,
            max_url_len: mcp_execution_core::MAX_URL_LEN,
            max_header_count: mcp_execution_core::MAX_HEADER_COUNT,
            max_header_value_len: mcp_execution_core::MAX_HEADER_VALUE_LEN,
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
        // `validateCommandString` accept every shell metacharacter. Same reasoning applies to
        // an empty `env_name_charset_pattern`: `new RegExp('')` matches every string.
        assert!(!context.forbidden_chars().is_empty());
        assert!(!context.forbidden_env_names().is_empty());
        assert!(!context.forbidden_env_prefix().is_empty());
        assert!(!context.env_name_charset_pattern().is_empty());

        // #471: the DoS size/count ceilings must be populated from mcp_execution_core, not
        // left at zero (which would reject every config, silently breaking every generated
        // server rather than failing open — a different but still real correctness bug).
        assert_eq!(context.max_arg_count, mcp_execution_core::MAX_ARG_COUNT);
        assert_eq!(context.max_arg_len, mcp_execution_core::MAX_ARG_LEN);
        assert_eq!(context.max_env_count, mcp_execution_core::MAX_ENV_COUNT);
        assert_eq!(
            context.max_env_value_len,
            mcp_execution_core::MAX_ENV_VALUE_LEN
        );
        assert_eq!(context.max_url_len, mcp_execution_core::MAX_URL_LEN);
        assert_eq!(
            context.max_header_count,
            mcp_execution_core::MAX_HEADER_COUNT
        );
        assert_eq!(
            context.max_header_value_len,
            mcp_execution_core::MAX_HEADER_VALUE_LEN
        );
        // Escaping is a no-op on this quote/backslash-free pattern, so the sanitized copy
        // still equals the raw Rust source of truth.
        assert_eq!(
            context.env_name_charset_pattern(),
            mcp_execution_core::env_name_charset_pattern()
        );
        assert_eq!(
            context.env_name_charset_desc,
            mcp_execution_core::env_name_charset_desc()
        );
    }
}
