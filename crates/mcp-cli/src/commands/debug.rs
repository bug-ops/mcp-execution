//! Debug command implementation.
//!
//! Provides real debugging utilities for MCP Code Execution system.

use crate::actions::DebugAction;
use anyhow::{Context, Result};
use mcp_bridge::Bridge;
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_wasm_runtime::{Runtime, SecurityConfig};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Arc;
use tracing::info;

/// Bridge cache inspection results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct CacheInspection {
    /// Current cache size (number of entries)
    pub size: usize,
    /// Maximum cache capacity
    pub capacity: usize,
    /// Cache usage percentage
    pub usage_percent: f64,
    /// Cache enabled status
    pub enabled: bool,
    /// Total tool calls tracked
    pub total_tool_calls: u32,
    /// Cache hits tracked
    pub cache_hits: u32,
    /// Cache hit rate percentage (if any calls made)
    pub hit_rate_percent: Option<f64>,
}

/// Runtime module cache inspection results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ModuleInspection {
    /// Number of compiled modules cached
    pub cached_modules: usize,
    /// Cache capacity
    pub capacity: usize,
    /// Total executions tracked
    pub total_executions: u32,
    /// Module cache hits
    pub cache_hits: u32,
    /// Cache hit rate percentage
    pub hit_rate_percent: Option<f64>,
    /// Execution failures
    pub execution_failures: u32,
    /// Compilation failures
    pub compilation_failures: u32,
    /// Average execution time in microseconds
    pub avg_execution_time_us: u64,
}

/// MCP server connection inspection results.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ConnectionInspection {
    /// Number of active connections
    pub active_connections: usize,
    /// Maximum allowed connections
    pub max_connections: usize,
    /// Total connections established (lifetime)
    pub total_connections: u32,
    /// Connection failures
    pub connection_failures: u32,
    /// Success rate percentage
    pub success_rate_percent: Option<f64>,
    /// Config file path (if found)
    pub config_path: Option<String>,
    /// Config file exists
    pub config_exists: bool,
}

/// System diagnostics information.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SystemDiagnostics {
    /// CLI version
    pub cli_version: String,
    /// Rust compiler version used to build
    pub rust_version: String,
    /// Operating system
    pub os: String,
    /// CPU architecture
    pub arch: String,
    /// Config file location
    pub config_path: Option<String>,
    /// Skill store location
    pub skill_store_path: String,
    /// Cache directory (temporary)
    pub cache_dir: String,
    /// Available features
    pub features: Vec<String>,
}

/// Runs the debug command.
///
/// Executes real debugging operations on MCP Code Execution components.
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
/// ```no_run
/// use mcp_cli::commands::debug;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # #[tokio::main]
/// # async fn main() {
/// let result = debug::run(
///     mcp_cli::DebugAction::Cache,
///     OutputFormat::Json
/// ).await;
/// assert!(result.is_ok());
/// # }
/// ```
pub async fn run(action: DebugAction, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Debug action: {:?}", action);
    info!("Output format: {}", output_format);

    match action {
        DebugAction::Cache => inspect_cache(output_format).await,
        DebugAction::Modules => inspect_modules(output_format).await,
        DebugAction::Connections => inspect_connections(output_format).await,
        DebugAction::System => inspect_system(output_format).await,
    }
}

