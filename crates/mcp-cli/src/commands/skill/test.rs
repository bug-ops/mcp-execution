//! Skill testing command.
//!
//! Tests skills for validity and integrity using the skill validation framework.

use anyhow::{Context, Result};
use clap::Parser;
use colored::Colorize;
use mcp_core::SkillName;
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_skill_store::{ClaudeSkill, SkillStore, SkillValidator, ValidationReport};
use serde::Serialize;

/// Test skill validity and integrity.
///
/// Validates skill metadata, content, and checksums to ensure
/// skills are properly formatted and haven't been corrupted.
///
/// # Examples
///
/// ```bash
/// # Test a single skill
/// mcp-cli skill test my-skill
///
/// # Test all skills
/// mcp-cli skill test --all
///
/// # Test with strict validation
/// mcp-cli skill test my-skill --strict
///
/// # Test with JSON output
/// mcp-cli skill test my-skill --format json
/// ```
#[derive(Debug, Parser)]
pub struct TestCommand {
    /// Skill name to test
    skill_name: Option<String>,

    /// Test all skills
    #[arg(long)]
    all: bool,

    /// Use strict validation mode
    ///
    /// Strict mode enables additional warnings for best practices:
    /// - Warns if no tools are specified
    /// - Warns about missing descriptions
    /// - Warns about short content
    #[arg(long)]
    strict: bool,
}

/// Test result for a single skill.
#[derive(Debug, Serialize)]
struct SkillTestResult {
    /// Skill name
    skill_name: String,
    /// Whether the skill is valid
    valid: bool,
    /// Number of errors
    error_count: usize,
    /// Number of warnings
    warning_count: usize,
    /// List of errors
    #[serde(skip_serializing_if = "Vec::is_empty")]
    errors: Vec<ValidationIssue>,
    /// List of warnings
    #[serde(skip_serializing_if = "Vec::is_empty")]
    warnings: Vec<ValidationIssue>,
}

/// Validation issue (error or warning).
#[derive(Debug, Serialize)]
struct ValidationIssue {
    /// Field that failed validation
    field: String,
    /// Issue message
    message: String,
}

/// Test summary for multiple skills.
#[derive(Debug, Serialize)]
struct TestSummary {
    /// Total number of skills tested
    total: usize,
    /// Number of passing skills
    passed: usize,
    /// Number of failing skills
    failed: usize,
    /// Individual test results
    #[serde(skip_serializing_if = "Vec::is_empty")]
    results: Vec<SkillTestResult>,
}

impl TestCommand {
    /// Executes the test command.
    ///
    /// # Errors
    ///
    /// Returns an error if skill loading or validation fails.
    pub fn execute(&self, output_format: OutputFormat) -> Result<ExitCode> {
        if self.all {
            self.test_all_skills(output_format)
        } else if let Some(name) = &self.skill_name {
            self.test_skill(name, output_format)
        } else {
            anyhow::bail!("Either provide skill name or use --all flag")
        }
    }

