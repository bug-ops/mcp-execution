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

/// Maximum `server_id` length (denial-of-service protection).
///
/// Re-exported from `mcp_execution_core`, the authoritative owner of the server id slug
/// invariant (see [`validate_server_id`]). `pub` (rather than private) so downstream crates'
/// schemars drift-guard tests — e.g. `mcp-execution-server`'s, for
/// `IntrospectServerParams::server_id` — can assert the declared schema length against this
/// real constant instead of a hardcoded literal (issue #198 S3).
pub use mcp_execution_core::MAX_SERVER_ID_LENGTH;

/// Maximum `skill_name` length, in `char`s (UX cap, with a comfortable frontmatter-budget
/// margin as a side effect — see below).
///
/// Counted in `char`s, not bytes, to match `GenerateSkillParams::skill_name`'s
/// `#[schemars(length(max = ..))]` annotation: JSON Schema's `maxLength` counts Unicode code
/// points, so an MCP client validating a candidate name against the declared schema and this
/// crate's own [`validate_skill_name`] must agree on what "200" counts, or a multi-byte name
/// (e.g. Cyrillic, CJK) the schema accepts as valid could still be rejected at runtime (or vice
/// versa) purely from a unit mismatch, not an actual length problem (issue #413, S2).
///
/// 200 `char`s is generous as a human-readable label — the default `{server_id}-progressive`
/// form is well under 100 `char`s even at `server_id`'s own 64-byte maximum — while still
/// bounding worst-case UTF-8 size: 200 `char`s is at most 800 bytes (4 bytes/char), comfortably
/// inside [`crate::parser::MAX_FRONTMATTER_SIZE`] (8 KiB), the overall cap `skill_name` shares
/// with `description` in `SKILL.md`'s YAML frontmatter.
pub const MAX_SKILL_NAME_LENGTH: usize = 200;

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
    /// Default: `{server_id}-progressive`. Max 200 characters (see `validate_skill_name`'s
    /// `MAX_SKILL_NAME_LENGTH`, mirrored here as a literal since schemars attributes cannot
    /// reference a `const`; JSON Schema's `maxLength` already counts Unicode code points, the
    /// same unit `validate_skill_name` checks at runtime). When supplied, this name is embedded
    /// in both the response's `skill_name` field and its `generation_prompt` (the `**Skill
    /// Name**` line the LLM is instructed to use in `SKILL.md`'s frontmatter) — both hold the
    /// same control-character-flattened, length-truncated text (see
    /// `mcp_execution_skill::build_skill_context`'s doc comment for the one cosmetic exception:
    /// the prompt's surrounding untrusted-data block additionally HTML-escapes `<`/`>`/`&` as an
    /// injection defense).
    #[schemars(length(max = 200))]
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

    /// Suggested output path for the skill file, for display purposes only.
    ///
    /// Shaped like `~/.claude/skills/{server_id}/SKILL.md` (with a literal, unexpanded `~`) —
    /// this shows where the file will land under its *default* location, but is never validated
    /// as a real filesystem path. **Do not** pass this value as `save_skill`'s `output_path`
    /// parameter: despite sharing the same field name, that parameter has entirely different
    /// semantics (a bare relative path with no `~`, resolved under `base_dir/{server_id}` — see
    /// [`SaveSkillParams::output_path`]) and will reject a `~`-containing value (issue #434).
    /// Omit `save_skill`'s `output_path` entirely to use its own default.
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
    /// Default: `SKILL.md` under `~/.claude/skills/{server_id}/`. Must be a **bare relative
    /// path** with no leading path separator, no `..` component, and no literal `~` component —
    /// it is resolved relative to `base_dir/{server_id}`, not the caller's home directory, so a
    /// `~` here is just a directory name, not a home-directory shortcut. In particular, do
    /// **not** pass [`GenerateSkillResult::output_path`] (a display-only string of the shape
    /// `~/.claude/skills/{server_id}/SKILL.md`) here — it is rejected (issue #434). See
    /// [`crate::resolve_skill_output_path`] for the full resolution/confinement contract.
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
///
/// A re-export of [`mcp_execution_core::ServerIdSlugError`] — the authoritative error type for
/// this invariant — under this crate's own name, so existing callers of
/// `mcp_execution_skill::SkillServerIdError` are unaffected. This crate previously hand-rolled a
/// structurally identical enum plus a manual `From` conversion; that let this crate's error
/// wording drift from `mcp_execution_core`'s (the MCP tool handlers in `mcp-execution-server`
/// surface the core wording directly, so the two had visibly disagreed on identical input). A
/// re-export makes that drift structurally impossible: there is only one type, and therefore
/// only one `Display` wording, to keep in sync.
pub use mcp_execution_core::ServerIdSlugError as SkillServerIdError;

/// Validate `server_id` format and length.
///
/// Delegates to [`mcp_execution_core::validate_server_id_slug`], the authoritative owner of
/// this invariant.
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
/// Returns [`SkillServerIdError`] if:
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
pub fn validate_server_id(server_id: &str) -> Result<(), SkillServerIdError> {
    mcp_execution_core::validate_server_id_slug(server_id)
}

/// Errors returned by [`validate_skill_name`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
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
}

