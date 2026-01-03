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
const MAX_SERVER_ID_LENGTH: usize = 64;

// ============================================================================
// generate_skill types
// ============================================================================

/// Parameters for generating a skill.
///
/// # Examples
///
/// ```
/// use mcp_skill::types::GenerateSkillParams;
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
    /// Must contain only lowercase letters, digits, and hyphens.
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
#[derive(Debug, Clone, Serialize, JsonSchema)]
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
/// use mcp_skill::types::SaveSkillParams;
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
    pub server_id: String,

    /// SKILL.md content (markdown with YAML frontmatter).
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
/// Returns `Err` with descriptive message if:
/// - Length exceeds 64 characters
/// - Contains characters other than lowercase letters, digits, and hyphens
///
/// # Validation Rules
///
/// - Length must not exceed 64 characters
/// - Must contain only lowercase letters, digits, and hyphens
///
/// # Examples
///
/// ```
/// use mcp_skill::validate_server_id;
///
/// assert!(validate_server_id("github").is_ok());
/// assert!(validate_server_id("my-server-123").is_ok());
/// assert!(validate_server_id("GitHub").is_err()); // uppercase
/// assert!(validate_server_id("my_server").is_err()); // underscore
/// ```
pub fn validate_server_id(server_id: &str) -> Result<(), String> {
    // Check length
    if server_id.len() > MAX_SERVER_ID_LENGTH {
        return Err(format!(
            "server_id too long: {} chars exceeds {} limit",
            server_id.len(),
            MAX_SERVER_ID_LENGTH
        ));
    }

    // Check format
    if !server_id
        .chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
    {
        return Err(
            "server_id must contain only lowercase letters, digits, and hyphens".to_string(),
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_server_id_valid() {
        assert!(validate_server_id("github").is_ok());
        assert!(validate_server_id("my-server").is_ok());
        assert!(validate_server_id("server123").is_ok());
        assert!(validate_server_id("my-server-123").is_ok());
    }

    #[test]
    fn test_validate_server_id_uppercase() {
        let result = validate_server_id("GitHub");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("lowercase"));
    }

    #[test]
    fn test_validate_server_id_underscore() {
        let result = validate_server_id("my_server");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("lowercase"));
    }

    #[test]
    fn test_validate_server_id_special_chars() {
        let result = validate_server_id("my@server");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("lowercase"));
    }

    #[test]
    fn test_validate_server_id_too_long() {
        let long_id = "a".repeat(65);
        let result = validate_server_id(&long_id);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("too long"));
    }

    #[test]
    fn test_validate_server_id_max_length() {
        let max_id = "a".repeat(64);
        assert!(validate_server_id(&max_id).is_ok());
    }
}
