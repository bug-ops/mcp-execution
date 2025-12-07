//! Progressive loading code generator.
//!
//! Generates TypeScript files for progressive loading where each tool
//! is in a separate file, enabling Claude Code to load only what it needs.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_codegen::progressive::ProgressiveGenerator;
//! use mcp_introspector::{Introspector, ServerInfo};
//! use mcp_core::{ServerId, ServerConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let mut introspector = Introspector::new();
//! let server_id = ServerId::new("github");
//! let config = ServerConfig::builder().command("/path/to/server".to_string()).build();
//! let info = introspector.discover_server(server_id, &config).await?;
//!
//! let generator = ProgressiveGenerator::new()?;
//! let code = generator.generate(&info)?;
//!
//! // Generated files:
//! // - index.ts (re-exports)
//! // - createIssue.ts
//! // - updateIssue.ts
//! // - ...
//! // - _runtime/mcp-bridge.ts
//! println!("Generated {} files", code.file_count());
//! # Ok(())
//! # }
//! ```

use crate::common::types::{GeneratedCode, GeneratedFile};
use crate::common::typescript::{extract_properties, to_camel_case};
use crate::progressive::types::{
    BridgeContext, CategoryInfo, IndexContext, PropertyInfo, ToolContext, ToolSummary,
};
use crate::template_engine::TemplateEngine;
use mcp_core::{Error, Result};
use mcp_introspector::ServerInfo;
use std::collections::HashMap;

/// Generator for progressive loading TypeScript files.
///
/// Creates one file per tool plus an index file and runtime bridge,
/// enabling progressive loading where only needed tools are loaded.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, allowing safe use across threads.
///
/// # Examples
///
/// ```
/// use mcp_codegen::progressive::ProgressiveGenerator;
///
/// let generator = ProgressiveGenerator::new().unwrap();
/// ```
#[derive(Debug)]
pub struct ProgressiveGenerator<'a> {
    engine: TemplateEngine<'a>,
}

impl<'a> ProgressiveGenerator<'a> {
    /// Creates a new progressive generator.
    ///
    /// Initializes the template engine and registers all progressive
    /// loading templates.
    ///
    /// # Errors
    ///
    /// Returns error if template registration fails (should not happen
    /// with valid built-in templates).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::progressive::ProgressiveGenerator;
    ///
    /// let generator = ProgressiveGenerator::new().unwrap();
    /// ```
    pub fn new() -> Result<Self> {
        let engine = TemplateEngine::new()?;
        Ok(Self { engine })
    }

    /// Generates progressive loading files for a server.
    ///
    /// Creates one TypeScript file per tool, plus:
    /// - `index.ts`: Re-exports all tools
    /// - `_runtime/mcp-bridge.ts`: Runtime bridge for calling MCP tools
    ///
    /// # Arguments
    ///
    /// * `server_info` - MCP server introspection data
    ///
    /// # Returns
    ///
    /// Generated code with one file per tool plus index and runtime bridge.
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
    /// use mcp_codegen::progressive::ProgressiveGenerator;
    /// use mcp_introspector::{ServerInfo, ServerCapabilities};
    /// use mcp_core::ServerId;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let generator = ProgressiveGenerator::new()?;
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
    ///
    /// // Files generated:
    /// // - index.ts
    /// // - _runtime/mcp-bridge.ts
    /// // - one file per tool
    /// println!("Generated {} files", code.file_count());
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate(&self, server_info: &ServerInfo) -> Result<GeneratedCode> {
        tracing::info!(
            "Generating progressive loading code for server: {}",
            server_info.name
        );

        let mut code = GeneratedCode::new();
        let server_id = server_info.id.as_str();

        // Generate tool files (one per tool)
        for tool in &server_info.tools {
            let tool_context = self.create_tool_context(server_id, tool, None)?;
            let tool_code = self.engine.render("progressive/tool", &tool_context)?;

            code.add_file(GeneratedFile {
                path: format!("{}.ts", tool_context.typescript_name),
                content: tool_code,
            });

            tracing::debug!("Generated tool file: {}.ts", tool_context.typescript_name);
        }

        // Generate index.ts
        let index_context = self.create_index_context(server_info, None)?;
        let index_code = self.engine.render("progressive/index", &index_context)?;

        code.add_file(GeneratedFile {
            path: "index.ts".to_string(),
            content: index_code,
        });

        tracing::debug!("Generated index.ts");

        // Generate runtime bridge
        let bridge_context = BridgeContext::default();
        let bridge_code = self
            .engine
            .render("progressive/runtime-bridge", &bridge_context)?;

        code.add_file(GeneratedFile {
            path: "_runtime/mcp-bridge.ts".to_string(),
            content: bridge_code,
        });

        tracing::debug!("Generated _runtime/mcp-bridge.ts");

        tracing::info!(
            "Successfully generated {} files for {} (progressive loading)",
            code.file_count(),
            server_info.name
        );

        Ok(code)
    }

