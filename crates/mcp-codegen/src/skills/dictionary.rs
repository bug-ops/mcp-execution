//! External dictionary for categorization heuristics.
//!
//! Provides loading and querying of categorization patterns from YAML files.
//! Allows users to customize categorization behavior without code changes.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_codegen::skills::CategorizationDictionary;
//!
//! // Load default embedded dictionary
//! let dict = CategorizationDictionary::default_dictionary().unwrap();
//!
//! // Find verb for tool name
//! assert_eq!(dict.find_verb("create_user"), Some("create"));
//! assert_eq!(dict.find_verb("get_file"), Some("read"));
//!
//! // Find entity for tool name
//! assert_eq!(dict.find_entity("create_user"), Some("users"));
//! assert_eq!(dict.find_entity("read_file"), Some("files"));
//! ```

use mcp_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Categorization dictionary loaded from YAML.
///
/// Contains patterns for verbs, entities, and categorization rules.
/// Can be loaded from embedded default or custom file.
///
/// # Examples
///
/// ```no_run
/// use mcp_codegen::skills::CategorizationDictionary;
///
/// // Use default dictionary
/// let dict = CategorizationDictionary::default_dictionary()?;
///
/// // Or load custom dictionary
/// let dict = CategorizationDictionary::from_file("./custom_rules.yaml")?;
/// # Ok::<(), mcp_core::Error>(())
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorizationDictionary {
    /// Dictionary version
    pub version: String,
    /// Verb patterns for CRUD operations
    pub verbs: HashMap<String, VerbPattern>,
    /// Entity patterns for common resources
    pub entities: HashMap<String, EntityPattern>,
    /// Categorization rules and preferences
    pub rules: CategoryRules,
}

/// Pattern definition for a verb (CRUD operation).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerbPattern {
    /// Patterns to match in tool names (e.g., `["create", "add", "new"]`)
    pub patterns: Vec<String>,
    /// Human-readable description of this verb
    pub description: String,
}

/// Pattern definition for an entity (resource type).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EntityPattern {
    /// Patterns to match in tool names (e.g., `["user", "member", "account"]`)
    pub patterns: Vec<String>,
    /// Human-readable description of this entity
    pub description: String,
    /// Normalized synonyms (e.g., `["users", "accounts", "members"]`)
    pub synonyms: Vec<String>,
}

/// Categorization rules and preferences.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryRules {
    /// Maximum tools per category before splitting
    pub max_per_category: usize,
    /// Minimum tools per category before merging to "other"
    pub min_per_category: usize,
    /// Preferred grouping strategy: "verbs", "entities", or "hybrid"
    pub prefer: String,
    /// Case sensitivity for pattern matching
    pub case_sensitive: bool,
    /// Fallback strategy: "other", "auto", or "individual"
    pub fallback: String,
}

impl CategorizationDictionary {
    /// Load dictionary from embedded default.
    ///
    /// Returns the default dictionary bundled with the crate.
    ///
    /// # Errors
    ///
    /// Returns error if YAML parsing fails (should never happen with valid default).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::skills::CategorizationDictionary;
    ///
    /// let dict = CategorizationDictionary::default_dictionary().unwrap();
    /// assert_eq!(dict.version, "1.0");
    /// ```
    pub fn default_dictionary() -> Result<Self> {
        let yaml = include_str!("../../data/categorization_dictionary.yaml");
        Self::from_yaml(yaml)
    }

    /// Load dictionary from custom file.
    ///
    /// # Arguments
    ///
    /// * `path` - Path to YAML dictionary file
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - File cannot be read
    /// - YAML is malformed
    /// - Required fields are missing
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::CategorizationDictionary;
    ///
    /// let dict = CategorizationDictionary::from_file("./custom_rules.yaml")?;
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    pub fn from_file<P: AsRef<Path>>(path: P) -> Result<Self> {
        let content = std::fs::read_to_string(path).map_err(|e| Error::ConfigError {
            message: format!("Failed to read dictionary file: {e}"),
        })?;
        Self::from_yaml(&content)
    }

