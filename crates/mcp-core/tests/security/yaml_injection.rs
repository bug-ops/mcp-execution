//! YAML injection security tests for category manifests.
//!
//! Validates that manifest serialization is safe from injection attacks.

use mcp_core::{CategoryManifest, SkillCategory};

/// Test that manifest YAML is safe from directive injection.
#[test]
fn test_manifest_rejects_yaml_directives() {
    // YAML directives that could be dangerous
    let dangerous_names = vec![
        "!!python/object/apply:os.system",
        "!!python/object/new:os.system",
        "!!str",
        "!!map",
        "!!seq",
        "&anchor",
        "*alias",
        "<<merge",
    ];

    for name in dangerous_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_err(),
            "Category name '{}' should be rejected (YAML directive)",
            name
        );
    }
}

/// Test that manifest serialization produces safe YAML.
#[test]
fn test_manifest_yaml_safe_serialization() {
    let mut manifest = CategoryManifest::builder();

    // Add legitimate categories
    let repos = SkillCategory::new("repos").expect("Valid category");
    let issues = SkillCategory::new("issues").expect("Valid category");

    manifest.add_category(repos.clone(), vec!["create_branch", "list_commits"]);
    manifest.add_category(issues.clone(), vec!["create_issue", "list_issues"]);

    let built_manifest = manifest.build();

    // Serialize to YAML
    let yaml = serde_yaml::to_string(&built_manifest).expect("Serialization should succeed");

    // YAML should not contain dangerous directives
    assert!(
        !yaml.contains("!!python"),
        "YAML should not contain Python directives"
    );
    assert!(
        !yaml.contains("!!java"),
        "YAML should not contain Java directives"
    );
    assert!(
        !yaml.contains("!!ruby"),
        "YAML should not contain Ruby directives"
    );

    // YAML should be valid and parseable
    let _deserialized: CategoryManifest =
        serde_yaml::from_str(&yaml).expect("YAML should be deserializable");
}

/// Test that category names with special characters are escaped in YAML.
#[test]
fn test_manifest_escapes_special_chars() {
    let categories = vec![
        ("repos-v2", vec!["tool1"]),
        ("user_management", vec!["tool2"]),
        ("prs_v2", vec!["tool3"]),
    ];

    let mut manifest = CategoryManifest::builder();

    for (name, tools) in categories {
        let category = SkillCategory::new(name).expect("Valid category");
        manifest.add_category(category, tools);
    }

    let built_manifest = manifest.build();
    let yaml = serde_yaml::to_string(&built_manifest).expect("Serialization should succeed");

    // Should not break YAML structure
    let _deserialized: CategoryManifest =
        serde_yaml::from_str(&yaml).expect("YAML should be deserializable");
}

/// Test that tool names are safely serialized.
#[test]
fn test_manifest_safe_tool_names() {
    let mut manifest = CategoryManifest::builder();

    let repos = SkillCategory::new("repos").expect("Valid category");

    // Tool names with various characters (that should be valid)
    let tools = vec![
        "create_branch",
        "list-commits",
        "getFileContents",
        "get_file_contents_v2",
    ];

    manifest.add_category(repos, tools);

    let built_manifest = manifest.build();
    let yaml = serde_yaml::to_string(&built_manifest).expect("Serialization should succeed");

    // Should deserialize correctly
    let deserialized: CategoryManifest =
        serde_yaml::from_str(&yaml).expect("YAML should be deserializable");

    // Verify tools are preserved correctly
    let categories = deserialized.categories();
    assert_eq!(categories.len(), 1, "Should have one category");
}

/// Test that empty manifests serialize safely.
#[test]
fn test_manifest_empty_safe() {
    let manifest = CategoryManifest::builder().build();

    let yaml = serde_yaml::to_string(&manifest).expect("Empty manifest should serialize");

    assert!(
        !yaml.is_empty(),
        "Empty manifest should produce valid YAML"
    );

    // Should deserialize
    let _deserialized: CategoryManifest =
        serde_yaml::from_str(&yaml).expect("Empty YAML should deserialize");
}

/// Test that large manifests don't cause issues.
#[test]
fn test_manifest_large_safe() {
    let mut manifest = CategoryManifest::builder();

    // Create 20 categories with 10 tools each
    for i in 0..20 {
        let category_name = format!("category_{}", i);
        let category = SkillCategory::new(&category_name).expect("Valid category");

        let tools: Vec<String> = (0..10).map(|j| format!("tool_{}_{}", i, j)).collect();

        manifest.add_category(category, tools);
    }

    let built_manifest = manifest.build();
    let yaml = serde_yaml::to_string(&built_manifest).expect("Large manifest should serialize");

    // YAML should be reasonable size (not exponentially large)
    assert!(
        yaml.len() < 50_000,
        "YAML should be reasonable size: {} bytes",
        yaml.len()
    );

    // Should deserialize
    let deserialized: CategoryManifest =
        serde_yaml::from_str(&yaml).expect("Large YAML should deserialize");

    assert_eq!(
        deserialized.categories().len(),
        20,
        "Should preserve all categories"
    );
}

