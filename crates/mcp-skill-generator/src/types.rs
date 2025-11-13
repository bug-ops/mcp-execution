//! Core types for MCP skill generation.
//!
//! This module defines strong types for skill generation following ADR-003
//! (strong types over primitives) and Microsoft Rust Guidelines.
//!
//! # Type Safety
//!
//! All types implement `Send + Sync + Debug` for Tokio compatibility.
//! Strong types prevent invalid states at compile time.
//!
//! # Examples
//!
//! ```
//! use mcp_skill_generator::{SkillName, SkillMetadata, GeneratedSkill};
//! use mcp_core::ServerId;
//!
//! // Create validated skill name
//! let name = SkillName::new("vkteams-bot").unwrap();
//! assert_eq!(name.as_str(), "vkteams-bot");
//!
//! // Invalid names fail validation
//! assert!(SkillName::new("123invalid").is_err());
//! assert!(SkillName::new("invalid-").is_err());
//! ```

use chrono::{DateTime, Utc};
use mcp_core::{ServerId, ToolName};
use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Errors that can occur during skill generation.
///
/// Following Microsoft Rust Guidelines, errors expose `is_xxx()` methods
/// instead of `ErrorKind` enums.
#[derive(Error, Debug)]
pub enum Error {
    /// Invalid skill name validation error.
    ///
    /// Skill names must follow Claude Code rules:
    /// - 1-64 characters
    /// - Only a-z, 0-9, hyphens, underscores
    /// - Start with letter
    /// - End with letter or number
    #[error("Invalid skill name: {name}. {reason}")]
    ValidationError {
        /// The invalid skill name.
        name: String,
        /// The reason the name is invalid.
        reason: String,
    },

    /// Template rendering failed.
    #[error("Template rendering failed: {message}")]
    TemplateError {
        /// The error message.
        message: String,
        /// The underlying error, if any.
        #[source]
        source: Option<Box<dyn std::error::Error + Send + Sync>>,
    },

    /// Server introspection failed.
    #[error("Introspection failed for server {server}")]
    IntrospectionError {
        /// The server ID that failed introspection.
        server: ServerId,
        /// The underlying error.
        #[source]
        source: Box<dyn std::error::Error + Send + Sync>,
    },

    /// File I/O operation failed.
    #[error("File I/O error for {path:?}")]
    IoError {
        /// The path that caused the error.
        path: std::path::PathBuf,
        /// The underlying I/O error.
        #[source]
        source: std::io::Error,
    },
}

impl Error {
    /// Returns true if this is a validation error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::{Error, SkillName};
    ///
    /// let err = SkillName::new("123invalid").unwrap_err();
    /// assert!(err.is_validation_error());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_validation_error(&self) -> bool {
        matches!(self, Self::ValidationError { .. })
    }

    /// Returns true if this is a template error.
    #[inline]
    #[must_use]
    pub fn is_template_error(&self) -> bool {
        matches!(self, Self::TemplateError { .. })
    }

    /// Returns true if this is an introspection error.
    #[inline]
    #[must_use]
    pub fn is_introspection_error(&self) -> bool {
        matches!(self, Self::IntrospectionError { .. })
    }

    /// Returns true if this is an I/O error.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::Error;
    /// use std::path::PathBuf;
    /// use std::io;
    ///
    /// let err = Error::IoError {
    ///     path: PathBuf::from("/tmp/test"),
    ///     source: io::Error::new(io::ErrorKind::NotFound, "file not found"),
    /// };
    /// assert!(err.is_io_error());
    /// ```
    #[inline]
    #[must_use]
    pub fn is_io_error(&self) -> bool {
        matches!(self, Self::IoError { .. })
    }
}

/// Result type for skill generator operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Validated skill name (newtype over String).
///
/// Enforces Claude Code skill naming rules at the type level.
/// Names must be 1-64 characters, contain only lowercase letters,
/// numbers, hyphens, and underscores, start with a letter, and
/// end with a letter or number.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::SkillName;
///
/// // Valid names
/// let name = SkillName::new("vkteams-bot").unwrap();
/// assert_eq!(name.as_str(), "vkteams-bot");
///
/// let name2 = SkillName::new("my_skill_123").unwrap();
/// assert_eq!(name2.as_str(), "my_skill_123");
///
/// // Invalid names
/// assert!(SkillName::new("").is_err());              // Empty
/// assert!(SkillName::new("123start").is_err());      // Starts with number
/// assert!(SkillName::new("invalid-").is_err());      // Ends with hyphen
/// assert!(SkillName::new("Invalid").is_err());       // Uppercase
/// assert!(SkillName::new("a".repeat(65)).is_err());  // Too long
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SkillName(String);

