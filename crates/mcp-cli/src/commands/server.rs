//! Server command implementation.
//!
//! Manages MCP server listing, inspection, and validation using
//! `~/.claude/mcp.json` as the single source of truth for server definitions.

use crate::actions::ServerAction;
use crate::commands::common::{McpServerEntry, get_mcp_server, list_mcp_servers};
use anyhow::{Context, Result};
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_introspector::Introspector;
use serde::Serialize;
use tracing::{info, warn};

/// Status of a configured server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerStatus {
    /// Server command exists and is executable.
    Available,
    /// Server command not found in PATH.
    Unavailable,
}

impl ServerStatus {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Available => "available",
            Self::Unavailable => "unavailable",
        }
    }
}

/// Represents a configured server entry for output.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerEntry {
    /// Server identifier.
    pub id: String,
    /// Command used to start the server.
    pub command: String,
    /// Current server status.
    pub status: String,
}

/// List of configured servers.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerList {
    /// All configured servers.
    pub servers: Vec<ServerEntry>,
}

/// Detailed server information for output.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerInfo {
    /// Server identifier.
    pub id: String,
    /// Server name from introspection.
    pub name: String,
    /// Server version.
    pub version: String,
    /// Command used to start the server.
    pub command: String,
    /// Current server status.
    pub status: String,
    /// Available tools.
    pub tools: Vec<ToolSummary>,
    /// Server capabilities.
    pub capabilities: Vec<String>,
}

/// Tool summary for output.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolSummary {
    /// Tool name.
    pub name: String,
    /// Tool description.
    pub description: String,
}

/// Validation result for a server command.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ValidationResult {
    /// The validated command.
    pub command: String,
    /// Whether the command is valid.
    pub valid: bool,
    /// Validation message.
    pub message: String,
}

/// Runs the server command.
///
/// Manages server listing, detailed info, and validation.
/// All server definitions are loaded from `~/.claude/mcp.json`.
///
/// # Arguments
///
/// * `action` - Server management action
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if the server operation fails.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::server;
/// use mcp_execution_core::cli::{ExitCode, OutputFormat};
///
/// # #[tokio::main]
/// # async fn main() {
/// let result = server::run(
///     mcp_execution_cli::ServerAction::List,
///     OutputFormat::Json
/// ).await;
/// assert!(result.is_ok());
/// # }
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

/// Lists all servers configured in `~/.claude/mcp.json`.
///
/// Returns an empty list (not an error) when the config file does not exist.
async fn list_servers(output_format: OutputFormat) -> Result<ExitCode> {
    let servers = list_mcp_servers()
        .context("failed to read server configuration from ~/.claude/mcp.json")?;

    if servers.is_empty() {
        info!("No MCP servers configured in ~/.claude/mcp.json");
        let server_list = ServerList {
            servers: Vec::new(),
        };
        let formatted = crate::formatters::format_output(&server_list, output_format)?;
        println!("{formatted}");
        return Ok(ExitCode::SUCCESS);
    }

    let mut entries = Vec::new();
    for (name, entry) in servers {
        let command = build_command_string(&entry);
        let status = if check_command_exists(&entry.command) {
            ServerStatus::Available
        } else {
            ServerStatus::Unavailable
        };

        entries.push(ServerEntry {
            id: name,
            command,
            status: status.as_str().to_string(),
        });
    }

    let server_list = ServerList { servers: entries };
    let formatted = crate::formatters::format_output(&server_list, output_format)?;
    println!("{formatted}");

    Ok(ExitCode::SUCCESS)
}

/// Shows detailed information about a specific server.
///
/// Connects to the server and introspects its capabilities, tools, and status.
async fn show_server_info(server: String, output_format: OutputFormat) -> Result<ExitCode> {
    let (server_id, server_config, entry) = get_mcp_server(&server)
        .with_context(|| format!("server '{server}' not found in ~/.claude/mcp.json"))?;

    let command = build_command_string(&entry);

    info!("Introspecting server '{}'...", server);

    let mut introspector = Introspector::new();
    match introspector
        .discover_server(server_id, &server_config)
        .await
    {
        Ok(introspected) => {
            let mut capabilities = Vec::new();
            if introspected.capabilities.supports_tools {
                capabilities.push("tools".to_string());
            }
            if introspected.capabilities.supports_resources {
                capabilities.push("resources".to_string());
            }
            if introspected.capabilities.supports_prompts {
                capabilities.push("prompts".to_string());
            }

            let tools = introspected
                .tools
                .iter()
                .map(|t| ToolSummary {
                    name: t.name.as_str().to_string(),
                    description: t.description.clone(),
                })
                .collect();

            let server_info = ServerInfo {
                id: server,
                name: introspected.name,
                version: introspected.version,
                command,
                status: ServerStatus::Available.as_str().to_string(),
                tools,
                capabilities,
            };

            let formatted = crate::formatters::format_output(&server_info, output_format)?;
            println!("{formatted}");

            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            warn!("Failed to introspect server '{}': {}", server, e);

            let server_info = ServerInfo {
                id: server.clone(),
                name: server,
                version: "unknown".to_string(),
                command,
                status: ServerStatus::Unavailable.as_str().to_string(),
                tools: Vec::new(),
                capabilities: Vec::new(),
            };

            let formatted = crate::formatters::format_output(&server_info, output_format)?;
            println!("{formatted}");

            Ok(ExitCode::ERROR)
        }
    }
}

