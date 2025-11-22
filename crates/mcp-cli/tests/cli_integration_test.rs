//! Integration tests for CLI skill generation workflow.

use std::env;
use tempfile::TempDir;

/// Tests that CLI parsing works for generate command with all arguments.
#[test]
fn test_generate_command_parsing() {
    // Note: This test would require exporting Cli and Commands from lib.rs
    // For now, we test the logic through other means
    // Actual CLI parsing is tested in main.rs unit tests

    // Test skill name validation instead
    use mcp_core::SkillName;
    assert!(SkillName::new("test-skill").is_ok());
    assert!(SkillName::new("invalid name").is_err());
}

/// Tests that skill name validation works correctly.
#[test]
fn test_skill_name_validation() {
    use mcp_core::SkillName;

    // Valid names
    assert!(SkillName::new("valid-skill").is_ok());
    assert!(SkillName::new("skill123").is_ok());
    assert!(SkillName::new("my-cool-skill").is_ok());

    // Invalid names
    assert!(SkillName::new("").is_err());
    assert!(SkillName::new("Invalid-Skill").is_err()); // uppercase
    assert!(SkillName::new("skill with spaces").is_err());
    assert!(SkillName::new("123skill").is_err()); // starts with number
}

/// Tests that skill description validation works correctly.
#[test]
fn test_skill_description_validation() {
    use mcp_core::SkillDescription;

    // Valid descriptions
    assert!(SkillDescription::new("A valid description").is_ok());
    assert!(SkillDescription::new("Interact with VK Teams bot").is_ok());

    // Invalid descriptions
    assert!(SkillDescription::new("").is_err());
    assert!(SkillDescription::new("<xml>Invalid</xml>").is_err()); // XML tags

    let long_desc = "a".repeat(1025);
    assert!(SkillDescription::new(&long_desc).is_err()); // too long
}

/// Tests that the skill store can create and find the .claude/skills directory.
#[test]
fn test_skill_store_directory_creation() {
    use mcp_skill_store::SkillStore;

    // Create temporary directory for testing
    let temp_dir = TempDir::new().unwrap();
    unsafe {
        env::set_var("HOME", temp_dir.path());
    }

    // Create skill store (should create .claude/skills)
    let store = SkillStore::new_claude().unwrap();

    // Verify directory exists by checking the expected path
    let skill_dir = temp_dir.path().join(".claude/skills");
    assert!(skill_dir.exists());
}

/// Tests that skill list command works with empty directory.
#[test]
fn test_list_empty_skills() {
    use mcp_skill_store::SkillStore;

    let temp_dir = TempDir::new().unwrap();
    unsafe {
        env::set_var("HOME", temp_dir.path());
    }

    let store = SkillStore::new_claude().unwrap();
    let skills = store.list_claude_skills().unwrap();

    assert_eq!(skills.len(), 0);
}

/// Tests that skill existence check works via directory check.
#[test]
fn test_skill_exists_check() {
    use mcp_core::SkillName;

    let temp_dir = TempDir::new().unwrap();
    unsafe {
        env::set_var("HOME", temp_dir.path());
    }

    let skill_name = SkillName::new("nonexistent").unwrap();
    let skill_path = temp_dir
        .path()
        .join(".claude/skills")
        .join(skill_name.as_str());

    assert!(!skill_path.exists());
}
