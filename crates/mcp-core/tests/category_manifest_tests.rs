//! Comprehensive tests for CategoryManifest type.
//!
//! Tests cover manifest building, tool-to-category mapping, and serialization.

use mcp_core::{CategoryManifest, ManifestMetadata, SkillCategory};

// ============================================================================
// Builder Tests
// ============================================================================

#[test]
fn test_manifest_builder_empty() {
    let manifest = CategoryManifest::builder().build();

    assert_eq!(manifest.category_count(), 0);
    assert_eq!(manifest.tool_count(), 0);
    assert_eq!(manifest.metadata().version, "1.0");
}

#[test]
fn test_manifest_builder_single_tool() {
    let category = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &category)
        .unwrap()
        .build();

    assert_eq!(manifest.category_count(), 1);
    assert_eq!(manifest.tool_count(), 1);
}

#[test]
fn test_manifest_builder_multiple_tools_single_category() {
    let category = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &category)
        .unwrap()
        .add_tool("list_commits", &category)
        .unwrap()
        .add_tool("delete_branch", &category)
        .unwrap()
        .build();

    assert_eq!(manifest.category_count(), 1);
    assert_eq!(manifest.tool_count(), 3);
}

#[test]
fn test_manifest_builder_multiple_categories() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();
    let prs = SkillCategory::new("prs").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .add_tool("list_commits", &repos)
        .unwrap()
        .add_tool("create_issue", &issues)
        .unwrap()
        .add_tool("list_issues", &issues)
        .unwrap()
        .add_tool("create_pr", &prs)
        .unwrap()
        .build();

    assert_eq!(manifest.category_count(), 3);
    assert_eq!(manifest.tool_count(), 5);
}

#[test]
fn test_manifest_builder_add_tools_batch() {
    let repos = SkillCategory::new("repos").unwrap();
    let tools = vec!["create_branch", "list_commits", "delete_branch"];

    let manifest = CategoryManifest::builder()
        .add_tools(tools, &repos)
        .unwrap()
        .build();

    assert_eq!(manifest.category_count(), 1);
    assert_eq!(manifest.tool_count(), 3);
}

#[test]
fn test_manifest_builder_chaining() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .add_tools(vec!["list_commits", "delete_branch"], &repos)
        .unwrap()
        .add_tool("create_issue", &issues)
        .unwrap()
        .build();

    assert_eq!(manifest.category_count(), 2);
    assert_eq!(manifest.tool_count(), 4);
}

// ============================================================================
// Tool Reassignment Tests
// ============================================================================

#[test]
fn test_manifest_tool_moved_to_new_category() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("my_tool", &repos)
        .unwrap()
        .add_tool("my_tool", &issues)
        .unwrap() // Tool moved to issues
        .build();

    // Both categories exist in map, but repos has no tools
    assert_eq!(manifest.tool_count(), 1);

    let found = manifest.find_category("my_tool");
    assert_eq!(found, Some(&issues));
}

#[test]
fn test_manifest_tool_only_in_one_category() {
    let cat1 = SkillCategory::new("cat1").unwrap();
    let cat2 = SkillCategory::new("cat2").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("shared_tool", &cat1)
        .unwrap()
        .add_tool("other_tool", &cat2)
        .unwrap()
        .add_tool("shared_tool", &cat2)
        .unwrap() // Move to cat2
        .build();

    assert_eq!(manifest.tool_count(), 2);

    let found = manifest.find_category("shared_tool");
    assert_eq!(found, Some(&cat2));
}

// ============================================================================
// Category Finding Tests
// ============================================================================

#[test]
fn test_manifest_find_category_exists() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let found = manifest.find_category("create_branch");
    assert!(found.is_some());
    assert_eq!(found.unwrap(), &repos);
}

#[test]
fn test_manifest_find_category_not_exists() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let found = manifest.find_category("nonexistent_tool");
    assert!(found.is_none());
}

#[test]
fn test_manifest_find_category_multiple_tools() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .add_tool("list_commits", &repos)
        .unwrap()
        .add_tool("create_issue", &issues)
        .unwrap()
        .build();

    assert_eq!(manifest.find_category("create_branch"), Some(&repos));
    assert_eq!(manifest.find_category("list_commits"), Some(&repos));
    assert_eq!(manifest.find_category("create_issue"), Some(&issues));
    assert_eq!(manifest.find_category("unknown"), None);
}

// ============================================================================
// Metadata Tests
// ============================================================================

#[test]
fn test_manifest_metadata_creation() {
    let metadata = ManifestMetadata::new(5, 40);

    assert_eq!(metadata.version, "1.0");
    assert_eq!(metadata.category_count, 5);
    assert_eq!(metadata.tool_count, 40);
    assert!(!metadata.generated_at.is_empty());
}

#[test]
fn test_manifest_metadata_timestamp_format() {
    let metadata = ManifestMetadata::new(1, 1);

    // Should be RFC3339 format
    assert!(metadata.generated_at.contains('T'));
    assert!(metadata.generated_at.contains('Z') || metadata.generated_at.contains('+'));
}

#[test]
fn test_manifest_metadata_accessor() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let metadata = manifest.metadata();
    assert_eq!(metadata.version, "1.0");
    assert_eq!(metadata.category_count, 1);
    assert_eq!(metadata.tool_count, 1);
}

#[test]
fn test_manifest_metadata_correct_counts() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tools(vec!["tool1", "tool2", "tool3"], &repos)
        .unwrap()
        .add_tools(vec!["tool4", "tool5"], &issues)
        .unwrap()
        .build();

    let metadata = manifest.metadata();
    assert_eq!(metadata.category_count, 2);
    assert_eq!(metadata.tool_count, 5);
}

