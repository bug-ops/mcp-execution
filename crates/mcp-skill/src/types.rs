//! Type definitions for skill generation.
//!
//! This module defines all parameter and result types for skill generation:
//! - `GenerateSkillParams`: Parameters for generating a skill
//! - `GenerateSkillResult`: Result from skill generation
//! - `SaveSkillParams`: Parameters for saving a skill
//! - `SaveSkillResult`: Result from saving a skill

use mcp_execution_core::ServerSlug;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Maximum `server_id` length (denial-of-service protection).
///
/// Re-exported from `mcp_execution_core`, the authoritative owner of the server id slug
/// invariant (see [`ServerSlug`]). `pub` (rather than private) so downstream crates'
/// schemars drift-guard tests — e.g. `mcp-execution-server`'s, for
/// `IntrospectServerParams::server_id` — can assert the declared schema length against this
/// real constant instead of a hardcoded literal (issue #198 S3).
pub use mcp_execution_core::MAX_SERVER_ID_LENGTH;

/// Maximum `skill_name` length, in `char`s.
///
/// The Agent Skills format limits `name` to 64 characters and requires it to equal the skill's
/// directory name; the default name is the `server_id` (also at most 64), so the default always
/// fits. Counted in `char`s to match `GenerateSkillParams::skill_name`'s
/// `#[schemars(length(max = ..))]` annotation, since JSON Schema's `maxLength` counts Unicode
/// code points.
pub const MAX_SKILL_NAME_LENGTH: usize = MAX_SERVER_ID_LENGTH;

/// Maximum length, in `char`s, of a skill `description` (the Agent Skills format limit).
pub const MAX_SKILL_DESCRIPTION_LENGTH: usize = 1024;

/// Maximum length, in `char`s, of a single `use_case_hints` entry.
///
/// Re-exported from [`mcp_execution_core::untrusted::MAX_UNTRUSTED_FIELD_LEN`], the same cap
/// every hint is truncated to by `sanitize_untrusted_text` in `context::build_generation_prompt`
/// before it reaches the LLM-facing prompt, so the declared JSON Schema bound and the runtime
/// truncation agree. The `#[schemars(inner(length(max = ..)))]` literal below mirrors this
/// value (schemars attributes cannot reference a `const`) — see
/// `test_generate_skill_params_schema_declares_use_case_hint_bound` for the drift guard
/// (issue #429).
pub const MAX_USE_CASE_HINT_LENGTH: usize = mcp_execution_core::untrusted::MAX_UNTRUSTED_FIELD_LEN;

/// Maximum number of `use_case_hints` entries.
///
/// A per-entry length bound alone does not stop a caller from supplying an unbounded number of
/// entries (e.g. 10,000 hints at [`MAX_USE_CASE_HINT_LENGTH`] chars each is a ~5 MB prompt) —
/// this bounds the collection itself, mirroring `MAX_TOOL_FILES`'s role as this crate's
/// existing precedent for a resource-exhaustion (CWE-400) bound on a caller-controlled
/// collection (critic finding M1). `context::build_generation_prompt` truncates to this many
/// entries at runtime (dropping the excess, not erroring, since a caller supplying too many
/// hints is not attacker behavior worth failing the whole request over) and the
/// `#[schemars(length(max = ..)))]` literal on `GenerateSkillParams::use_case_hints` mirrors
/// this value for the declared schema.
pub const MAX_USE_CASE_HINTS: usize = 20;

// ============================================================================
// generate_skill types
// ============================================================================

