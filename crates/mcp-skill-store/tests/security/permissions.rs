//! File permission security tests for categorized skills.
//!
//! Validates that category files and directories have appropriate permissions.

use mcp_core::{CategoryManifest, ScriptFile, SkillBundle, SkillCategory, SkillName};
use mcp_skill_store::SkillStore;
use std::fs;
use tempfile::TempDir;

/// Creates a test skill store in a temporary directory.
fn create_test_store() -> (SkillStore, TempDir) {
    let temp_dir = TempDir::new().expect("Failed to create temp dir");
    let skills_dir = temp_dir.path().join("skills");
    fs::create_dir_all(&skills_dir).expect("Failed to create skills dir");

    let store = SkillStore::new(skills_dir).expect("Failed to create store");
    (store, temp_dir)
}

/// Creates a test categorized skill bundle.
fn create_test_categorized_bundle() -> CategorizedSkillBundle {
    let skill_name = SkillName::new("test-skill").expect("Valid skill name");

    let mut manifest = CategoryManifest::builder();

    let repos = SkillCategory::new("repos").expect("Valid category");
    let issues = SkillCategory::new("issues").expect("Valid category");

    manifest.add_category(repos.clone(), vec!["create_branch", "list_commits"]);
    manifest.add_category(issues.clone(), vec!["create_issue", "list_issues"]);

    let built_manifest = manifest.build();

    // Create category content
    let mut categories = std::collections::HashMap::new();
    categories.insert(repos, "# Repository Operations\n\nTools for repos.".to_string());
    categories.insert(
        issues,
        "# Issue Operations\n\nTools for issues.".to_string(),
    );

    CategorizedSkillBundle::builder(skill_name)
        .manifest(built_manifest)
        .categories(categories)
        .skill_md("# Test Skill\n\nTest skill content.")
        .script(ScriptFile::new(
            "create_branch",
            "ts",
            "// TypeScript code",
        ))
        .build()
}

/// Test that category files have correct permissions on Unix systems.
#[cfg(unix)]
#[test]
fn test_category_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    store
        .save_categorized_bundle(&bundle)
        .expect("Failed to save bundle");

    let skill_dir = store
        .skills_dir()
        .join(bundle.name().as_str())
        .join("categories");

    // Check each category file
    for (category, _content) in bundle.categories() {
        let cat_path = skill_dir.join(format!("{}.md", category.as_str()));

        assert!(cat_path.exists(), "Category file should exist");

        let metadata = fs::metadata(&cat_path).expect("Failed to get metadata");
        let perms = metadata.permissions();
        let mode = perms.mode();

        // Should be rw-r--r-- (0644)
        let expected_mode = 0o644;
        let actual_mode = mode & 0o777;

        assert_eq!(
            actual_mode, expected_mode,
            "Category file {:?} should have mode 0{:o}, but has 0{:o}",
            cat_path, expected_mode, actual_mode
        );

        // Should not be executable
        assert!(
            !perms.readonly(),
            "Category file should be writable by owner"
        );
        assert_eq!(
            mode & 0o111,
            0,
            "Category file should not be executable"
        );
    }
}

/// Test that categories directory has correct permissions on Unix systems.
#[cfg(unix)]
#[test]
fn test_categories_directory_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    store
        .save_categorized_bundle(&bundle)
        .expect("Failed to save bundle");

    let categories_dir = store
        .skills_dir()
        .join(bundle.name().as_str())
        .join("categories");

    assert!(
        categories_dir.exists(),
        "Categories directory should exist"
    );
    assert!(
        categories_dir.is_dir(),
        "Categories path should be a directory"
    );

    let metadata = fs::metadata(&categories_dir).expect("Failed to get metadata");
    let perms = metadata.permissions();
    let mode = perms.mode();

    // Should be rwxr-xr-x (0755)
    let expected_mode = 0o755;
    let actual_mode = mode & 0o777;

    assert_eq!(
        actual_mode, expected_mode,
        "Categories directory should have mode 0{:o}, but has 0{:o}",
        expected_mode, actual_mode
    );

    // Should be executable (searchable) by all
    assert_ne!(
        mode & 0o111,
        0,
        "Categories directory should be executable/searchable"
    );
}

/// Test that manifest file has correct permissions on Unix systems.
#[cfg(unix)]
#[test]
fn test_manifest_file_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    store
        .save_categorized_bundle(&bundle)
        .expect("Failed to save bundle");

    let manifest_path = store
        .skills_dir()
        .join(bundle.name().as_str())
        .join("manifest.yaml");

    assert!(manifest_path.exists(), "Manifest file should exist");

    let metadata = fs::metadata(&manifest_path).expect("Failed to get metadata");
    let perms = metadata.permissions();
    let mode = perms.mode();

    // Should be rw-r--r-- (0644)
    let expected_mode = 0o644;
    let actual_mode = mode & 0o777;

    assert_eq!(
        actual_mode, expected_mode,
        "Manifest file should have mode 0{:o}, but has 0{:o}",
        expected_mode, actual_mode
    );

    // Should not be executable
    assert_eq!(
        mode & 0o111,
        0,
        "Manifest file should not be executable"
    );
}