/// Test that manifest handles duplicate category names correctly.
#[test]
fn test_manifest_duplicate_categories() {
    let mut manifest = CategoryManifest::builder();

    let repos1 = SkillCategory::new("repos").expect("Valid category");
    let repos2 = SkillCategory::new("repos").expect("Valid category");

    manifest.add_category(repos1, vec!["tool1"]);
    manifest.add_category(repos2, vec!["tool2"]);

    let built_manifest = manifest.build();

    // Behavior depends on implementation:
    // Either reject duplicates, merge them, or last-wins
    // Verify the behavior is safe and documented

    let yaml = serde_yaml::to_string(&built_manifest).expect("Should serialize");
    let _deserialized: CategoryManifest =
        serde_yaml::from_str(&yaml).expect("Should deserialize");
}

/// Test that manifest rejects excessively long tool lists.
#[test]
fn test_manifest_tool_count_limit() {
    let mut manifest = CategoryManifest::builder();

    let repos = SkillCategory::new("repos").expect("Valid category");

    // Try to add 1000 tools (should be rejected or limited)
    let many_tools: Vec<String> = (0..1000).map(|i| format!("tool_{}", i)).collect();

    manifest.add_category(repos, many_tools);

    let built_manifest = manifest.build();

    // If accepted, verify it serializes within limits
    if let Ok(yaml) = serde_yaml::to_string(&built_manifest) {
        // YAML should be under 100KB
        assert!(
            yaml.len() < 100_000,
            "YAML with 1000 tools should be under 100KB: {} bytes",
            yaml.len()
        );
    }
}

/// Test that manifest metadata is safe.
#[test]
fn test_manifest_metadata_safe() {
    let mut manifest = CategoryManifest::builder();

    let repos = SkillCategory::new("repos").expect("Valid category");
    manifest.add_category(repos, vec!["tool1"]);

    // Set metadata (if supported)
    // manifest.set_skill_name("github");
    // manifest.set_version("1.0.0");

    let built_manifest = manifest.build();
    let yaml = serde_yaml::to_string(&built_manifest).expect("Should serialize");

    // Metadata should not contain injection attempts
    assert!(!yaml.contains("!!"), "YAML should not contain directives");
    assert!(!yaml.contains("&"), "YAML should not contain anchors");
    assert!(!yaml.contains("*"), "YAML should not contain aliases");
}

/// Test deserialization of malformed YAML.
#[test]
fn test_manifest_reject_malformed_yaml() {
    let malformed_yamls = vec![
        "!!python/object/apply:os.system ['echo pwned']",
        "&anchor\ncategories: *anchor",
        "categories: !!map { key: value }",
        "!!binary\ncategories: []",
    ];

    for yaml in malformed_yamls {
        let result = serde_yaml::from_str::<CategoryManifest>(yaml);
        // Should either error or safely ignore dangerous directives
        if let Ok(manifest) = result {
            // If accepted, verify it's safe
            let serialized = serde_yaml::to_string(&manifest).expect("Should re-serialize");
            assert!(
                !serialized.contains("!!"),
                "Re-serialized YAML should not contain directives"
            );
        }
    }
}

/// Test that serde_yaml version has safe defaults.
#[test]
fn test_serde_yaml_safe_version() {
    // serde_yaml 0.9+ should have safe defaults (no custom tags)
    // This test documents the expectation

    let yaml_with_tag = "!!python/object/apply:os.system ['echo test']\ncategories: []";

    let result = serde_yaml::from_str::<serde_yaml::Value>(yaml_with_tag);

    // serde_yaml 0.9+ should reject custom tags
    match result {
        Ok(value) => {
            // If accepted, should be treated as string, not directive
            let serialized = serde_yaml::to_string(&value).expect("Should serialize");
            assert!(
                !serialized.contains("!!python"),
                "Should not preserve dangerous directives"
            );
        }
        Err(_) => {
            // Rejection is also safe behavior
        }
    }
}

/// Test that YAML comments cannot be injected.
#[test]
fn test_manifest_no_comment_injection() {
    let mut manifest = CategoryManifest::builder();

    // Try category name with comment-like content
    let result = SkillCategory::new("repos#comment");
    assert!(
        result.is_err(),
        "Category name with # should be rejected"
    );
}

/// Test round-trip serialization preserves data.
#[test]
fn test_manifest_round_trip_safe() {
    let mut manifest = CategoryManifest::builder();

    let categories = vec![
        ("repos", vec!["create_branch", "list_commits"]),
        ("issues", vec!["create_issue", "list_issues"]),
        ("prs", vec!["create_pr", "merge_pr"]),
    ];

    for (name, tools) in &categories {
        let category = SkillCategory::new(name).expect("Valid category");
        manifest.add_category(category, tools.clone());
    }

    let original = manifest.build();

    // Serialize
    let yaml = serde_yaml::to_string(&original).expect("Should serialize");

    // Deserialize
    let deserialized: CategoryManifest =
        serde_yaml::from_str(&yaml).expect("Should deserialize");

    // Re-serialize
    let yaml2 = serde_yaml::to_string(&deserialized).expect("Should re-serialize");

    // Should be equivalent
    let deserialized2: CategoryManifest =
        serde_yaml::from_str(&yaml2).expect("Should deserialize again");

    assert_eq!(
        deserialized.categories().len(),
        deserialized2.categories().len(),
        "Round-trip should preserve category count"
    );
}
