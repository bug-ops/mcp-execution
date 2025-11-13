//! Skill template utilities for SKILL.md generation.
//!
//! Provides helper functions to create a template engine configured
//! for skill generation by reusing the mcp-codegen template engine
//! infrastructure.

use crate::{Error, Result, SkillContext};
use mcp_codegen::template_engine::TemplateEngine;

/// Template name for skill generation.
const SKILL_TEMPLATE_NAME: &str = "skill";

/// Creates a template engine configured for skill generation.
///
/// This function creates a template engine from mcp-codegen and registers
/// the skill template for SKILL.md generation.
///
/// # Errors
///
/// Returns error if template registration fails.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::create_skill_template_engine;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let engine = create_skill_template_engine()?;
/// assert!(engine.has_template("skill"));
/// # Ok(())
/// # }
/// ```
pub fn create_skill_template_engine() -> Result<TemplateEngine<'static>> {
    let mut engine = TemplateEngine::new().map_err(|e| Error::TemplateError {
        message: format!("Failed to create template engine: {e}"),
        source: Some(Box::new(e)),
    })?;

    // Register skill template
    engine
        .register_template_string(SKILL_TEMPLATE_NAME, include_str!("../templates/skill.yaml.hbs"))
        .map_err(|e| Error::TemplateError {
            message: format!("Failed to register skill template: {e}"),
            source: Some(Box::new(e)),
        })?;

    Ok(engine)
}

/// Renders a skill template with the given context.
///
/// This is a convenience function that validates the context before rendering.
///
/// # Errors
///
/// Returns error if:
/// - Context validation fails (template injection detected)
/// - Template rendering fails
///
/// # Security
///
/// This function validates that no template syntax ({{ or }}) exists
/// in user-controlled fields to prevent template injection attacks.
///
/// # Examples
///
/// ```no_run
/// use mcp_skill_generator::{render_skill, SkillContext};
/// use mcp_codegen::template_engine::TemplateEngine;
/// use mcp_core::ServerId;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let mut engine = mcp_skill_generator::create_skill_template_engine()?;
///
/// let context = SkillContext {
///     name: "test".to_string(),
///     description: "A test skill".to_string(),
///     server_id: ServerId::new("test-server"),
///     tool_count: 0,
///     tools: vec![],
///     generator_version: "0.1.0".to_string(),
///     generated_at: "2025-11-13T10:00:00Z".to_string(),
/// };
///
/// let result = render_skill(&engine, &context)?;
/// # Ok(())
/// # }
/// ```
pub fn render_skill(engine: &TemplateEngine, context: &SkillContext) -> Result<String> {
    // SECURITY: Validate context before rendering to prevent template injection
    context.validate()?;

    engine
        .render(SKILL_TEMPLATE_NAME, context)
        .map_err(|e| Error::TemplateError {
            message: format!("Skill template rendering failed: {e}"),
            source: Some(Box::new(e)),
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::ServerId;

    #[test]
    fn test_create_skill_template_engine() {
        let engine = create_skill_template_engine();
        assert!(engine.is_ok());

        let engine = engine.unwrap();
        assert!(engine.has_template("skill"));
    }

    #[test]
    fn test_render_skill_basic() {
        let engine = create_skill_template_engine().unwrap();

        let context = SkillContext {
            name: "test-skill".to_string(),
            description: "A test skill".to_string(),
            server_id: ServerId::new("test-server"),
            tool_count: 0,
            tools: vec![],
            generator_version: "0.1.0".to_string(),
            generated_at: "2025-11-13T10:00:00Z".to_string(),
        };

        let result = render_skill(&engine, &context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("test-skill"));
        assert!(rendered.contains("A test skill"));
        assert!(rendered.contains("test-server"));
    }

    #[test]
    fn test_render_skill_with_tools() {
        use crate::ParameterContext;
        use mcp_core::ToolName;

        let engine = create_skill_template_engine().unwrap();

        let context = SkillContext {
            name: "test-skill".to_string(),
            description: "A test skill".to_string(),
            server_id: ServerId::new("test-server"),
            tool_count: 1,
            tools: vec![crate::ToolContext {
                name: ToolName::new("test_tool"),
                description: "A test tool".to_string(),
                parameters: vec![ParameterContext {
                    name: "param1".to_string(),
                    type_name: "string".to_string(),
                    required: true,
                    description: "First parameter".to_string(),
                }],
            }],
            generator_version: "0.1.0".to_string(),
            generated_at: "2025-11-13T10:00:00Z".to_string(),
        };

        let result = render_skill(&engine, &context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("test_tool"));
        assert!(rendered.contains("param1"));
    }

    #[test]
    fn test_render_skill_validates_context() {
        let engine = create_skill_template_engine().unwrap();

        // Context with template injection attempt
        let context = SkillContext {
            name: "test".to_string(),
            description: "Bad {{injection}} attempt".to_string(),
            server_id: ServerId::new("test-server"),
            tool_count: 0,
            tools: vec![],
            generator_version: "0.1.0".to_string(),
            generated_at: "2025-11-13T10:00:00Z".to_string(),
        };

        let result = render_skill(&engine, &context);
        assert!(result.is_err());
        assert!(result.unwrap_err().is_validation_error());
    }
}
