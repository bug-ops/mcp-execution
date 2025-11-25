//! TypeScript script generation for MCP tools.
//!
//! This module generates executable TypeScript scripts for individual MCP tools,
//! separated from SKILL.md for progressive loading in Claude Code.

#![allow(dead_code)] // TODO: Remove when used in Phase 3 orchestrator

use crate::TemplateEngine;
use mcp_core::{Error, Result, ScriptFile};
use mcp_introspector::ToolInfo;
use serde::Serialize;
use serde_json::Value;

/// Context for rendering a single tool script.
#[derive(Debug, Clone, Serialize)]
pub struct ToolScriptContext {
    /// Tool name (snake_case)
    pub tool_name: String,
    /// Function name (camelCase from snake_case)
    pub function_name: String,
    /// Tool description
    pub description: String,
    /// Tool parameters
    pub parameters: Vec<ParameterScriptContext>,
    /// Required parameters only
    pub required_parameters: Vec<ParameterScriptContext>,
    /// Optional parameters only
    pub optional_parameters: Vec<ParameterScriptContext>,
    /// JSON Schema for input (pretty-printed)
    pub input_schema_json: String,
}

/// Parameter context for script templates.
#[derive(Debug, Clone, Serialize)]
pub struct ParameterScriptContext {
    /// Parameter name
    pub name: String,
    /// TypeScript type (e.g., "string", "number", "boolean")
    pub ts_type: String,
    /// Whether required
    pub required: bool,
    /// Description
    pub description: String,
    /// Example value as TypeScript literal
    pub example: String,
}

/// Generates TypeScript scripts for MCP tools.
///
/// Creates executable TypeScript files that wrap MCP tool calls
/// with proper typing and documentation.
#[derive(Debug)]
pub(crate) struct ScriptGenerator {
    engine: TemplateEngine<'static>,
}

impl ScriptGenerator {
    /// Creates a new script generator.
    ///
    /// # Errors
    ///
    /// Returns error if template registration fails.
    pub(crate) fn new() -> Result<Self> {
        let mut engine = TemplateEngine::new()?;

        // Register the tool script template
        engine.register_template_string(
            "tool_script",
            include_str!("../../templates/skills/tool_script.ts.hbs"),
        )?;

        Ok(Self { engine })
    }

    /// Generates a TypeScript script file for a tool.
    ///
    /// # Arguments
    ///
    /// * `tool` - Tool information from MCP introspection
    ///
    /// # Returns
    ///
    /// A `ScriptFile` ready for persistence.
    ///
    /// # Errors
    ///
    /// Returns error if template rendering fails.
    pub(crate) fn generate_script(&self, tool: &ToolInfo) -> Result<ScriptFile> {
        let context = self.build_context(tool)?;
        let content = self.engine.render("tool_script", &context)?;

        Ok(ScriptFile::new(tool.name.as_str(), "ts", content))
    }

    /// Generates scripts for all tools.
    ///
    /// # Arguments
    ///
    /// * `tools` - Iterator of tool information
    ///
    /// # Returns
    ///
    /// Vector of `ScriptFile` for all tools.
    ///
    /// # Errors
    ///
    /// Returns error if any script generation fails.
    pub(crate) fn generate_all<'t>(
        &self,
        tools: impl IntoIterator<Item = &'t ToolInfo>,
    ) -> Result<Vec<ScriptFile>> {
        tools
            .into_iter()
            .map(|tool| self.generate_script(tool))
            .collect()
    }

    fn build_context(&self, tool: &ToolInfo) -> Result<ToolScriptContext> {
        let parameters = self.extract_parameters(&tool.input_schema)?;
        let required_parameters: Vec<_> =
            parameters.iter().filter(|p| p.required).cloned().collect();
        let optional_parameters: Vec<_> =
            parameters.iter().filter(|p| !p.required).cloned().collect();

        Ok(ToolScriptContext {
            tool_name: tool.name.as_str().to_string(),
            function_name: to_camel_case(tool.name.as_str()),
            description: tool.description.clone(),
            parameters,
            required_parameters,
            optional_parameters,
            input_schema_json: serde_json::to_string_pretty(&tool.input_schema).map_err(|e| {
                Error::ScriptGenerationError {
                    tool: tool.name.as_str().to_string(),
                    message: format!("Failed to serialize input schema: {e}"),
                    source: Some(Box::new(e)),
                }
            })?,
        })
    }

    fn extract_parameters(&self, schema: &Value) -> Result<Vec<ParameterScriptContext>> {
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

        // Convert each property to ParameterScriptContext
        let mut parameters = Vec::new();
        for (name, prop_schema) in properties {
            let param = self.convert_parameter(name, prop_schema, &required_fields)?;
            parameters.push(param);
        }

        Ok(parameters)
    }

    fn convert_parameter(
        &self,
        name: &str,
        schema: &Value,
        required_fields: &[String],
    ) -> Result<ParameterScriptContext> {
        let type_name = extract_type_name(schema);
        let required = required_fields.contains(&name.to_string());
        let description = schema
            .get("description")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let example_value = generate_example_value(schema);

        Ok(ParameterScriptContext {
            name: name.to_string(),
            ts_type: json_schema_to_ts_type(&type_name),
            required,
            description,
            example: example_value,
        })
    }
}

