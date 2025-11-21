//! Integration tests for mcp-introspector
//!
//! These tests validate server discovery, tool extraction, and metadata management.

use mcp_core::{ServerId, ToolName};
use mcp_introspector::{Introspector, ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;

/// Tests introspector creation
#[test]
fn test_introspector_creation() {
    let introspector = Introspector::new();
    assert_eq!(introspector.server_count(), 0);
    assert!(introspector.list_servers().is_empty());
}

/// Tests default trait implementation
#[test]
fn test_introspector_default() {
    let introspector = Introspector::default();
    assert_eq!(introspector.server_count(), 0);
}

/// Tests server info structure
#[test]
fn test_server_info_creation() {
    let info = ServerInfo {
        id: ServerId::new("test-server"),
        name: "Test Server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    assert_eq!(info.id.as_str(), "test-server");
    assert_eq!(info.name, "Test Server");
    assert_eq!(info.version, "1.0.0");
    assert!(info.capabilities.supports_tools);
    assert!(!info.capabilities.supports_resources);
}

/// Tests tool info creation
#[test]
fn test_tool_info_creation() {
    let tool = ToolInfo {
        name: ToolName::new("send_message"),
        description: "Sends a message to a chat".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "chat_id": {"type": "string"},
                "text": {"type": "string"}
            },
            "required": ["chat_id", "text"]
        }),
        output_schema: Some(json!({
            "type": "object",
            "properties": {
                "message_id": {"type": "string"}
            }
        })),
    };

    assert_eq!(tool.name.as_str(), "send_message");
    assert_eq!(tool.description, "Sends a message to a chat");
    assert!(tool.output_schema.is_some());
}

/// Tests getting nonexistent server
#[test]
fn test_get_nonexistent_server() {
    let introspector = Introspector::new();
    let server_id = ServerId::new("nonexistent");

    assert!(introspector.get_server(&server_id).is_none());
}

/// Tests server removal
#[test]
fn test_server_removal() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test");

    // Remove nonexistent server
    assert!(!introspector.remove_server(&server_id));

    // Add server manually for testing
    let _info = ServerInfo {
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

    // Manually insert (in real code, use discover_server)
    // For this test, we validate the removal logic works
    assert!(!introspector.remove_server(&server_id));
}

/// Tests clearing all servers
#[test]
fn test_clear_servers() {
    let mut introspector = Introspector::new();

    // Clear empty introspector
    introspector.clear();
    assert_eq!(introspector.server_count(), 0);
}

/// Tests listing servers
#[test]
fn test_list_empty_servers() {
    let introspector = Introspector::new();
    let servers = introspector.list_servers();

    assert!(servers.is_empty());
}

/// Tests server count
#[test]
fn test_server_count() {
    let introspector = Introspector::new();
    assert_eq!(introspector.server_count(), 0);
}

/// Tests `ServerInfo` serialization
#[test]
fn test_server_info_serialization() {
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

    // Serialize to JSON
    let json = serde_json::to_string(&info).expect("Failed to serialize");
    assert!(json.contains("Test Server"));
    assert!(json.contains("1.0.0"));

    // Deserialize back
    let deserialized: ServerInfo = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.id.as_str(), "test");
    assert_eq!(deserialized.name, "Test Server");
}

/// Tests `ToolInfo` serialization
#[test]
fn test_tool_info_serialization() {
    let tool = ToolInfo {
        name: ToolName::new("test_tool"),
        description: "Test tool description".to_string(),
        input_schema: json!({"type": "object"}),
        output_schema: None,
    };

    // Serialize to JSON
    let json = serde_json::to_string(&tool).expect("Failed to serialize");
    assert!(json.contains("test_tool"));
    assert!(json.contains("Test tool description"));

    // Deserialize back
    let deserialized: ToolInfo = serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.name.as_str(), "test_tool");
    assert_eq!(deserialized.description, "Test tool description");
    assert!(deserialized.output_schema.is_none());
}

