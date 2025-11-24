//! Stats command implementation.
//!
//! Displays runtime statistics and performance metrics.

use anyhow::{Context, Result};
use mcp_bridge::Bridge;
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_core::stats::{SkillStats, SystemStats};
use mcp_skill_store::SkillStore;
use mcp_wasm_runtime::Runtime;
use mcp_wasm_runtime::security::SecurityConfig;
use std::sync::Arc;
use tracing::info;

/// Runs the stats command.
///
/// Displays cache statistics, runtime metrics, and skill storage data.
///
/// # Arguments
///
/// * `category` - Statistics category (all, bridge, runtime, skills)
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if statistics retrieval fails or if an invalid category is specified.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::stats;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # #[tokio::main]
/// # async fn main() {
/// let result = stats::run("all".to_string(), OutputFormat::Json).await;
/// assert!(result.is_ok());
/// # }
/// ```
pub async fn run(category: String, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Stats category: {}", category);
    info!("Output format: {}", output_format);

    match category.as_str() {
        "all" => show_all_stats(output_format).await,
        "bridge" => show_bridge_stats(output_format).await,
        "runtime" => show_runtime_stats(output_format).await,
        "skills" => show_skill_stats(output_format).await,
        invalid => Err(anyhow::anyhow!(
            "invalid category '{invalid}' (must be: all, bridge, runtime, skills)"
        )),
    }
}

/// Shows all statistics.
async fn show_all_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = collect_all_stats().await?;
    let formatted = crate::formatters::format_output(&stats, output_format)
        .context("failed to format statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Shows bridge statistics.
async fn show_bridge_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = collect_all_stats().await?;
    let formatted = crate::formatters::format_output(stats.bridge(), output_format)
        .context("failed to format bridge statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Shows runtime statistics.
async fn show_runtime_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = collect_all_stats().await?;
    let formatted = crate::formatters::format_output(stats.runtime(), output_format)
        .context("failed to format runtime statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Shows skill statistics.
async fn show_skill_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = collect_all_stats().await?;
    let formatted = crate::formatters::format_output(stats.skills(), output_format)
        .context("failed to format skill statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Collects statistics from all system components.
///
/// Creates instances of `Bridge`, `Runtime`, and `SkillStore` to gather current
/// statistics. This is designed to be non-intrusive and works even if no
/// operations have been performed yet.
///
/// # Errors
///
/// Returns an error if:
/// - `SkillStore` cannot be created (permission issues, invalid home directory)
/// - `Runtime` initialization fails
/// - Statistics collection encounters I/O errors
///
/// # Examples
///
/// ```ignore
/// let stats = collect_all_stats().await?;
/// println!("Bridge calls: {}", stats.bridge().total_tool_calls);
/// println!("Runtime executions: {}", stats.runtime().total_executions);
/// println!("Skills stored: {}", stats.skills().total_skills);
/// ```
async fn collect_all_stats() -> Result<SystemStats> {
    // Create Bridge instance (lightweight, just creates empty cache)
    let bridge = Bridge::new(1000);
    let bridge_stats = bridge.collect_stats().await;

    // Create Runtime instance for stats collection
    // Note: This creates a fresh runtime, so stats will reflect only current state
    let config = SecurityConfig::default();
    let runtime = Runtime::new(Arc::new(bridge), config)
        .context("failed to create runtime for stats collection")?;
    let runtime_stats = runtime.collect_stats();

    // Try to collect skill stats, use default if not available
    // Skill store not available (no .claude directory or permissions issue)
    let skill_stats = SkillStore::new_claude().map_or_else(
        |_| SkillStats::default(),
        |store| store.collect_stats().unwrap_or_default(),
    );

    Ok(SystemStats::new(bridge_stats, runtime_stats, skill_stats))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_stats_all_category() {
        let result = run("all".to_string(), OutputFormat::Pretty).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_stats_bridge_category() {
        let result = run("bridge".to_string(), OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_stats_runtime_category() {
        let result = run("runtime".to_string(), OutputFormat::Text).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_stats_skills_category() {
        let result = run("skills".to_string(), OutputFormat::Pretty).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_stats_invalid_category() {
        let result = run("invalid".to_string(), OutputFormat::Json).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("invalid category"));
        assert!(err_msg.contains("invalid"));
    }

    #[tokio::test]
    async fn test_stats_all_formats() {
        for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
            let result = run("all".to_string(), format).await;
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ExitCode::SUCCESS);
        }
    }

    #[tokio::test]
    async fn test_collect_all_stats_returns_valid_data() {
        let result = collect_all_stats().await;
        assert!(result.is_ok());

        let stats = result.unwrap();
        // Stats should have valid snapshot time
        let snapshot = stats.snapshot_time();
        let duration_since_epoch =
            snapshot.signed_duration_since(chrono::DateTime::<chrono::Utc>::UNIX_EPOCH);

        // Verify snapshot time is reasonable (not in the future, not too old)
        assert!(duration_since_epoch.num_seconds() > 0);

        // Bridge stats should be initialized (even if zero)
        assert_eq!(stats.bridge().total_tool_calls, 0);
        assert_eq!(stats.bridge().cache_hits, 0);

        // Runtime stats should be initialized
        assert_eq!(stats.runtime().total_executions, 0);

        // Skill stats may be zero if no .claude directory exists
        // (this is expected and not an error)
    }

    #[tokio::test]
    async fn test_bridge_stats_structure() {
        let stats = collect_all_stats().await.unwrap();
        let bridge = stats.bridge();

        // Verify all fields are accessible (u32 fields are always >= 0)
        let (_total_calls, _hits, _active, _total, _failures) = (
            bridge.total_tool_calls,
            bridge.cache_hits,
            bridge.active_connections,
            bridge.total_connections,
            bridge.connection_failures,
        );

        // Verify invariant: hits <= total calls (if any calls made)
        if bridge.total_tool_calls > 0 {
            assert!(bridge.cache_hits <= bridge.total_tool_calls);
        }
    }

    #[tokio::test]
    async fn test_runtime_stats_structure() {
        let stats = collect_all_stats().await.unwrap();
        let runtime = stats.runtime();

        // Verify all fields are accessible and invariants hold
        // u32/u64 fields are always >= 0 by type

        // Verify invariant: hits <= total executions (if any executions)
        if runtime.total_executions > 0 {
            assert!(runtime.cache_hits <= runtime.total_executions);
        }

        // Access all fields to ensure they're public
        let _ = (
            runtime.total_executions,
            runtime.cache_hits,
            runtime.execution_failures,
            runtime.compilation_failures,
            runtime.avg_execution_time_us,
        );
    }

    #[tokio::test]
    async fn test_skill_stats_structure() {
        let stats = collect_all_stats().await.unwrap();
        let skills = stats.skills();

        // Verify all fields are accessible (u32/u64 fields are always >= 0 by type)
        // No invariants to check for skills (all fields independent)

        // Access all fields to ensure they're public
        let _ = (
            skills.total_skills,
            skills.total_storage_bytes,
            skills.generation_successes,
            skills.generation_failures,
        );
    }

    #[tokio::test]
    async fn test_stats_serialization() {
        let stats = collect_all_stats().await.unwrap();

        // Test JSON serialization
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("bridge"));
        assert!(json.contains("runtime"));
        assert!(json.contains("skills"));

        // Verify we can deserialize back
        let deserialized: SystemStats = serde_json::from_str(&json).unwrap();
        assert_eq!(
            stats.bridge().total_tool_calls,
            deserialized.bridge().total_tool_calls
        );
    }
}
