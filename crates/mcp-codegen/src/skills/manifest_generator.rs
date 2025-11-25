//! Universal manifest generation for categorized skills.
//!
//! Automatically categorizes MCP tools from ANY server using heuristic analysis
//! of tool names. Works with GitHub, Slack, Jira, filesystem, database servers,
//! and any other MCP implementation without hardcoded domain knowledge.
//!
//! # Categorization Strategies
//!
//! - **Auto**: Heuristic analysis of verbs (CRUD) and entities in tool names
//! - **Custom**: User-provided rules with configurable fallback behavior
//! - **Llm**: Intelligent categorization using Claude API
//!
//! # Examples
//!
//! ```no_run
//! use mcp_codegen::skills::ManifestGenerator;
//! use mcp_introspector::ToolInfo;
//!
//! # fn example(tools: &[ToolInfo]) -> Result<(), mcp_core::Error> {
//! // Universal: Works for ANY MCP server
//! let generator = ManifestGenerator::auto().unwrap()?;
//! let manifest = generator.generate(tools)?;
//!
//! println!("Generated {} categories", manifest.category_count());
//! # Ok(())
//! # }
//! ```

use super::{CategorizationDictionary, LlmCategorizer};
use mcp_core::{CategoryManifest, Result, SkillCategory};
use mcp_introspector::ToolInfo;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Categorization strategy for tool organization.
///
/// Controls how tools are grouped into categories for progressive loading.
#[derive(Debug, Clone)]
pub enum CategorizationStrategy {
    /// Automatic categorization using heuristic analysis.
    ///
    /// Analyzes tool names to extract verbs (CRUD patterns) and entities,
    /// then groups tools based on configurable preferences.
    Auto {
        /// Prefer verb-based (CRUD) or entity-based grouping
        prefer: GroupingPreference,
        /// Maximum tools per category before splitting
        max_per_category: usize,
        /// Minimum tools per category before merging to "other"
        min_per_category: usize,
    },
    /// Custom rules provided by user.
    ///
    /// Allows precise control over categorization for domain-specific needs.
    Custom {
        /// Category rules: (category, patterns to match)
        rules: Vec<(SkillCategory, Vec<String>)>,
        /// What to do with uncategorized tools
        fallback: FallbackStrategy,
    },
    /// LLM-based intelligent categorization using Claude.
    ///
    /// Uses Claude API to analyze tools and create semantic categories.
    /// Requires ANTHROPIC_API_KEY environment variable or explicit key.
    Llm {
        /// Model to use (e.g., "claude-sonnet-4")
        model: String,
        /// Maximum categories to create
        max_categories: usize,
        /// API key (from env or config)
        api_key: Option<String>,
    },
}

/// Preference for grouping tools in automatic categorization.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GroupingPreference {
    /// Group by CRUD verbs (create, read, update, delete, search)
    Verbs,
    /// Group by entities extracted from tool names (users, files, messages)
    Entities,
    /// Use both verb and entity (e.g., "create-users", "read-files")
    Hybrid,
}

/// Strategy for handling tools that don't match any category.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FallbackStrategy {
    /// Put uncategorized tools in "other" category
    Other,
    /// Apply auto-categorization heuristics
    AutoCategorize,
    /// Create individual categories for each tool
    Individual,
}

/// Universal manifest generator.
///
/// Analyzes tool names from any MCP server and automatically assigns them
/// to categories using heuristic patterns. No hardcoded domain knowledge.
///
/// # Examples
///
/// ```no_run
/// use mcp_codegen::skills::{ManifestGenerator, GroupingPreference};
/// use mcp_introspector::ToolInfo;
///
/// # fn example(tools: &[ToolInfo]) -> Result<(), mcp_core::Error> {
/// // Simple: use defaults
/// let generator = ManifestGenerator::auto().unwrap()?;
/// let manifest = generator.generate(tools)?;
///
/// // Advanced: customize grouping
/// let generator = ManifestGenerator::auto_with_preference(
///     GroupingPreference::Entities,
///     15, // max per category
///     3,  // min per category
/// )?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct ManifestGenerator {
    strategy: CategorizationStrategy,
    dictionary: CategorizationDictionary,
}

