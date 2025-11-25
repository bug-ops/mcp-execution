//! Skill bundle types for multi-file Claude Code skills.
//!
//! This module provides types for generating and managing skill bundles that comply
//! with the Claude Code specification, where TypeScript scripts are separated from
//! SKILL.md into a `scripts/` subdirectory for progressive loading.
//!
//! # Structure
//!
//! A skill bundle represents a complete skill with multiple files:
//! ```text
//! skill-name/
//!   SKILL.md           # Documentation with script references
//!   scripts/
//!     tool1.ts         # Executable TypeScript
//!     tool2.ts
//!   reference.md       # Optional detailed documentation
//! ```
//!
//! # Examples
//!
//! ```
//! use mcp_core::{SkillBundle, SkillName, ScriptFile};
//!
//! # fn example() -> Result<(), mcp_core::Error> {
//! let bundle = SkillBundle::builder("github")?
//!     .skill_md("---\nname: github\n---\n# GitHub Skill")
//!     .script(ScriptFile::new("create_issue", "ts", "// TypeScript code"))
//!     .reference_md("# GitHub Reference\n...")
//!     .build();
//!
//! assert_eq!(bundle.name().as_str(), "github");
//! assert_eq!(bundle.scripts().len(), 1);
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result, SkillName};
use serde::{Deserialize, Serialize};

/// A reference to a script file within a skill.
///
/// Used in SKILL.md to reference executable scripts with relative paths.
/// All paths use forward slashes for cross-platform compatibility.
///
/// # Examples
///
/// ```
/// use mcp_core::ScriptReference;
///
/// let reference = ScriptReference::new("send_message", "ts");
/// assert_eq!(reference.relative_path(), "scripts/send_message.ts");
/// assert_eq!(reference.filename(), "send_message.ts");
/// assert_eq!(reference.tool_name(), "send_message");
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ScriptReference {
    /// Tool name (used as filename base)
    tool_name: String,
    /// File extension (e.g., "ts", "py")
    extension: String,
}

impl ScriptReference {
    /// Creates a new script reference for a tool.
    ///
    /// # Arguments
    ///
    /// * `tool_name` - The tool name (will be used as filename base)
    /// * `extension` - File extension without leading dot
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptReference;
    ///
    /// let ref1 = ScriptReference::new("send_message", "ts");
    /// let ref2 = ScriptReference::new("get-chat-info", "ts"); // Hyphens allowed
    /// ```
    #[must_use]
    pub fn new(tool_name: impl Into<String>, extension: impl Into<String>) -> Self {
        Self {
            tool_name: tool_name.into(),
            extension: extension.into(),
        }
    }

    /// Returns the relative path for use in SKILL.md.
    ///
    /// Always uses forward slashes for cross-platform compatibility.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptReference;
    ///
    /// let reference = ScriptReference::new("send_message", "ts");
    /// assert_eq!(reference.relative_path(), "scripts/send_message.ts");
    /// ```
    #[must_use]
    pub fn relative_path(&self) -> String {
        format!("scripts/{}.{}", self.tool_name, self.extension)
    }

    /// Returns the filename (without directory).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptReference;
    ///
    /// let reference = ScriptReference::new("send_message", "ts");
    /// assert_eq!(reference.filename(), "send_message.ts");
    /// ```
    #[must_use]
    pub fn filename(&self) -> String {
        format!("{}.{}", self.tool_name, self.extension)
    }

    /// Returns the tool name.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptReference;
    ///
    /// let reference = ScriptReference::new("send_message", "ts");
    /// assert_eq!(reference.tool_name(), "send_message");
    /// ```
    #[must_use]
    pub fn tool_name(&self) -> &str {
        &self.tool_name
    }

    /// Returns the file extension.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptReference;
    ///
    /// let reference = ScriptReference::new("send_message", "ts");
    /// assert_eq!(reference.extension(), "ts");
    /// ```
    #[must_use]
    pub fn extension(&self) -> &str {
        &self.extension
    }
}

/// A generated script file ready for writing to disk.
///
/// Contains both the content and metadata needed for persistence.
///
/// # Examples
///
/// ```
/// use mcp_core::ScriptFile;
///
/// let script = ScriptFile::new(
///     "send_message",
///     "ts",
///     "// Generated script\nexport async function sendMessage() { }",
/// );
///
/// assert_eq!(script.reference().relative_path(), "scripts/send_message.ts");
/// assert!(script.content().contains("sendMessage"));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScriptFile {
    /// Reference for this script
    reference: ScriptReference,
    /// Script content
    content: String,
}

