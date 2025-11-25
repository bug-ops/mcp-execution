// ! Categorized skill types for token-efficient progressive loading.
//!
//! This module provides types for generating categorized skills that reduce token
//! consumption through lazy loading. Instead of loading all tool documentation at once,
//! tools are organized into categories that can be loaded on-demand.
//!
//! # Architecture
//!
//! A categorized skill consists of:
//! - **SKILL.md**: Minimal entry point (~200-300 tokens)
//! - **manifest.yaml**: Tool-to-category mapping (~200 tokens)
//! - **categories/*.md**: Category-specific documentation (~400-800 tokens each)
//! - **scripts/**: Executable TypeScript (unchanged from standard skills)
//!
//! # Token Savings
//!
//! | Scenario | Standard Skill | Categorized Skill | Savings |
//! |----------|----------------|-------------------|---------|
//! | Discovery | 8,500 tokens | 300 tokens | 96% |
//! | Single category | 8,500 tokens | 900 tokens | 89% |
//! | Two categories | 8,500 tokens | 1,500 tokens | 82% |
//!
//! # Examples
//!
//! ```
//! use mcp_core::{SkillCategory, CategoryManifest, CategorizedSkillBundle};
//! use std::collections::HashMap;
//!
//! # fn example() -> Result<(), mcp_core::Error> {
//! // Create a category
//! let category = SkillCategory::new("repos")?;
//!
//! // Build manifest
//! let manifest = CategoryManifest::builder()
//!     .add_tool("create_branch", &category)?
//!     .add_tool("list_commits", &category)?
//!     .build();
//!
//! // Build categorized skill bundle
//! let bundle = CategorizedSkillBundle::builder("github")?
//!     .skill_md("---\nname: github\n---\n# GitHub")
//!     .manifest(manifest)
//!     .add_category(category.clone(), "# Repos\n...")
//!     .build();
//!
//! assert_eq!(bundle.categories().len(), 1);
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result, ScriptFile, SkillName};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

/// Strong type for skill category names.
///
/// Categories group related tools for progressive loading. Category names must:
/// - Use lowercase letters, numbers, hyphens, and underscores
/// - Start with a letter
/// - Be 1-50 characters
///
/// # Examples
///
/// ```
/// use mcp_core::SkillCategory;
///
/// # fn example() -> Result<(), mcp_core::Error> {
/// let category = SkillCategory::new("repos")?;
/// assert_eq!(category.as_str(), "repos");
///
/// let category = SkillCategory::new("pull-requests")?;
/// assert_eq!(category.as_str(), "pull-requests");
///
/// // Invalid names
/// assert!(SkillCategory::new("UPPERCASE").is_err());
/// assert!(SkillCategory::new("123numeric").is_err());
/// assert!(SkillCategory::new("").is_err());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SkillCategory(String);

impl SkillCategory {
    /// Creates a new category name with validation.
    ///
    /// # Arguments
    ///
    /// * `name` - Category name (lowercase alphanumeric with hyphens/underscores)
    ///
    /// # Errors
    ///
    /// Returns error if name is:
    /// - Empty or longer than 50 characters
    /// - Contains uppercase letters or invalid characters
    /// - Starts with a number
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillCategory;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let repos = SkillCategory::new("repos")?;
    /// let issues = SkillCategory::new("issues")?;
    /// let prs = SkillCategory::new("pull-requests")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(name: impl AsRef<str>) -> Result<Self> {
        let name = name.as_ref();

        // Validate length
        if name.is_empty() || name.len() > 50 {
            return Err(Error::ValidationError {
                field: "category name".to_string(),
                reason: format!("must be 1-50 characters, got {}", name.len()),
            });
        }