impl SkillName {
    /// Creates a new validated skill name.
    ///
    /// # Errors
    ///
    /// Returns `Error::ValidationError` if the name doesn't match requirements:
    /// - Must be 1-64 characters
    /// - Only lowercase letters, numbers, hyphens, underscores
    /// - Must start with a letter
    /// - Must end with a letter or number
    ///
    /// # Panics
    ///
    /// Will not panic as empty names are caught by validation.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillName;
    ///
    /// let name = SkillName::new("valid-name")?;
    /// assert_eq!(name.as_str(), "valid-name");
    ///
    /// // Invalid names
    /// assert!(SkillName::new("123invalid").is_err());
    /// assert!(SkillName::new("invalid-").is_err());
    /// assert!(SkillName::new("UPPERCASE").is_err());
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(name: impl AsRef<str>) -> Result<Self> {
        let name = name.as_ref();

        // Check length
        if name.is_empty() {
            return Err(Error::ValidationError {
                name: name.to_string(),
                reason: "Skill name cannot be empty".to_string(),
            });
        }

        if name.len() > 64 {
            return Err(Error::ValidationError {
                name: name.to_string(),
                reason: format!("Skill name too long ({} > 64 characters)", name.len()),
            });
        }

        // Check first character (must be letter)
        // SAFETY: name is not empty (checked above)
        let first_char = name.chars().next().unwrap();
        if !first_char.is_ascii_lowercase() {
            return Err(Error::ValidationError {
                name: name.to_string(),
                reason: "Skill name must start with a lowercase letter".to_string(),
            });
        }

        // Check last character (must be letter or number)
        // SAFETY: name is not empty (checked above)
        let last_char = name.chars().last().unwrap();
        if !last_char.is_ascii_alphanumeric() {
            return Err(Error::ValidationError {
                name: name.to_string(),
                reason: "Skill name must end with a letter or number".to_string(),
            });
        }

        // Check all characters (only a-z, 0-9, hyphens, underscores)
        for ch in name.chars() {
            if !ch.is_ascii_lowercase() && !ch.is_ascii_digit() && ch != '-' && ch != '_' {
                return Err(Error::ValidationError {
                    name: name.to_string(),
                    reason: format!(
                        "Invalid character '{ch}'. Only lowercase letters, numbers, hyphens, and underscores allowed"
                    ),
                });
            }
        }

        Ok(Self(name.to_string()))
    }

    /// Returns the skill name as a string slice.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillName;
    ///
    /// let name = SkillName::new("test-skill")?;
    /// assert_eq!(name.as_str(), "test-skill");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// Consumes the `SkillName` and returns the inner `String`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillName;
    ///
    /// let name = SkillName::new("test")?;
    /// let inner: String = name.into_inner();
    /// assert_eq!(inner, "test");
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    #[inline]
    #[must_use]
    pub fn into_inner(self) -> String {
        self.0
    }
}

impl fmt::Display for SkillName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Generated skill with metadata.
///
/// Represents a fully generated Claude Code skill including
/// the SKILL.md content and metadata about the generation.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::{GeneratedSkill, SkillName, SkillMetadata};
/// use mcp_core::ServerId;
/// use chrono::Utc;
///
/// let metadata = SkillMetadata {
///     server_id: ServerId::new("test-server"),
///     tool_count: 5,
///     generated_at: Utc::now(),
///     generator_version: env!("CARGO_PKG_VERSION").to_string(),
/// };
///
/// let skill = GeneratedSkill {
///     name: SkillName::new("test-skill")?,
///     content: "# Test Skill\n...".to_string(),
///     metadata,
/// };
///
/// assert_eq!(skill.name.as_str(), "test-skill");
/// assert_eq!(skill.metadata.tool_count, 5);
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneratedSkill {
    /// Skill name (validated).
    pub name: SkillName,

    /// Generated SKILL.md content.
    pub content: String,

    /// Generation metadata.
    pub metadata: SkillMetadata,
}

