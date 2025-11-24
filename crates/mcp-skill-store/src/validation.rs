//! Skill validation framework.
//!
//! Provides comprehensive validation for Claude skills including:
//! - Metadata validation (skill name, tools, checksums)
//! - Content validation (YAML frontmatter, required fields)
//! - Checksum verification using Blake3
//! - Strict mode for enhanced validation
//!
//! # Examples
//!
//! ## Basic validation
//!
//! ```
//! use mcp_skill_store::validation::{SkillValidator, ClaudeSkill};
//! use mcp_skill_store::ClaudeSkillMetadata;
//! # use chrono::Utc;
//! # use mcp_skill_store::SkillChecksums;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let skill = ClaudeSkill {
//!     metadata: ClaudeSkillMetadata {
//!         skill_name: "my-skill".to_string(),
//!         server_name: "server".to_string(),
//!         server_version: "1.0.0".to_string(),
//!         protocol_version: "1.0".to_string(),
//!         tool_count: 1,
//!         generated_at: Utc::now(),
//!         generator_version: "0.1.0".to_string(),
//!         checksums: SkillChecksums {
//!             skill_md: "blake3:test".to_string(),
//!             reference_md: None,
//!         },
//!     },
//!     content: "---\nname: my-skill\ndescription: Test skill\n---\n\n# My Skill".to_string(),
//! };
//!
//! let validator = SkillValidator::new();
//! let report = validator.validate(&skill)?;
//!
//! if report.valid {
//!     println!("Skill is valid!");
//! } else {
//!     for error in &report.errors {
//!         eprintln!("Error in {}: {}", error.field, error.message);
//!     }
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Strict validation
//!
//! ```
//! use mcp_skill_store::validation::{SkillValidator, ClaudeSkill};
//! # use mcp_skill_store::ClaudeSkillMetadata;
//! # use chrono::Utc;
//! # use mcp_skill_store::SkillChecksums;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let skill = ClaudeSkill {
//! #     metadata: ClaudeSkillMetadata {
//! #         skill_name: "my-skill".to_string(),
//! #         server_name: "server".to_string(),
//! #         server_version: "1.0.0".to_string(),
//! #         protocol_version: "1.0".to_string(),
//! #         tool_count: 0,
//! #         generated_at: Utc::now(),
//! #         generator_version: "0.1.0".to_string(),
//! #         checksums: SkillChecksums {
//! #             skill_md: "blake3:test".to_string(),
//! #             reference_md: None,
//! #         },
//! #     },
//! #     content: "---\nname: my-skill\n---\n".to_string(),
//! # };
//! // Strict mode produces warnings for optional best practices
//! let validator = SkillValidator::strict();
//! let report = validator.validate(&skill)?;
//!
//! for warning in &report.warnings {
//!     println!("Warning in {}: {}", warning.field, warning.message);
//! }
//! # Ok(())
//! # }
//! ```

use crate::error::Result;
use crate::types::ClaudeSkillMetadata;

/// Skill data for validation.
///
/// Combines metadata and content for comprehensive validation.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::validation::ClaudeSkill;
/// use mcp_skill_store::ClaudeSkillMetadata;
/// # use chrono::Utc;
/// # use mcp_skill_store::SkillChecksums;
///
/// let skill = ClaudeSkill {
///     metadata: ClaudeSkillMetadata {
///         skill_name: "my-skill".to_string(),
///         server_name: "server".to_string(),
///         server_version: "1.0.0".to_string(),
///         protocol_version: "1.0".to_string(),
///         tool_count: 1,
///         generated_at: Utc::now(),
///         generator_version: "0.1.0".to_string(),
///         checksums: SkillChecksums {
///             skill_md: "blake3:test".to_string(),
///             reference_md: None,
///         },
///     },
///     content: "---\nname: my-skill\n---\n\nContent".to_string(),
/// };
/// ```
#[derive(Debug, Clone)]
pub struct ClaudeSkill {
    /// Skill metadata from .metadata.json
    pub metadata: ClaudeSkillMetadata,
    /// SKILL.md content
    pub content: String,
}

