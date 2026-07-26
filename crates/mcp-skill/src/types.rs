//! Type definitions for skill generation.
//!
//! This module defines all parameter and result types for skill generation:
//! - `GenerateSkillParams`: Parameters for generating a skill
//! - `GenerateSkillResult`: Result from skill generation
//! - `SaveSkillParams`: Parameters for saving a skill
//! - `SaveSkillResult`: Result from saving a skill

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use thiserror::Error;

/// Maximum `server_id` length (denial-of-service protection).
///
/// `pub` (rather than private) so downstream crates' schemars drift-guard tests — e.g.
/// `mcp-execution-server`'s, for `IntrospectServerParams::server_id` — can assert the declared
/// schema length against this real constant instead of a hardcoded literal (issue #198 S3).
pub const MAX_SERVER_ID_LENGTH: usize = 64;

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
    /// Must be 1-64 lowercase letters, digits, or hyphens (see `validate_server_id`'s
    /// `MAX_SERVER_ID_LENGTH`, mirrored here as a literal since schemars attributes cannot
    /// reference a `const`).
    #[schemars(length(max = 64), regex(pattern = r"^[a-z0-9-]+$"))]
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
#[derive(Debug, Clone, Serialize, Deserialize, JsonSchema)]
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

    /// Non-fatal drift warnings, e.g. `.ts` files on disk excluded from
    /// `categories`/`tool_count` because `_meta.json` has no matching entry
    /// for them. Empty when the scanned directory has no drift.
    #[serde(default)]
    pub warnings: Vec<String>,
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
/// use mcp_execution_skill::types::SaveSkillParams;
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
    ///
    /// Must be 1-64 lowercase letters, digits, or hyphens (see `validate_server_id`'s
    /// `MAX_SERVER_ID_LENGTH`, mirrored here as a literal since schemars attributes cannot
    /// reference a `const`).
    #[schemars(length(max = 64), regex(pattern = r"^[a-z0-9-]+$"))]
    pub server_id: String,

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
// Validation functions
// ============================================================================

/// Errors returned by [`validate_server_id`].
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum ServerIdError {
    /// `server_id` was empty.
    ///
    /// An empty `server_id` would collapse a per-server confinement
    /// directory back to its shared parent, so it is rejected explicitly
    /// rather than falling through the (vacuously true) character-class
    /// check below.
    #[error("server_id must not be empty")]
    Empty,

    /// `server_id` exceeded the maximum allowed length.
    #[error("server_id too long: {len} chars exceeds {limit} limit")]
    TooLong {
        /// Actual length of the rejected `server_id`, in bytes (`str::len`,
        /// not `chars().count()`).
        len: usize,
        /// Maximum allowed length (`MAX_SERVER_ID_LENGTH`).
        limit: usize,
    },

    /// `server_id` contained a character other than a lowercase letter,
    /// digit, or hyphen.
    #[error("server_id must contain only lowercase letters, digits, and hyphens")]
    InvalidCharacters,
}

/// Validate `server_id` format and length.
///
/// # Arguments
///
/// * `server_id` - Server identifier to validate
///
/// # Returns
///
/// `Ok(())` if valid.
///
/// # Errors
///
/// Returns [`ServerIdError`] if:
/// - Empty
/// - Length exceeds 64 characters
/// - Contains characters other than lowercase letters, digits, and hyphens
///
/// # Validation Rules
///
/// - Must not be empty
/// - Length must not exceed 64 characters
/// - Must contain only lowercase letters, digits, and hyphens
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::validate_server_id;
///
/// assert!(validate_server_id("github").is_ok());
/// assert!(validate_server_id("my-server-123").is_ok());
/// assert!(validate_server_id("").is_err()); // empty
/// assert!(validate_server_id("GitHub").is_err()); // uppercase
/// assert!(validate_server_id("my_server").is_err()); // underscore
/// ```
pub fn validate_server_id(server_id: &str) -> Result<(), ServerIdError> {
    // Check emptiness (the character-class check below is vacuously true for
    // an empty string, and an empty server_id would collapse a per-server
    // confinement directory back to its shared parent)
    if server_id.is_empty() {
        return Err(ServerIdError::Empty);
    }

    // Check length
    if server_id.len() > MAX_SERVER_ID_LENGTH {
        return Err(ServerIdError::TooLong {
            len: server_id.len(),
            limit: MAX_SERVER_ID_LENGTH,
        });
    }

    // Check format
    if !server_id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(ServerIdError::InvalidCharacters);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn test_validate_server_id_valid() {
        assert!(validate_server_id("github").is_ok());
        assert!(validate_server_id("my-server").is_ok());
        assert!(validate_server_id("server123").is_ok());
        assert!(validate_server_id("my-server-123").is_ok());
    }

    #[test]
    fn test_validate_server_id_empty() {
        let result = validate_server_id("");
        assert_eq!(result, Err(ServerIdError::Empty));
    }

    #[test]
    fn test_validate_server_id_uppercase() {
        let result = validate_server_id("GitHub");
        assert_eq!(result, Err(ServerIdError::InvalidCharacters));
    }

    #[test]
    fn test_validate_server_id_underscore() {
        let result = validate_server_id("my_server");
        assert_eq!(result, Err(ServerIdError::InvalidCharacters));
    }

    #[test]
    fn test_validate_server_id_special_chars() {
        let result = validate_server_id("my@server");
        assert_eq!(result, Err(ServerIdError::InvalidCharacters));
    }

    #[test]
    fn test_validate_server_id_too_long() {
        let long_id = "a".repeat(65);
        let result = validate_server_id(&long_id);
        assert_eq!(
            result,
            Err(ServerIdError::TooLong {
                len: 65,
                limit: MAX_SERVER_ID_LENGTH
            })
        );
    }

    #[test]
    fn test_validate_server_id_max_length() {
        let max_id = "a".repeat(64);
        assert!(validate_server_id(&max_id).is_ok());
    }
}
