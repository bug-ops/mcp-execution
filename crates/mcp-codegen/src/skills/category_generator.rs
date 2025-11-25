//! Category markdown generation for categorized skills.
//!
//! Generates individual category markdown files from tool definitions.

use crate::template_engine::TemplateEngine;
use mcp_core::{Result, SkillCategory};
use mcp_introspector::ToolInfo;
use serde::Serialize;
use std::collections::HashSet;

/// Generator for category markdown files.
///
/// Creates structured markdown files for each category, containing
/// tool documentation with parameters extracted from JSON schemas.
///
/// # Examples
///
/// ```no_run
/// use mcp_codegen::template_engine::TemplateEngine;
/// use mcp_codegen::skills::CategoryGenerator;
/// use mcp_core::SkillCategory;
/// use mcp_introspector::ToolInfo;
///
/// # fn example() -> mcp_core::Result<()> {
/// let engine = TemplateEngine::new()?;
/// let generator = CategoryGenerator::new(&engine);
///
/// let category = SkillCategory::new("repositories")?;
/// let tools: Vec<&ToolInfo> = vec![]; // Tool definitions
/// let description = "Repository management operations";
///
/// let markdown = generator.generate_category(&category, &tools, description)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct CategoryGenerator<'a> {
    engine: &'a TemplateEngine<'a>,
}

#[derive(Debug, Serialize)]
struct CategoryContext {
    category_name: String,
    category_description: String,
    tools: Vec<ToolContext>,
}

#[derive(Debug, Serialize)]
struct ToolContext {
    name: String,
    description: String,
    script_path: String,
    parameters: Vec<ParameterContext>,
}

#[derive(Debug, Serialize)]
struct ParameterContext {
    name: String,
    type_name: String,
    description: String,
    required: bool,
}

impl<'a> CategoryGenerator<'a> {
    /// Creates a new category generator.
    ///
    /// # Arguments
    ///
    /// * `engine` - Template engine for rendering markdown
    #[must_use]
    pub fn new(engine: &'a TemplateEngine<'a>) -> Self {
        Self { engine }
    }

    /// Generates markdown content for a category.
    ///
    /// # Arguments
    ///
    /// * `category` - The category to generate markdown for
    /// * `tools` - Tools belonging to this category
    /// * `category_description` - Human-readable category description
    ///
    /// # Returns
    ///
    /// Rendered markdown content for the category file.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Template rendering fails
    /// - Parameter extraction from JSON schema fails
    pub fn generate_category(
        &self,
        category: &SkillCategory,
        tools: &[&ToolInfo],
        category_description: &str,
    ) -> Result<String> {
        let tool_contexts = tools
            .iter()
            .map(|tool| self.build_tool_context(tool))
            .collect::<Result<Vec<_>>>()?;

        let context = CategoryContext {
            category_name: category.as_str().to_string(),
            category_description: category_description.to_string(),
            tools: tool_contexts,
        };

        self.engine.render("category_md", &context)
    }

    fn build_tool_context(&self, tool: &ToolInfo) -> Result<ToolContext> {
        let script_path = format!("scripts/{}.ts", tool.name.as_str());

        let parameters = self.extract_parameters(tool)?;

        Ok(ToolContext {
            name: tool.name.as_str().to_string(),
            description: tool.description.clone(),
            script_path,
            parameters,
        })
    }

    fn extract_parameters(&self, tool: &ToolInfo) -> Result<Vec<ParameterContext>> {
        // Extract from JSON schema
        let schema = &tool.input_schema;

        // Get properties object, return empty if not present
        let Some(properties_value) = schema.get("properties") else {
            return Ok(Vec::new());
        };
        let Some(properties) = properties_value.as_object() else {
            return Ok(Vec::new());
        };

        let required = schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<HashSet<_>>()
            })
            .unwrap_or_default();

        let mut params = Vec::new();
        for (name, prop) in properties {
            let type_name = prop
                .get("type")
                .and_then(|t| t.as_str())
                .unwrap_or("any")
                .to_string();

            let description = prop
                .get("description")
                .and_then(|d| d.as_str())
                .unwrap_or("")
                .to_string();

            params.push(ParameterContext {
                name: name.clone(),
                type_name,
                description,
                required: required.contains(name.as_str()),
            });
        }

        Ok(params)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::ToolName;
    use serde_json::json;

    fn create_test_tool(name: &str, params: serde_json::Value) -> ToolInfo {
        ToolInfo {
            name: ToolName::new(name),
            description: format!("Test tool {name}"),
            input_schema: params,
            output_schema: None,
        }
    }

    #[test]
    fn test_extract_parameters_simple() {
        let engine = TemplateEngine::new().unwrap();
        let generator = CategoryGenerator::new(&engine);

        let tool = create_test_tool(
            "test_tool",
            json!({
                "type": "object",
                "properties": {
                    "name": {
                        "type": "string",
                        "description": "User name"
                    },
                    "age": {
                        "type": "number",
                        "description": "User age"
                    }
                },
                "required": ["name"]
            }),
        );

        let params = generator.extract_parameters(&tool).unwrap();

        assert_eq!(params.len(), 2);
        let name_param = params.iter().find(|p| p.name == "name").unwrap();
        assert_eq!(name_param.type_name, "string");
        assert_eq!(name_param.description, "User name");
        assert!(name_param.required);

        let age_param = params.iter().find(|p| p.name == "age").unwrap();
        assert_eq!(age_param.type_name, "number");
        assert!(!age_param.required);
    }

    #[test]
    fn test_extract_parameters_no_required() {
        let engine = TemplateEngine::new().unwrap();
        let generator = CategoryGenerator::new(&engine);

        let tool = create_test_tool(
            "test_tool",
            json!({
                "type": "object",
                "properties": {
                    "optional_param": {
                        "type": "string"
                    }
                }
            }),
        );

        let params = generator.extract_parameters(&tool).unwrap();

        assert_eq!(params.len(), 1);
        assert!(!params[0].required);
    }

    #[test]
    fn test_extract_parameters_empty() {
        let engine = TemplateEngine::new().unwrap();
        let generator = CategoryGenerator::new(&engine);

        let tool = create_test_tool(
            "test_tool",
            json!({
                "type": "object",
                "properties": {}
            }),
        );

        let params = generator.extract_parameters(&tool).unwrap();

        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_build_tool_context() {
        let engine = TemplateEngine::new().unwrap();
        let generator = CategoryGenerator::new(&engine);

        let tool = create_test_tool(
            "create_issue",
            json!({
                "type": "object",
                "properties": {
                    "title": {
                        "type": "string",
                        "description": "Issue title"
                    }
                },
                "required": ["title"]
            }),
        );

        let context = generator.build_tool_context(&tool).unwrap();

        assert_eq!(context.name, "create_issue");
        assert_eq!(context.script_path, "scripts/create_issue.ts");
        assert_eq!(context.parameters.len(), 1);
        assert_eq!(context.parameters[0].name, "title");
    }
}