/// Parameters for generating a skill.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::types::GenerateSkillParams;
///
/// let params: GenerateSkillParams =
///     serde_json::from_value(serde_json::json!({ "server_id": "github" })).unwrap();
/// assert_eq!(params.server_id.as_str(), "github");
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct GenerateSkillParams {
    /// Server identifier (e.g., "github").
    ///
    /// Must be 1-64 lowercase letters, digits, or hyphens.
    pub server_id: ServerSlug,

    /// Base directory for generated servers.
    ///
    /// Default: `~/.claude/servers`
    pub servers_dir: Option<PathBuf>,

    /// Custom skill name.
    ///
    /// Default: the `server_id`, which is also the skill's directory name. A supplied name must
    /// equal that directory name (the `server_id`) and follow the Agent Skills `name` rule:
    /// 1-64 lowercase letters, digits and hyphens (see `SkillName`'s `MAX_SKILL_NAME_LENGTH`,
    /// mirrored here as a literal since schemars attributes cannot reference a `const`). Any
    /// other name is rejected with `invalid_params`. The name is embedded in both the
    /// response's `skill_name` field and its `generation_prompt` (the `**Skill Name**` line the
    /// LLM is instructed to use in `SKILL.md`'s frontmatter).
    #[schemars(length(max = 64), regex(pattern = r"^[a-z0-9-]+$"))]
    pub skill_name: Option<String>,

    /// Additional context about intended use cases.
    ///
    /// Helps generate more relevant documentation. Each hint is capped at
    /// [`MAX_USE_CASE_HINT_LENGTH`] characters and the collection itself at
    /// [`MAX_USE_CASE_HINTS`] entries, both mirrored below as literals since schemars
    /// attributes cannot reference a `const`.
    #[schemars(length(max = 20), inner(length(max = 500)))]
    pub use_case_hints: Option<Vec<String>>,
}

/// Result from `generate_skill` tool.
///
/// Contains all context Claude needs to generate optimal SKILL.md content.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::ServerSlug;
/// use mcp_execution_skill::build_skill_context;
///
/// let server_id = ServerSlug::new("github").unwrap();
/// let result = build_skill_context(&server_id, &[], None, None);
///
/// assert_eq!(result.server_id.as_str(), "github");
/// assert_eq!(result.tool_count, 0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct GenerateSkillResult {
    /// Server identifier.
    pub server_id: ServerSlug,

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

    /// Suggested output path for the skill file, for display purposes only.
    ///
    /// Shaped like `~/.claude/skills/{server_id}/SKILL.md` (with a literal, unexpanded `~`) —
    /// this shows where the file will land under its *default* location, but is never validated
    /// as a real filesystem path, and — unlike `SaveSkillParams`/`SaveSkillResult`, which share
    /// real filesystem-path semantics and so keep the `output_path` name — this field is named
    /// `default_output_path_hint` specifically so it cannot be mistaken for one of those (issue
    /// #436). **Do not** pass this value as `save_skill`'s `output_path` parameter: that
    /// parameter has entirely different semantics (a bare relative path with no `~`, resolved
    /// under `base_dir/{server_id}` — see [`SaveSkillParams::output_path`]) and will reject a
    /// `~`-containing value (issue #434). Omit `save_skill`'s `output_path` entirely to use its
    /// own default.
    pub default_output_path_hint: String,

    /// Non-fatal drift/degradation warnings. Two independent sources feed this field, both
    /// additive rather than either overwriting the other: [`crate::build_skill_context`] seeds
    /// it with any `use_case_hints` sanitization warnings (a hint dropped past
    /// [`MAX_USE_CASE_HINTS`] or truncated past [`MAX_USE_CASE_HINT_LENGTH`], issue #473), and
    /// both callers (`mcp-cli`'s `skill` command, `mcp-server`'s `generate_skill` tool) extend
    /// it with `ScanResult::warnings` (e.g. `.ts` files on disk excluded from
    /// `categories`/`tool_count` because `_meta.json` has no matching entry for them) —
    /// `build_skill_context` only sees already-scanned `tools`, not the drift detected while
    /// scanning, so it cannot populate that half itself. Empty when neither source has anything
    /// to report.
    #[serde(default)]
    pub warnings: Vec<String>,

    /// Sanitized, capped `use_case_hints` rendered as SKILL.md's "Use Cases" section by
    /// [`crate::render_skill_md`]. Empty when no hints were supplied. `#[serde(default)]`
    /// so a value serialized before this field existed still deserializes (issue #473).
    #[serde(default)]
    pub use_case_hints: Vec<String>,
}