/// Metadata about a generated skill.
///
/// Contains information about the source MCP server and
/// the generation process.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::SkillMetadata;
/// use mcp_core::ServerId;
/// use chrono::Utc;
///
/// let metadata = SkillMetadata {
///     server_id: ServerId::new("vkteams-bot"),
///     tool_count: 3,
///     generated_at: Utc::now(),
///     generator_version: "0.1.0".to_string(),
/// };
///
/// assert_eq!(metadata.server_id.as_str(), "vkteams-bot");
/// assert_eq!(metadata.tool_count, 3);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillMetadata {
    /// Source MCP server ID.
    pub server_id: ServerId,

    /// Number of tools included in the skill.
    pub tool_count: usize,

    /// Generation timestamp.
    pub generated_at: DateTime<Utc>,

    /// Generator version (semver).
    pub generator_version: String,
}

/// Configuration options for skill generation.
///
/// Controls how skills are generated from MCP servers.
/// Use the builder pattern for construction.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::{SkillGenerationOptions, TemplateType};
///
/// // Use default options
/// let options = SkillGenerationOptions::default();
/// assert_eq!(options.template_type, TemplateType::Standard);
///
/// // Use builder pattern
/// let options = SkillGenerationOptions::builder()
///     .template_type(TemplateType::Verbose)
///     .include_examples(true)
///     .build();
///
/// assert_eq!(options.template_type, TemplateType::Verbose);
/// assert!(options.include_examples);
/// ```
#[derive(Debug, Clone)]
pub struct SkillGenerationOptions {
    /// Template type to use for generation.
    pub template_type: TemplateType,

    /// Include usage examples in the generated skill.
    pub include_examples: bool,

    /// Custom prompt to add to the skill (optional).
    pub custom_prompt: Option<String>,
}

impl Default for SkillGenerationOptions {
    fn default() -> Self {
        Self {
            template_type: TemplateType::Standard,
            include_examples: false,
            custom_prompt: None,
        }
    }
}

impl SkillGenerationOptions {
    /// Creates a new builder for `SkillGenerationOptions`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillGenerationOptions;
    ///
    /// let options = SkillGenerationOptions::builder()
    ///     .include_examples(true)
    ///     .build();
    ///
    /// assert!(options.include_examples);
    /// ```
    #[must_use]
    pub fn builder() -> SkillGenerationOptionsBuilder {
        SkillGenerationOptionsBuilder::default()
    }
}

/// Builder for `SkillGenerationOptions`.
///
/// Provides a fluent API for constructing options.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::{SkillGenerationOptions, TemplateType};
///
/// let options = SkillGenerationOptions::builder()
///     .template_type(TemplateType::Minimal)
///     .include_examples(false)
///     .custom_prompt("Custom instructions here")
///     .build();
/// ```
#[derive(Debug, Clone, Default)]
pub struct SkillGenerationOptionsBuilder {
    template_type: Option<TemplateType>,
    include_examples: Option<bool>,
    custom_prompt: Option<String>,
}

impl SkillGenerationOptionsBuilder {
    /// Sets the template type.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::{SkillGenerationOptions, TemplateType};
    ///
    /// let options = SkillGenerationOptions::builder()
    ///     .template_type(TemplateType::Verbose)
    ///     .build();
    /// ```
    #[must_use]
    pub fn template_type(mut self, template_type: TemplateType) -> Self {
        self.template_type = Some(template_type);
        self
    }

    /// Sets whether to include examples.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillGenerationOptions;
    ///
    /// let options = SkillGenerationOptions::builder()
    ///     .include_examples(true)
    ///     .build();
    /// ```
    #[must_use]
    pub fn include_examples(mut self, include_examples: bool) -> Self {
        self.include_examples = Some(include_examples);
        self
    }

    /// Sets a custom prompt.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillGenerationOptions;
    ///
    /// let options = SkillGenerationOptions::builder()
    ///     .custom_prompt("Always be polite")
    ///     .build();
    /// ```
    #[must_use]
    pub fn custom_prompt(mut self, prompt: impl Into<String>) -> Self {
        self.custom_prompt = Some(prompt.into());
        self
    }

    /// Builds the `SkillGenerationOptions`.
    ///
    /// Uses defaults for any unset fields.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillGenerationOptions;
    ///
    /// let options = SkillGenerationOptions::builder().build();
    /// ```
    #[must_use]
    pub fn build(self) -> SkillGenerationOptions {
        SkillGenerationOptions {
            template_type: self.template_type.unwrap_or(TemplateType::Standard),
            include_examples: self.include_examples.unwrap_or(false),
            custom_prompt: self.custom_prompt,
        }
    }
}

