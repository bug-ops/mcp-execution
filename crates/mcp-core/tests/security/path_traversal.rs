//! Path traversal security tests for categorized skills.
//!
//! Validates that category names cannot be exploited for directory traversal attacks.

use mcp_core::{Error, SkillCategory};

/// Test that category name rejects empty strings.
#[test]
fn test_category_name_rejects_empty() {
    let result = SkillCategory::new("");
    assert!(result.is_err(), "Empty category name should be rejected");

    if let Err(e) = result {
        assert!(
            e.to_string().contains("empty") || e.to_string().contains("invalid"),
            "Error should mention empty/invalid: {}",
            e
        );
    }
}

/// Test that category name rejects path traversal attempts with ../
#[test]
fn test_category_name_rejects_parent_directory() {
    let dangerous_names = vec![
        "../etc/passwd",
        "../../root",
        "../../../etc",
        "foo/../bar",
        "foo/../../etc",
    ];

    for name in dangerous_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_err(),
            "Category name '{}' should be rejected (path traversal)",
            name
        );

        if let Err(e) = result {
            assert!(
                e.to_string().contains("invalid")
                    || e.to_string().contains("traversal")
                    || e.to_string().contains("separator"),
                "Error should indicate path traversal issue for '{}': {}",
                name,
                e
            );
        }
    }
}

/// Test that category name rejects names starting with dot (hidden files).
#[test]
fn test_category_name_rejects_hidden_files() {
    let hidden_names = vec![".hidden", "..", ".git", ".env"];

    for name in hidden_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_err(),
            "Category name '{}' should be rejected (hidden file)",
            name
        );

        if let Err(e) = result {
            assert!(
                e.to_string().contains("invalid") || e.to_string().contains("dot"),
                "Error should indicate hidden file issue for '{}': {}",
                name,
                e
            );
        }
    }
}

/// Test that category name rejects forward slashes (Unix path separator).
#[test]
fn test_category_name_rejects_forward_slash() {
    let slash_names = vec!["foo/bar", "repos/sub", "/root", "foo/"];

    for name in slash_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_err(),
            "Category name '{}' should be rejected (forward slash)",
            name
        );

        if let Err(e) = result {
            assert!(
                e.to_string().contains("separator") || e.to_string().contains("invalid"),
                "Error should indicate path separator issue for '{}': {}",
                name,
                e
            );
        }
    }
}

/// Test that category name rejects backslashes (Windows path separator).
#[test]
fn test_category_name_rejects_backslash() {
    let backslash_names = vec!["foo\\bar", "repos\\sub", "C:\\Windows", "foo\\"];

    for name in backslash_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_err(),
            "Category name '{}' should be rejected (backslash)",
            name
        );

        if let Err(e) = result {
            assert!(
                e.to_string().contains("separator") || e.to_string().contains("invalid"),
                "Error should indicate path separator issue for '{}': {}",
                name,
                e
            );
        }
    }
}

/// Test that category name rejects excessively long names.
#[test]
fn test_category_name_rejects_too_long() {
    // Category names should be limited to reasonable length (e.g., 64 chars)
    let too_long = "a".repeat(65);

    let result = SkillCategory::new(&too_long);
    assert!(
        result.is_err(),
        "Category name should be rejected (too long: {} chars)",
        too_long.len()
    );

    if let Err(e) = result {
        assert!(
            e.to_string().contains("long") || e.to_string().contains("length"),
            "Error should mention length: {}",
            e
        );
    }
}

/// Test that category name rejects special characters.
#[test]
fn test_category_name_rejects_special_chars() {
    let special_names = vec![
        "foo@bar",
        "repos#123",
        "test$var",
        "foo%20bar",
        "repos&issues",
        "test*wild",
        "foo?query",
        "repos|pipe",
        "foo<tag",
        "bar>output",
        "test:colon",
        "foo\"quote",
        "bar'quote",
        "repos;semi",
        "test`backtick",
        "foo~tilde",
        "bar!exclaim",
        "test(paren",
        "foo)paren",
        "bar[bracket",
        "test]bracket",
        "foo{brace",
        "bar}brace",
        "test=equals",
        "foo+plus",
    ];

    for name in special_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_err(),
            "Category name '{}' should be rejected (special character)",
            name
        );

        if let Err(e) = result {
            assert!(
                e.to_string().contains("invalid") || e.to_string().contains("character"),
                "Error should indicate invalid character for '{}': {}",
                name,
                e
            );
        }
    }
}

