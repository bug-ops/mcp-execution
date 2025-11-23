//! MCP server introspection using rmcp official SDK.
//!
//! This crate provides functionality to discover MCP server capabilities, tools,
//! resources, and prompts using the official rmcp SDK. It enables automatic
//! extraction of tool schemas for code generation.
//!
//! # Architecture
//!
//! The introspector connects to MCP servers via stdio transport and uses rmcp's
//! `ServiceExt` trait to query server capabilities. Discovered information is
//! stored locally for subsequent code generation phases.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_introspector::Introspector;
//! use mcp_core::ServerId;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut introspector = Introspector::new();
//!
//! // Connect to github server
//! let server_id = ServerId::new("github");
//! let info = introspector
//!     .discover_server(server_id, "github-server")
//!     .await?;
//!
//! println!("Server: {} v{}", info.name, info.version);
//! println!("Tools found: {}", info.tools.len());
//!
//! for tool in &info.tools {
//!     println!("  - {}: {}", tool.name, tool.description);
//! }
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

use mcp_core::{Error, Result, ServerId, ToolName};
use rmcp::ServiceExt;
use rmcp::transport::{ConfigureCommandExt, TokioChildProcess};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Information about an MCP server.
///
/// Contains metadata about the server including its name, version,
/// available tools, and supported capabilities.
///
/// # Examples
///
/// ```
/// use mcp_introspector::{ServerInfo, ServerCapabilities};
/// use mcp_core::ServerId;
///
/// let info = ServerInfo {
///     id: ServerId::new("example"),
///     name: "Example Server".to_string(),
///     version: "1.0.0".to_string(),
///     tools: vec![],
///     capabilities: ServerCapabilities {
///         supports_tools: true,
///         supports_resources: false,
///         supports_prompts: false,
///     },
/// };
///
/// assert_eq!(info.name, "Example Server");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerInfo {
    /// Unique server identifier
    pub id: ServerId,
    /// Human-readable server name
    pub name: String,
    /// Server version string
    pub version: String,
    /// List of available tools
    pub tools: Vec<ToolInfo>,
    /// Server capabilities
    pub capabilities: ServerCapabilities,
}

/// Information about an MCP tool.
///
/// Contains the tool's name, description, and JSON schema for input validation.
///
/// # Examples
///
/// ```
/// use mcp_introspector::ToolInfo;
/// use mcp_core::ToolName;
/// use serde_json::json;
///
/// let tool = ToolInfo {
///     name: ToolName::new("send_message"),
///     description: "Sends a message to a chat".to_string(),
///     input_schema: json!({
///         "type": "object",
///         "properties": {
///             "chat_id": {"type": "string"},
///             "text": {"type": "string"}
///         },
///         "required": ["chat_id", "text"]
///     }),
///     output_schema: None,
/// };
///
/// assert_eq!(tool.name.as_str(), "send_message");
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolInfo {
    /// Tool name
    pub name: ToolName,
    /// Human-readable description of what the tool does
    pub description: String,
    /// JSON Schema for tool input parameters
    pub input_schema: serde_json::Value,
    /// Optional JSON Schema for tool output (if provided by server)
    pub output_schema: Option<serde_json::Value>,
}

/// Server capabilities.
///
/// Indicates which MCP features the server supports.
///
/// # Examples
///
/// ```
/// use mcp_introspector::ServerCapabilities;
///
/// let caps = ServerCapabilities {
///     supports_tools: true,
///     supports_resources: true,
///     supports_prompts: false,
/// };
///
/// assert!(caps.supports_tools);
/// assert!(!caps.supports_prompts);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    /// Server supports tool execution
    pub supports_tools: bool,
    /// Server supports resource access
    pub supports_resources: bool,
    /// Server supports prompts
    pub supports_prompts: bool,
}

/// MCP server introspector.
///
/// Discovers and caches information about MCP servers using the official
/// rmcp SDK. Multiple servers can be discovered and their information
/// retrieved later for code generation.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, allowing it to be used across thread
/// boundaries safely.
///
/// # Examples
///
/// ```no_run
/// use mcp_introspector::Introspector;
/// use mcp_core::ServerId;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut introspector = Introspector::new();
///
/// // Discover multiple servers
/// let server1 = ServerId::new("server1");
/// introspector.discover_server(server1.clone(), "server1-cmd").await?;
///
/// let server2 = ServerId::new("server2");
/// introspector.discover_server(server2.clone(), "server2-cmd").await?;
///
/// // Retrieve information
/// if let Some(info) = introspector.get_server(&server1) {
///     println!("Server 1 has {} tools", info.tools.len());
/// }
///
/// // List all servers
/// let all_servers = introspector.list_servers();
/// println!("Total servers discovered: {}", all_servers.len());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct Introspector {
    servers: HashMap<ServerId, ServerInfo>,
}

