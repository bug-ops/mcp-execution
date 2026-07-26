//! Type definitions for MCP server tools.
//!
//! This module defines all parameter and result types for the three main tools:
//! - `introspect_server`: Connect to and introspect an MCP server
//! - `save_categorized_tools`: Generate TypeScript files with categorization
//! - `list_generated_servers`: List all servers with generated files

use crate::clock::Clock;
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
/// This type only ever builds a stdio [`ServerConfig`] (see
/// `GeneratorService::introspect_server`, which calls `ServerConfig::builder().command(...)`
/// and never sets a transport of `Http` or `Sse`). It must never gain a field capable of
/// setting `ServerConfig`'s transport to `Http`/`Sse` (e.g. `url`, `http`, `sse`, `headers`)
/// without SSRF allowlisting logic added alongside it: `ServerConfig::url`
/// (`crates/mcp-core/src/server_config.rs`) documents that this crate does not apply SSRF
/// allowlisting itself and expects a server-context embedder - which is exactly what
/// `mcp-execution-server` is - to add its own before connecting. `tests::introspect_server_params_shape_is_pinned`
/// pins the current field set so a silent addition fails to compile instead of merely
/// widening the attack surface unnoticed.
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
///     connect_timeout_secs: None,
///     discover_timeout_secs: None,
/// };
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema)]
pub struct IntrospectServerParams {
    /// Unique identifier for the server (e.g., "github", "filesystem").
    ///
    /// Must be 1-64 lowercase letters, digits, or hyphens (see `validate_server_id`'s
    /// `MAX_SERVER_ID_LENGTH`, mirrored here as a literal since schemars attributes cannot
    /// reference a `const`).
    #[schemars(length(max = 64), regex(pattern = r"^[a-z0-9-]+$"))]
    pub server_id: String,

    /// Command to start the server (e.g., "npx", "docker").
    ///
    /// Capped at `mcp_execution_core::MAX_ARG_LEN` (4096 bytes) at runtime; mirrored here as a
    /// literal since schemars attributes cannot reference a `const`.
    #[schemars(length(max = 4096))]
    pub command: String,

    /// Arguments to pass to the command.
    ///
    /// Capped at `mcp_execution_core::MAX_ARG_COUNT` (256) entries at runtime (enforced when
    /// this becomes part of the `ServerConfig` built from these params); mirrored here as a
    /// literal since schemars attributes cannot reference a `const`.
    #[serde(default)]
    #[schemars(length(max = 256))]
    pub args: Vec<String>,

    /// Environment variables for the server process.
    ///
    /// Capped at `mcp_execution_core::MAX_ENV_COUNT` (256) entries and
    /// `mcp_execution_core::MAX_ENV_VALUE_LEN` (32KB) per value at runtime. No schemars
    /// attribute is set here: schemars' `length` validation only emits `minProperties`/
    /// `maxProperties` for `object`-typed schemas via the `map`/`set` traits it does not yet
    /// support for derived struct fields, so a map-size constraint would be a silent no-op.
    #[serde(default)]
    pub env: HashMap<String, String>,

    /// Custom output subdirectory, relative to `~/.claude/servers/{server_id}/`
    /// (default: `~/.claude/servers/{server_id}` itself). Confined to that
    /// directory: an absolute path, a `..` component, or a path that escapes it
    /// via a symlink is rejected.
    pub output_dir: Option<PathBuf>,

    /// Connection (handshake) timeout in seconds, overriding the 30-second
    /// default when set.
    #[serde(default)]
    pub connect_timeout_secs: Option<u64>,

    /// Tool discovery timeout in seconds, overriding the 30-second default
    /// when set.
    #[serde(default)]
    pub discover_timeout_secs: Option<u64>,
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
    pub tools: Vec<IntrospectedToolSummary>,

    /// Session ID for `save_categorized_tools` call
    pub session_id: Uuid,

    /// Session expiration time (ISO 8601)
    pub expires_at: DateTime<Utc>,
}

/// Summary of an introspected tool, returned to Claude for categorization.
///
/// Includes the tool name, description, and parameter names to help
/// Claude understand the tool's purpose and assign appropriate categories.
#[derive(Debug, Clone, Serialize, JsonSchema)]
pub struct IntrospectedToolSummary {
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