/// Validation report with detailed results.
///
/// Contains the validation status and lists of errors and warnings.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::validation::ValidationReport;
///
/// let report = ValidationReport {
///     valid: true,
///     errors: vec![],
///     warnings: vec![],
/// };
///
/// assert!(report.valid);
/// ```
#[derive(Debug, Clone)]
pub struct ValidationReport {
    /// Whether the skill is valid (no errors)
    pub valid: bool,
    /// List of validation errors (prevent usage)
    pub errors: Vec<ValidationError>,
    /// List of validation warnings (best practices)
    pub warnings: Vec<ValidationWarning>,
}

/// Validation error.
///
/// Represents a critical issue that makes the skill invalid.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::validation::ValidationError;
///
/// let error = ValidationError {
///     field: "skill_name".to_string(),
///     message: "Skill name cannot be empty".to_string(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    /// Field that failed validation
    pub field: String,
    /// Error message
    pub message: String,
}

/// Validation warning.
///
/// Represents a non-critical issue or best practice violation.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::validation::ValidationWarning;
///
/// let warning = ValidationWarning {
///     field: "tool_count".to_string(),
///     message: "No tools specified".to_string(),
/// };
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationWarning {
    /// Field that triggered the warning
    pub field: String,
    /// Warning message
    pub message: String,
}

/// Skill validator.
///
/// Validates Claude skills for correctness, integrity, and best practices.
///
/// # Examples
///
/// ```
/// use mcp_skill_store::validation::SkillValidator;
///
/// // Normal validation
/// let validator = SkillValidator::new();
///
/// // Strict validation (more warnings)
/// let strict_validator = SkillValidator::strict();
/// ```
#[derive(Debug, Clone)]
pub struct SkillValidator {
    strict_mode: bool,
}

