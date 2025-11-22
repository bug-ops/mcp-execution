//! Comprehensive tests for Claude skill format.
//!
//! Tests cover:
//! - Save and load (round-trip)
//! - List skills
//! - Remove skills
//! - Checksum verification
//! - Missing file handling
//! - Invalid skill names
//! - Concurrent operations
//! - Atomic writes

use mcp_codegen::skills::claude::{SkillData, ToolData};
use mcp_core::SkillName;
use mcp_skill_store::{
    CLAUDE_METADATA_FILE, CLAUDE_REFERENCE_FILE, CLAUDE_SKILL_FILE, ClaudeSkillMetadata,
    SkillStore, SkillStoreError,
};
use std::fs;
use tempfile::TempDir;

/// Creates test `SkillData` for testing.
fn create_test_skill_data(name: &str, tool_count: usize) -> SkillData {
    let tools: Vec<ToolData> = (0..tool_count)
        .map(|i| ToolData {
            name: format!("tool_{i}"),
            description: format!("Tool {i} description"),
            parameters: vec![],
            input_schema_json: r#"{"type": "object"}"#.to_string(),
        })
        .collect();

    SkillData::new(
        name.to_string(),
        format!("{name} skill description"),
        format!("{name}-server"),
        "1.0.0".to_string(),
        format!("{name} MCP server"),
        "1.0".to_string(),
        tools,
        vec!["tools".to_string()],
    )
}

#[test]
fn test_new_claude_with_temp_dir() {
    let temp = TempDir::new().unwrap();
    let _store = SkillStore::new(temp.path()).unwrap();

    // Store should be created successfully
    assert!(temp.path().exists());
}

#[test]
fn test_save_claude_skill_success() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("test-skill").unwrap();
    let skill_data = create_test_skill_data("test-skill", 2);
    let skill_md = "# Test Skill\n\nThis is a test skill.";
    let reference_md = "# Reference\n\nReference documentation.";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Verify directory structure
    let skill_dir = temp.path().join("test-skill");
    assert!(skill_dir.exists());
    assert!(skill_dir.join(CLAUDE_SKILL_FILE).exists());
    assert!(skill_dir.join(CLAUDE_REFERENCE_FILE).exists());
    assert!(skill_dir.join(CLAUDE_METADATA_FILE).exists());

    // Verify file contents
    let saved_skill_md = fs::read_to_string(skill_dir.join(CLAUDE_SKILL_FILE)).unwrap();
    assert_eq!(saved_skill_md, skill_md);

    let saved_reference_md = fs::read_to_string(skill_dir.join(CLAUDE_REFERENCE_FILE)).unwrap();
    assert_eq!(saved_reference_md, reference_md);

    // Verify metadata
    let metadata_content = fs::read_to_string(skill_dir.join(CLAUDE_METADATA_FILE)).unwrap();
    let metadata: ClaudeSkillMetadata = serde_json::from_str(&metadata_content).unwrap();
    assert_eq!(metadata.skill_name, "test-skill");
    assert_eq!(metadata.server_name, "test-skill-server");
    assert_eq!(metadata.tool_count, 2);
    assert!(!metadata.checksums.skill_md.is_empty());
    assert!(metadata.checksums.reference_md.is_some());
}

#[test]
fn test_save_claude_skill_already_exists() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("duplicate").unwrap();
    let skill_data = create_test_skill_data("duplicate", 1);
    let skill_md = "# Skill";
    let reference_md = "# Reference";

    // First save succeeds
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Second save fails
    let result = store.save_claude_skill(&skill_name, skill_md, reference_md, &skill_data);
    assert!(matches!(
        result,
        Err(SkillStoreError::SkillAlreadyExists { .. })
    ));
}

#[test]
fn test_load_claude_skill_success() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("loadable").unwrap();
    let skill_data = create_test_skill_data("loadable", 3);
    let skill_md = "# Loadable Skill\n\nContent here.";
    let reference_md = "# Loadable Reference\n\nReference here.";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Load skill
    let loaded = store.load_claude_skill(&skill_name).unwrap();

    // Verify loaded data
    assert_eq!(loaded.name, "loadable");
    assert_eq!(loaded.skill_md, skill_md);
    assert_eq!(loaded.reference_md, Some(reference_md.to_string()));
    assert_eq!(loaded.metadata.skill_name, "loadable");
    assert_eq!(loaded.metadata.server_name, "loadable-server");
    assert_eq!(loaded.metadata.tool_count, 3);
}

#[test]
fn test_load_claude_skill_not_found() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("nonexistent").unwrap();
    let result = store.load_claude_skill(&skill_name);

    assert!(matches!(result, Err(SkillStoreError::SkillNotFound { .. })));
}

