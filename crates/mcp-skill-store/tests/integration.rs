//! Integration tests for cache/skills separation
//!
//! These tests verify the complete workflow of skill storage with
//! separate cache and public skills directories.

use mcp_codegen::skills::claude::SkillData;
use mcp_core::SkillName;
use mcp_skill_store::SkillStore;
use std::fs;
use tempfile::TempDir;

/// Helper to create test `SkillData`
fn test_skill_data(skill_name: &str) -> SkillData {
    SkillData::new(
        skill_name.to_string(),
        "Test skill".to_string(),
        "test-server".to_string(),
        "1.0.0".to_string(),
        "Test server".to_string(),
        "1.0".to_string(),
        vec![], // tools
        vec![], // capabilities
    )
}

/// Tests end-to-end workflow with cache/skills separation
#[test]
fn test_cache_separation_e2e() {
    // 1. Create SkillStore with separate directories
    let temp_skills = TempDir::new().expect("failed to create temp skills dir");
    let temp_cache = TempDir::new().expect("failed to create temp cache dir");

    let store = SkillStore::with_directories(temp_skills.path(), temp_cache.path())
        .expect("failed to create store");

    // 2. Save Claude skill (SKILL.md → skills, cache → cache)
    let skill_name = SkillName::new("test-skill").expect("valid skill name");
    let skill_content = "# Test Skill\n\nThis is a test skill.";
    let reference_content = "# Reference\n\nTest reference.";
    let skill_data = test_skill_data("test-skill");

    store
        .save_claude_skill(&skill_name, skill_content, reference_content, &skill_data)
        .expect("failed to save skill");

    // 3. Verify files in correct locations
    let skill_path = temp_skills
        .path()
        .join(skill_name.as_str())
        .join("SKILL.md");
    let reference_path = temp_skills
        .path()
        .join(skill_name.as_str())
        .join("REFERENCE.md");
    let metadata_path = temp_skills
        .path()
        .join(skill_name.as_str())
        .join(".metadata.json");

    assert!(
        skill_path.exists(),
        "SKILL.md should exist in skills dir: {}",
        skill_path.display()
    );
    assert!(
        reference_path.exists(),
        "REFERENCE.md should exist in skills dir: {}",
        reference_path.display()
    );
    assert!(
        metadata_path.exists(),
        ".metadata.json should exist in skills dir: {}",
        metadata_path.display()
    );

    let saved_skill = fs::read_to_string(&skill_path).expect("failed to read SKILL.md");
    let saved_reference = fs::read_to_string(&reference_path).expect("failed to read REFERENCE.md");

    assert_eq!(saved_skill, skill_content);
    assert_eq!(saved_reference, reference_content);

    // 4. Load skill - verify content
    let loaded_skill = store
        .load_claude_skill(&skill_name)
        .expect("failed to load skill");
    assert_eq!(loaded_skill.skill_md, skill_content);
    assert_eq!(
        loaded_skill.reference_md.as_deref(),
        Some(reference_content)
    );

    // 5. Remove skill - verify public files removed
    store
        .remove_claude_skill(&skill_name)
        .expect("failed to remove skill");

    assert!(!skill_path.exists(), "SKILL.md should be removed");
    assert!(!reference_path.exists(), "REFERENCE.md should be removed");
    assert!(!metadata_path.exists(), ".metadata.json should be removed");
}