    /// Tools with Claude's categorization.
    ///
    /// The session-specific cap enforced at runtime is `min(introspected tool count,
    /// MAX_TOOL_FILES)`, tighter than the flat limit below in nearly all cases; `MAX_TOOL_FILES`
    /// (`mcp_execution_skill`, 500) is still the absolute ceiling regardless of session, so it
    /// is what's mirrored here as a literal (schemars attributes cannot reference a `const`).
    #[schemars(length(max = 500))]
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
    /// Original tool name (must match introspected tool).
    ///
    /// Capped at `MAX_CATEGORIZED_TOOL_NAME_LEN` (128 bytes) at runtime; mirrored here as a
    /// literal since schemars attributes cannot reference a `const`. Note: JSON Schema's
    /// `maxLength` counts Unicode code points, not bytes, so the two bounds only coincide
    /// exactly for ASCII input — for legitimate multi-byte UTF-8 text, the runtime byte check
    /// can reject a string the declared schema would still accept (never the reverse), since
    /// bytes-per-char >= 1 (issue #198 M2).
    #[schemars(length(max = 128))]
    pub name: String,

    /// Category assigned by Claude (e.g., "issues", "repos", "users").
    ///
    /// Capped at `MAX_CATEGORY_LEN` (100 bytes) at runtime; mirrored here as a literal since
    /// schemars attributes cannot reference a `const`. See [`CategorizedTool::name`]'s doc
    /// comment for the bytes-vs-characters caveat.
    #[schemars(length(max = 100))]
    pub category: String,

    /// Comma-separated keywords for discovery.
    ///
    /// Capped at `MAX_KEYWORDS_LEN` (500 bytes) at runtime; mirrored here as a literal since
    /// schemars attributes cannot reference a `const`. See [`CategorizedTool::name`]'s doc
    /// comment for the bytes-vs-characters caveat.
    #[schemars(length(max = 500))]
    pub keywords: String,

    /// Concise description (max 80 chars) for header comment.
    ///
    /// Capped at `MAX_SHORT_DESCRIPTION_LEN` (320 bytes — 4x the 80-char target, headroom for
    /// multi-byte UTF-8) at runtime; mirrored here as a literal since schemars attributes
    /// cannot reference a `const`. See [`CategorizedTool::name`]'s doc comment for the
    /// bytes-vs-characters caveat.
    #[schemars(length(max = 320))]
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
    /// Base directory to scan, relative to `~/.claude/servers`
    /// (default: `~/.claude/servers` itself). Confined to that directory: an
    /// absolute path, a `..` component, or a path that escapes it via a
    /// symlink is rejected.
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

    /// Caller-supplied `output_dir` override from `introspect_server`, exactly as received
    /// (already validated as syntactically safe - relative, no `..` - but not yet confinement-
    /// checked against the filesystem). `None` when the default `{server_id}` directory was
    /// used.
    ///
    /// `save_categorized_tools` derives the real export target fresh from this and
    /// [`Self::server_id`] immediately before writing anything - see
    /// `crate::output_dir::resolve_output_dir`.
    pub output_dir_override: Option<PathBuf>,

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
    /// The session's `created_at`/`expires_at` are derived from `clock.now()`,
    /// so tests can inject a fake clock instead of rewinding `expires_at`
    /// after construction. Production callers should pass [`SystemClock`](crate::clock::SystemClock).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::types::PendingGeneration;
    /// use mcp_execution_server::clock::SystemClock;
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
    ///     .build()
    ///     .unwrap();
    /// let pending = PendingGeneration::new(
    ///     server_id,
    ///     server_info,
    ///     config,
    ///     None,
    ///     &SystemClock,
    /// );
    /// # }
    /// ```
    #[must_use]
    pub fn new(
        server_id: ServerId,
        server_info: ServerInfo,
        config: ServerConfig,
        output_dir_override: Option<PathBuf>,
        clock: &dyn Clock,
    ) -> Self {
        let now = clock.now();
        Self {
            server_id,
            server_info,
            config,
            output_dir_override,
            created_at: now,
            expires_at: now + chrono::Duration::minutes(Self::DEFAULT_TIMEOUT_MINUTES),
        }
    }