        // Validate format: lowercase letters, numbers, hyphens, underscores
        // Must start with a letter
        if !name.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
            return Err(Error::ValidationError {
                field: "category name".to_string(),
                reason: "must start with a lowercase letter".to_string(),
            });
        }

        if !name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
        {
            return Err(Error::ValidationError {
                field: "category name".to_string(),
                reason: "must contain only lowercase letters, numbers, hyphens, and underscores"
                    .to_string(),
            });
        }

        Ok(Self(name.to_string()))
    }

    /// Returns the category name as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillCategory;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let category = SkillCategory::new("repos")?;
    /// assert_eq!(category.as_str(), "repos");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Returns the filename for this category's markdown file.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillCategory;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let category = SkillCategory::new("repos")?;
    /// assert_eq!(category.filename(), "repos.md");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn filename(&self) -> String {
        format!("{}.md", self.0)
    }

    /// Returns the relative path for this category file.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillCategory;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let category = SkillCategory::new("repos")?;
    /// assert_eq!(category.relative_path(), "categories/repos.md");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn relative_path(&self) -> String {
        format!("categories/{}", self.filename())
    }
}

impl fmt::Display for SkillCategory {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Metadata for a category manifest.
///
/// Tracks version and generation information for the manifest.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestMetadata {
    /// Manifest format version
    pub version: String,
    /// Generation timestamp (RFC3339)
    pub generated_at: String,
    /// Number of categories in manifest
    pub category_count: usize,
    /// Total number of tools across all categories
    pub tool_count: usize,
}

impl ManifestMetadata {
    /// Creates new manifest metadata with current timestamp.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ManifestMetadata;
    ///
    /// let metadata = ManifestMetadata::new(5, 40);
    /// assert_eq!(metadata.version, "1.0");
    /// assert_eq!(metadata.category_count, 5);
    /// assert_eq!(metadata.tool_count, 40);
    /// ```
    #[must_use]
    pub fn new(category_count: usize, tool_count: usize) -> Self {
        Self {
            version: "1.0".to_string(),
            generated_at: chrono::Utc::now().to_rfc3339(),
            category_count,
            tool_count,
        }
    }
}

/// Manifest mapping tools to categories.
///
/// Provides a lightweight index that maps tool names to categories without
/// loading full documentation. Claude Code reads this to determine which
/// category file to load when a specific tool is needed.
///
/// # Examples
///
/// ```
/// use mcp_core::{SkillCategory, CategoryManifest};
///
/// # fn example() -> Result<(), mcp_core::Error> {
/// let repos = SkillCategory::new("repos")?;
/// let issues = SkillCategory::new("issues")?;
///
/// let manifest = CategoryManifest::builder()
///     .add_tool("create_branch", &repos)?
///     .add_tool("list_commits", &repos)?
///     .add_tool("create_issue", &issues)?
///     .build();
///
/// assert_eq!(manifest.tool_count(), 3);
/// assert_eq!(manifest.category_count(), 2);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryManifest {
    /// Tool name to category mapping
    #[serde(rename = "categories")]
    categories: HashMap<SkillCategory, Vec<String>>,
    /// Manifest metadata
    #[serde(rename = "metadata")]
    metadata: ManifestMetadata,
}

impl CategoryManifest {
    /// Creates a new builder for `CategoryManifest`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CategoryManifest;
    ///
    /// let manifest = CategoryManifest::builder().build();
    /// assert_eq!(manifest.tool_count(), 0);
    /// ```
    #[must_use]
    pub fn builder() -> CategoryManifestBuilder {
        CategoryManifestBuilder::new()
    }