/// Validate a custom `skill_name`.
///
/// Mirrors [`validate_server_id`]'s bound-checking style. Unlike `server_id`, `skill_name` is a
/// free-form human-readable label with no character-set restriction — non-emptiness and a
/// length bound (counted in `char`s, matching `GenerateSkillParams::skill_name`'s
/// `#[schemars(length(max = ..))]` annotation) are the only invariants enforced here, so that an
/// invalid name is rejected up front instead of being rendered and written to disk only for
/// [`crate::parser::extract_skill_metadata`] to reject the file — either for a blank `name` or
/// once its `MAX_FRONTMATTER_SIZE` cap is exceeded (issue #413).
///
/// # Errors
///
/// Returns [`SkillNameError::Empty`] if `name` is empty or all whitespace, or
/// [`SkillNameError::TooLong`] if it exceeds [`MAX_SKILL_NAME_LENGTH`] `char`s.
///
/// # Examples
///
/// ```
/// use mcp_execution_skill::validate_skill_name;
///
/// assert!(validate_skill_name("github-progressive").is_ok());
/// assert!(validate_skill_name(&"a".repeat(201)).is_err());
/// assert!(validate_skill_name("").is_err());
/// assert!(validate_skill_name("   ").is_err());
/// ```
pub fn validate_skill_name(name: &str) -> Result<(), SkillNameError> {
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
    fn test_generate_skill_params_schema_declares_skill_name_bound() {
        let schema = schemars::schema_for!(GenerateSkillParams);
        let props = schema.get("properties").unwrap().as_object().unwrap();

        // Asserted against the real runtime constant, not a hardcoded literal, so bumping
        // `MAX_SKILL_NAME_LENGTH` without updating the `#[schemars(length(max = ..))]` literal
        // above fails this test instead of leaving the declared schema silently stale
        // (mirrors `test_generate_skill_params_schema_declares_server_id_bounds`, issue #413).
        assert_eq!(props["skill_name"]["maxLength"], MAX_SKILL_NAME_LENGTH);
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
        assert_eq!(result, Err(SkillServerIdError::Empty));
    }

    #[test]
    fn test_validate_server_id_uppercase() {
        let result = validate_server_id("GitHub");
        assert_eq!(result, Err(SkillServerIdError::InvalidCharacters));
    }

    #[test]
    fn test_validate_server_id_underscore() {
        let result = validate_server_id("my_server");
        assert_eq!(result, Err(SkillServerIdError::InvalidCharacters));
    }

    #[test]
    fn test_validate_server_id_special_chars() {
        let result = validate_server_id("my@server");
        assert_eq!(result, Err(SkillServerIdError::InvalidCharacters));
    }

    #[test]
    fn test_validate_server_id_too_long() {
        let long_id = "a".repeat(65);
        let result = validate_server_id(&long_id);
        assert_eq!(
            result,
            Err(SkillServerIdError::TooLong {
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

    // ── skill_name length bound (issue #413) ──────────────────────────────

    #[test]
    fn test_validate_skill_name_valid() {
        assert!(validate_skill_name("github-progressive").is_ok());
        assert!(validate_skill_name("My Custom Skill").is_ok());
    }

    #[test]
    fn test_validate_skill_name_max_length() {
        let max_name = "a".repeat(MAX_SKILL_NAME_LENGTH);
        assert!(validate_skill_name(&max_name).is_ok());
    }

    #[test]
    fn test_validate_skill_name_too_long() {
        let long_name = "a".repeat(MAX_SKILL_NAME_LENGTH + 1);
        let result = validate_skill_name(&long_name);
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
    fn test_validate_skill_name_empty_rejected() {
        assert_eq!(validate_skill_name(""), Err(SkillNameError::Empty));
    }

    #[test]
    fn test_validate_skill_name_whitespace_only_rejected() {
        assert_eq!(validate_skill_name("   \t\n  "), Err(SkillNameError::Empty));
    }

    /// Issue #413, S2: `MAX_SKILL_NAME_LENGTH` and `validate_skill_name` must count `char`s,
    /// not bytes, to agree with `GenerateSkillParams::skill_name`'s
    /// `#[schemars(length(max = ..))]` annotation (JSON Schema's `maxLength` counts Unicode
    /// code points). A 200-character Cyrillic name is 2 bytes/char = 400 bytes — well over
    /// 200 bytes but exactly at the 200-char limit the schema and this validator both declare;
    /// if the validator counted bytes, it would reject a name the schema says is valid.
    #[test]
    fn test_validate_skill_name_counts_chars_not_bytes_for_multi_byte_text() {
        let cyrillic_name = "я".repeat(MAX_SKILL_NAME_LENGTH);
        assert_eq!(cyrillic_name.chars().count(), MAX_SKILL_NAME_LENGTH);
        assert!(
            cyrillic_name.len() > MAX_SKILL_NAME_LENGTH,
            "sanity: 'я' is multi-byte"
        );

        assert!(
            validate_skill_name(&cyrillic_name).is_ok(),
            "a name at exactly MAX_SKILL_NAME_LENGTH chars must be accepted regardless of its \
             byte length"
        );

        let over_by_one_char = format!("{cyrillic_name}я");
        assert_eq!(
            validate_skill_name(&over_by_one_char),
            Err(SkillNameError::TooLong {
                len: MAX_SKILL_NAME_LENGTH + 1,
                limit: MAX_SKILL_NAME_LENGTH
            })
        );
    }
}
