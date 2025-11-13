//! Security tests for mcp-skill-generator.
//!
//! Tests for:
//! - Path traversal prevention
//! - Command injection prevention
//! - Template injection prevention
//! - Unicode control character sanitization
//! - Shell metacharacter blocking

use crate::template_engine::TemplateEngine;
use crate::{ParameterContext, SkillContext, SkillName, ToolContext};
use mcp_core::{ServerId, ToolName};

#[test]
fn test_path_traversal_blocked() {
    // Unix-style path traversal
    assert!(SkillName::new("../etc/passwd").is_err());
    assert!(SkillName::new("../../etc/shadow").is_err());
    assert!(SkillName::new("./etc/passwd").is_err());

    // Windows-style path traversal
    assert!(SkillName::new("..\\windows\\system32").is_err());
    assert!(SkillName::new("..\\..\\windows\\system32").is_err());

    // Absolute paths
    assert!(SkillName::new("/etc/passwd").is_err());
    assert!(SkillName::new("/var/log/messages").is_err());
    assert!(SkillName::new("C:\\Windows\\System32").is_err());

    // Hidden files
    assert!(SkillName::new(".hidden").is_err());
    assert!(SkillName::new(".ssh/id_rsa").is_err());
}

#[test]
fn test_command_injection_blocked() {
    // Shell command separators
    assert!(SkillName::new("skill; rm -rf /").is_err());
    assert!(SkillName::new("skill && whoami").is_err());
    assert!(SkillName::new("skill || ls").is_err());
    assert!(SkillName::new("skill | cat").is_err());

    // Command substitution
    assert!(SkillName::new("skill`whoami`").is_err());
    assert!(SkillName::new("skill$(id)").is_err());
    assert!(SkillName::new("skill$USER").is_err());

    // Redirection
    assert!(SkillName::new("skill > /tmp/output").is_err());
    assert!(SkillName::new("skill < /etc/passwd").is_err());
    assert!(SkillName::new("skill >> /var/log/app.log").is_err());

    // Wildcards
    assert!(SkillName::new("skill*").is_err());
    assert!(SkillName::new("skill?").is_err());
    assert!(SkillName::new("skill[abc]").is_err());
}

#[test]
fn test_template_injection_blocked() {
    let engine = TemplateEngine::new().unwrap();

    // Template injection in description
    let context = SkillContext {
        name: "test".to_string(),
        description: "Normal}}{{#if true}}INJECTED{{/if}}{{#text".to_string(),
        server_id: ServerId::new("test-server"),
        tool_count: 0,
        tools: vec![],
        generator_version: "0.1.0".to_string(),
        generated_at: "2025-11-13T10:00:00Z".to_string(),
    };

    let result = engine.render_skill(&context);
    assert!(result.is_err());
    assert!(result.unwrap_err().is_validation_error());

    // Template injection in tool description
    let context = SkillContext {
        name: "test".to_string(),
        description: "Normal description".to_string(),
        server_id: ServerId::new("test-server"),
        tool_count: 1,
        tools: vec![ToolContext {
            name: ToolName::new("tool"),
            description: "Bad {{@root}} injection".to_string(),
            parameters: vec![],
        }],
        generator_version: "0.1.0".to_string(),
        generated_at: "2025-11-13T10:00:00Z".to_string(),
    };

    let result = engine.render_skill(&context);
    assert!(result.is_err());
    assert!(result.unwrap_err().is_validation_error());

    // Template injection in parameter description
    let context = SkillContext {
        name: "test".to_string(),
        description: "Normal description".to_string(),
        server_id: ServerId::new("test-server"),
        tool_count: 1,
        tools: vec![ToolContext {
            name: ToolName::new("tool"),
            description: "Normal tool".to_string(),
            parameters: vec![ParameterContext {
                name: "param".to_string(),
                type_name: "string".to_string(),
                required: true,
                description: "{{#each @root}}{{this}}{{/each}}".to_string(),
            }],
        }],
        generator_version: "0.1.0".to_string(),
        generated_at: "2025-11-13T10:00:00Z".to_string(),
    };

    let result = engine.render_skill(&context);
    assert!(result.is_err());
    assert!(result.unwrap_err().is_validation_error());
}

