//! Plugin management command implementation.
//!
//! Provides commands to save, load, list, and manage plugins saved to disk.
//! Plugins are stored in a directory structure with VFS files and WASM modules.

use anyhow::{Context, Result, bail};
use clap::Subcommand;
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_plugin_store::PluginStore;
use serde::Serialize;
use std::path::PathBuf;
use tracing::{info, warn};

/// Plugin management actions.
#[derive(Subcommand, Debug)]
pub enum PluginAction {
    /// Load a plugin from disk
    Load {
        /// Plugin name (server name)
        name: String,

        /// Plugin directory (defaults to ./plugins)
        #[arg(long, default_value = "./plugins")]
        plugin_dir: PathBuf,
    },

    /// List available plugins
    List {
        /// Plugin directory
        #[arg(long, default_value = "./plugins")]
        plugin_dir: PathBuf,
    },

    /// Remove a plugin
    Remove {
        /// Plugin name
        name: String,

        /// Plugin directory
        #[arg(long, default_value = "./plugins")]
        plugin_dir: PathBuf,

        /// Skip confirmation
        #[arg(short = 'y', long)]
        yes: bool,
    },

    /// Show plugin information
    Info {
        /// Plugin name
        name: String,

        /// Plugin directory
        #[arg(long, default_value = "./plugins")]
        plugin_dir: PathBuf,
    },
}

/// Result of loading a plugin.
#[derive(Debug, Serialize)]
struct LoadResult {
    /// Plugin name
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

/// Result of listing plugins.
#[derive(Debug, Serialize)]
struct ListResult {
    /// Plugin directory
    plugin_dir: String,
    /// Number of plugins found
    plugin_count: usize,
    /// Plugin information
    plugins: Vec<PluginSummary>,
}

/// Summary of a plugin for listing.
#[derive(Debug, Serialize)]
struct PluginSummary {
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
    /// Plugin name
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
    /// Plugin name
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
/// use mcp_cli::commands::plugin::{PluginAction, run};
/// use mcp_core::cli::{ExitCode, OutputFormat};
/// use std::path::PathBuf;
///
/// # async fn example() -> Result<(), anyhow::Error> {
/// let action = PluginAction::List {
///     plugin_dir: PathBuf::from("./plugins"),
/// };
///
/// let result = run(action, OutputFormat::Pretty).await?;
/// assert_eq!(result, ExitCode::SUCCESS);
/// # Ok(())
/// # }
/// ```
pub async fn run(action: PluginAction, output_format: OutputFormat) -> Result<ExitCode> {
    match action {
        PluginAction::Load { name, plugin_dir } => load_plugin(&name, &plugin_dir, output_format),
        PluginAction::List { plugin_dir } => list_plugins(&plugin_dir, output_format),
        PluginAction::Remove {
            name,
            plugin_dir,
            yes,
        } => remove_plugin(&name, &plugin_dir, yes, output_format),
        PluginAction::Info { name, plugin_dir } => {
            show_plugin_info(&name, &plugin_dir, output_format)
        }
    }
}

/// Loads a plugin from disk.
///
/// # Errors
///
/// Returns an error if the plugin doesn't exist or fails checksum verification.
fn load_plugin(name: &str, plugin_dir: &PathBuf, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Loading plugin: {}", name);

    let store = PluginStore::new(plugin_dir).context("failed to initialize plugin store")?;

    let loaded = store
        .load_plugin(name)
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
/// Returns an error if the plugin directory cannot be read.
fn list_plugins(plugin_dir: &PathBuf, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Listing plugins in: {}", plugin_dir.display());

    let store = PluginStore::new(&plugin_dir).context("failed to initialize plugin store")?;

    let plugins = store.list_plugins().context("failed to list plugins")?;

    if plugins.is_empty() {
        warn!("No plugins found in {}", plugin_dir.display());
    }

    let summaries: Vec<PluginSummary> = plugins
        .iter()
        .map(|p| PluginSummary {
            name: p.server_name.clone(),
            version: p.version.clone(),
            tool_count: p.tool_count,
            generated_at: p.generated_at.to_rfc3339(),
        })
        .collect();

    let result = ListResult {
        plugin_dir: plugin_dir.display().to_string(),
        plugin_count: plugins.len(),
        plugins: summaries,
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    info!("Found {} plugin(s)", result.plugin_count);

    Ok(ExitCode::SUCCESS)
}

/// Removes a plugin from disk.
///
/// # Errors
///
/// Returns an error if the plugin doesn't exist or cannot be removed.
fn remove_plugin(
    name: &str,
    plugin_dir: &PathBuf,
    yes: bool,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    info!("Removing plugin: {}", name);

    let store = PluginStore::new(plugin_dir).context("failed to initialize plugin store")?;

    // Check if plugin exists
    if !store.plugin_exists(name)? {
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
        .remove_plugin(name)
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
fn show_plugin_info(
    name: &str,
    plugin_dir: &PathBuf,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    info!("Showing info for plugin: {}", name);

    let store = PluginStore::new(plugin_dir).context("failed to initialize plugin store")?;

    let loaded = store
        .load_plugin(name)
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
            plugin_dir: "./plugins".to_string(),
            plugin_count: 2,
            plugins: vec![
                PluginSummary {
                    name: "plugin1".to_string(),
                    version: "1.0.0".to_string(),
                    tool_count: 3,
                    generated_at: "2025-11-21T12:00:00Z".to_string(),
                },
                PluginSummary {
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
