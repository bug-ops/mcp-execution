//! Skill management command implementation.
//!
//! Provides commands to save, load, list, and manage skills saved to disk.
//! Skills are stored in a directory structure with VFS files and WASM modules.

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_core::SkillName;
use mcp_skill_store::SkillStore;
use serde::Serialize;
use tracing::{info, warn};

/// Skill management actions.
#[derive(Subcommand, Debug)]
pub enum SkillAction {
    /// Load a Claude skill from disk
    Load {
        /// Skill name
        name: String,
    },

    /// List available Claude skills
    List,

    /// Remove a Claude skill
    Remove {
        /// Skill name
        name: String,

        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show Claude skill information
    Info {
        /// Skill name
        name: String,
    },
}

/// Result of loading a Claude skill.
#[derive(Debug, Serialize)]
struct LoadResult {
    /// Skill name
    name: String,
    /// Server name
    server_name: String,
    /// Number of tools
    tool_count: usize,
    /// Skill file size
    skill_md_size: usize,
    /// Reference file size
    reference_md_size: usize,
}

/// Result of listing skills.
#[derive(Debug, Serialize)]
struct ListResult {
    /// Skill directory
    skill_dir: String,
    /// Number of skills found
    skill_count: usize,
    /// Skill information
    skills: Vec<SkillSummary>,
}

/// Summary of a skill for listing.
#[derive(Debug, Serialize)]
struct SkillSummary {
    /// Server name
    name: String,
    /// Server version
    version: String,
    /// Number of tools
    tool_count: usize,
    /// Generation timestamp
    generated_at: String,
}

/// Result of showing skill info.
#[derive(Debug, Serialize)]
struct InfoResult {
    /// Skill name
    name: String,
    /// Server version
    version: String,
    /// Protocol version
    protocol_version: String,
    /// Generation timestamp
    generated_at: String,
    /// Generator version
    generator_version: String,
    /// Number of tools
    tool_count: usize,
    /// Number of VFS files
    file_count: usize,
    /// WASM module size
    wasm_size: usize,
    /// Tool names
    tools: Vec<ToolSummary>,
}

/// Tool summary for info display.
#[derive(Debug, Serialize)]
struct ToolSummary {
    /// Tool name
    name: String,
    /// Tool description
    description: String,
}

/// Result of removing a skill.
#[derive(Debug, Serialize)]
struct RemoveResult {
    /// Skill name
    name: String,
    /// Whether removal was successful
    success: bool,
}

/// Runs the skill management command.
///
/// Routes skill actions to their respective handlers.
///
/// # Arguments
///
/// * `action` - Plugin management action to execute
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if the skill operation fails.
///
/// # Examples
///
/// ```no_run
/// use mcp_cli::commands::skill::{SkillAction, run};
/// use mcp_core::cli::{ExitCode, OutputFormat};
/// use std::path::PathBuf;
///
/// # async fn example() -> Result<(), anyhow::Error> {
/// let action = SkillAction::List {
///     skill_dir: PathBuf::from("./skills"),
/// };
///
/// let result = run(action, OutputFormat::Pretty).await?;
/// assert_eq!(result, ExitCode::SUCCESS);
/// # Ok(())
/// # }
/// ```
pub async fn run(action: SkillAction, output_format: OutputFormat) -> Result<ExitCode> {
    match action {
        SkillAction::Load { name } => load_skill(&name, output_format),
        SkillAction::List => list_skills(output_format),
        SkillAction::Remove { name, yes } => remove_skill(&name, yes, output_format),
        SkillAction::Info { name } => show_skill_info(&name, output_format),
    }
}

/// Loads a Claude skill from disk.
///
/// # Errors
///
/// Returns an error if the skill doesn't exist or fails checksum verification.
pub fn load_skill(name: &str, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Loading Claude skill: {}", name);

    let skill_name = SkillName::new(name).context("invalid skill name")?;

    let store = SkillStore::new_claude().context("failed to initialize skill store")?;

    let loaded = store
        .load_claude_skill(&skill_name)
        .with_context(|| format!("failed to load skill '{name}'"))?;

    let result = LoadResult {
        name: loaded.metadata.skill_name.clone(),
        server_name: loaded.metadata.server_name.clone(),
        tool_count: loaded.metadata.tool_count,
        skill_md_size: loaded.skill_md.len(),
        reference_md_size: loaded.reference_md.as_ref().map_or(0, String::len),
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    info!(
        "Successfully loaded skill: {} ({} tools)",
        result.name, result.tool_count
    );

    Ok(ExitCode::SUCCESS)
}

/// Lists all available Claude skills.
///
/// # Errors
///
/// Returns an error if the skill directory cannot be read.
pub fn list_skills(output_format: OutputFormat) -> Result<ExitCode> {
    let store = SkillStore::new_claude().context("failed to initialize skill store")?;

    // Get skill directory from HOME/.claude/skills
    let home = dirs::home_dir().context("failed to get home directory")?;
    let skill_dir = home.join(".claude/skills");

    info!("Listing Claude skills in: {}", skill_dir.display());

    let skills = store
        .list_claude_skills()
        .context("failed to list skills")?;

    if skills.is_empty() {
        warn!("No skills found in {}", skill_dir.display());
    }

    let summaries: Vec<SkillSummary> = skills
        .iter()
        .map(|p| SkillSummary {
            name: p.skill_name.clone(),
            version: p.server_version.clone(),
            tool_count: p.tool_count,
            generated_at: p.generated_at.to_rfc3339(),
        })
        .collect();

    let result = ListResult {
        skill_dir: skill_dir.display().to_string(),
        skill_count: skills.len(),
        skills: summaries,
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    info!("Found {} skill(s)", result.skill_count);

    Ok(ExitCode::SUCCESS)
}

/// Removes a Claude skill from disk.
///
/// # Errors
///
/// Returns an error if the skill doesn't exist or cannot be removed.
pub fn remove_skill(name: &str, yes: bool, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Removing Claude skill: {}", name);

    let skill_name = SkillName::new(name).context("invalid skill name")?;

    let store = SkillStore::new_claude().context("failed to initialize skill store")?;

    // Check if skill exists by checking the directory
    let home = dirs::home_dir().context("failed to get home directory")?;
    let skill_path = home.join(".claude/skills").join(skill_name.as_str());
    if !skill_path.exists() {
        bail!("skill '{name}' not found");
    }

    // Prompt for confirmation unless --yes flag is set
    if !yes {
        use dialoguer::Confirm;

        let confirmed = Confirm::new()
            .with_prompt(format!("Are you sure you want to remove skill '{name}'?"))
            .default(false)
            .interact()
            .context("failed to read confirmation")?;

        if !confirmed {
            info!("Skill removal cancelled by user");
            return Ok(ExitCode::SUCCESS);
        }
    }

    store
        .remove_claude_skill(&skill_name)
        .with_context(|| format!("failed to remove skill '{name}'"))?;

    let result = RemoveResult {
        name: name.to_string(),
        success: true,
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    info!("Successfully removed skill: {}", name);

    Ok(ExitCode::SUCCESS)
}

/// Shows detailed information about a Claude skill.
///
/// # Errors
///
/// Returns an error if the skill doesn't exist or cannot be loaded.
pub fn show_skill_info(name: &str, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Showing info for Claude skill: {}", name);

    let skill_name = SkillName::new(name).context("invalid skill name")?;

    let store = SkillStore::new_claude().context("failed to initialize skill store")?;

    let loaded = store
        .load_claude_skill(&skill_name)
        .with_context(|| format!("failed to load skill '{name}'"))?;

    // Parse tools from skill data if available (for detailed view)
    // For now, we don't have individual tool data in metadata, so show empty list
    let tools: Vec<ToolSummary> = vec![];

    let result = InfoResult {
        name: loaded.metadata.skill_name,
        version: loaded.metadata.server_version,
        protocol_version: loaded.metadata.protocol_version,
        generated_at: loaded.metadata.generated_at.to_rfc3339(),
        generator_version: loaded.metadata.generator_version,
        tool_count: loaded.metadata.tool_count,
        file_count: 2, // SKILL.md and REFERENCE.md
        wasm_size: 0,  // No WASM in Claude format
        tools,
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_result_serialization() {
        let result = LoadResult {
            name: "test-skill".to_string(),
            server_name: "test-server".to_string(),
            tool_count: 5,
            skill_md_size: 1024,
            reference_md_size: 2048,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-skill"));
        assert!(json.contains("test-server"));
        assert!(json.contains('5'));
    }

    #[test]
    fn test_list_result_serialization() {
        let result = ListResult {
            skill_dir: "./skills".to_string(),
            skill_count: 2,
            skills: vec![
                SkillSummary {
                    name: "skill1".to_string(),
                    version: "1.0.0".to_string(),
                    tool_count: 3,
                    generated_at: "2025-11-21T12:00:00Z".to_string(),
                },
                SkillSummary {
                    name: "skill2".to_string(),
                    version: "2.0.0".to_string(),
                    tool_count: 5,
                    generated_at: "2025-11-21T13:00:00Z".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("skill1"));
        assert!(json.contains("skill2"));
    }

    #[test]
    fn test_info_result_serialization() {
        let result = InfoResult {
            name: "test-server".to_string(),
            version: "1.0.0".to_string(),
            protocol_version: "2024-11-05".to_string(),
            generated_at: "2025-11-21T12:00:00Z".to_string(),
            generator_version: "0.1.0".to_string(),
            tool_count: 2,
            file_count: 8,
            wasm_size: 2048,
            tools: vec![
                ToolSummary {
                    name: "tool1".to_string(),
                    description: "First tool".to_string(),
                },
                ToolSummary {
                    name: "tool2".to_string(),
                    description: "Second tool".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-server"));
        assert!(json.contains("tool1"));
        assert!(json.contains("tool2"));
    }

    #[test]
    fn test_remove_result_serialization() {
        let result = RemoveResult {
            name: "old-server".to_string(),
            success: true,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("old-server"));
        assert!(json.contains("true"));
    }
}
