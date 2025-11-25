//! Stats command implementation.
//!
//! Displays runtime statistics and performance metrics for MCP Bridge.

use anyhow::Result;
use mcp_bridge::Bridge;
use mcp_core::cli::{ExitCode, OutputFormat};
use std::sync::Arc;
use tracing::info;

/// Runs the stats command.
///
/// Displays cache statistics for MCP Bridge.
///
/// # Arguments
///
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if statistics retrieval fails.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::stats;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # #[tokio::main]
/// # async fn main() {
/// let result = stats::run(OutputFormat::Json).await;
/// assert!(result.is_ok());
/// # }
/// ```
pub async fn run(output_format: OutputFormat) -> Result<ExitCode> {
    info!("Output format: {}", output_format);

    let stats = get_bridge_stats().await?;

    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&stats)?);
        }
        OutputFormat::Text | OutputFormat::Pretty => {
            print_bridge_stats(&stats);
        }
    }

    Ok(ExitCode::SUCCESS)
}

/// Gets bridge cache statistics.
async fn get_bridge_stats() -> Result<mcp_bridge::CacheStats> {
    // Create a temporary bridge to get stats
    // In a real scenario, this would connect to an existing bridge instance
    let bridge = Arc::new(Bridge::new(1000));

    Ok(bridge.cache_stats().await)
}

/// Prints bridge statistics in human-readable format.
fn print_bridge_stats(stats: &mcp_bridge::CacheStats) {
    println!("=== MCP Bridge Cache Statistics ===");
    println!("Cache Size: {} entries", stats.size);
    println!("Cache Capacity: {} entries", stats.capacity);
    #[allow(clippy::cast_precision_loss)]
    let usage = (stats.size as f64 / stats.capacity as f64) * 100.0;
    println!("Cache Usage: {usage:.1}%");
    println!("Cache Enabled: {}", stats.enabled);
    println!("Total Tool Calls: {}", stats.total_tool_calls);
    println!("Cache Hits: {}", stats.cache_hits);

    if stats.total_tool_calls > 0 {
        let hit_rate = (f64::from(stats.cache_hits) / f64::from(stats.total_tool_calls)) * 100.0;
        println!("Cache Hit Rate: {hit_rate:.1}%");
    } else {
        println!("Cache Hit Rate: N/A (no tool calls yet)");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_get_bridge_stats() {
        let result = get_bridge_stats().await;
        assert!(result.is_ok());

        let stats = result.unwrap();
        assert!(stats.capacity > 0);
    }

    #[tokio::test]
    async fn test_run_json() {
        let result = run(OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_run_text() {
        let result = run(OutputFormat::Text).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_run_pretty() {
        let result = run(OutputFormat::Pretty).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}