#[test]
fn test_load_claude_skill_missing_skill_md() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("incomplete").unwrap();
    let skill_data = create_test_skill_data("incomplete", 1);
    let skill_md = "# Skill";
    let reference_md = "# Reference";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Delete SKILL.md
    let skill_dir = temp.path().join("incomplete");
    fs::remove_file(skill_dir.join(CLAUDE_SKILL_FILE)).unwrap();

    // Load should fail
    let result = store.load_claude_skill(&skill_name);
    assert!(matches!(result, Err(SkillStoreError::MissingFile { .. })));
}

#[test]
fn test_load_claude_skill_checksum_mismatch() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("corrupted").unwrap();
    let skill_data = create_test_skill_data("corrupted", 1);
    let skill_md = "# Original Skill";
    let reference_md = "# Reference";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Corrupt SKILL.md
    let skill_dir = temp.path().join("corrupted");
    fs::write(skill_dir.join(CLAUDE_SKILL_FILE), "# Corrupted Content").unwrap();

    // Load should fail with checksum mismatch
    let result = store.load_claude_skill(&skill_name);
    assert!(matches!(
        result,
        Err(SkillStoreError::ChecksumMismatch { .. })
    ));
}

#[test]
fn test_list_claude_skills_empty() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skills = store.list_claude_skills().unwrap();
    assert_eq!(skills.len(), 0);
}

#[test]
fn test_list_claude_skills_multiple() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save multiple skills
    for i in 1..=3 {
        let name = format!("skill-{i}");
        let skill_name = SkillName::new(&name).unwrap();
        let skill_data = create_test_skill_data(&name, i);
        let skill_md = format!("# Skill {i}");
        let reference_md = format!("# Reference {i}");

        store
            .save_claude_skill(&skill_name, &skill_md, &reference_md, &skill_data)
            .unwrap();
    }

    // List skills
    let skills = store.list_claude_skills().unwrap();
    assert_eq!(skills.len(), 3);

    // Verify skill names (order may vary)
    let names: Vec<String> = skills.iter().map(|s| s.skill_name.clone()).collect();
    assert!(names.contains(&"skill-1".to_string()));
    assert!(names.contains(&"skill-2".to_string()));
    assert!(names.contains(&"skill-3".to_string()));
}

#[test]
fn test_remove_claude_skill_success() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("removable").unwrap();
    let skill_data = create_test_skill_data("removable", 1);
    let skill_md = "# Skill";
    let reference_md = "# Reference";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Verify exists
    let skill_dir = temp.path().join("removable");
    assert!(skill_dir.exists());

    // Remove skill
    store.remove_claude_skill(&skill_name).unwrap();

    // Verify deleted
    assert!(!skill_dir.exists());

    // Load should fail
    let result = store.load_claude_skill(&skill_name);
    assert!(matches!(result, Err(SkillStoreError::SkillNotFound { .. })));
}

#[test]
fn test_remove_claude_skill_not_found() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("nonexistent").unwrap();
    let result = store.remove_claude_skill(&skill_name);

    assert!(matches!(result, Err(SkillStoreError::SkillNotFound { .. })));
}

#[test]
fn test_invalid_skill_name() {
    let temp = TempDir::new().unwrap();
    let _store = SkillStore::new(temp.path()).unwrap();

    // These should fail at SkillName::new()
    assert!(SkillName::new("").is_err());
    assert!(SkillName::new("../escape").is_err());
    assert!(SkillName::new("path/traversal").is_err());
}

#[test]
fn test_save_load_roundtrip() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("roundtrip").unwrap();
    let skill_data = create_test_skill_data("roundtrip", 5);
    let skill_md = "# Roundtrip Skill\n\nMultiline\ncontent\nhere.";
    let reference_md = "# Roundtrip Reference\n\nDetailed\nAPI\ndocumentation.";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Load skill
    let loaded = store.load_claude_skill(&skill_name).unwrap();

    // Verify exact match
    assert_eq!(loaded.name, "roundtrip");
    assert_eq!(loaded.skill_md, skill_md);
    assert_eq!(loaded.reference_md, Some(reference_md.to_string()));
    assert_eq!(loaded.metadata.skill_name, "roundtrip");
    assert_eq!(loaded.metadata.server_name, "roundtrip-server");
    assert_eq!(loaded.metadata.tool_count, 5);
}

