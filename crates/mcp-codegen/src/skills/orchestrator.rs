//! Skill bundle orchestration.
//!
//! Coordinates script generation, template rendering, and bundle assembly
//! to produce complete multi-file Claude Code skills.

use crate::TemplateEngine;
use crate::skills::script_generator::ScriptGenerator;
use mcp_core::{Result, ScriptFile, SkillBundle, SkillDescription, SkillName};
use mcp_introspector::ServerInfo;
use serde::Serialize;

/// Context for rendering SKILL.md template.
#[derive(Debug, Clone, Serialize)]
struct SkillMdContext {
    /// Skill name
    skill_name: String,
    /// Skill description
    skill_description: String,
    /// Server name
    server_name: String,
    /// Tool count
    tool_count: usize,
    /// Protocol version
    protocol_version: String,
    /// Generation timestamp
    generated_at: String,
    /// Tools with script references
    tools: Vec<ToolWithScript>,
}

/// Tool information with script reference.
#[derive(Debug, Clone, Serialize)]
struct ToolWithScript {
    /// Tool name
    name: String,
    /// Tool description
    description: String,
    /// Script filename (e.g., "send_message.ts")
    script_filename: String,
    /// Relative path to script (e.g., "scripts/send_message.ts")
    script_path: String,
    /// Parameters for documentation
    parameters: Vec<ParameterInfo>,
}

/// Parameter information for documentation.
#[derive(Debug, Clone, Serialize)]
struct ParameterInfo {
    /// Parameter name
    name: String,
    /// Type name (e.g., "string", "number")
    type_name: String,
    /// Whether required
    required: bool,
    /// Description
    description: String,
    /// Example value
    example_value: String,
}

/// Orchestrates complete skill bundle generation.
///
/// Coordinates script generation, template rendering, and bundle assembly
/// to produce a complete multi-file skill ready for persistence.
///
/// # Examples
///
/// ```no_run
/// use mcp_codegen::skills::SkillOrchestrator;
/// use mcp_core::{SkillName, SkillDescription};
/// use mcp_introspector::ServerInfo;
///
/// # fn example(server_info: &ServerInfo) -> Result<(), mcp_core::Error> {
/// let orchestrator = SkillOrchestrator::new()?;
/// let skill_name = SkillName::new("github")?;
/// let skill_description = SkillDescription::new("GitHub integration")?;
///
/// let bundle = orchestrator.generate_bundle(
///     server_info,
///     &skill_name,
///     &skill_description,
/// )?;
///
/// println!("Generated {} scripts", bundle.scripts().len());
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SkillOrchestrator {
    script_generator: ScriptGenerator,
    template_engine: TemplateEngine<'static>,
}

impl SkillOrchestrator {
    /// Creates a new orchestrator with default configuration.
    ///
    /// # Errors
    ///
    /// Returns error if template registration fails.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_codegen::skills::SkillOrchestrator;
    ///
    /// let orchestrator = SkillOrchestrator::new()?;
    /// # Ok::<(), mcp_core::Error>(())
    /// ```
    pub fn new() -> Result<Self> {
        let mut template_engine = TemplateEngine::new()?;

        // Register updated SKILL.md template (with script references)
        template_engine.register_template_string(
            "skill_md_multifile",
            include_str!("../../templates/skills/skill_multifile.md.hbs"),
        )?;

        // Register REFERENCE.md template (reuse existing)
        template_engine.register_template_string(
            "reference_md",
            include_str!("../../templates/claude/reference.md.hbs"),
        )?;

        Ok(Self {
            script_generator: ScriptGenerator::new()?,
            template_engine,
        })
    }