// ============================================================================
// Accessors Tests
// ============================================================================

#[test]
fn test_manifest_categories_accessor() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let categories = manifest.categories();
    assert_eq!(categories.len(), 1);
    assert!(categories.contains_key(&repos));
}

#[test]
fn test_manifest_category_count() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();
    let prs = SkillCategory::new("prs").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("tool1", &repos)
        .unwrap()
        .add_tool("tool2", &issues)
        .unwrap()
        .add_tool("tool3", &prs)
        .unwrap()
        .build();

    assert_eq!(manifest.category_count(), 3);
}

#[test]
fn test_manifest_tool_count() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tools(vec!["tool1", "tool2", "tool3", "tool4"], &repos)
        .unwrap()
        .build();

    assert_eq!(manifest.tool_count(), 4);
}

#[test]
fn test_manifest_tool_count_across_categories() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tools(vec!["tool1", "tool2"], &repos)
        .unwrap()
        .add_tools(vec!["tool3", "tool4", "tool5"], &issues)
        .unwrap()
        .build();

    assert_eq!(manifest.tool_count(), 5);
}

// ============================================================================
// Serialization Tests
// ============================================================================

#[test]
fn test_manifest_serialization_json() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let json = serde_json::to_string_pretty(&manifest).unwrap();

    assert!(json.contains("repos"));
    assert!(json.contains("create_branch"));
    assert!(json.contains("metadata"));
    assert!(json.contains("1.0"));
}

#[test]
fn test_manifest_deserialization_json() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let json = serde_json::to_string(&manifest).unwrap();
    let deserialized: CategoryManifest = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.category_count(), 1);
    assert_eq!(deserialized.tool_count(), 1);
    assert!(deserialized.find_category("create_branch").is_some());
}

#[test]
fn test_manifest_serialization_yaml() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .add_tool("list_commits", &repos)
        .unwrap()
        .add_tool("create_issue", &issues)
        .unwrap()
        .build();

    let yaml = serde_yaml::to_string(&manifest).unwrap();

    assert!(yaml.contains("repos"));
    assert!(yaml.contains("issues"));
    assert!(yaml.contains("create_branch"));
    assert!(yaml.contains("create_issue"));
}

#[test]
fn test_manifest_deserialization_yaml() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let yaml = serde_yaml::to_string(&manifest).unwrap();
    let deserialized: CategoryManifest = serde_yaml::from_str(&yaml).unwrap();

    assert_eq!(deserialized.category_count(), 1);
    assert_eq!(deserialized.tool_count(), 1);
}

// ============================================================================
// Debug Implementation Tests
// ============================================================================

#[test]
fn test_manifest_debug() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let debug_str = format!("{manifest:?}");
    assert!(debug_str.contains("CategoryManifest"));
}

// ============================================================================
// Send + Sync Tests
// ============================================================================

#[test]
fn test_manifest_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<CategoryManifest>();
    assert_send::<ManifestMetadata>();
}

#[test]
fn test_manifest_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<CategoryManifest>();
    assert_sync::<ManifestMetadata>();
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_manifest_empty_category_name_as_tool() {
    let category = SkillCategory::new("category").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("", &category)
        .unwrap()
        .build();

    assert_eq!(manifest.tool_count(), 1);

    let found = manifest.find_category("");
    assert_eq!(found, Some(&category));
}

#[test]
fn test_manifest_tool_name_with_spaces() {
    let category = SkillCategory::new("category").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("tool with spaces", &category)
        .unwrap()
        .build();

    assert_eq!(manifest.tool_count(), 1);

    let found = manifest.find_category("tool with spaces");
    assert_eq!(found, Some(&category));
}

#[test]
fn test_manifest_duplicate_tool_names_different_categories() {
    let cat1 = SkillCategory::new("cat1").unwrap();
    let cat2 = SkillCategory::new("cat2").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("duplicate", &cat1)
        .unwrap()
        .add_tool("other", &cat2)
        .unwrap()
        .add_tool("duplicate", &cat2)
        .unwrap() // Should move to cat2
        .build();

    assert_eq!(manifest.tool_count(), 2);
    assert_eq!(manifest.find_category("duplicate"), Some(&cat2));
}

#[test]
fn test_manifest_large_number_of_tools() {
    let category = SkillCategory::new("category").unwrap();

    let mut builder = CategoryManifest::builder();
    for i in 0..100 {
        builder = builder.add_tool(format!("tool_{i}"), &category).unwrap();
    }

    let manifest = builder.build();

    assert_eq!(manifest.category_count(), 1);
    assert_eq!(manifest.tool_count(), 100);
}

#[test]
fn test_manifest_large_number_of_categories() {
    let mut builder = CategoryManifest::builder();

    for i in 0..50 {
        let category = SkillCategory::new(format!("category_{i}")).unwrap();
        builder = builder.add_tool(format!("tool_{i}"), &category).unwrap();
    }

    let manifest = builder.build();

    assert_eq!(manifest.category_count(), 50);
    assert_eq!(manifest.tool_count(), 50);
}

#[test]
fn test_manifest_categories_returns_const_reference() {
    let repos = SkillCategory::new("repos").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let categories = manifest.categories();
    let categories2 = manifest.categories();

    // Should return same reference
    assert_eq!(categories, categories2);
}

#[test]
fn test_manifest_metadata_equality() {
    let meta1 = ManifestMetadata::new(5, 40);
    let meta2 = ManifestMetadata::new(5, 40);

    // Timestamps will differ, so only check fields
    assert_eq!(meta1.version, meta2.version);
    assert_eq!(meta1.category_count, meta2.category_count);
    assert_eq!(meta1.tool_count, meta2.tool_count);
}
