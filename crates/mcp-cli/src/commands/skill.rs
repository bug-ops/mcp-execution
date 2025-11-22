//! Plugin management command implementation.
//!
//! Provides commands to save, load, list, and manage plugins saved to disk.
//! Plugins are stored in a directory structure with VFS files and WASM modules.

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_skill_store::SkillStore;
use serde::Serialize;
use std::path::PathBuf;
use tracing::{info, warn};

/// Plugin management actions.
#[derive(Subcommand, Debug)]
pub enum SkillAction {
    /// Load a plugin from disk
    Load {
        /// Skill name (server name)
        name: String,

        /// Skill directory (defaults to ./plugins)
        #[arg(long, default_value = "./plugins")]
        skill_dir: PathBuf,
    },

    /// List available plugins
    List {
        /// Skill directory
        #[arg(long, default_value = "./plugins")]
        skill_dir: PathBuf,
    },

    /// Remove a skill
    Remove {
        /// Skill name
        name: String,

        /// Skill directory
        #[arg(long, default_value = "./plugins")]
        skill_dir: PathBuf,

        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show skill information
    Info {
        /// Skill name
        name: String,

        /// Skill directory
        #[arg(long, default_value = "./plugins")]
        skill_dir: PathBuf,
    },
}

/// Result of loading a plugin.
#[derive(Debug, Serialize)]
struct LoadResult {
    /// Skill name
    name: String,
    /// Server version
    version: String,
    /// Number of tools
    tool_count: usize,
    /// Number of VFS files
    file_count: usize,
    /// WASM module size in bytes
    wasm_size: usize,
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

/// Result of showing plugin info.
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

/// Result of removing a plugin.
#[derive(Debug, Serialize)]
struct RemoveResult {
    /// Skill name
    name: String,
    /// Whether removal was successful
    success: bool,
}

/// Runs the plugin management command.
///
/// Routes plugin actions to their respective handlers.
///
/// # Arguments
///
/// * `action` - Plugin management action to execute
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if the plugin operation fails.
///
/// # Examples
///
/// ```no_run
/// use mcp_cli::commands::plugin::{SkillAction, run};
/// use mcp_core::cli::{ExitCode, OutputFormat};
/// use std::path::PathBuf;
///
/// # async fn example() -> Result<(), anyhow::Error> {
/// let action = SkillAction::List {
///     skill_dir: PathBuf::from("./plugins"),
/// };
///
/// let result = run(action, OutputFormat::Pretty).await?;
/// assert_eq!(result, ExitCode::SUCCESS);
/// # Ok(())
/// # }
/// ```
pub async fn run(action: SkillAction, output_format: OutputFormat) -> Result<ExitCode> {
    match action {
        SkillAction::Load { name, skill_dir } => load_plugin(&name, &skill_dir, output_format),
        SkillAction::List { skill_dir } => list_plugins(&skill_dir, output_format),
        SkillAction::Remove {
            name,
            skill_dir,
            yes,
        } => remove_plugin(&name, &skill_dir, yes, output_format),
        SkillAction::Info { name, skill_dir } => show_plugin_info(&name, &skill_dir, output_format),
    }
}

/// Loads a plugin from disk.
///
/// # Errors
///
/// Returns an error if the plugin doesn't exist or fails checksum verification.
pub fn load_plugin(
    name: &str,
    skill_dir: &PathBuf,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    info!("Loading plugin: {}", name);

    let store = SkillStore::new(skill_dir).context("failed to initialize skill store")?;

    let loaded = store
        .load_skill(name)
        .with_context(|| format!("failed to load plugin '{name}'"))?;

    let result = LoadResult {
        name: loaded.metadata.server.name,
        version: loaded.metadata.server.version,
        tool_count: loaded.metadata.tools.len(),
        file_count: loaded.vfs.file_count(),
        wasm_size: loaded.wasm_module.len(),
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    info!(
        "Successfully loaded plugin: {} (v{}, {} tools)",
        result.name, result.version, result.tool_count
    );

    Ok(ExitCode::SUCCESS)
}

/// Lists all available plugins.
///
/// # Errors
///
/// Returns an error if the skill directory cannot be read.
pub fn list_plugins(skill_dir: &PathBuf, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Listing plugins in: {}", skill_dir.display());

    let store = SkillStore::new(&skill_dir).context("failed to initialize skill store")?;

    let skills = store.list_skills().context("failed to list skills")?;

    if skills.is_empty() {
        warn!("No skills found in {}", skill_dir.display());
    }

    let summaries: Vec<SkillSummary> = skills
        .iter()
        .map(|p| SkillSummary {
            name: p.server_name.clone(),
            version: p.version.clone(),
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

/// Removes a plugin from disk.
///
/// # Errors
///
/// Returns an error if the plugin doesn't exist or cannot be removed.
pub fn remove_plugin(
    name: &str,
    skill_dir: &PathBuf,
    yes: bool,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    info!("Removing plugin: {}", name);

    let store = SkillStore::new(skill_dir).context("failed to initialize skill store")?;

    // Check if plugin exists
    if !store.skill_exists(name)? {
        bail!("plugin '{name}' not found");
    }

    // Prompt for confirmation unless --yes flag is set
    if !yes {
        use dialoguer::Confirm;

        let confirmed = Confirm::new()
            .with_prompt(format!("Are you sure you want to remove plugin '{name}'?"))
            .default(false)
            .interact()
            .context("failed to read confirmation")?;

        if !confirmed {
            info!("Plugin removal cancelled by user");
            return Ok(ExitCode::SUCCESS);
        }
    }

    store
        .remove_skill(name)
        .with_context(|| format!("failed to remove plugin '{name}'"))?;

    let result = RemoveResult {
        name: name.to_string(),
        success: true,
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    info!("Successfully removed plugin: {}", name);

    Ok(ExitCode::SUCCESS)
}

/// Shows detailed information about a plugin.
///
/// # Errors
///
/// Returns an error if the plugin doesn't exist or cannot be loaded.
pub fn show_plugin_info(
    name: &str,
    skill_dir: &PathBuf,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    info!("Showing info for plugin: {}", name);

    let store = SkillStore::new(skill_dir).context("failed to initialize skill store")?;

    let loaded = store
        .load_skill(name)
        .with_context(|| format!("failed to load plugin '{name}'"))?;

    let tools: Vec<ToolSummary> = loaded
        .metadata
        .tools
        .iter()
        .map(|t| ToolSummary {
            name: t.name.clone(),
            description: t.description.clone(),
        })
        .collect();

    let result = InfoResult {
        name: loaded.metadata.server.name,
        version: loaded.metadata.server.version,
        protocol_version: loaded.metadata.server.protocol_version,
        generated_at: loaded.metadata.generated_at.to_rfc3339(),
        generator_version: loaded.metadata.generator_version,
        tool_count: loaded.metadata.tools.len(),
        file_count: loaded.vfs.file_count(),
        wasm_size: loaded.wasm_module.len(),
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
            name: "test-server".to_string(),
            version: "1.0.0".to_string(),
            tool_count: 5,
            file_count: 10,
            wasm_size: 1024,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-server"));
        assert!(json.contains("1.0.0"));
    }

    #[test]
    fn test_list_result_serialization() {
        let result = ListResult {
            skill_dir: "./plugins".to_string(),
            skill_count: 2,
            plugins: vec![
                SkillSummary {
                    name: "plugin1".to_string(),
                    version: "1.0.0".to_string(),
                    tool_count: 3,
                    generated_at: "2025-11-21T12:00:00Z".to_string(),
                },
                SkillSummary {
                    name: "plugin2".to_string(),
                    version: "2.0.0".to_string(),
                    tool_count: 5,
                    generated_at: "2025-11-21T13:00:00Z".to_string(),
                },
            ],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("plugin1"));
        assert!(json.contains("plugin2"));
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