impl SkillValidator {
    /// Creates a new validator with normal validation rules.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_store::validation::SkillValidator;
    ///
    /// let validator = SkillValidator::new();
    /// assert!(!validator.is_strict());
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self { strict_mode: false }
    }

    /// Creates a strict validator with enhanced warnings.
    ///
    /// Strict mode enforces additional best practices:
    /// - Warns if no tools are specified
    /// - Warns about missing descriptions
    /// - Warns about short content
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_store::validation::SkillValidator;
    ///
    /// let validator = SkillValidator::strict();
    /// assert!(validator.is_strict());
    /// ```
    #[must_use]
    pub const fn strict() -> Self {
        Self { strict_mode: true }
    }

    /// Returns whether this validator uses strict mode.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_store::validation::SkillValidator;
    ///
    /// let normal = SkillValidator::new();
    /// assert!(!normal.is_strict());
    ///
    /// let strict = SkillValidator::strict();
    /// assert!(strict.is_strict());
    /// ```
    #[must_use]
    pub const fn is_strict(&self) -> bool {
        self.strict_mode
    }

    /// Validates a skill.
    ///
    /// Performs comprehensive validation including:
    /// - Metadata validation (skill name, server info)
    /// - Content validation (YAML frontmatter, required fields)
    /// - Checksum verification (if checksums are present)
    ///
    /// # Errors
    ///
    /// Returns error if validation process fails (not if skill is invalid).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_store::validation::{SkillValidator, ClaudeSkill};
    /// use mcp_skill_store::ClaudeSkillMetadata;
    /// # use chrono::Utc;
    /// # use mcp_skill_store::SkillChecksums;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let skill = ClaudeSkill {
    ///     metadata: ClaudeSkillMetadata {
    ///         skill_name: "my-skill".to_string(),
    ///         server_name: "server".to_string(),
    ///         server_version: "1.0.0".to_string(),
    ///         protocol_version: "1.0".to_string(),
    ///         tool_count: 1,
    ///         generated_at: Utc::now(),
    ///         generator_version: "0.1.0".to_string(),
    ///         checksums: SkillChecksums {
    ///             skill_md: "blake3:test".to_string(),
    ///             reference_md: None,
    ///         },
    ///     },
    ///     content: "---\nname: my-skill\ndescription: Test\n---\n\nContent".to_string(),
    /// };
    ///
    /// let validator = SkillValidator::new();
    /// let report = validator.validate(&skill)?;
    ///
    /// if report.valid {
    ///     println!("Skill is valid!");
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn validate(&self, skill: &ClaudeSkill) -> Result<ValidationReport> {
        let mut report = ValidationReport {
            valid: true,
            errors: Vec::new(),
            warnings: Vec::new(),
        };

        // Validate metadata
        self.validate_metadata(&skill.metadata, &mut report);

        // Validate content
        self.validate_content(&skill.content, &mut report);

        // Validate checksum
        Self::validate_checksum(
            &skill.content,
            &skill.metadata.checksums.skill_md,
            &mut report,
        );

        // Update validity based on errors
        report.valid = report.errors.is_empty();
        Ok(report)
    }

    /// Validates skill metadata.
    fn validate_metadata(&self, metadata: &ClaudeSkillMetadata, report: &mut ValidationReport) {
        // Validate skill name
        if metadata.skill_name.is_empty() {
            report.errors.push(ValidationError {
                field: "skill_name".to_string(),
                message: "Skill name cannot be empty".to_string(),
            });
        } else if !Self::is_valid_skill_name(&metadata.skill_name) {
            report.errors.push(ValidationError {
                field: "skill_name".to_string(),
                message: format!(
                    "Invalid skill name '{}': must contain only lowercase letters, numbers, hyphens, and underscores",
                    metadata.skill_name
                ),
            });
        }

        // Validate server name
        if metadata.server_name.is_empty() {
            report.errors.push(ValidationError {
                field: "server_name".to_string(),
                message: "Server name cannot be empty".to_string(),
            });
        }

        // Validate server version
        if metadata.server_version.is_empty() {
            report.errors.push(ValidationError {
                field: "server_version".to_string(),
                message: "Server version cannot be empty".to_string(),
            });
        }

        // Validate protocol version
        if metadata.protocol_version.is_empty() {
            report.errors.push(ValidationError {
                field: "protocol_version".to_string(),
                message: "Protocol version cannot be empty".to_string(),
            });
        }

        // Validate tool count
        if metadata.tool_count == 0 && self.strict_mode {
            report.warnings.push(ValidationWarning {
                field: "tool_count".to_string(),
                message: "No tools specified (tool_count is 0)".to_string(),
            });
        }
    }

    /// Validates skill content (SKILL.md).
    fn validate_content(&self, content: &str, report: &mut ValidationReport) {
        // Check content is not empty
        if content.is_empty() {
            report.errors.push(ValidationError {
                field: "content".to_string(),
                message: "Skill content cannot be empty".to_string(),
            });
            return;
        }

        // Check YAML frontmatter exists
        if !content.starts_with("---\n") && !content.starts_with("---\r\n") {
            report.errors.push(ValidationError {
                field: "content".to_string(),
                message: "Missing YAML frontmatter (must start with '---')".to_string(),
            });
        } else {
            // Extract frontmatter for detailed validation
            self.validate_frontmatter(content, report);
        }

        // Strict mode: warn about short content
        if self.strict_mode && content.len() < 100 {
            report.warnings.push(ValidationWarning {
                field: "content".to_string(),
                message: format!("Content is very short ({} bytes)", content.len()),
            });
        }
    }

    /// Validates YAML frontmatter.
    fn validate_frontmatter(&self, content: &str, report: &mut ValidationReport) {
        // Find the second --- delimiter
        let content_after_first = content
            .strip_prefix("---\n")
            .or_else(|| content.strip_prefix("---\r\n"));

        if let Some(remaining) = content_after_first {
            if let Some(end_pos) = remaining
                .find("\n---\n")
                .or_else(|| remaining.find("\r\n---\r\n"))
            {
                let frontmatter = &remaining[..end_pos];

                // Check for required 'name' field
                if !frontmatter.contains("name:") {
                    report.errors.push(ValidationError {
                        field: "content.frontmatter".to_string(),
                        message: "Missing required 'name' field in frontmatter".to_string(),
                    });
                }

                // Strict mode: check for description
                if self.strict_mode && !frontmatter.contains("description:") {
                    report.warnings.push(ValidationWarning {
                        field: "content.frontmatter".to_string(),
                        message: "Missing 'description' field in frontmatter".to_string(),
                    });
                }
            } else {
                report.errors.push(ValidationError {
                    field: "content.frontmatter".to_string(),
                    message: "YAML frontmatter not properly closed (missing closing '---')"
                        .to_string(),
                });
            }
        }
    }

    /// Validates checksum matches content.
    fn validate_checksum(content: &str, stored_checksum: &str, report: &mut ValidationReport) {
        use blake3::Hasher;

        // Extract actual hash from "blake3:HASH" format
        let expected = if let Some(hash) = stored_checksum.strip_prefix("blake3:") {
            hash
        } else {
            report.warnings.push(ValidationWarning {
                field: "checksums.skill_md".to_string(),
                message: format!("Checksum does not use 'blake3:' prefix: {stored_checksum}"),
            });
            stored_checksum
        };

        // Calculate actual checksum
        let mut hasher = Hasher::new();
        hasher.update(content.as_bytes());
        let actual = hasher.finalize().to_hex().to_string();

        // Compare
        if actual != expected {
            report.errors.push(ValidationError {
                field: "checksums.skill_md".to_string(),
                message: format!("Checksum mismatch: expected '{expected}', got '{actual}'"),
            });
        }
    }

    /// Validates skill name format.
    ///
    /// Valid skill names:
    /// - Lowercase letters (a-z)
    /// - Numbers (0-9)
    /// - Hyphens (-)
    /// - Underscores (_)
    fn is_valid_skill_name(name: &str) -> bool {
        !name.is_empty()
            && name
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
    }
}