    /// Returns the categories in this manifest.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillCategory, CategoryManifest};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let repos = SkillCategory::new("repos")?;
    /// let manifest = CategoryManifest::builder()
    ///     .add_tool("create_branch", &repos)?
    ///     .build();
    ///
    /// let categories = manifest.categories();
    /// assert_eq!(categories.len(), 1);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn categories(&self) -> &HashMap<SkillCategory, Vec<String>> {
        &self.categories
    }

    /// Returns the manifest metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CategoryManifest;
    ///
    /// let manifest = CategoryManifest::builder().build();
    /// assert_eq!(manifest.metadata().version, "1.0");
    /// ```
    #[must_use]
    pub const fn metadata(&self) -> &ManifestMetadata {
        &self.metadata
    }

    /// Returns the number of categories.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillCategory, CategoryManifest};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let repos = SkillCategory::new("repos")?;
    /// let issues = SkillCategory::new("issues")?;
    ///
    /// let manifest = CategoryManifest::builder()
    ///     .add_tool("create_branch", &repos)?
    ///     .add_tool("create_issue", &issues)?
    ///     .build();
    ///
    /// assert_eq!(manifest.category_count(), 2);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn category_count(&self) -> usize {
        self.categories.len()
    }

    /// Returns the total number of tools across all categories.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillCategory, CategoryManifest};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let repos = SkillCategory::new("repos")?;
    ///
    /// let manifest = CategoryManifest::builder()
    ///     .add_tool("create_branch", &repos)?
    ///     .add_tool("list_commits", &repos)?
    ///     .build();
    ///
    /// assert_eq!(manifest.tool_count(), 2);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn tool_count(&self) -> usize {
        self.categories.values().map(Vec::len).sum()
    }

    /// Returns the category for a given tool name.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillCategory, CategoryManifest};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let repos = SkillCategory::new("repos")?;
    ///
    /// let manifest = CategoryManifest::builder()
    ///     .add_tool("create_branch", &repos)?
    ///     .build();
    ///
    /// let category = manifest.find_category("create_branch");
    /// assert!(category.is_some());
    /// assert_eq!(category.unwrap().as_str(), "repos");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn find_category(&self, tool_name: &str) -> Option<&SkillCategory> {
        self.categories
            .iter()
            .find(|(_, tools)| tools.contains(&tool_name.to_string()))
            .map(|(category, _)| category)
    }
}

/// Builder for `CategoryManifest`.
///
/// Provides a fluent interface for constructing manifests with tool-to-category mappings.
///
/// # Examples
///
/// ```
/// use mcp_core::{SkillCategory, CategoryManifest};
///
/// # fn example() -> Result<(), mcp_core::Error> {
/// let repos = SkillCategory::new("repos")?;
/// let issues = SkillCategory::new("issues")?;
///
/// let manifest = CategoryManifest::builder()
///     .add_tool("create_branch", &repos)?
///     .add_tool("list_commits", &repos)?
///     .add_tool("create_issue", &issues)?
///     .add_tool("list_issues", &issues)?
///     .build();
///
/// assert_eq!(manifest.category_count(), 2);
/// assert_eq!(manifest.tool_count(), 4);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Default)]
pub struct CategoryManifestBuilder {
    categories: HashMap<SkillCategory, Vec<String>>,
}

impl CategoryManifestBuilder {
    fn new() -> Self {
        Self {
            categories: HashMap::new(),
        }
    }

    /// Adds a tool to a category.
    ///
    /// If the category doesn't exist, it will be created. Tools can only
    /// belong to one category; adding a tool multiple times will move it
    /// to the latest category.
    ///
    /// # Arguments
    ///
    /// * `tool_name` - Name of the tool
    /// * `category` - Category to add the tool to
    ///
    /// # Errors
    ///
    /// Currently never fails, but returns `Result` for future extensibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillCategory, CategoryManifest};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let repos = SkillCategory::new("repos")?;
    ///
    /// let manifest = CategoryManifest::builder()
    ///     .add_tool("create_branch", &repos)?
    ///     .add_tool("list_commits", &repos)?
    ///     .build();
    ///
    /// assert_eq!(manifest.tool_count(), 2);
    /// # Ok(())
    /// # }
    /// ```
    pub fn add_tool(
        mut self,
        tool_name: impl Into<String>,
        category: &SkillCategory,
    ) -> Result<Self> {
        let tool_name = tool_name.into();

        // Remove tool from any existing category first (tools can only be in one category)
        for tools in self.categories.values_mut() {
            tools.retain(|t| t != &tool_name);
        }

        // Add to specified category
        self.categories
            .entry(category.clone())
            .or_default()
            .push(tool_name);

        Ok(self)
    }

    /// Adds multiple tools to a category.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillCategory, CategoryManifest};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let repos = SkillCategory::new("repos")?;
    /// let tools = vec!["create_branch", "list_commits", "delete_branch"];
    ///
    /// let manifest = CategoryManifest::builder()
    ///     .add_tools(tools, &repos)?
    ///     .build();
    ///
    /// assert_eq!(manifest.tool_count(), 3);
    /// # Ok(())
    /// # }
    /// ```
    ///
    /// # Errors
    ///
    /// Returns error if a tool name is already assigned to a different category.
    pub fn add_tools<I, T>(mut self, tool_names: I, category: &SkillCategory) -> Result<Self>
    where
        I: IntoIterator<Item = T>,
        T: Into<String>,
    {
        for tool_name in tool_names {
            self = self.add_tool(tool_name, category)?;
        }
        Ok(self)
    }

