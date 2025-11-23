//! Config command implementation.
//!
//! Manages CLI configuration files and settings.
//!
//! Configuration is stored in TOML format at:
//! - Linux/macOS: `~/.config/mcp-execution/config.toml`
//! - Windows: `%APPDATA%\mcp-execution\config.toml`

use crate::actions::ConfigAction;
use anyhow::{Context, Result};
use mcp_core::cli::{ExitCode, OutputFormat};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;
use tracing::{debug, info};

/// CLI configuration.
///
/// Stored in TOML format with support for runtime settings,
/// security policies, and server management.
///
/// # Examples
///
/// ```toml
/// [general]
/// cache_dir = "~/.cache/mcp-execution"
/// default_format = "pretty"
/// log_level = "info"
///
/// [security]
/// policy = "default"
/// allowed_servers = ["vkteams-bot", "github"]
/// max_calls_per_second = 10
///
/// [runtime]
/// max_memory_mb = 256
/// timeout_seconds = 60
/// max_fuel = 10000000
/// max_host_calls = 1000
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Config {
    /// General settings
    #[serde(default)]
    pub general: GeneralConfig,

    /// Security configuration
    #[serde(default)]
    pub security: SecurityConfig,

    /// Runtime configuration
    #[serde(default)]
    pub runtime: RuntimeConfig,
}

/// General configuration settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GeneralConfig {
    /// Default output format (json, text, pretty)
    pub default_format: String,

    /// Cache directory path
    pub cache_dir: String,

    /// Logging level (trace, debug, info, warn, error)
    pub log_level: String,
}

/// Security configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SecurityConfig {
    /// Security policy (default, strict, development)
    pub policy: String,

    /// List of allowed server names
    pub allowed_servers: Vec<String>,

    /// Maximum tool calls per second
    pub max_calls_per_second: Option<u32>,
}

/// Runtime configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RuntimeConfig {
    /// Maximum memory per WASM instance in MB
    pub max_memory_mb: u64,

    /// Execution timeout in seconds
    pub timeout_seconds: u64,

    /// Maximum fuel units for WASM execution
    pub max_fuel: Option<u64>,

    /// Maximum host function calls
    pub max_host_calls: Option<u32>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            default_format: "pretty".to_string(),
            cache_dir: get_default_cache_dir(),
            log_level: "info".to_string(),
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            policy: "default".to_string(),
            allowed_servers: Vec::new(),
            max_calls_per_second: Some(10),
        }
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            max_memory_mb: 256,
            timeout_seconds: 60,
            max_fuel: Some(10_000_000),
            max_host_calls: Some(1000),
        }
    }
}

impl Config {
    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration values are invalid.
    pub fn validate(&self) -> Result<()> {
        // Validate default format
        let valid_formats = ["json", "text", "pretty"];
        if !valid_formats.contains(&self.general.default_format.as_str()) {
            anyhow::bail!(
                "invalid default_format '{}', must be one of: {}",
                self.general.default_format,
                valid_formats.join(", ")
            );
        }

        // Validate log level
        let valid_levels = ["trace", "debug", "info", "warn", "error"];
        if !valid_levels.contains(&self.general.log_level.as_str()) {
            anyhow::bail!(
                "invalid log_level '{}', must be one of: {}",
                self.general.log_level,
                valid_levels.join(", ")
            );
        }

        // Validate security policy
        let valid_policies = ["default", "strict", "development"];
        if !valid_policies.contains(&self.security.policy.as_str()) {
            anyhow::bail!(
                "invalid security.policy '{}', must be one of: {}",
                self.security.policy,
                valid_policies.join(", ")
            );
        }

        // Validate runtime values
        if self.runtime.max_memory_mb == 0 {
            anyhow::bail!("runtime.max_memory_mb must be greater than 0");
        }

        if self.runtime.max_memory_mb > 4096 {
            anyhow::bail!("runtime.max_memory_mb cannot exceed 4096 MB (4 GB)");
        }

        if self.runtime.timeout_seconds == 0 {
            anyhow::bail!("runtime.timeout_seconds must be greater than 0");
        }

        if self.runtime.timeout_seconds > 600 {
            anyhow::bail!("runtime.timeout_seconds cannot exceed 600 seconds (10 minutes)");
        }

        Ok(())
    }
}

