//! Main code generator for MCP tools.
//!
//! Orchestrates the generation of TypeScript code from MCP server
//! information, using templates and type conversion utilities.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_codegen::CodeGenerator;
//! use mcp_introspector::{Introspector, ServerInfo};
//! use mcp_core::{ServerId, ServerConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut introspector = Introspector::new();
//! let server_id = ServerId::new("github");
//! let config = ServerConfig::builder().command("/path/to/server".to_string()).build();
//! let info = introspector.discover_server(server_id, &config).await?;
//!
//! let generator = CodeGenerator::new()?;
//! let code = generator.generate(&info)?;
//!
//! println!("Generated {} files", code.file_count());
//! # Ok(())
//! # }
//! ```

use crate::common::types::{GeneratedCode, GeneratedFile, TemplateContext, ToolDefinition};
use crate::common::typescript;
use crate::template_engine::TemplateEngine;
use mcp_core::{Error, Result};
use mcp_introspector::ServerInfo;
use std::collections::HashMap;

/// Main code generator for MCP tools.
///
/// Generates complete TypeScript project structure from MCP server
/// information, including tool definitions, types, and manifest.
///
/// # Examples
///
/// ```no_run
/// use mcp_codegen::CodeGenerator;
///
/// # fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let generator = CodeGenerator::new()?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct CodeGenerator<'a> {
    engine: TemplateEngine<'a>,
}

impl<'a> CodeGenerator<'a> {
    /// Creates a new code generator.
    ///
    /// # Errors
    ///
    /// Returns error if template engine initialization fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::CodeGenerator;
    ///
    /// let generator = CodeGenerator::new().unwrap();
    /// ```
    pub fn new() -> Result<Self> {
        let engine = TemplateEngine::new()?;
        Ok(Self { engine })
    }

    /// Generates TypeScript code for an MCP server.
    ///
    /// Creates a complete project structure with:
    /// - `manifest.json`: Server metadata
    /// - `types.ts`: Shared type definitions
    /// - `tools/*.ts`: Individual tool implementations
    /// - `index.ts`: Barrel exports
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Template rendering fails
    /// - Type conversion fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::CodeGenerator;
    /// use mcp_introspector::{ServerInfo, ServerCapabilities};
    /// use mcp_core::ServerId;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let generator = CodeGenerator::new()?;
    ///
    /// let info = ServerInfo {
    ///     id: ServerId::new("test"),
    ///     name: "Test Server".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     tools: vec![],
    ///     capabilities: ServerCapabilities {
    ///         supports_tools: true,
    ///         supports_resources: false,
    ///         supports_prompts: false,
    ///     },
    /// };
    ///
    /// let code = generator.generate(&info)?;
    /// println!("Generated {} files", code.file_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate(&self, server_info: &ServerInfo) -> Result<GeneratedCode> {
        tracing::info!("Generating code for server: {}", server_info.name);

        let mut generated = GeneratedCode::new();

        // Convert tools to template format
        let tools = self.convert_tools(server_info)?;

        // Create template context
        let context = TemplateContext {
            server_name: server_info.name.clone(),
            server_version: server_info.version.clone(),
            tools: tools.clone(),
            metadata: HashMap::new(),
        };

        // Generate manifest.json
        generated.add_file(self.generate_manifest(&context)?);

        // Generate types.ts
        generated.add_file(self.generate_types(&context)?);

        // Generate individual tool files
        for tool in &tools {
            generated.add_file(self.generate_tool(tool)?);
        }

        // Generate index.ts
        generated.add_file(self.generate_index(&context)?);

        tracing::info!(
            "Successfully generated {} files for {}",
            generated.file_count(),
            server_info.name
        );

