//! Integration tests for skill generation pipeline.
//!
//! Tests the full workflow from MCP server introspection to skill file generation.

#![cfg(feature = "skills")]

use mcp_codegen::TemplateEngine;
use mcp_codegen::skills::claude::{render_reference_md, render_skill_md};
use mcp_codegen::skills::converter::SkillConverter;
use mcp_core::{ServerId, SkillDescription, SkillName, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;

/// Creates a realistic mock ServerInfo for testing.
fn create_mock_server_info() -> ServerInfo {
    ServerInfo {
        id: ServerId::new("test-server"),
        name: "test-server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![
            ToolInfo {
                name: ToolName::new("send_message"),
                description: "Sends a message to a chat".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "string",
                            "description": "Chat identifier"
                        },
                        "text": {
                            "type": "string",
                            "description": "Message text to send"
                        },
                        "urgent": {
                            "type": "boolean",
                            "description": "Mark message as urgent"
                        }
                    },
                    "required": ["chat_id", "text"]
                }),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("get_chat_info"),
                description: "Retrieves information about a chat".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "string",
                            "description": "Chat identifier"
                        }
                    },
                    "required": ["chat_id"]
                }),
                output_schema: None,
            },
        ],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
}

#[test]
fn test_full_skill_generation_pipeline() {
    // 1. Create mock ServerInfo (simulate introspection)
    let server_info = create_mock_server_info();

    // 2. Create skill metadata
    let name = SkillName::new("test-skill").unwrap();
    let desc = SkillDescription::new("Test skill for MCP server integration").unwrap();

    // 3. Convert to SkillData
    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();

    // 4. Verify skill data structure
    assert_eq!(skill_data.skill_name, "test-skill");
    assert_eq!(
        skill_data.skill_description,
        "Test skill for MCP server integration"
    );
    assert_eq!(skill_data.server_name, "test-server");
    assert_eq!(skill_data.server_version, "1.0.0");
    assert_eq!(skill_data.tool_count, 2);
    assert_eq!(skill_data.tools.len(), 2);

    // 5. Render to SKILL.md
    let engine = TemplateEngine::new().unwrap();
    let skill_md = render_skill_md(&engine, &skill_data).unwrap();

    // 6. Verify SKILL.md output
    assert!(
        skill_md.starts_with("---\n"),
        "Should start with YAML frontmatter"
    );
    assert!(
        skill_md.contains("name: test-skill"),
        "Should contain skill name"
    );
    assert!(
        skill_md.contains("description: |"),
        "Should contain description field"
    );
    assert!(
        skill_md.contains("Test skill for MCP server integration"),
        "Should contain description text"
    );
    assert!(
        skill_md.contains("## Available Tools"),
        "Should have tools section"
    );
    assert!(
        skill_md.contains("send_message"),
        "Should list send_message tool"
    );
    assert!(
        skill_md.contains("get_chat_info"),
        "Should list get_chat_info tool"
    );

    // 7. Render to REFERENCE.md
    let reference_md = render_reference_md(&engine, &skill_data).unwrap();

    // 8. Verify REFERENCE.md output
    assert!(
        reference_md.contains("# test-server MCP Server Reference"),
        "Should have server reference header"
    );
    assert!(
        reference_md.contains("## send_message"),
        "Should document send_message tool"
    );
    assert!(
        reference_md.contains("## get_chat_info"),
        "Should document get_chat_info tool"
    );
    assert!(
        reference_md.contains("chat_id"),
        "Should document parameters"
    );
}

#[test]
fn test_pipeline_with_no_tools() {
    let server_info = ServerInfo {
        id: ServerId::new("empty-server"),
        name: "empty-server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![],
        capabilities: ServerCapabilities {
            supports_tools: false,
            supports_resources: true,
            supports_prompts: false,
        },
    };

    let name = SkillName::new("empty-skill").unwrap();
    let desc = SkillDescription::new("Skill with no tools").unwrap();

    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();
    assert_eq!(skill_data.tool_count, 0);

    let engine = TemplateEngine::new().unwrap();
    let skill_md = render_skill_md(&engine, &skill_data).unwrap();

    assert!(skill_md.contains("name: empty-skill"));
    assert!(skill_md.contains("## Available Tools"));
}

#[test]
fn test_pipeline_with_complex_parameters() {
    let server_info = ServerInfo {
        id: ServerId::new("complex-server"),
        name: "complex-server".to_string(),
        version: "2.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("complex_tool"),
            description: "Tool with complex parameter types".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "email": {
                        "type": "string",
                        "format": "email",
                        "description": "User email address"
                    },
                    "age": {
                        "type": "integer",
                        "description": "User age"
                    },
                    "tags": {
                        "type": "array",
                        "description": "User tags",
                        "items": {"type": "string"}
                    },
                    "metadata": {
                        "type": "object",
                        "description": "Additional metadata"
                    },
                    "enabled": {
                        "type": "boolean",
                        "description": "Whether feature is enabled"
                    }
                },
                "required": ["email", "age"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: true,
            supports_prompts: true,
        },
    };

    let name = SkillName::new("complex-skill").unwrap();
    let desc = SkillDescription::new("Skill with complex parameters").unwrap();

    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();

    let engine = TemplateEngine::new().unwrap();
    let skill_md = render_skill_md(&engine, &skill_data).unwrap();

    assert!(skill_md.contains("complex_tool"));
    assert!(skill_md.contains("email"));
    assert!(skill_md.contains("age"));
    assert!(skill_md.contains("tags"));
    assert!(skill_md.contains("metadata"));
    assert!(skill_md.contains("enabled"));

    let reference_md = render_reference_md(&engine, &skill_data).unwrap();

    assert!(reference_md.contains("## complex_tool"));
    assert!(reference_md.contains("email"));
    assert!(reference_md.contains("user@example.com")); // Email format example
}

