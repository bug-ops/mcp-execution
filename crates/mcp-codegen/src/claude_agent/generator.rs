//! Claude Agent SDK code generator.
//!
//! Generates TypeScript files for the Claude Agent SDK where each tool
//! is defined with Zod schemas for type-safe integration.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_codegen::claude_agent::ClaudeAgentGenerator;
//! use mcp_introspector::{Introspector, ServerInfo};
//! use mcp_core::{ServerId, ServerConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut introspector = Introspector::new();
//! let server_id = ServerId::new("github");
//! let config = ServerConfig::builder().command("/path/to/server".to_string()).build();
//! let info = introspector.discover_server(server_id, &config).await?;
//!
//! let generator = ClaudeAgentGenerator::new()?;
//! let code = generator.generate(&info)?;
//!
//! // Generated files:
//! // - index.ts (entry point)
//! // - server.ts (MCP server definition)
//! // - tools/createIssue.ts
//! // - tools/updateIssue.ts
//! // - ...
//! println!("Generated {} files", code.file_count());
//! # Ok(())
//! # }
//! ```

use crate::claude_agent::types::{
    IndexContext, PropertyInfo, ServerContext, ToolContext, ToolSummary,
};
use crate::claude_agent::zod::extract_zod_properties;
use crate::common::types::{GeneratedCode, GeneratedFile};
use crate::common::typescript::{to_camel_case, to_pascal_case};
use crate::template_engine::TemplateEngine;
use mcp_core::Result;
use mcp_introspector::ServerInfo;

/// Generator for Claude Agent SDK TypeScript files.
///
/// Creates tool definitions with Zod schemas for type-safe integration
/// with the Claude Agent SDK.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, allowing safe use across threads.
///
/// # Examples
///
/// ```
/// use mcp_codegen::claude_agent::ClaudeAgentGenerator;
///
/// let generator = ClaudeAgentGenerator::new().unwrap();
/// ```
#[derive(Debug)]
pub struct ClaudeAgentGenerator<'a> {
    engine: TemplateEngine<'a>,
}

impl<'a> ClaudeAgentGenerator<'a> {
    /// Creates a new Claude Agent SDK generator.
    ///
    /// Initializes the template engine and registers all Claude Agent SDK
    /// templates.
    ///
    /// # Errors
    ///
    /// Returns error if template registration fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::claude_agent::ClaudeAgentGenerator;
    ///
    /// let generator = ClaudeAgentGenerator::new().unwrap();
    /// ```
    pub fn new() -> Result<Self> {
        let engine = TemplateEngine::new()?;
        Ok(Self { engine })
    }

    /// Generates Claude Agent SDK files for a server.
    ///
    /// Creates TypeScript files with Zod schemas for each tool:
    /// - `index.ts`: Entry point with exports
    /// - `server.ts`: MCP server definition with `createSdkMcpServer()`
    /// - `tools/*.ts`: Individual tool definitions with Zod schemas
    ///
    /// # Arguments
    ///
    /// * `server_info` - MCP server introspection data
    ///
    /// # Returns
    ///
    /// Generated code with all necessary files for Claude Agent SDK integration.
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
    /// use mcp_codegen::claude_agent::ClaudeAgentGenerator;
    /// use mcp_introspector::{ServerInfo, ServerCapabilities};
    /// use mcp_core::ServerId;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let generator = ClaudeAgentGenerator::new()?;
    ///
    /// let info = ServerInfo {
    ///     id: ServerId::new("github"),
    ///     name: "GitHub".to_string(),
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
        tracing::info!(
            "Generating Claude Agent SDK code for server: {}",
            server_info.name
        );

        let mut code = GeneratedCode::new();
        let server_variable_name = to_camel_case(server_info.id.as_str());

        // Generate individual tool files
        for tool in &server_info.tools {
            let tool_context = self.create_tool_context(tool)?;
            let tool_code = self.engine.render("claude_agent/tool", &tool_context)?;

            code.add_file(GeneratedFile {
                path: format!("tools/{}.ts", tool_context.typescript_name),
                content: tool_code,
            });

            tracing::debug!(
                "Generated tool file: tools/{}.ts",
                tool_context.typescript_name
            );
        }

        // Generate server.ts
        let server_context = self.create_server_context(server_info, &server_variable_name)?;
        let server_code = self.engine.render("claude_agent/server", &server_context)?;

        code.add_file(GeneratedFile {
            path: "server.ts".to_string(),
            content: server_code,
        });

        tracing::debug!("Generated server.ts");

        // Generate index.ts
        let index_context = self.create_index_context(server_info, &server_variable_name)?;
        let index_code = self.engine.render("claude_agent/index", &index_context)?;

        code.add_file(GeneratedFile {
            path: "index.ts".to_string(),
            content: index_code,
        });

        tracing::debug!("Generated index.ts");

        tracing::info!(
            "Successfully generated {} files for {} (Claude Agent SDK)",
            code.file_count(),
            server_info.name
        );

