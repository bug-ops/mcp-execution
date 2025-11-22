//! Converts MCP server introspection data to Claude skill format.
//!
//! This module provides conversion logic from `mcp_introspector::ServerInfo`
//! to `SkillData` for template rendering in the Claude Agent Skills format.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_codegen::skills::converter::SkillConverter;
//! use mcp_introspector::{ServerInfo, ToolInfo, ServerCapabilities};
//! use mcp_core::{ServerId, ToolName, SkillName, SkillDescription};
//! use serde_json::json;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let server_info = ServerInfo {
//!     id: ServerId::new("test-server"),
//!     name: "test-server".to_string(),
//!     version: "1.0.0".to_string(),
//!     tools: vec![
//!         ToolInfo {
//!             name: ToolName::new("send_message"),
//!             description: "Sends a message".to_string(),
//!             input_schema: json!({
//!                 "type": "object",
//!                 "properties": {
//!                     "chat_id": {
//!                         "type": "string",
//!                         "description": "Chat ID"
//!                     }
//!                 },
//!                 "required": ["chat_id"]
//!             }),
//!             output_schema: None,
//!         }
//!     ],
//!     capabilities: ServerCapabilities {
//!         supports_tools: true,
//!         supports_resources: false,
//!         supports_prompts: false,
//!     },
//! };
//!
//! let name = SkillName::new("test-skill")?;
//! let desc = SkillDescription::new("Test skill description")?;
//!
//! let skill_data = SkillConverter::convert(&server_info, &name, &desc)?;
//! assert_eq!(skill_data.skill_name, "test-skill");
//! assert_eq!(skill_data.tools.len(), 1);
//! # Ok(())
//! # }
//! ```

use crate::skills::claude::{ParameterData, SkillData, ToolData};
use mcp_core::{Error, Result, SkillDescription, SkillName};
use mcp_introspector::ServerInfo;
use serde_json::Value;

/// Converts MCP server introspection data to Claude skill format.
///
/// This converter transforms MCP server information into the data structure
/// needed for rendering Anthropic Claude Agent Skills templates.
#[derive(Debug)]
pub struct SkillConverter;

