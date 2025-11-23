//! Server command implementation.
//!
//! Manages MCP server connections and configurations.

use crate::actions::ServerAction;
use anyhow::{Context, Result};
use mcp_core::ServerId;
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_introspector::Introspector;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Claude Desktop configuration file structure.
///
/// Represents the JSON structure of `claude_desktop_config.json`.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct ClaudeDesktopConfig {
    #[serde(rename = "mcpServers")]
    mcp_servers: HashMap<String, ServerConfig>,
}

/// MCP server configuration from Claude Desktop config.
///
/// Represents a single server entry with command, args, and environment variables.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
struct ServerConfig {
    /// Command to execute (e.g., "node", "python", "npx")
    command: String,
    /// Command arguments
    #[serde(default)]
    args: Vec<String>,
    /// Environment variables
    #[serde(default)]
    env: HashMap<String, String>,
}

/// Status of a configured server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ServerStatus {
    /// Server is available and responds
    Available,
    /// Server command not found or not executable
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

/// Detailed server information for output.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ServerInfo {
    /// Server identifier
    pub id: String,
    /// Server name from introspection
    pub name: String,
    /// Server version
    pub version: String,
    /// Command used to start the server
    pub command: String,
    /// Current server status
    pub status: String,
    /// Available tools
    pub tools: Vec<ToolSummary>,
    /// Server capabilities
    pub capabilities: Vec<String>,
}

/// Tool summary for output.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ToolSummary {
    /// Tool name
    pub name: String,
    /// Tool description
    pub description: String,
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

/// Manages MCP server discovery and validation.
///
/// Reads Claude Desktop configuration and provides server management operations.
#[derive(Debug)]
struct ServerManager {
    config_path: PathBuf,
}

impl ServerManager {
    /// Creates a new server manager.
    ///
    /// Discovers the Claude Desktop config file location automatically.
    fn new() -> Result<Self> {
        let config_path = Self::find_config_path()?;
        Ok(Self { config_path })
    }

    /// Finds the Claude Desktop configuration file.
    ///
    /// Searches in platform-specific locations:
    /// - macOS: `~/Library/Application Support/Claude/claude_desktop_config.json`
    /// - Linux: `~/.config/Claude/claude_desktop_config.json`
    /// - Windows: `%APPDATA%\Claude\claude_desktop_config.json`
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
                debug!("Using config from CLAUDE_CONFIG_PATH: {}", custom.display());
                return Ok(custom);
            }
        }

        // Find first existing path
        for path in paths {
            if path.exists() {
                debug!("Found Claude Desktop config at: {}", path.display());
                return Ok(path);
            }
        }

        anyhow::bail!(
            "Claude Desktop configuration not found. \
             Please ensure Claude Desktop is installed or set CLAUDE_CONFIG_PATH environment variable."
        )
    }

    /// Reads and parses the Claude Desktop configuration file.
    fn read_config(&self) -> Result<ClaudeDesktopConfig> {
        let contents = std::fs::read_to_string(&self.config_path).context(format!(
            "Failed to read config file: {}",
            self.config_path.display()
        ))?;

        let config: ClaudeDesktopConfig = serde_json::from_str(&contents).context(format!(
            "Failed to parse config file: {}",
            self.config_path.display()
        ))?;

        Ok(config)
    }

    /// Lists all configured servers.
    fn list_servers(&self) -> Result<Vec<(String, ServerConfig)>> {
        let config = self.read_config()?;
        Ok(config.mcp_servers.into_iter().collect())
    }

    /// Gets configuration for a specific server.
    fn get_server_config(&self, server_name: &str) -> Result<ServerConfig> {
        let config = self.read_config()?;
        config
            .mcp_servers
            .get(server_name)
            .cloned()
            .context(format!("Server '{server_name}' not found in configuration"))
    }

    /// Builds the full command string for a server.
    fn build_command_string(config: &ServerConfig) -> String {
        if config.args.is_empty() {
            config.command.clone()
        } else {
            format!("{} {}", config.command, config.args.join(" "))
        }
    }

    /// Checks if a command is executable.
    fn check_command_exists(command: &str) -> bool {
        which::which(command).is_ok()
    }

    /// Validates a server by attempting to connect and introspect it.
    async fn validate_server(&self, server_name: &str) -> Result<ServerStatus> {
        let config = self.get_server_config(server_name)?;

        // First check if command exists
        if !Self::check_command_exists(&config.command) {
            warn!(
                "Command '{}' not found in PATH for server '{}'",
                config.command, server_name
            );
            return Ok(ServerStatus::Unavailable);
        }

        // Try to connect and introspect
        match self.introspect_server(server_name).await {
            Ok(_) => Ok(ServerStatus::Available),
            Err(e) => {
                warn!("Failed to introspect server '{}': {}", server_name, e);
                Ok(ServerStatus::Unavailable)
            }
        }
    }

    /// Introspects a server using mcp-introspector.
    async fn introspect_server(&self, server_name: &str) -> Result<mcp_introspector::ServerInfo> {
        let config = self.get_server_config(server_name)?;
        let command_str = Self::build_command_string(&config);

        let mut introspector = Introspector::new();
        let server_id = ServerId::new(server_name);

        introspector
            .discover_server(server_id, &command_str)
            .await
            .context(format!("Failed to introspect server '{server_name}'"))
    }
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
/// ```no_run
/// use mcp_cli::commands::server;
/// use mcp_core::cli::{ExitCode, OutputFormat};
///
/// # #[tokio::main]
/// # async fn main() {
/// let result = server::run(
///     mcp_cli::ServerAction::List,
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

