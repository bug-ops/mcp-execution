//! Comprehensive tests for CategorizedSkillBundle type.
//!
//! Tests cover bundle building, category management, and validation.

use mcp_core::{CategorizedSkillBundle, CategoryManifest, ScriptFile, SkillCategory};

// ============================================================================
// Builder Tests - Basic
// ============================================================================

#[test]
fn test_categorized_bundle_builder_minimal() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---\nname: github\n---\n# GitHub")
        .manifest(manifest)
        .build();

    assert_eq!(bundle.name().as_str(), "github");
    assert!(!bundle.skill_md().is_empty());
    assert_eq!(bundle.manifest().tool_count(), 1);
}

#[test]
fn test_categorized_bundle_builder_with_categories() {
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();

    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .add_tool("create_issue", &issues)
        .unwrap()
        .build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("# GitHub Skill")
        .manifest(manifest)
        .add_category(repos.clone(), "# Repository Operations\n...")
        .add_category(issues.clone(), "# Issue Operations\n...")
        .build();

    assert_eq!(bundle.categories().len(), 2);
    assert!(bundle.categories().contains_key(&repos));
    assert!(bundle.categories().contains_key(&issues));
}

#[test]
fn test_categorized_bundle_builder_with_scripts() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("# GitHub Skill")
        .manifest(manifest)
        .script(ScriptFile::new("create_branch", "ts", "// TypeScript code"))
        .script(ScriptFile::new("list_commits", "ts", "// More code"))
        .build();

    assert_eq!(bundle.scripts().len(), 2);
}

#[test]
fn test_categorized_bundle_builder_with_reference() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("# GitHub Skill")
        .manifest(manifest)
        .reference_md("# Full API Reference\n...")
        .build();

    assert!(bundle.reference_md().is_some());
    assert!(bundle.reference_md().unwrap().contains("Reference"));
}

#[test]
fn test_categorized_bundle_builder_chaining() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---\nname: github\n---")
        .manifest(manifest)
        .add_category(repos.clone(), "# Repos\n...")
        .script(ScriptFile::new("tool1", "ts", "code1"))
        .script(ScriptFile::new("tool2", "ts", "code2"))
        .reference_md("# Reference")
        .build();

    assert_eq!(bundle.name().as_str(), "github");
    assert_eq!(bundle.categories().len(), 1);
    assert_eq!(bundle.scripts().len(), 2);
    assert!(bundle.reference_md().is_some());
}

// ============================================================================
// Builder Tests - Error Cases
// ============================================================================

#[test]
#[should_panic(expected = "skill_md is required")]
fn test_categorized_bundle_builder_missing_skill_md() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder().build();

    let _bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .manifest(manifest)
        .add_category(repos, "content")
        .build();
}

#[test]
#[should_panic(expected = "manifest is required")]
fn test_categorized_bundle_builder_missing_manifest() {
    let _bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .build();
}

#[test]
fn test_categorized_bundle_builder_invalid_name() {
    let result = CategorizedSkillBundle::builder("INVALID");
    assert!(result.is_err());
}

#[test]
fn test_categorized_bundle_try_build_missing_skill_md() {
    let manifest = CategoryManifest::builder().build();

    let result = CategorizedSkillBundle::builder("github")
        .unwrap()
        .manifest(manifest)
        .try_build();

    assert!(result.is_err());
}

#[test]
fn test_categorized_bundle_try_build_missing_manifest() {
    let result = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .try_build();

    assert!(result.is_err());
}

#[test]
fn test_categorized_bundle_try_build_success() {
    let manifest = CategoryManifest::builder().build();

    let result = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .try_build();

    assert!(result.is_ok());
}

// ============================================================================
// Accessor Tests
// ============================================================================

#[test]
fn test_categorized_bundle_name() {
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .build();

    assert_eq!(bundle.name().as_str(), "github");
}

#[test]
fn test_categorized_bundle_skill_md() {
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("# GitHub Skill Content")
        .manifest(manifest)
        .build();

    assert_eq!(bundle.skill_md(), "# GitHub Skill Content");
}

#[test]
fn test_categorized_bundle_manifest() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest.clone())
        .build();

    assert_eq!(bundle.manifest().tool_count(), 1);
}

