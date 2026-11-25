//! Integration tests for mcp-introspector
//!
//! These tests validate server discovery, tool extraction, and metadata management.

use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{Introspector, ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;

/// Tests server info structure
#[test]
fn test_server_info_creation() {
    let info = ServerInfo::new(
        ServerId::new("test-server").unwrap(),
        "Test Server".to_string(),
        "1.0.0".to_string(),
        vec![],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    assert_eq!(info.id.as_str(), "test-server");
    assert_eq!(info.name, "Test Server");
    assert_eq!(info.version, "1.0.0");
    assert!(info.capabilities.supports_tools);
    assert!(!info.capabilities.supports_resources);
}

/// Tests tool info creation
#[test]
fn test_tool_info_creation() {
    let tool = {
        let mut value = ToolInfo::new(
            ToolName::new("send_message").unwrap(),
            "Sends a message to a chat".to_string(),
            json!({
                "type": "object",
                "properties": {
                    "chat_id": {"type": "string"},
                    "text": {"type": "string"}
                },
                "required": ["chat_id", "text"]
            }),
        );
        value.output_schema = Some(json!({
            "type": "object",
            "properties": {
                "message_id": {"type": "string"}
            }
        }));
        value
    };

    assert_eq!(tool.name.as_str(), "send_message");
    assert_eq!(tool.description, "Sends a message to a chat");
    assert!(tool.output_schema.is_some());
}

/// Tests `ServerInfo` serialization
#[test]
fn test_server_info_serialization() {
    let info = ServerInfo::new(
        ServerId::new("test").unwrap(),
        "Test Server".to_string(),
        "1.0.0".to_string(),
        vec![],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

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
    let tool = ToolInfo::new(
        ToolName::new("test_tool").unwrap(),
        "Test tool description".to_string(),
        json!({"type": "object"}),
    );

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
    let caps = {
        let mut value = ServerCapabilities::default();
        value.supports_tools = true;
        value.supports_resources = true;
        value.supports_prompts = true;
        value
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

/// Tests Debug implementation for `ServerInfo`
#[test]
fn test_server_info_debug() {
    let info = ServerInfo::new(
        ServerId::new("test").unwrap(),
        "Test Server".to_string(),
        "1.0.0".to_string(),
        vec![],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    let debug_str = format!("{info:?}");
    assert!(debug_str.contains("Test Server"));
    assert!(debug_str.contains("1.0.0"));
}

/// Tests Debug implementation for `ToolInfo`
#[test]
fn test_tool_info_debug() {
    let tool = ToolInfo::new(
        ToolName::new("test").unwrap(),
        "Description".to_string(),
        json!({}),
    );

    let debug_str = format!("{tool:?}");
    assert!(debug_str.contains("test"));
    assert!(debug_str.contains("Description"));
}

/// Tests that invalid server commands are rejected at construction time —
/// `ServerConfigBuilder::build()` now runs security validation itself, so an
/// unvalidated `ServerConfig` never reaches `discover_server`.
#[test]
fn test_invalid_command_rejection() {
    let result = ServerConfig::builder()
        .command("echo test; rm -rf /".to_string())
        .build();

    assert!(result.is_err());
}

/// Tests empty tool list handling
#[test]
fn test_empty_tool_list() {
    let info = ServerInfo::new(
        ServerId::new("test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        vec![],
        ServerCapabilities::default(),
    );

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

    let tool = {
        let mut value = ToolInfo::new(
            ToolName::new("complex_tool").unwrap(),
            "A tool with complex schema".to_string(),
            complex_schema,
        );
        value.output_schema = Some(json!({"type": "boolean"}));
        value
    };

    assert_eq!(tool.name.as_str(), "complex_tool");
    assert!(tool.input_schema["properties"]["user"].is_object());
    assert!(tool.output_schema.is_some());
}

/// Tests `discover_server` with empty command
#[test]
fn test_discover_server_empty_command() {
    // Empty command should fail during build
    let result = ServerConfig::builder().command(String::new()).build();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

/// Tests `discover_server` with whitespace command
#[test]
fn test_discover_server_whitespace_command() {
    // Whitespace-only command should fail during build
    let result = ServerConfig::builder().command("   ".to_string()).build();
    assert!(result.is_err());
    assert!(result.unwrap_err().to_string().contains("empty"));
}

/// Tests that commands containing shell operators are rejected at
/// construction time (see `test_invalid_command_rejection`'s doc comment).
#[test]
fn test_discover_server_shell_operators() {
    let dangerous_commands = vec![
        "cmd && malicious",
        "cmd | pipe",
        "cmd > redirect",
        "cmd $(sub)",
        "cmd `backtick`",
        "cmd;chain",
    ];

    for cmd in dangerous_commands {
        let result = ServerConfig::builder().command(cmd.to_string()).build();
        assert!(result.is_err(), "Command should be rejected: {cmd}");
    }
}

/// Tests `discover_server` with nonexistent command
#[tokio::test]
async fn test_discover_server_nonexistent_command() {
    let introspector = Introspector::new();
    let server_id = ServerId::new("test").unwrap();

    // A bare (non-absolute) binary name passes construction-time validation
    // (it's resolved via PATH at spawn time), so the failure below comes
    // from the process spawn itself, not from `build()`.
    let config = ServerConfig::builder()
        .command("nonexistent-command-12345".to_string())
        .build()
        .unwrap();
    let result = introspector.discover_server(server_id, &config).await;

    // Should fail to spawn process
    assert!(result.is_err());
}

/// Tests `ServerInfo` with tools containing empty descriptions
#[test]
fn test_tool_info_empty_description() {
    let tool = ToolInfo::new(
        ToolName::new("test").unwrap(),
        String::new(),
        json!({"type": "null"}),
    );

    assert_eq!(tool.description, "");
    assert!(tool.output_schema.is_none());
}

/// Tests `ServerInfo` with maximum capabilities
#[test]
fn test_server_capabilities_all_enabled() {
    let caps = {
        let mut value = ServerCapabilities::default();
        value.supports_tools = true;
        value.supports_resources = true;
        value.supports_prompts = true;
        value
    };

    assert!(caps.supports_tools);
    assert!(caps.supports_resources);
    assert!(caps.supports_prompts);
}

/// Tests `ServerInfo` with all capabilities disabled
#[test]
fn test_server_capabilities_all_disabled() {
    let caps = ServerCapabilities::default();

    assert!(!caps.supports_tools);
    assert!(!caps.supports_resources);
    assert!(!caps.supports_prompts);
}

/// Tests `ServerInfo` clone
#[test]
fn test_server_info_clone() {
    let info = ServerInfo::new(
        ServerId::new("test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        vec![],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    let cloned = info.clone();
    assert_eq!(cloned.id.as_str(), info.id.as_str());
    assert_eq!(cloned.name, info.name);
    assert_eq!(cloned.version, info.version);
}

/// Tests `ToolInfo` clone
#[test]
fn test_tool_info_clone() {
    let tool = ToolInfo::new(
        ToolName::new("test").unwrap(),
        "Test".to_string(),
        json!({}),
    );

    let cloned = tool.clone();
    assert_eq!(cloned.name.as_str(), tool.name.as_str());
    assert_eq!(cloned.description, tool.description);
}

/// Tests `ServerCapabilities` clone
#[test]
fn test_server_capabilities_clone() {
    let caps = {
        let mut value = ServerCapabilities::default();
        value.supports_tools = true;
        value.supports_resources = true;
        value
    };

    let cloned = caps.clone();
    assert_eq!(cloned.supports_tools, caps.supports_tools);
    assert_eq!(cloned.supports_resources, caps.supports_resources);
    assert_eq!(cloned.supports_prompts, caps.supports_prompts);
}

/// Tests `ServerInfo` with many tools
#[test]
fn test_server_info_with_many_tools() {
    let tools: Vec<ToolInfo> = (0..100)
        .map(|i| {
            ToolInfo::new(
                ToolName::new(&format!("tool_{i}")).unwrap(),
                format!("Tool number {i}"),
                json!({"type": "object"}),
            )
        })
        .collect();

    let info = ServerInfo::new(
        ServerId::new("test").unwrap(),
        "Test".to_string(),
        "1.0.0".to_string(),
        tools,
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    assert_eq!(info.tools.len(), 100);
    assert!(info.capabilities.supports_tools);
}

/// Tests `ToolInfo` with null input schema
#[test]
fn test_tool_info_null_schema() {
    let tool = {
        let mut value = ToolInfo::new(
            ToolName::new("test").unwrap(),
            "Test".to_string(),
            json!(null),
        );
        value.output_schema = Some(json!(null));
        value
    };

    assert!(tool.input_schema.is_null());
    assert!(tool.output_schema.is_some());
}

/// Tests `ServerInfo` with special characters in name
#[test]
fn test_server_info_special_chars() {
    let info = ServerInfo::new(
        ServerId::new("test-server-123").unwrap(),
        "Test Server (v1.0) [beta]".to_string(),
        "1.0.0-alpha.1+build.123".to_string(),
        vec![],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    assert!(info.name.contains('('));
    assert!(info.version.contains('+'));
}

/// Tests `ToolInfo` with very long description
#[test]
fn test_tool_info_long_description() {
    let long_desc = "a".repeat(10000);

    let tool = ToolInfo::new(ToolName::new("test").unwrap(), long_desc.clone(), json!({}));

    assert_eq!(tool.description.len(), 10000);
    assert_eq!(tool.description, long_desc);
}

/// Tests `ServerCapabilities` Debug
#[test]
fn test_server_capabilities_debug() {
    let caps = {
        let mut value = ServerCapabilities::default();
        value.supports_tools = true;
        value.supports_prompts = true;
        value
    };

    let debug_str = format!("{caps:?}");
    assert_ne!(debug_str, "");
}

/// Tests Introspector Debug
#[test]
fn test_introspector_debug() {
    let introspector = Introspector::new();
    let debug_str = format!("{introspector:?}");

    assert!(debug_str.contains("Introspector"));
}

/// Tests `ToolInfo` with nested schema
#[test]
fn test_tool_info_nested_schema() {
    let nested_schema = json!({
        "type": "object",
        "properties": {
            "level1": {
                "type": "object",
                "properties": {
                    "level2": {
                        "type": "object",
                        "properties": {
                            "level3": {"type": "string"}
                        }
                    }
                }
            }
        }
    });

    let tool = {
        let mut value = ToolInfo::new(
            ToolName::new("nested").unwrap(),
            "Nested schema test".to_string(),
            nested_schema.clone(),
        );
        value.output_schema = Some(nested_schema);
        value
    };

    assert!(tool.input_schema["properties"]["level1"].is_object());
    assert!(tool.output_schema.is_some());
}

/// Tests `ServerInfo` serialization with tools
#[test]
fn test_server_info_serialization_with_tools() {
    let tool = ToolInfo::new(
        ToolName::new("test_tool").unwrap(),
        "Test".to_string(),
        json!({"type": "object"}),
    );

    let info = ServerInfo::new(
        ServerId::new("test").unwrap(),
        "Test Server".to_string(),
        "1.0.0".to_string(),
        vec![tool],
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    );

    let json = serde_json::to_string(&info).expect("Serialization failed");
    assert!(json.contains("test_tool"));
    assert!(json.contains("Test Server"));

    let deserialized: ServerInfo = serde_json::from_str(&json).expect("Deserialization failed");
    assert_eq!(deserialized.tools.len(), 1);
    assert_eq!(deserialized.tools[0].name.as_str(), "test_tool");
}