/// Test that script files maintain correct permissions.
#[cfg(unix)]
#[test]
fn test_script_file_permissions_unchanged() {
    use std::os::unix::fs::PermissionsExt;

    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    store
        .save_categorized_bundle(&bundle)
        .expect("Failed to save bundle");

    let scripts_dir = store
        .skills_dir()
        .join(bundle.name().as_str())
        .join("scripts");

    // Check script files still have correct permissions
    for script in bundle.scripts() {
        let script_path = scripts_dir.join(script.reference().filename());

        assert!(script_path.exists(), "Script file should exist");

        let metadata = fs::metadata(&script_path).expect("Failed to get metadata");
        let perms = metadata.permissions();
        let mode = perms.mode();

        // Scripts should be rw-r--r-- (0644) - not executable
        // They are TypeScript, not shell scripts
        let expected_mode = 0o644;
        let actual_mode = mode & 0o777;

        assert_eq!(
            actual_mode, expected_mode,
            "Script file should have mode 0{:o}, but has 0{:o}",
            expected_mode, actual_mode
        );
    }
}

/// Test that SKILL.md has correct permissions on Unix systems.
#[cfg(unix)]
#[test]
fn test_skill_md_permissions() {
    use std::os::unix::fs::PermissionsExt;

    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    store
        .save_categorized_bundle(&bundle)
        .expect("Failed to save bundle");

    let skill_md_path = store
        .skills_dir()
        .join(bundle.name().as_str())
        .join("SKILL.md");

    assert!(skill_md_path.exists(), "SKILL.md should exist");

    let metadata = fs::metadata(&skill_md_path).expect("Failed to get metadata");
    let perms = metadata.permissions();
    let mode = perms.mode();

    // Should be rw-r--r-- (0644)
    let expected_mode = 0o644;
    let actual_mode = mode & 0o777;

    assert_eq!(
        actual_mode, expected_mode,
        "SKILL.md should have mode 0{:o}, but has 0{:o}",
        expected_mode, actual_mode
    );
}

/// Test that permission setting is idempotent.
#[cfg(unix)]
#[test]
fn test_permission_setting_idempotent() {
    use std::os::unix::fs::PermissionsExt;

    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    // Save bundle twice
    store
        .save_categorized_bundle(&bundle)
        .expect("First save failed");
    store
        .save_categorized_bundle(&bundle)
        .expect("Second save failed");

    // Check permissions are still correct
    let categories_dir = store
        .skills_dir()
        .join(bundle.name().as_str())
        .join("categories");

    for (category, _) in bundle.categories() {
        let cat_path = categories_dir.join(format!("{}.md", category.as_str()));
        let metadata = fs::metadata(&cat_path).expect("Failed to get metadata");
        let mode = metadata.permissions().mode() & 0o777;

        assert_eq!(
            mode, 0o644,
            "Permissions should remain 0644 after re-save"
        );
    }
}

/// Test permissions on Windows (limited test - just check files exist and are accessible).
#[cfg(windows)]
#[test]
fn test_category_files_accessible_windows() {
    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    store
        .save_categorized_bundle(&bundle)
        .expect("Failed to save bundle");

    let categories_dir = store
        .skills_dir()
        .join(bundle.name().as_str())
        .join("categories");

    // On Windows, just verify files are readable and writable
    for (category, _content) in bundle.categories() {
        let cat_path = categories_dir.join(format!("{}.md", category.as_str()));

        assert!(cat_path.exists(), "Category file should exist");

        let metadata = fs::metadata(&cat_path).expect("Failed to get metadata");
        assert!(!metadata.permissions().readonly(), "File should be writable");

        // Verify we can read the file
        let _content = fs::read_to_string(&cat_path).expect("File should be readable");
    }
}

/// Test that no files have setuid/setgid bits set.
#[cfg(unix)]
#[test]
fn test_no_setuid_setgid_bits() {
    use std::os::unix::fs::PermissionsExt;

    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    store
        .save_categorized_bundle(&bundle)
        .expect("Failed to save bundle");

    let skill_dir = store.skills_dir().join(bundle.name().as_str());

    // Check all files recursively
    for entry in walkdir::WalkDir::new(&skill_dir) {
        let entry = entry.expect("Failed to read entry");
        if entry.file_type().is_file() {
            let metadata = fs::metadata(entry.path()).expect("Failed to get metadata");
            let mode = metadata.permissions().mode();

            // No setuid (04000)
            assert_eq!(
                mode & 0o4000,
                0,
                "File {:?} should not have setuid bit",
                entry.path()
            );

            // No setgid (02000)
            assert_eq!(
                mode & 0o2000,
                0,
                "File {:?} should not have setgid bit",
                entry.path()
            );

            // No sticky bit (01000) - not typically harmful but unnecessary
            assert_eq!(
                mode & 0o1000,
                0,
                "File {:?} should not have sticky bit",
                entry.path()
            );
        }
    }
}

/// Test that permission errors are handled gracefully.
#[cfg(unix)]
#[test]
fn test_permission_error_handling() {
    use std::os::unix::fs::PermissionsExt;

    let (store, _temp_dir) = create_test_store();
    let bundle = create_test_categorized_bundle();

    store
        .save_categorized_bundle(&bundle)
        .expect("Failed to save bundle");

    let categories_dir = store
        .skills_dir()
        .join(bundle.name().as_str())
        .join("categories");

    // Make directory read-only
    let mut perms = fs::metadata(&categories_dir)
        .expect("Failed to get metadata")
        .permissions();
    perms.set_mode(0o555); // r-xr-xr-x (read-only)
    fs::set_permissions(&categories_dir, perms).expect("Failed to set permissions");

    // Try to save again - should fail gracefully
    let result = store.save_categorized_bundle(&bundle);

    // Restore permissions for cleanup
    let mut perms = fs::metadata(&categories_dir)
        .expect("Failed to get metadata")
        .permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&categories_dir, perms).expect("Failed to restore permissions");

    assert!(
        result.is_err(),
        "Should fail when directory is read-only"
    );
}
