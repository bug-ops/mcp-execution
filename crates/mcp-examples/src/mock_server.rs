//! Mock MCP server for testing.
//!
//! Provides a lightweight in-memory MCP server implementation for integration testing
//! without requiring external server processes.

use mcp_core::{ServerId, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::{Value, json};
use std::collections::HashMap;
use thiserror::Error;

/// Errors that can occur during mock server operations.
#[derive(Error, Debug)]
pub enum MockServerError {
    /// Tool not found in server.
    #[error("tool not found: {0}")]
    ToolNotFound(String),

    /// Invalid parameters provided to tool.
    #[error("invalid parameters: {0}")]
    InvalidParameters(String),

    /// Simulated server error for testing error handling.
    #[error("simulated error: {0}")]
    SimulatedError(String),
}

/// Mock MCP server for testing purposes.
///
/// Simulates a real MCP server with configurable tools and responses.
/// Used for integration testing without requiring actual MCP server processes.
///
/// # Examples
///
/// ```
/// use mcp_examples::mock_server::MockMcpServer;
///
/// let server = MockMcpServer::new_vkteams_bot();
/// let info = server.server_info();
/// assert_eq!(info.name, "vkteams-bot");
/// assert!(!info.tools.is_empty());
/// ```
#[derive(Debug, Clone)]
pub struct MockMcpServer {
    info: ServerInfo,
    responses: HashMap<String, Value>,
}

impl MockMcpServer {
    /// Creates a new mock server with the given configuration.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::mock_server::MockMcpServer;
    /// use mcp_core::ServerId;
    /// use mcp_introspector::{ServerInfo, ServerCapabilities};
    ///
    /// let info = ServerInfo {
    ///     id: ServerId::new("test"),
    ///     name: "Test Server".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     capabilities: ServerCapabilities {
    ///         supports_tools: true,
    ///         supports_resources: false,
    ///         supports_prompts: false,
    ///     },
    ///     tools: vec![],
    /// };
    ///
    /// let server = MockMcpServer::new(info);
    /// ```
    #[must_use]
    pub fn new(info: ServerInfo) -> Self {
        Self {
            info,
            responses: HashMap::new(),
        }
    }

    /// Creates a mock `VKTeams` Bot server with realistic tools.
    ///
    /// This server simulates the `VKTeams` Bot MCP server with the following tools:
    /// - `send_message`: Send a message to a chat
    /// - `get_message`: Retrieve a message by ID
    /// - `get_chat`: Get chat information
    /// - `list_chats`: List all available chats
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::mock_server::MockMcpServer;
    ///
    /// let server = MockMcpServer::new_vkteams_bot();
    /// assert_eq!(server.server_info().name, "vkteams-bot");
    /// assert_eq!(server.server_info().tools.len(), 4);
    /// ```
    #[must_use]
    pub fn new_vkteams_bot() -> Self {
        let tools = vec![
            ToolInfo {
                name: ToolName::new("send_message"),
                description: "Send a message to a VKTeams chat".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "string",
                            "description": "Chat ID to send message to"
                        },
                        "text": {
                            "type": "string",
                            "description": "Message text"
                        }
                    },
                    "required": ["chat_id", "text"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "message_id": {"type": "string"},
                        "timestamp": {"type": "number"}
                    }
                })),
            },
            ToolInfo {
                name: ToolName::new("get_message"),
                description: "Retrieve a message by ID".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "string",
                            "description": "Chat ID"
                        },
                        "message_id": {
                            "type": "string",
                            "description": "Message ID to retrieve"
                        }
                    },
                    "required": ["chat_id", "message_id"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "text": {"type": "string"},
                        "sender": {"type": "string"},
                        "timestamp": {"type": "number"}
                    }
                })),
            },
            ToolInfo {
                name: ToolName::new("get_chat"),
                description: "Get information about a chat".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "string",
                            "description": "Chat ID"
                        }
                    },
                    "required": ["chat_id"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "type": {"type": "string"},
                        "members_count": {"type": "number"}
                    }
                })),
            },
            ToolInfo {
                name: ToolName::new("list_chats"),
                description: "List all available chats".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "limit": {
                            "type": "number",
                            "description": "Maximum number of chats to return"
                        }
                    }
                }),
                output_schema: Some(json!({
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "chat_id": {"type": "string"},
                            "name": {"type": "string"}
                        }
                    }
                })),
            },
        ];

        let info = ServerInfo {
            id: ServerId::new("vkteams-bot"),
            name: "vkteams-bot".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools,
        };

        let mut server = Self::new(info);

        // Pre-configure some responses
        server.set_response(
            "send_message",
            json!({
                "message_id": "msg_123456",
                "timestamp": 1699900000
            }),
        );

        server.set_response(
            "get_message",
            json!({
                "text": "Hello from VKTeams!",
                "sender": "user_789",
                "timestamp": 1699900000
            }),
        );

        server.set_response(
            "get_chat",
            json!({
                "name": "Development Team",
                "type": "group",
                "members_count": 15
            }),
        );

        server.set_response(
            "list_chats",
            json!([
                {"chat_id": "chat_1", "name": "Development Team"},
                {"chat_id": "chat_2", "name": "Project Alpha"},
                {"chat_id": "chat_3", "name": "General Discussion"}
            ]),
        );

        server
    }

    /// Returns server information including capabilities and available tools.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::mock_server::MockMcpServer;
    ///
    /// let server = MockMcpServer::new_vkteams_bot();
    /// let info = server.server_info();
    /// assert!(info.capabilities.supports_tools);
    /// ```
    #[must_use]
    pub const fn server_info(&self) -> &ServerInfo {
        &self.info
    }

    /// Sets a predefined response for a tool call.
    ///
    /// This allows configuring the mock server to return specific responses
    /// for testing purposes.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::mock_server::MockMcpServer;
    /// use serde_json::json;
    ///
    /// let mut server = MockMcpServer::new_vkteams_bot();
    /// server.set_response("send_message", json!({"message_id": "test_123"}));
    /// ```
    pub fn set_response(&mut self, tool_name: &str, response: Value) {
        self.responses.insert(tool_name.to_string(), response);
    }

    /// Simulates calling a tool on the server.
    ///
    /// Returns a predefined response if one was set with `set_response`,
    /// otherwise returns an error.
    ///
    /// # Errors
    ///
    /// Returns `MockServerError::ToolNotFound` if the tool doesn't exist on the server.
    /// Returns `MockServerError::InvalidParameters` if parameters are missing or invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::mock_server::MockMcpServer;
    /// use serde_json::json;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let server = MockMcpServer::new_vkteams_bot();
    /// let result = server.call_tool(
    ///     "send_message",
    ///     json!({"chat_id": "123", "text": "Hello"})
    /// ).await?;
    /// assert!(result.get("message_id").is_some());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_tool(&self, name: &str, params: Value) -> Result<Value, MockServerError> {
        // Verify tool exists
        let tool = self
            .info
            .tools
            .iter()
            .find(|t| t.name.as_str() == name)
            .ok_or_else(|| MockServerError::ToolNotFound(name.to_string()))?;

        // Basic parameter validation
        if let Some(obj) = params.as_object()
            && let Some(schema) = tool.input_schema.as_object()
            && let Some(required) = schema.get("required").and_then(|r| r.as_array())
        {
            for req_field in required {
                if let Some(field_name) = req_field.as_str()
                    && !obj.contains_key(field_name)
                {
                    return Err(MockServerError::InvalidParameters(format!(
                        "missing required field: {field_name}"
                    )));
                }
            }
        }

        // Return configured response or default
        Ok(self
            .responses
            .get(name)
            .cloned()
            .unwrap_or_else(|| json!({"success": true})))
    }

    /// Returns the list of available tool names.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_examples::mock_server::MockMcpServer;
    ///
    /// let server = MockMcpServer::new_vkteams_bot();
    /// let tools = server.tool_names();
    /// assert!(tools.contains(&"send_message".to_string()));
    /// ```
    #[must_use]
    pub fn tool_names(&self) -> Vec<String> {
        self.info
            .tools
            .iter()
            .map(|t| t.name.as_str().to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_new_vkteams_bot() {
        let server = MockMcpServer::new_vkteams_bot();
        let info = server.server_info();

        assert_eq!(info.name, "vkteams-bot");
        assert_eq!(info.version, "1.0.0");
        assert!(info.capabilities.supports_tools);
        assert_eq!(info.tools.len(), 4);
    }

    #[test]
    fn test_tool_names() {
        let server = MockMcpServer::new_vkteams_bot();
        let tools = server.tool_names();

        assert_eq!(tools.len(), 4);
        assert!(tools.contains(&"send_message".to_string()));
        assert!(tools.contains(&"get_message".to_string()));
        assert!(tools.contains(&"get_chat".to_string()));
        assert!(tools.contains(&"list_chats".to_string()));
    }

    #[tokio::test]
    async fn test_call_tool_success() {
        let server = MockMcpServer::new_vkteams_bot();
        let result = server
            .call_tool("send_message", json!({"chat_id": "123", "text": "Hello"}))
            .await
            .unwrap();

        assert!(result.get("message_id").is_some());
    }

    #[tokio::test]
    async fn test_call_tool_not_found() {
        let server = MockMcpServer::new_vkteams_bot();
        let result = server.call_tool("nonexistent_tool", json!({})).await;

        assert!(matches!(result, Err(MockServerError::ToolNotFound(_))));
    }

    #[tokio::test]
    async fn test_call_tool_missing_params() {
        let server = MockMcpServer::new_vkteams_bot();
        let result = server
            .call_tool("send_message", json!({"chat_id": "123"}))
            .await;

        assert!(matches!(result, Err(MockServerError::InvalidParameters(_))));
    }

    #[test]
    fn test_set_response() {
        let mut server = MockMcpServer::new_vkteams_bot();
        let custom_response = json!({"custom": "response"});

        server.set_response("send_message", custom_response.clone());

        // Response should be stored
        assert_eq!(server.responses.get("send_message"), Some(&custom_response));
    }
}