/// Lists all configured servers.
///
/// Reads Claude Desktop configuration and returns list of all servers with their status.
async fn list_servers(output_format: OutputFormat) -> Result<ExitCode> {
    let manager = ServerManager::new().context("Failed to initialize server manager")?;

    let servers = manager
        .list_servers()
        .context("Failed to read server configuration")?;

    if servers.is_empty() {
        info!("No MCP servers configured in Claude Desktop");
        let server_list = ServerList {
            servers: Vec::new(),
        };
        let formatted = crate::formatters::format_output(&server_list, output_format)?;
        println!("{formatted}");
        return Ok(ExitCode::SUCCESS);
    }

    // Build server entries
    let mut entries = Vec::new();
    for (name, config) in servers {
        let command = ServerManager::build_command_string(&config);
        let status = if ServerManager::check_command_exists(&config.command) {
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
    let manager = ServerManager::new().context("Failed to initialize server manager")?;

    let config = manager
        .get_server_config(&server)
        .context(format!("Server '{server}' not found in configuration"))?;

    let command = ServerManager::build_command_string(&config);

    // Try to introspect the server
    info!("Introspecting server '{}'...", server);
    match manager.introspect_server(&server).await {
        Ok(introspected) => {
            // Successfully introspected
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
            // Failed to introspect - return basic info
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

            // Return error code since introspection failed
            Ok(ExitCode::ERROR)
        }
    }
}

/// Validates a server by checking if it can be reached and introspected.
///
/// This validates a server name (not a raw command). The server must be configured
/// in Claude Desktop configuration.
async fn validate_command(server_name: String, output_format: OutputFormat) -> Result<ExitCode> {
    let manager = ServerManager::new().context("Failed to initialize server manager")?;

    // Get server configuration
    let config = match manager.get_server_config(&server_name) {
        Ok(cfg) => cfg,
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

    let command = ServerManager::build_command_string(&config);
    info!("Validating server '{}'...", server_name);

    // Check if command exists
    if !ServerManager::check_command_exists(&config.command) {
        let result = ValidationResult {
            command: command.clone(),
            valid: false,
            message: format!("Command '{}' not found in PATH", config.command),
        };
        let formatted = crate::formatters::format_output(&result, output_format)?;
        println!("{formatted}");
        return Ok(ExitCode::ERROR);
    }

    // Try to connect and introspect
    match manager.validate_server(&server_name).await? {
        ServerStatus::Available => {
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
        ServerStatus::Unavailable => {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    /// Creates a temporary config file for testing.
    fn create_test_config(content: &str) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(content.as_bytes()).unwrap();
        file.flush().unwrap();
        file
    }

    #[test]
    fn test_server_status_as_str() {
        assert_eq!(ServerStatus::Available.as_str(), "available");
        assert_eq!(ServerStatus::Unavailable.as_str(), "unavailable");
    }

    #[test]
    fn test_server_config_deserialization() {
        let json = r#"{
            "command": "node",
            "args": ["/path/to/server.js"],
            "env": {"KEY": "value"}
        }"#;

        let config: ServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.command, "node");
        assert_eq!(config.args, vec!["/path/to/server.js"]);
        assert_eq!(config.env.get("KEY"), Some(&"value".to_string()));
    }

    #[test]
    fn test_server_config_deserialization_minimal() {
        let json = r#"{
            "command": "python"
        }"#;

        let config: ServerConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.command, "python");
        assert!(config.args.is_empty());
        assert!(config.env.is_empty());
    }

    #[test]
    fn test_claude_desktop_config_deserialization() {
        let json = r#"{
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        }"#;

        let config: ClaudeDesktopConfig = serde_json::from_str(json).unwrap();
        assert!(config.mcp_servers.contains_key("test-server"));
    }

    #[test]
    fn test_build_command_string_no_args() {
        let config = ServerConfig {
            command: "node".to_string(),
            args: Vec::new(),
            env: HashMap::new(),
        };

        assert_eq!(ServerManager::build_command_string(&config), "node");
    }

    #[test]
    fn test_build_command_string_with_args() {
        let config = ServerConfig {
            command: "node".to_string(),
            args: vec!["/path/to/server.js".to_string(), "--verbose".to_string()],
            env: HashMap::new(),
        };

        assert_eq!(
            ServerManager::build_command_string(&config),
            "node /path/to/server.js --verbose"
        );
    }

    #[test]
    fn test_check_command_exists() {
        // Should exist on all platforms
        assert!(ServerManager::check_command_exists("ls"));

        // Should not exist
        assert!(!ServerManager::check_command_exists(
            "this_command_definitely_does_not_exist_12345"
        ));
    }

    #[test]
    fn test_server_manager_read_config() {
        let config_content = r#"{
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        }"#;

        let temp_file = create_test_config(config_content);

        let manager = ServerManager {
            config_path: temp_file.path().to_path_buf(),
        };

        let config = manager.read_config().unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        assert!(config.mcp_servers.contains_key("test-server"));
    }

    #[test]
    fn test_server_manager_list_servers() {
        let config_content = r#"{
            "mcpServers": {
                "server1": {
                    "command": "node",
                    "args": ["s1.js"]
                },
                "server2": {
                    "command": "python",
                    "args": ["s2.py"]
                }
            }
        }"#;

        let temp_file = create_test_config(config_content);

        let manager = ServerManager {
            config_path: temp_file.path().to_path_buf(),
        };

        let servers = manager.list_servers().unwrap();
        assert_eq!(servers.len(), 2);

        let names: Vec<String> = servers.iter().map(|(name, _)| name.clone()).collect();
        assert!(names.contains(&"server1".to_string()));
        assert!(names.contains(&"server2".to_string()));
    }

    #[test]
    fn test_server_manager_get_server_config() {
        let config_content = r#"{
            "mcpServers": {
                "test-server": {
                    "command": "node",
                    "args": ["server.js"]
                }
            }
        }"#;

        let temp_file = create_test_config(config_content);

        let manager = ServerManager {
            config_path: temp_file.path().to_path_buf(),
        };

        let config = manager.get_server_config("test-server").unwrap();
        assert_eq!(config.command, "node");
        assert_eq!(config.args, vec!["server.js"]);
    }

    #[test]
    fn test_server_manager_get_server_config_not_found() {
        let config_content = r#"{
            "mcpServers": {}
        }"#;

        let temp_file = create_test_config(config_content);

        let manager = ServerManager {
            config_path: temp_file.path().to_path_buf(),
        };

        let result = manager.get_server_config("nonexistent");
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("not found in configuration")
        );
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

    // Integration tests (require CLAUDE_CONFIG_PATH or Claude Desktop installed)
    #[tokio::test]
    #[ignore = "requires CLAUDE_CONFIG_PATH environment variable"]
    async fn test_list_servers_integration() {
        // This test requires CLAUDE_CONFIG_PATH to be set
        if std::env::var("CLAUDE_CONFIG_PATH").is_err() {
            return;
        }

        let result = run(ServerAction::List, OutputFormat::Json).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    #[ignore = "requires CLAUDE_CONFIG_PATH and configured server"]
    async fn test_server_info_integration() {
        // This test requires CLAUDE_CONFIG_PATH and a configured server
        if std::env::var("CLAUDE_CONFIG_PATH").is_err() {
            return;
        }

        // Note: Replace with actual server name from your config
        let result = run(
            ServerAction::Info {
                server: "test-server".to_string(),
            },
            OutputFormat::Json,
        )
        .await;

        // May fail if server doesn't exist or can't be reached
        assert!(result.is_ok());
    }
}