    /// Generates a complete skill bundle from server info.
    ///
    /// This is the main entry point for skill generation. It:
    /// 1. Generates TypeScript scripts for each tool
    /// 2. Renders SKILL.md with script references
    /// 3. Renders REFERENCE.md with API documentation
    /// 4. Assembles everything into a `SkillBundle`
    ///
    /// # Arguments
    ///
    /// * `server_info` - MCP server information with tools
    /// * `skill_name` - Name for the skill
    /// * `skill_description` - Description for the skill
    ///
    /// # Returns
    ///
    /// A `SkillBundle` containing SKILL.md, REFERENCE.md, and all scripts.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Script generation fails
    /// - Template rendering fails
    /// - Bundle assembly fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::SkillOrchestrator;
    /// use mcp_core::{SkillName, SkillDescription};
    /// # use mcp_introspector::ServerInfo;
    ///
    /// # fn example(server_info: &ServerInfo) -> Result<(), mcp_core::Error> {
    /// let orchestrator = SkillOrchestrator::new()?;
    /// let name = SkillName::new("github")?;
    /// let desc = SkillDescription::new("GitHub integration")?;
    ///
    /// let bundle = orchestrator.generate_bundle(server_info, &name, &desc)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate_bundle(
        &self,
        server_info: &ServerInfo,
        skill_name: &SkillName,
        skill_description: &SkillDescription,
    ) -> Result<SkillBundle> {
        // Step 1: Generate scripts for all tools
        let scripts = self.generate_scripts(&server_info.tools)?;

        // Step 2: Render SKILL.md with script references
        let skill_md =
            self.render_skill_md(skill_name, skill_description, server_info, &scripts)?;

        // Step 3: Render REFERENCE.md
        let reference_md = self.render_reference_md(server_info)?;

        // Step 4: Build and return bundle
        let bundle = SkillBundle::builder(skill_name.as_str())?
            .skill_md(skill_md)
            .scripts(scripts)
            .reference_md(reference_md)
            .build();

        Ok(bundle)
    }

    fn generate_scripts(&self, tools: &[mcp_introspector::ToolInfo]) -> Result<Vec<ScriptFile>> {
        self.script_generator.generate_all(tools)
    }

    fn render_skill_md(
        &self,
        skill_name: &SkillName,
        skill_description: &SkillDescription,
        server_info: &ServerInfo,
        scripts: &[ScriptFile],
    ) -> Result<String> {
        // Build context with script references
        let tools_with_scripts: Vec<ToolWithScript> = server_info
            .tools
            .iter()
            .zip(scripts.iter())
            .map(|(tool, script)| {
                let parameters = self.extract_parameters_info(tool)?;
                Ok(ToolWithScript {
                    name: tool.name.as_str().to_string(),
                    description: tool.description.clone(),
                    script_filename: script.reference().filename(),
                    script_path: script.reference().relative_path(),
                    parameters,
                })
            })
            .collect::<Result<Vec<_>>>()?;

        let context = SkillMdContext {
            skill_name: skill_name.as_str().to_string(),
            skill_description: skill_description.as_str().to_string(),
            server_name: server_info.name.clone(),
            tool_count: server_info.tools.len(),
            protocol_version: "2024-11-05".to_string(), // Default MCP protocol version
            generated_at: chrono::Utc::now().to_rfc3339(),
            tools: tools_with_scripts,
        };

        self.template_engine.render("skill_md_multifile", &context)
    }

    fn render_reference_md(&self, server_info: &ServerInfo) -> Result<String> {
        // Reuse existing converter for REFERENCE.md data
        use crate::skills::converter::SkillConverter;

        // Create temporary SkillData for rendering
        let temp_name = SkillName::new(server_info.id.as_str())?;
        let temp_desc =
            mcp_core::SkillDescription::new(&format!("Reference for {}", server_info.name))?;

        let skill_data = SkillConverter::convert(server_info, &temp_name, &temp_desc)?;

        self.template_engine.render("reference_md", &skill_data)
    }

    fn extract_parameters_info(
        &self,
        tool: &mcp_introspector::ToolInfo,
    ) -> Result<Vec<ParameterInfo>> {
        use serde_json::Value;

        let schema = &tool.input_schema;

        // Get required fields
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

        // Get properties
        let Some(Value::Object(properties)) = schema.get("properties") else {
            return Ok(Vec::new()); // No parameters
        };

        // Convert to ParameterInfo
        let mut params = Vec::new();
        for (name, prop_schema) in properties {
            let type_name = extract_type_name(prop_schema);
            let required = required_fields.contains(name);
            let description = prop_schema
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("")
                .to_string();
            let example_value = generate_example_value(prop_schema);

            params.push(ParameterInfo {
                name: name.clone(),
                type_name,
                required,
                description,
                example_value,
            });
        }

        Ok(params)
    }

    /// Generates a categorized skill bundle (synchronous).
    ///
    /// Uses dictionary-based or universal categorization to organize tools into categories.
    ///
    /// # Arguments
    ///
    /// * `server_info` - MCP server information with tools
    /// * `skill_name` - Name for the skill
    /// * `skill_description` - Description for the skill
    ///
    /// # Returns
    ///
    /// A `CategorizedSkillBundle` containing manifest, category files, and scripts.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Manifest generation fails
    /// - Category markdown generation fails
    /// - Script generation fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_codegen::skills::SkillOrchestrator;
    /// use mcp_core::{SkillName, SkillDescription};
    /// # use mcp_introspector::ServerInfo;
    ///
    /// # fn example(server_info: &ServerInfo) -> Result<(), mcp_core::Error> {
    /// let orchestrator = SkillOrchestrator::new()?;
    /// let name = SkillName::new("github")?;
    /// let desc = SkillDescription::new("GitHub integration")?;
    ///
    /// let bundle = orchestrator.generate_categorized_bundle(
    ///     server_info,
    ///     &name,
    ///     &desc,
    /// )?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate_categorized_bundle(
        &self,
        server_info: &ServerInfo,
        skill_name: &SkillName,
        skill_description: &SkillDescription,
    ) -> Result<mcp_core::CategorizedSkillBundle> {
        use crate::skills::{CategoryGenerator, ManifestGenerator};
        use std::collections::HashMap;

        // Step 1: Generate scripts for all tools
        let scripts = self.generate_scripts(&server_info.tools)?;

        // Step 2: Generate manifest (auto-detect strategy)
        let manifest_gen = ManifestGenerator::auto()?;
        let manifest = manifest_gen.generate(&server_info.tools)?;

        // Step 3: Generate category markdown files
        let category_gen = CategoryGenerator::new(&self.template_engine);
        let mut categories = HashMap::new();

        for (category, tool_names) in manifest.categories() {
            let tools: Vec<_> = server_info
                .tools
                .iter()
                .filter(|t| tool_names.contains(&t.name.as_str().to_string()))
                .collect();

            let description = Self::get_category_description(category);
            let content = category_gen.generate_category(category, &tools, &description)?;
            categories.insert(category.clone(), content);
        }

        // Step 4: Render minimal SKILL.md
        let skill_md =
            self.render_categorized_skill_md(skill_name, skill_description, &manifest)?;

        // Step 5: Render REFERENCE.md (optional)
        let reference_md = self.render_reference_md(server_info)?;

        // Step 6: Build bundle
        Ok(
            mcp_core::CategorizedSkillBundle::builder(skill_name.as_str())?
                .skill_md(skill_md)
                .manifest(manifest)
                .categories(categories)
                .scripts(scripts)
                .reference_md(reference_md)
                .build(),
        )
    }

    /// Generates a categorized skill bundle with LLM assistance (asynchronous).
    ///
    /// Uses LLM-based categorization for intelligent tool grouping.
    ///
    /// # Arguments
    ///
    /// * `server_info` - MCP server information with tools
    /// * `skill_name` - Name for the skill
    /// * `skill_description` - Description for the skill
    /// * `model_name` - LLM model to use (e.g., "claude-sonnet-4")
    /// * `max_categories` - Maximum number of categories
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - LLM categorization fails
    /// - Manifest generation fails
    /// - Category markdown generation fails
    pub async fn generate_categorized_bundle_async(
        &self,
        server_info: &ServerInfo,
        skill_name: &SkillName,
        skill_description: &SkillDescription,
        model_name: &str,
        max_categories: usize,
    ) -> Result<mcp_core::CategorizedSkillBundle> {
        use crate::skills::{CategoryGenerator, ManifestGenerator};
        use std::collections::HashMap;

        // Step 1: Generate scripts for all tools
        let scripts = self.generate_scripts(&server_info.tools)?;

        // Step 2: Generate manifest with LLM
        let manifest_gen = ManifestGenerator::with_llm(model_name, max_categories)?;
        let manifest = manifest_gen.generate_async(&server_info.tools).await?;

        // Step 3: Generate category markdown files
        let category_gen = CategoryGenerator::new(&self.template_engine);
        let mut categories = HashMap::new();

        for (category, tool_names) in manifest.categories() {
            let tools: Vec<_> = server_info
                .tools
                .iter()
                .filter(|t| tool_names.contains(&t.name.as_str().to_string()))
                .collect();

            let description = Self::get_category_description(category);
            let content = category_gen.generate_category(category, &tools, &description)?;
            categories.insert(category.clone(), content);
        }

        // Step 4: Render minimal SKILL.md
        let skill_md =
            self.render_categorized_skill_md(skill_name, skill_description, &manifest)?;

        // Step 5: Render REFERENCE.md (optional)
        let reference_md = self.render_reference_md(server_info)?;

        // Step 6: Build bundle
        Ok(
            mcp_core::CategorizedSkillBundle::builder(skill_name.as_str())?
                .skill_md(skill_md)
                .manifest(manifest)
                .categories(categories)
                .scripts(scripts)
                .reference_md(reference_md)
                .build(),
        )
    }

    fn render_categorized_skill_md(
        &self,
        skill_name: &SkillName,
        skill_description: &SkillDescription,
        manifest: &mcp_core::CategoryManifest,
    ) -> Result<String> {
        #[derive(serde::Serialize)]
        struct Context {
            skill_name: String,
            skill_description: String,
            categories: Vec<CategoryInfo>,
        }

        #[derive(serde::Serialize)]
        struct CategoryInfo {
            name: String,
            description: String,
            filename: String,
            tool_count: usize,
        }

        let category_infos: Vec<_> = manifest
            .categories()
            .iter()
            .map(|(cat, tools)| CategoryInfo {
                name: cat.as_str().to_string(),
                description: Self::get_category_description(cat),
                filename: cat.filename(),
                tool_count: tools.len(),
            })
            .collect();

        let context = Context {
            skill_name: skill_name.as_str().to_string(),
            skill_description: skill_description.as_str().to_string(),
            categories: category_infos,
        };

        self.template_engine
            .render("skill_categorized_md", &context)
    }

    fn get_category_description(category: &mcp_core::SkillCategory) -> String {
        // Generate description based on category name
        match category.as_str() {
            "create" => "Creation and initialization operations",
            "read" => "Read and retrieval operations",
            "update" => "Update and modification operations",
            "delete" => "Deletion and cleanup operations",
            "search" => "Search and query operations",
            "users" => "User and account management",
            "files" => "File and document operations",
            "messages" => "Messaging and communication",
            "issues" => "Issue and task tracking",
            "repositories" => "Repository management",
            "pull_requests" => "Pull request operations",
            "branches" => "Branch management",
            "commits" => "Commit operations",
            "reviews" => "Code review operations",
            "workflows" => "Workflow and automation",
            "deployments" => "Deployment operations",
            "releases" => "Release management",
            "projects" => "Project management",
            "teams" => "Team management",
            "organizations" => "Organization management",
            _ => "Additional operations",
        }
        .to_string()
    }
}

