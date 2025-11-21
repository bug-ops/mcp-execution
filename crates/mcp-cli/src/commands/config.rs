//! Config command implementation.
//!
//! Manages CLI configuration files and settings.

use crate::ConfigAction;
use anyhow::{Context, Result};
use mcp_core::cli::{ExitCode, OutputFormat};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use tracing::info;

/// CLI configuration.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Config {
    /// Default output format
    pub default_format: String,
    /// Cache directory path
    pub cache_dir: String,
    /// Logging level
    pub log_level: String,
    /// Additional custom settings
    #[serde(flatten)]
    pub custom: HashMap<String, String>,
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
/// ```
/// use mcp_cli::commands::config;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # tokio_test::block_on(async {
/// let result = config::run(
///     mcp_cli::ConfigAction::Show,
///     OutputFormat::Json
/// ).await;
/// assert!(result.is_ok());
/// # })
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
/// For MVP, simulates initialization. Real file I/O will be added in Phase 7.5.
async fn init_config(output_format: OutputFormat) -> Result<ExitCode> {
    let result = InitResult {
        success: true,
        message: "configuration initialized (in-memory only for MVP)".to_string(),
        path: "~/.config/mcp-cli/config.toml".to_string(),
    };

    let formatted = crate::formatters::format_output(&result, output_format)
        .context("failed to format init result")?;
    println!("{}", formatted);

    Ok(ExitCode::SUCCESS)
}

/// Shows current configuration.
///
/// For MVP, returns in-memory defaults. Real config file reading will be added in Phase 7.5.
async fn show_config(output_format: OutputFormat) -> Result<ExitCode> {
    let config = get_default_config();

    let formatted = crate::formatters::format_output(&config, output_format)
        .context("failed to format configuration")?;
    println!("{}", formatted);

    Ok(ExitCode::SUCCESS)
}

/// Sets a configuration value.
///
/// For MVP, validates the key/value but doesn't persist. Real persistence will be added in Phase 7.5.
async fn set_config(key: String, value: String, output_format: OutputFormat) -> Result<ExitCode> {
    // Validate key
    let valid_keys = ["default_format", "cache_dir", "log_level"];
    let is_valid = valid_keys.contains(&key.as_str());

    let result = if is_valid {
        SetResult {
            success: true,
            key: key.clone(),
            value: value.clone(),
            message: format!("set '{}' to '{}' (in-memory only for MVP)", key, value),
        }
    } else {
        SetResult {
            success: false,
            key,
            value,
            message: format!("invalid key (valid keys: {})", valid_keys.join(", ")),
        }
    };

    let formatted = crate::formatters::format_output(&result, output_format)
        .context("failed to format result")?;
    println!("{}", formatted);

    Ok(ExitCode::SUCCESS)
}

/// Gets a configuration value.
///
/// For MVP, returns default values. Real config file reading will be added in Phase 7.5.
async fn get_config(key: String, output_format: OutputFormat) -> Result<ExitCode> {
    let config = get_default_config();

    let value = match key.as_str() {
        "default_format" => Some(config.default_format),
        "cache_dir" => Some(config.cache_dir),
        "log_level" => Some(config.log_level),
        _ => config.custom.get(&key).cloned(),
    };

    match value {
        Some(v) => {
            let result = ConfigValue { key, value: v };
            let formatted = crate::formatters::format_output(&result, output_format)
                .context("failed to format config value")?;
            println!("{}", formatted);
            Ok(ExitCode::SUCCESS)
        }
        None => Err(anyhow::anyhow!("configuration key '{}' not found", key)),
    }
}

/// Gets default configuration.
///
/// For MVP, returns hardcoded defaults. Will load from file in Phase 7.5.
fn get_default_config() -> Config {
    Config {
        default_format: "pretty".to_string(),
        cache_dir: "~/.cache/mcp-cli".to_string(),
        log_level: "info".to_string(),
        custom: HashMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_config_init_success() {
        let result = run(ConfigAction::Init, OutputFormat::Pretty).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_config_show_success() {
        let result = run(ConfigAction::Show, OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_config_set_valid_key() {
        let result = run(
            ConfigAction::Set {
                key: "default_format".to_string(),
                value: "json".to_string(),
            },
            OutputFormat::Text,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_config_set_invalid_key() {
        let result = run(
            ConfigAction::Set {
                key: "invalid_key".to_string(),
                value: "value".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_config_get_valid_key() {
        let result = run(
            ConfigAction::Get {
                key: "default_format".to_string(),
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
                key: "nonexistent".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_config_all_actions_all_formats() {
        let formats = [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty];

        for format in formats {
            // Test init
            let result = run(ConfigAction::Init, format).await;
            assert!(result.is_ok());

            // Test show
            let result = run(ConfigAction::Show, format).await;
            assert!(result.is_ok());

            // Test set
            let result = run(
                ConfigAction::Set {
                    key: "log_level".to_string(),
                    value: "debug".to_string(),
                },
                format,
            )
            .await;
            assert!(result.is_ok());

            // Test get
            let result = run(
                ConfigAction::Get {
                    key: "cache_dir".to_string(),
                },
                format,
            )
            .await;
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_default_config_values() {
        let config = get_default_config();
        assert_eq!(config.default_format, "pretty");
        assert!(!config.cache_dir.is_empty());
        assert!(!config.log_level.is_empty());
    }

    #[test]
    fn test_config_serialization() {
        let config = get_default_config();
        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("default_format"));
        assert!(json.contains("cache_dir"));
        assert!(json.contains("log_level"));
    }

    #[test]
    fn test_config_deserialization() {
        let json = r#"{
            "default_format": "json",
            "cache_dir": "/tmp",
            "log_level": "debug"
        }"#;
        let config: Config = serde_json::from_str(json).unwrap();
        assert_eq!(config.default_format, "json");
        assert_eq!(config.cache_dir, "/tmp");
        assert_eq!(config.log_level, "debug");
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

    #[test]
    fn test_config_with_custom_fields() {
        let mut custom = HashMap::new();
        custom.insert("custom_key".to_string(), "custom_value".to_string());

        let config = Config {
            default_format: "json".to_string(),
            cache_dir: "/tmp".to_string(),
            log_level: "debug".to_string(),
            custom,
        };

        let json = serde_json::to_string(&config).unwrap();
        assert!(json.contains("custom_key"));
        assert!(json.contains("custom_value"));
    }
}