/// Gets the default configuration file path.
///
/// Returns platform-specific config path:
/// - Linux/macOS: `~/.config/mcp-execution/config.toml`
/// - Windows: `%APPDATA%\mcp-execution\config.toml`
fn get_config_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("failed to determine config directory")?;

    Ok(config_dir.join("mcp-execution").join("config.toml"))
}

/// Gets the default cache directory path.
fn get_default_cache_dir() -> String {
    dirs::cache_dir().map_or_else(
        || "~/.cache/mcp-execution".to_string(),
        |p| p.join("mcp-execution").display().to_string(),
    )
}

/// Loads configuration from file or returns defaults.
fn load_config() -> Result<Config> {
    let config_path = get_config_path()?;

    if !config_path.exists() {
        debug!("Config file not found, using defaults");
        return Ok(Config::default());
    }

    let content = fs::read_to_string(&config_path).context("failed to read config file")?;

    let config: Config = toml::from_str(&content).context("failed to parse config file")?;

    config.validate()?;

    Ok(config)
}

/// Saves configuration to file.
fn save_config(config: &Config) -> Result<()> {
    config.validate()?;

    let config_path = get_config_path()?;

    // Create parent directory if it doesn't exist
    if let Some(parent) = config_path.parent() {
        fs::create_dir_all(parent).context("failed to create config directory")?;
    }

    let toml_str = toml::to_string_pretty(config).context("failed to serialize config")?;

    fs::write(&config_path, toml_str).context("failed to write config file")?;

    debug!("Saved config to {}", config_path.display());

    Ok(())
}

/// Gets a configuration value by key path (e.g., "security.policy").
fn get_config_value_by_key(config: &Config, key: &str) -> Option<String> {
    match key {
        // General
        "general.default_format" => Some(config.general.default_format.clone()),
        "general.cache_dir" => Some(config.general.cache_dir.clone()),
        "general.log_level" => Some(config.general.log_level.clone()),

        // Security
        "security.policy" => Some(config.security.policy.clone()),
        "security.allowed_servers" => Some(config.security.allowed_servers.join(", ")),
        "security.max_calls_per_second" => {
            config.security.max_calls_per_second.map(|v| v.to_string())
        }

        // Runtime
        "runtime.max_memory_mb" => Some(config.runtime.max_memory_mb.to_string()),
        "runtime.timeout_seconds" => Some(config.runtime.timeout_seconds.to_string()),
        "runtime.max_fuel" => config.runtime.max_fuel.map(|v| v.to_string()),
        "runtime.max_host_calls" => config.runtime.max_host_calls.map(|v| v.to_string()),

        _ => None,
    }
}

/// Sets a configuration value by key path.
fn set_config_value_by_key(config: &mut Config, key: &str, value: &str) -> Result<()> {
    match key {
        // General
        "general.default_format" => {
            config.general.default_format = value.to_string();
        }
        "general.cache_dir" => {
            config.general.cache_dir = value.to_string();
        }
        "general.log_level" => {
            config.general.log_level = value.to_string();
        }

        // Security
        "security.policy" => {
            config.security.policy = value.to_string();
        }
        "security.allowed_servers" => {
            config.security.allowed_servers = value
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect();
        }
        "security.max_calls_per_second" => {
            if value.is_empty() || value == "none" {
                config.security.max_calls_per_second = None;
            } else {
                let val: u32 = value
                    .parse()
                    .context("invalid value for max_calls_per_second, must be a number")?;
                config.security.max_calls_per_second = Some(val);
            }
        }

        // Runtime
        "runtime.max_memory_mb" => {
            let val: u64 = value
                .parse()
                .context("invalid value for max_memory_mb, must be a number")?;
            config.runtime.max_memory_mb = val;
        }
        "runtime.timeout_seconds" => {
            let val: u64 = value
                .parse()
                .context("invalid value for timeout_seconds, must be a number")?;
            config.runtime.timeout_seconds = val;
        }
        "runtime.max_fuel" => {
            if value.is_empty() || value == "none" {
                config.runtime.max_fuel = None;
            } else {
                let val: u64 = value
                    .parse()
                    .context("invalid value for max_fuel, must be a number")?;
                config.runtime.max_fuel = Some(val);
            }
        }
        "runtime.max_host_calls" => {
            if value.is_empty() || value == "none" {
                config.runtime.max_host_calls = None;
            } else {
                let val: u32 = value
                    .parse()
                    .context("invalid value for max_host_calls, must be a number")?;
                config.runtime.max_host_calls = Some(val);
            }
        }

        _ => anyhow::bail!("unknown configuration key: {key}"),
    }

    Ok(())
}