/// Tests independence of cache and skills directories
#[test]
fn test_cache_skills_independence() {
    // 1. Create separate directories
    let temp_skills = TempDir::new().expect("failed to create temp skills dir");
    let temp_cache = TempDir::new().expect("failed to create temp cache dir");

    let store = SkillStore::with_directories(temp_skills.path(), temp_cache.path())
        .expect("failed to create store");

    // 2. Save skill
    let skill_name = SkillName::new("independent-skill").expect("valid skill name");
    let skill_content = "# Independent Skill\n\nTest independence.";
    let reference_content = "# Reference\n\nTest reference.";
    let skill_data = test_skill_data("independent-skill");

    store
        .save_claude_skill(&skill_name, skill_content, reference_content, &skill_data)
        .expect("failed to save skill");

    let skill_path = temp_skills
        .path()
        .join(skill_name.as_str())
        .join("SKILL.md");
    let skill_dir = temp_skills.path().join(skill_name.as_str());

    assert!(skill_path.exists(), "SKILL.md should exist");
    assert!(skill_dir.exists(), "skill directory should exist");

    // 3. Clear all cache
    store
        .cache()
        .clear_all()
        .expect("failed to clear all cache");

    // 4. Verify SKILL.md still exists after cache clear
    assert!(
        skill_path.exists(),
        "SKILL.md should remain after cache clear"
    );

    // Verify skill can still be loaded
    let loaded_skill = store
        .load_claude_skill(&skill_name)
        .expect("failed to load skill");
    assert_eq!(loaded_skill.skill_md, skill_content);

    // Verify cache directory is empty (no WASM/VFS for skills without cache)
    let wasm_dir = temp_cache.path().join("wasm");
    let vfs_dir = temp_cache.path().join("vfs");

    // Cache directories might not exist or be empty after clear_all
    if wasm_dir.exists() {
        let wasm_files = fs::read_dir(&wasm_dir).expect("read wasm dir");
        assert_eq!(
            wasm_files.count(),
            0,
            "WASM cache should be empty after clear_all"
        );
    }
    if vfs_dir.exists() {
        let vfs_files = fs::read_dir(&vfs_dir).expect("read vfs dir");
        assert_eq!(
            vfs_files.count(),
            0,
            "VFS cache should be empty after clear_all"
        );
    }
}

/// Tests cross-platform path handling
#[test]
fn test_cross_platform_paths() {
    // Test with temp directories to ensure paths work on Windows/Unix
    let temp_skills = TempDir::new().expect("failed to create temp skills dir");
    let temp_cache = TempDir::new().expect("failed to create temp cache dir");

    let store = SkillStore::with_directories(temp_skills.path(), temp_cache.path())
        .expect("failed to create store");

    // Test with various skill names (alphanumeric, hyphens, underscores)
    let skill_names = vec![
        "simple",
        "with-hyphens",
        "with_underscores",
        "mixed-name_123",
    ];

    for skill_name_str in skill_names {
        let skill_name = SkillName::new(skill_name_str).expect("valid skill name");
        let skill_content = format!("# Skill: {skill_name_str}\n\nTest content.");
        let reference_content = "# Reference\n\nTest reference.";
        let skill_data = test_skill_data(skill_name_str);

        store
            .save_claude_skill(&skill_name, &skill_content, reference_content, &skill_data)
            .expect("failed to save skill");

        let skill_path = temp_skills
            .path()
            .join(skill_name.as_str())
            .join("SKILL.md");
        assert!(
            skill_path.exists(),
            "SKILL.md should exist for {skill_name_str}"
        );

        // Verify path separators are handled correctly
        let loaded = store
            .load_claude_skill(&skill_name)
            .expect("failed to load skill");
        assert_eq!(loaded.skill_md, skill_content);

        store
            .remove_claude_skill(&skill_name)
            .expect("failed to remove skill");
        assert!(
            !skill_path.exists(),
            "SKILL.md should be removed for {skill_name_str}"
        );
    }
}

/// Tests that cache directory is independent from skills directory
#[test]
fn test_cache_directory_separation() {
    let temp_skills = TempDir::new().expect("failed to create temp skills dir");
    let temp_cache = TempDir::new().expect("failed to create temp cache dir");

    let store = SkillStore::with_directories(temp_skills.path(), temp_cache.path())
        .expect("failed to create store");

    // Create multiple skills
    let skills = vec![
        ("skill1", "# Skill 1"),
        ("skill2", "# Skill 2"),
        ("skill3", "# Skill 3"),
    ];

    for (name_str, content) in &skills {
        let skill_name = SkillName::new(*name_str).expect("valid skill name");
        let skill_data = test_skill_data(name_str);
        store
            .save_claude_skill(&skill_name, content, "# Reference", &skill_data)
            .expect("failed to save skill");
    }

    // Verify all skills exist in skills directory
    for (name_str, _) in &skills {
        let skill_path = temp_skills.path().join(name_str).join("SKILL.md");
        assert!(skill_path.exists(), "{name_str} SKILL.md should exist");
    }

    // Clear all cache
    store
        .cache()
        .clear_all()
        .expect("failed to clear all cache");

    // Verify all skills still exist in skills directory
    for (name_str, content) in &skills {
        let skill_name = SkillName::new(*name_str).expect("valid skill name");
        let loaded = store
            .load_claude_skill(&skill_name)
            .expect("failed to load skill");
        assert_eq!(loaded.skill_md, *content);
    }
}