    /// Tests a single skill.
    fn test_skill(&self, name: &str, output_format: OutputFormat) -> Result<ExitCode> {
        let skill_name = SkillName::new(name).context("invalid skill name")?;

        // Load skill from store
        let store = SkillStore::new_claude().context("failed to initialize skill store")?;
        let loaded = store
            .load_claude_skill(&skill_name)
            .with_context(|| format!("failed to load skill '{name}'"))?;

        // Create skill for validation
        let skill = ClaudeSkill {
            metadata: loaded.metadata,
            content: loaded.skill_md,
        };

        // Create validator
        let validator = if self.strict {
            SkillValidator::strict()
        } else {
            SkillValidator::new()
        };

        // Validate skill
        let report = validator
            .validate(&skill)
            .with_context(|| format!("failed to validate skill '{name}'"))?;

        // Convert to test result
        let test_result = Self::report_to_result(name, &report);

        // Output result
        match output_format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&test_result)?;
                println!("{json}");
            }
            OutputFormat::Text => {
                Self::print_text_result(&test_result);
            }
            OutputFormat::Pretty => {
                Self::print_pretty_result(&test_result);
            }
        }

        // Return exit code based on validity
        if test_result.valid {
            Ok(ExitCode::SUCCESS)
        } else {
            Ok(ExitCode::ERROR)
        }
    }

    /// Tests all skills in the skill store.
    fn test_all_skills(&self, output_format: OutputFormat) -> Result<ExitCode> {
        let store = SkillStore::new_claude().context("failed to initialize skill store")?;
        let skills = store
            .list_claude_skills()
            .context("failed to list skills")?;

        if skills.is_empty() {
            println!("No skills found");
            return Ok(ExitCode::SUCCESS);
        }

        let validator = if self.strict {
            SkillValidator::strict()
        } else {
            SkillValidator::new()
        };

        let mut results = Vec::new();
        let mut passed = 0;
        let mut failed = 0;

        for skill_info in &skills {
            let skill_name =
                SkillName::new(&skill_info.skill_name).context("invalid skill name in metadata")?;

            // Load and validate skill
            match store.load_claude_skill(&skill_name) {
                Ok(loaded) => {
                    let skill = ClaudeSkill {
                        metadata: loaded.metadata,
                        content: loaded.skill_md,
                    };

                    match validator.validate(&skill) {
                        Ok(report) => {
                            let test_result =
                                Self::report_to_result(&skill_info.skill_name, &report);
                            if test_result.valid {
                                passed += 1;
                            } else {
                                failed += 1;
                            }
                            results.push(test_result);
                        }
                        Err(e) => {
                            eprintln!(
                                "Failed to validate skill '{}': {}",
                                skill_info.skill_name, e
                            );
                            failed += 1;
                        }
                    }
                }
                Err(e) => {
                    eprintln!("Failed to load skill '{}': {}", skill_info.skill_name, e);
                    failed += 1;
                }
            }
        }

        let summary = TestSummary {
            total: skills.len(),
            passed,
            failed,
            results,
        };

        // Output summary
        match output_format {
            OutputFormat::Json => {
                let json = serde_json::to_string_pretty(&summary)?;
                println!("{json}");
            }
            OutputFormat::Text => {
                Self::print_text_summary(&summary);
            }
            OutputFormat::Pretty => {
                Self::print_pretty_summary(&summary);
            }
        }

        // Return exit code based on results
        if failed > 0 {
            Ok(ExitCode::ERROR)
        } else {
            Ok(ExitCode::SUCCESS)
        }
    }

    /// Converts a validation report to a test result.
    fn report_to_result(skill_name: &str, report: &ValidationReport) -> SkillTestResult {
        let errors = report
            .errors
            .iter()
            .map(|e| ValidationIssue {
                field: e.field.clone(),
                message: e.message.clone(),
            })
            .collect();

        let warnings = report
            .warnings
            .iter()
            .map(|w| ValidationIssue {
                field: w.field.clone(),
                message: w.message.clone(),
            })
            .collect();

        SkillTestResult {
            skill_name: skill_name.to_string(),
            valid: report.valid,
            error_count: report.errors.len(),
            warning_count: report.warnings.len(),
            errors,
            warnings,
        }
    }

    /// Prints test result in text format.
    fn print_text_result(result: &SkillTestResult) {
        println!("Skill: {}", result.skill_name);
        println!("Valid: {}", result.valid);
        println!("Errors: {}", result.error_count);
        println!("Warnings: {}", result.warning_count);

        for error in &result.errors {
            println!("  ERROR [{}]: {}", error.field, error.message);
        }
        for warning in &result.warnings {
            println!("  WARN [{}]: {}", warning.field, warning.message);
        }
    }

    /// Prints test result in pretty format.
    fn print_pretty_result(result: &SkillTestResult) {
        println!("{}", "=".repeat(60));
        println!("Skill Test Report: {}", result.skill_name.bold());
        println!("{}", "=".repeat(60));

        if result.valid {
            println!("\n{} {}", "✓".green().bold(), "VALID".green().bold());
        } else {
            println!("\n{} {}", "✗".red().bold(), "INVALID".red().bold());
        }

        if !result.errors.is_empty() {
            println!("\n{} Errors:", "❌".red());
            for error in &result.errors {
                println!("  • {}: {}", error.field.yellow(), error.message);
            }
        }

        if !result.warnings.is_empty() {
            println!("\n{} Warnings:", "⚠️".yellow());
            for warning in &result.warnings {
                println!("  • {}: {}", warning.field.cyan(), warning.message);
            }
        }

        if result.errors.is_empty() && result.warnings.is_empty() {
            println!("\n{} No issues found!", "🎉".green());
        }

        println!("\n{}", "=".repeat(60));
    }

    /// Prints test summary in text format.
    fn print_text_summary(summary: &TestSummary) {
        println!("Total: {}", summary.total);
        println!("Passed: {}", summary.passed);
        println!("Failed: {}", summary.failed);

        for result in &summary.results {
            println!(
                "\n{}: {}",
                result.skill_name,
                if result.valid { "PASS" } else { "FAIL" }
            );
            if !result.errors.is_empty() {
                println!("  Errors: {}", result.error_count);
            }
            if !result.warnings.is_empty() {
                println!("  Warnings: {}", result.warning_count);
            }
        }
    }

    /// Prints test summary in pretty format.
    fn print_pretty_summary(summary: &TestSummary) {
        println!("{}", "=".repeat(60));
        println!("{}", "Skill Test Summary".bold());
        println!("{}", "=".repeat(60));

        println!(
            "\nTested {} skill(s): {} passed, {} failed",
            summary.total.to_string().bold(),
            summary.passed.to_string().green(),
            summary.failed.to_string().red()
        );

        if !summary.results.is_empty() {
            println!("\n{}", "Results:".bold());
            for result in &summary.results {
                let status = if result.valid {
                    "✓ PASS".green()
                } else {
                    "✗ FAIL".red()
                };
                println!("  {} {}", status, result.skill_name);

                if result.error_count > 0 {
                    println!(
                        "    {} {} error(s)",
                        "❌".red(),
                        result.error_count.to_string().red()
                    );
                }
                if result.warning_count > 0 {
                    println!(
                        "    {} {} warning(s)",
                        "⚠️".yellow(),
                        result.warning_count.to_string().yellow()
                    );
                }
            }
        }

        println!("\n{}", "=".repeat(60));

        if summary.failed > 0 {
            println!(
                "\n{} {} skill(s) failed validation",
                "⚠️".yellow(),
                summary.failed
            );
        } else {
            println!("\n{} All skills passed!", "🎉".green());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_test_command_creation() {
        let cmd = TestCommand {
            skill_name: Some("test-skill".to_string()),
            all: false,
            strict: false,
        };

        assert_eq!(cmd.skill_name, Some("test-skill".to_string()));
        assert!(!cmd.all);
        assert!(!cmd.strict);
    }

    #[test]
    fn test_test_command_all_flag() {
        let cmd = TestCommand {
            skill_name: None,
            all: true,
            strict: false,
        };

        assert!(cmd.all);
        assert!(!cmd.strict);
    }

    #[test]
    fn test_test_command_strict_flag() {
        let cmd = TestCommand {
            skill_name: Some("test-skill".to_string()),
            all: false,
            strict: true,
        };

        assert!(cmd.strict);
    }

    #[test]
    fn test_validation_issue_serialization() {
        let issue = ValidationIssue {
            field: "skill_name".to_string(),
            message: "Invalid skill name".to_string(),
        };

        let json = serde_json::to_string(&issue).unwrap();
        assert!(json.contains("skill_name"));
        assert!(json.contains("Invalid skill name"));
    }

    #[test]
    fn test_skill_test_result_serialization() {
        let result = SkillTestResult {
            skill_name: "test-skill".to_string(),
            valid: true,
            error_count: 0,
            warning_count: 1,
            errors: vec![],
            warnings: vec![ValidationIssue {
                field: "content".to_string(),
                message: "Content is short".to_string(),
            }],
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-skill"));
        assert!(json.contains("true"));
        assert!(json.contains("Content is short"));
    }

    #[test]
    fn test_test_summary_serialization() {
        let summary = TestSummary {
            total: 3,
            passed: 2,
            failed: 1,
            results: vec![
                SkillTestResult {
                    skill_name: "skill1".to_string(),
                    valid: true,
                    error_count: 0,
                    warning_count: 0,
                    errors: vec![],
                    warnings: vec![],
                },
                SkillTestResult {
                    skill_name: "skill2".to_string(),
                    valid: false,
                    error_count: 1,
                    warning_count: 0,
                    errors: vec![ValidationIssue {
                        field: "skill_name".to_string(),
                        message: "Empty name".to_string(),
                    }],
                    warnings: vec![],
                },
            ],
        };

        let json = serde_json::to_string(&summary).unwrap();
        assert!(json.contains("\"total\":3"));
        assert!(json.contains("\"passed\":2"));
        assert!(json.contains("\"failed\":1"));
        assert!(json.contains("skill1"));
        assert!(json.contains("skill2"));
    }

    #[test]
    fn test_report_to_result_valid() {
        let report = ValidationReport {
            valid: true,
            errors: vec![],
            warnings: vec![],
        };

        let result = TestCommand::report_to_result("test-skill", &report);

        assert_eq!(result.skill_name, "test-skill");
        assert!(result.valid);
        assert_eq!(result.error_count, 0);
        assert_eq!(result.warning_count, 0);
    }

    #[test]
    fn test_report_to_result_with_errors() {
        use mcp_skill_store::ValidationError;

        let report = ValidationReport {
            valid: false,
            errors: vec![ValidationError {
                field: "skill_name".to_string(),
                message: "Name is empty".to_string(),
            }],
            warnings: vec![],
        };

        let result = TestCommand::report_to_result("bad-skill", &report);

        assert_eq!(result.skill_name, "bad-skill");
        assert!(!result.valid);
        assert_eq!(result.error_count, 1);
        assert_eq!(result.warnings.len(), 0);
        assert_eq!(result.errors[0].field, "skill_name");
    }

    #[test]
    fn test_report_to_result_with_warnings() {
        use mcp_skill_store::ValidationWarning;

        let report = ValidationReport {
            valid: true,
            errors: vec![],
            warnings: vec![ValidationWarning {
                field: "tool_count".to_string(),
                message: "No tools".to_string(),
            }],
        };

        let result = TestCommand::report_to_result("warning-skill", &report);

        assert!(result.valid);
        assert_eq!(result.warning_count, 1);
        assert_eq!(result.warnings[0].field, "tool_count");
    }

    #[test]
    fn test_execute_requires_name_or_all() {
        let cmd = TestCommand {
            skill_name: None,
            all: false,
            strict: false,
        };

        let result = cmd.execute(OutputFormat::Json);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("provide skill name or use --all")
        );
    }
}