#[test]
fn test_categorized_bundle_categories() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .add_category(repos.clone(), "# Repos\n...")
        .build();

    let categories = bundle.categories();
    assert_eq!(categories.len(), 1);
    assert!(categories.contains_key(&repos));
    assert_eq!(categories.get(&repos).unwrap(), "# Repos\n...");
}

#[test]
fn test_categorized_bundle_scripts() {
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .script(ScriptFile::new("tool1", "ts", "code1"))
        .script(ScriptFile::new("tool2", "ts", "code2"))
        .build();

    let scripts = bundle.scripts();
    assert_eq!(scripts.len(), 2);
    assert_eq!(scripts[0].reference().tool_name(), "tool1");
    assert_eq!(scripts[1].reference().tool_name(), "tool2");
}

#[test]
fn test_categorized_bundle_reference_md_some() {
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .reference_md("# Reference")
        .build();

    assert!(bundle.reference_md().is_some());
    assert_eq!(bundle.reference_md().unwrap(), "# Reference");
}

#[test]
fn test_categorized_bundle_reference_md_none() {
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .build();

    assert!(bundle.reference_md().is_none());
}

// ============================================================================
// Builder Methods Tests
// ============================================================================

#[test]
fn test_categorized_bundle_add_category() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .add_category(repos.clone(), "# Content 1")
        .build();

    assert_eq!(bundle.categories().len(), 1);
    assert_eq!(bundle.categories().get(&repos).unwrap(), "# Content 1");
}

#[test]
fn test_categorized_bundle_add_category_overwrite() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .add_category(repos.clone(), "# Content 1")
        .add_category(repos.clone(), "# Content 2") // Overwrites
        .build();

    assert_eq!(bundle.categories().len(), 1);
    assert_eq!(bundle.categories().get(&repos).unwrap(), "# Content 2");
}

#[test]
fn test_categorized_bundle_categories_method() {
    use std::collections::HashMap;

    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();
    let manifest = CategoryManifest::builder().build();

    let mut categories = HashMap::new();
    categories.insert(repos.clone(), "# Repos".to_string());
    categories.insert(issues.clone(), "# Issues".to_string());

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .categories(categories.clone())
        .build();

    assert_eq!(bundle.categories().len(), 2);
    assert_eq!(bundle.categories(), &categories);
}

#[test]
fn test_categorized_bundle_scripts_method() {
    let manifest = CategoryManifest::builder().build();

    let scripts = vec![
        ScriptFile::new("tool1", "ts", "code1"),
        ScriptFile::new("tool2", "ts", "code2"),
    ];

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .scripts(scripts)
        .build();

    assert_eq!(bundle.scripts().len(), 2);
}

// ============================================================================
// Edge Cases Tests
// ============================================================================

#[test]
fn test_categorized_bundle_empty_categories() {
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .build();

    assert_eq!(bundle.categories().len(), 0);
}

#[test]
fn test_categorized_bundle_empty_scripts() {
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .build();

    assert_eq!(bundle.scripts().len(), 0);
}

#[test]
fn test_categorized_bundle_large_skill_md() {
    let manifest = CategoryManifest::builder().build();
    let large_content = "# Large Content\n".repeat(1000);

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md(large_content.clone())
        .manifest(manifest)
        .build();

    assert_eq!(bundle.skill_md(), large_content);
}

#[test]
fn test_categorized_bundle_large_category_content() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder().build();
    let large_content = "# Content\n".repeat(1000);

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .add_category(repos.clone(), large_content.clone())
        .build();

    assert_eq!(bundle.categories().get(&repos).unwrap(), &large_content);
}

#[test]
fn test_categorized_bundle_many_categories() {
    let manifest = CategoryManifest::builder().build();

    let mut builder = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest);

    for i in 0..50 {
        let category = SkillCategory::new(format!("cat_{i}")).unwrap();
        builder = builder.add_category(category, format!("# Content {i}"));
    }

    let bundle = builder.build();

    assert_eq!(bundle.categories().len(), 50);
}

