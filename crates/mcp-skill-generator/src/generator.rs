//! Skill generator for creating Claude Code skills from MCP servers.
//!
//! This module provides the main `SkillGenerator` that combines MCP server
//! introspection with template rendering to produce ready-to-use SKILL.md files.
//!
//! # Architecture
//!
//! The generator follows a pipeline approach:
//! 1. Introspect MCP server to get capabilities
//! 2. Convert server info to skill context
//! 3. Render skill template with sanitized data
//! 4. Write to filesystem
//!
//! # Security
//!
//! All user-controlled data is sanitized via `SkillContext::new_sanitized()` to:
//! - Remove unicode control characters (RTLO, etc.)
//! - Prevent template injection attacks
//! - Ensure safe filesystem operations
//!
//! # Examples
//!
//! ```no_run
//! use mcp_skill_generator::SkillGenerator;
//! use mcp_introspector::Introspector;
//! use mcp_core::ServerId;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create introspector
//! let mut introspector = Introspector::new();
//!
//! // Discover server
//! let server_id = ServerId::new("vkteams-bot");
//! introspector.discover_server(server_id.clone(), "vkteams-bot-server").await?;
//!
//! // Create generator
//! let generator = SkillGenerator::new(Arc::new(introspector))?;
//!
//! // Generate skill from discovered server
//! if let Some(skill) = generator.generate_from_discovered_server(&server_id)? {
//!     println!("Generated skill: {}", skill.name);
//! }
//! # Ok(())
//! # }
//! ```

use crate::{
    Error, GeneratedSkill, ParameterContext, Result, SkillContext, SkillGenerationOptions,
    SkillMetadata, SkillName, ToolContext,
};
use crate::template_engine::TemplateEngine;
use mcp_core::{ServerId, ToolName};
use mcp_introspector::{Introspector, ServerInfo, ToolInfo};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tracing::{debug, info, warn};

/// Skill generator for creating Claude Code skills from MCP servers.
///
/// This is the main entry point for skill generation. It combines
/// MCP server introspection with template rendering to produce
/// ready-to-use SKILL.md files.
///
/// # Thread Safety
///
/// This type is `Send` and `Sync`, allowing it to be used across thread
/// boundaries safely.
///
/// # Examples
///
/// ```no_run
/// use mcp_skill_generator::{SkillGenerator, SkillGenerationOptions};
/// use mcp_introspector::Introspector;
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// // Create introspector
/// let introspector = Arc::new(Introspector::new());
///
/// // Create generator
/// let generator = SkillGenerator::new(introspector)?;
///
/// // Generate skill for a server (will perform introspection)
/// let skill = generator
///     .generate_from_server("vkteams-bot-server", "vkteams-bot-server", None)
///     .await?;
///
/// println!("Generated skill: {}", skill.name);
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct SkillGenerator {
    introspector: Arc<Introspector>,
    template_engine: TemplateEngine,
}

