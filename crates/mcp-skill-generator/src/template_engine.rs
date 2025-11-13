//! Template engine for skill generation using Handlebars.
//!
//! Provides a wrapper around Handlebars with pre-registered templates
//! for SKILL.md generation. Follows the pattern from mcp-codegen.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_skill_generator::template_engine::TemplateEngine;
//! use mcp_skill_generator::SkillContext;
//! use mcp_core::ServerId;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let engine = TemplateEngine::new()?;
//!
//! let context = SkillContext {
//!     name: "test-skill".to_string(),
//!     description: "A test skill".to_string(),
//!     server_id: ServerId::new("test-server"),
//!     tool_count: 0,
//!     tools: vec![],
//!     generator_version: "0.1.0".to_string(),
//!     generated_at: chrono::Utc::now().to_rfc3339(),
//! };
//!
//! let skill_md = engine.render_skill(&context)?;
//! # Ok(())
//! # }
//! ```

use crate::{Error, Result, SkillContext};
use handlebars::Handlebars;

/// Template engine for skill generation.
///
/// Wraps Handlebars and provides pre-registered templates for
/// generating SKILL.md files from MCP server information.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, allowing it to be used across
/// thread boundaries safely.
///
/// # Examples
///
/// ```
/// use mcp_skill_generator::template_engine::TemplateEngine;
///
/// let engine = TemplateEngine::new().unwrap();
/// assert!(engine.has_template("skill"));
/// ```
#[derive(Debug)]
pub struct TemplateEngine<'a> {
    handlebars: Handlebars<'a>,
}

impl<'a> TemplateEngine<'a> {
    /// Creates a new template engine with registered templates.
    ///
    /// Registers all built-in templates for skill generation.
    ///
    /// # Errors
    ///
    /// Returns error if template registration fails (should not happen
    /// with valid built-in templates).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::template_engine::TemplateEngine;
    ///
    /// let engine = TemplateEngine::new().unwrap();
    /// ```
    pub fn new() -> Result<Self> {
        let mut handlebars = Handlebars::new();

        // Strict mode: fail on missing variables
        handlebars.set_strict_mode(true);

        // Register built-in templates
        Self::register_templates(&mut handlebars)?;

        Ok(Self { handlebars })
    }

    /// Registers all built-in Handlebars templates.
    fn register_templates(handlebars: &mut Handlebars<'a>) -> Result<()> {
        // Skill template: generates SKILL.md
        handlebars
            .register_template_string("skill", include_str!("../templates/skill.yaml.hbs"))
            .map_err(|e| Error::TemplateError {
                message: format!("Failed to register skill template: {e}"),
                source: None,
            })?;

        Ok(())
    }

    /// Renders a skill template with the given context.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Context cannot be serialized
    /// - Template rendering fails
    /// - Required fields are missing
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_generator::template_engine::TemplateEngine;
    /// use mcp_skill_generator::SkillContext;
    /// use mcp_core::ServerId;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let engine = TemplateEngine::new()?;
    ///
    /// let context = SkillContext {
    ///     name: "test".to_string(),
    ///     description: "A test skill".to_string(),
    ///     server_id: ServerId::new("test-server"),
    ///     tool_count: 0,
    ///     tools: vec![],
    ///     generator_version: "0.1.0".to_string(),
    ///     generated_at: chrono::Utc::now().to_rfc3339(),
    /// };
    ///
    /// let result = engine.render_skill(&context)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn render_skill(&self, context: &SkillContext) -> Result<String> {
        // SECURITY: Validate context before rendering to prevent template injection
        context.validate()?;

        self.handlebars
            .render("skill", context)
            .map_err(|e| Error::TemplateError {
                message: format!("Skill template rendering failed: {e}"),
                source: Some(Box::new(e)),
            })
    }

    /// Checks if a template is registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_skill_generator::template_engine::TemplateEngine;
    ///
    /// let engine = TemplateEngine::new().unwrap();
    /// assert!(engine.has_template("skill"));
    /// assert!(!engine.has_template("nonexistent"));
    /// ```
    #[inline]
    #[must_use]
    pub fn has_template(&self, name: &str) -> bool {
        self.handlebars.has_template(name)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ToolContext;
    use mcp_core::{ServerId, ToolName};

    #[test]
    fn test_template_engine_new() {
        let engine = TemplateEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_has_template() {
        let engine = TemplateEngine::new().unwrap();
        assert!(engine.has_template("skill"));
        assert!(!engine.has_template("nonexistent"));
    }

    #[test]
    fn test_render_skill_basic() {
        let engine = TemplateEngine::new().unwrap();

        let context = SkillContext {
            name: "test-skill".to_string(),
            description: "A test skill".to_string(),
            server_id: ServerId::new("test-server"),
            tool_count: 0,
            tools: vec![],
            generator_version: "0.1.0".to_string(),
            generated_at: "2025-11-13T10:00:00Z".to_string(),
        };

        let result = engine.render_skill(&context);
        assert!(result.is_ok());

        let content = result.unwrap();
        assert!(content.contains("test-skill"));
        assert!(content.contains("A test skill"));
    }

    #[test]
    fn test_render_skill_with_tools() {
        let engine = TemplateEngine::new().unwrap();

        let context = SkillContext {
            name: "test-skill".to_string(),
            description: "A test skill".to_string(),
            server_id: ServerId::new("test-server"),
            tool_count: 1,
            tools: vec![ToolContext {
                name: ToolName::new("test_tool"),
                description: "A test tool".to_string(),
                parameters: vec![],
            }],
            generator_version: "0.1.0".to_string(),
            generated_at: "2025-11-13T10:00:00Z".to_string(),
        };

        let result = engine.render_skill(&context);
        assert!(result.is_ok());

        let content = result.unwrap();
        assert!(content.contains("test_tool"));
        assert!(content.contains("A test tool"));
    }
}