/// Validates a server by checking its command and attempting introspection.
///
/// The server must be configured in `~/.claude/mcp.json`.
async fn validate_command(server_name: String, output_format: OutputFormat) -> Result<ExitCode> {
    let (server_id, server_config, entry) = match get_mcp_server(&server_name) {
        Ok(result) => result,
        Err(e) => {
            let result = ValidationResult {
                command: server_name,
                valid: false,
                message: format!("Server not found in configuration: {e}"),
            };
            let formatted = crate::formatters::format_output(&result, output_format)?;
            println!("{formatted}");
            return Ok(ExitCode::ERROR);
        }
    };

    let command = build_command_string(&entry);
    info!("Validating server '{}'...", server_name);

    if !check_command_exists(&entry.command) {
        let result = ValidationResult {
            command: command.clone(),
            valid: false,
            message: format!("Command '{}' not found in PATH", entry.command),
        };
        let formatted = crate::formatters::format_output(&result, output_format)?;
        println!("{formatted}");
        return Ok(ExitCode::ERROR);
    }

    let mut introspector = Introspector::new();
    match introspector
        .discover_server(server_id, &server_config)
        .await
    {
        Ok(_) => {
            let result = ValidationResult {
                command,
                valid: true,
                message: format!(
                    "Server '{server_name}' is available and responds to MCP protocol"
                ),
            };
            let formatted = crate::formatters::format_output(&result, output_format)?;
            println!("{formatted}");
            Ok(ExitCode::SUCCESS)
        }
        Err(e) => {
            warn!(
                "Failed to introspect server '{}' during validation: {}",
                server_name, e
            );
            let result = ValidationResult {
                command,
                valid: false,
                message: format!(
                    "Server '{server_name}' command exists but failed to respond to MCP protocol"
                ),
            };
            let formatted = crate::formatters::format_output(&result, output_format)?;
            println!("{formatted}");
            Ok(ExitCode::ERROR)
        }
    }
}

/// Builds a displayable command string from a server entry.
fn build_command_string(entry: &McpServerEntry) -> String {
    if entry.args.is_empty() {
        entry.command.clone()
    } else {
        format!("{} {}", entry.command, entry.args.join(" "))
    }
}

/// Returns `true` if the given command binary is available in PATH.
fn check_command_exists(command: &str) -> bool {
    which::which(command).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_server_status_as_str() {
        assert_eq!(ServerStatus::Available.as_str(), "available");
        assert_eq!(ServerStatus::Unavailable.as_str(), "unavailable");
    }

    #[test]
    fn test_build_command_string_no_args() {
        let entry = McpServerEntry {
            command: "node".to_string(),
            args: Vec::new(),
            env: HashMap::default(),
        };
        assert_eq!(build_command_string(&entry), "node");
    }

    #[test]
    fn test_build_command_string_with_args() {
        let entry = McpServerEntry {
            command: "node".to_string(),
            args: vec!["/path/to/server.js".to_string(), "--verbose".to_string()],
            env: HashMap::default(),
        };
        assert_eq!(
            build_command_string(&entry),
            "node /path/to/server.js --verbose"
        );
    }

    #[test]
    fn test_check_command_exists() {
        assert!(check_command_exists("ls"));
        assert!(!check_command_exists(
            "this_command_definitely_does_not_exist_12345"
        ));
    }

    #[test]
    fn test_server_entry_serialization() {
        let entry = ServerEntry {
            id: "test".to_string(),
            command: "test-cmd".to_string(),
            status: "available".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("test-cmd"));
        assert!(json.contains("available"));
    }

    #[test]
    fn test_server_list_serialization() {
        let list = ServerList {
            servers: vec![ServerEntry {
                id: "test".to_string(),
                command: "test-cmd".to_string(),
                status: "available".to_string(),
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
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            command: "test-cmd".to_string(),
            status: "available".to_string(),
            tools: vec![ToolSummary {
                name: "test_tool".to_string(),
                description: "A test tool".to_string(),
            }],
            capabilities: vec!["tools".to_string()],
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("Test Server"));
        assert!(json.contains("capabilities"));
        assert!(json.contains("tools"));
    }

    #[test]
    fn test_tool_summary_serialization() {
        let tool = ToolSummary {
            name: "send_message".to_string(),
            description: "Sends a message".to_string(),
        };

        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("send_message"));
        assert!(json.contains("Sends a message"));
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
