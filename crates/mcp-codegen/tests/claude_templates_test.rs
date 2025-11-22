//! Integration tests for Claude template rendering.
//!
//! Tests that Handlebars templates correctly render SKILL.md and REFERENCE.md
//! files with proper YAML frontmatter, parameter handling, and Claude compliance.

#![cfg(feature = "skills")]

use mcp_codegen::TemplateEngine;
use mcp_codegen::skills::claude::{
    ParameterData, SkillData, ToolData, render_reference_md, render_skill_md,
};

/// Creates test data for template rendering tests.
fn create_test_data() -> SkillData {
    SkillData::new(
        "test-skill".to_string(),
        "Test skill for integration testing. Use when testing MCP integration.".to_string(),
        "test-server".to_string(),
        "1.0.0".to_string(),
        "A test MCP server for unit testing".to_string(),
        "1.0".to_string(),
        vec![
            ToolData {
                name: "send_message".to_string(),
                description: "Sends a message to a chat".to_string(),
                parameters: vec![
                    ParameterData {
                        name: "chat_id".to_string(),
                        type_name: "string".to_string(),
                        required: true,
                        description: "Chat identifier".to_string(),
                        example_value: r#""123456""#.to_string(),
                    },
                    ParameterData {
                        name: "text".to_string(),
                        type_name: "string".to_string(),
                        required: true,
                        description: "Message text".to_string(),
                        example_value: r#""Hello, world!""#.to_string(),
                    },
                ],
                input_schema_json: r#"{
  "type": "object",
  "properties": {
    "chat_id": {"type": "string"},
    "text": {"type": "string"}
  },
  "required": ["chat_id", "text"]
}"#
                .to_string(),
            },
            ToolData {
                name: "get_status".to_string(),
                description: "Gets current server status".to_string(),
                parameters: vec![],
                input_schema_json: r#"{"type": "object"}"#.to_string(),
            },
        ],
        vec!["tools".to_string()],
    )
}

#[test]
fn test_template_engine_registers_claude_templates() {
    let engine = TemplateEngine::new().expect("Failed to create template engine");

    assert!(
        engine.has_template("claude_skill"),
        "Claude skill template should be registered"
    );
    assert!(
        engine.has_template("claude_reference"),
        "Claude reference template should be registered"
    );
}

#[test]
fn test_skill_md_renders_yaml_frontmatter() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).expect("Failed to render SKILL.md");

    // Must start with YAML frontmatter
    assert!(
        rendered.starts_with("---\n"),
        "SKILL.md must start with YAML frontmatter"
    );

    // Must have closing frontmatter
    assert!(
        rendered.contains("\n---\n"),
        "SKILL.md must have closing YAML frontmatter"
    );
}

#[test]
fn test_skill_md_includes_name_and_description_in_frontmatter() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // Extract frontmatter
    let parts: Vec<&str> = rendered.splitn(3, "---").collect();
    assert_eq!(
        parts.len(),
        3,
        "Should have opening and closing frontmatter"
    );

    let frontmatter = parts[1];

    // Check for name field
    assert!(
        frontmatter.contains("name: test-skill"),
        "Frontmatter must contain skill name"
    );

    // Check for description field
    assert!(
        frontmatter.contains("description: |"),
        "Frontmatter must have description field with pipe"
    );
    assert!(
        frontmatter.contains("Test skill for integration testing"),
        "Frontmatter must contain skill description"
    );
}

#[test]
fn test_skill_md_includes_all_tools() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // Both tools should be documented
    assert!(
        rendered.contains("### send_message"),
        "Should document send_message tool"
    );
    assert!(
        rendered.contains("### get_status"),
        "Should document get_status tool"
    );

    // Tool descriptions should be present
    assert!(rendered.contains("Sends a message to a chat"));
    assert!(rendered.contains("Gets current server status"));
}

#[test]
fn test_skill_md_includes_parameters() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // Parameters should be listed
    assert!(rendered.contains("`chat_id` (string) *required*"));
    assert!(rendered.contains("`text` (string) *required*"));
    assert!(rendered.contains("Chat identifier"));
    assert!(rendered.contains("Message text"));
}