/// Extracts type name from JSON Schema.
fn extract_type_name(schema: &serde_json::Value) -> String {
    use serde_json::Value;

    if let Some(type_str) = schema.get("type").and_then(Value::as_str) {
        return type_str.to_string();
    }

    if schema.get("enum").is_some() {
        return "string".to_string();
    }

    if schema.get("oneOf").is_some() || schema.get("anyOf").is_some() {
        return "any".to_string();
    }

    "any".to_string()
}

/// Generates example value for documentation.
fn generate_example_value(schema: &serde_json::Value) -> String {
    use serde_json::Value;

    // Check for explicit example
    if let Some(example) = schema.get("example")
        && let Ok(example_str) = serde_json::to_string(example)
    {
        return example_str;
    }

    // Check for enum
    if let Some(Value::Array(enum_values)) = schema.get("enum")
        && let Some(first) = enum_values.first()
        && let Ok(value_str) = serde_json::to_string(first)
    {
        return value_str;
    }

    // Generate based on type
    let type_name = extract_type_name(schema);
    match type_name.as_str() {
        "string" => r#""example""#.to_string(),
        "number" | "integer" => "42".to_string(),
        "boolean" => "true".to_string(),
        "array" => "[]".to_string(),
        "object" => "{}".to_string(),
        _ => r#""value""#.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_core::ToolName;
    use mcp_introspector::ToolInfo;
    use serde_json::json;

    fn create_test_server_info() -> ServerInfo {
        ServerInfo {
            id: mcp_core::ServerId::new("test-server"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: mcp_introspector::ServerCapabilities {
                supports_tools: true,
                supports_prompts: false,
                supports_resources: false,
            },
            tools: vec![ToolInfo {
                name: ToolName::new("send_message"),
                description: "Sends a message".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "text": {
                            "type": "string",
                            "description": "Message text"
                        }
                    },
                    "required": ["text"]
                }),
                output_schema: None,
            }],
        }
    }

    #[test]
    fn test_orchestrator_new() {
        let result = SkillOrchestrator::new();
        assert!(result.is_ok());
    }

    #[test]
    fn test_generate_bundle_basic() {
        let orchestrator = SkillOrchestrator::new().unwrap();
        let server_info = create_test_server_info();
        let skill_name = SkillName::new("test").unwrap();
        let skill_description = SkillDescription::new("Test skill").unwrap();

        let result = orchestrator.generate_bundle(&server_info, &skill_name, &skill_description);
        assert!(result.is_ok());

        let bundle = result.unwrap();
        assert_eq!(bundle.name().as_str(), "test");
        assert_eq!(bundle.scripts().len(), 1);
        assert!(bundle.skill_md().contains("test"));
        assert!(bundle.reference_md().is_some());
    }

    #[test]
    fn test_skill_md_contains_script_references() {
        let orchestrator = SkillOrchestrator::new().unwrap();
        let server_info = create_test_server_info();
        let skill_name = SkillName::new("test").unwrap();
        let skill_description = SkillDescription::new("Test skill").unwrap();

        let bundle = orchestrator
            .generate_bundle(&server_info, &skill_name, &skill_description)
            .unwrap();

        let skill_md = bundle.skill_md();
        assert!(skill_md.contains("scripts/"));
        assert!(skill_md.contains("send_message.ts"));
    }

    #[test]
    fn test_extract_type_name() {
        assert_eq!(extract_type_name(&json!({"type": "string"})), "string");
        assert_eq!(extract_type_name(&json!({"type": "number"})), "number");
        assert_eq!(extract_type_name(&json!({"enum": ["a", "b"]})), "string");
        assert_eq!(extract_type_name(&json!({"oneOf": []})), "any");
        assert_eq!(extract_type_name(&json!({})), "any");
    }

    #[test]
    fn test_generate_example_value() {
        assert_eq!(
            generate_example_value(&json!({"type": "string"})),
            r#""example""#
        );
        assert_eq!(generate_example_value(&json!({"type": "number"})), "42");
        assert_eq!(generate_example_value(&json!({"type": "boolean"})), "true");
        assert_eq!(generate_example_value(&json!({"type": "array"})), "[]");
        assert_eq!(generate_example_value(&json!({"type": "object"})), "{}");
    }

    #[test]
    fn test_generate_example_value_with_explicit_example() {
        let schema = json!({
            "type": "string",
            "example": "custom-value"
        });
        assert_eq!(generate_example_value(&schema), r#""custom-value""#);
    }

    #[test]
    fn test_generate_example_value_with_enum() {
        let schema = json!({
            "enum": ["option1", "option2"]
        });
        assert_eq!(generate_example_value(&schema), r#""option1""#);
    }

    #[test]
    fn test_extract_parameters_info() {
        let orchestrator = SkillOrchestrator::new().unwrap();
        let tool = ToolInfo {
            name: ToolName::new("test_tool"),
            description: "Test".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "param1": {
                        "type": "string",
                        "description": "First parameter"
                    },
                    "param2": {
                        "type": "number",
                        "description": "Second parameter"
                    }
                },
                "required": ["param1"]
            }),
            output_schema: None,
        };

        let params = orchestrator.extract_parameters_info(&tool).unwrap();
        assert_eq!(params.len(), 2);

        let param1 = params.iter().find(|p| p.name == "param1").unwrap();
        assert_eq!(param1.type_name, "string");
        assert!(param1.required);

        let param2 = params.iter().find(|p| p.name == "param2").unwrap();
        assert_eq!(param2.type_name, "number");
        assert!(!param2.required);
    }

    #[test]
    fn test_generate_bundle_with_multiple_tools() {
        let orchestrator = SkillOrchestrator::new().unwrap();
        let mut server_info = create_test_server_info();
        server_info.tools.push(ToolInfo {
            name: ToolName::new("get_info"),
            description: "Gets information".to_string(),
            input_schema: json!({"type": "object", "properties": {}}),
            output_schema: None,
        });

        let skill_name = SkillName::new("test").unwrap();
        let skill_description = SkillDescription::new("Test skill").unwrap();

        let bundle = orchestrator
            .generate_bundle(&server_info, &skill_name, &skill_description)
            .unwrap();

        assert_eq!(bundle.scripts().len(), 2);
        assert!(bundle.skill_md().contains("send_message"));
        assert!(bundle.skill_md().contains("get_info"));
    }

    #[test]
    fn test_bundle_has_all_required_files() {
        let orchestrator = SkillOrchestrator::new().unwrap();
        let server_info = create_test_server_info();
        let skill_name = SkillName::new("test").unwrap();
        let skill_description = SkillDescription::new("Test skill").unwrap();

        let bundle = orchestrator
            .generate_bundle(&server_info, &skill_name, &skill_description)
            .unwrap();

        // Check SKILL.md exists and has content
        assert!(!bundle.skill_md().is_empty());

        // Check scripts exist
        assert!(!bundle.scripts().is_empty());

        // Check REFERENCE.md exists
        assert!(bundle.reference_md().is_some());
        assert!(!bundle.reference_md().unwrap().is_empty());
    }
}