impl Default for SkillValidator {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::SkillChecksums;
    use chrono::Utc;

    fn create_valid_skill() -> ClaudeSkill {
        let content = "---\nname: test-skill\ndescription: A test skill\n---\n\n# Test Skill\n\nThis is a test skill.";

        // Calculate checksum
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(content.as_bytes());
        let checksum = format!("blake3:{}", hasher.finalize().to_hex());

        ClaudeSkill {
            metadata: ClaudeSkillMetadata {
                skill_name: "test-skill".to_string(),
                server_name: "test-server".to_string(),
                server_version: "1.0.0".to_string(),
                protocol_version: "1.0".to_string(),
                tool_count: 1,
                generated_at: Utc::now(),
                generator_version: "0.1.0".to_string(),
                checksums: SkillChecksums {
                    skill_md: checksum,
                    reference_md: None,
                },
            },
            content: content.to_string(),
        }
    }

    #[test]
    fn test_validator_creation() {
        let validator = SkillValidator::new();
        assert!(!validator.is_strict());

        let strict = SkillValidator::strict();
        assert!(strict.is_strict());
    }

    #[test]
    fn test_valid_skill_passes() {
        let skill = create_valid_skill();
        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(report.valid);
        assert!(report.errors.is_empty());
    }

    #[test]
    fn test_empty_skill_name_fails() {
        let mut skill = create_valid_skill();
        skill.metadata.skill_name = String::new();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert_eq!(report.errors.len(), 1);
        assert_eq!(report.errors[0].field, "skill_name");
    }