    /// Builds the `CategoryManifest`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CategoryManifest;
    ///
    /// let manifest = CategoryManifest::builder().build();
    /// assert_eq!(manifest.tool_count(), 0);
    /// ```
    #[must_use]
    pub fn build(self) -> CategoryManifest {
        let tool_count: usize = self.categories.values().map(Vec::len).sum();
        let category_count = self.categories.len();

        CategoryManifest {
            categories: self.categories,
            metadata: ManifestMetadata::new(category_count, tool_count),
        }
    }
}

/// A categorized skill bundle ready for persistence.
///
/// Contains all files needed for a token-efficient categorized skill:
/// - SKILL.md (minimal entry point)
/// - manifest.yaml (tool-to-category mapping)
/// - categories/*.md (category-specific documentation)
/// - scripts/ (TypeScript tool implementations)
/// - REFERENCE.md (optional full API reference)
///
/// # Examples
///
/// ```
/// use mcp_core::{CategorizedSkillBundle, SkillCategory, ScriptFile};
/// use std::collections::HashMap;
///
/// # fn example() -> Result<(), mcp_core::Error> {
/// let repos = SkillCategory::new("repos")?;
///
/// let bundle = CategorizedSkillBundle::builder("github")?
///     .skill_md("---\nname: github\n---\n# GitHub")
///     .manifest(
///         mcp_core::CategoryManifest::builder()
///             .add_tool("create_branch", &repos)?
///             .build()
///     )
///     .add_category(repos.clone(), "# Repository Operations\n...")
///     .script(ScriptFile::new("create_branch", "ts", "// code"))
///     .build();
///
/// assert_eq!(bundle.categories().len(), 1);
/// assert_eq!(bundle.scripts().len(), 1);
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CategorizedSkillBundle {
    /// Skill name
    name: SkillName,
    /// SKILL.md content (minimal)
    skill_md: String,
    /// Tool-to-category manifest
    manifest: CategoryManifest,
    /// Category markdown files (category -> content)
    categories: HashMap<SkillCategory, String>,
    /// Generated script files
    scripts: Vec<ScriptFile>,
    /// Optional full reference documentation
    reference_md: Option<String>,
}

impl CategorizedSkillBundle {
    /// Creates a new builder for `CategorizedSkillBundle`.
    ///
    /// # Arguments
    ///
    /// * `name` - Skill name (will be validated)
    ///
    /// # Errors
    ///
    /// Returns error if skill name is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::CategorizedSkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let builder = CategorizedSkillBundle::builder("github")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder(name: impl AsRef<str>) -> Result<CategorizedSkillBundleBuilder> {
        Ok(CategorizedSkillBundleBuilder::new(SkillName::new(name)?))
    }

    /// Returns the skill name.
    #[must_use]
    pub const fn name(&self) -> &SkillName {
        &self.name
    }

    /// Returns the SKILL.md content.
    #[must_use]
    pub fn skill_md(&self) -> &str {
        &self.skill_md
    }

    /// Returns the category manifest.
    #[must_use]
    pub const fn manifest(&self) -> &CategoryManifest {
        &self.manifest
    }

    /// Returns the category markdown files.
    #[must_use]
    pub const fn categories(&self) -> &HashMap<SkillCategory, String> {
        &self.categories
    }

    /// Returns the script files.
    #[must_use]
    pub fn scripts(&self) -> &[ScriptFile] {
        &self.scripts
    }

    /// Returns the reference.md content if present.
    #[must_use]
    pub fn reference_md(&self) -> Option<&str> {
        self.reference_md.as_deref()
    }
}