#[test]
fn test_concurrent_save_same_skill() {
    use std::sync::Arc;
    use std::thread;

    let temp = TempDir::new().unwrap();
    let store = Arc::new(SkillStore::new(temp.path()).unwrap());

    let skill_name = SkillName::new("concurrent-test").unwrap();
    let skill_data = create_test_skill_data("concurrent-test", 1);
    let skill_md = "# Concurrent Skill";
    let reference_md = "# Concurrent Reference";

    // Spawn two threads trying to save the same skill
    let store1 = Arc::clone(&store);
    let skill_name1 = skill_name.clone();
    let skill_data1 = skill_data.clone();
    let t1 = thread::spawn(move || {
        store1.save_claude_skill(&skill_name1, skill_md, reference_md, &skill_data1)
    });

    let store2 = Arc::clone(&store);
    let skill_name2 = skill_name.clone();
    let t2 = thread::spawn(move || {
        store2.save_claude_skill(&skill_name2, skill_md, reference_md, &skill_data)
    });

    let r1 = t1.join().unwrap();
    let r2 = t2.join().unwrap();

    // Exactly one should succeed, one should get AlreadyExists
    let success_count = [&r1, &r2].iter().filter(|r| r.is_ok()).count();
    let already_exists_count = [&r1, &r2]
        .iter()
        .filter(|r| matches!(r, Err(SkillStoreError::SkillAlreadyExists { .. })))
        .count();

    assert_eq!(success_count, 1, "Exactly one save should succeed");
    assert_eq!(
        already_exists_count, 1,
        "Exactly one save should fail with AlreadyExists"
    );

    // Skill should exist and be valid
    let loaded = store.load_claude_skill(&skill_name).unwrap();
    assert_eq!(loaded.name, "concurrent-test");
}

#[test]
fn test_atomic_write_failure_cleanup() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("atomic-test").unwrap();
    let skill_data = create_test_skill_data("atomic-test", 1);
    let skill_md = "# Atomic Skill";
    let reference_md = "# Atomic Reference";

    // First create the skill directory manually
    let skill_dir = temp.path().join("atomic-test");
    fs::create_dir(&skill_dir).unwrap();

    // Now save should fail with AlreadyExists
    let result = store.save_claude_skill(&skill_name, skill_md, reference_md, &skill_data);
    assert!(matches!(
        result,
        Err(SkillStoreError::SkillAlreadyExists { .. })
    ));

    // Directory should still exist since we created it manually
    assert!(skill_dir.exists());
}

#[test]
fn test_missing_metadata_file() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("no-metadata").unwrap();
    let skill_data = create_test_skill_data("no-metadata", 1);
    let skill_md = "# Skill";
    let reference_md = "# Reference";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Delete metadata file
    let skill_dir = temp.path().join("no-metadata");
    fs::remove_file(skill_dir.join(CLAUDE_METADATA_FILE)).unwrap();

    // Load should fail
    let result = store.load_claude_skill(&skill_name);
    assert!(matches!(result, Err(SkillStoreError::MissingFile { .. })));
}

#[test]
fn test_unicode_content() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("unicode-test").unwrap();
    let skill_data = create_test_skill_data("unicode-test", 1);
    let skill_md = "# Unicode Skill\n\n日本語コンテンツ\nДобро пожаловать\n🚀 Emoji support!";
    let reference_md = "# Unicode Reference\n\n中文参考文档\nΔοκιμή";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Load skill
    let loaded = store.load_claude_skill(&skill_name).unwrap();

    // Verify Unicode preserved
    assert_eq!(loaded.skill_md, skill_md);
    assert_eq!(loaded.reference_md, Some(reference_md.to_string()));
}

#[test]
fn test_empty_reference_md() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("empty-ref").unwrap();
    let skill_data = create_test_skill_data("empty-ref", 1);
    let skill_md = "# Skill with empty reference";
    let reference_md = "";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Load skill
    let loaded = store.load_claude_skill(&skill_name).unwrap();

    // Empty reference should still be saved
    assert_eq!(loaded.reference_md, Some(String::new()));
}

#[test]
fn test_metadata_timestamps() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("timestamp-test").unwrap();
    let skill_data = create_test_skill_data("timestamp-test", 1);
    let skill_md = "# Skill";
    let reference_md = "# Reference";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Load and verify timestamp is present and valid
    let loaded = store.load_claude_skill(&skill_name).unwrap();
    assert!(loaded.metadata.generated_at < chrono::Utc::now());

    // Timestamp should be recent (within last minute)
    let age = chrono::Utc::now() - loaded.metadata.generated_at;
    assert!(age.num_seconds() < 60);
}

#[test]
fn test_checksum_format() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let skill_name = SkillName::new("checksum-test").unwrap();
    let skill_data = create_test_skill_data("checksum-test", 1);
    let skill_md = "# Skill";
    let reference_md = "# Reference";

    // Save skill
    store
        .save_claude_skill(&skill_name, skill_md, reference_md, &skill_data)
        .unwrap();

    // Load and verify checksum format
    let loaded = store.load_claude_skill(&skill_name).unwrap();
    assert!(loaded.metadata.checksums.skill_md.starts_with("blake3:"));
    assert!(
        loaded
            .metadata
            .checksums
            .reference_md
            .as_ref()
            .unwrap()
            .starts_with("blake3:")
    );

    // Checksums should be 64 hex characters after "blake3:"
    let skill_checksum = &loaded.metadata.checksums.skill_md["blake3:".len()..];
    assert_eq!(skill_checksum.len(), 64);
    assert!(skill_checksum.chars().all(|c| c.is_ascii_hexdigit()));
}