impl ManifestGenerator {
    /// Creates generator with automatic categorization (recommended).
    ///
    /// Uses hybrid grouping (verbs + entities) with balanced category sizes.
    /// Works universally with any MCP server.
    ///
    /// # Errors
    ///
    /// Returns error if default dictionary cannot be loaded.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::skills::ManifestGenerator;
    ///
    /// let generator = ManifestGenerator::auto().unwrap()?;
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    pub fn auto() -> Result<Self> {
        Ok(Self {
            strategy: CategorizationStrategy::Auto {
                prefer: GroupingPreference::Hybrid,
                max_per_category: 12,
                min_per_category: 3,
            },
            dictionary: CategorizationDictionary::default_dictionary()?,
        })
    }

    /// Creates generator with specific auto-categorization preferences.
    ///
    /// # Arguments
    ///
    /// * `prefer` - Grouping strategy (verbs, entities, or hybrid)
    /// * `max_per_category` - Split categories exceeding this size
    /// * `min_per_category` - Merge categories smaller than this
    ///
    /// # Errors
    ///
    /// Returns error if default dictionary cannot be loaded.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::skills::{ManifestGenerator, GroupingPreference};
    ///
    /// // Prefer entity-based grouping with larger categories
    /// let generator = ManifestGenerator::auto_with_preference(
    ///     GroupingPreference::Entities,
    ///     15,
    ///     2,
    /// )?;
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    pub fn auto_with_preference(
        prefer: GroupingPreference,
        max_per_category: usize,
        min_per_category: usize,
    ) -> Result<Self> {
        Ok(Self {
            strategy: CategorizationStrategy::Auto {
                prefer,
                max_per_category,
                min_per_category,
            },
            dictionary: CategorizationDictionary::default_dictionary()?,
        })
    }

    /// Creates generator with custom categorization rules.
    ///
    /// # Arguments
    ///
    /// * `rules` - Category rules: (category, tool name patterns)
    /// * `fallback` - Strategy for tools not matching any rule
    ///
    /// # Errors
    ///
    /// Returns error if default dictionary cannot be loaded.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::{ManifestGenerator, FallbackStrategy};
    /// use mcp_core::SkillCategory;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let admin = SkillCategory::new("admin")?;
    /// let rules = vec![
    ///     (admin, vec!["admin".to_string(), "config".to_string()]),
    /// ];
    ///
    /// let generator = ManifestGenerator::with_custom_rules(
    ///     rules,
    ///     FallbackStrategy::AutoCategorize,
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn with_custom_rules(
        rules: Vec<(SkillCategory, Vec<String>)>,
        fallback: FallbackStrategy,
    ) -> Result<Self> {
        Ok(Self {
            strategy: CategorizationStrategy::Custom { rules, fallback },
            dictionary: CategorizationDictionary::default_dictionary()?,
        })
    }

    /// Creates generator with LLM-based categorization.
    ///
    /// Uses Claude API to intelligently categorize tools based on
    /// semantic analysis of tool names and descriptions.
    ///
    /// # Arguments
    ///
    /// * `model` - Claude model to use (e.g., "claude-sonnet-4")
    /// * `max_categories` - Maximum categories to create
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Default dictionary cannot be loaded
    /// - ANTHROPIC_API_KEY environment variable not set
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::ManifestGenerator;
    ///
    /// // Requires ANTHROPIC_API_KEY environment variable
    /// let generator = ManifestGenerator::with_llm("claude-sonnet-4", 10)?;
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    pub fn with_llm(model: &str, max_categories: usize) -> Result<Self> {
        let api_key = std::env::var("ANTHROPIC_API_KEY").ok();
        Ok(Self {
            strategy: CategorizationStrategy::Llm {
                model: model.to_string(),
                max_categories,
                api_key,
            },
            dictionary: CategorizationDictionary::default_dictionary()?,
        })
    }

    /// Creates generator with LLM using explicit API key.
    ///
    /// # Arguments
    ///
    /// * `model` - Claude model to use
    /// * `max_categories` - Maximum categories to create
    /// * `api_key` - Anthropic API key
    ///
    /// # Errors
    ///
    /// Returns error if default dictionary cannot be loaded.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::ManifestGenerator;
    ///
    /// let generator = ManifestGenerator::with_llm_key(
    ///     "claude-sonnet-4",
    ///     10,
    ///     "sk-ant-api-key",
    /// )?;
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    pub fn with_llm_key(model: &str, max_categories: usize, api_key: &str) -> Result<Self> {
        Ok(Self {
            strategy: CategorizationStrategy::Llm {
                model: model.to_string(),
                max_categories,
                api_key: Some(api_key.to_string()),
            },
            dictionary: CategorizationDictionary::default_dictionary()?,
        })
    }