/// Initialization result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct InitResult {
    /// Whether initialization was successful
    pub success: bool,
    /// Status message
    pub message: String,
    /// Path where config would be written (for MVP)
    pub path: String,
}

/// Configuration value result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ConfigValue {
    /// Configuration key
    pub key: String,
    /// Configuration value
    pub value: String,
}

/// Set configuration result.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetResult {
    /// Whether set was successful
    pub success: bool,
    /// The key that was set
    pub key: String,
    /// The new value
    pub value: String,
    /// Status message
    pub message: String,
}

/// Runs the config command.
///
/// Initializes, displays, and modifies CLI configuration.
///
/// # Arguments
///
/// * `action` - Configuration action to perform
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if configuration operation fails.
///
/// # Examples
///
/// ```no_run
/// use mcp_cli::commands::config;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # #[tokio::main]
/// # async fn main() {
/// let result = config::run(
///     mcp_cli::ConfigAction::Show,
///     OutputFormat::Json
/// ).await;
/// assert!(result.is_ok());
/// # }
/// ```
pub async fn run(action: ConfigAction, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Config action: {:?}", action);
    info!("Output format: {}", output_format);

    match action {
        ConfigAction::Init => init_config(output_format).await,
        ConfigAction::Show => show_config(output_format).await,
        ConfigAction::Set { key, value } => set_config(key, value, output_format).await,
        ConfigAction::Get { key } => get_config(key, output_format).await,
    }
}

/// Initializes configuration.
///
/// Creates a default configuration file if it doesn't exist.
/// Will not overwrite an existing configuration.
async fn init_config(output_format: OutputFormat) -> Result<ExitCode> {
    let config_path = get_config_path()?;

    if config_path.exists() {
        let result = InitResult {
            success: false,
            message: "configuration file already exists".to_string(),
            path: config_path.display().to_string(),
        };

        let formatted = crate::formatters::format_output(&result, output_format)
            .context("failed to format init result")?;
        println!("{formatted}");

        return Ok(ExitCode::SUCCESS);
    }

    let config = Config::default();
    save_config(&config)?;

    let result = InitResult {
        success: true,
        message: "configuration file created with default values".to_string(),
        path: config_path.display().to_string(),
    };

    let formatted = crate::formatters::format_output(&result, output_format)
        .context("failed to format init result")?;
    println!("{formatted}");

    Ok(ExitCode::SUCCESS)
}

/// Shows current configuration.
///
/// Displays all configuration values from file or defaults.
async fn show_config(output_format: OutputFormat) -> Result<ExitCode> {
    let config = load_config()?;

    let formatted = crate::formatters::format_output(&config, output_format)
        .context("failed to format configuration")?;
    println!("{formatted}");

    Ok(ExitCode::SUCCESS)
}