/// Template type for skill generation.
///
/// Controls the level of detail in the generated skill.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::TemplateType;
///
/// let standard = TemplateType::Standard;
/// let minimal = TemplateType::Minimal;
/// let verbose = TemplateType::Verbose;
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum TemplateType {
    /// Standard template with balanced detail.
    #[default]
    Standard,

    /// Minimal template with only essential information.
    Minimal,

    /// Verbose template with extensive documentation.
    Verbose,
}

impl fmt::Display for TemplateType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Standard => write!(f, "standard"),
            Self::Minimal => write!(f, "minimal"),
            Self::Verbose => write!(f, "verbose"),
        }
    }
}

/// Template context for skill rendering.
///
/// Contains all data needed to render a SKILL.md template.
/// This is serialized to JSON and passed to Handlebars.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::SkillContext;
/// use mcp_core::ServerId;
///
/// let context = SkillContext {
///     name: "test-skill".to_string(),
///     description: "A test skill".to_string(),
///     server_id: ServerId::new("test-server"),
///     tool_count: 3,
///     tools: vec![],
///     generator_version: "0.1.0".to_string(),
///     generated_at: chrono::Utc::now().to_rfc3339(),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SkillContext {
    /// Skill name.
    pub name: String,

    /// Skill description.
    pub description: String,

    /// Source server ID.
    pub server_id: ServerId,

    /// Number of tools.
    pub tool_count: usize,

    /// Tool documentation.
    pub tools: Vec<ToolContext>,

    /// Generator version.
    pub generator_version: String,

    /// Generation timestamp (RFC3339 format).
    pub generated_at: String,
}

/// Tool documentation for templates.
///
/// Represents a single tool's documentation in the generated skill.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::ToolContext;
/// use mcp_core::ToolName;
///
/// let tool = ToolContext {
///     name: ToolName::new("send_message"),
///     description: "Sends a message to a chat".to_string(),
///     parameters: vec![],
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolContext {
    /// Tool name.
    pub name: ToolName,

    /// Tool description.
    pub description: String,

    /// Parameter documentation.
    pub parameters: Vec<ParameterContext>,
}

/// Parameter documentation for templates.
///
/// Represents a single parameter's documentation.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::ParameterContext;
///
/// let param = ParameterContext {
///     name: "chat_id".to_string(),
///     type_name: "string".to_string(),
///     required: true,
///     description: "The chat ID to send to".to_string(),
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParameterContext {
    /// Parameter name.
    pub name: String,

    /// Parameter type (TypeScript syntax).
    pub type_name: String,

    /// Whether the parameter is required.
    pub required: bool,

    /// Parameter description.
    pub description: String,
}

/// Sanitizes a string by removing control characters except whitespace.
///
/// Prevents unicode control character attacks (RTLO, LTRO, etc.)
/// while preserving normal formatting (newlines, tabs, spaces).
///
/// # Examples
///
/// ```
/// # use mcp_skill_generator::sanitize_string;
/// let sanitized = sanitize_string("Normal\u{202E}REVERSED\u{202D}");
/// assert!(!sanitized.contains('\u{202E}'));
/// ```
#[must_use]
pub fn sanitize_string(s: &str) -> String {
    s.chars()
        .filter(|c| {
            // Allow normal whitespace
            if matches!(*c, '\n' | '\t' | ' ') {
                return true;
            }

            // Block control characters
            if c.is_control() {
                return false;
            }

            // Block unicode directional formatting (RTLO, LTRO, etc.)
            // These are in the range U+202A to U+202E and U+2066 to U+2069
            let ch = *c as u32;
            if (0x202A..=0x202E).contains(&ch) || (0x2066..=0x2069).contains(&ch) {
                return false;
            }

            true
        })
        .collect()
}

/// Validates that a string doesn't contain Handlebars template syntax.
///
/// Prevents template injection attacks via user-controlled data.
///
/// # Errors
///
/// Returns `Error::ValidationError` if template syntax is detected.
///
/// # Examples
///
/// ```
/// # use mcp_skill_generator::validate_no_template_syntax;
/// assert!(validate_no_template_syntax("Normal text", "field").is_ok());
/// assert!(validate_no_template_syntax("Bad {{injection}}", "field").is_err());
/// ```
pub fn validate_no_template_syntax(s: &str, field_name: &str) -> Result<()> {
    if s.contains("{{") || s.contains("}}") {
        return Err(Error::ValidationError {
            name: field_name.to_string(),
            reason: "Template syntax ({{ or }}) not allowed".to_string(),
        });
    }
    Ok(())
}