    /// Parse dictionary from YAML string.
    ///
    /// # Errors
    ///
    /// Returns error if YAML is malformed or missing required fields.
    fn from_yaml(yaml: &str) -> Result<Self> {
        serde_yaml::from_str(yaml).map_err(|e| Error::SerializationError {
            message: format!("Failed to parse dictionary YAML: {e}"),
            source: None,
        })
    }

    /// Find verb for tool name.
    ///
    /// Returns the canonical verb name if any pattern matches.
    /// Case sensitivity is controlled by `rules.case_sensitive`.
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of tool to analyze
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::CategorizationDictionary;
    ///
    /// let dict = CategorizationDictionary::default_dictionary()?;
    /// assert_eq!(dict.find_verb("create_user"), Some("create"));
    /// assert_eq!(dict.find_verb("get_file"), Some("read"));
    /// assert_eq!(dict.find_verb("unknown_operation"), None);
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    #[must_use]
    pub fn find_verb(&self, tool_name: &str) -> Option<&str> {
        let name = if self.rules.case_sensitive {
            tool_name.to_string()
        } else {
            tool_name.to_lowercase()
        };

        for (verb, pattern) in &self.verbs {
            for pat in &pattern.patterns {
                let search_pattern = if self.rules.case_sensitive {
                    pat.clone()
                } else {
                    pat.to_lowercase()
                };
                if name.contains(&search_pattern) {
                    return Some(verb);
                }
            }
        }
        None
    }

    /// Find entity for tool name.
    ///
    /// Returns the normalized entity name if any pattern matches.
    /// Case sensitivity is controlled by `rules.case_sensitive`.
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of tool to analyze
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::CategorizationDictionary;
    ///
    /// let dict = CategorizationDictionary::default_dictionary()?;
    /// assert_eq!(dict.find_entity("create_user"), Some("users"));
    /// assert_eq!(dict.find_entity("read_file"), Some("files"));
    /// assert_eq!(dict.find_entity("unknown_resource"), None);
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    #[must_use]
    pub fn find_entity(&self, tool_name: &str) -> Option<&str> {
        let name = if self.rules.case_sensitive {
            tool_name.to_string()
        } else {
            tool_name.to_lowercase()
        };

        for (entity, pattern) in &self.entities {
            for pat in &pattern.patterns {
                let search_pattern = if self.rules.case_sensitive {
                    pat.clone()
                } else {
                    pat.to_lowercase()
                };
                if name.contains(&search_pattern) {
                    return Some(entity);
                }
            }
        }
        None
    }

    /// Get verb description.
    ///
    /// # Arguments
    ///
    /// * `verb` - Verb name to look up
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::CategorizationDictionary;
    ///
    /// let dict = CategorizationDictionary::default_dictionary()?;
    /// let desc = dict.verb_description("create");
    /// assert!(desc.is_some());
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    #[must_use]
    pub fn verb_description(&self, verb: &str) -> Option<&str> {
        self.verbs.get(verb).map(|p| p.description.as_str())
    }

    /// Get entity description.
    ///
    /// # Arguments
    ///
    /// * `entity` - Entity name to look up
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::CategorizationDictionary;
    ///
    /// let dict = CategorizationDictionary::default_dictionary()?;
    /// let desc = dict.entity_description("users");
    /// assert!(desc.is_some());
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    #[must_use]
    pub fn entity_description(&self, entity: &str) -> Option<&str> {
        self.entities.get(entity).map(|p| p.description.as_str())
    }
}