        Ok(generated)
    }

    /// Converts MCP tool info to template format.
    fn convert_tools(&self, server_info: &ServerInfo) -> Result<Vec<ToolDefinition>> {
        let mut tools = Vec::new();

        for tool_info in &server_info.tools {
            let typescript_name = typescript::to_camel_case(tool_info.name.as_str());

            let tool_def = ToolDefinition {
                name: tool_info.name.as_str().to_string(),
                description: tool_info.description.clone(),
                input_schema: tool_info.input_schema.clone(),
                typescript_name,
            };

            tools.push(tool_def);
        }

        Ok(tools)
    }

    /// Generates manifest.json file.
    fn generate_manifest(&self, context: &TemplateContext) -> Result<GeneratedFile> {
        let mut manifest_context =
            serde_json::to_value(context).map_err(|e| Error::SerializationError {
                message: format!("Failed to serialize context: {}", e),
                source: Some(e),
            })?;

        // Add timestamp
        manifest_context["timestamp"] = serde_json::json!(chrono::Utc::now().to_rfc3339());

        let content = self.engine.render("manifest", &manifest_context)?;

        Ok(GeneratedFile {
            path: "manifest.json".to_string(),
            content,
        })
    }

    /// Generates types.ts file.
    fn generate_types(&self, context: &TemplateContext) -> Result<GeneratedFile> {
        let content = self.engine.render("types", context)?;

        Ok(GeneratedFile {
            path: "types.ts".to_string(),
            content,
        })
    }

    /// Generates a single tool TypeScript file.
    fn generate_tool(&self, tool: &ToolDefinition) -> Result<GeneratedFile> {
        // Extract properties for template
        let properties = typescript::extract_properties(&tool.input_schema);

        let mut tool_context =
            serde_json::to_value(tool).map_err(|e| Error::SerializationError {
                message: format!("Failed to serialize tool: {}", e),
                source: Some(e),
            })?;

        tool_context["properties"] = serde_json::json!(properties);

        let content = self.engine.render("tool", &tool_context)?;

        Ok(GeneratedFile {
            path: format!("tools/{}.ts", tool.typescript_name),
            content,
        })
    }

    /// Generates index.ts file.
    fn generate_index(&self, context: &TemplateContext) -> Result<GeneratedFile> {
        let content = self.engine.render("index", context)?;

        Ok(GeneratedFile {
            path: "index.ts".to_string(),
            content,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::{ServerId, ToolName};
    use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};

    fn create_test_server_info() -> ServerInfo {
        ServerInfo {
            id: ServerId::new("test-server"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("send_message"),
                    description: "Sends a message".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "text": {"type": "string"},
                            "chat_id": {"type": "string"}
                        },
                        "required": ["text", "chat_id"]
                    }),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("get_user"),
                    description: "Gets user info".to_string(),
                    input_schema: serde_json::json!({
                        "type": "object",
                        "properties": {
                            "user_id": {"type": "string"}
                        },
                        "required": ["user_id"]
                    }),
                    output_schema: None,
                },
            ],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        }
    }

    #[test]
    fn test_generator_new() {
        let generator = CodeGenerator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_generate() {
        let generator = CodeGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let result = generator.generate(&server_info);
        assert!(result.is_ok());

        let code = result.unwrap();
        assert!(code.file_count() > 0);

        // Should have manifest, types, index, and 2 tool files
        assert_eq!(code.file_count(), 5);
    }

    #[test]
    fn test_convert_tools() {
        let generator = CodeGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let tools = generator.convert_tools(&server_info).unwrap();
        assert_eq!(tools.len(), 2);

        assert_eq!(tools[0].name, "send_message");
        assert_eq!(tools[0].typescript_name, "sendMessage");

        assert_eq!(tools[1].name, "get_user");
        assert_eq!(tools[1].typescript_name, "getUser");
    }

    #[test]
    fn test_generated_file_paths() {
        let generator = CodeGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();

        let paths: Vec<_> = code.files.iter().map(|f| &f.path).collect();

        assert!(paths.contains(&&"manifest.json".to_string()));
        assert!(paths.contains(&&"types.ts".to_string()));
        assert!(paths.contains(&&"index.ts".to_string()));
        assert!(paths.contains(&&"tools/sendMessage.ts".to_string()));
        assert!(paths.contains(&&"tools/getUser.ts".to_string()));
    }
}