/// A category of tools for the skill.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::types::SkillCategory;
///
/// let category: SkillCategory = serde_json::from_value(serde_json::json!({
///     "name": "issues", "display_name": "Issues", "tools": [],
/// }))
/// .unwrap();
/// assert_eq!(category.display_name, "Issues");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct SkillCategory {
    /// Category name (e.g., "issues", "repositories").
    pub name: String,

    /// Human-readable display name.
    pub display_name: String,

    /// Tools in this category.
    pub tools: Vec<SkillTool>,
}

/// Tool information for skill generation.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::types::SkillTool;
///
/// let tool: SkillTool = serde_json::from_value(serde_json::json!({
///     "name": "create_issue",
///     "typescript_name": "createIssue",
///     "description": "Create a new issue",
///     "keywords": ["create", "issue"],
///     "required_params": ["title"],
///     "optional_params": ["body"],
/// }))
/// .unwrap();
/// assert_eq!(tool.name, "create_issue");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
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
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::types::ToolExample;
///
/// let example: ToolExample = serde_json::from_value(serde_json::json!({
///     "tool_name": "create_issue",
///     "description": "Create a new issue",
///     "cli_command": "node ~/.claude/servers/github/createIssue.ts '{}'",
///     "params_json": "{}",
/// }))
/// .unwrap();
/// assert_eq!(example.tool_name, "create_issue");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
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
/// use mcp_execution_skill::types::SaveSkillParams;
///
/// let params: SaveSkillParams = serde_json::from_value(serde_json::json!({
///     "server_id": "github",
///     "content": "---\nname: github\n---\n# GitHub",
/// }))
/// .unwrap();
/// assert_eq!(params.server_id.as_str(), "github");
/// ```
#[derive(Debug, Clone, Deserialize, JsonSchema)]
#[non_exhaustive]
pub struct SaveSkillParams {
    /// Server identifier.
    ///
    /// Must be 1-64 lowercase letters, digits, or hyphens.
    pub server_id: ServerSlug,

    /// SKILL.md content (markdown with YAML frontmatter).
    ///
    /// Capped at 100KB (`MAX_SKILL_CONTENT_SIZE` in `mcp_execution_server::service`) at
    /// runtime; mirrored here as a literal since schemars attributes require literals and
    /// this crate does not depend on `mcp-execution-server` (the dependency runs the other
    /// way). Note: JSON Schema's `maxLength` counts Unicode code points, not bytes, so the two
    /// bounds only coincide exactly for ASCII input — for legitimate multi-byte UTF-8 content,
    /// the runtime byte check can reject content the declared schema would still accept
    /// (never the reverse), since bytes-per-char >= 1 (issue #198 M2).
    #[schemars(length(max = 102_400))]
    pub content: String,

    /// Custom output path.
    ///
    /// Default: `SKILL.md` under `~/.claude/skills/{server_id}/`. Must be a **bare relative
    /// path** with no leading path separator, no `..` component, and no literal `~` component —
    /// it is resolved relative to `base_dir/{server_id}`, not the caller's home directory, so a
    /// `~` here is just a directory name, not a home-directory shortcut. In particular, do
    /// **not** pass [`GenerateSkillResult::default_output_path_hint`] (a display-only string of
    /// the shape `~/.claude/skills/{server_id}/SKILL.md`) here — it is rejected (issue #434).
    /// See [`crate::resolve_skill_output_path`] for the full resolution/confinement contract.
    pub output_path: Option<PathBuf>,

    /// Overwrite if exists.
    #[serde(default)]
    pub overwrite: bool,
}

/// Result from saving a skill.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::types::{SaveSkillResult, SkillMetadata};
///
/// let metadata: SkillMetadata = serde_json::from_value(serde_json::json!({
///     "name": "github", "description": "GitHub MCP skill", "section_count": 5, "word_count": 320,
/// }))
/// .unwrap();
/// let result = SaveSkillResult::new(true, "SKILL.md".to_string(), false, metadata);
/// assert!(result.success);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
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

impl SaveSkillResult {
    /// Creates the outcome of saving a skill.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_skill::SaveSkillResult;
    /// use serde_json::json;
    ///
    /// let metadata = serde_json::from_value(json!({
    ///     "name": "github", "description": "d", "section_count": 1, "word_count": 3,
    /// }))
    /// .unwrap();
    /// let result = SaveSkillResult::new(true, "SKILL.md".to_string(), false, metadata);
    /// assert!(result.success);
    /// ```
    #[must_use]
    pub const fn new(
        success: bool,
        output_path: String,
        overwritten: bool,
        metadata: SkillMetadata,
    ) -> Self {
        Self {
            success,
            output_path,
            overwritten,
            metadata,
        }
    }
}

