//! Stats command implementation.
//!
//! Displays runtime statistics and performance metrics.

use anyhow::{Context, Result};
use mcp_core::cli::{ExitCode, OutputFormat};
use serde::Serialize;
use tracing::info;

/// Cache statistics.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CacheStats {
    /// Bridge cache hit rate (0.0 - 1.0)
    pub bridge_hit_rate: f64,
    /// Runtime cache hit rate (0.0 - 1.0)
    pub runtime_hit_rate: f64,
    /// Total number of cache lookups
    pub total_calls: usize,
    /// Number of cache hits
    pub hits: usize,
    /// Number of cache misses
    pub misses: usize,
}

/// Performance statistics.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct PerformanceStats {
    /// Average execution time in milliseconds
    pub avg_execution_ms: f64,
    /// Peak memory usage in MB
    pub peak_memory_mb: f64,
    /// Total modules executed
    pub modules_executed: usize,
    /// Modules cached
    pub modules_cached: usize,
}

/// Token savings statistics.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct TokenStats {
    /// Estimated tokens saved
    pub tokens_saved: usize,
    /// Baseline token usage (without code execution)
    pub baseline_tokens: usize,
    /// Actual token usage (with code execution)
    pub actual_tokens: usize,
    /// Token reduction percentage (0.0 - 1.0)
    pub reduction_rate: f64,
}

/// Combined statistics for all categories.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct Statistics {
    /// Cache statistics
    pub cache: CacheStats,
    /// Performance statistics
    pub performance: PerformanceStats,
    /// Token statistics
    pub tokens: TokenStats,
}

/// Runs the stats command.
///
/// Displays cache statistics, runtime metrics, and performance data.
///
/// # Arguments
///
/// * `category` - Statistics category (cache, performance, tokens, all)
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if statistics retrieval fails or if an invalid category is specified.
///
/// # Examples
///
/// ```
/// use mcp_cli::commands::stats;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # tokio_test::block_on(async {
/// let result = stats::run("all".to_string(), OutputFormat::Json).await;
/// assert!(result.is_ok());
/// # })
/// ```
pub async fn run(category: String, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Stats category: {}", category);
    info!("Output format: {}", output_format);

    match category.as_str() {
        "all" => show_all_stats(output_format).await,
        "cache" => show_cache_stats(output_format).await,
        "performance" => show_performance_stats(output_format).await,
        "tokens" => show_token_stats(output_format).await,
        invalid => Err(anyhow::anyhow!(
            "invalid category '{invalid}' (must be: all, cache, performance, tokens)"
        )),
    }
}

/// Shows all statistics.
async fn show_all_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = get_all_stats();
    let formatted = crate::formatters::format_output(&stats, output_format)
        .context("failed to format statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Shows cache statistics.
async fn show_cache_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = get_cache_stats();
    let formatted = crate::formatters::format_output(&stats, output_format)
        .context("failed to format cache statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Shows performance statistics.
async fn show_performance_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = get_performance_stats();
    let formatted = crate::formatters::format_output(&stats, output_format)
        .context("failed to format performance statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Shows token statistics.
async fn show_token_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = get_token_stats();
    let formatted = crate::formatters::format_output(&stats, output_format)
        .context("failed to format token statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Gets all statistics.
///
/// For MVP, returns stub data. Real metrics collection will be added in Phase 7.4.
const fn get_all_stats() -> Statistics {
    Statistics {
        cache: get_cache_stats(),
        performance: get_performance_stats(),
        tokens: get_token_stats(),
    }
}

/// Gets cache statistics.
///
/// For MVP, returns stub data showing demonstration values.
const fn get_cache_stats() -> CacheStats {
    CacheStats {
        bridge_hit_rate: 0.85,
        runtime_hit_rate: 0.92,
        total_calls: 1000,
        hits: 880,
        misses: 120,
    }
}

/// Gets performance statistics.
///
/// For MVP, returns stub data showing demonstration values.
const fn get_performance_stats() -> PerformanceStats {
    PerformanceStats {
        avg_execution_ms: 12.5,
        peak_memory_mb: 128.0,
        modules_executed: 45,
        modules_cached: 42,
    }
}

/// Gets token statistics.
///
/// For MVP, returns stub data showing demonstration values.
const fn get_token_stats() -> TokenStats {
    TokenStats {
        tokens_saved: 45000,
        baseline_tokens: 50000,
        actual_tokens: 5000,
        reduction_rate: 0.90,
    }
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
    async fn test_stats_cache_category() {
        let result = run("cache".to_string(), OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_stats_performance_category() {
        let result = run("performance".to_string(), OutputFormat::Text).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_stats_tokens_category() {
        let result = run("tokens".to_string(), OutputFormat::Pretty).await;
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

    #[test]
    fn test_cache_stats_values() {
        let stats = get_cache_stats();
        assert!(stats.bridge_hit_rate >= 0.0 && stats.bridge_hit_rate <= 1.0);
        assert!(stats.runtime_hit_rate >= 0.0 && stats.runtime_hit_rate <= 1.0);
        assert!(stats.total_calls > 0);
        assert_eq!(stats.hits + stats.misses, stats.total_calls);
    }

    #[test]
    fn test_performance_stats_values() {
        let stats = get_performance_stats();
        assert!(stats.avg_execution_ms > 0.0);
        assert!(stats.peak_memory_mb > 0.0);
        assert!(stats.modules_executed >= stats.modules_cached);
    }

    #[test]
    fn test_token_stats_values() {
        let stats = get_token_stats();
        assert!(stats.reduction_rate >= 0.0 && stats.reduction_rate <= 1.0);
        assert!(stats.baseline_tokens > stats.actual_tokens);
        assert!(stats.tokens_saved > 0);
    }

    #[test]
    fn test_cache_stats_serialization() {
        let stats = get_cache_stats();
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("bridge_hit_rate"));
        assert!(json.contains("runtime_hit_rate"));
        assert!(json.contains("total_calls"));
    }

    #[test]
    fn test_performance_stats_serialization() {
        let stats = get_performance_stats();
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("avg_execution_ms"));
        assert!(json.contains("peak_memory_mb"));
        assert!(json.contains("modules_executed"));
    }

    #[test]
    fn test_token_stats_serialization() {
        let stats = get_token_stats();
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("tokens_saved"));
        assert!(json.contains("baseline_tokens"));
        assert!(json.contains("reduction_rate"));
    }

    #[test]
    fn test_all_stats_serialization() {
        let stats = get_all_stats();
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("cache"));
        assert!(json.contains("performance"));
        assert!(json.contains("tokens"));
    }
}