impl ScriptFile {
    /// Creates a new script file.
    ///
    /// # Arguments
    ///
    /// * `tool_name` - The tool name (used as filename base)
    /// * `extension` - File extension without leading dot
    /// * `content` - The script content
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptFile;
    ///
    /// let script = ScriptFile::new(
    ///     "send_message",
    ///     "ts",
    ///     "export async function sendMessage() {}",
    /// );
    /// ```
    #[must_use]
    pub fn new(
        tool_name: impl Into<String>,
        extension: impl Into<String>,
        content: impl Into<String>,
    ) -> Self {
        Self {
            reference: ScriptReference::new(tool_name, extension),
            content: content.into(),
        }
    }

    /// Returns the script reference.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptFile;
    ///
    /// let script = ScriptFile::new("tool", "ts", "code");
    /// assert_eq!(script.reference().tool_name(), "tool");
    /// ```
    #[must_use]
    pub const fn reference(&self) -> &ScriptReference {
        &self.reference
    }

    /// Returns the script content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptFile;
    ///
    /// let script = ScriptFile::new("tool", "ts", "code");
    /// assert_eq!(script.content(), "code");
    /// ```
    #[must_use]
    pub fn content(&self) -> &str {
        &self.content
    }

    /// Consumes self and returns the content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::ScriptFile;
    ///
    /// let script = ScriptFile::new("tool", "ts", "code");
    /// let content = script.into_content();
    /// assert_eq!(content, "code");
    /// ```
    #[must_use]
    pub fn into_content(self) -> String {
        self.content
    }
}

/// A complete skill bundle ready for persistence.
///
/// Contains all files needed for a Claude Code skill:
/// - SKILL.md (required)
/// - scripts/ directory with TypeScript files
/// - reference.md (optional)
///
/// # Examples
///
/// ```
/// use mcp_core::{SkillBundle, SkillName, ScriptFile};
///
/// # fn example() -> Result<(), mcp_core::Error> {
/// let bundle = SkillBundle::builder("github")?
///     .skill_md("---\nname: github\n---\n# GitHub\n...")
///     .script(ScriptFile::new("create_issue", "ts", "// ..."))
///     .reference_md("# GitHub Reference\n...")
///     .build();
///
/// assert_eq!(bundle.scripts().len(), 1);
/// assert!(bundle.reference_md().is_some());
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillBundle {
    /// Skill name
    name: SkillName,
    /// SKILL.md content
    skill_md: String,
    /// Generated script files
    scripts: Vec<ScriptFile>,
    /// Optional reference.md content
    reference_md: Option<String>,
}

impl SkillBundle {
    /// Creates a new builder for `SkillBundle`.
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
    /// use mcp_core::SkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let builder = SkillBundle::builder("github")?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn builder(name: impl AsRef<str>) -> Result<SkillBundleBuilder> {
        Ok(SkillBundleBuilder::new(SkillName::new(name)?))
    }

    /// Returns the skill name.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---\nname: github\n---")
    ///     .build();
    /// assert_eq!(bundle.name().as_str(), "github");
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub const fn name(&self) -> &SkillName {
        &self.name
    }

    /// Returns the SKILL.md content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("# GitHub Skill")
    ///     .build();
    /// assert!(bundle.skill_md().contains("GitHub"));
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn skill_md(&self) -> &str {
        &self.skill_md
    }

    /// Returns the script files.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillBundle, ScriptFile};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---")
    ///     .script(ScriptFile::new("tool", "ts", "code"))
    ///     .build();
    /// assert_eq!(bundle.scripts().len(), 1);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn scripts(&self) -> &[ScriptFile] {
        &self.scripts
    }

    /// Returns the reference.md content if present.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---")
    ///     .reference_md("# Reference")
    ///     .build();
    /// assert!(bundle.reference_md().is_some());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn reference_md(&self) -> Option<&str> {
        self.reference_md.as_deref()
    }
}

/// Builder for `SkillBundle`.
///
/// Provides a fluent interface for constructing skill bundles.
///
/// # Examples
///
/// ```
/// use mcp_core::{SkillBundle, ScriptFile};
///
/// # fn example() -> Result<(), mcp_core::Error> {
/// let bundle = SkillBundle::builder("github")?
///     .skill_md("---\nname: github\n---\n# GitHub")
///     .script(ScriptFile::new("create_issue", "ts", "// code"))
///     .script(ScriptFile::new("close_issue", "ts", "// code"))
///     .reference_md("# GitHub API Reference")
///     .build();
///
/// assert_eq!(bundle.scripts().len(), 2);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SkillBundleBuilder {
    name: SkillName,
    skill_md: Option<String>,
    scripts: Vec<ScriptFile>,
    reference_md: Option<String>,
}

