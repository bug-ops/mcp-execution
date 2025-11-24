//! Generate command implementation.
//!
//! Generates Claude skills from MCP server tool definitions.
//! This command:
//! 1. Introspects the server to discover tools and schemas
//! 2. Prompts user for skill name and description
//! 3. Converts to `SkillData` and renders templates
//! 4. Saves skill to `.claude/skills/` directory

use anyhow::{Context, Result, bail};
use mcp_codegen::TemplateEngine;
use mcp_codegen::skills::claude::{render_reference_md, render_skill_md};
use mcp_codegen::skills::converter::SkillConverter;
use mcp_core::cli::{ExitCode, OutputFormat, ServerConnectionString};
use mcp_core::{ServerId, SkillDescription, SkillName};
use mcp_introspector::Introspector;
use mcp_skill_store::SkillStore;
use serde::Serialize;
use tracing::{info, warn};

/// Result of Claude skill generation.
#[derive(Debug, Serialize)]
struct GenerationResult {
    /// Skill name
    skill_name: String,
    /// Server name
    server_name: String,
    /// Number of tools in skill
    tool_count: usize,
    /// Path where skill was saved
    skill_path: String,
}

/// Runs the generate command.
///
/// Generates a Claude skill from an MCP server.
///
/// This command performs the following steps:
/// 1. Introspects the MCP server to discover tools
/// 2. Prompts user for skill name and description (or uses CLI args)
/// 3. Converts server info to `SkillData`
/// 4. Renders `SKILL.md` and `REFERENCE.md` templates
/// 5. Saves skill to `.claude/skills/` directory
///
/// # Arguments
///
/// * `server_name` - MCP server name to introspect
/// * `server_command` - Optional server command (defaults to `server_name`)
/// * `skill_name` - Optional skill name (interactive if not provided)
/// * `skill_description` - Optional skill description (interactive if not provided)
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if:
/// - Server connection fails
/// - Server has no tools
/// - Skill name/description validation fails
/// - Template rendering fails
/// - File system operations fail
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::generate;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # async fn example() -> Result<(), anyhow::Error> {
/// let result = generate::run(
///     "github".to_string(),
///     None,
///     Some("github".to_string()),
///     Some("VK Teams bot integration".to_string()),
///     OutputFormat::Pretty,
/// ).await?;
/// assert_eq!(result, ExitCode::SUCCESS);
/// # Ok(())
/// # }
/// ```
pub async fn run(
    server_name: String,
    server_command: Option<String>,
    skill_name_input: Option<String>,
    skill_description_input: Option<String>,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    info!("Generating Claude skill for server: {}", server_name);

    // Step 1: Introspect MCP server
    let server_conn =
        ServerConnectionString::new(&server_name).context("invalid server connection string")?;

    let server_id = ServerId::new(server_conn.as_str());
    let mut introspector = Introspector::new();

    let server_cmd = server_command.as_deref().unwrap_or(server_conn.as_str());
    let server_info = introspector
        .discover_server(server_id, server_cmd)
        .await
        .context("failed to introspect MCP server")?;

    if server_info.tools.is_empty() {
        warn!("Server '{server_name}' has no tools");
        bail!("no tools found on server '{server_name}'");
    }

    info!("Discovered {} tools from server", server_info.tools.len());

    // Step 2: Get skill metadata (interactive or from CLI args)
    let skill_name = if let Some(name) = skill_name_input {
        SkillName::new(&name).context("invalid skill name")?
    } else {
        prompt_skill_name(&server_name)?
    };

    let skill_description = if let Some(desc) = skill_description_input {
        SkillDescription::new(&desc).context("invalid skill description")?
    } else {
        prompt_skill_description(&server_name, &server_info)?
    };

    info!("Creating skill: {}", skill_name.as_str());

    // Step 3: Convert to SkillData
    let skill_data = SkillConverter::convert(&server_info, &skill_name, &skill_description)
        .context("failed to convert server info to skill data")?;

    // Step 4: Render templates
    let engine = TemplateEngine::new().context("failed to create template engine")?;
    let skill_md = render_skill_md(&engine, &skill_data).context("failed to render SKILL.md")?;
    let reference_md =
        render_reference_md(&engine, &skill_data).context("failed to render REFERENCE.md")?;

    info!("Generated SKILL.md ({} bytes)", skill_md.len());
    info!("Generated REFERENCE.md ({} bytes)", reference_md.len());

    // Step 5: Save to .claude/skills/
    let store = SkillStore::new_claude().context("failed to initialize skill store")?;
    store
        .save_claude_skill(&skill_name, &skill_md, &reference_md, &skill_data)
        .context("failed to save Claude skill")?;

    // Get skill path from HOME/.claude/skills/skill-name
    let home = dirs::home_dir().context("failed to get home directory")?;
    let skill_path = home.join(".claude/skills").join(skill_name.as_str());
    info!("Skill saved to: {}", skill_path.display());

    // Step 6: Output success message
    let result = GenerationResult {
        skill_name: skill_name.to_string(),
        server_name: server_info.name,
        tool_count: server_info.tools.len(),
        skill_path: skill_path.display().to_string(),
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    info!(
        "Successfully created skill '{}' with {} tools",
        result.skill_name, result.tool_count
    );

    Ok(ExitCode::SUCCESS)
}

/// Prompts user for skill name interactively.
///
/// Generates a sensible default from the server name by:
/// - Removing common suffixes (-server, -bot)
/// - Converting to lowercase
/// - Replacing underscores with hyphens
///
/// # Errors
///
/// Returns an error if user input cannot be read or validation fails repeatedly.
fn prompt_skill_name(server_name: &str) -> Result<SkillName> {
    use dialoguer::Input;

    let default_name = server_name
        .trim_end_matches("-server")
        .trim_end_matches("-bot")
        .to_lowercase()
        .replace('_', "-");

    loop {
        let input: String = Input::new()
            .with_prompt("Skill name (lowercase, alphanumeric, hyphens)")
            .default(default_name.clone())
            .interact()
            .context("failed to read user input")?;

        match SkillName::new(&input) {
            Ok(name) => return Ok(name),
            Err(e) => {
                eprintln!("Invalid skill name: {e}");
                eprintln!("Requirements: 1-64 chars, [a-z0-9-_], start with letter");
            }
        }
    }
}

/// Prompts user for skill description interactively.
///
/// Generates a default description from server metadata.
///
/// # Errors
///
/// Returns an error if user input cannot be read or validation fails repeatedly.
fn prompt_skill_description(
    server_name: &str,
    server_info: &mcp_introspector::ServerInfo,
) -> Result<SkillDescription> {
    use dialoguer::Input;

    let default_desc = format!(
        "Interact with {} MCP server ({} tools available)",
        server_name,
        server_info.tools.len()
    );

    loop {
        let input: String = Input::new()
            .with_prompt("Skill description (max 1024 chars, actionable)")
            .default(default_desc.clone())
            .interact()
            .context("failed to read user input")?;

        match SkillDescription::new(&input) {
            Ok(desc) => return Ok(desc),
            Err(e) => {
                eprintln!("Invalid description: {e}");
                eprintln!("Requirements: max 1024 chars, no XML tags, no reserved words");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generation_result_serialization() {
        let result = GenerationResult {
            skill_name: "test-skill".to_string(),
            server_name: "test-server".to_string(),
            tool_count: 5,
            skill_path: "/home/user/.claude/skills/test-skill".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-skill"));
        assert!(json.contains("test-server"));
        assert!(json.contains('5'));
    }

    #[test]
    fn test_generation_result_fields() {
        let result = GenerationResult {
            skill_name: "my-skill".to_string(),
            server_name: "my-server".to_string(),
            tool_count: 10,
            skill_path: "/path/to/skill".to_string(),
        };

        assert_eq!(result.skill_name, "my-skill");
        assert_eq!(result.server_name, "my-server");
        assert_eq!(result.tool_count, 10);
        assert_eq!(result.skill_path, "/path/to/skill");
    }

    #[test]
    fn test_server_connection_string_validation() {
        // Valid connection strings
        assert!(ServerConnectionString::new("test-server").is_ok());
        assert!(ServerConnectionString::new("github").is_ok());
        assert!(ServerConnectionString::new("/path/to/server").is_ok());

        // Invalid connection strings
        assert!(ServerConnectionString::new("").is_err());
        assert!(ServerConnectionString::new("server with spaces").is_err());
        assert!(ServerConnectionString::new("server && rm -rf /").is_err());
    }

    #[test]
    fn test_skill_name_default_generation() {
        // Test suffix removal
        let server1 = "github";
        let default1 = server1.trim_end_matches("-bot").to_lowercase();
        assert_eq!(default1, "github");

        let server2 = "my-server";
        let default2 = server2.trim_end_matches("-server").to_lowercase();
        assert_eq!(default2, "my");

        // Test underscore replacement
        let server3 = "my_cool_server";
        let default3 = server3.to_lowercase().replace('_', "-");
        assert_eq!(default3, "my-cool-server");
    }

    #[test]
    fn test_skill_name_validation() {
        // Valid skill names
        assert!(SkillName::new("valid-skill").is_ok());
        assert!(SkillName::new("skill123").is_ok());
        assert!(SkillName::new("a").is_ok());

        // Invalid skill names
        assert!(SkillName::new("").is_err());
        assert!(SkillName::new("Invalid-Skill").is_err()); // uppercase
        assert!(SkillName::new("skill with spaces").is_err());
        assert!(SkillName::new("123skill").is_err()); // starts with number
    }

    #[test]
    fn test_skill_description_validation() {
        // Valid descriptions
        assert!(SkillDescription::new("A valid description").is_ok());
        assert!(SkillDescription::new("Interact with VK Teams bot").is_ok());

        // Invalid descriptions
        assert!(SkillDescription::new("").is_err());
        assert!(SkillDescription::new("<xml>Invalid</xml>").is_err()); // XML tags
        let long_desc = "a".repeat(1025);
        assert!(SkillDescription::new(&long_desc).is_err()); // too long
    }

    #[test]
    fn test_default_description_format() {
        let server_name = "github";
        let tool_count = 5;
        let desc = format!("Interact with {server_name} MCP server ({tool_count} tools available)");

        assert!(desc.contains("github"));
        assert!(desc.contains("5 tools"));
    }
}
