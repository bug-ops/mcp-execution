//! Debug command implementation.
//!
//! Provides debugging utilities and diagnostic information.

use crate::actions::DebugAction;
use anyhow::{Context, Result};
use mcp_core::cli::{ExitCode, OutputFormat};
use serde::Serialize;
use tracing::info;

/// System and environment debug information.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct DebugInfo {
    /// Application version
    pub version: String,
    /// Rust compiler version
    pub rust_version: String,
    /// Operating system
    pub os: String,
    /// CPU architecture
    pub arch: String,
    /// Enabled features
    pub features: Vec<String>,
    /// Target triple
    pub target: String,
}

/// Detailed cache statistics for debugging.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct CacheStatistics {
    /// Bridge cache size in entries
    pub bridge_cache_size: usize,
    /// Runtime module cache size in entries
    pub runtime_cache_size: usize,
    /// Total cache evictions
    pub evictions: usize,
    /// Cache memory usage in bytes
    pub memory_bytes: usize,
}

/// Runtime performance metrics.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct RuntimeMetrics {
    /// Uptime in seconds
    pub uptime_seconds: f64,
    /// Total requests processed
    pub total_requests: usize,
    /// Active WASM instances
    pub active_instances: usize,
    /// Average request duration in milliseconds
    pub avg_request_ms: f64,
}

/// Runs the debug command.
///
/// Displays system information, cache stats, and runtime metrics.
///
/// # Arguments
///
/// * `action` - Debug action to perform
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if debug operation fails.
///
/// # Examples
///
/// ```
/// use mcp_cli::commands::debug;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # tokio_test::block_on(async {
/// let result = debug::run(
///     mcp_cli::DebugAction::Info,
///     OutputFormat::Json
/// ).await;
/// assert!(result.is_ok());
/// # })
/// ```
pub async fn run(action: DebugAction, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Debug action: {:?}", action);
    info!("Output format: {}", output_format);

    match action {
        DebugAction::Info => show_debug_info(output_format).await,
        DebugAction::CacheStats => show_cache_stats(output_format).await,
        DebugAction::RuntimeMetrics => show_runtime_metrics(output_format).await,
    }
}

/// Shows system and environment debug information.
async fn show_debug_info(output_format: OutputFormat) -> Result<ExitCode> {
    let info = get_debug_info();
    let formatted =
        crate::formatters::format_output(&info, output_format).context("failed to format info")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Shows detailed cache statistics.
async fn show_cache_stats(output_format: OutputFormat) -> Result<ExitCode> {
    let stats = get_cache_statistics();
    let formatted = crate::formatters::format_output(&stats, output_format)
        .context("failed to format cache statistics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Shows runtime performance metrics.
async fn show_runtime_metrics(output_format: OutputFormat) -> Result<ExitCode> {
    let metrics = get_runtime_metrics();
    let formatted = crate::formatters::format_output(&metrics, output_format)
        .context("failed to format runtime metrics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Gets system and environment debug information.
///
/// For MVP, returns compile-time information. Dynamic metrics will be added in Phase 7.4.
fn get_debug_info() -> DebugInfo {
    // Note: mcp-cli doesn't have feature flags currently
    // This is kept for future extensibility
    let features = Vec::new();

    DebugInfo {
        version: env!("CARGO_PKG_VERSION").to_string(),
        rust_version: std::env!("CARGO_PKG_RUST_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        features,
        target: get_target_triple(),
    }
}

/// Gets the target triple.
fn get_target_triple() -> String {
    format!(
        "{}-{}-{}",
        std::env::consts::ARCH,
        std::env::consts::OS,
        std::env::consts::FAMILY
    )
}

/// Gets cache statistics.
///
/// For MVP, returns stub data. Real cache introspection will be added in Phase 7.4.
const fn get_cache_statistics() -> CacheStatistics {
    CacheStatistics {
        bridge_cache_size: 42,
        runtime_cache_size: 15,
        evictions: 7,
        memory_bytes: 1024 * 1024 * 5, // 5 MB
    }
}

/// Gets runtime performance metrics.
///
/// For MVP, returns stub data. Real metrics collection will be added in Phase 7.4.
const fn get_runtime_metrics() -> RuntimeMetrics {
    RuntimeMetrics {
        uptime_seconds: 3600.0,
        total_requests: 1250,
        active_instances: 3,
        avg_request_ms: 15.5,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_debug_info_success() {
        let result = run(DebugAction::Info, OutputFormat::Pretty).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_debug_cache_stats_success() {
        let result = run(DebugAction::CacheStats, OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_debug_runtime_metrics_success() {
        let result = run(DebugAction::RuntimeMetrics, OutputFormat::Text).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_debug_all_actions_all_formats() {
        let actions = [
            DebugAction::Info,
            DebugAction::CacheStats,
            DebugAction::RuntimeMetrics,
        ];
        let formats = [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty];

        for action in actions {
            for format in formats {
                let result = run(action.clone(), format).await;
                assert!(result.is_ok());
                assert_eq!(result.unwrap(), ExitCode::SUCCESS);
            }
        }
    }

    #[test]
    fn test_debug_info_values() {
        let info = get_debug_info();
        assert!(!info.version.is_empty());
        assert!(!info.rust_version.is_empty());
        assert!(!info.os.is_empty());
        assert!(!info.arch.is_empty());
        assert!(!info.target.is_empty());
    }

    #[test]
    fn test_cache_statistics_values() {
        let stats = get_cache_statistics();
        assert!(stats.bridge_cache_size > 0);
        assert!(stats.runtime_cache_size > 0);
        assert!(stats.memory_bytes > 0);
    }

    #[test]
    fn test_runtime_metrics_values() {
        let metrics = get_runtime_metrics();
        assert!(metrics.uptime_seconds > 0.0);
        assert!(metrics.total_requests > 0);
        assert!(metrics.avg_request_ms > 0.0);
    }

    #[test]
    fn test_debug_info_serialization() {
        let info = get_debug_info();
        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("version"));
        assert!(json.contains("rust_version"));
        assert!(json.contains("os"));
        assert!(json.contains("arch"));
        assert!(json.contains("features"));
        assert!(json.contains("target"));
    }

    #[test]
    fn test_cache_statistics_serialization() {
        let stats = get_cache_statistics();
        let json = serde_json::to_string(&stats).unwrap();
        assert!(json.contains("bridge_cache_size"));
        assert!(json.contains("runtime_cache_size"));
        assert!(json.contains("evictions"));
        assert!(json.contains("memory_bytes"));
    }

    #[test]
    fn test_runtime_metrics_serialization() {
        let metrics = get_runtime_metrics();
        let json = serde_json::to_string(&metrics).unwrap();
        assert!(json.contains("uptime_seconds"));
        assert!(json.contains("total_requests"));
        assert!(json.contains("active_instances"));
        assert!(json.contains("avg_request_ms"));
    }

    #[test]
    fn test_target_triple_format() {
        let target = get_target_triple();
        assert!(target.contains('-'));
        // Should be in format: arch-os-family
        let parts: Vec<&str> = target.split('-').collect();
        assert_eq!(parts.len(), 3);
    }
}