/// Test that category name accepts valid names.
#[test]
fn test_category_name_accepts_valid() {
    let valid_names = vec![
        "repos",
        "issues",
        "prs",
        "user",
        "search",
        "user-management",
        "pull_requests",
        "prs_v2",
        "repos123",
        "test-category_1",
    ];

    for name in valid_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_ok(),
            "Category name '{}' should be accepted: {:?}",
            name,
            result.err()
        );

        if let Ok(category) = result {
            assert_eq!(
                category.as_str(),
                name,
                "Category should preserve name exactly"
            );
        }
    }
}

/// Test that category filename is safe.
#[test]
fn test_category_filename_safe() {
    let category = SkillCategory::new("repos").expect("Valid category name");
    let filename = category.filename(); // e.g., "repos.md"

    // Filename should not contain path separators
    assert!(
        !filename.contains('/'),
        "Filename should not contain forward slash: {}",
        filename
    );
    assert!(
        !filename.contains('\\'),
        "Filename should not contain backslash: {}",
        filename
    );

    // Filename should not start with dot
    assert!(
        !filename.starts_with('.'),
        "Filename should not start with dot: {}",
        filename
    );

    // Filename should have .md extension
    assert!(
        filename.ends_with(".md"),
        "Filename should have .md extension: {}",
        filename
    );

    // Filename should be just name + extension
    assert_eq!(
        filename, "repos.md",
        "Filename should be exactly 'repos.md'"
    );
}

/// Test that multiple categories with similar names are distinct.
#[test]
fn test_category_name_uniqueness() {
    let cat1 = SkillCategory::new("repos").expect("Valid name");
    let cat2 = SkillCategory::new("repos-v2").expect("Valid name");
    let cat3 = SkillCategory::new("repos_backup").expect("Valid name");

    assert_ne!(cat1.as_str(), cat2.as_str());
    assert_ne!(cat1.as_str(), cat3.as_str());
    assert_ne!(cat2.as_str(), cat3.as_str());

    assert_ne!(cat1.filename(), cat2.filename());
    assert_ne!(cat1.filename(), cat3.filename());
    assert_ne!(cat2.filename(), cat3.filename());
}

/// Test category name case sensitivity.
#[test]
fn test_category_name_case_sensitive() {
    let cat1 = SkillCategory::new("Repos").expect("Valid name");
    let cat2 = SkillCategory::new("repos").expect("Valid name");
    let cat3 = SkillCategory::new("REPOS").expect("Valid name");

    // All should be accepted
    assert_eq!(cat1.as_str(), "Repos");
    assert_eq!(cat2.as_str(), "repos");
    assert_eq!(cat3.as_str(), "REPOS");

    // Filenames should preserve case
    assert_eq!(cat1.filename(), "Repos.md");
    assert_eq!(cat2.filename(), "repos.md");
    assert_eq!(cat3.filename(), "REPOS.md");
}

/// Test that category name rejects null bytes.
#[test]
fn test_category_name_rejects_null_byte() {
    let null_names = vec!["foo\0bar", "\0", "test\0"];

    for name in null_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_err(),
            "Category name with null byte should be rejected"
        );
    }
}

/// Test that category name rejects whitespace-only names.
#[test]
fn test_category_name_rejects_whitespace_only() {
    let whitespace_names = vec![" ", "  ", "\t", "\n", "\r\n", "   \t\n  "];

    for name in whitespace_names {
        let result = SkillCategory::new(name);
        assert!(
            result.is_err(),
            "Whitespace-only category name should be rejected: {:?}",
            name
        );
    }
}

/// Test that category name handles maximum valid length.
#[test]
fn test_category_name_maximum_valid_length() {
    // Exactly 64 characters should be accepted
    let max_valid = "a".repeat(64);
    let result = SkillCategory::new(&max_valid);
    assert!(
        result.is_ok(),
        "Category name with 64 chars should be accepted"
    );
}

/// Test that category name normalizes or rejects leading/trailing whitespace.
#[test]
fn test_category_name_whitespace_handling() {
    let names_with_whitespace = vec![" repos", "repos ", " repos ", "\trepos", "repos\n"];

    for name in names_with_whitespace {
        let result = SkillCategory::new(name);
        // Either reject or normalize - implementation dependent
        if let Ok(category) = result {
            // If accepted, should be trimmed
            assert!(
                !category.as_str().starts_with(' '),
                "Category name should not have leading space"
            );
            assert!(
                !category.as_str().ends_with(' '),
                "Category name should not have trailing space"
            );
            assert!(
                !category.as_str().contains('\n'),
                "Category name should not contain newline"
            );
            assert!(
                !category.as_str().contains('\t'),
                "Category name should not contain tab"
            );
        }
    }
}