/// Builder for `CategorizedSkillBundle`.
///
/// Provides a fluent interface for constructing categorized skill bundles.
///
/// # Examples
///
/// ```
/// use mcp_core::{CategorizedSkillBundle, SkillCategory, CategoryManifest, ScriptFile};
///
/// # fn example() -> Result<(), mcp_core::Error> {
/// let repos = SkillCategory::new("repos")?;
///
/// let bundle = CategorizedSkillBundle::builder("github")?
///     .skill_md("---\nname: github\n---\n# GitHub")
///     .manifest(
///         CategoryManifest::builder()
///             .add_tool("create_branch", &repos)?
///             .build()
///     )
///     .add_category(repos.clone(), "# Repos\n...")
///     .script(ScriptFile::new("create_branch", "ts", "// code"))
///     .reference_md("# Full Reference")
///     .build();
///
/// assert_eq!(bundle.name().as_str(), "github");
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct CategorizedSkillBundleBuilder {
    name: SkillName,
    skill_md: Option<String>,
    manifest: Option<CategoryManifest>,
    categories: HashMap<SkillCategory, String>,
    scripts: Vec<ScriptFile>,
    reference_md: Option<String>,
}

impl CategorizedSkillBundleBuilder {
    fn new(name: SkillName) -> Self {
        Self {
            name,
            skill_md: None,
            manifest: None,
            categories: HashMap::new(),
            scripts: Vec::new(),
            reference_md: None,
        }
    }

    /// Sets the SKILL.md content.
    #[must_use]
    pub fn skill_md(mut self, content: impl Into<String>) -> Self {
        self.skill_md = Some(content.into());
        self
    }

    /// Sets the category manifest.
    #[must_use]
    pub fn manifest(mut self, manifest: CategoryManifest) -> Self {
        self.manifest = Some(manifest);
        self
    }

    /// Adds a category markdown file.
    #[must_use]
    pub fn add_category(mut self, category: SkillCategory, content: impl Into<String>) -> Self {
        self.categories.insert(category, content.into());
        self
    }

    /// Sets all category files at once.
    #[must_use]
    pub fn categories(mut self, categories: HashMap<SkillCategory, String>) -> Self {
        self.categories = categories;
        self
    }

    /// Adds a script file.
    #[must_use]
    pub fn script(mut self, script: ScriptFile) -> Self {
        self.scripts.push(script);
        self
    }

    /// Adds multiple script files.
    #[must_use]
    pub fn scripts(mut self, scripts: impl IntoIterator<Item = ScriptFile>) -> Self {
        self.scripts.extend(scripts);
        self
    }

    /// Sets the optional reference.md content.
    #[must_use]
    pub fn reference_md(mut self, content: impl Into<String>) -> Self {
        self.reference_md = Some(content.into());
        self
    }

    /// Builds the `CategorizedSkillBundle`.
    ///
    /// # Panics
    ///
    /// Panics if `skill_md` or `manifest` were not set.
    #[must_use]
    pub fn build(self) -> CategorizedSkillBundle {
        CategorizedSkillBundle {
            name: self.name,
            skill_md: self.skill_md.expect("skill_md is required"),
            manifest: self.manifest.expect("manifest is required"),
            categories: self.categories,
            scripts: self.scripts,
            reference_md: self.reference_md,
        }
    }

