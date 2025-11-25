//! Skills code generation module.
//!
//! Generates skill files in the Claude Agent Skills format.
//! Only available when the `skills` feature is enabled.
//!
//! # Format
//!
//! This module generates skills exclusively in the Claude format:
//! - Storage: `.claude/skills/skill-name/`
//! - Main file: `SKILL.md` with YAML frontmatter
//! - Reference: `REFERENCE.md` with detailed API docs
//!
//! # Examples
//!
//! ```no_run
//! use mcp_codegen::skills::converter::SkillConverter;
//! use mcp_codegen::skills::claude::render_skill_md;
//! use mcp_codegen::TemplateEngine;
//! use mcp_introspector::{Introspector, ServerInfo};
//! use mcp_core::{ServerId, ServerConfig, SkillName, SkillDescription};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // 1. Introspect MCP server
//! let mut introspector = Introspector::new();
//! let server_id = ServerId::new("github");
//! let config = ServerConfig::builder().command("github-server".to_string()).build();
//! let server_info = introspector.discover_server(server_id, &config).await?;
//!
//! // 2. Create skill metadata
//! let name = SkillName::new("github")?;
//! let desc = SkillDescription::new("VK Teams bot integration")?;
//!
//! // 3. Convert to SkillData
//! let skill_data = SkillConverter::convert(&server_info, &name, &desc)?;
//!
//! // 4. Render skill file
//! let engine = TemplateEngine::new()?;
//! let skill_md = render_skill_md(&engine, &skill_data)?;
//! # Ok(())
//! # }
//! ```

pub mod claude;
pub mod converter;
pub mod dictionary;
pub mod llm_categorizer;
pub mod manifest_generator;
pub mod orchestrator;
pub mod script_generator;

// Re-export main types for convenient access
pub use dictionary::CategorizationDictionary;
pub use llm_categorizer::LlmCategorizer;
pub use manifest_generator::{FallbackStrategy, GroupingPreference, ManifestGenerator};
pub use orchestrator::SkillOrchestrator;