impl Default for CategorizationDictionary {
    fn default() -> Self {
        Self::default_dictionary().expect("Default dictionary should always load successfully")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_load_default_dictionary() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.version, "1.0");
        assert!(!dict.verbs.is_empty());
        assert!(!dict.entities.is_empty());
    }

    #[test]
    fn test_default_trait() {
        let dict = CategorizationDictionary::default();
        assert_eq!(dict.version, "1.0");
    }

    #[test]
    fn test_find_verb_create() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_verb("create_user"), Some("create"));
        assert_eq!(dict.find_verb("add_item"), Some("create"));
        assert_eq!(dict.find_verb("new_record"), Some("create"));
        assert_eq!(dict.find_verb("insert_row"), Some("create"));
    }

    #[test]
    fn test_find_verb_read() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_verb("get_user"), Some("read"));
        assert_eq!(dict.find_verb("list_items"), Some("read"));
        assert_eq!(dict.find_verb("fetch_data"), Some("read"));
        assert_eq!(dict.find_verb("show_details"), Some("read"));
    }

    #[test]
    fn test_find_verb_update() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_verb("update_user"), Some("update"));
        assert_eq!(dict.find_verb("edit_item"), Some("update"));
        assert_eq!(dict.find_verb("modify_record"), Some("update"));
        assert_eq!(dict.find_verb("set_value"), Some("update"));
    }

    #[test]
    fn test_find_verb_delete() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_verb("delete_user"), Some("delete"));
        assert_eq!(dict.find_verb("remove_item"), Some("delete"));
        assert_eq!(dict.find_verb("destroy_record"), Some("delete"));
    }

    #[test]
    fn test_find_verb_search() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_verb("search_users"), Some("search"));
        assert_eq!(dict.find_verb("find_items"), Some("search"));
        assert_eq!(dict.find_verb("query_data"), Some("search"));
    }

    #[test]
    fn test_find_verb_none() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_verb("unknown_operation"), None);
    }

    #[test]
    fn test_find_entity_users() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_entity("create_user"), Some("users"));
        assert_eq!(dict.find_entity("get_member"), Some("users"));
        assert_eq!(dict.find_entity("update_account"), Some("users"));
    }

    #[test]
    fn test_find_entity_files() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_entity("read_file"), Some("files"));
        assert_eq!(dict.find_entity("write_document"), Some("files"));
    }

    #[test]
    fn test_find_entity_messages() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_entity("send_message"), Some("messages"));
        assert_eq!(dict.find_entity("add_comment"), Some("messages"));
    }

    #[test]
    fn test_find_entity_repositories() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_entity("create_repository"), Some("repositories"));
        assert_eq!(dict.find_entity("fork_repo"), Some("repositories"));
    }

    #[test]
    fn test_find_entity_none() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_entity("unknown_resource"), None);
    }

    #[test]
    fn test_case_insensitive_verb() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_verb("CREATE_USER"), Some("create"));
        assert_eq!(dict.find_verb("Get_File"), Some("read"));
        assert_eq!(dict.find_verb("UPDATE_record"), Some("update"));
    }

    #[test]
    fn test_case_insensitive_entity() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.find_entity("CREATE_USER"), Some("users"));
        assert_eq!(dict.find_entity("Read_FILE"), Some("files"));
    }

    #[test]
    fn test_verb_description() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        let desc = dict.verb_description("create");
        assert!(desc.is_some());
        assert!(desc.unwrap().contains("Creation"));
    }

    #[test]
    fn test_entity_description() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        let desc = dict.entity_description("users");
        assert!(desc.is_some());
        assert!(desc.unwrap().contains("User"));
    }

    #[test]
    fn test_rules_loaded() {
        let dict = CategorizationDictionary::default_dictionary().unwrap();
        assert_eq!(dict.rules.max_per_category, 15);
        assert_eq!(dict.rules.min_per_category, 2);
        assert_eq!(dict.rules.prefer, "hybrid");
        assert!(!dict.rules.case_sensitive);
        assert_eq!(dict.rules.fallback, "auto");
    }

    #[test]
    fn test_from_yaml_invalid() {
        let invalid_yaml = "invalid: yaml: content:\n  - broken";
        let result = CategorizationDictionary::from_yaml(invalid_yaml);
        assert!(result.is_err());
    }

    #[test]
    fn test_from_file_not_exists() {
        let result = CategorizationDictionary::from_file("/nonexistent/path.yaml");
        assert!(result.is_err());
    }
}
