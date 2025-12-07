//! Template engine for code generation using Handlebars.
//!
//! Provides a wrapper around Handlebars with pre-registered templates
//! for TypeScript code generation with progressive loading.
//!
//! # Examples
//!
//! ```
//! use mcp_codegen::template_engine::TemplateEngine;
//! use serde_json::json;
//!
//! let engine = TemplateEngine::new().unwrap();
//! let context = json!({"name": "test"});
//! // let result = engine.render("progressive/tool", &context).unwrap();
//! ```

use handlebars::Handlebars;
use mcp_core::{Error, Result};
use serde::Serialize;

/// Template engine for code generation.
///
/// Wraps Handlebars and provides pre-registered templates for
/// generating TypeScript code from MCP tool schemas using progressive loading.
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
    /// Registers all built-in progressive loading templates.
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

        // Register progressive loading templates
        Self::register_progressive_templates(&mut handlebars)?;

        // Register Claude Agent SDK templates
        Self::register_claude_agent_templates(&mut handlebars)?;

        Ok(Self { handlebars })
    }

    /// Registers progressive loading templates.
    ///
    /// Registers templates for progressive loading pattern where each tool
    /// is a separate file.
    fn register_progressive_templates(handlebars: &mut Handlebars<'a>) -> Result<()> {
        // Tool template: generates a single tool function (progressive loading)
        handlebars
            .register_template_string(
                "progressive/tool",
                include_str!("../templates/progressive/tool.ts.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register progressive tool template: {e}"),
                source: None,
            })?;

        // Index template: generates index.ts with re-exports (progressive loading)
        handlebars
            .register_template_string(
                "progressive/index",
                include_str!("../templates/progressive/index.ts.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register progressive index template: {e}"),
                source: None,
            })?;

        // Runtime bridge template: generates runtime helper for MCP calls
        handlebars
            .register_template_string(
                "progressive/runtime-bridge",
                include_str!("../templates/progressive/runtime-bridge.ts.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register progressive runtime-bridge template: {e}"),
                source: None,
            })?;

        Ok(())
    }

    /// Registers Claude Agent SDK templates.
    ///
    /// Registers templates for Claude Agent SDK format with Zod schemas.
    fn register_claude_agent_templates(handlebars: &mut Handlebars<'a>) -> Result<()> {
        // Tool template: generates a single tool with Zod schema
        handlebars
            .register_template_string(
                "claude_agent/tool",
                include_str!("../templates/claude_agent/tool.ts.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register claude_agent tool template: {e}"),
                source: None,
            })?;

        // Server template: generates MCP server with createSdkMcpServer
        handlebars
            .register_template_string(
                "claude_agent/server",
                include_str!("../templates/claude_agent/server.ts.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register claude_agent server template: {e}"),
                source: None,
            })?;

        // Index template: entry point with exports
        handlebars
            .register_template_string(
                "claude_agent/index",
                include_str!("../templates/claude_agent/index.ts.hbs"),
            )
            .map_err(|e| Error::SerializationError {
                message: format!("Failed to register claude_agent index template: {e}"),
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
    /// let result = engine.render("progressive/tool", &context)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn render<T: Serialize>(&self, template_name: &str, context: &T) -> Result<String> {
        self.handlebars
            .render(template_name, context)
            .map_err(|e| Error::SerializationError {
                message: format!("Template rendering failed: {e}"),
                source: None,
            })
    }

    /// Registers a custom template.
    ///
    /// Allows registering additional templates at runtime.
    ///
    /// # Errors
    ///
    /// Returns error if template string is invalid.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::template_engine::TemplateEngine;
    ///
    /// let mut engine = TemplateEngine::new().unwrap();
    /// engine.register_template_string(
    ///     "custom",
    ///     "// Custom template: {{name}}"
    /// ).unwrap();
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

impl<'a> Default for TemplateEngine<'a> {
    fn default() -> Self {
        Self::new().expect("Failed to create default TemplateEngine")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_template_engine_creation() {
        let engine = TemplateEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_render_progressive_templates() {
        let engine = TemplateEngine::new().unwrap();

        // Test progressive/tool template
        let tool_context = json!({
            "typescript_name": "testTool",
            "description": "Test tool",
            "server_id": "test",
            "name": "test_tool",
            "properties": [],
            "has_required_properties": false,
            "input_schema": {}
        });

        let result = engine.render("progressive/tool", &tool_context);
        if let Err(e) = &result {
            eprintln!("Error rendering template: {}", e);
        }
        assert!(result.is_ok(), "Failed to render: {:?}", result.err());
        assert!(result.unwrap().contains("testTool"));
    }

    #[test]
    fn test_custom_template_registration() {
        let mut engine = TemplateEngine::new().unwrap();

        engine
            .register_template_string("test", "Hello {{name}}")
            .unwrap();

        let context = json!({"name": "World"});
        let result = engine.render("test", &context).unwrap();
        assert_eq!(result, "Hello World");
    }

    #[test]
    fn test_render_nonexistent_template() {
        let engine = TemplateEngine::new().unwrap();
        let context = json!({"name": "test"});
        let result = engine.render("nonexistent", &context);
        assert!(result.is_err());
    }

    #[test]
    fn test_default_trait() {
        let _engine = TemplateEngine::default();
    }
}