    /// Creates generator with custom dictionary file.
    ///
    /// Loads categorization patterns from a YAML file instead of
    /// using the default embedded dictionary.
    ///
    /// # Arguments
    ///
    /// * `dictionary_path` - Path to custom dictionary YAML file
    ///
    /// # Errors
    ///
    /// Returns error if dictionary file cannot be loaded or parsed.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::ManifestGenerator;
    ///
    /// let generator = ManifestGenerator::with_dictionary("./custom_rules.yaml")?;
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    pub fn with_dictionary<P: AsRef<Path>>(dictionary_path: P) -> Result<Self> {
        Ok(Self {
            strategy: CategorizationStrategy::Auto {
                prefer: GroupingPreference::Hybrid,
                max_per_category: 12,
                min_per_category: 3,
            },
            dictionary: CategorizationDictionary::from_file(dictionary_path)?,
        })
    }

    /// Generates category manifest from tool definitions.
    ///
    /// Analyzes tool names and assigns categories based on the configured
    /// strategy. Works with tools from any MCP server.
    ///
    /// Note: This method is synchronous. For LLM-based categorization,
    /// use `generate_async()` instead.
    ///
    /// # Arguments
    ///
    /// * `tools` - Tool definitions to categorize
    ///
    /// # Returns
    ///
    /// A `CategoryManifest` with all tools assigned to categories.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Category creation fails (invalid category name)
    /// - LLM strategy is used (requires async, use `generate_async`)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::ManifestGenerator;
    /// use mcp_introspector::ToolInfo;
    ///
    /// # fn example(tools: &[ToolInfo]) -> Result<(), mcp_core::Error> {
    /// let generator = ManifestGenerator::auto().unwrap()?;
    /// let manifest = generator.generate(tools)?;
    ///
    /// for (category, tool_names) in manifest.categories() {
    ///     println!("{}: {} tools", category.as_str(), tool_names.len());
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate(&self, tools: &[ToolInfo]) -> Result<CategoryManifest> {
        match &self.strategy {
            CategorizationStrategy::Auto {
                prefer,
                max_per_category,
                min_per_category,
            } => self.auto_categorize(tools, *prefer, *max_per_category, *min_per_category),
            CategorizationStrategy::Custom { rules, fallback } => {
                self.custom_categorize(tools, rules, *fallback)
            }
            CategorizationStrategy::Llm { .. } => Err(mcp_core::Error::ConfigError {
                message: "LLM categorization requires async. Use generate_async() instead"
                    .to_string(),
            }),
        }
    }