impl SkillContext {
    /// Validates that context data is safe for template rendering.
    ///
    /// Checks for:
    /// - Template injection attempts ({{ or }})
    ///
    /// # Errors
    ///
    /// Returns `Error::ValidationError` if any field contains unsafe content.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillContext;
    /// use mcp_core::ServerId;
    ///
    /// let context = SkillContext {
    ///     name: "test".to_string(),
    ///     description: "Safe description".to_string(),
    ///     server_id: ServerId::new("server"),
    ///     tool_count: 0,
    ///     tools: vec![],
    ///     generator_version: "0.1.0".to_string(),
    ///     generated_at: "2025-11-13T10:00:00Z".to_string(),
    /// };
    ///
    /// assert!(context.validate().is_ok());
    /// ```
    pub fn validate(&self) -> Result<()> {
        // Validate description
        validate_no_template_syntax(&self.description, "description")?;

        // Validate all tool contexts
        for tool in &self.tools {
            validate_no_template_syntax(
                &tool.description,
                &format!("tool:{}:description", tool.name),
            )?;

            // Validate parameters
            for param in &tool.parameters {
                validate_no_template_syntax(
                    &param.description,
                    &format!("tool:{}:param:{}:description", tool.name, param.name),
                )?;
            }
        }

        Ok(())
    }

    /// Creates a new `SkillContext` with sanitized fields.
    ///
    /// Removes unicode control characters from all text fields.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::SkillContext;
    /// use mcp_core::ServerId;
    ///
    /// let context = SkillContext::new_sanitized(
    ///     "test",
    ///     "Description\u{202E}REVERSED", // Will be sanitized
    ///     ServerId::new("server"),
    ///     vec![],
    ///     "0.1.0",
    /// );
    ///
    /// assert!(!context.description.contains('\u{202E}'));
    /// ```
    #[must_use]
    pub fn new_sanitized(
        name: impl AsRef<str>,
        description: impl AsRef<str>,
        server_id: ServerId,
        tools: Vec<ToolContext>,
        generator_version: impl AsRef<str>,
    ) -> Self {
        Self {
            name: sanitize_string(name.as_ref()),
            description: sanitize_string(description.as_ref()),
            server_id,
            tool_count: tools.len(),
            tools: tools.into_iter().map(ToolContext::sanitize).collect(),
            generator_version: sanitize_string(generator_version.as_ref()),
            generated_at: chrono::Utc::now().to_rfc3339(),
        }
    }
}

impl ToolContext {
    /// Returns a sanitized copy of this `ToolContext`.
    ///
    /// Removes control characters from descriptions.
    #[must_use]
    pub fn sanitize(self) -> Self {
        Self {
            name: self.name,
            description: sanitize_string(&self.description),
            parameters: self
                .parameters
                .into_iter()
                .map(ParameterContext::sanitize)
                .collect(),
        }
    }
}