impl SkillConverter {
    /// Converts `ServerInfo` to `SkillData` for template rendering.
    ///
    /// This method extracts all relevant information from an MCP server
    /// and transforms it into the format required by Claude skill templates.
    ///
    /// # Arguments
    ///
    /// * `server_info` - Introspection data from MCP server
    /// * `skill_name` - Validated skill name (from `SkillName`)
    /// * `skill_description` - Validated description (from `SkillDescription`)
    ///
    /// # Returns
    ///
    /// `SkillData` ready for template rendering with `render_skill_md` or `render_reference_md`
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Tool conversion fails (invalid schema, etc.)
    /// - JSON schema parsing fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::converter::SkillConverter;
    /// use mcp_introspector::{ServerInfo, ServerCapabilities};
    /// use mcp_core::{ServerId, SkillName, SkillDescription};
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let server_info = ServerInfo {
    ///     id: ServerId::new("vkteams-bot"),
    ///     name: "vkteams-bot".to_string(),
    ///     version: "1.0.0".to_string(),
    ///     tools: vec![],
    ///     capabilities: ServerCapabilities {
    ///         supports_tools: true,
    ///         supports_resources: false,
    ///         supports_prompts: false,
    ///     },
    /// };
    ///
    /// let name = SkillName::new("vkteams")?;
    /// let desc = SkillDescription::new("VK Teams bot integration")?;
    ///
    /// let skill_data = SkillConverter::convert(&server_info, &name, &desc)?;
    /// assert_eq!(skill_data.skill_name, "vkteams");
    /// # Ok(())
    /// # }
    /// ```
    pub fn convert(
        server_info: &ServerInfo,
        skill_name: &SkillName,
        skill_description: &SkillDescription,
    ) -> Result<SkillData> {
        tracing::debug!(
            "Converting server '{}' to skill '{}'",
            server_info.name,
            skill_name.as_str()
        );

        // Convert all tools
        let tools: Result<Vec<ToolData>> =
            server_info.tools.iter().map(Self::convert_tool).collect();
        let tools = tools?;

        // Build capabilities list
        let mut capabilities = Vec::new();
        if server_info.capabilities.supports_tools {
            capabilities.push("tools".to_string());
        }
        if server_info.capabilities.supports_resources {
            capabilities.push("resources".to_string());
        }
        if server_info.capabilities.supports_prompts {
            capabilities.push("prompts".to_string());
        }

        // Create SkillData using constructor (handles timestamp automatically)
        let skill_data = SkillData::new(
            skill_name.as_str().to_string(),
            skill_description.as_str().to_string(),
            server_info.name.clone(),
            server_info.version.clone(),
            format!("MCP server: {}", server_info.name),
            "2024-11-05".to_string(), // MCP protocol version
            tools,
            capabilities,
        );

        tracing::info!(
            "Successfully converted skill '{}' with {} tools",
            skill_name.as_str(),
            skill_data.tool_count
        );

        Ok(skill_data)
    }

    /// Converts a single MCP tool to `ToolData`.
    ///
    /// Extracts tool name, description, and parses the input schema to
    /// generate parameter documentation.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Input schema is malformed
    /// - Schema properties cannot be parsed
    fn convert_tool(tool: &mcp_introspector::ToolInfo) -> Result<ToolData> {
        tracing::trace!("Converting tool: {}", tool.name.as_str());

        // Parse input schema to extract parameters
        let parameters = Self::extract_parameters(&tool.input_schema)?;

        // Pretty-print JSON schema for documentation
        let input_schema_json = serde_json::to_string_pretty(&tool.input_schema).map_err(|e| {
            Error::SerializationError {
                message: format!("Failed to serialize input schema: {e}"),
                source: Some(e),
            }
        })?;

        Ok(ToolData {
            name: tool.name.as_str().to_string(),
            description: tool.description.clone(),
            parameters,
            input_schema_json,
        })
    }

    /// Extracts parameters from JSON Schema.
    ///
    /// Parses a JSON Schema object to extract parameter names, types,
    /// descriptions, and whether they're required.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Schema is not an object type
    /// - Schema properties are malformed
    fn extract_parameters(schema: &Value) -> Result<Vec<ParameterData>> {
        // Get required fields list
        let required_fields: Vec<String> = schema
            .get("required")
            .and_then(Value::as_array)
            .map(|arr| {
                arr.iter()
                    .filter_map(Value::as_str)
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        // Get properties object
        let properties = match schema.get("properties") {
            Some(Value::Object(props)) => props,
            Some(_) => {
                return Err(Error::ConfigError {
                    message: "Schema 'properties' must be an object".to_string(),
                });
            }
            None => {
                // No properties means no parameters (valid case)
                return Ok(Vec::new());
            }
        };

        // Convert each property to ParameterData
        let mut parameters = Vec::new();
        for (name, prop_schema) in properties {
            let param = Self::convert_parameter(name, prop_schema, &required_fields)?;
            parameters.push(param);
        }

        Ok(parameters)
    }

    /// Converts a single parameter from JSON Schema.
    fn convert_parameter(
        name: &str,
        schema: &Value,
        required_fields: &[String],
    ) -> Result<ParameterData> {
        let type_name = Self::extract_type_name(schema);
        let required = required_fields.contains(&name.to_string());
        let description = schema
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let example_value = Self::generate_example_value(schema);

        Ok(ParameterData {
            name: name.to_string(),
            type_name,
            required,
            description,
            example_value,
        })
    }

    /// Extracts type name from JSON Schema.
    ///
    /// Returns the TypeScript/JSON Schema type name for a parameter.
    fn extract_type_name(schema: &Value) -> String {
        // Check for type field
        if let Some(type_value) = schema.get("type")
            && let Some(type_str) = type_value.as_str()
        {
            return type_str.to_string();
        }

        // Check for enum (treat as string)
        if schema.get("enum").is_some() {
            return "string".to_string();
        }

        // Check for oneOf/anyOf (complex type)
        if schema.get("oneOf").is_some() || schema.get("anyOf").is_some() {
            return "any".to_string();
        }

        // Default to any
        "any".to_string()
    }

    /// Generates example value for a parameter based on its JSON Schema type.
    ///
    /// Creates realistic example values that will appear in the generated
    /// skill documentation.
    ///
    /// # Examples
    ///
    /// - `string` → `"example"`
    /// - `number` → `42`
    /// - `boolean` → `true`
    /// - `array` → `[]`
    /// - `object` → `{}`
    fn generate_example_value(schema: &Value) -> String {
        // Check for explicit example
        if let Some(example) = schema.get("example")
            && let Ok(example_str) = serde_json::to_string(example)
        {
            return example_str;
        }

        // Check for enum values
        if let Some(Value::Array(enum_values)) = schema.get("enum")
            && let Some(first_value) = enum_values.first()
            && let Ok(value_str) = serde_json::to_string(first_value)
        {
            return value_str;
        }

        // Generate example based on type
        let type_name = Self::extract_type_name(schema);
        match type_name.as_str() {
            "string" => {
                // Check for format hints
                if let Some(format) = schema.get("format").and_then(Value::as_str) {
                    match format {
                        "email" => return r#""user@example.com""#.to_string(),
                        "uri" | "url" => return r#""https://example.com""#.to_string(),
                        "date" => return r#""2024-01-01""#.to_string(),
                        "date-time" => return r#""2024-01-01T00:00:00Z""#.to_string(),
                        "uuid" => return r#""550e8400-e29b-41d4-a716-446655440000""#.to_string(),
                        _ => {}
                    }
                }
                r#""example""#.to_string()
            }
            "number" | "integer" => "42".to_string(),
            "boolean" => "true".to_string(),
            "array" => "[]".to_string(),
            "object" => "{}".to_string(),
            "null" => "null".to_string(),
            _ => r#""value""#.to_string(),
        }
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
            name: "test-server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: ToolName::new("send_message"),
                description: "Sends a message to a chat".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "string",
                            "description": "Chat identifier"
                        },
                        "text": {
                            "type": "string",
                            "description": "Message text"
                        },
                        "urgent": {
                            "type": "boolean",
                            "description": "Mark as urgent"
                        }
                    },
                    "required": ["chat_id", "text"]
                }),
                output_schema: None,
            }],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        }
    }

    #[test]
    fn test_convert_basic() {
        let server_info = create_test_server_info();
        let name = SkillName::new("test-skill").unwrap();
        let desc = SkillDescription::new("Test skill description").unwrap();

        let result = SkillConverter::convert(&server_info, &name, &desc);
        assert!(result.is_ok());

        let skill_data = result.unwrap();
        assert_eq!(skill_data.skill_name, "test-skill");
        assert_eq!(skill_data.skill_description, "Test skill description");
        assert_eq!(skill_data.server_name, "test-server");
        assert_eq!(skill_data.server_version, "1.0.0");
        assert_eq!(skill_data.tool_count, 1);
        assert_eq!(skill_data.tools.len(), 1);
    }

    #[test]
    fn test_convert_capabilities() {
        let server_info = create_test_server_info();
        let name = SkillName::new("test-skill").unwrap();
        let desc = SkillDescription::new("Test skill").unwrap();

        let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();
        assert_eq!(skill_data.capabilities, vec!["tools"]);
    }

    #[test]
    fn test_convert_tool_basic() {
        let tool = ToolInfo {
            name: ToolName::new("test_tool"),
            description: "Test tool".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "param1": {
                        "type": "string",
                        "description": "First parameter"
                    }
                },
                "required": ["param1"]
            }),
            output_schema: None,
        };

        let result = SkillConverter::convert_tool(&tool);
        assert!(result.is_ok());

        let tool_data = result.unwrap();
        assert_eq!(tool_data.name, "test_tool");
        assert_eq!(tool_data.description, "Test tool");
        assert_eq!(tool_data.parameters.len(), 1);
        assert!(tool_data.input_schema_json.contains("param1"));
    }

    #[test]
    fn test_extract_parameters_with_required() {
        let schema = json!({
            "type": "object",
            "properties": {
                "required_param": {
                    "type": "string",
                    "description": "Required parameter"
                },
                "optional_param": {
                    "type": "number",
                    "description": "Optional parameter"
                }
            },
            "required": ["required_param"]
        });

        let params = SkillConverter::extract_parameters(&schema).unwrap();
        assert_eq!(params.len(), 2);

        let required_param = params.iter().find(|p| p.name == "required_param").unwrap();
        assert!(required_param.required);

        let optional_param = params.iter().find(|p| p.name == "optional_param").unwrap();
        assert!(!optional_param.required);
    }

    #[test]
    fn test_extract_parameters_no_properties() {
        let schema = json!({
            "type": "object"
        });

        let params = SkillConverter::extract_parameters(&schema).unwrap();
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_parameters_empty_properties() {
        let schema = json!({
            "type": "object",
            "properties": {}
        });

        let params = SkillConverter::extract_parameters(&schema).unwrap();
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_convert_parameter() {
        let schema = json!({
            "type": "string",
            "description": "A string parameter"
        });

        let param = SkillConverter::convert_parameter("test_param", &schema, &[]).unwrap();
        assert_eq!(param.name, "test_param");
        assert_eq!(param.type_name, "string");
        assert!(!param.required);
        assert_eq!(param.description, "A string parameter");
        assert_eq!(param.example_value, r#""example""#);
    }

    #[test]
    fn test_extract_type_name_basic_types() {
        assert_eq!(
            SkillConverter::extract_type_name(&json!({"type": "string"})),
            "string"
        );
        assert_eq!(
            SkillConverter::extract_type_name(&json!({"type": "number"})),
            "number"
        );
        assert_eq!(
            SkillConverter::extract_type_name(&json!({"type": "boolean"})),
            "boolean"
        );
        assert_eq!(
            SkillConverter::extract_type_name(&json!({"type": "array"})),
            "array"
        );
        assert_eq!(
            SkillConverter::extract_type_name(&json!({"type": "object"})),
            "object"
        );
    }

    #[test]
    fn test_extract_type_name_enum() {
        let schema = json!({
            "enum": ["option1", "option2", "option3"]
        });
        assert_eq!(SkillConverter::extract_type_name(&schema), "string");
    }

    #[test]
    fn test_extract_type_name_oneof() {
        let schema = json!({
            "oneOf": [
                {"type": "string"},
                {"type": "number"}
            ]
        });
        assert_eq!(SkillConverter::extract_type_name(&schema), "any");
    }

    #[test]
    fn test_extract_type_name_no_type() {
        let schema = json!({
            "description": "A parameter"
        });
        assert_eq!(SkillConverter::extract_type_name(&schema), "any");
    }

    #[test]
    fn test_generate_example_value_string() {
        let schema = json!({"type": "string"});
        assert_eq!(
            SkillConverter::generate_example_value(&schema),
            r#""example""#
        );
    }

    #[test]
    fn test_generate_example_value_number() {
        let schema = json!({"type": "number"});
        assert_eq!(SkillConverter::generate_example_value(&schema), "42");
    }

    #[test]
    fn test_generate_example_value_integer() {
        let schema = json!({"type": "integer"});
        assert_eq!(SkillConverter::generate_example_value(&schema), "42");
    }

    #[test]
    fn test_generate_example_value_boolean() {
        let schema = json!({"type": "boolean"});
        assert_eq!(SkillConverter::generate_example_value(&schema), "true");
    }

    #[test]
    fn test_generate_example_value_array() {
        let schema = json!({"type": "array"});
        assert_eq!(SkillConverter::generate_example_value(&schema), "[]");
    }

    #[test]
    fn test_generate_example_value_object() {
        let schema = json!({"type": "object"});
        assert_eq!(SkillConverter::generate_example_value(&schema), "{}");
    }

    #[test]
    fn test_generate_example_value_null() {
        let schema = json!({"type": "null"});
        assert_eq!(SkillConverter::generate_example_value(&schema), "null");
    }

    #[test]
    fn test_generate_example_value_with_explicit_example() {
        let schema = json!({
            "type": "string",
            "example": "custom_example"
        });
        assert_eq!(
            SkillConverter::generate_example_value(&schema),
            r#""custom_example""#
        );
    }

    #[test]
    fn test_generate_example_value_enum() {
        let schema = json!({
            "enum": ["option1", "option2", "option3"]
        });
        assert_eq!(
            SkillConverter::generate_example_value(&schema),
            r#""option1""#
        );
    }

    #[test]
    fn test_generate_example_value_email_format() {
        let schema = json!({
            "type": "string",
            "format": "email"
        });
        assert_eq!(
            SkillConverter::generate_example_value(&schema),
            r#""user@example.com""#
        );
    }

    #[test]
    fn test_generate_example_value_uri_format() {
        let schema = json!({
            "type": "string",
            "format": "uri"
        });
        assert_eq!(
            SkillConverter::generate_example_value(&schema),
            r#""https://example.com""#
        );
    }

    #[test]
    fn test_generate_example_value_date_format() {
        let schema = json!({
            "type": "string",
            "format": "date"
        });
        assert_eq!(
            SkillConverter::generate_example_value(&schema),
            r#""2024-01-01""#
        );
    }

    #[test]
    fn test_generate_example_value_datetime_format() {
        let schema = json!({
            "type": "string",
            "format": "date-time"
        });
        assert_eq!(
            SkillConverter::generate_example_value(&schema),
            r#""2024-01-01T00:00:00Z""#
        );
    }

    #[test]
    fn test_generate_example_value_uuid_format() {
        let schema = json!({
            "type": "string",
            "format": "uuid"
        });
        assert_eq!(
            SkillConverter::generate_example_value(&schema),
            r#""550e8400-e29b-41d4-a716-446655440000""#
        );
    }

    #[test]
    fn test_convert_empty_tool_list() {
        let server_info = ServerInfo {
            id: ServerId::new("empty-server"),
            name: "empty-server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![],
            capabilities: ServerCapabilities {
                supports_tools: false,
                supports_resources: true,
                supports_prompts: true,
            },
        };

        let name = SkillName::new("empty-skill").unwrap();
        let desc = SkillDescription::new("Empty skill").unwrap();

        let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();
        assert_eq!(skill_data.tool_count, 0);
        assert_eq!(skill_data.tools.len(), 0);
        assert_eq!(skill_data.capabilities, vec!["resources", "prompts"]);
    }

    #[test]
    fn test_convert_multiple_tools() {
        let server_info = ServerInfo {
            id: ServerId::new("multi-tool-server"),
            name: "multi-tool-server".to_string(),
            version: "2.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: ToolName::new("tool1"),
                    description: "First tool".to_string(),
                    input_schema: json!({"type": "object", "properties": {}}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("tool2"),
                    description: "Second tool".to_string(),
                    input_schema: json!({"type": "object", "properties": {}}),
                    output_schema: None,
                },
                ToolInfo {
                    name: ToolName::new("tool3"),
                    description: "Third tool".to_string(),
                    input_schema: json!({"type": "object", "properties": {}}),
                    output_schema: None,
                },
            ],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: true,
                supports_prompts: true,
            },
        };

        let name = SkillName::new("multi-skill").unwrap();
        let desc = SkillDescription::new("Multi-tool skill").unwrap();

        let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();
        assert_eq!(skill_data.tool_count, 3);
        assert_eq!(skill_data.tools.len(), 3);
        assert_eq!(
            skill_data.capabilities,
            vec!["tools", "resources", "prompts"]
        );
    }

    #[test]
    fn test_convert_complex_parameter_types() {
        let tool = ToolInfo {
            name: ToolName::new("complex_tool"),
            description: "Tool with complex parameters".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "string_param": {
                        "type": "string",
                        "description": "A string"
                    },
                    "number_param": {
                        "type": "number",
                        "description": "A number"
                    },
                    "bool_param": {
                        "type": "boolean",
                        "description": "A boolean"
                    },
                    "array_param": {
                        "type": "array",
                        "description": "An array",
                        "items": {"type": "string"}
                    },
                    "object_param": {
                        "type": "object",
                        "description": "An object",
                        "properties": {
                            "nested": {"type": "string"}
                        }
                    }
                },
                "required": ["string_param", "number_param"]
            }),
            output_schema: None,
        };

        let tool_data = SkillConverter::convert_tool(&tool).unwrap();
        assert_eq!(tool_data.parameters.len(), 5);

        let string_param = tool_data
            .parameters
            .iter()
            .find(|p| p.name == "string_param")
            .unwrap();
        assert_eq!(string_param.type_name, "string");
        assert!(string_param.required);

        let array_param = tool_data
            .parameters
            .iter()
            .find(|p| p.name == "array_param")
            .unwrap();
        assert_eq!(array_param.type_name, "array");
        assert!(!array_param.required);

        let object_param = tool_data
            .parameters
            .iter()
            .find(|p| p.name == "object_param")
            .unwrap();
        assert_eq!(object_param.type_name, "object");
    }

    #[test]
    fn test_timestamp_generation() {
        let server_info = create_test_server_info();
        let name = SkillName::new("test-skill").unwrap();
        let desc = SkillDescription::new("Test skill").unwrap();

        let skill_data = SkillConverter::convert(&server_info, &name, &desc).unwrap();

        // Should be valid RFC3339 timestamp
        assert!(chrono::DateTime::parse_from_rfc3339(&skill_data.generated_at).is_ok());
    }
}