    /// Generates category manifest asynchronously.
    ///
    /// Supports all categorization strategies including LLM-based.
    /// Use this method when LLM categorization is required.
    ///
    /// # Arguments
    ///
    /// * `tools` - Tool definitions to categorize
    ///
    /// # Returns
    ///
    /// A `CategoryManifest` with all tools assigned to categories.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Category creation fails
    /// - LLM API call fails
    /// - API key is missing (for LLM strategy)
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::ManifestGenerator;
    /// use mcp_introspector::ToolInfo;
    ///
    /// # async fn example(tools: Vec<ToolInfo>) -> Result<(), mcp_core::Error> {
    /// // LLM-based categorization
    /// let generator = ManifestGenerator::with_llm("claude-sonnet-4", 10)?;
    /// let manifest = generator.generate_async(&tools).await?;
    ///
    /// println!("Created {} categories", manifest.category_count());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn generate_async(&self, tools: &[ToolInfo]) -> Result<CategoryManifest> {
        match &self.strategy {
            CategorizationStrategy::Llm {
                model,
                max_categories,
                api_key,
            } => {
                let api_key = api_key
                    .clone()
                    .or_else(|| std::env::var("ANTHROPIC_API_KEY").ok())
                    .ok_or_else(|| mcp_core::Error::ConfigError {
                        message: "ANTHROPIC_API_KEY not found in config or environment".to_string(),
                    })?;

                let categorizer = LlmCategorizer::new(model.clone(), api_key, *max_categories);

                categorizer.categorize(tools).await
            }
            // Fall back to sync methods for non-LLM strategies
            _ => self.generate(tools),
        }
    }

    /// Automatic categorization using heuristic analysis.
    ///
    /// Step 1: Extract verbs and entities from tool names
    /// Step 2: Group based on preference (verbs/entities/hybrid)
    /// Step 3: Balance category sizes (split large, merge small)
    /// Step 4: Build manifest
    fn auto_categorize(
        &self,
        tools: &[ToolInfo],
        prefer: GroupingPreference,
        max_per_category: usize,
        min_per_category: usize,
    ) -> Result<CategoryManifest> {
        // Step 1: Analyze each tool
        let analyzed: Vec<_> = tools
            .iter()
            .map(|tool| {
                let name = tool.name.as_str();
                let verb = self.extract_verb(name);
                let entity = self.extract_entity(name);
                (tool, verb, entity)
            })
            .collect();

        // Step 2: Group based on preference
        let groups = match prefer {
            GroupingPreference::Verbs => self.group_by_verb(&analyzed),
            GroupingPreference::Entities => self.group_by_entity(&analyzed),
            GroupingPreference::Hybrid => self.group_hybrid(&analyzed),
        };

        // Step 3: Balance category sizes
        let balanced = self.balance_categories(&groups, max_per_category, min_per_category)?;

        // Step 4: Build manifest
        self.build_manifest(balanced)
    }

    /// Custom categorization with user-provided rules.
    fn custom_categorize(
        &self,
        tools: &[ToolInfo],
        rules: &[(SkillCategory, Vec<String>)],
        fallback: FallbackStrategy,
    ) -> Result<CategoryManifest> {
        let mut categorized = HashSet::new();
        let mut builder = CategoryManifest::builder();

        // Apply custom rules
        for (category, patterns) in rules {
            for tool in tools {
                let tool_name = tool.name.as_str();
                if patterns.iter().any(|p| tool_name.contains(p)) {
                    builder = builder.add_tool(tool_name, category)?;
                    categorized.insert(tool_name);
                }
            }
        }

        // Handle uncategorized tools based on fallback strategy
        let uncategorized: Vec<_> = tools
            .iter()
            .filter(|t| !categorized.contains(t.name.as_str()))
            .collect();

        match fallback {
            FallbackStrategy::Other => {
                let other = SkillCategory::new("other")?;
                for tool in uncategorized {
                    builder = builder.add_tool(tool.name.as_str(), &other)?;
                }
            }
            FallbackStrategy::AutoCategorize => {
                // Use auto-categorization for remaining tools
                let auto_manifest = self.auto_categorize(
                    &uncategorized.into_iter().cloned().collect::<Vec<_>>(),
                    GroupingPreference::Hybrid,
                    12,
                    3,
                )?;
                // Merge auto-categorized tools into builder
                for (category, tool_names) in auto_manifest.categories() {
                    for tool_name in tool_names {
                        builder = builder.add_tool(tool_name.as_str(), category)?;
                    }
                }
            }
            FallbackStrategy::Individual => {
                for tool in uncategorized {
                    let category = SkillCategory::new(tool.name.as_str())?;
                    builder = builder.add_tool(tool.name.as_str(), &category)?;
                }
            }
        }

        Ok(builder.build())
    }

    /// Extract verb from tool name using dictionary patterns.
    ///
    /// Uses the loaded dictionary to identify CRUD operation verbs.
    fn extract_verb(&self, tool_name: &str) -> Option<&str> {
        self.dictionary.find_verb(tool_name)
    }

    /// Extract entity from tool name using dictionary patterns.
    ///
    /// Uses the loaded dictionary to identify entities and normalize them.
    fn extract_entity(&self, tool_name: &str) -> Option<String> {
        self.dictionary.find_entity(tool_name).map(String::from)
    }

    /// Group tools by verb (CRUD operations).
    fn group_by_verb<'a>(
        &self,
        analyzed: &[(&'a ToolInfo, Option<&str>, Option<String>)],
    ) -> HashMap<String, Vec<&'a ToolInfo>> {
        let mut groups: HashMap<String, Vec<&'a ToolInfo>> = HashMap::new();

        for (tool, verb, _entity) in analyzed {
            let category = verb.unwrap_or("other").to_string();
            groups.entry(category).or_default().push(tool);
        }

        groups
    }

    /// Group tools by entity.
    fn group_by_entity<'a>(
        &self,
        analyzed: &[(&'a ToolInfo, Option<&str>, Option<String>)],
    ) -> HashMap<String, Vec<&'a ToolInfo>> {
        let mut groups: HashMap<String, Vec<&'a ToolInfo>> = HashMap::new();

        for (tool, _verb, entity) in analyzed {
            let category = entity.clone().unwrap_or_else(|| "other".to_string());
            groups.entry(category).or_default().push(tool);
        }

        groups
    }

    /// Group tools by hybrid (verb-entity pairs).
    fn group_hybrid<'a>(
        &self,
        analyzed: &[(&'a ToolInfo, Option<&str>, Option<String>)],
    ) -> HashMap<String, Vec<&'a ToolInfo>> {
        let mut groups: HashMap<String, Vec<&'a ToolInfo>> = HashMap::new();

        for (tool, verb, entity) in analyzed {
            let category = match (verb, entity) {
                (Some(v), Some(e)) => format!("{v}_{e}"),
                (Some(v), None) => (*v).to_string(),
                (None, Some(e)) => e.clone(),
                (None, None) => "other".to_string(),
            };
            groups.entry(category).or_default().push(tool);
        }

        groups
    }

    /// Balance category sizes by splitting large and merging small categories.
    fn balance_categories<'a>(
        &self,
        groups: &HashMap<String, Vec<&'a ToolInfo>>,
        max_per_category: usize,
        min_per_category: usize,
    ) -> Result<HashMap<String, Vec<&'a ToolInfo>>> {
        let mut balanced: HashMap<String, Vec<&'a ToolInfo>> = HashMap::new();

        // Split large categories
        for (category, tools) in groups {
            if tools.len() > max_per_category {
                // Split by first letter of tool name
                for tool in tools {
                    let first_char = tool
                        .name
                        .as_str()
                        .chars()
                        .next()
                        .unwrap_or('a')
                        .to_lowercase()
                        .next()
                        .unwrap_or('a');
                    let subcategory = format!("{category}_{first_char}");
                    balanced.entry(subcategory).or_default().push(tool);
                }
            } else {
                balanced.entry(category.clone()).or_default().extend(tools);
            }
        }

        // Merge small categories into "other"
        let small_categories: Vec<_> = balanced
            .iter()
            .filter(|(_, tools)| tools.len() < min_per_category)
            .map(|(cat, _)| cat.clone())
            .collect();

        if !small_categories.is_empty() {
            let mut other_tools = Vec::new();
            for category in small_categories {
                if let Some(tools) = balanced.remove(&category) {
                    other_tools.extend(tools);
                }
            }
            if !other_tools.is_empty() {
                balanced.insert("other".to_string(), other_tools);
            }
        }

        Ok(balanced)
    }

    /// Build manifest from grouped tools.
    fn build_manifest(&self, groups: HashMap<String, Vec<&ToolInfo>>) -> Result<CategoryManifest> {
        let mut builder = CategoryManifest::builder();

        for (category_name, tools) in groups {
            let category = SkillCategory::new(&category_name)?;
            for tool in tools {
                builder = builder.add_tool(tool.name.as_str(), &category)?;
            }
        }

        Ok(builder.build())
    }

    /// Returns dynamic descriptions for each category in the manifest.
    ///
    /// Generates human-readable descriptions based on actual tool names
    /// and categorization patterns. Works with any MCP server.
    ///
    /// # Arguments
    ///
    /// * `manifest` - The generated manifest to describe
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::ManifestGenerator;
    /// use mcp_introspector::ToolInfo;
    ///
    /// # fn example(tools: &[ToolInfo]) -> Result<(), mcp_core::Error> {
    /// let generator = ManifestGenerator::auto().unwrap();
    /// let manifest = generator.generate(tools)?;
    /// let descriptions = generator.category_descriptions(&manifest);
    ///
    /// for (category, description) in descriptions {
    ///     println!("{}: {}", category, description);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn category_descriptions(&self, manifest: &CategoryManifest) -> HashMap<String, String> {
        let mut descriptions = HashMap::new();

        for (category, tool_names) in manifest.categories() {
            let description = self.generate_category_description(category.as_str(), tool_names);
            descriptions.insert(category.as_str().to_string(), description);
        }

        descriptions
    }

    /// Generate description for a category based on its tools.
    fn generate_category_description(&self, category: &str, tools: &[String]) -> String {
        // Standard descriptions for known CRUD categories
        match category {
            "create" => "Creation operations (add, create, insert new items)".to_string(),
            "read" => "Read operations (get, list, fetch, retrieve information)".to_string(),
            "update" => "Update operations (edit, modify, change existing items)".to_string(),
            "delete" => "Delete operations (remove, destroy, drop items)".to_string(),
            "search" => "Search operations (find, query, lookup items)".to_string(),
            "other" => "Other operations not fitting standard categories".to_string(),
            _ => {
                // Generate description from tool names
                let tool_count = tools.len();
                let sample_tools: Vec<_> = tools.iter().take(3).map(String::as_str).collect();
                format!(
                    "{} operations ({} tools: {}{})",
                    category,
                    tool_count,
                    sample_tools.join(", "),
                    if tool_count > 3 { ", ..." } else { "" }
                )
            }
        }
    }
}