#[test]
fn test_pipeline_with_enum_parameter() {
    let server_info = ServerInfo {
        id: ServerId::new("enum-server"),
        name: "enum-server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("set_status"),
            description: "Sets user status".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "status": {
                        "enum": ["online", "offline", "away", "busy"],
                        "description": "User status"
                    }
                },
                "required": ["status"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let name = SkillName::new("status-skill").unwrap();
    let desc = SkillDescription::new("Manages user status").unwrap();

    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();
    let tool = &skill_data.tools[0];
    let status_param = tool.parameters.iter().find(|p| p.name == "status").unwrap();

    // Enum should be treated as string type
    assert_eq!(status_param.type_name, "string");
    // Example should be first enum value
    assert_eq!(status_param.example_value, r#""online""#);
}

#[test]
fn test_pipeline_preserves_tool_order() {
    let server_info = ServerInfo {
        id: ServerId::new("ordered-server"),
        name: "ordered-server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![
            ToolInfo {
                name: ToolName::new("first_tool"),
                description: "First".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("second_tool"),
                description: "Second".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("third_tool"),
                description: "Third".to_string(),
                input_schema: json!({"type": "object", "properties": {}}),
                output_schema: None,
            },
        ],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let name = SkillName::new("ordered-skill").unwrap();
    let desc = SkillDescription::new("Tools in order").unwrap();

    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();

    assert_eq!(skill_data.tools[0].name, "first_tool");
    assert_eq!(skill_data.tools[1].name, "second_tool");
    assert_eq!(skill_data.tools[2].name, "third_tool");
}

#[test]
fn test_pipeline_with_all_capabilities() {
    let server_info = ServerInfo {
        id: ServerId::new("full-server"),
        name: "full-server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: true,
            supports_prompts: true,
        },
    };

    let name = SkillName::new("full-skill").unwrap();
    let desc = SkillDescription::new("Skill with all capabilities").unwrap();

    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();

    assert_eq!(
        skill_data.capabilities,
        vec!["tools", "resources", "prompts"]
    );
}

#[test]
fn test_pipeline_yaml_frontmatter_format() {
    let server_info = create_mock_server_info();
    let name = SkillName::new("yaml-test").unwrap();
    let desc = SkillDescription::new("Testing YAML frontmatter format").unwrap();

    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();

    let engine = TemplateEngine::new().unwrap();
    let skill_md = render_skill_md(&engine, &skill_data).unwrap();

    // Verify YAML frontmatter structure
    assert!(skill_md.starts_with("---\n"));
    let lines: Vec<&str> = skill_md.lines().collect();

    // Find the closing --- (skip the first one at index 0)
    let frontmatter_end = lines
        .iter()
        .skip(1)
        .position(|line| *line == "---")
        .map(|pos| pos + 1) // Adjust for skip(1)
        .unwrap_or(0);
    assert!(frontmatter_end > 1, "YAML frontmatter should have content");

    // Verify required YAML fields are present
    let frontmatter = &lines[1..frontmatter_end].join("\n");
    assert!(frontmatter.contains("name:"));
    assert!(frontmatter.contains("description:"));
}

#[test]
fn test_pipeline_json_schema_formatting() {
    let server_info = ServerInfo {
        id: ServerId::new("schema-server"),
        name: "schema-server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("test_tool"),
            description: "Test".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "param": {
                        "type": "string"
                    }
                }
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let name = SkillName::new("schema-skill").unwrap();
    let desc = SkillDescription::new("Schema formatting test").unwrap();

    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();

    // Verify JSON schema is pretty-printed
    let tool = &skill_data.tools[0];
    assert!(
        tool.input_schema_json.contains('\n'),
        "Schema should be multi-line"
    );
    assert!(
        tool.input_schema_json.contains("  "),
        "Schema should be indented"
    );
}

#[test]
fn test_pipeline_timestamp_validity() {
    let server_info = create_mock_server_info();
    let name = SkillName::new("timestamp-test").unwrap();
    let desc = SkillDescription::new("Testing timestamp generation").unwrap();

    let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();

    // Verify timestamp is valid RFC3339
    assert!(chrono::DateTime::parse_from_rfc3339(&skill_data.generated_at).is_ok());
}

#[test]
fn test_pipeline_error_handling_invalid_schema() {
    let server_info = ServerInfo {
        id: ServerId::new("invalid-server"),
        name: "invalid-server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("bad_tool"),
            description: "Tool with invalid schema".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": "this should be an object, not a string"
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let name = SkillName::new("error-skill").unwrap();
    let desc = SkillDescription::new("Error handling test").unwrap();

    let result = SkillConverter::convert(&server_info, &name, &desc);
    assert!(result.is_err(), "Should fail on invalid schema");
}