/// Metadata extracted from saved skill.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::types::SkillMetadata;
///
/// let metadata: SkillMetadata = serde_json::from_value(serde_json::json!({
///     "name": "github-progressive",
///     "description": "GitHub MCP skill",
///     "section_count": 5,
///     "word_count": 320,
/// }))
/// .unwrap();
/// assert_eq!(metadata.name, "github-progressive");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
#[non_exhaustive]
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
// Validation functions
// ============================================================================

/// Errors returned by [`SkillName::new`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum SkillNameError {
    /// The candidate was empty, or contained only whitespace.
    ///
    /// A blank name isn't caught by the length check below (it's certainly not "too long"),
    /// but [`crate::parser::extract_skill_metadata`]'s frontmatter parser rejects an
    /// empty/blank-after-trim `name` unconditionally — so a blank `skill_name` still has to be
    /// rejected here, or it would render and get written to disk only to fail the very
    /// round-trip this validator exists to prevent (issue #413, S3).
    #[error("skill_name must not be empty")]
    Empty,

    /// The candidate exceeded [`MAX_SKILL_NAME_LENGTH`].
    #[error("skill_name too long: {len} chars exceeds {limit} limit")]
    TooLong {
        /// Actual length of the rejected name, in `char`s — matching
        /// [`MAX_SKILL_NAME_LENGTH`]'s unit (`chars().count()`, not `str::len()`), so it
        /// agrees with `GenerateSkillParams::skill_name`'s `#[schemars(length(max = ..))]`
        /// annotation, which JSON Schema also counts in Unicode code points.
        len: usize,
        /// Maximum allowed length ([`MAX_SKILL_NAME_LENGTH`]).
        limit: usize,
    },

    /// The candidate contained a character other than a lowercase ASCII letter, digit, or
    /// hyphen (the Agent Skills `name` rule).
    #[error("skill_name must contain only lowercase letters, digits, and hyphens")]
    InvalidCharacters,
}

/// A [`SkillName`] that differs from the name of the directory the skill is written to.
///
/// Carries no text, so a hostile name is never echoed back.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("skill_name must equal the name of the directory the skill is written to")]
pub struct SkillDirectoryMismatch;

/// A validated skill name: 1-64 lowercase ASCII letters, digits and hyphens.
///
/// Enforces the Agent Skills `name` rule so the name can equal the skill's directory name (the
/// `server_id`). Holding a `SkillName` proves the rule was checked, so callers never re-validate
/// or pass a bare `String`; an invalid name is rejected up front instead of being rendered and
/// written to disk only for [`crate::parser::extract_skill_metadata`] to reject the file.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::SkillName;
///
/// let name = SkillName::new("github").unwrap();
/// assert_eq!(name.as_str(), "github");
/// assert!(SkillName::new("My Skill").is_err());
/// assert!(SkillName::new(&"a".repeat(65)).is_err());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SkillName(String);

impl SkillName {
    /// Validates `name` against the Agent Skills `name` rule and wraps it.
    ///
    /// # Errors
    ///
    /// Returns [`SkillNameError::Empty`] if `name` is empty or all whitespace,
    /// [`SkillNameError::TooLong`] if it exceeds [`MAX_SKILL_NAME_LENGTH`] `char`s, or
    /// [`SkillNameError::InvalidCharacters`] if it holds anything but lowercase letters, digits
    /// and hyphens.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_skill::SkillName;
    ///
    /// assert!(SkillName::new("a-1").is_ok());
    /// assert!(SkillName::new("").is_err());
    /// assert!(SkillName::new("   ").is_err());
    /// ```
    pub fn new(name: &str) -> Result<Self, SkillNameError> {
        if name.trim().is_empty() {
            return Err(SkillNameError::Empty);
        }
        let len = name.chars().count();
        if len > MAX_SKILL_NAME_LENGTH {
            return Err(SkillNameError::TooLong {
                len,
                limit: MAX_SKILL_NAME_LENGTH,
            });
        }
        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
        {
            return Err(SkillNameError::InvalidCharacters);
        }
        Ok(Self(name.to_owned()))
    }