    /// Checks if this session has expired, using `clock.now()` as the current time.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_server::types::PendingGeneration;
    /// use mcp_execution_server::clock::SystemClock;
    /// # use mcp_execution_core::{ServerId, ServerConfig};
    /// # use mcp_execution_introspector::ServerInfo;
    ///
    /// # fn example(server_info: ServerInfo) {
    /// let pending = PendingGeneration::new(
    ///     ServerId::new("test"),
    ///     server_info,
    ///     ServerConfig::builder().command("echo".to_string()).build().unwrap(),
    ///     None,
    ///     &SystemClock,
    /// );
    ///
    /// assert!(!pending.is_expired(&SystemClock));
    /// # }
    /// ```
    #[must_use]
    pub fn is_expired(&self, clock: &dyn Clock) -> bool {
        clock.now() > self.expires_at
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::clock::{SystemClock, TestClock};

    #[test]
    fn test_pending_generation_not_expired() {
        let pending = create_test_pending();
        assert!(!pending.is_expired(&SystemClock));
    }

    #[test]
    fn test_pending_generation_not_expired_at_exact_boundary() {
        let clock = TestClock::new(Utc::now());
        let pending = create_test_pending_with_clock(&clock);

        // `is_expired` uses strict `>`, so the exact expiry instant is not expired.
        clock.advance(chrono::Duration::minutes(
            PendingGeneration::DEFAULT_TIMEOUT_MINUTES,
        ));
        assert!(!pending.is_expired(&clock));
    }

    #[test]
    fn test_pending_generation_not_expired_one_second_before_boundary() {
        let clock = TestClock::new(Utc::now());
        let pending = create_test_pending_with_clock(&clock);

        clock.advance(
            chrono::Duration::minutes(PendingGeneration::DEFAULT_TIMEOUT_MINUTES)
                - chrono::Duration::seconds(1),
        );
        assert!(!pending.is_expired(&clock));
    }

    #[test]
    fn test_pending_generation_expired_one_second_after_boundary() {
        let clock = TestClock::new(Utc::now());
        let pending = create_test_pending_with_clock(&clock);

        clock.advance(
            chrono::Duration::minutes(PendingGeneration::DEFAULT_TIMEOUT_MINUTES)
                + chrono::Duration::seconds(1),
        );
        assert!(pending.is_expired(&clock));
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

    /// Regression guard for issue #209: `IntrospectServerParams` must never gain a field
    /// capable of setting `ServerConfig`'s transport to `Http`/`Sse` (e.g. `url`, `http`,
    /// `sse`, `headers`) without SSRF allowlisting logic added alongside it. Destructuring
    /// with a struct pattern that names every current field, with no `..` rest pattern, means
    /// Rust's exhaustiveness check fails this file to compile the moment a field is added or
    /// removed - a stronger guarantee than a runtime assertion could give.
    #[test]
    fn introspect_server_params_shape_is_pinned() {
        let params = IntrospectServerParams {
            server_id: "test".to_string(),
            command: "echo".to_string(),
            args: vec![],
            env: HashMap::new(),
            output_dir: None,
            connect_timeout_secs: None,
            discover_timeout_secs: None,
        };

        // Fields that must never appear on this type without SSRF allowlisting logic: `url`,
        // `http`, `sse`, `headers`.
        let IntrospectServerParams {
            server_id: _,
            command: _,
            args: _,
            env: _,
            output_dir: _,
            connect_timeout_secs: _,
            discover_timeout_secs: _,
        } = params;
    }

    // ── schemars bounds (issue #205) ──────────────────────────────────────

    #[test]
    fn test_categorized_tool_schema_declares_length_bounds() {
        use crate::service::{
            MAX_CATEGORIZED_TOOL_NAME_LEN, MAX_CATEGORY_LEN, MAX_KEYWORDS_LEN,
            MAX_SHORT_DESCRIPTION_LEN,
        };

        let schema = schemars::schema_for!(CategorizedTool);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        // Asserted against the real runtime constants (not hardcoded literals), so bumping one
        // without updating the matching `#[schemars(length(max = ..))]` literal fails this test
        // instead of leaving the declared schema silently stale (issue #198 S3).
        assert_eq!(props["name"]["maxLength"], MAX_CATEGORIZED_TOOL_NAME_LEN);
        assert_eq!(props["category"]["maxLength"], MAX_CATEGORY_LEN);
        assert_eq!(props["keywords"]["maxLength"], MAX_KEYWORDS_LEN);
        assert_eq!(
            props["short_description"]["maxLength"],
            MAX_SHORT_DESCRIPTION_LEN
        );
    }

    #[test]
    fn test_save_categorized_tools_params_schema_declares_vec_length_bound() {
        let schema = schemars::schema_for!(SaveCategorizedToolsParams);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        assert_eq!(
            props["categorized_tools"]["maxItems"],
            mcp_execution_skill::MAX_TOOL_FILES
        );
    }

    #[test]
    fn test_introspect_server_params_schema_declares_server_id_bounds() {
        let schema = schemars::schema_for!(IntrospectServerParams);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        assert_eq!(
            props["server_id"]["maxLength"],
            mcp_execution_skill::MAX_SERVER_ID_LENGTH
        );
        assert_eq!(props["server_id"]["pattern"], "^[a-z0-9-]+$");
        assert_eq!(props["args"]["maxItems"], mcp_execution_core::MAX_ARG_COUNT);
        assert_eq!(
            props["command"]["maxLength"],
            mcp_execution_core::MAX_ARG_LEN
        );
    }

    // Test helpers
    fn create_test_pending() -> PendingGeneration {
        create_test_pending_with_clock(&SystemClock)
    }

    fn create_test_pending_with_clock(clock: &dyn Clock) -> PendingGeneration {
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
        let config = ServerConfig::builder()
            .command("echo".to_string())
            .build()
            .unwrap();

        PendingGeneration::new(server_id, server_info, config, None, clock)
    }
}