/// Tests that listing skills doesn't depend on cache
#[test]
fn test_list_skills_independent_of_cache() {
    let temp_skills = TempDir::new().expect("failed to create temp skills dir");
    let temp_cache = TempDir::new().expect("failed to create temp cache dir");

    let store = SkillStore::with_directories(temp_skills.path(), temp_cache.path())
        .expect("failed to create store");

    // Save skills
    let visible1 = SkillName::new("visible1").expect("valid skill name");
    let visible2 = SkillName::new("visible2").expect("valid skill name");

    store
        .save_claude_skill(
            &visible1,
            "# Visible 1",
            "# Reference 1",
            &test_skill_data("visible1"),
        )
        .expect("failed to save skill");
    store
        .save_claude_skill(
            &visible2,
            "# Visible 2",
            "# Reference 2",
            &test_skill_data("visible2"),
        )
        .expect("failed to save skill");

    // List skills should show both
    let skills = store.list_claude_skills().expect("failed to list skills");
    assert_eq!(skills.len(), 2, "should list exactly 2 skills");

    let skill_names: Vec<String> = skills.iter().map(|s| s.skill_name.clone()).collect();
    assert!(skill_names.contains(&"visible1".to_string()));
    assert!(skill_names.contains(&"visible2".to_string()));

    // Clear cache
    store.cache().clear_all().expect("failed to clear cache");

    // List skills should still show both (unchanged)
    let skills_after_clear = store
        .list_claude_skills()
        .expect("failed to list skills after cache clear");
    assert_eq!(
        skills_after_clear.len(),
        2,
        "should still list exactly 2 skills after cache clear"
    );

    let skill_names_after: Vec<String> = skills_after_clear
        .iter()
        .map(|s| s.skill_name.clone())
        .collect();
    assert!(skill_names_after.contains(&"visible1".to_string()));
    assert!(skill_names_after.contains(&"visible2".to_string()));
}

/// Tests skill metadata persists separately from cache
#[test]
fn test_metadata_independence() {
    let temp_skills = TempDir::new().expect("failed to create temp skills dir");
    let temp_cache = TempDir::new().expect("failed to create temp cache dir");

    let store = SkillStore::with_directories(temp_skills.path(), temp_cache.path())
        .expect("failed to create store");

    let skill_name = SkillName::new("meta-test").expect("valid skill name");
    let skill_content = "# Metadata Test\n\nTest metadata persistence.";
    let reference_content = "# Reference\n\nTest reference.";
    let skill_data = test_skill_data("meta-test");

    // Save skill with metadata
    store
        .save_claude_skill(&skill_name, skill_content, reference_content, &skill_data)
        .expect("failed to save skill");

    // Verify metadata file exists in skills directory
    let metadata_path = temp_skills
        .path()
        .join(skill_name.as_str())
        .join(".metadata.json");
    assert!(
        metadata_path.exists(),
        "metadata should exist in skills dir"
    );

    // Read and verify metadata
    let metadata_json = fs::read_to_string(&metadata_path).expect("failed to read metadata");
    assert!(
        metadata_json.contains("test-server"),
        "metadata should contain server name"
    );
    assert!(
        metadata_json.contains("1.0.0"),
        "metadata should contain server version"
    );

    // Clear all cache
    store.cache().clear_all().expect("failed to clear cache");

    // Verify metadata still exists
    assert!(
        metadata_path.exists(),
        "metadata should remain after cache clear"
    );

    // Verify skill can still be loaded with metadata
    let loaded = store
        .load_claude_skill(&skill_name)
        .expect("failed to load skill");
    assert_eq!(loaded.skill_md, skill_content);
    assert_eq!(loaded.metadata.server_name, "test-server");
    assert_eq!(loaded.metadata.server_version, "1.0.0");
}