    /// Requires the name to equal `directory`, the name of the directory the skill is written
    /// to; a renamed skill would otherwise orphan the scripts saved next to it.
    ///
    /// # Errors
    ///
    /// Returns [`SkillDirectoryMismatch`] when the two differ.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_skill::SkillName;
    ///
    /// let name = SkillName::new("github").unwrap();
    /// assert!(name.clone().in_directory("github").is_ok());
    /// assert!(name.in_directory("other").is_err());
    /// ```
    pub fn in_directory(self, directory: &str) -> Result<Self, SkillDirectoryMismatch> {
        if self.0 == directory {
            Ok(self)
        } else {
            Err(SkillDirectoryMismatch)
        }
    }

    /// Returns the name as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_skill::SkillName;
    ///
    /// assert_eq!(SkillName::new("abc").unwrap().as_str(), "abc");
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for SkillName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_name_enforces_the_agent_skills_rule() {
        for ok in ["github", "a-1", "0"] {
            assert!(SkillName::new(ok).is_ok(), "{ok:?}");
        }
        for bad in [
            "",
            "   ",
            "My Skill",
            "../x",
            "evil\nname",
            "evil\n### Injected</untrusted-data>",
            &"a".repeat(65),
        ] {
            assert!(SkillName::new(bad).is_err(), "{bad:?}");
        }
        assert_eq!(SkillName::new("abc").unwrap().to_string(), "abc");
    }

    #[test]
    fn skill_name_must_equal_its_directory() {
        let name = SkillName::new("abc").unwrap();
        assert_eq!(name.clone().in_directory("abc").unwrap(), name);
        assert_eq!(name.in_directory("abd"), Err(SkillDirectoryMismatch));
    }

    // ── schemars bounds (issue #205) ──────────────────────────────────────

    #[test]
    fn test_generate_skill_params_schema_declares_server_id_bounds() {
        let schema = schemars::schema_for!(GenerateSkillParams);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        // Asserted against the real runtime constant (not a hardcoded literal), so bumping
        // `MAX_SERVER_ID_LENGTH` without updating the `#[schemars(length(max = ..))]` literal
        // above fails this test instead of leaving the declared schema silently stale
        // (issue #198 S3).
        assert_eq!(props["server_id"]["maxLength"], MAX_SERVER_ID_LENGTH);
        assert_eq!(props["server_id"]["pattern"], "^[a-z0-9-]+$");
    }

    #[test]
    fn test_generate_skill_params_schema_declares_skill_name_bound() {
        let schema = schemars::schema_for!(GenerateSkillParams);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        // Asserted against the real runtime constant, not a hardcoded literal, so bumping
        // `MAX_SKILL_NAME_LENGTH` without updating the `#[schemars(length(max = ..))]` literal
        // above fails this test instead of leaving the declared schema silently stale
        // (mirrors `test_generate_skill_params_schema_declares_server_id_bounds`, issue #413).
        assert_eq!(props["skill_name"]["maxLength"], MAX_SKILL_NAME_LENGTH);
        assert_eq!(props["skill_name"]["pattern"], "^[a-z0-9-]+$");
    }

    #[test]
    fn test_generate_skill_params_schema_states_the_directory_rule() {
        let schema = schemars::schema_for!(GenerateSkillParams);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        let description = props["skill_name"]["description"].as_str().unwrap();
        assert!(
            description.contains("equal that directory name"),
            "{description}"
        );
    }

    #[test]
    fn test_generate_skill_params_schema_declares_use_case_hint_bound() {
        let schema = schemars::schema_for!(GenerateSkillParams);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        // Asserted against the real runtime constant, not a hardcoded literal, so bumping
        // `MAX_USE_CASE_HINT_LENGTH` without updating the `#[schemars(inner(length(max =
        // ..)))]` literal above fails this test instead of leaving the declared schema
        // silently stale (mirrors `test_generate_skill_params_schema_declares_skill_name_bound`,
        // issue #429).
        assert_eq!(
            props["use_case_hints"]["items"]["maxLength"],
            MAX_USE_CASE_HINT_LENGTH
        );
        // Same drift guard for the collection-size bound (critic finding M1): an unbounded
        // entry count defeats a per-entry length bound just as easily as an unbounded
        // per-entry length would.
        assert_eq!(props["use_case_hints"]["maxItems"], MAX_USE_CASE_HINTS);
    }