#[test]
fn test_unicode_control_chars_sanitized() {
    let sanitized = SkillContext::new_sanitized(
        "test",
        "Normal\u{202E}REVERSED\u{202D}text", // RTLO attack
        ServerId::new("server"),
        vec![],
        "0.1.0",
    );

    // Control characters should be removed
    assert!(!sanitized.description.contains('\u{202E}')); // RTLO
    assert!(!sanitized.description.contains('\u{202D}')); // LTR override
    assert_eq!(sanitized.description, "NormalREVERSEDtext");
}

#[test]
fn test_unicode_homograph_blocked() {
    // Cyrillic 'а' (U+0430) looks like Latin 'a' (U+0061)
    assert!(SkillName::new("skill\u{0430}").is_err());

    // Greek 'ο' (U+03BF) looks like Latin 'o' (U+006F)
    assert!(SkillName::new("skill\u{03BF}").is_err());

    // Only ASCII lowercase allowed
    assert!(SkillName::new("skill").is_ok());
}

#[test]
fn test_shell_metacharacters_blocked() {
    // Quotes
    assert!(SkillName::new("skill'name").is_err());
    assert!(SkillName::new("skill\"name").is_err());

    // Special characters
    assert!(SkillName::new("skill@name").is_err());
    assert!(SkillName::new("skill#name").is_err());
    assert!(SkillName::new("skill%name").is_err());
    assert!(SkillName::new("skill^name").is_err());
    assert!(SkillName::new("skill&name").is_err());
    assert!(SkillName::new("skill*name").is_err());
    assert!(SkillName::new("skill(name)").is_err());
    assert!(SkillName::new("skill{name}").is_err());
    assert!(SkillName::new("skill[name]").is_err());
}

#[test]
fn test_whitespace_preserved_in_sanitization() {
    let sanitized = SkillContext::new_sanitized(
        "test",
        "Line 1\nLine 2\tTabbed\n\nDouble newline",
        ServerId::new("server"),
        vec![],
        "0.1.0",
    );

    // Normal whitespace should be preserved
    assert!(sanitized.description.contains('\n'));
    assert!(sanitized.description.contains('\t'));
}

#[test]
fn test_safe_skill_name_accepted() {
    // Valid lowercase with hyphens and underscores
    assert!(SkillName::new("vkteams-bot").is_ok());
    assert!(SkillName::new("my_skill_name").is_ok());
    assert!(SkillName::new("skill123").is_ok());
    assert!(SkillName::new("a").is_ok());
    assert!(SkillName::new("a1b2c3").is_ok());
}

#[test]
fn test_length_bounds_enforced() {
    // Empty (too short)
    assert!(SkillName::new("").is_err());

    // 64 characters (max allowed)
    assert!(SkillName::new("a".repeat(64)).is_ok());

    // 65 characters (too long)
    assert!(SkillName::new("a".repeat(65)).is_err());

    // 100 characters (way too long)
    assert!(SkillName::new("a".repeat(100)).is_err());
}

#[test]
fn test_mixed_case_blocked() {
    assert!(SkillName::new("SkillName").is_err());
    assert!(SkillName::new("skillName").is_err());
    assert!(SkillName::new("SKILLNAME").is_err());
    assert!(SkillName::new("skill-Name").is_err());
}

#[test]
fn test_spaces_blocked() {
    assert!(SkillName::new("skill name").is_err());
    assert!(SkillName::new("skill  name").is_err());
    assert!(SkillName::new(" skill").is_err());
    assert!(SkillName::new("skill ").is_err());
}