    #[test]
    fn test_invalid_skill_name_format_fails() {
        let mut skill = create_valid_skill();
        skill.metadata.skill_name = "Invalid-Name!".to_string();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.field == "skill_name" && e.message.contains("Invalid skill name"))
        );
    }

    #[test]
    fn test_valid_skill_names() {
        let valid_names = vec!["test", "test-skill", "test_skill", "test-123", "a"];

        for name in valid_names {
            assert!(
                SkillValidator::is_valid_skill_name(name),
                "Expected '{}' to be valid",
                name
            );
        }
    }

    #[test]
    fn test_invalid_skill_names() {
        let invalid_names = vec![
            "",
            "Test",
            "test skill",
            "test.skill",
            "test@skill",
            "test/skill",
        ];

        for name in invalid_names {
            assert!(
                !SkillValidator::is_valid_skill_name(name),
                "Expected '{}' to be invalid",
                name
            );
        }
    }

    #[test]
    fn test_empty_server_name_fails() {
        let mut skill = create_valid_skill();
        skill.metadata.server_name = String::new();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.field == "server_name"));
    }

    #[test]
    fn test_empty_server_version_fails() {
        let mut skill = create_valid_skill();
        skill.metadata.server_version = String::new();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.field == "server_version"));
    }

    #[test]
    fn test_empty_protocol_version_fails() {
        let mut skill = create_valid_skill();
        skill.metadata.protocol_version = String::new();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.field == "protocol_version"));
    }

    #[test]
    fn test_zero_tools_warns_in_strict_mode() {
        let mut skill = create_valid_skill();
        skill.metadata.tool_count = 0;

        // Normal mode: no warning
        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();
        assert!(report.warnings.is_empty());

        // Strict mode: warning
        let strict_validator = SkillValidator::strict();
        let report = strict_validator.validate(&skill).unwrap();
        assert!(!report.warnings.is_empty());
        assert!(report.warnings.iter().any(|w| w.field == "tool_count"));
    }

    #[test]
    fn test_empty_content_fails() {
        let mut skill = create_valid_skill();
        skill.content = String::new();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(report.errors.iter().any(|e| e.field == "content"));
    }

    #[test]
    fn test_missing_frontmatter_fails() {
        let mut skill = create_valid_skill();
        skill.content = "# No frontmatter here".to_string();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.field == "content" && e.message.contains("frontmatter"))
        );
    }

    #[test]
    fn test_unclosed_frontmatter_fails() {
        let mut skill = create_valid_skill();
        skill.content = "---\nname: test\n# Missing closing ---".to_string();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.field.contains("frontmatter")
                    && e.message.contains("not properly closed"))
        );
    }

    #[test]
    fn test_missing_name_in_frontmatter_fails() {
        let mut skill = create_valid_skill();
        skill.content = "---\ndescription: test\n---\n\nContent".to_string();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.field.contains("frontmatter") && e.message.contains("name"))
        );
    }

    #[test]
    fn test_missing_description_warns_in_strict_mode() {
        let mut skill = create_valid_skill();
        skill.content = "---\nname: test\n---\n\nContent here".to_string();

        // Recalculate checksum
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(skill.content.as_bytes());
        skill.metadata.checksums.skill_md = format!("blake3:{}", hasher.finalize().to_hex());

        // Normal mode: no warning
        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.field.contains("frontmatter") && w.message.contains("description"))
        );

        // Strict mode: warning
        let strict_validator = SkillValidator::strict();
        let report = strict_validator.validate(&skill).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.field.contains("frontmatter") && w.message.contains("description"))
        );
    }

    #[test]
    fn test_short_content_warns_in_strict_mode() {
        let short_content = "---\nname: test\n---\n\nShort";

        // Calculate checksum
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(short_content.as_bytes());
        let checksum = format!("blake3:{}", hasher.finalize().to_hex());

        let skill = ClaudeSkill {
            metadata: ClaudeSkillMetadata {
                skill_name: "test".to_string(),
                server_name: "server".to_string(),
                server_version: "1.0.0".to_string(),
                protocol_version: "1.0".to_string(),
                tool_count: 1,
                generated_at: Utc::now(),
                generator_version: "0.1.0".to_string(),
                checksums: SkillChecksums {
                    skill_md: checksum,
                    reference_md: None,
                },
            },
            content: short_content.to_string(),
        };

        // Normal mode: no warning
        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();
        assert!(
            !report
                .warnings
                .iter()
                .any(|w| w.field == "content" && w.message.contains("short"))
        );

        // Strict mode: warning
        let strict_validator = SkillValidator::strict();
        let report = strict_validator.validate(&skill).unwrap();
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.field == "content" && w.message.contains("short"))
        );
    }

    #[test]
    fn test_checksum_mismatch_fails() {
        let mut skill = create_valid_skill();
        skill.metadata.checksums.skill_md = "blake3:wrong_checksum".to_string();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(
            report
                .errors
                .iter()
                .any(|e| e.field.contains("checksum") && e.message.contains("mismatch"))
        );
    }

    #[test]
    fn test_checksum_without_prefix_warns() {
        let content = "---\nname: test\n---\n\nContent";

        // Calculate checksum without prefix
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(content.as_bytes());
        let checksum_no_prefix = hasher.finalize().to_hex().to_string();

        let skill = ClaudeSkill {
            metadata: ClaudeSkillMetadata {
                skill_name: "test".to_string(),
                server_name: "server".to_string(),
                server_version: "1.0.0".to_string(),
                protocol_version: "1.0".to_string(),
                tool_count: 1,
                generated_at: Utc::now(),
                generator_version: "0.1.0".to_string(),
                checksums: SkillChecksums {
                    skill_md: checksum_no_prefix.clone(),
                    reference_md: None,
                },
            },
            content: content.to_string(),
        };

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        // Should warn about missing prefix
        assert!(
            report
                .warnings
                .iter()
                .any(|w| w.field.contains("checksum") && w.message.contains("blake3:"))
        );
    }

    #[test]
    fn test_validation_report_structure() {
        let report = ValidationReport {
            valid: true,
            errors: vec![],
            warnings: vec![],
        };

        assert!(report.valid);
        assert!(report.errors.is_empty());
        assert!(report.warnings.is_empty());
    }

    #[test]
    fn test_validation_error_equality() {
        let error1 = ValidationError {
            field: "test".to_string(),
            message: "Error".to_string(),
        };
        let error2 = ValidationError {
            field: "test".to_string(),
            message: "Error".to_string(),
        };
        let error3 = ValidationError {
            field: "other".to_string(),
            message: "Error".to_string(),
        };

        assert_eq!(error1, error2);
        assert_ne!(error1, error3);
    }

    #[test]
    fn test_validation_warning_equality() {
        let warn1 = ValidationWarning {
            field: "test".to_string(),
            message: "Warning".to_string(),
        };
        let warn2 = ValidationWarning {
            field: "test".to_string(),
            message: "Warning".to_string(),
        };
        let warn3 = ValidationWarning {
            field: "other".to_string(),
            message: "Warning".to_string(),
        };

        assert_eq!(warn1, warn2);
        assert_ne!(warn1, warn3);
    }

    #[test]
    fn test_default_validator() {
        let validator = SkillValidator::default();
        assert!(!validator.is_strict());
    }

    #[test]
    fn test_multiple_errors_reported() {
        let mut skill = create_valid_skill();
        skill.metadata.skill_name = String::new();
        skill.metadata.server_name = String::new();
        skill.metadata.protocol_version = String::new();

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(!report.valid);
        assert!(report.errors.len() >= 3);
    }

    #[test]
    fn test_windows_line_endings() {
        let content = "---\r\nname: test\r\ndescription: Test\r\n---\r\n\r\nContent";

        // Calculate checksum
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(content.as_bytes());
        let checksum = format!("blake3:{}", hasher.finalize().to_hex());

        let skill = ClaudeSkill {
            metadata: ClaudeSkillMetadata {
                skill_name: "test".to_string(),
                server_name: "server".to_string(),
                server_version: "1.0.0".to_string(),
                protocol_version: "1.0".to_string(),
                tool_count: 1,
                generated_at: Utc::now(),
                generator_version: "0.1.0".to_string(),
                checksums: SkillChecksums {
                    skill_md: checksum,
                    reference_md: None,
                },
            },
            content: content.to_string(),
        };

        let validator = SkillValidator::new();
        let report = validator.validate(&skill).unwrap();

        assert!(report.valid);
    }
}