    #[test]
    fn test_save_skill_params_schema_declares_bounds() {
        let schema = schemars::schema_for!(SaveSkillParams);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        assert_eq!(props["server_id"]["maxLength"], MAX_SERVER_ID_LENGTH);
        assert_eq!(props["server_id"]["pattern"], "^[a-z0-9-]+$");
        // `content`'s bound (`MAX_SKILL_CONTENT_SIZE`) lives in `mcp-execution-server`, a
        // reverse dependency this crate cannot reference — see
        // `mcp-server::service::tests::test_save_skill_params_content_schema_matches_max_skill_content_size`
        // for the drift-proof version of this specific assertion.
        assert_eq!(props["content"]["maxLength"], 102_400);
    }

    #[test]
    fn test_params_reject_a_bad_server_id_at_deserialization() {
        for bad in ["", "GitHub", "my_server", "a.b", "../x"] {
            let value = serde_json::json!({ "server_id": bad });
            assert!(
                serde_json::from_value::<GenerateSkillParams>(value).is_err(),
                "{bad:?}"
            );
            let value = serde_json::json!({ "server_id": bad, "content": "x" });
            assert!(
                serde_json::from_value::<SaveSkillParams>(value).is_err(),
                "{bad:?}"
            );
        }
    }

    // ── skill_name length bound (issue #413) ──────────────────────────────

    #[test]
    fn test_skill_name_valid() {
        assert!(SkillName::new("github").is_ok());
        assert!(SkillName::new("my-server-123").is_ok());
    }

    #[test]
    fn test_skill_name_max_length() {
        let max_name = "a".repeat(MAX_SKILL_NAME_LENGTH);
        assert!(SkillName::new(&max_name).is_ok());
    }

    #[test]
    fn test_skill_name_too_long() {
        let long_name = "a".repeat(MAX_SKILL_NAME_LENGTH + 1);
        let result = SkillName::new(&long_name);
        assert_eq!(
            result,
            Err(SkillNameError::TooLong {
                len: MAX_SKILL_NAME_LENGTH + 1,
                limit: MAX_SKILL_NAME_LENGTH
            })
        );
    }

    /// Issue #413, S3: a blank `skill_name` isn't "too long" (the length check alone would
    /// accept it), but `extract_skill_metadata` rejects an empty/blank `name` unconditionally
    /// — so it must be rejected here too, or it produces exactly the round-trip failure this
    /// validator exists to prevent.
    #[test]
    fn test_skill_name_empty_rejected() {
        assert_eq!(SkillName::new(""), Err(SkillNameError::Empty));
    }

    #[test]
    fn test_skill_name_whitespace_only_rejected() {
        assert_eq!(SkillName::new("   \t\n  "), Err(SkillNameError::Empty));
    }

    #[test]
    fn test_skill_name_rejects_non_slug_characters() {
        for name in ["My Skill", "my_skill", "Github", "my.skill", "я"] {
            assert_eq!(
                SkillName::new(name),
                Err(SkillNameError::InvalidCharacters),
                "{name}"
            );
        }
    }

    #[test]
    fn test_skill_name_counts_chars_not_bytes() {
        let cyrillic_over_limit = "я".repeat(MAX_SKILL_NAME_LENGTH + 1);
        assert_eq!(
            SkillName::new(&cyrillic_over_limit),
            Err(SkillNameError::TooLong {
                len: MAX_SKILL_NAME_LENGTH + 1,
                limit: MAX_SKILL_NAME_LENGTH
            })
        );
    }

    #[test]
    fn test_default_skill_name_bound_equals_server_id_bound() {
        assert_eq!(MAX_SKILL_NAME_LENGTH, MAX_SERVER_ID_LENGTH);
    }
}
