//! Template engine for code generation using Handlebars.
//!
//! Provides a wrapper around Handlebars with pre-registered templates
//! for TypeScript code generation.
//!
//! # Examples
//!
//! ```
//! use mcp_codegen::template_engine::TemplateEngine;
//! use serde_json::json;
//!
//! let engine = TemplateEngine::new().unwrap();
//! let context = json!({"name": "test"});
//! // let result = engine.render("tool", &context).unwrap();
//! ```

use handlebars::Handlebars;
use mcp_core::{Error, Result};
use serde::Serialize;

/// Template engine for code generation.
///
/// Wraps Handlebars and provides pre-registered templates for
/// generating TypeScript code from MCP tool schemas.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, allowing it to be used across
/// thread boundaries safely.
#[derive(Debug)]
pub struct TemplateEngine<'a> {
    handlebars: Handlebars<'a>,
}

impl<'a> TemplateEngine<'a> {
    /// Creates a new template engine with registered templates.
    ///
    /// Registers all built-in templates for TypeScript code generation.
    ///
    /// # Errors
    ///
    /// Returns error if template registration fails (should not happen
    /// with valid built-in templates).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::template_engine::TemplateEngine;
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
    ///
    /// Templates are registered based on enabled features:
    /// - `wasm` feature: WASM-specific templates from templates/wasm/
    /// - `skills` feature: Skills-specific templates from templates/skills/
    #[allow(unused_variables)]
    fn register_templates(handlebars: &mut Handlebars<'a>) -> Result<()> {
        // Register WASM templates if feature enabled
        #[cfg(feature = "wasm")]
        Self::register_wasm_templates(handlebars)?;

        // Register Skills templates if feature enabled
        #[cfg(feature = "skills")]
        Self::register_skills_templates(handlebars)?;

        Ok(())
    }

    /// Registers WASM-specific templates.
    #[cfg(feature = "wasm")]
    fn register_wasm_templates(handlebars: &mut Handlebars<'a>) -> Result<()> {
        // Tool template: generates a single tool function
        handlebars
            .register_template_string("tool", include_str!("../templates/wasm/tool.ts.hbs"))
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register WASM tool template: {}", e),
                source: None,
            })?;

        // Manifest template: generates manifest.json
        handlebars
            .register_template_string(
                "manifest",
                include_str!("../templates/wasm/manifest.json.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register WASM manifest template: {}", e),
                source: None,
            })?;

        // Types template: generates types.ts with shared types
        handlebars
            .register_template_string("types", include_str!("../templates/wasm/types.ts.hbs"))
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register WASM types template: {}", e),
                source: None,
            })?;

        // Index template: generates index.ts with exports
        handlebars
            .register_template_string("index", include_str!("../templates/wasm/index.ts.hbs"))
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register WASM index template: {}", e),
                source: None,
            })?;

        Ok(())
    }

    /// Registers Skills-specific templates.
    #[cfg(feature = "skills")]
    fn register_skills_templates(handlebars: &mut Handlebars<'a>) -> Result<()> {
        // Claude skill template: generates SKILL.md with YAML frontmatter
        handlebars
            .register_template_string(
                "claude_skill",
                include_str!("../templates/claude/skill.md.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register Claude skill template: {}", e),
                source: None,
            })?;

        // Claude reference template: generates REFERENCE.md with detailed API docs
        handlebars
            .register_template_string(
                "claude_reference",
                include_str!("../templates/claude/reference.md.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register Claude reference template: {}", e),
                source: None,
            })?;

        // Multi-file skill template: generates SKILL.md that references scripts/
        handlebars
            .register_template_string(
                "skill_md_multifile",
                include_str!("../templates/skills/skill_multifile.md.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register skill_md_multifile template: {}", e),
                source: None,
            })?;

        // Reference MD template (reuse existing claude_reference as reference_md alias)
        handlebars
            .register_template_string(
                "reference_md",
                include_str!("../templates/claude/reference.md.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register reference_md template: {}", e),
                source: None,
            })?;

        // Categorized skill templates
        handlebars
            .register_template_string(
                "skill_categorized_md",
                include_str!("../templates/skills/skill_categorized.md.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register skill_categorized_md template: {}", e),
                source: None,
            })?;

        handlebars
            .register_template_string(
                "category_md",
                include_str!("../templates/skills/category.md.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register category_md template: {}", e),
                source: None,
            })?;

        handlebars
            .register_template_string(
                "manifest_yaml",
                include_str!("../templates/skills/manifest.yaml.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register manifest_yaml template: {}", e),
                source: None,
            })?;

        Ok(())
    }

    /// Renders a template with the given context.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Template name is not registered
    /// - Context cannot be serialized
    /// - Template rendering fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::template_engine::TemplateEngine;
    /// use serde_json::json;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let engine = TemplateEngine::new()?;
    /// let context = json!({"name": "test", "description": "A test tool"});
    /// let result = engine.render("tool", &context)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn render<T: Serialize>(&self, template_name: &str, context: &T) -> Result<String> {
        self.handlebars
            .render(template_name, context)
            .map_err(|e| Error::SerializationError {
                message: format!("Template rendering failed: {}", e),
                source: None,
            })
    }

    /// Checks if a template is registered.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::template_engine::TemplateEngine;
    ///
    /// let engine = TemplateEngine::new().unwrap();
    /// assert!(engine.has_template("tool"));
    /// assert!(!engine.has_template("nonexistent"));
    /// ```
    #[inline]
    #[must_use]
    pub fn has_template(&self, name: &str) -> bool {
        self.handlebars.has_template(name)
    }

    /// Registers a custom template from a string.
    ///
    /// This allows other crates to register their own templates
    /// using the same template engine.
    ///
    /// # Errors
    ///
    /// Returns error if template registration fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::template_engine::TemplateEngine;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut engine = TemplateEngine::new()?;
    /// engine.register_template_string("custom", "Hello {{name}}!")?;
    /// assert!(engine.has_template("custom"));
    /// # Ok(())
    /// # }
    /// ```
    pub fn register_template_string(&mut self, name: &str, template: &str) -> Result<()> {
        self.handlebars
            .register_template_string(name, template)
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register template '{name}': {e}"),
                source: None,
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_template_engine_new() {
        let engine = TemplateEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_has_template() {
        let engine = TemplateEngine::new().unwrap();
        assert!(engine.has_template("tool"));
        assert!(engine.has_template("manifest"));
        assert!(engine.has_template("types"));
        assert!(engine.has_template("index"));
        assert!(!engine.has_template("nonexistent"));
    }

    #[test]
    fn test_render_with_invalid_template() {
        let engine = TemplateEngine::new().unwrap();
        let context = json!({"name": "test"});
        let result = engine.render("nonexistent", &context);
        assert!(result.is_err());
    }

    #[test]
    fn test_register_template_string() {
        let mut engine = TemplateEngine::new().unwrap();

        // Register a custom template
        let result = engine.register_template_string("custom", "Hello {{name}}!");
        assert!(result.is_ok());

        // Verify template is registered
        assert!(engine.has_template("custom"));

        // Render the custom template
        let context = json!({"name": "World"});
        let result = engine.render("custom", &context).unwrap();
        assert_eq!(result, "Hello World!");
    }
}
