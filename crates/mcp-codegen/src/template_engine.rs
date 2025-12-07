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

    // ========================================================================
    // Template Engine Creation Tests
    // ========================================================================

    #[test]
    fn test_template_engine_creation() {
        let engine = TemplateEngine::new();
        assert!(engine.is_ok());
    }

    #[test]
    fn test_default_trait() {
        let _engine = TemplateEngine::default();
    }

    // ========================================================================
    // Progressive Loading Template Tests
    // ========================================================================

    #[test]
    fn test_render_progressive_tool_template() {
        let engine = TemplateEngine::new().unwrap();

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
        assert!(result.is_ok(), "Failed to render: {:?}", result.err());

        let rendered = result.unwrap();
        assert!(rendered.contains("testTool"));
        assert!(rendered.contains("Test tool"));
        assert!(rendered.contains("test_tool"));
    }

    #[test]
    fn test_render_progressive_tool_with_properties() {
        let engine = TemplateEngine::new().unwrap();

        let tool_context = json!({
            "typescript_name": "createIssue",
            "description": "Create a GitHub issue",
            "server_id": "github",
            "name": "create_issue",
            "properties": [
                {
                    "name": "title",
                    "typescript_type": "string",
                    "required": true,
                    "description": "Issue title"
                },
                {
                    "name": "body",
                    "typescript_type": "string",
                    "required": false,
                    "description": "Issue body"
                }
            ],
            "has_required_properties": true,
            "input_schema": {
                "type": "object",
                "properties": {
                    "title": { "type": "string" },
                    "body": { "type": "string" }
                },
                "required": ["title"]
            }
        });

        let result = engine.render("progressive/tool", &tool_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("createIssue"));
        assert!(rendered.contains("Create a GitHub issue"));
    }

    #[test]
    fn test_render_progressive_index_template() {
        let engine = TemplateEngine::new().unwrap();

        let index_context = json!({
            "server_name": "GitHub MCP Server",
            "server_version": "1.0.0",
            "tool_count": 2,
            "tools": [
                {
                    "typescript_name": "createIssue",
                    "description": "Create an issue"
                },
                {
                    "typescript_name": "listRepos",
                    "description": "List repositories"
                }
            ]
        });

        let result = engine.render("progressive/index", &index_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("GitHub MCP Server"));
        assert!(rendered.contains("1.0.0"));
        assert!(rendered.contains("createIssue"));
        assert!(rendered.contains("listRepos"));
        assert!(rendered.contains("2 tools"));
    }

    #[test]
    fn test_render_progressive_runtime_bridge_template() {
        let engine = TemplateEngine::new().unwrap();

        // Runtime bridge doesn't need context
        let context = json!({});

        let result = engine.render("progressive/runtime-bridge", &context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("callMCPTool"));
        assert!(rendered.contains("closeAllConnections"));
        assert!(rendered.contains("MCP Runtime Bridge"));
    }

    // ========================================================================
    // Claude Agent SDK Template Tests
    // ========================================================================

    #[test]
    fn test_render_claude_agent_tool_template() {
        let engine = TemplateEngine::new().unwrap();

        let tool_context = json!({
            "typescript_name": "searchFiles",
            "name": "search_files",
            "description": "Search for files in repository",
            "pascal_name": "SearchFiles",
            "properties": [
                {
                    "name": "query",
                    "zod_type": "string",
                    "zod_modifiers": [],
                    "description": "Search query",
                    "required": true
                },
                {
                    "name": "limit",
                    "zod_type": "number",
                    "zod_modifiers": [".int()", ".positive()"],
                    "description": "Maximum results",
                    "required": false
                }
            ]
        });

        let result = engine.render("claude_agent/tool", &tool_context);
        assert!(result.is_ok(), "Failed to render: {:?}", result.err());

        let rendered = result.unwrap();
        assert!(rendered.contains("import { tool }"));
        assert!(rendered.contains("import { z }"));
        assert!(rendered.contains("searchFiles"));
        assert!(rendered.contains("search_files"));
        assert!(rendered.contains("Search for files"));
        assert!(rendered.contains("z.string()"));
        assert!(rendered.contains("z.number()"));
        assert!(rendered.contains(".optional()"));
    }

    #[test]
    fn test_render_claude_agent_tool_with_complex_types() {
        let engine = TemplateEngine::new().unwrap();

        let tool_context = json!({
            "typescript_name": "analyzeCode",
            "name": "analyze_code",
            "description": "Analyze source code quality",
            "pascal_name": "AnalyzeCode",
            "properties": [
                {
                    "name": "files",
                    "zod_type": "array",
                    "zod_modifiers": [".of(z.string())"],
                    "description": "File paths to analyze",
                    "required": true
                },
                {
                    "name": "options",
                    "zod_type": "object",
                    "zod_modifiers": [],
                    "description": "Analysis options",
                    "required": false
                }
            ]
        });

        let result = engine.render("claude_agent/tool", &tool_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("analyzeCode"));
        assert!(rendered.contains("z.array()"));
        assert!(rendered.contains("z.object()"));
    }

    #[test]
    fn test_render_claude_agent_server_template() {
        let engine = TemplateEngine::new().unwrap();

        let server_context = json!({
            "server_name": "filesystem",
            "server_version": "2.0.0",
            "server_variable_name": "filesystem",
            "tools": [
                {
                    "typescript_name": "readFile"
                },
                {
                    "typescript_name": "writeFile"
                },
                {
                    "typescript_name": "deleteFile"
                }
            ]
        });

        let result = engine.render("claude_agent/server", &server_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("import { createSdkMcpServer }"));
        assert!(rendered.contains("filesystem"));
        assert!(rendered.contains("2.0.0"));
        assert!(rendered.contains("readFile"));
        assert!(rendered.contains("writeFile"));
        assert!(rendered.contains("deleteFile"));
        assert!(rendered.contains("filesystemServer"));
    }

    #[test]
    fn test_render_claude_agent_index_template() {
        let engine = TemplateEngine::new().unwrap();

        let index_context = json!({
            "server_name": "database",
            "server_version": "3.1.0",
            "server_variable_name": "database",
            "tool_count": 4,
            "tools": [
                {
                    "typescript_name": "query"
                },
                {
                    "typescript_name": "insert"
                },
                {
                    "typescript_name": "update"
                },
                {
                    "typescript_name": "delete"
                }
            ]
        });

        let result = engine.render("claude_agent/index", &index_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("database"));
        assert!(rendered.contains("3.1.0"));
        assert!(rendered.contains("databaseServer"));
        assert!(rendered.contains("toolCount: 4"));
        assert!(rendered.contains("query"));
        assert!(rendered.contains("insert"));
        assert!(rendered.contains("update"));
        assert!(rendered.contains("delete"));
    }

    // ========================================================================
    // Error Handling Tests
    // ========================================================================

    #[test]
    fn test_render_nonexistent_template() {
        let engine = TemplateEngine::new().unwrap();
        let context = json!({"name": "test"});

        let result = engine.render("nonexistent/template", &context);
        assert!(result.is_err());

        let err = result.unwrap_err();
        assert!(matches!(err, Error::SerializationError { .. }));
    }

    #[test]
    fn test_render_with_missing_required_field() {
        let engine = TemplateEngine::new().unwrap();

        // Missing required field "typescript_name"
        let invalid_context = json!({
            "description": "Test tool",
            "server_id": "test"
        });

        let result = engine.render("progressive/tool", &invalid_context);
        assert!(result.is_err(), "Should fail with missing required field");
    }

    #[test]
    fn test_render_with_empty_context() {
        let engine = TemplateEngine::new().unwrap();
        let empty_context = json!({});

        let result = engine.render("progressive/tool", &empty_context);
        assert!(result.is_err());
    }

    #[test]
    fn test_register_invalid_template_syntax() {
        let mut engine = TemplateEngine::new().unwrap();

        // Invalid Handlebars syntax: unclosed tag
        let result = engine.register_template_string("invalid", "Hello {{name");

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            Error::SerializationError { .. }
        ));
    }

    #[test]
    fn test_register_template_with_invalid_helper() {
        let mut engine = TemplateEngine::new().unwrap();

        // Template with non-existent helper
        let result = engine.register_template_string("bad_helper", "{{nonexistent_helper name}}");

        // Registration might succeed, but rendering will fail
        assert!(result.is_ok());
    }

    // ========================================================================
    // Edge Case Tests
    // ========================================================================

    #[test]
    fn test_render_with_empty_tool_array() {
        let engine = TemplateEngine::new().unwrap();

        let context = json!({
            "server_name": "Empty Server",
            "server_version": "1.0.0",
            "tool_count": 0,
            "tools": []
        });

        let result = engine.render("progressive/index", &context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("Empty Server"));
        assert!(rendered.contains("0 tools"));
    }

    #[test]
    fn test_render_with_special_characters_in_description() {
        let engine = TemplateEngine::new().unwrap();

        let tool_context = json!({
            "typescript_name": "testTool",
            "description": "A tool with \"quotes\" and 'apostrophes' & special chars: <>&",
            "server_id": "test",
            "name": "test_tool",
            "properties": [],
            "has_required_properties": false,
            "input_schema": {}
        });

        let result = engine.render("progressive/tool", &tool_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("quotes"));
        assert!(rendered.contains("apostrophes"));
    }

    #[test]
    fn test_render_with_unicode_characters() {
        let engine = TemplateEngine::new().unwrap();

        let tool_context = json!({
            "typescript_name": "unicodeTool",
            "description": "支持中文 и Русский язык 🚀",
            "server_id": "test",
            "name": "unicode_tool",
            "properties": [],
            "has_required_properties": false,
            "input_schema": {}
        });

        let result = engine.render("progressive/tool", &tool_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("支持中文"));
        assert!(rendered.contains("Русский"));
        assert!(rendered.contains("🚀"));
    }

    #[test]
    fn test_render_with_very_long_description() {
        let engine = TemplateEngine::new().unwrap();

        let long_description = "A".repeat(5000);
        let tool_context = json!({
            "typescript_name": "longDescriptionTool",
            "description": long_description,
            "server_id": "test",
            "name": "long_tool",
            "properties": [],
            "has_required_properties": false,
            "input_schema": {}
        });

        let result = engine.render("progressive/tool", &tool_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.len() > 5000);
    }

    #[test]
    fn test_render_with_nested_properties() {
        let engine = TemplateEngine::new().unwrap();

        let tool_context = json!({
            "typescript_name": "complexTool",
            "name": "complex_tool",
            "description": "Tool with nested schema",
            "pascal_name": "ComplexTool",
            "properties": [
                {
                    "name": "config",
                    "zod_type": "object",
                    "zod_modifiers": [".shape({ nested: z.string() })"],
                    "description": "Configuration object",
                    "required": true
                }
            ]
        });

        let result = engine.render("claude_agent/tool", &tool_context);
        assert!(result.is_ok());
    }

    #[test]
    fn test_render_claude_agent_tool_without_properties() {
        let engine = TemplateEngine::new().unwrap();

        let tool_context = json!({
            "typescript_name": "noParamsTool",
            "name": "no_params",
            "description": "Tool with no parameters",
            "pascal_name": "NoParamsTool",
            "properties": []
        });

        let result = engine.render("claude_agent/tool", &tool_context);
        assert!(result.is_ok());

        let rendered = result.unwrap();
        assert!(rendered.contains("noParamsTool"));
    }

    // ========================================================================
    // Custom Template Tests
    // ========================================================================

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
    fn test_custom_template_with_loops() {
        let mut engine = TemplateEngine::new().unwrap();

        engine
            .register_template_string("list", "Items: {{#each items}}{{this}}, {{/each}}")
            .unwrap();

        let context = json!({"items": ["a", "b", "c"]});
        let result = engine.render("list", &context).unwrap();
        assert_eq!(result, "Items: a, b, c, ");
    }

    #[test]
    fn test_custom_template_with_conditionals() {
        let mut engine = TemplateEngine::new().unwrap();

        engine
            .register_template_string("conditional", "{{#if enabled}}ON{{else}}OFF{{/if}}")
            .unwrap();

        let context_on = json!({"enabled": true});
        let result_on = engine.render("conditional", &context_on).unwrap();
        assert_eq!(result_on, "ON");

        let context_off = json!({"enabled": false});
        let result_off = engine.render("conditional", &context_off).unwrap();
        assert_eq!(result_off, "OFF");
    }

    #[test]
    fn test_custom_template_override() {
        let mut engine = TemplateEngine::new().unwrap();

        // Register template
        engine
            .register_template_string("override", "Version 1")
            .unwrap();

        // Override with new version
        engine
            .register_template_string("override", "Version 2")
            .unwrap();

        let result = engine.render("override", &json!({})).unwrap();
        assert_eq!(result, "Version 2");
    }

    #[test]
    fn test_render_with_null_values() {
        let mut engine = TemplateEngine::new().unwrap();

        engine
            .register_template_string("nullable", "Value: {{value}}")
            .unwrap();

        let context = json!({"value": null});
        let result = engine.render("nullable", &context).unwrap();
        assert_eq!(result, "Value: ");
    }

    #[test]
    fn test_render_with_boolean_values() {
        let mut engine = TemplateEngine::new().unwrap();

        engine
            .register_template_string("bool", "{{#if flag}}true{{else}}false{{/if}}")
            .unwrap();

        let context_true = json!({"flag": true});
        assert_eq!(engine.render("bool", &context_true).unwrap(), "true");

        let context_false = json!({"flag": false});
        assert_eq!(engine.render("bool", &context_false).unwrap(), "false");
    }

    #[test]
    fn test_render_with_numeric_values() {
        let mut engine = TemplateEngine::new().unwrap();

        engine
            .register_template_string("numbers", "Count: {{count}}, Ratio: {{ratio}}")
            .unwrap();

        let context = json!({"count": 42, "ratio": 1.618});
        let result = engine.render("numbers", &context).unwrap();
        assert!(result.contains("42"));
        assert!(result.contains("1.618"));
    }

    // ========================================================================
    // Strict Mode Tests
    // ========================================================================

    #[test]
    fn test_strict_mode_fails_on_missing_variable() {
        let mut custom_engine = TemplateEngine::new().unwrap();
        custom_engine
            .register_template_string("strict", "Value: {{missing_var}}")
            .unwrap();

        let context = json!({"other_var": "value"});
        let result = custom_engine.render("strict", &context);

        // Strict mode should fail on missing variable
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_template_renders() {
        let engine = TemplateEngine::new().unwrap();

        let tool_context = json!({
            "typescript_name": "tool1",
            "description": "First tool",
            "server_id": "test",
            "name": "tool_1",
            "properties": [],
            "has_required_properties": false,
            "input_schema": {}
        });

        // Render same template multiple times
        for _ in 0..10 {
            let result = engine.render("progressive/tool", &tool_context);
            assert!(result.is_ok());
        }
    }

    #[test]
    fn test_concurrent_template_usage() {
        // TemplateEngine should be Send + Sync
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<TemplateEngine>();
    }
}