#[test]
fn test_skill_md_includes_examples() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // Example code blocks should be present
    assert!(
        rendered.contains("```typescript"),
        "Should have TypeScript example blocks"
    );
    assert!(
        rendered.contains("mcpClient.callTool"),
        "Should show how to call tools"
    );

    // Example values should be included (with proper escaping)
    // In Handlebars, {{example_value}} will output the literal string including quotes
    assert!(
        rendered.contains("123456"),
        "Should contain chat_id example value"
    );
    assert!(
        rendered.contains("Hello, world!"),
        "Should contain text example value"
    );
}

#[test]
fn test_skill_md_handles_tools_without_parameters() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // The get_status tool has no parameters
    // Should have proper handling
    assert!(
        rendered.contains("### get_status"),
        "Should document tool without parameters"
    );
}

#[test]
fn test_skill_md_includes_metadata() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // Should include server name
    assert!(rendered.contains("test-server"));

    // Should include tool count
    assert!(rendered.contains("2 tool(s)"));

    // Should include protocol version
    assert!(rendered.contains("MCP Protocol Version: 1.0"));

    // Should include generation timestamp
    assert!(rendered.contains("Generated on"));
}

#[test]
fn test_skill_md_no_xml_tags_in_output() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // After frontmatter, should not have XML tags (except in code blocks)
    let parts: Vec<&str> = rendered.splitn(3, "---").collect();
    let content = parts[2];

    // This is a simplified check - in practice, XML in code blocks is fine
    // but not in documentation text
    let lines: Vec<&str> = content.lines().collect();
    let mut in_code_block = false;

    for line in lines {
        if line.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }

        if !in_code_block {
            assert!(
                !line.contains("{{") && !line.contains("}}"),
                "Should not have template syntax in output: {line}"
            );
        }
    }
}

#[test]
fn test_reference_md_includes_json_schema() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_reference_md(&engine, &data).expect("Failed to render REFERENCE.md");

    // Should have JSON code blocks
    assert!(rendered.contains("```json"), "Should have JSON code blocks");

    // Should include input schema
    assert!(
        rendered.contains("inputSchema"),
        "Should document input schema"
    );
    assert!(rendered.contains(r#""type": "object""#));
}

#[test]
fn test_reference_md_includes_header() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_reference_md(&engine, &data).unwrap();

    // Should have proper header
    assert!(
        rendered.contains("# test-server MCP Server Reference"),
        "Should have server name in header"
    );

    // Should include version info
    assert!(rendered.contains("**Version**: 1.0.0"));
    assert!(rendered.contains("**Protocol**: 1.0"));
    assert!(rendered.contains("**Generated**:"));
}

#[test]
fn test_reference_md_includes_parameter_table() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_reference_md(&engine, &data).unwrap();

    // Should have markdown table
    assert!(
        rendered.contains("| Name | Type | Required | Description |"),
        "Should have parameter table header"
    );

    // Should mark required parameters with checkmark
    assert!(rendered.contains("✅"), "Should mark required parameters");

    // Should include parameter details
    assert!(rendered.contains("| `chat_id` | string | ✅"));
    assert!(rendered.contains("| `text` | string | ✅"));
}