/// Sets a configuration value.
///
/// Updates the configuration file with the new value.
async fn set_config(key: String, value: String, output_format: OutputFormat) -> Result<ExitCode> {
    let mut config = load_config()?;

    match set_config_value_by_key(&mut config, &key, &value) {
        Ok(()) => {
            save_config(&config)?;

            let result = SetResult {
                success: true,
                key: key.clone(),
                value: value.clone(),
                message: format!("set '{key}' to '{value}'"),
            };

            let formatted = crate::formatters::format_output(&result, output_format)
                .context("failed to format result")?;
            println!("{formatted}");

            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            let result = SetResult {
                success: false,
                key,
                value,
                message: format!("failed to set value: {e}"),
            };

            let formatted = crate::formatters::format_output(&result, output_format)
                .context("failed to format result")?;
            println!("{formatted}");

            Err(e)
        }
    }
}

/// Gets a configuration value.
///
/// Retrieves a specific value from the configuration file.
async fn get_config(key: String, output_format: OutputFormat) -> Result<ExitCode> {
    let config = load_config()?;

    let value = get_config_value_by_key(&config, &key);

    match value {
        Some(v) => {
            let result = ConfigValue { key, value: v };
            let formatted = crate::formatters::format_output(&result, output_format)
                .context("failed to format config value")?;
            println!("{formatted}");
            Ok(ExitCode::SUCCESS)
        }
        None => {
            anyhow::bail!(
                "configuration key '{key}' not found\n\nAvailable keys:\n\
                 - general.default_format\n\
                 - general.cache_dir\n\
                 - general.log_level\n\
                 - security.policy\n\
                 - security.allowed_servers\n\
                 - security.max_calls_per_second\n\
                 - runtime.max_memory_mb\n\
                 - runtime.timeout_seconds\n\
                 - runtime.max_fuel\n\
                 - runtime.max_host_calls"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = Config::default();
        assert_eq!(config.general.default_format, "pretty");
        assert_eq!(config.general.log_level, "info");
        assert_eq!(config.security.policy, "default");
        assert_eq!(config.runtime.max_memory_mb, 256);
        assert_eq!(config.runtime.timeout_seconds, 60);
    }

    #[test]
    fn test_default_general_config() {
        let config = GeneralConfig::default();
        assert_eq!(config.default_format, "pretty");
        assert!(!config.cache_dir.is_empty());
        assert_eq!(config.log_level, "info");
    }

    #[test]
    fn test_default_security_config() {
        let config = SecurityConfig::default();
        assert_eq!(config.policy, "default");
        assert!(config.allowed_servers.is_empty());
        assert_eq!(config.max_calls_per_second, Some(10));
    }

    #[test]
    fn test_default_runtime_config() {
        let config = RuntimeConfig::default();
        assert_eq!(config.max_memory_mb, 256);
        assert_eq!(config.timeout_seconds, 60);
        assert_eq!(config.max_fuel, Some(10_000_000));
        assert_eq!(config.max_host_calls, Some(1000));
    }

    #[test]
    fn test_config_validation_valid() {
        let config = Config::default();
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_config_validation_invalid_format() {
        let mut config = Config::default();
        config.general.default_format = "invalid".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_invalid_log_level() {
        let mut config = Config::default();
        config.general.log_level = "invalid".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_invalid_policy() {
        let mut config = Config::default();
        config.security.policy = "invalid".to_string();
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_memory() {
        let mut config = Config::default();
        config.runtime.max_memory_mb = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_excessive_memory() {
        let mut config = Config::default();
        config.runtime.max_memory_mb = 5000;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_zero_timeout() {
        let mut config = Config::default();
        config.runtime.timeout_seconds = 0;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_excessive_timeout() {
        let mut config = Config::default();
        config.runtime.timeout_seconds = 700;
        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_serialization() {
        let config = Config::default();
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("[general]"));
        assert!(toml_str.contains("[security]"));
        assert!(toml_str.contains("[runtime]"));
        assert!(toml_str.contains("default_format"));
    }

    #[test]
    fn test_config_deserialization() {
        let toml_str = r#"
            [general]
            default_format = "json"
            cache_dir = "/tmp/test"
            log_level = "debug"

            [security]
            policy = "strict"
            allowed_servers = ["server1", "server2"]
            max_calls_per_second = 20

            [runtime]
            max_memory_mb = 512
            timeout_seconds = 120
            max_fuel = 20000000
            max_host_calls = 2000
        "#;

        let config: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(config.general.default_format, "json");
        assert_eq!(config.general.cache_dir, "/tmp/test");
        assert_eq!(config.general.log_level, "debug");
        assert_eq!(config.security.policy, "strict");
        assert_eq!(config.security.allowed_servers, vec!["server1", "server2"]);
        assert_eq!(config.security.max_calls_per_second, Some(20));
        assert_eq!(config.runtime.max_memory_mb, 512);
        assert_eq!(config.runtime.timeout_seconds, 120);
        assert_eq!(config.runtime.max_fuel, Some(20_000_000));
        assert_eq!(config.runtime.max_host_calls, Some(2000));
    }

    #[test]
    fn test_get_config_value_by_key() {
        let config = Config::default();

        assert_eq!(
            get_config_value_by_key(&config, "general.default_format"),
            Some("pretty".to_string())
        );
        assert_eq!(
            get_config_value_by_key(&config, "security.policy"),
            Some("default".to_string())
        );
        assert_eq!(
            get_config_value_by_key(&config, "runtime.max_memory_mb"),
            Some("256".to_string())
        );
        assert_eq!(get_config_value_by_key(&config, "invalid.key"), None);
    }

    #[test]
    fn test_set_config_value_by_key() {
        let mut config = Config::default();

        // Test setting general values
        assert!(set_config_value_by_key(&mut config, "general.default_format", "json").is_ok());
        assert_eq!(config.general.default_format, "json");

        // Test setting security values
        assert!(set_config_value_by_key(&mut config, "security.policy", "strict").is_ok());
        assert_eq!(config.security.policy, "strict");

        assert!(
            set_config_value_by_key(&mut config, "security.allowed_servers", "s1,s2,s3").is_ok()
        );
        assert_eq!(config.security.allowed_servers, vec!["s1", "s2", "s3"]);

        // Test setting runtime values
        assert!(set_config_value_by_key(&mut config, "runtime.max_memory_mb", "512").is_ok());
        assert_eq!(config.runtime.max_memory_mb, 512);

        assert!(set_config_value_by_key(&mut config, "runtime.timeout_seconds", "120").is_ok());
        assert_eq!(config.runtime.timeout_seconds, 120);

        // Test setting None values
        assert!(set_config_value_by_key(&mut config, "runtime.max_fuel", "none").is_ok());
        assert_eq!(config.runtime.max_fuel, None);

        // Test invalid key
        assert!(set_config_value_by_key(&mut config, "invalid.key", "value").is_err());

        // Test invalid number
        assert!(
            set_config_value_by_key(&mut config, "runtime.max_memory_mb", "not_a_number").is_err()
        );
    }

    #[test]
    fn test_init_result_serialization() {
        let result = InitResult {
            success: true,
            message: "test".to_string(),
            path: "/test/path".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("message"));
        assert!(json.contains("path"));
    }

    #[test]
    fn test_config_value_serialization() {
        let value = ConfigValue {
            key: "test".to_string(),
            value: "value".to_string(),
        };
        let json = serde_json::to_string(&value).unwrap();
        assert!(json.contains("key"));
        assert!(json.contains("value"));
    }

    #[test]
    fn test_set_result_serialization() {
        let result = SetResult {
            success: true,
            key: "test".to_string(),
            value: "value".to_string(),
            message: "ok".to_string(),
        };
        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("success"));
        assert!(json.contains("key"));
        assert!(json.contains("value"));
        assert!(json.contains("message"));
    }

    #[tokio::test]
    async fn test_config_show_defaults() {
        let result = run(ConfigAction::Show, OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_config_get_valid_key() {
        let result = run(
            ConfigAction::Get {
                key: "general.default_format".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_config_get_invalid_key() {
        let result = run(
            ConfigAction::Get {
                key: "nonexistent.key".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_err());
    }
}