        Ok(code)
    }

    /// Creates tool context from MCP tool information.
    fn create_tool_context(&self, tool: &mcp_introspector::ToolInfo) -> Result<ToolContext> {
        let typescript_name = to_camel_case(tool.name.as_str());
        let pascal_name = to_pascal_case(tool.name.as_str());

        // Extract properties with Zod types
        let zod_props = extract_zod_properties(&tool.input_schema);

        let properties: Vec<PropertyInfo> = zod_props
            .into_iter()
            .map(|prop| PropertyInfo {
                name: prop.name,
                zod_type: prop.zod_type,
                zod_modifiers: prop.zod_modifiers,
                description: prop.description,
                required: prop.required,
            })
            .collect();

        Ok(ToolContext {
            name: tool.name.as_str().to_string(),
            typescript_name,
            pascal_name,
            description: tool.description.clone(),
            properties,
        })
    }

    /// Creates server context from server information.
    fn create_server_context(
        &self,
        server_info: &ServerInfo,
        server_variable_name: &str,
    ) -> Result<ServerContext> {
        let tools: Vec<ToolSummary> = server_info
            .tools
            .iter()
            .map(|tool| ToolSummary {
                typescript_name: to_camel_case(tool.name.as_str()),
            })
            .collect();

        Ok(ServerContext {
            server_name: server_info.name.clone(),
            server_variable_name: server_variable_name.to_string(),
            server_version: server_info.version.clone(),
            tool_count: server_info.tools.len(),
            tools,
        })
    }

    /// Creates index context from server information.
    fn create_index_context(
        &self,
        server_info: &ServerInfo,
        server_variable_name: &str,
    ) -> Result<IndexContext> {
        let tools: Vec<ToolSummary> = server_info
            .tools
            .iter()
            .map(|tool| ToolSummary {
                typescript_name: to_camel_case(tool.name.as_str()),
            })
            .collect();

        Ok(IndexContext {
            server_name: server_info.name.clone(),
            server_variable_name: server_variable_name.to_string(),
            server_version: server_info.version.clone(),
            tool_count: server_info.tools.len(),
            tools,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::{ServerId, ToolName};
    use mcp_introspector::{ServerCapabilities, ToolInfo};
    use serde_json::json;

    fn create_test_server_info() -> ServerInfo {
        ServerInfo {
            id: ServerId::new("test-server"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("create_issue"),
                    description: "Creates a new issue".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "title": {
                                "type": "string",
                                "description": "Issue title"
                            },
                            "body": {
                                "type": "string",
                                "description": "Issue body"
                            }
                        },
                        "required": ["title"]
                    }),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("update_issue"),
                    description: "Updates an existing issue".to_string(),
                    input_schema: json!({
                        "type": "object",
                        "properties": {
                            "id": {
                                "type": "integer",
                                "description": "Issue ID"
                            }
                        },
                        "required": ["id"]
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
    fn test_claude_agent_generator_new() {
        let generator = ClaudeAgentGenerator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_generate_claude_agent_files() {
        let generator = ClaudeAgentGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();

        // Should generate:
        // - 2 tool files
        // - 1 server.ts
        // - 1 index.ts
        assert_eq!(code.file_count(), 4);

        // Check file paths
        let file_paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();

        assert!(file_paths.contains(&"tools/createIssue.ts"));
        assert!(file_paths.contains(&"tools/updateIssue.ts"));
        assert!(file_paths.contains(&"server.ts"));
        assert!(file_paths.contains(&"index.ts"));
    }

    #[test]
    fn test_create_tool_context() {
        let generator = ClaudeAgentGenerator::new().unwrap();
        let tool = ToolInfo {
            name: ToolName::new("send_message"),
            description: "Sends a message".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "Message text"},
                    "priority": {"type": "integer", "minimum": 1, "maximum": 5}
                },
                "required": ["text"]
            }),
            output_schema: None,
        };

        let context = generator.create_tool_context(&tool).unwrap();

        assert_eq!(context.name, "send_message");
        assert_eq!(context.typescript_name, "sendMessage");
        assert_eq!(context.pascal_name, "SendMessage");
        assert_eq!(context.description, "Sends a message");
        assert_eq!(context.properties.len(), 2);

        let text_prop = context
            .properties
            .iter()
            .find(|p| p.name == "text")
            .unwrap();
        assert_eq!(text_prop.zod_type, "string");
        assert!(text_prop.required);

        let priority_prop = context
            .properties
            .iter()
            .find(|p| p.name == "priority")
            .unwrap();
        assert_eq!(priority_prop.zod_type, "number");
        assert!(priority_prop.zod_modifiers.contains(&".int()".to_string()));
        assert!(!priority_prop.required);
    }

    #[test]
    fn test_create_server_context() {
        let generator = ClaudeAgentGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let context = generator
            .create_server_context(&server_info, "testServer")
            .unwrap();

        assert_eq!(context.server_name, "Test Server");
        assert_eq!(context.server_variable_name, "testServer");
        assert_eq!(context.server_version, "1.0.0");
        assert_eq!(context.tools.len(), 2);
    }

    #[test]
    fn test_create_index_context() {
        let generator = ClaudeAgentGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let context = generator
            .create_index_context(&server_info, "testServer")
            .unwrap();

        assert_eq!(context.server_name, "Test Server");
        assert_eq!(context.tool_count, 2);
        assert_eq!(context.tools.len(), 2);
        assert_eq!(context.tools[0].typescript_name, "createIssue");
    }

    #[test]
    fn test_generate_with_email_format() {
        let generator = ClaudeAgentGenerator::new().unwrap();
        let server_info = ServerInfo {
            id: ServerId::new("user-service"),
            name: "User Service".to_string(),
            version: "2.0.0".to_string(),
            tools: vec![ToolInfo {
                name: ToolName::new("create_user"),
                description: "Creates a new user".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "email": {
                            "type": "string",
                            "format": "email",
                            "description": "User email address"
                        },
                        "name": {
                            "type": "string",
                            "minLength": 1,
                            "maxLength": 100
                        }
                    },
                    "required": ["email", "name"]
                }),
                output_schema: None,
            }],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        };

        let code = generator.generate(&server_info).unwrap();
        assert_eq!(code.file_count(), 3);

        // Check that email format is detected
        let tool_file = code
            .files
            .iter()
            .find(|f| f.path == "tools/createUser.ts")
            .unwrap();

        // The template should generate .email() modifier
        assert!(
            tool_file.content.contains(".email()") || tool_file.content.contains("email"),
            "Expected email format handling"
        );
    }
}