impl ParameterContext {
    /// Returns a sanitized copy of this `ParameterContext`.
    ///
    /// Removes control characters from name and description.
    #[must_use]
    pub fn sanitize(self) -> Self {
        Self {
            name: sanitize_string(&self.name),
            type_name: self.type_name,
            required: self.required,
            description: sanitize_string(&self.description),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_skill_name_valid() {
        assert!(SkillName::new("vkteams-bot").is_ok());
        assert!(SkillName::new("my_skill").is_ok());
        assert!(SkillName::new("skill123").is_ok());
        assert!(SkillName::new("a").is_ok());
        assert!(SkillName::new("skill-name-with-hyphens").is_ok());
        assert!(SkillName::new("skill_name_with_underscores").is_ok());
    }

    #[test]
    fn test_skill_name_invalid_start() {
        // Starts with number
        assert!(SkillName::new("123skill").is_err());
        // Starts with hyphen
        assert!(SkillName::new("-skill").is_err());
        // Starts with underscore
        assert!(SkillName::new("_skill").is_err());
        // Starts with uppercase
        assert!(SkillName::new("Skill").is_err());
    }

    #[test]
    fn test_skill_name_invalid_end() {
        // Ends with hyphen
        assert!(SkillName::new("skill-").is_err());
        // Ends with underscore
        assert!(SkillName::new("skill_").is_err());
    }

    #[test]
    fn test_skill_name_invalid_characters() {
        // Contains uppercase
        assert!(SkillName::new("skillName").is_err());
        // Contains space
        assert!(SkillName::new("skill name").is_err());
        // Contains special characters
        assert!(SkillName::new("skill@name").is_err());
        assert!(SkillName::new("skill.name").is_err());
        assert!(SkillName::new("skill/name").is_err());
    }

    #[test]
    fn test_skill_name_length() {
        // Empty
        assert!(SkillName::new("").is_err());
        // Too long (65 characters)
        assert!(SkillName::new("a".repeat(65)).is_err());
        // Maximum length (64 characters)
        assert!(SkillName::new("a".repeat(64)).is_ok());
    }

    #[test]
    fn test_skill_name_as_str() {
        let name = SkillName::new("test-skill").unwrap();
        assert_eq!(name.as_str(), "test-skill");
    }

    #[test]
    fn test_skill_name_display() {
        let name = SkillName::new("test-skill").unwrap();
        assert_eq!(format!("{name}"), "test-skill");
    }

    #[test]
    fn test_skill_name_into_inner() {
        let name = SkillName::new("test").unwrap();
        assert_eq!(name.into_inner(), "test");
    }

    #[test]
    fn test_skill_name_equality() {
        let name1 = SkillName::new("skill").unwrap();
        let name2 = SkillName::new("skill").unwrap();
        let name3 = SkillName::new("other").unwrap();

        assert_eq!(name1, name2);
        assert_ne!(name1, name3);
    }

    #[test]
    fn test_generated_skill_creation() {
        let metadata = SkillMetadata {
            server_id: ServerId::new("test-server"),
            tool_count: 5,
            generated_at: Utc::now(),
            generator_version: "0.1.0".to_string(),
        };

        let skill = GeneratedSkill {
            name: SkillName::new("test-skill").unwrap(),
            content: "# Test Skill\n...".to_string(),
            metadata,
        };

        assert_eq!(skill.name.as_str(), "test-skill");
        assert_eq!(skill.metadata.tool_count, 5);
    }

    #[test]
    fn test_skill_generation_options_default() {
        let options = SkillGenerationOptions::default();
        assert_eq!(options.template_type, TemplateType::Standard);
        assert!(!options.include_examples);
        assert!(options.custom_prompt.is_none());
    }

    #[test]
    fn test_skill_generation_options_builder() {
        let options = SkillGenerationOptions::builder()
            .template_type(TemplateType::Verbose)
            .include_examples(true)
            .custom_prompt("Custom prompt")
            .build();

        assert_eq!(options.template_type, TemplateType::Verbose);
        assert!(options.include_examples);
        assert_eq!(options.custom_prompt, Some("Custom prompt".to_string()));
    }

    #[test]
    fn test_skill_generation_options_builder_partial() {
        let options = SkillGenerationOptions::builder()
            .include_examples(true)
            .build();

        // Should use defaults for unset fields
        assert_eq!(options.template_type, TemplateType::Standard);
        assert!(options.include_examples);
        assert!(options.custom_prompt.is_none());
    }

    #[test]
    fn test_template_type_display() {
        assert_eq!(format!("{}", TemplateType::Standard), "standard");
        assert_eq!(format!("{}", TemplateType::Minimal), "minimal");
        assert_eq!(format!("{}", TemplateType::Verbose), "verbose");
    }

    #[test]
    fn test_error_is_validation_error() {
        let err = Error::ValidationError {
            name: "invalid".to_string(),
            reason: "test".to_string(),
        };
        assert!(err.is_validation_error());
        assert!(!err.is_template_error());
        assert!(!err.is_introspection_error());
    }

    #[test]
    fn test_error_is_template_error() {
        let err = Error::TemplateError {
            message: "test".to_string(),
            source: None,
        };
        assert!(!err.is_validation_error());
        assert!(err.is_template_error());
        assert!(!err.is_introspection_error());
    }

    #[test]
    fn test_skill_context_serialization() {
        let context = SkillContext {
            name: "test".to_string(),
            description: "A test".to_string(),
            server_id: ServerId::new("server"),
            tool_count: 1,
            tools: vec![],
            generator_version: "0.1.0".to_string(),
            generated_at: "2025-11-13T10:00:00Z".to_string(),
        };

        let json = serde_json::to_string(&context).unwrap();
        assert!(json.contains("test"));
        assert!(json.contains("server"));
    }
}