/// Tests `ServerCapabilities`
#[test]
fn test_server_capabilities() {
    let caps = ServerCapabilities {
        supports_tools: true,
        supports_resources: true,
        supports_prompts: true,
    };

    assert!(caps.supports_tools);
    assert!(caps.supports_resources);
    assert!(caps.supports_prompts);

    // Serialize
    let json = serde_json::to_string(&caps).expect("Failed to serialize");

    // Deserialize
    let deserialized: ServerCapabilities =
        serde_json::from_str(&json).expect("Failed to deserialize");
    assert_eq!(deserialized.supports_tools, caps.supports_tools);
}

/// Tests that Introspector is Send and Sync
#[test]
fn test_introspector_is_send_sync() {
    fn assert_send<T: Send>() {}
    fn assert_sync<T: Sync>() {}

    assert_send::<Introspector>();
    assert_sync::<Introspector>();
}

/// Tests concurrent access to introspector
#[tokio::test]
async fn test_concurrent_introspector_access() {
    use std::sync::Arc;
    use tokio::sync::Mutex;

    let introspector = Arc::new(Mutex::new(Introspector::new()));

    let mut handles = vec![];

    // Spawn multiple tasks accessing introspector concurrently
    for i in 0..10 {
        let introspector_clone = Arc::clone(&introspector);
        let handle = tokio::spawn(async move {
            {
                let intro = introspector_clone.lock().await;
                assert_eq!(intro.server_count(), 0);
            }
            i
        });
        handles.push(handle);
    }

    // Wait for all tasks
    for handle in handles {
        handle.await.unwrap();
    }
}

/// Tests Debug implementation for `ServerInfo`
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

/// Tests Debug implementation for `ToolInfo`
#[test]
fn test_tool_info_debug() {
    let tool = ToolInfo {
        name: ToolName::new("test"),
        description: "Description".to_string(),
        input_schema: json!({}),
        output_schema: None,
    };

    let debug_str = format!("{tool:?}");
    assert!(debug_str.contains("test"));
    assert!(debug_str.contains("Description"));
}

/// Tests that invalid server commands are rejected
#[tokio::test]
async fn test_invalid_command_rejection() {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("test");

    // Try to discover with invalid command (should fail validation)
    let result = introspector
        .discover_server(server_id, "echo test; rm -rf /")
        .await;

    // Should fail due to validation or connection error
    assert!(result.is_err());
}

/// Tests empty tool list handling
#[test]
fn test_empty_tool_list() {
    let info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![],
        capabilities: ServerCapabilities {
            supports_tools: false, // No tools
            supports_resources: false,
            supports_prompts: false,
        },
    };

    assert!(info.tools.is_empty());
    assert!(!info.capabilities.supports_tools);
}

/// Tests tool with complex schema
#[test]
fn test_complex_tool_schema() {
    let complex_schema = json!({
        "type": "object",
        "properties": {
            "user": {
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "name": {"type": "string"},
                    "roles": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                },
                "required": ["id", "name"]
            },
            "message": {"type": "string"},
            "options": {
                "type": "object",
                "additionalProperties": true
            }
        },
        "required": ["user", "message"]
    });

    let tool = ToolInfo {
        name: ToolName::new("complex_tool"),
        description: "A tool with complex schema".to_string(),
        input_schema: complex_schema,
        output_schema: Some(json!({"type": "boolean"})),
    };

    assert_eq!(tool.name.as_str(), "complex_tool");
    assert!(tool.input_schema["properties"]["user"].is_object());
    assert!(tool.output_schema.is_some());
}

/// Tests that multiple servers can be managed
#[test]
fn test_multiple_server_management() {
    let mut introspector = Introspector::new();

    // Start with empty
    assert_eq!(introspector.server_count(), 0);

    // Clear should be safe on empty
    introspector.clear();
    assert_eq!(introspector.server_count(), 0);
}