impl Default for ManifestGenerator {
    fn default() -> Self {
        Self::auto().expect("Default dictionary should always load successfully")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::ToolName;
    use serde_json::json;

    fn create_test_tool(name: &str) -> ToolInfo {
        ToolInfo {
            name: ToolName::new(name),
            description: format!("Test tool: {name}"),
            input_schema: json!({"type": "object", "properties": {}}),
            output_schema: None,
        }
    }

    #[test]
    fn test_auto_generator_creation() {
        let generator = ManifestGenerator::auto().unwrap();
        match generator.strategy {
            CategorizationStrategy::Auto {
                prefer,
                max_per_category,
                min_per_category,
            } => {
                assert_eq!(prefer, GroupingPreference::Hybrid);
                assert_eq!(max_per_category, 12);
                assert_eq!(min_per_category, 3);
            }
            _ => panic!("Expected Auto strategy"),
        }
    }

    #[test]
    fn test_custom_generator_creation() {
        let custom = SkillCategory::new("custom").unwrap();
        let rules = vec![(custom, vec!["test".to_string()])];
        let generator =
            ManifestGenerator::with_custom_rules(rules, FallbackStrategy::AutoCategorize).unwrap();

        match generator.strategy {
            CategorizationStrategy::Custom { rules, fallback } => {
                assert_eq!(rules.len(), 1);
                assert_eq!(fallback, FallbackStrategy::AutoCategorize);
            }
            _ => panic!("Expected Custom strategy"),
        }
    }

    #[test]
    fn test_llm_generator_creation() {
        let generator = ManifestGenerator::with_llm("claude-sonnet-4", 10).unwrap();
        match generator.strategy {
            CategorizationStrategy::Llm {
                model,
                max_categories,
                ..
            } => {
                assert_eq!(model, "claude-sonnet-4");
                assert_eq!(max_categories, 10);
            }
            _ => panic!("Expected Llm strategy"),
        }
    }

    #[test]
    fn test_dictionary_generator_creation() {
        let generator = ManifestGenerator::auto().unwrap();
        // Should have loaded dictionary
        assert_eq!(generator.dictionary.version, "1.0");
    }

    #[test]
    fn test_extract_verb_create() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(generator.extract_verb("create_user"), Some("create"));
        assert_eq!(generator.extract_verb("add_item"), Some("create"));
        assert_eq!(generator.extract_verb("new_record"), Some("create"));
        assert_eq!(generator.extract_verb("insert_row"), Some("create"));
    }

