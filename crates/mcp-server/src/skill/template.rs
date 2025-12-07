//! Template rendering for skill generation.
//!
//! Uses Handlebars templates to render the skill generation prompt.
//! The template is embedded at compile time for reliability.

use handlebars::Handlebars;
use thiserror::Error;

use crate::types::GenerateSkillResult;

/// Errors that can occur during template rendering.
#[derive(Debug, Error)]
pub enum TemplateError {
    /// Template rendering failed.
    #[error("template rendering failed: {0}")]
    RenderFailed(#[from] handlebars::RenderError),

    /// Template registration failed.
    #[error("template registration failed: {0}")]
    RegistrationFailed(#[from] handlebars::TemplateError),
}

/// Embedded Handlebars template for skill generation.
const SKILL_GENERATION_TEMPLATE: &str = include_str!("templates/skill-generation.hbs");

/// Render the skill generation prompt.
///
/// Takes the `GenerateSkillResult` context and renders it using
/// the embedded Handlebars template.
///
/// # Arguments
///
/// * `context` - Skill generation context from `build_skill_context`
///
/// # Returns
///
/// Rendered prompt string for the LLM.
///
/// # Errors
///
/// Returns `TemplateError` if template rendering fails.
///
/// # Examples
///
/// ```no_run
/// use mcp_server::skill::{build_skill_context, render_generation_prompt};
///
/// let context = build_skill_context("github", &[], None);
/// let prompt = render_generation_prompt(&context).unwrap();
/// ```
pub fn render_generation_prompt(context: &GenerateSkillResult) -> Result<String, TemplateError> {
    let mut handlebars = Handlebars::new();

    // Register the template
    handlebars.register_template_string("skill", SKILL_GENERATION_TEMPLATE)?;

    // Render with context
    let rendered = handlebars.render("skill", context)?;

    Ok(rendered)
}

/// Alternative: Use the pre-built prompt from context.
///
/// The `GenerateSkillResult` already contains a `generation_prompt` field
/// built by `build_skill_context`. This function is provided for cases
/// where custom template rendering is needed.
#[allow(dead_code)]
pub fn get_prebuilt_prompt(context: &GenerateSkillResult) -> &str {
    &context.generation_prompt
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{SkillCategory, SkillTool, ToolExample};

    fn create_test_context() -> GenerateSkillResult {
        GenerateSkillResult {
            server_id: "test".to_string(),
            skill_name: "test-progressive".to_string(),
            server_description: Some("Test server".to_string()),
            categories: vec![SkillCategory {
                name: "test".to_string(),
                display_name: "Test".to_string(),
                tools: vec![SkillTool {
                    name: "test_tool".to_string(),
                    typescript_name: "testTool".to_string(),
                    description: "Test tool description".to_string(),
                    keywords: vec!["test".to_string()],
                    required_params: vec!["param1".to_string()],
                    optional_params: vec![],
                }],
            }],
            tool_count: 1,
            example_tools: vec![ToolExample {
                tool_name: "test_tool".to_string(),
                description: "Test tool".to_string(),
                cli_command: "node test.ts".to_string(),
                params_json: "{}".to_string(),
            }],
            generation_prompt: "Pre-built prompt".to_string(),
            output_path: "~/.claude/skills/test/SKILL.md".to_string(),
        }
    }

    #[test]
    fn test_render_generation_prompt() {
        let context = create_test_context();
        let result = render_generation_prompt(&context);

        match result {
            Ok(prompt) => {
                // Verify key sections are present
                assert!(prompt.contains("test"));
                assert!(prompt.contains("SKILL.md"));
            }
            Err(e) => panic!("Template rendering failed: {e}"),
        }
    }

    #[test]
    fn test_get_prebuilt_prompt() {
        let context = create_test_context();
        let prompt = get_prebuilt_prompt(&context);

        assert_eq!(prompt, "Pre-built prompt");
    }
}
