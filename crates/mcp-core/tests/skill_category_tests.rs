//! Comprehensive tests for SkillCategory type.
//!
//! Tests cover all validation logic, edge cases, and security scenarios.

use mcp_core::{Error, SkillCategory};

// ============================================================================
// Basic Creation Tests
// ============================================================================

#[test]
fn test_skill_category_new() {
    assert!(SkillCategory::new("repos").is_ok());
    assert!(SkillCategory::new("user-management").is_ok());
    assert!(SkillCategory::new("prs_v2").is_ok());
    assert!(SkillCategory::new("a").is_ok());
    assert!(SkillCategory::new("category_123").is_ok());
}

#[test]
fn test_skill_category_as_str() {
    let cat = SkillCategory::new("repos").unwrap();
    assert_eq!(cat.as_str(), "repos");

    let cat2 = SkillCategory::new("pull-requests").unwrap();
    assert_eq!(cat2.as_str(), "pull-requests");
}

#[test]
fn test_skill_category_filename() {
    let cat = SkillCategory::new("repos").unwrap();
    assert_eq!(cat.filename(), "repos.md");

    let cat2 = SkillCategory::new("user-mgmt").unwrap();
    assert_eq!(cat2.filename(), "user-mgmt.md");
}

#[test]
fn test_skill_category_relative_path() {
    let cat = SkillCategory::new("repos").unwrap();
    assert_eq!(cat.relative_path(), "categories/repos.md");

    let cat2 = SkillCategory::new("pull_requests").unwrap();
    assert_eq!(cat2.relative_path(), "categories/pull_requests.md");
}

// ============================================================================
// Validation Tests - Empty and Length
// ============================================================================

#[test]
fn test_skill_category_empty_string() {
    let result = SkillCategory::new("");
    assert!(result.is_err());

    match result.unwrap_err() {
        Error::ValidationError { field, reason } => {
            assert!(field.contains("category"));
            assert!(reason.contains("1-50 characters"));
        }
        _ => panic!("Expected ValidationError"),
    }
}

#[test]
fn test_skill_category_too_long() {
    let long_name = "a".repeat(51);
    let result = SkillCategory::new(&long_name);
    assert!(result.is_err());

    match result.unwrap_err() {
        Error::ValidationError { field, reason } => {
            assert!(field.contains("category"));
            assert!(reason.contains("1-50 characters"));
            assert!(reason.contains("51"));
        }
        _ => panic!("Expected ValidationError"),
    }
}

#[test]
fn test_skill_category_max_valid_length() {
    let name = "a".repeat(50);
    let result = SkillCategory::new(&name);
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_str().len(), 50);
}

#[test]
fn test_skill_category_single_char() {
    let result = SkillCategory::new("a");
    assert!(result.is_ok());
    assert_eq!(result.unwrap().as_str(), "a");
}

// ============================================================================
// Validation Tests - Path Traversal
// ============================================================================

#[test]
fn test_skill_category_path_traversal_parent() {
    assert!(SkillCategory::new("..").is_err());
}

#[test]
fn test_skill_category_path_traversal_parent_dir() {
    assert!(SkillCategory::new("../etc").is_err());
}

#[test]
fn test_skill_category_dot_prefix() {
    assert!(SkillCategory::new(".hidden").is_err());
}

#[test]
fn test_skill_category_dot_dot_prefix() {
    assert!(SkillCategory::new("..hidden").is_err());
}

// ============================================================================
// Validation Tests - Invalid Characters
// ============================================================================

#[test]
fn test_skill_category_uppercase() {
    assert!(SkillCategory::new("UPPERCASE").is_err());
    assert!(SkillCategory::new("Repos").is_err());
    assert!(SkillCategory::new("rePos").is_err());
}

#[test]
fn test_skill_category_slash() {
    assert!(SkillCategory::new("repo/admin").is_err());
}

#[test]
fn test_skill_category_backslash() {
    assert!(SkillCategory::new("repo\\admin").is_err());
}

#[test]
fn test_skill_category_colon() {
    assert!(SkillCategory::new("repo:admin").is_err());
}

#[test]
fn test_skill_category_spaces() {
    assert!(SkillCategory::new("has spaces").is_err());
    assert!(SkillCategory::new(" repos").is_err());
    assert!(SkillCategory::new("repos ").is_err());
}

#[test]
fn test_skill_category_special_characters() {
    assert!(SkillCategory::new("repo@admin").is_err());
    assert!(SkillCategory::new("repo#admin").is_err());
    assert!(SkillCategory::new("repo$admin").is_err());
    assert!(SkillCategory::new("repo%admin").is_err());
    assert!(SkillCategory::new("repo&admin").is_err());
    assert!(SkillCategory::new("repo*admin").is_err());
    assert!(SkillCategory::new("repo(admin)").is_err());
    assert!(SkillCategory::new("repo[admin]").is_err());
    assert!(SkillCategory::new("repo{admin}").is_err());
    assert!(SkillCategory::new("repo+admin").is_err());
    assert!(SkillCategory::new("repo=admin").is_err());
}

#[test]
fn test_skill_category_starts_with_number() {
    assert!(SkillCategory::new("123numeric").is_err());
    assert!(SkillCategory::new("1repos").is_err());
    assert!(SkillCategory::new("0category").is_err());
}

#[test]
fn test_skill_category_starts_with_hyphen() {
    assert!(SkillCategory::new("-repos").is_err());
}

#[test]
fn test_skill_category_starts_with_underscore() {
    assert!(SkillCategory::new("_repos").is_err());
}

// ============================================================================
// Validation Tests - Valid Patterns
// ============================================================================