    #[test]
    fn test_extract_verb_read() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(generator.extract_verb("get_user"), Some("read"));
        assert_eq!(generator.extract_verb("list_items"), Some("read"));
        assert_eq!(generator.extract_verb("fetch_data"), Some("read"));
        assert_eq!(generator.extract_verb("show_details"), Some("read"));
    }

    #[test]
    fn test_extract_verb_update() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(generator.extract_verb("update_user"), Some("update"));
        assert_eq!(generator.extract_verb("edit_item"), Some("update"));
        assert_eq!(generator.extract_verb("modify_record"), Some("update"));
        assert_eq!(generator.extract_verb("set_value"), Some("update"));
    }

    #[test]
    fn test_extract_verb_delete() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(generator.extract_verb("delete_user"), Some("delete"));
        assert_eq!(generator.extract_verb("remove_item"), Some("delete"));
        assert_eq!(generator.extract_verb("destroy_record"), Some("delete"));
    }

    #[test]
    fn test_extract_verb_search() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(generator.extract_verb("search_users"), Some("search"));
        assert_eq!(generator.extract_verb("find_items"), Some("search"));
        assert_eq!(generator.extract_verb("query_data"), Some("search"));
    }

    #[test]
    fn test_extract_verb_none() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(generator.extract_verb("unknown_operation"), None);
    }

    #[test]
    fn test_extract_entity_users() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(
            generator.extract_entity("create_user"),
            Some("users".to_string())
        );
        assert_eq!(
            generator.extract_entity("get_member"),
            Some("users".to_string())
        );
        assert_eq!(
            generator.extract_entity("update_account"),
            Some("users".to_string())
        );
    }

    #[test]
    fn test_extract_entity_files() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(
            generator.extract_entity("read_file"),
            Some("files".to_string())
        );
        assert_eq!(
            generator.extract_entity("write_document"),
            Some("files".to_string())
        );
    }

    #[test]
    fn test_extract_entity_messages() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(
            generator.extract_entity("send_message"),
            Some("messages".to_string())
        );
        assert_eq!(
            generator.extract_entity("add_comment"),
            Some("messages".to_string())
        );
    }

    #[test]
    fn test_extract_entity_repositories() {
        let generator = ManifestGenerator::auto().unwrap();
        assert_eq!(
            generator.extract_entity("create_repository"),
            Some("repositories".to_string())
        );
        assert_eq!(
            generator.extract_entity("fork_repo"),
            Some("repositories".to_string())
        );
    }

    #[test]
    fn test_slack_tools_categorization() {
        // Use lower min_per_category to avoid merging into "other"
        let generator = ManifestGenerator::auto_with_preference(
            GroupingPreference::Hybrid,
            12,
            1, // Allow small categories
        );
        let tools = vec![
            create_test_tool("send_message"),
            create_test_tool("get_channels"),
            create_test_tool("create_channel"),
            create_test_tool("list_users"),
        ];

        let manifest = generator.unwrap().generate(&tools).unwrap();

        // Should create sensible categories (at least 2 with hybrid grouping)
        assert!(manifest.category_count() >= 2);
        assert!(manifest.category_count() <= 4);
        assert_eq!(manifest.tool_count(), 4);
    }

    #[test]
    fn test_filesystem_tools_categorization() {
        let generator = ManifestGenerator::auto().unwrap();
        let tools = vec![
            create_test_tool("read_file"),
            create_test_tool("write_file"),
            create_test_tool("delete_file"),
            create_test_tool("list_directory"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        // Should group by CRUD verbs or file operations
        assert!(manifest.category_count() >= 1);
        assert_eq!(manifest.tool_count(), 4);
    }

    #[test]
    fn test_database_tools_categorization() {
        let generator = ManifestGenerator::auto().unwrap();
        let tools = vec![
            create_test_tool("query_table"),
            create_test_tool("insert_row"),
            create_test_tool("update_record"),
            create_test_tool("delete_record"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        // Should work even without knowing database domain
        assert_eq!(manifest.tool_count(), 4);
        assert!(manifest.category_count() >= 1);
    }

    #[test]
    fn test_jira_tools_categorization() {
        let generator = ManifestGenerator::auto().unwrap();
        let tools = vec![
            create_test_tool("create_issue"),
            create_test_tool("update_issue"),
            create_test_tool("get_issue"),
            create_test_tool("search_issues"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        assert_eq!(manifest.tool_count(), 4);
        assert!(manifest.category_count() >= 1);
    }

    #[test]
    fn test_github_tools_categorization() {
        let generator = ManifestGenerator::auto().unwrap();
        let tools = vec![
            create_test_tool("create_pull_request"),
            create_test_tool("get_user"),
            create_test_tool("list_branches"),
            create_test_tool("search_code"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        assert_eq!(manifest.tool_count(), 4);
        assert!(manifest.category_count() >= 1);
    }

    #[test]
    fn test_grouping_preference_verbs() {
        let generator =
            ManifestGenerator::auto_with_preference(GroupingPreference::Verbs, 20, 2).unwrap();
        let tools = vec![
            create_test_tool("create_user"),
            create_test_tool("create_file"),
            create_test_tool("get_user"),
            create_test_tool("get_file"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        // Should group by verbs (create, read)
        assert!(manifest.category_count() >= 2);
    }

    #[test]
    fn test_grouping_preference_entities() {
        let generator =
            ManifestGenerator::auto_with_preference(GroupingPreference::Entities, 20, 2).unwrap();
        let tools = vec![
            create_test_tool("create_user"),
            create_test_tool("get_user"),
            create_test_tool("create_file"),
            create_test_tool("get_file"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        // Should group by entities (users, files)
        assert!(manifest.category_count() >= 2);
    }

    #[test]
    fn test_custom_rules_with_fallback_other() {
        let admin = SkillCategory::new("admin").unwrap();
        let rules = vec![(admin.clone(), vec!["admin".to_string()])];
        let generator =
            ManifestGenerator::with_custom_rules(rules, FallbackStrategy::Other).unwrap();

        let tools = vec![
            create_test_tool("admin_config"),
            create_test_tool("get_user"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        let found = manifest.find_category("admin_config");
        assert_eq!(found.unwrap(), &admin);

        let other = SkillCategory::new("other").unwrap();
        let found = manifest.find_category("get_user");
        assert_eq!(found.unwrap(), &other);
    }

    #[test]
    fn test_custom_rules_with_fallback_auto() {
        let admin = SkillCategory::new("admin").unwrap();
        let rules = vec![(admin.clone(), vec!["admin".to_string()])];
        let generator =
            ManifestGenerator::with_custom_rules(rules, FallbackStrategy::AutoCategorize).unwrap();

        let tools = vec![
            create_test_tool("admin_config"),
            create_test_tool("create_user"),
            create_test_tool("get_file"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        let found = manifest.find_category("admin_config");
        assert_eq!(found.unwrap(), &admin);

        // Other tools should be auto-categorized
        assert!(manifest.find_category("create_user").is_some());
        assert!(manifest.find_category("get_file").is_some());
    }

    #[test]
    fn test_custom_rules_with_fallback_individual() {
        let admin = SkillCategory::new("admin").unwrap();
        let rules = vec![(admin.clone(), vec!["admin".to_string()])];
        let generator =
            ManifestGenerator::with_custom_rules(rules, FallbackStrategy::Individual).unwrap();

        let tools = vec![
            create_test_tool("admin_config"),
            create_test_tool("unique_tool"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        let found = manifest.find_category("admin_config");
        assert_eq!(found.unwrap(), &admin);

        // Uncategorized tool should have its own category
        let unique = SkillCategory::new("unique_tool").unwrap();
        let found = manifest.find_category("unique_tool");
        assert_eq!(found.unwrap(), &unique);
    }

    #[test]
    fn test_empty_tools() {
        let generator = ManifestGenerator::auto().unwrap();
        let tools: Vec<ToolInfo> = vec![];

        let manifest = generator.generate(&tools).unwrap();
        assert_eq!(manifest.tool_count(), 0);
        assert_eq!(manifest.category_count(), 0);
    }

    #[test]
    fn test_category_descriptions_dynamic() {
        let generator = ManifestGenerator::auto().unwrap();
        let tools = vec![
            create_test_tool("create_user"),
            create_test_tool("get_file"),
        ];

        let manifest = generator.generate(&tools).unwrap();
        let descriptions = generator.category_descriptions(&manifest);

        // Should have descriptions for generated categories
        assert!(!descriptions.is_empty());
        for (category, description) in descriptions {
            assert!(!description.is_empty());
            println!("Category '{}': {}", category, description);
        }
    }

    #[test]
    fn test_case_insensitive_matching() {
        let generator = ManifestGenerator::auto().unwrap();
        let tools = vec![
            create_test_tool("CREATE_USER"),
            create_test_tool("Get_File"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        // Should work with any case
        assert!(manifest.find_category("CREATE_USER").is_some());
        assert!(manifest.find_category("Get_File").is_some());
    }

    #[test]
    fn test_mixed_domain_tools() {
        let generator = ManifestGenerator::auto().unwrap();
        let tools = vec![
            // GitHub-like
            create_test_tool("create_pull_request"),
            // Slack-like
            create_test_tool("send_message"),
            // Database-like
            create_test_tool("query_table"),
            // Filesystem-like
            create_test_tool("read_file"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        // Should categorize mixed domains without hardcoded knowledge
        assert_eq!(manifest.tool_count(), 4);
        assert!(manifest.category_count() >= 1);
    }

    #[test]
    fn test_balance_large_categories() {
        let generator =
            ManifestGenerator::auto_with_preference(GroupingPreference::Verbs, 3, 1).unwrap();

        // Create tools with different verbs to demonstrate categorization
        let tools = vec![
            create_test_tool("create_user"),
            create_test_tool("create_file"),
            create_test_tool("create_task"),
            create_test_tool("create_project"),
            create_test_tool("get_user"),
            create_test_tool("get_file"),
            create_test_tool("update_user"),
            create_test_tool("delete_user"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        // Should create multiple categories by verb (create, read, update, delete)
        assert!(manifest.category_count() >= 2);
        assert_eq!(manifest.tool_count(), 8);
    }

    #[test]
    fn test_merge_small_categories() {
        let generator =
            ManifestGenerator::auto_with_preference(GroupingPreference::Entities, 20, 3).unwrap();

        let tools = vec![
            create_test_tool("create_user"),
            create_test_tool("get_file"),
            create_test_tool("send_message"),
            create_test_tool("query_table"),
            create_test_tool("read_document"),
        ];

        let manifest = generator.generate(&tools).unwrap();

        // Small categories should be merged
        assert_eq!(manifest.tool_count(), 5);
    }

    #[test]
    fn test_default_trait() {
        let generator = ManifestGenerator::default();
        match generator.strategy {
            CategorizationStrategy::Auto { .. } => {}
            _ => panic!("Default should use Auto strategy"),
        }
    }
}
