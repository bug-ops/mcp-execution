//! Server command implementation.
//!
//! Manages MCP server connections and configurations.

use crate::ServerAction;
use anyhow::Result;
use mcp_core::cli::{ExitCode, OutputFormat};
use serde::Serialize;
use tracing::info;

/// Represents a configured server entry.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerEntry {
    /// Server identifier
    pub id: String,
    /// Command used to start the server
    pub command: String,
    /// Current server status
    pub status: String,
}

/// List of configured servers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerList {
    /// All configured servers
    pub servers: Vec<ServerEntry>,
}

/// Detailed server information.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerInfo {
    /// Server identifier
    pub id: String,
    /// Command used to start the server
    pub command: String,
    /// Current server status
    pub status: String,
    /// Server capabilities
    pub capabilities: Vec<String>,
    /// Server health status
    pub health: String,
}

/// Validation result for a server command.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidationResult {
    /// The validated command
    pub command: String,
    /// Whether the command is valid
    pub valid: bool,
    /// Validation message
    pub message: String,
}

/// Runs the server command.
///
/// Manages server connections, listing, and validation.
///
/// # Arguments
///
/// * `action` - Server management action
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if server operation fails.
///
/// # Examples
///
/// ```
/// use mcp_cli::commands::server;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # tokio_test::block_on(async {
/// let result = server::run(
///     mcp_cli::ServerAction::List,
///     OutputFormat::Json
/// ).await;
/// assert!(result.is_ok());
/// # })
/// ```
pub async fn run(action: ServerAction, output_format: OutputFormat) -> Result<ExitCode> {
    info!("Server action: {:?}", action);
    info!("Output format: {}", output_format);

    match action {
        ServerAction::List => list_servers(output_format).await,
        ServerAction::Info { server } => show_server_info(server, output_format).await,
        ServerAction::Validate { command } => validate_command(command, output_format).await,
    }
}

/// Lists all configured servers.
///
/// For MVP, returns an empty list. Full persistence will be added in Phase 7.3.
async fn list_servers(output_format: OutputFormat) -> Result<ExitCode> {
    let server_list = ServerList {
        servers: vec![
            // MVP: Return stub data for demonstration
            ServerEntry {
                id: "vkteams-bot".to_string(),
                command: "vkteams-bot".to_string(),
                status: "configured".to_string(),
            },
        ],
    };

    let formatted = crate::formatters::format_output(&server_list, output_format)?;
    println!("{}", formatted);

    Ok(ExitCode::SUCCESS)
}

/// Shows detailed information about a specific server.
///
/// For MVP, returns stub data. Real server introspection will be added in Phase 7.3.
async fn show_server_info(server: String, output_format: OutputFormat) -> Result<ExitCode> {
    let server_info = ServerInfo {
        id: server.clone(),
        command: server,
        status: "configured".to_string(),
        capabilities: vec![
            "tools".to_string(),
            "resources".to_string(),
            "prompts".to_string(),
        ],
        health: "unknown".to_string(),
    };

    let formatted = crate::formatters::format_output(&server_info, output_format)?;
    println!("{}", formatted);

    Ok(ExitCode::SUCCESS)
}

/// Validates a server command.
///
/// Performs basic validation on the command string.
async fn validate_command(command: String, output_format: OutputFormat) -> Result<ExitCode> {
    let (valid, message) = if command.is_empty() {
        (false, "command cannot be empty".to_string())
    } else if command.contains('\n') {
        (false, "command cannot contain newlines".to_string())
    } else {
        (true, "command appears valid".to_string())
    };

    let result = ValidationResult {
        command,
        valid,
        message,
    };

    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{}", formatted);

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_server_list_success() {
        let result = run(ServerAction::List, OutputFormat::Pretty).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_server_list_json_format() {
        let result = run(ServerAction::List, OutputFormat::Json).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_server_list_text_format() {
        let result = run(ServerAction::List, OutputFormat::Text).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_server_info_success() {
        let result = run(
            ServerAction::Info {
                server: "vkteams-bot".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_server_info_all_formats() {
        for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
            let result = run(
                ServerAction::Info {
                    server: "test-server".to_string(),
                },
                format,
            )
            .await;
            assert!(result.is_ok());
            assert_eq!(result.unwrap(), ExitCode::SUCCESS);
        }
    }

    #[tokio::test]
    async fn test_server_validate_valid_command() {
        let result = run(
            ServerAction::Validate {
                command: "node server.js".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_server_validate_empty_command() {
        let result = run(
            ServerAction::Validate {
                command: "".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_server_validate_invalid_command_with_newline() {
        let result = run(
            ServerAction::Validate {
                command: "node\nserver.js".to_string(),
            },
            OutputFormat::Json,
        )
        .await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[test]
    fn test_server_entry_serialization() {
        let entry = ServerEntry {
            id: "test".to_string(),
            command: "test-cmd".to_string(),
            status: "configured".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("test-cmd"));
        assert!(json.contains("configured"));
    }

    #[test]
    fn test_server_list_serialization() {
        let list = ServerList {
            servers: vec![ServerEntry {
                id: "test".to_string(),
                command: "test-cmd".to_string(),
                status: "configured".to_string(),
            }],
        };

        let json = serde_json::to_string(&list).unwrap();
        assert!(json.contains("servers"));
        assert!(json.contains("test"));
    }

    #[test]
    fn test_server_info_serialization() {
        let info = ServerInfo {
            id: "test".to_string(),
            command: "test-cmd".to_string(),
            status: "configured".to_string(),
            capabilities: vec!["tools".to_string()],
            health: "healthy".to_string(),
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("capabilities"));
        assert!(json.contains("health"));
    }

    #[test]
    fn test_validation_result_serialization() {
        let result = ValidationResult {
            command: "test".to_string(),
            valid: true,
            message: "ok".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("command"));
        assert!(json.contains("valid"));
        assert!(json.contains("message"));
    }
}
