//! Security tests module for categorized skills.
//!
//! This module contains comprehensive security tests for the categorized skills
//! feature to ensure:
//! - No path traversal vulnerabilities
//! - No YAML injection vulnerabilities
//! - Proper file permissions
//! - Protection against denial of service
//!
//! All tests in this module MUST pass before merging Issue #27.

// Re-export test modules
mod path_traversal;
mod yaml_injection;

// Note: These tests depend on types that will be implemented in Issue #27:
// - SkillCategory
// - CategoryManifest
// - CategorizedSkillBundle
//
// When implementing, ensure these types are exported from mcp_core::lib.rs

#[cfg(test)]
mod setup {
    /// Test helper for security tests
    pub fn assert_no_path_traversal(path: &str) {
        assert!(!path.contains(".."), "Path should not contain '..'");
        assert!(!path.starts_with('/'), "Path should be relative");
        assert!(!path.contains('\\'), "Path should use forward slashes");
    }
}