/// Converts snake_case to camelCase.
fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;

    for ch in s.chars() {
        if ch == '_' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    result
}

/// Extracts type name from JSON Schema.
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

/// Maps JSON Schema type to TypeScript type.
fn json_schema_to_ts_type(json_type: &str) -> String {
    match json_type {
        "string" => "string",
        "number" | "integer" => "number",
        "boolean" => "boolean",
        "array" => "Array<any>",
        "object" => "Record<string, any>",
        "null" => "null",
        _ => "any",
    }
    .to_string()
}

/// Generates example value for a parameter based on its JSON Schema type.
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
    let type_name = extract_type_name(schema);
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

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::ToolName;
    use serde_json::json;

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("send_message"), "sendMessage");
        assert_eq!(to_camel_case("get_chat_info"), "getChatInfo");
        assert_eq!(to_camel_case("simple"), "simple");
        assert_eq!(to_camel_case(""), "");
    }

    #[test]
    fn test_extract_type_name_basic_types() {
        assert_eq!(extract_type_name(&json!({"type": "string"})), "string");
        assert_eq!(extract_type_name(&json!({"type": "number"})), "number");
        assert_eq!(extract_type_name(&json!({"type": "boolean"})), "boolean");
        assert_eq!(extract_type_name(&json!({"type": "array"})), "array");
        assert_eq!(extract_type_name(&json!({"type": "object"})), "object");
    }

    #[test]
    fn test_extract_type_name_enum() {
        let schema = json!({
            "enum": ["option1", "option2", "option3"]
        });
        assert_eq!(extract_type_name(&schema), "string");
    }

    #[test]
    fn test_extract_type_name_oneof() {
        let schema = json!({
            "oneOf": [
                {"type": "string"},
                {"type": "number"}
            ]
        });
        assert_eq!(extract_type_name(&schema), "any");
    }

    #[test]
    fn test_json_schema_to_ts_type() {
        assert_eq!(json_schema_to_ts_type("string"), "string");
        assert_eq!(json_schema_to_ts_type("number"), "number");
        assert_eq!(json_schema_to_ts_type("integer"), "number");
        assert_eq!(json_schema_to_ts_type("boolean"), "boolean");
        assert_eq!(json_schema_to_ts_type("array"), "Array<any>");
        assert_eq!(json_schema_to_ts_type("object"), "Record<string, any>");
        assert_eq!(json_schema_to_ts_type("unknown"), "any");
    }

    #[test]
    fn test_generate_example_value_string() {
        let schema = json!({"type": "string"});
        assert_eq!(generate_example_value(&schema), r#""example""#);
    }

    #[test]
    fn test_generate_example_value_number() {
        let schema = json!({"type": "number"});
        assert_eq!(generate_example_value(&schema), "42");
    }

    #[test]
    fn test_generate_example_value_email_format() {
        let schema = json!({
            "type": "string",
            "format": "email"
        });
        assert_eq!(generate_example_value(&schema), r#""user@example.com""#);
    }

    #[test]
    fn test_script_generator_new() {
        let generator = ScriptGenerator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_generate_script_basic() {
        let generator = ScriptGenerator::new().unwrap();
        let tool = ToolInfo {
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
                    }
                },
                "required": ["chat_id", "text"]
            }),
            output_schema: None,
        };

        let result = generator.generate_script(&tool);
        assert!(result.is_ok());

        let script = result.unwrap();
        assert_eq!(script.reference().tool_name(), "send_message");
        assert!(script.content().contains("sendMessage"));
        assert!(script.content().contains("export"));
    }

    #[test]
    fn test_extract_parameters() {
        let generator = ScriptGenerator::new().unwrap();
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

        let params = generator.extract_parameters(&schema).unwrap();
        assert_eq!(params.len(), 2);

        let required_param = params.iter().find(|p| p.name == "required_param").unwrap();
        assert!(required_param.required);
        assert_eq!(required_param.ts_type, "string");

        let optional_param = params.iter().find(|p| p.name == "optional_param").unwrap();
        assert!(!optional_param.required);
        assert_eq!(optional_param.ts_type, "number");
    }
}