#[test]
fn test_skill_category_valid_patterns() {
    assert!(SkillCategory::new("repos").is_ok());
    assert!(SkillCategory::new("user-management").is_ok());
    assert!(SkillCategory::new("pull_requests").is_ok());
    assert!(SkillCategory::new("issues_v2").is_ok());
    assert!(SkillCategory::new("api-v1").is_ok());
    assert!(SkillCategory::new("category123").is_ok());
    assert!(SkillCategory::new("cat-123").is_ok());
    assert!(SkillCategory::new("cat_123").is_ok());
    assert!(SkillCategory::new("a1b2c3").is_ok());
}

#[test]
fn test_skill_category_numbers_in_middle() {
    assert!(SkillCategory::new("v2repos").is_ok());
    assert!(SkillCategory::new("api3tools").is_ok());
}

#[test]
fn test_skill_category_numbers_at_end() {
    assert!(SkillCategory::new("repos123").is_ok());
    assert!(SkillCategory::new("category999").is_ok());
}

#[test]
fn test_skill_category_mixed_separators() {
    assert!(SkillCategory::new("repo-name_v2").is_ok());
    assert!(SkillCategory::new("user_mgmt-v1").is_ok());
}

// ============================================================================
// Equality and Hashing Tests
// ============================================================================

#[test]
fn test_skill_category_equality() {
    let cat1 = SkillCategory::new("repos").unwrap();
    let cat2 = SkillCategory::new("repos").unwrap();
    let cat3 = SkillCategory::new("issues").unwrap();

    assert_eq!(cat1, cat2);
    assert_ne!(cat1, cat3);
}

#[test]
fn test_skill_category_clone() {
    let cat1 = SkillCategory::new("repos").unwrap();
    let cat2 = cat1.clone();

    assert_eq!(cat1, cat2);
}

#[test]
fn test_skill_category_hash() {
    use std::collections::HashSet;

    let mut set = HashSet::new();
    set.insert(SkillCategory::new("repos").unwrap());
    set.insert(SkillCategory::new("issues").unwrap());
    set.insert(SkillCategory::new("repos").unwrap()); // Duplicate

    assert_eq!(set.len(), 2);
}

// ============================================================================
// Display Tests
// ============================================================================

#[test]
fn test_skill_category_display() {
    let cat = SkillCategory::new("repos").unwrap();
    assert_eq!(format!("{cat}"), "repos");

    let cat2 = SkillCategory::new("pull-requests").unwrap();
    assert_eq!(format!("{cat2}"), "pull-requests");
}

#[test]
fn test_skill_category_debug() {
    let cat = SkillCategory::new("repos").unwrap();
    let debug_str = format!("{cat:?}");
    assert!(debug_str.contains("SkillCategory"));
    assert!(debug_str.contains("repos"));
}

// ============================================================================
// Serialization Tests
// ============================================================================

#[test]
fn test_skill_category_serialization_json() {
    let cat = SkillCategory::new("repos").unwrap();
    let json = serde_json::to_string(&cat).unwrap();

    // Should serialize as string (transparent)
    assert_eq!(json, r#""repos""#);
}

#[test]
fn test_skill_category_deserialization_json() {
    let json = r#""repos""#;
    let cat: SkillCategory = serde_json::from_str(json).unwrap();

    assert_eq!(cat.as_str(), "repos");
}

#[test]
fn test_skill_category_roundtrip_json() {
    let cat = SkillCategory::new("repos").unwrap();
    let json = serde_json::to_string(&cat).unwrap();
    let deserialized: SkillCategory = serde_json::from_str(&json).unwrap();

    assert_eq!(cat, deserialized);
}

#[test]
fn test_skill_category_serialization_yaml() {
    let cat = SkillCategory::new("repos").unwrap();
    let yaml = serde_yaml::to_string(&cat).unwrap();

    assert!(yaml.contains("repos"));
}

// ============================================================================
// Send + Sync Tests
// ============================================================================

#[test]
fn test_skill_category_is_send() {
    fn assert_send<T: Send>() {}
    assert_send::<SkillCategory>();
}

#[test]
fn test_skill_category_is_sync() {
    fn assert_sync<T: Sync>() {}
    assert_sync::<SkillCategory>();
}

// ============================================================================
// Edge Cases
// ============================================================================

#[test]
fn test_skill_category_unicode_rejected() {
    assert!(SkillCategory::new("репо").is_err()); // Cyrillic
    assert!(SkillCategory::new("レポ").is_err()); // Japanese
    assert!(SkillCategory::new("مستودع").is_err()); // Arabic
}

#[test]
fn test_skill_category_emoji_rejected() {
    assert!(SkillCategory::new("repo🚀").is_err());
    assert!(SkillCategory::new("🚀repo").is_err());
}

#[test]
fn test_skill_category_null_byte_rejected() {
    assert!(SkillCategory::new("repo\0").is_err());
    assert!(SkillCategory::new("\0repo").is_err());
}

#[test]
fn test_skill_category_newline_rejected() {
    assert!(SkillCategory::new("repo\n").is_err());
    assert!(SkillCategory::new("repo\r\n").is_err());
}

#[test]
fn test_skill_category_tab_rejected() {
    assert!(SkillCategory::new("repo\t").is_err());
}

#[test]
fn test_skill_category_only_numbers() {
    assert!(SkillCategory::new("123").is_err());
    assert!(SkillCategory::new("999").is_err());
}

#[test]
fn test_skill_category_only_hyphens() {
    assert!(SkillCategory::new("---").is_err());
}

#[test]
fn test_skill_category_only_underscores() {
    assert!(SkillCategory::new("___").is_err());
}