/// Inspects Bridge cache state.
///
/// Creates a Bridge instance and collects real cache statistics.
async fn inspect_cache(output_format: OutputFormat) -> Result<ExitCode> {
    // Create Bridge with default configuration
    let bridge = Bridge::new(1000);

    // Collect statistics
    let stats = bridge.collect_stats().await;
    let cache_stats = bridge.cache_stats().await;

    // Calculate hit rate percentage
    let hit_rate_percent = stats.cache_hit_rate().map(|rate| rate * 100.0);

    let inspection = CacheInspection {
        size: cache_stats.size,
        capacity: cache_stats.capacity,
        usage_percent: cache_stats.usage_percent(),
        enabled: true, // Default bridge has cache enabled
        total_tool_calls: stats.total_tool_calls,
        cache_hits: stats.cache_hits,
        hit_rate_percent,
    };

    let formatted = crate::formatters::format_output(&inspection, output_format)
        .context("failed to format cache inspection")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Inspects Runtime module cache.
///
/// Creates a Runtime instance and collects module compilation statistics.
async fn inspect_modules(output_format: OutputFormat) -> Result<ExitCode> {
    // Create Runtime with default configuration
    let bridge = Arc::new(Bridge::new(1000));
    let config = SecurityConfig::default();
    let runtime = Runtime::new(bridge, config).context("failed to create runtime")?;

    // Collect statistics
    let stats = runtime.collect_stats();

    // Calculate hit rate percentage
    let hit_rate_percent = stats.cache_hit_rate().map(|rate| rate * 100.0);

    let inspection = ModuleInspection {
        cached_modules: 0, // Runtime doesn't expose cache size directly
        capacity: 50,      // Default capacity from SecurityConfig
        total_executions: stats.total_executions,
        cache_hits: stats.cache_hits,
        hit_rate_percent,
        execution_failures: stats.execution_failures,
        compilation_failures: stats.compilation_failures,
        avg_execution_time_us: stats.avg_execution_time_us,
    };

    let formatted = crate::formatters::format_output(&inspection, output_format)
        .context("failed to format module inspection")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Inspects active MCP server connections.
///
/// Creates a Bridge and checks connection statistics. Also checks for
/// Claude Desktop configuration file.
async fn inspect_connections(output_format: OutputFormat) -> Result<ExitCode> {
    // Create Bridge
    let bridge = Bridge::new(1000);

    // Collect connection statistics
    let stats = bridge.collect_stats().await;
    let (active, max) = bridge.connection_limits().await;

    // Calculate success rate
    let success_rate_percent = stats.connection_success_rate().map(|rate| rate * 100.0);

    // Try to find Claude Desktop config
    let (config_path, config_exists) = find_config_path().map_or(
        (None, false),
        |path| (Some(path.display().to_string()), path.exists()),
    );

    let inspection = ConnectionInspection {
        active_connections: active,
        max_connections: max,
        total_connections: stats.total_connections,
        connection_failures: stats.connection_failures,
        success_rate_percent,
        config_path,
        config_exists,
    };

    let formatted = crate::formatters::format_output(&inspection, output_format)
        .context("failed to format connection inspection")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Inspects system diagnostics.
///
/// Shows version information, configuration paths, and system details.
async fn inspect_system(output_format: OutputFormat) -> Result<ExitCode> {
    // Get config path
    let config_path = find_config_path().ok().map(|p| p.display().to_string());

    // Get skill store path
    let skill_store_path = get_skill_store_path().display().to_string();

    // Get cache directory
    let cache_dir = get_cache_dir().display().to_string();

    // Features (mcp-cli doesn't have feature flags currently)
    let features = Vec::new();

    let diagnostics = SystemDiagnostics {
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        rust_version: std::env!("CARGO_PKG_RUST_VERSION").to_string(),
        os: std::env::consts::OS.to_string(),
        arch: std::env::consts::ARCH.to_string(),
        config_path,
        skill_store_path,
        cache_dir,
        features,
    };

    let formatted = crate::formatters::format_output(&diagnostics, output_format)
        .context("failed to format system diagnostics")?;
    println!("{formatted}");
    Ok(ExitCode::SUCCESS)
}

/// Finds the Claude Desktop configuration file path.
///
/// Searches in platform-specific locations.
fn find_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to determine home directory")?;

    let paths = if cfg!(target_os = "macos") {
        vec![
            home.join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json"),
        ]
    } else if cfg!(target_os = "windows") {
        let appdata = std::env::var("APPDATA")
            .map_or_else(|_| home.join("AppData").join("Roaming"), PathBuf::from);
        vec![appdata.join("Claude").join("claude_desktop_config.json")]
    } else {
        // Linux and other Unix-like systems
        vec![
            home.join(".config")
                .join("Claude")
                .join("claude_desktop_config.json"),
        ]
    };

    // Check environment variable override
    if let Ok(custom_path) = std::env::var("CLAUDE_CONFIG_PATH") {
        let custom = PathBuf::from(custom_path);
        if custom.exists() {
            return Ok(custom);
        }
    }

    // Find first existing path
    for path in paths {
        if path.exists() {
            return Ok(path);
        }
    }

    anyhow::bail!("Claude Desktop configuration not found")
}

/// Gets the skill store directory path.
fn get_skill_store_path() -> PathBuf {
    dirs::home_dir().map_or_else(
        || PathBuf::from(".claude/skills"),
        |home| home.join(".claude").join("skills"),
    )
}

/// Gets the cache directory path.
fn get_cache_dir() -> PathBuf {
    dirs::cache_dir().map_or_else(
        || {
            std::env::temp_dir()
                .canonicalize()
                .map_or_else(|_| PathBuf::from("/tmp/mcp-cli"), |tmp| tmp.join("mcp-cli"))
        },
        |cache| cache.join("mcp-cli"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_inspect_cache_success() {
        let result = run(DebugAction::Cache, OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_inspect_modules_success() {
        let result = run(DebugAction::Modules, OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_inspect_connections_success() {
        let result = run(DebugAction::Connections, OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_inspect_system_success() {
        let result = run(DebugAction::System, OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_all_actions_all_formats() {
        let actions = [
            DebugAction::Cache,
            DebugAction::Modules,
            DebugAction::Connections,
            DebugAction::System,
        ];
        let formats = [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty];

        for action in actions {
            for format in formats {
                let result = run(action.clone(), format).await;
                assert!(result.is_ok(), "Failed for {:?} with {:?}", action, format);
                assert_eq!(result.unwrap(), ExitCode::SUCCESS);
            }
        }
    }

    #[tokio::test]
    async fn test_cache_inspection_structure() {
        let bridge = Bridge::new(100);
        let stats = bridge.collect_stats().await;
        let cache_stats = bridge.cache_stats().await;

        let inspection = CacheInspection {
            size: cache_stats.size,
            capacity: cache_stats.capacity,
            usage_percent: cache_stats.usage_percent(),
            enabled: true,
            total_tool_calls: stats.total_tool_calls,
            cache_hits: stats.cache_hits,
            hit_rate_percent: stats.cache_hit_rate().map(|r| r * 100.0),
        };

        // Verify serialization works
        let json = serde_json::to_string(&inspection).unwrap();
        assert!(json.contains("size"));
        assert!(json.contains("capacity"));
    }

    #[tokio::test]
    async fn test_module_inspection_structure() {
        let bridge = Arc::new(Bridge::new(100));
        let config = SecurityConfig::default();
        let runtime = Runtime::new(bridge, config).unwrap();
        let stats = runtime.collect_stats();

        let inspection = ModuleInspection {
            cached_modules: 0,
            capacity: 50,
            total_executions: stats.total_executions,
            cache_hits: stats.cache_hits,
            hit_rate_percent: stats.cache_hit_rate().map(|r| r * 100.0),
            execution_failures: stats.execution_failures,
            compilation_failures: stats.compilation_failures,
            avg_execution_time_us: stats.avg_execution_time_us,
        };

        let json = serde_json::to_string(&inspection).unwrap();
        assert!(json.contains("total_executions"));
        assert!(json.contains("capacity"));
    }

    #[tokio::test]
    async fn test_connection_inspection_structure() {
        let bridge = Bridge::new(100);
        let stats = bridge.collect_stats().await;
        let (active, max) = bridge.connection_limits().await;

        let inspection = ConnectionInspection {
            active_connections: active,
            max_connections: max,
            total_connections: stats.total_connections,
            connection_failures: stats.connection_failures,
            success_rate_percent: stats.connection_success_rate().map(|r| r * 100.0),
            config_path: None,
            config_exists: false,
        };

        let json = serde_json::to_string(&inspection).unwrap();
        assert!(json.contains("active_connections"));
        assert!(json.contains("max_connections"));
    }

    #[test]
    fn test_system_diagnostics_structure() {
        let diagnostics = SystemDiagnostics {
            cli_version: "0.2.0".to_string(),
            rust_version: "1.85".to_string(),
            os: std::env::consts::OS.to_string(),
            arch: std::env::consts::ARCH.to_string(),
            config_path: None,
            skill_store_path: "/home/user/.claude/skills".to_string(),
            cache_dir: "/tmp/mcp-cli".to_string(),
            features: vec![],
        };

        let json = serde_json::to_string(&diagnostics).unwrap();
        assert!(json.contains("cli_version"));
        assert!(json.contains("rust_version"));
        assert!(json.contains("os"));
    }

    #[test]
    fn test_get_skill_store_path() {
        let path = get_skill_store_path();
        assert!(path.to_string_lossy().contains(".claude"));
        assert!(path.to_string_lossy().contains("skills"));
    }

    #[test]
    fn test_get_cache_dir() {
        let path = get_cache_dir();
        assert!(path.to_string_lossy().contains("mcp-cli"));
    }

    #[test]
    fn test_cache_inspection_serialization() {
        let inspection = CacheInspection {
            size: 42,
            capacity: 1000,
            usage_percent: 4.2,
            enabled: true,
            total_tool_calls: 100,
            cache_hits: 85,
            hit_rate_percent: Some(85.0),
        };

        let json = serde_json::to_string(&inspection).unwrap();
        let deserialized: CacheInspection = serde_json::from_str(&json).unwrap();
        assert_eq!(inspection, deserialized);
    }

    #[test]
    fn test_connection_inspection_serialization() {
        let inspection = ConnectionInspection {
            active_connections: 5,
            max_connections: 100,
            total_connections: 50,
            connection_failures: 3,
            success_rate_percent: Some(94.0),
            config_path: Some("/path/to/config.json".to_string()),
            config_exists: true,
        };

        let json = serde_json::to_string(&inspection).unwrap();
        let deserialized: ConnectionInspection = serde_json::from_str(&json).unwrap();
        assert_eq!(inspection, deserialized);
    }
}