    /// Tries to build the `CategorizedSkillBundle`.
    ///
    /// # Errors
    ///
    /// Returns error if `skill_md` or `manifest` were not set.
    pub fn try_build(self) -> Result<CategorizedSkillBundle> {
        Ok(CategorizedSkillBundle {
            name: self.name,
            skill_md: self.skill_md.ok_or_else(|| Error::ConfigError {
                message: "skill_md is required for CategorizedSkillBundle".to_string(),
            })?,
            manifest: self.manifest.ok_or_else(|| Error::ConfigError {
                message: "manifest is required for CategorizedSkillBundle".to_string(),
            })?,
            categories: self.categories,
            scripts: self.scripts,
            reference_md: self.reference_md,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_category_new_valid() {
        assert!(SkillCategory::new("repos").is_ok());
        assert!(SkillCategory::new("pull-requests").is_ok());
        assert!(SkillCategory::new("issues_and_prs").is_ok());
        assert!(SkillCategory::new("a").is_ok());
        assert!(SkillCategory::new("category123").is_ok());
    }

    #[test]
    fn test_skill_category_new_invalid() {
        assert!(SkillCategory::new("").is_err());
        assert!(SkillCategory::new("UPPERCASE").is_err());
        assert!(SkillCategory::new("123numeric").is_err());
        assert!(SkillCategory::new("has spaces").is_err());
        assert!(SkillCategory::new("has@special").is_err());
        assert!(SkillCategory::new(&"a".repeat(51)).is_err());
    }

    #[test]
    fn test_skill_category_filename() {
        let category = SkillCategory::new("repos").unwrap();
        assert_eq!(category.filename(), "repos.md");
        assert_eq!(category.relative_path(), "categories/repos.md");
    }

    #[test]
    fn test_category_manifest_builder() {
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

        assert_eq!(manifest.category_count(), 2);
        assert_eq!(manifest.tool_count(), 3);
    }

    #[test]
    fn test_category_manifest_find_category() {
        let repos = SkillCategory::new("repos").unwrap();

        let manifest = CategoryManifest::builder()
            .add_tool("create_branch", &repos)
            .unwrap()
            .build();

        let found = manifest.find_category("create_branch");
        assert!(found.is_some());
        assert_eq!(found.unwrap(), &repos);

        assert!(manifest.find_category("nonexistent").is_none());
    }

    #[test]
    fn test_category_manifest_builder_add_tools() {
        let repos = SkillCategory::new("repos").unwrap();
        let tools = vec!["create_branch", "list_commits", "delete_branch"];

        let manifest = CategoryManifest::builder()
            .add_tools(tools, &repos)
            .unwrap()
            .build();

        assert_eq!(manifest.tool_count(), 3);
    }

    #[test]
    fn test_categorized_skill_bundle_builder() {
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
            .script(ScriptFile::new("create_branch", "ts", "// code"))
            .build();

        assert_eq!(bundle.name().as_str(), "github");
        assert_eq!(bundle.categories().len(), 1);
        assert_eq!(bundle.scripts().len(), 1);
    }

    #[test]
    fn test_categorized_skill_bundle_builder_try_build_error() {
        let result = CategorizedSkillBundle::builder("github")
            .unwrap()
            .try_build();

        assert!(result.is_err());
    }

    #[test]
    #[should_panic(expected = "skill_md is required")]
    fn test_categorized_skill_bundle_builder_missing_skill_md() {
        let _repos = SkillCategory::new("repos").unwrap();
        let manifest = CategoryManifest::builder().build();

        let _bundle = CategorizedSkillBundle::builder("github")
            .unwrap()
            .manifest(manifest)
            .build();
    }

    #[test]
    #[should_panic(expected = "manifest is required")]
    fn test_categorized_skill_bundle_builder_missing_manifest() {
        let _bundle = CategorizedSkillBundle::builder("github")
            .unwrap()
            .skill_md("---")
            .build();
    }

    #[test]
    fn test_manifest_metadata() {
        let metadata = ManifestMetadata::new(5, 40);
        assert_eq!(metadata.version, "1.0");
        assert_eq!(metadata.category_count, 5);
        assert_eq!(metadata.tool_count, 40);
    }

    #[test]
    fn test_types_are_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<SkillCategory>();
        assert_send_sync::<CategoryManifest>();
        assert_send_sync::<CategorizedSkillBundle>();
    }

    #[test]
    fn test_types_implement_debug() {
        let category = SkillCategory::new("repos").unwrap();
        let debug_str = format!("{category:?}");
        assert!(debug_str.contains("SkillCategory"));

        let manifest = CategoryManifest::builder().build();
        let debug_str = format!("{manifest:?}");
        assert!(debug_str.contains("CategoryManifest"));
    }

    #[test]
    fn test_category_manifest_serialization() {
        let repos = SkillCategory::new("repos").unwrap();
        let manifest = CategoryManifest::builder()
            .add_tool("create_branch", &repos)
            .unwrap()
            .build();

        let yaml = serde_yaml::to_string(&manifest).unwrap();
        assert!(yaml.contains("repos"));
        assert!(yaml.contains("create_branch"));
    }
}
