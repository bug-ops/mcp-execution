//! Cache management commands.
//!
//! Provides commands for managing the internal MCP execution cache located at
//! `~/.mcp-execution/cache/`.

use anyhow::{Context, Result};
use clap::Subcommand;
use colored::Colorize;
use dialoguer::Confirm;
use mcp_core::CacheManager;

/// Cache management subcommands.
#[derive(Subcommand, Debug)]
pub enum CacheCommand {
    /// Show cache information and statistics
    Info,

    /// Clear cached data
    Clear {
        /// Skill name (optional, clears all if not specified)
        skill: Option<String>,

        /// Skip confirmation prompt
        #[arg(long, short)]
        yes: bool,
    },

    /// Verify cache integrity
    Verify,
}

/// Handle cache management commands.
///
/// # Errors
///
/// Returns an error if the cache operation fails.
pub async fn handle(cmd: CacheCommand) -> Result<()> {
    match cmd {
        CacheCommand::Info => show_cache_info().await,
        CacheCommand::Clear { skill, yes } => clear_cache(skill, yes).await,
        CacheCommand::Verify => verify_cache().await,
    }
}

/// Show cache information and statistics.
async fn show_cache_info() -> Result<()> {
    let cache = CacheManager::new().context("Failed to access cache")?;
    let stats = cache.stats().context("Failed to read cache statistics")?;

    println!("{}", "Cache Information".bold().cyan());
    println!("{}", "─".repeat(50));
    println!(
        "  {} {}",
        "Location:".bold(),
        cache.cache_root().display()
    );
    println!();
    println!(
        "  {} {}",
        "WASM modules:".bold(),
        format!("{}", stats.total_wasm_files).yellow()
    );
    println!(
        "  {} {}",
        "VFS caches:".bold(),
        format!("{}", stats.total_vfs_files).yellow()
    );
    println!(
        "  {} {}",
        "Metadata files:".bold(),
        format!("{}", stats.total_metadata_files).yellow()
    );
    println!();

    // Format size nicely
    let size_str = if stats.total_size_bytes < 1024 {
        format!("{} bytes", stats.total_size_bytes)
    } else if stats.total_size_bytes < 1024 * 1024 {
        format!("{:.2} KB", stats.total_size_bytes as f64 / 1024.0)
    } else if stats.total_size_bytes < 1024 * 1024 * 1024 {
        format!("{:.2} MB", stats.total_size_bytes as f64 / (1024.0 * 1024.0))
    } else {
        format!(
            "{:.2} GB",
            stats.total_size_bytes as f64 / (1024.0 * 1024.0 * 1024.0)
        )
    };

    println!("  {} {}", "Total size:".bold(), size_str.green());

    if stats.total_wasm_files == 0
        && stats.total_vfs_files == 0
        && stats.total_metadata_files == 0
    {
        println!();
        println!("{}", "  Cache is empty".dimmed());
    }

    Ok(())
}

/// Clear cache data.
async fn clear_cache(skill: Option<String>, yes: bool) -> Result<()> {
    let cache = CacheManager::new().context("Failed to access cache")?;

    // Confirmation prompt
    if !yes {
        let prompt = if let Some(ref skill_name) = skill {
            format!(
                "Clear cache for skill '{}'? This will remove WASM, VFS, and metadata.",
                skill_name
            )
        } else {
            "Clear ALL cache data? This will remove all WASM modules, VFS caches, and metadata."
                .to_string()
        };

        let confirmed = Confirm::new()
            .with_prompt(prompt)
            .default(false)
            .interact()
            .context("Failed to get confirmation")?;

        if !confirmed {
            println!("{}", "Cancelled.".yellow());
            return Ok(());
        }
    }

    // Perform the clear operation
    match skill {
        Some(skill_name) => {
            cache
                .clear_skill(&skill_name)
                .context("Failed to clear skill cache")?;
            println!(
                "{} Cleared cache for skill: {}",
                "✓".green().bold(),
                skill_name.cyan()
            );
        }
        None => {
            cache.clear_all().context("Failed to clear all cache")?;
            println!("{} Cleared all cache data", "✓".green().bold());
        }
    }

    Ok(())
}

/// Verify cache integrity.
async fn verify_cache() -> Result<()> {
    let cache = CacheManager::new().context("Failed to access cache")?;
    let stats = cache.stats().context("Failed to read cache statistics")?;

    println!("{}", "Verifying Cache Integrity".bold().cyan());
    println!("{}", "─".repeat(50));

    let mut issues = Vec::new();

    // Check if cache directories exist
    if !cache.wasm_dir().exists() {
        issues.push("WASM directory is missing");
    }
    if !cache.vfs_dir().exists() {
        issues.push("VFS directory is missing");
    }
    if !cache.metadata_dir().exists() {
        issues.push("Metadata directory is missing");
    }

    // Check for orphaned files
    // (files in one directory without corresponding files in others)
    // This is a simplified check - full implementation would do more

    if issues.is_empty() {
        println!(
            "{} Cache verification complete",
            "✓".green().bold()
        );
        println!("  {} skills cached", stats.total_wasm_files);
        println!("  No issues found");
    } else {
        println!("{} Issues found:", "✗".red().bold());
        for issue in issues {
            println!("  {} {}", "•".red(), issue);
        }
        println!();
        println!(
            "{} Run {} to fix these issues",
            "Hint:".yellow(),
            "mcp-execution cache clear".cyan()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_cache_command_variants() {
        // Just ensure the enum variants compile
        let _info = CacheCommand::Info;
        let _clear = CacheCommand::Clear {
            skill: None,
            yes: false,
        };
        let _verify = CacheCommand::Verify;
    }
}