    /// Generates progressive loading files with category metadata.
    ///
    /// Like `generate`, but includes category information from Claude's
    /// categorization. Categories are displayed in the index file and
    /// included in individual tool file headers.
    ///
    /// # Arguments
    ///
    /// * `server_info` - MCP server introspection data
    /// * `categories` - Map of tool name to category name
    ///
    /// # Returns
    ///
    /// Generated code with category metadata included.
    ///
    /// # Errors
    ///
    /// Returns error if template rendering fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::progressive::ProgressiveGenerator;
    /// use mcp_introspector::{ServerInfo, ServerCapabilities};
    /// use mcp_core::ServerId;
    /// use std::collections::HashMap;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let generator = ProgressiveGenerator::new()?;
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
    /// let mut categories = HashMap::new();
    /// categories.insert("create_issue".to_string(), "issues".to_string());
    /// categories.insert("list_issues".to_string(), "issues".to_string());
    /// categories.insert("create_pr".to_string(), "pull-requests".to_string());
    ///
    /// let code = generator.generate_with_categories(&info, &categories)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate_with_categories(
        &self,
        server_info: &ServerInfo,
        categories: &HashMap<String, String>,
    ) -> Result<GeneratedCode> {
        tracing::info!(
            "Generating progressive loading code with categories for server: {}",
            server_info.name
        );

        let mut code = GeneratedCode::new();
        let server_id = server_info.id.as_str();

        // Generate tool files (one per tool) with category metadata
        for tool in &server_info.tools {
            let tool_name = tool.name.as_str();
            let category = categories.get(tool_name).map(String::as_str);
            let tool_context = self.create_tool_context(server_id, tool, category)?;
            let tool_code = self.engine.render("progressive/tool", &tool_context)?;

            code.add_file(GeneratedFile {
                path: format!("{}.ts", tool_context.typescript_name),
                content: tool_code,
            });

            tracing::debug!(
                "Generated tool file: {}.ts (category: {:?})",
                tool_context.typescript_name,
                category
            );
        }

        // Generate index.ts with category grouping
        let index_context = self.create_index_context(server_info, Some(categories))?;
        let index_code = self.engine.render("progressive/index", &index_context)?;

        code.add_file(GeneratedFile {
            path: "index.ts".to_string(),
            content: index_code,
        });

        tracing::debug!("Generated index.ts with {} categories", categories.len());

        // Generate runtime bridge (same as non-categorized)
        let bridge_context = BridgeContext::default();
        let bridge_code = self
            .engine
            .render("progressive/runtime-bridge", &bridge_context)?;

        code.add_file(GeneratedFile {
            path: "_runtime/mcp-bridge.ts".to_string(),
            content: bridge_code,
        });

        tracing::debug!("Generated _runtime/mcp-bridge.ts");

        tracing::info!(
            "Successfully generated {} files for {} with categories (progressive loading)",
            code.file_count(),
            server_info.name
        );

        Ok(code)
    }

    /// Creates tool context from MCP tool information.
    ///
    /// Converts MCP tool schema to the format needed for template rendering.
    ///
    /// # Errors
    ///
    /// Returns error if schema conversion fails.
    fn create_tool_context(
        &self,
        server_id: &str,
        tool: &mcp_introspector::ToolInfo,
        category: Option<&str>,
    ) -> Result<ToolContext> {
        let typescript_name = to_camel_case(tool.name.as_str());

        // Extract properties from input schema
        let properties = self.extract_property_infos(&tool.input_schema)?;

        Ok(ToolContext {
            server_id: server_id.to_string(),
            name: tool.name.as_str().to_string(),
            typescript_name,
            description: tool.description.clone(),
            input_schema: tool.input_schema.clone(),
            properties,
            category: category.map(String::from),
        })
    }

    /// Creates index context from server information.
    fn create_index_context(
        &self,
        server_info: &ServerInfo,
        categories: Option<&HashMap<String, String>>,
    ) -> Result<IndexContext> {
        let tools: Vec<ToolSummary> = server_info
            .tools
            .iter()
            .map(|tool| {
                let tool_name = tool.name.as_str();
                let category = categories.and_then(|c| c.get(tool_name)).cloned();
                ToolSummary {
                    typescript_name: to_camel_case(tool_name),
                    description: tool.description.clone(),
                    category,
                }
            })
            .collect();

        // Build category groups if categories are provided
        let category_groups = categories.map(|_| {
            let mut groups: HashMap<String, Vec<ToolSummary>> = HashMap::new();

            for tool in &tools {
                let cat_name = tool
                    .category
                    .clone()
                    .unwrap_or_else(|| "uncategorized".to_string());
                groups.entry(cat_name).or_default().push(tool.clone());
            }

            let mut result: Vec<CategoryInfo> = groups
                .into_iter()
                .map(|(name, tools)| CategoryInfo { name, tools })
                .collect();

            // Sort categories alphabetically, but keep "uncategorized" last
            result.sort_by(|a, b| {
                if a.name == "uncategorized" {
                    std::cmp::Ordering::Greater
                } else if b.name == "uncategorized" {
                    std::cmp::Ordering::Less
                } else {
                    a.name.cmp(&b.name)
                }
            });

            result
        });

        Ok(IndexContext {
            server_name: server_info.name.clone(),
            server_version: server_info.version.clone(),
            tool_count: server_info.tools.len(),
            tools,
            categories: category_groups,
        })
    }

    /// Extracts property information from JSON Schema.
    ///
    /// Converts JSON Schema properties into `PropertyInfo` structures
    /// suitable for template rendering.
    ///
    /// # Errors
    ///
    /// Returns error if schema is malformed or type conversion fails.
    fn extract_property_infos(&self, schema: &serde_json::Value) -> Result<Vec<PropertyInfo>> {
        let raw_properties = extract_properties(schema);

        let mut properties = Vec::new();
        for prop in raw_properties {
            let name = prop["name"]
                .as_str()
                .ok_or_else(|| Error::ValidationError {
                    field: "name".to_string(),
                    reason: "Property name is not a string".to_string(),
                })?
                .to_string();

            let typescript_type = prop["type"]
                .as_str()
                .ok_or_else(|| Error::ValidationError {
                    field: "type".to_string(),
                    reason: "Property type is not a string".to_string(),
                })?
                .to_string();

            let required = prop["required"].as_bool().unwrap_or(false);

            // Extract description if available
            let description = if let Some(obj) = schema.as_object() {
                obj.get("properties")
                    .and_then(|props| props.as_object())
                    .and_then(|props| props.get(&name))
                    .and_then(|prop_schema| prop_schema.as_object())
                    .and_then(|obj| obj.get("description"))
                    .and_then(|desc| desc.as_str())
                    .map(String::from)
            } else {
                None
            };

            properties.push(PropertyInfo {
                name,
                typescript_type,
                description,
                required,
            });
        }

        Ok(properties)
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
                                "type": "number"
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
    fn test_progressive_generator_new() {
        let generator = ProgressiveGenerator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_generate_progressive_files() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let code = generator.generate(&server_info).unwrap();

        // Should generate:
        // - 2 tool files
        // - 1 index.ts
        // - 1 runtime bridge
        assert_eq!(code.file_count(), 4);

        // Check tool files exist
        let tool_files: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();

        assert!(tool_files.contains(&"createIssue.ts"));
        assert!(tool_files.contains(&"updateIssue.ts"));
        assert!(tool_files.contains(&"index.ts"));
        assert!(tool_files.contains(&"_runtime/mcp-bridge.ts"));
    }

    #[test]
    fn test_create_tool_context() {
        let generator = ProgressiveGenerator::new().unwrap();
        let tool = ToolInfo {
            name: ToolName::new("send_message"),
            description: "Sends a message".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"}
                },
                "required": ["text"]
            }),
            output_schema: None,
        };

        let context = generator
            .create_tool_context("test-server", &tool, Some("messaging"))
            .unwrap();

        assert_eq!(context.server_id, "test-server");
        assert_eq!(context.name, "send_message");
        assert_eq!(context.typescript_name, "sendMessage");
        assert_eq!(context.description, "Sends a message");
        assert_eq!(context.properties.len(), 1);
        assert_eq!(context.properties[0].name, "text");
        assert_eq!(context.category, Some("messaging".to_string()));
    }

    #[test]
    fn test_create_index_context() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_test_server_info();

        let context = generator.create_index_context(&server_info, None).unwrap();

        assert_eq!(context.server_name, "Test Server");
        assert_eq!(context.server_version, "1.0.0");
        assert_eq!(context.tool_count, 2);
        assert_eq!(context.tools.len(), 2);
        assert_eq!(context.tools[0].typescript_name, "createIssue");
        assert!(context.categories.is_none());
    }

    #[test]
    fn test_extract_property_infos() {
        let generator = ProgressiveGenerator::new().unwrap();
        let schema = json!({
            "type": "object",
            "properties": {
                "name": {
                    "type": "string",
                    "description": "User name"
                },
                "age": {
                    "type": "number"
                }
            },
            "required": ["name"]
        });

        let props = generator.extract_property_infos(&schema).unwrap();

        assert_eq!(props.len(), 2);

        // Find name property
        let name_prop = props.iter().find(|p| p.name == "name").unwrap();
        assert_eq!(name_prop.typescript_type, "string");
        assert_eq!(name_prop.description, Some("User name".to_string()));
        assert!(name_prop.required);

        // Find age property
        let age_prop = props.iter().find(|p| p.name == "age").unwrap();
        assert_eq!(age_prop.typescript_type, "number");
        assert!(!age_prop.required);
    }
}