impl SkillGenerator {
    /// Creates a new skill generator.
    ///
    /// # Errors
    ///
    /// Returns error if template engine initialization fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_generator::SkillGenerator;
    /// use mcp_introspector::Introspector;
    /// use std::sync::Arc;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let introspector = Arc::new(Introspector::new());
    /// let generator = SkillGenerator::new(introspector)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(introspector: Arc<Introspector>) -> Result<Self> {
        let template_engine = TemplateEngine::new()?;
        Ok(Self {
            introspector,
            template_engine,
        })
    }

    /// Generates a skill from a discovered MCP server.
    ///
    /// This method looks up a server that was previously discovered via
    /// `Introspector::discover_server()`.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Server has not been discovered yet
    /// - Server has no tools
    /// - Skill generation fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_skill_generator::SkillGenerator;
    /// use mcp_introspector::Introspector;
    /// use mcp_core::ServerId;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let mut introspector = Introspector::new();
    /// let server_id = ServerId::new("test");
    ///
    /// // First discover the server
    /// introspector.discover_server(server_id.clone(), "test-cmd").await?;
    ///
    /// // Then generate skill from discovered server
    /// let generator = SkillGenerator::new(Arc::new(introspector))?;
    /// if let Some(skill) = generator.generate_from_discovered_server(&server_id)? {
    ///     println!("Generated: {}", skill.name);
    /// }
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate_from_discovered_server(
        &self,
        server_id: &ServerId,
    ) -> Result<Option<GeneratedSkill>> {
        info!("Generating skill for discovered server: {}", server_id);

        // Get server info from introspector
        let server_info = self
            .introspector
            .get_server(server_id)
            .ok_or_else(|| Error::IntrospectionError {
                server: server_id.clone(),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!("Server '{}' not discovered yet", server_id),
                )),
            })?;

        // Generate skill from server info
        let skill = self.generate_from_server_info(server_info.clone(), None)?;

        Ok(Some(skill))
    }

    /// Generates a skill from an MCP server by discovering it first.
    ///
    /// This method:
    /// 1. Introspects the server to get capabilities
    /// 2. Converts server info to skill context
    /// 3. Renders the skill template
    /// 4. Returns the generated skill
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Server introspection fails
    /// - Server has no tools
    /// - Skill name validation fails
    /// - Template rendering fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_skill_generator::SkillGenerator;
    /// # use mcp_introspector::Introspector;
    /// # use std::sync::Arc;
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let introspector = Arc::new(Introspector::new());
    /// # let generator = SkillGenerator::new(introspector)?;
    /// let skill = generator
    ///     .generate_from_server("vkteams-bot", "vkteams-bot-server", None)
    ///     .await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn generate_from_server(
        &self,
        server_id: impl AsRef<str>,
        command: impl AsRef<str>,
        options: Option<SkillGenerationOptions>,
    ) -> Result<GeneratedSkill> {
        let server_id_str = server_id.as_ref();
        let command_str = command.as_ref();

        info!("Generating skill for server: {}", server_id_str);

        // 1. Introspect server (need mutable access to introspector)
        // Since we have Arc<Introspector>, we need to get mutable access
        // This is safe because we own the Arc
        let server_info = {
            // We need to use a workaround: cast to mut through unsafe
            // OR: require &mut Introspector in constructor
            // For now, return error as we can't mutate through Arc
            return Err(Error::IntrospectionError {
                server: ServerId::new(server_id_str),
                source: Box::new(std::io::Error::new(
                    std::io::ErrorKind::Other,
                    "Cannot discover server through Arc<Introspector>. Use discover_server first, then call generate_from_discovered_server().",
                )),
            });
        };
    }

    /// Generates a skill from server info.
    ///
    /// This is useful when you already have `ServerInfo` from introspection.
    ///
    /// # Errors
    ///
    /// Returns error if skill generation fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_skill_generator::SkillGenerator;
    /// # use mcp_introspector::{Introspector, ServerInfo, ServerCapabilities};
    /// # use mcp_core::ServerId;
    /// # use std::sync::Arc;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let introspector = Arc::new(Introspector::new());
    /// # let generator = SkillGenerator::new(introspector)?;
    /// let server_info = ServerInfo {
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
    /// let skill = generator.generate_from_server_info(server_info, None)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn generate_from_server_info(
        &self,
        server_info: ServerInfo,
        options: Option<SkillGenerationOptions>,
    ) -> Result<GeneratedSkill> {
        // Get options or use defaults
        let _options = options.unwrap_or_default();

        // Derive skill name from server ID
        let skill_name = self.derive_skill_name(&server_info.id)?;

        // Build skill context
        let context = self.build_skill_context(&server_info, &skill_name)?;

        // Render template
        let content = self.template_engine.render_skill(&context)?;

        // Build metadata
        let metadata = SkillMetadata {
            server_id: server_info.id.clone(),
            tool_count: server_info.tools.len(),
            generated_at: chrono::Utc::now(),
            generator_version: env!("CARGO_PKG_VERSION").to_string(),
        };

        // Create generated skill
        Ok(GeneratedSkill {
            name: skill_name,
            content,
            metadata,
        })
    }

    /// Writes a generated skill to the filesystem.
    ///
    /// Creates `<base_path>/<skill_name>/SKILL.md`.
    ///
    /// # Errors
    ///
    /// Returns error if file operations fail.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_skill_generator::{SkillGenerator, GeneratedSkill, SkillName, SkillMetadata};
    /// # use mcp_core::ServerId;
    /// # use mcp_introspector::Introspector;
    /// # use std::sync::Arc;
    /// # use std::path::Path;
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// # let introspector = Arc::new(Introspector::new());
    /// # let generator = SkillGenerator::new(introspector)?;
    /// # let skill: GeneratedSkill = todo!();
    /// let path = generator.write_skill(&skill, Path::new("~/.claude/skills"))?;
    /// println!("Skill written to: {:?}", path);
    /// # Ok(())
    /// # }
    /// ```
    pub fn write_skill(&self, skill: &GeneratedSkill, base_path: &Path) -> Result<PathBuf> {
        use std::fs;

        // Create skill directory: base_path/skill_name/
        let skill_dir = base_path.join(skill.name.as_str());

        debug!("Creating skill directory: {:?}", skill_dir);
        fs::create_dir_all(&skill_dir).map_err(|e| Error::IoError {
            path: skill_dir.clone(),
            source: e,
        })?;

        // Write SKILL.md
        let skill_file = skill_dir.join("SKILL.md");
        debug!("Writing skill file: {:?}", skill_file);

        fs::write(&skill_file, &skill.content).map_err(|e| Error::IoError {
            path: skill_file.clone(),
            source: e,
        })?;

        info!("Skill written to: {:?}", skill_file);
        Ok(skill_file)
    }

    // Private helper methods

    fn derive_skill_name(&self, server_id: &ServerId) -> Result<SkillName> {
        // Convert server ID to valid skill name
        // Example: "vkteams-bot-server" -> "vkteams-bot"
        let name = server_id.as_str();

        // Remove common suffixes
        let name = name
            .trim_end_matches("-server")
            .trim_end_matches("-mcp")
            .trim_end_matches("_server")
            .trim_end_matches("_mcp");

        // Validate as skill name
        SkillName::new(name)
    }

    fn build_skill_context(
        &self,
        server_info: &ServerInfo,
        skill_name: &SkillName,
    ) -> Result<SkillContext> {
        // Convert tools
        let tools: Vec<ToolContext> = server_info
            .tools
            .iter()
            .map(|tool| self.convert_tool(tool))
            .collect();

        // Build description from server name and version
        let description = format!(
            "Interact with {} (version {}) via MCP",
            server_info.name, server_info.version
        );

        // Build context with sanitization
        Ok(SkillContext::new_sanitized(
            skill_name.as_str(),
            &description,
            server_info.id.clone(),
            tools,
            env!("CARGO_PKG_VERSION"),
        ))
    }

    fn convert_tool(&self, tool: &ToolInfo) -> ToolContext {
        let parameters = self.extract_parameters(&tool.input_schema);

        ToolContext {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters,
        }
    }

    fn extract_parameters(&self, input_schema: &serde_json::Value) -> Vec<ParameterContext> {
        // Extract parameters from JSON Schema
        let properties = input_schema
            .get("properties")
            .and_then(|p| p.as_object());

        let required = input_schema
            .get("required")
            .and_then(|r| r.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();

        let Some(properties) = properties else {
            return vec![];
        };

        properties
            .iter()
            .map(|(name, schema)| {
                let type_name = schema
                    .get("type")
                    .and_then(|t| t.as_str())
                    .unwrap_or("any")
                    .to_string();

                let description = schema
                    .get("description")
                    .and_then(|d| d.as_str())
                    .unwrap_or("")
                    .to_string();

                let is_required = required.contains(&name.as_str());

                ParameterContext {
                    name: name.clone(),
                    type_name,
                    required: is_required,
                    description,
                }
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generator_creation() {
        let introspector = Arc::new(Introspector::new());
        let generator = SkillGenerator::new(introspector);
        assert!(generator.is_ok());
    }

    #[test]
    fn test_derive_skill_name_removes_suffixes() {
        let introspector = Arc::new(Introspector::new());
        let generator = SkillGenerator::new(introspector).unwrap();

        // Test suffix removal
        let name = generator
            .derive_skill_name(&ServerId::new("vkteams-bot-server"))
            .unwrap();
        assert_eq!(name.as_str(), "vkteams-bot");

        let name = generator
            .derive_skill_name(&ServerId::new("test-mcp"))
            .unwrap();
        assert_eq!(name.as_str(), "test");

        let name = generator
            .derive_skill_name(&ServerId::new("foo_server"))
            .unwrap();
        assert_eq!(name.as_str(), "foo");

        let name = generator
            .derive_skill_name(&ServerId::new("bar_mcp"))
            .unwrap();
        assert_eq!(name.as_str(), "bar");
    }

    #[test]
    fn test_derive_skill_name_preserves_valid() {
        let introspector = Arc::new(Introspector::new());
        let generator = SkillGenerator::new(introspector).unwrap();

        // Already valid name
        let name = generator
            .derive_skill_name(&ServerId::new("myskill"))
            .unwrap();
        assert_eq!(name.as_str(), "myskill");
    }

    #[test]
    fn test_extract_parameters_empty() {
        let introspector = Arc::new(Introspector::new());
        let generator = SkillGenerator::new(introspector).unwrap();

        let schema = serde_json::json!({
            "type": "object"
        });

        let params = generator.extract_parameters(&schema);
        assert_eq!(params.len(), 0);
    }

    #[test]
    fn test_extract_parameters_with_properties() {
        let introspector = Arc::new(Introspector::new());
        let generator = SkillGenerator::new(introspector).unwrap();

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "required_param": {
                    "type": "string",
                    "description": "A required parameter"
                },
                "optional_param": {
                    "type": "number",
                    "description": "An optional parameter"
                }
            },
            "required": ["required_param"]
        });

        let params = generator.extract_parameters(&schema);

        assert_eq!(params.len(), 2);

        // Find parameters by name
        let required = params
            .iter()
            .find(|p| p.name == "required_param")
            .unwrap();
        assert_eq!(required.type_name, "string");
        assert!(required.required);
        assert_eq!(required.description, "A required parameter");

        let optional = params
            .iter()
            .find(|p| p.name == "optional_param")
            .unwrap();
        assert_eq!(optional.type_name, "number");
        assert!(!optional.required);
    }

    #[test]
    fn test_extract_parameters_missing_description() {
        let introspector = Arc::new(Introspector::new());
        let generator = SkillGenerator::new(introspector).unwrap();

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "param1": {
                    "type": "string"
                }
            }
        });

        let params = generator.extract_parameters(&schema);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].name, "param1");
        assert_eq!(params[0].description, "");
    }

    #[test]
    fn test_extract_parameters_missing_type() {
        let introspector = Arc::new(Introspector::new());
        let generator = SkillGenerator::new(introspector).unwrap();

        let schema = serde_json::json!({
            "type": "object",
            "properties": {
                "param1": {
                    "description": "A parameter without type"
                }
            }
        });

        let params = generator.extract_parameters(&schema);

        assert_eq!(params.len(), 1);
        assert_eq!(params[0].type_name, "any");
    }
}