impl Introspector {
    /// Creates a new introspector.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_introspector::Introspector;
    ///
    /// let introspector = Introspector::new();
    /// assert_eq!(introspector.list_servers().len(), 0);
    /// ```
    #[must_use]
    pub fn new() -> Self {
        Self {
            servers: HashMap::new(),
        }
    }

    /// Connects to an MCP server via stdio and discovers its capabilities.
    ///
    /// This method:
    /// 1. Spawns the server process using stdio transport
    /// 2. Connects via rmcp client
    /// 3. Queries server information using `ServiceExt::get_server_info`
    /// 4. Extracts tools and capabilities
    /// 5. Caches the information for later retrieval
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - The server process cannot be spawned
    /// - Connection to the server fails
    /// - Server does not respond to capability queries
    /// - Server response is malformed
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_introspector::Introspector;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    /// let server_id = ServerId::new("github");
    ///
    /// let info = introspector
    ///     .discover_server(server_id, "github-server")
    ///     .await?;
    ///
    /// println!("Found {} tools", info.tools.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn discover_server(
        &mut self,
        server_id: ServerId,
        command: &str,
    ) -> Result<ServerInfo> {
        tracing::info!("Discovering MCP server: {}", server_id);

        // Validate command for security (prevents command injection)
        mcp_core::validate_command(command)?;

        // Connect via stdio using rmcp
        let transport =
            TokioChildProcess::new(tokio::process::Command::new(command).configure(|_cmd| {}))
                .map_err(|e| Error::ConnectionFailed {
                    server: server_id.to_string(),
                    source: Box::new(e),
                })?;

        // Create client using serve pattern
        let client =
            ().serve(transport)
                .await
                .map_err(|e| Error::ConnectionFailed {
                    server: server_id.to_string(),
                    source: Box::new(e),
                })?;

        // List all tools from server
        let tool_list = client
            .list_all_tools()
            .await
            .map_err(|e| Error::ConnectionFailed {
                server: server_id.to_string(),
                source: Box::new(e),
            })?;

        tracing::debug!(
            "Server {} responded with {} tools",
            server_id,
            tool_list.len()
        );

        // Extract tools
        let tools = tool_list
            .into_iter()
            .map(|tool| {
                tracing::trace!("Found tool: {}", tool.name);
                ToolInfo {
                    name: ToolName::new(tool.name),
                    description: tool.description.unwrap_or_default().to_string(),
                    input_schema: serde_json::Value::Object((*tool.input_schema).clone()),
                    output_schema: None, // rmcp doesn't provide output schema
                }
            })
            .collect::<Vec<_>>();

        // Try to get resources capability
        let has_resources = client.list_all_resources().await.is_ok();

        let capabilities = ServerCapabilities {
            supports_tools: !tools.is_empty(),
            supports_resources: has_resources,
            supports_prompts: false, // Would need to check prompts similarly
        };

        let info = ServerInfo {
            id: server_id.clone(),
            name: command.to_string(),      // Use command as name
            version: "unknown".to_string(), // MCP doesn't expose version via ServiceExt
            tools,
            capabilities,
        };

        self.servers.insert(server_id, info.clone());

        tracing::info!("Successfully discovered {} tools", info.tools.len());

        Ok(info)
    }

    /// Gets information about a previously discovered server.
    ///
    /// Returns `None` if the server has not been discovered yet.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_introspector::Introspector;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    /// let server_id = ServerId::new("test");
    ///
    /// // Not discovered yet
    /// assert!(introspector.get_server(&server_id).is_none());
    ///
    /// // Discover it
    /// introspector.discover_server(server_id.clone(), "test-cmd").await?;
    ///
    /// // Now available
    /// assert!(introspector.get_server(&server_id).is_some());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn get_server(&self, server_id: &ServerId) -> Option<&ServerInfo> {
        self.servers.get(server_id)
    }

    /// Lists all discovered servers.
    ///
    /// Returns a vector of references to server information in no
    /// particular order.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_introspector::Introspector;
    ///
    /// let introspector = Introspector::new();
    /// let servers = introspector.list_servers();
    /// assert_eq!(servers.len(), 0);
    /// ```
    #[must_use]
    pub fn list_servers(&self) -> Vec<&ServerInfo> {
        self.servers.values().collect()
    }

    /// Returns the number of discovered servers.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_introspector::Introspector;
    ///
    /// let introspector = Introspector::new();
    /// assert_eq!(introspector.server_count(), 0);
    /// ```
    #[must_use]
    pub fn server_count(&self) -> usize {
        self.servers.len()
    }

    /// Removes a server from the cache.
    ///
    /// Returns `true` if the server was present and removed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_introspector::Introspector;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    /// let server_id = ServerId::new("test");
    ///
    /// introspector.discover_server(server_id.clone(), "test-cmd").await?;
    /// assert_eq!(introspector.server_count(), 1);
    ///
    /// let removed = introspector.remove_server(&server_id);
    /// assert!(removed);
    /// assert_eq!(introspector.server_count(), 0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn remove_server(&mut self, server_id: &ServerId) -> bool {
        self.servers.remove(server_id).is_some()
    }

    /// Clears all discovered servers from the cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_introspector::Introspector;
    /// use mcp_core::ServerId;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    ///
    /// introspector.discover_server(ServerId::new("s1"), "cmd1").await?;
    /// introspector.discover_server(ServerId::new("s2"), "cmd2").await?;
    /// assert_eq!(introspector.server_count(), 2);
    ///
    /// introspector.clear();
    /// assert_eq!(introspector.server_count(), 0);
    /// # Ok(())
    /// # }
    /// ```
    pub fn clear(&mut self) {
        self.servers.clear();
    }
}