impl SkillBundleBuilder {
    const fn new(name: SkillName) -> Self {
        Self {
            name,
            skill_md: None,
            scripts: Vec::new(),
            reference_md: None,
        }
    }

    /// Sets the SKILL.md content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---\nname: github\n---\n# GitHub Skill")
    ///     .build();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn skill_md(mut self, content: impl Into<String>) -> Self {
        self.skill_md = Some(content.into());
        self
    }

    /// Adds a script file.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillBundle, ScriptFile};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---")
    ///     .script(ScriptFile::new("create_issue", "ts", "// code"))
    ///     .build();
    /// assert_eq!(bundle.scripts().len(), 1);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn script(mut self, script: ScriptFile) -> Self {
        self.scripts.push(script);
        self
    }

    /// Adds multiple script files.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::{SkillBundle, ScriptFile};
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let scripts = vec![
    ///     ScriptFile::new("tool1", "ts", "code1"),
    ///     ScriptFile::new("tool2", "ts", "code2"),
    /// ];
    ///
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---")
    ///     .scripts(scripts)
    ///     .build();
    /// assert_eq!(bundle.scripts().len(), 2);
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn scripts(mut self, scripts: impl IntoIterator<Item = ScriptFile>) -> Self {
        self.scripts.extend(scripts);
        self
    }

    /// Sets the optional reference.md content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---")
    ///     .reference_md("# GitHub API Reference")
    ///     .build();
    /// assert!(bundle.reference_md().is_some());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn reference_md(mut self, content: impl Into<String>) -> Self {
        self.reference_md = Some(content.into());
        self
    }

    /// Builds the `SkillBundle`.
    ///
    /// # Panics
    ///
    /// Panics if `skill_md` was not set. Use `try_build()` for error handling.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---\nname: github\n---")
    ///     .build();
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn build(self) -> SkillBundle {
        SkillBundle {
            name: self.name,
            skill_md: self.skill_md.expect("skill_md is required"),
            scripts: self.scripts,
            reference_md: self.reference_md,
        }
    }

    /// Tries to build the `SkillBundle`.
    ///
    /// # Errors
    ///
    /// Returns error if `skill_md` was not set.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_core::SkillBundle;
    ///
    /// # fn example() -> Result<(), mcp_core::Error> {
    /// let bundle = SkillBundle::builder("github")?
    ///     .skill_md("---\nname: github\n---")
    ///     .try_build()?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn try_build(self) -> Result<SkillBundle> {
        Ok(SkillBundle {
            name: self.name,
            skill_md: self.skill_md.ok_or_else(|| Error::ConfigError {
                message: "skill_md is required for SkillBundle".to_string(),
            })?,
            scripts: self.scripts,
            reference_md: self.reference_md,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_script_reference_new() {
        let reference = ScriptReference::new("send_message", "ts");
        assert_eq!(reference.tool_name(), "send_message");
        assert_eq!(reference.extension(), "ts");
    }

    #[test]
    fn test_script_reference_relative_path() {
        let reference = ScriptReference::new("send_message", "ts");
        assert_eq!(reference.relative_path(), "scripts/send_message.ts");
    }

    #[test]
    fn test_script_reference_filename() {
        let reference = ScriptReference::new("send_message", "ts");
        assert_eq!(reference.filename(), "send_message.ts");
    }

    #[test]
    fn test_script_reference_with_hyphens() {
        let reference = ScriptReference::new("get-chat-info", "ts");
        assert_eq!(reference.filename(), "get-chat-info.ts");
        assert_eq!(reference.relative_path(), "scripts/get-chat-info.ts");
    }

    #[test]
    fn test_script_file_new() {
        let script = ScriptFile::new("send_message", "ts", "// TypeScript code");
        assert_eq!(script.reference().tool_name(), "send_message");
        assert_eq!(script.content(), "// TypeScript code");
    }

    #[test]
    fn test_script_file_content() {
        let script = ScriptFile::new("tool", "ts", "export async function tool() {}");
        assert!(script.content().contains("export"));
        assert!(script.content().contains("async"));
    }

    #[test]
    fn test_script_file_into_content() {
        let script = ScriptFile::new("tool", "ts", "code");
        let content = script.into_content();
        assert_eq!(content, "code");
    }

    #[test]
    fn test_skill_bundle_builder() {
        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---\nname: github\n---\n# GitHub")
            .script(ScriptFile::new("create_issue", "ts", "// code"))
            .build();

        assert_eq!(bundle.name().as_str(), "github");
        assert_eq!(bundle.scripts().len(), 1);
        assert!(bundle.skill_md().contains("GitHub"));
    }

    #[test]
    fn test_skill_bundle_builder_invalid_name() {
        let result = SkillBundle::builder("INVALID");
        assert!(result.is_err());
    }

    #[test]
    fn test_skill_bundle_builder_multiple_scripts() {
        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---")
            .script(ScriptFile::new("tool1", "ts", "code1"))
            .script(ScriptFile::new("tool2", "ts", "code2"))
            .script(ScriptFile::new("tool3", "ts", "code3"))
            .build();

        assert_eq!(bundle.scripts().len(), 3);
    }

    #[test]
    fn test_skill_bundle_builder_scripts_method() {
        let scripts = vec![
            ScriptFile::new("tool1", "ts", "code1"),
            ScriptFile::new("tool2", "ts", "code2"),
        ];

        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---")
            .scripts(scripts)
            .build();

        assert_eq!(bundle.scripts().len(), 2);
    }

    #[test]
    fn test_skill_bundle_builder_with_reference() {
        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---")
            .reference_md("# GitHub Reference\n...")
            .build();

        assert!(bundle.reference_md().is_some());
        assert!(bundle.reference_md().unwrap().contains("Reference"));
    }

    #[test]
    fn test_skill_bundle_builder_without_reference() {
        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---")
            .build();

        assert!(bundle.reference_md().is_none());
    }

    #[test]
    fn test_skill_bundle_builder_empty_scripts() {
        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---")
            .build();

        assert_eq!(bundle.scripts().len(), 0);
    }

    #[test]
    #[should_panic(expected = "skill_md is required")]
    fn test_skill_bundle_builder_missing_skill_md() {
        let _bundle = SkillBundle::builder("github").unwrap().build();
    }

    #[test]
    fn test_skill_bundle_builder_try_build_missing_skill_md() {
        let result = SkillBundle::builder("github").unwrap().try_build();

        assert!(result.is_err());
        assert!(result.unwrap_err().is_config_error());
    }

    #[test]
    fn test_skill_bundle_builder_try_build_success() {
        let result = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---")
            .try_build();

        assert!(result.is_ok());
    }

    #[test]
    fn test_skill_bundle_name_validation() {
        // Valid names
        assert!(SkillBundle::builder("github").is_ok());
        assert!(SkillBundle::builder("my_skill").is_ok());
        assert!(SkillBundle::builder("my-skill").is_ok());

        // Invalid names
        assert!(SkillBundle::builder("INVALID").is_err());
        assert!(SkillBundle::builder("").is_err());
        assert!(SkillBundle::builder("anthropic-skill").is_err());
    }

    #[test]
    fn test_script_reference_equality() {
        let ref1 = ScriptReference::new("tool", "ts");
        let ref2 = ScriptReference::new("tool", "ts");
        let ref3 = ScriptReference::new("other", "ts");

        assert_eq!(ref1, ref2);
        assert_ne!(ref1, ref3);
    }

    #[test]
    fn test_script_reference_hash() {
        use std::collections::HashSet;

        let mut set = HashSet::new();
        set.insert(ScriptReference::new("tool1", "ts"));
        set.insert(ScriptReference::new("tool2", "ts"));
        set.insert(ScriptReference::new("tool1", "ts")); // Duplicate

        assert_eq!(set.len(), 2);
    }

    // Additional edge case tests

    #[test]
    fn test_script_reference_is_send_sync() {
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<ScriptReference>();
        assert_send_sync::<ScriptFile>();
        assert_send_sync::<SkillBundle>();
    }

    #[test]
    fn test_types_implement_debug() {
        let reference = ScriptReference::new("test", "ts");
        let debug_str = format!("{reference:?}");
        assert!(debug_str.contains("ScriptReference"));
        assert!(debug_str.contains("test"));

        let script = ScriptFile::new("test", "ts", "code");
        let debug_str = format!("{script:?}");
        assert!(debug_str.contains("ScriptFile"));

        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---")
            .build();
        let debug_str = format!("{bundle:?}");
        assert!(debug_str.contains("SkillBundle"));
    }

    #[test]
    fn test_script_reference_with_special_characters() {
        // Underscores and hyphens should be allowed
        let ref1 = ScriptReference::new("send_message", "ts");
        assert_eq!(ref1.filename(), "send_message.ts");

        let ref2 = ScriptReference::new("get-chat-info", "ts");
        assert_eq!(ref2.filename(), "get-chat-info.ts");

        let ref3 = ScriptReference::new("create_issue_v2", "ts");
        assert_eq!(ref3.relative_path(), "scripts/create_issue_v2.ts");
    }

    #[test]
    fn test_script_file_with_different_extensions() {
        let ts_script = ScriptFile::new("tool", "ts", "typescript code");
        assert_eq!(ts_script.reference().extension(), "ts");

        let py_script = ScriptFile::new("tool", "py", "python code");
        assert_eq!(py_script.reference().extension(), "py");

        let js_script = ScriptFile::new("tool", "js", "javascript code");
        assert_eq!(js_script.reference().extension(), "js");
    }

    #[test]
    fn test_script_file_with_empty_content() {
        let script = ScriptFile::new("tool", "ts", "");
        assert_eq!(script.content(), "");
        assert!(script.content().is_empty());
    }

    #[test]
    fn test_script_file_with_multiline_content() {
        let content = "line1\nline2\nline3";
        let script = ScriptFile::new("tool", "ts", content);
        assert!(script.content().contains('\n'));
        assert_eq!(script.content().lines().count(), 3);
    }

    #[test]
    fn test_script_file_with_unicode_content() {
        let content = "// 🚀 Rocket emoji\nfunction test() { return '日本語'; }";
        let script = ScriptFile::new("tool", "ts", content);
        assert!(script.content().contains('🚀'));
        assert!(script.content().contains("日本語"));
    }

    #[test]
    fn test_skill_bundle_builder_chain() {
        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---\nname: github\n---")
            .script(ScriptFile::new("tool1", "ts", "code1"))
            .script(ScriptFile::new("tool2", "ts", "code2"))
            .reference_md("# Reference")
            .build();

        assert_eq!(bundle.name().as_str(), "github");
        assert_eq!(bundle.scripts().len(), 2);
        assert!(bundle.reference_md().is_some());
        assert!(bundle.skill_md().contains("github"));
    }

    #[test]
    fn test_skill_bundle_accessors() {
        let scripts = vec![
            ScriptFile::new("tool1", "ts", "code1"),
            ScriptFile::new("tool2", "ts", "code2"),
        ];

        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("# GitHub Skill\n...")
            .scripts(scripts)
            .reference_md("# Reference Documentation")
            .build();

        // Test all accessors
        assert_eq!(bundle.name().as_str(), "github");
        assert_eq!(bundle.scripts().len(), 2);
        assert!(bundle.skill_md().starts_with("# GitHub"));
        assert_eq!(bundle.reference_md(), Some("# Reference Documentation"));
    }

    #[test]
    fn test_skill_bundle_with_large_number_of_scripts() {
        let mut builder = SkillBundle::builder("github").unwrap().skill_md("---");

        for i in 0..100 {
            builder = builder.script(ScriptFile::new(
                format!("tool{i}"),
                "ts",
                format!("code{i}"),
            ));
        }

        let bundle = builder.build();
        assert_eq!(bundle.scripts().len(), 100);
    }

    #[test]
    fn test_script_reference_path_uses_forward_slashes() {
        // Always use forward slashes regardless of platform
        let reference = ScriptReference::new("tool", "ts");
        let path = reference.relative_path();
        assert!(path.contains('/'));
        assert!(!path.contains('\\'));
        assert_eq!(path, "scripts/tool.ts");
    }

    #[test]
    fn test_skill_bundle_serialization() {
        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---\nname: github\n---")
            .script(ScriptFile::new("tool", "ts", "code"))
            .build();

        // Test that it can be serialized (required for persistence)
        let serialized = serde_json::to_string(&bundle);
        assert!(serialized.is_ok());
    }

    #[test]
    fn test_skill_bundle_deserialization() {
        let bundle = SkillBundle::builder("github")
            .unwrap()
            .skill_md("---\nname: github\n---")
            .script(ScriptFile::new("tool", "ts", "code"))
            .build();

        let serialized = serde_json::to_string(&bundle).unwrap();
        let deserialized: std::result::Result<SkillBundle, _> = serde_json::from_str(&serialized);
        assert!(deserialized.is_ok());

        let deserialized = deserialized.unwrap();
        assert_eq!(deserialized.name().as_str(), "github");
        assert_eq!(deserialized.scripts().len(), 1);
    }
}