#[test]
fn test_categorized_bundle_many_scripts() {
    let manifest = CategoryManifest::builder().build();

    let mut builder = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest);

    for i in 0..100 {
        builder = builder.script(ScriptFile::new(
            format!("tool_{i}"),
            "ts",
            format!("code{i}"),
        ));
    }

    let bundle = builder.build();

    assert_eq!(bundle.scripts().len(), 100);
}

// ============================================================================
// Clone Tests
// ============================================================================

#[test]
fn test_categorized_bundle_clone() {
    let repos = SkillCategory::new("repos").unwrap();
    let manifest = CategoryManifest::builder()
        .add_tool("create_branch", &repos)
        .unwrap()
        .build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("# GitHub")
        .manifest(manifest)
        .add_category(repos.clone(), "# Repos")
        .script(ScriptFile::new("tool", "ts", "code"))
        .build();

    let cloned = bundle.clone();

    assert_eq!(bundle.name().as_str(), cloned.name().as_str());
    assert_eq!(bundle.skill_md(), cloned.skill_md());
    assert_eq!(bundle.categories().len(), cloned.categories().len());
    assert_eq!(bundle.scripts().len(), cloned.scripts().len());
}

// ============================================================================
// Debug Tests
// ============================================================================

#[test]
fn test_categorized_bundle_debug() {
    let manifest = CategoryManifest::builder().build();

    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---")
        .manifest(manifest)
        .build();

    let debug_str = format!("{bundle:?}");
    assert!(debug_str.contains("CategorizedSkillBundle"));
}

// ============================================================================
// Integration-Style Tests
// ============================================================================

#[test]
fn test_categorized_bundle_complete_workflow() {
    // Create categories
    let repos = SkillCategory::new("repos").unwrap();
    let issues = SkillCategory::new("issues").unwrap();
    let prs = SkillCategory::new("prs").unwrap();

    // Build manifest
    let manifest = CategoryManifest::builder()
        .add_tools(vec!["create_branch", "list_commits"], &repos)
        .unwrap()
        .add_tools(vec!["create_issue", "list_issues"], &issues)
        .unwrap()
        .add_tools(vec!["create_pr", "merge_pr"], &prs)
        .unwrap()
        .build();

    // Build bundle
    let bundle = CategorizedSkillBundle::builder("github")
        .unwrap()
        .skill_md("---\nname: github\n---\n# GitHub Skill")
        .manifest(manifest)
        .add_category(
            repos.clone(),
            "# Repository Operations\n\nTools for repo management.",
        )
        .add_category(
            issues.clone(),
            "# Issue Operations\n\nTools for issue management.",
        )
        .add_category(
            prs.clone(),
            "# Pull Request Operations\n\nTools for PR management.",
        )
        .script(ScriptFile::new("create_branch", "ts", "// create branch"))
        .script(ScriptFile::new("list_commits", "ts", "// list commits"))
        .script(ScriptFile::new("create_issue", "ts", "// create issue"))
        .script(ScriptFile::new("list_issues", "ts", "// list issues"))
        .script(ScriptFile::new("create_pr", "ts", "// create pr"))
        .script(ScriptFile::new("merge_pr", "ts", "// merge pr"))
        .reference_md("# Full GitHub API Reference")
        .build();

    // Verify structure
    assert_eq!(bundle.name().as_str(), "github");
    assert_eq!(bundle.manifest().category_count(), 3);
    assert_eq!(bundle.manifest().tool_count(), 6);
    assert_eq!(bundle.categories().len(), 3);
    assert_eq!(bundle.scripts().len(), 6);
    assert!(bundle.reference_md().is_some());

    // Verify categories
    assert!(
        bundle
            .categories()
            .get(&repos)
            .unwrap()
            .contains("Repository")
    );
    assert!(bundle.categories().get(&issues).unwrap().contains("Issue"));
    assert!(
        bundle
            .categories()
            .get(&prs)
            .unwrap()
            .contains("Pull Request")
    );

    // Verify manifest mappings
    assert_eq!(
        bundle.manifest().find_category("create_branch"),
        Some(&repos)
    );
    assert_eq!(
        bundle.manifest().find_category("create_issue"),
        Some(&issues)
    );
    assert_eq!(bundle.manifest().find_category("create_pr"), Some(&prs));
}