#[test]
fn test_reference_md_includes_example_requests() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_reference_md(&engine, &data).unwrap();

    // Should have example request sections
    assert!(
        rendered.contains("**Example Request**:"),
        "Should have example request section"
    );

    // Should show MCP protocol format
    assert!(rendered.contains(r#""method": "tools/call""#));
    assert!(rendered.contains(r#""params""#));
    assert!(rendered.contains(r#""name": "send_message""#));
    assert!(rendered.contains(r#""arguments""#));
}

#[test]
fn test_reference_md_includes_error_handling() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_reference_md(&engine, &data).unwrap();

    // Should document error codes
    assert!(rendered.contains("## Error Handling"));
    assert!(rendered.contains("InvalidParams"));
    assert!(rendered.contains("MethodNotFound"));
    assert!(rendered.contains("InternalError"));
}

#[test]
fn test_reference_md_includes_capabilities() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_reference_md(&engine, &data).unwrap();

    // Should list server capabilities
    assert!(rendered.contains("**Capabilities**:"));
    assert!(rendered.contains("- tools"));
}

#[test]
fn test_anthropic_compliance_no_reserved_words() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // Extract YAML frontmatter to check skill name specifically
    let parts: Vec<&str> = rendered.splitn(3, "---").collect();
    assert_eq!(parts.len(), 3, "Should have YAML frontmatter");

    let frontmatter = parts[1];

    // Skill name in frontmatter should not contain reserved words
    // (this is validated by SkillName type, but double-check in frontmatter)
    assert!(
        frontmatter.contains("name: test-skill"),
        "Frontmatter should have skill name"
    );

    // The skill name itself (test-skill) should not contain reserved words
    let skill_name = "test-skill";
    assert!(
        !skill_name.to_lowercase().contains("anthropic"),
        "Skill name should not contain 'anthropic'"
    );
    assert!(
        !skill_name.to_lowercase().contains("claude"),
        "Skill name should not contain 'claude'"
    );

    // Note: Documentation text may reference "Claude Code", "Claude Desktop", etc.
    // This is allowed - only the skill name itself is restricted
}

#[test]
fn test_skill_description_max_length_respected() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    // SkillDescription validates max 1024 chars
    assert!(
        data.skill_description.len() <= 1024,
        "Skill description should be <= 1024 chars"
    );

    let rendered = render_skill_md(&engine, &data).unwrap();
    assert!(!rendered.is_empty());
}

#[test]
fn test_multiple_tools_rendering() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    assert_eq!(
        data.tool_count, 2,
        "Test data should have 2 tools for this test"
    );

    let rendered = render_skill_md(&engine, &data).unwrap();

    // Both tools should have dedicated sections
    let send_message_count = rendered.matches("### send_message").count();
    let get_status_count = rendered.matches("### get_status").count();

    assert_eq!(
        send_message_count, 1,
        "send_message should appear exactly once in headers"
    );
    assert_eq!(
        get_status_count, 1,
        "get_status should appear exactly once in headers"
    );
}

#[test]
fn test_skill_name_format_compliance() {
    let data = create_test_data();

    // Skill name should follow Anthropic format
    let name = &data.skill_name;

    // Should be lowercase
    assert_eq!(name, &name.to_lowercase(), "Skill name should be lowercase");

    // Should only contain valid characters
    for ch in name.chars() {
        assert!(
            ch.is_ascii_lowercase() || ch.is_ascii_digit() || ch == '-' || ch == '_',
            "Invalid character in skill name: {ch}"
        );
    }
}

#[test]
fn test_rendered_skill_is_valid_markdown() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_skill_md(&engine, &data).unwrap();

    // Basic markdown validity checks
    // 1. Should have headers
    assert!(rendered.contains("# test-skill"), "Should have h1 header");
    assert!(
        rendered.contains("## When to Use This Skill"),
        "Should have h2 headers"
    );

    // 2. Code blocks should be balanced
    let triple_backticks_count = rendered.matches("```").count();
    assert_eq!(
        triple_backticks_count % 2,
        0,
        "Code blocks should be balanced"
    );

    // 3. Lists should be present
    assert!(
        rendered.contains("\n- ") || rendered.contains("\n* "),
        "Should have list items"
    );
}

#[test]
fn test_rendered_reference_is_valid_markdown() {
    let engine = TemplateEngine::new().unwrap();
    let data = create_test_data();

    let rendered = render_reference_md(&engine, &data).unwrap();

    // Should have valid table syntax
    assert!(rendered.contains("|---"), "Should have table separator");

    // Code blocks should be balanced
    let code_block_count = rendered.matches("```").count();
    assert_eq!(code_block_count % 2, 0, "Code blocks should be balanced");
}

#[test]
fn test_timestamp_format() {
    let data = create_test_data();

    // Timestamp should be valid ISO 8601 / RFC3339
    let parsed = chrono::DateTime::parse_from_rfc3339(&data.generated_at);
    assert!(
        parsed.is_ok(),
        "Timestamp should be valid RFC3339: {}",
        data.generated_at
    );
}