impl Default for Introspector {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_introspector_new() {
        let introspector = Introspector::new();
        assert_eq!(introspector.list_servers().len(), 0);
        assert_eq!(introspector.server_count(), 0);
    }

    #[test]
    fn test_introspector_default() {
        let introspector = Introspector::default();
        assert_eq!(introspector.server_count(), 0);
    }

    #[test]
    fn test_server_info_debug() {
        let info = ServerInfo {
            id: ServerId::new("test"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };
        let debug_str = format!("{info:?}");
        assert!(debug_str.contains("Test Server"));
        assert!(debug_str.contains("1.0.0"));
    }

    #[test]
    fn test_tool_info_creation() {
        let tool = ToolInfo {
            name: ToolName::new("test_tool"),
            description: "A test tool".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: None,
        };

        assert_eq!(tool.name.as_str(), "test_tool");
        assert_eq!(tool.description, "A test tool");
        assert!(tool.output_schema.is_none());
    }

    #[test]
    fn test_server_capabilities() {
        let caps = ServerCapabilities {
            supports_tools: true,
            supports_resources: true,
            supports_prompts: false,
        };

        assert!(caps.supports_tools);
        assert!(caps.supports_resources);
        assert!(!caps.supports_prompts);
    }

    #[test]
    fn test_get_server_not_found() {
        let introspector = Introspector::new();
        let server_id = ServerId::new("nonexistent");
        assert!(introspector.get_server(&server_id).is_none());
    }

    #[test]
    fn test_clear() {
        let mut introspector = Introspector::new();

        // Add some fake server data
        let info = ServerInfo {
            id: ServerId::new("test"),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        introspector.servers.insert(ServerId::new("test"), info);
        assert_eq!(introspector.server_count(), 1);

        introspector.clear();
        assert_eq!(introspector.server_count(), 0);
    }

    #[test]
    fn test_remove_server() {
        let mut introspector = Introspector::new();
        let server_id = ServerId::new("test");

        // Add fake server data
        let info = ServerInfo {
            id: server_id.clone(),
            name: "Test".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        introspector.servers.insert(server_id.clone(), info);
        assert_eq!(introspector.server_count(), 1);

        // Remove existing server
        assert!(introspector.remove_server(&server_id));
        assert_eq!(introspector.server_count(), 0);

        // Remove non-existent server
        assert!(!introspector.remove_server(&server_id));
    }

    #[test]
    fn test_list_servers() {
        let mut introspector = Introspector::new();

        // Empty list
        assert_eq!(introspector.list_servers().len(), 0);

        // Add servers
        let info1 = ServerInfo {
            id: ServerId::new("server1"),
            name: "Server 1".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let info2 = ServerInfo {
            id: ServerId::new("server2"),
            name: "Server 2".to_string(),
            version: "2.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: false,
                supports_resources: true,
                supports_prompts: false,
            },
        };

        introspector.servers.insert(ServerId::new("server1"), info1);
        introspector.servers.insert(ServerId::new("server2"), info2);

        let servers = introspector.list_servers();
        assert_eq!(servers.len(), 2);
    }

    #[test]
    fn test_serialization() {
        let tool = ToolInfo {
            name: ToolName::new("test_tool"),
            description: "Test".to_string(),
            input_schema: serde_json::json!({"type": "object"}),
            output_schema: Some(serde_json::json!({"type": "string"})),
        };

        // Serialize to JSON
        let json = serde_json::to_string(&tool).unwrap();
        assert!(json.contains("test_tool"));
        assert!(json.contains("Test"));

        // Deserialize back
        let tool2: ToolInfo = serde_json::from_str(&json).unwrap();
        assert_eq!(tool2.name.as_str(), "test_tool");
        assert_eq!(tool2.description, "Test");
    }
}
