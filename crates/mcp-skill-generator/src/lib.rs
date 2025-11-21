// Temporary allow for Phase 7.1 - will be cleaned up in follow-up
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::cargo_common_metadata)]
#![allow(clippy::multiple_crate_versions)]

//! MCP Skill Generator - Generate Claude Code skills from MCP servers.
//!
//! This crate provides functionality to automatically generate Claude Code
//! skills (SKILL.md files) from MCP server definitions. It uses the
//! `mcp-introspector` to discover server capabilities and generates
//! type-safe, validated skills using Handlebars templates.
//!
//! # Overview
//!
//! The skill generator enables zero-configuration integration of MCP servers
//! into Claude Code by automatically creating skill definitions with proper
//! documentation and metadata.
//!
//! # Key Features
//!
//! - **Type-Safe**: Strong types (`SkillName`) with compile-time validation
//! - **Template-Based**: Flexible Handlebars templates for customization
//! - **Validated**: Runtime validation ensures Claude Code compliance
//! - **Documented**: Comprehensive documentation with examples
//!
//! # Architecture
//!
//! This crate follows Microsoft Rust Guidelines:
//! - Strong types over primitives (ADR-003)
//! - `thiserror` for error handling
//! - All public types are `Send + Sync + Debug`
//! - Builder pattern for complex configuration
//! - Accept `impl AsRef<T>` for flexible APIs
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```no_run
//! use mcp_skill_generator::{SkillName, SkillContext, create_skill_template_engine, render_skill};
//! use mcp_core::ServerId;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Create template engine
//! let engine = create_skill_template_engine()?;
//!
//! // Create skill context
//! let context = SkillContext {
//!     name: "vkteams-bot".to_string(),
//!     description: "Interact with VK Teams messenger".to_string(),
//!     server_id: ServerId::new("vkteams-bot-server"),
//!     tool_count: 3,
//!     tools: vec![],
//!     generator_version: env!("CARGO_PKG_VERSION").to_string(),
//!     generated_at: chrono::Utc::now().to_rfc3339(),
//! };
//!
//! // Generate skill
//! let skill_md = render_skill(&engine, &context)?;
//! println!("{}", skill_md);
//! # Ok(())
//! # }
//! ```
//!
//! ## Using Builder Pattern
//!
//! ```
//! use mcp_skill_generator::{SkillGenerationOptions, TemplateType};
//!
//! let options = SkillGenerationOptions::builder()
//!     .template_type(TemplateType::Verbose)
//!     .include_examples(true)
//!     .custom_prompt("Always be polite")
//!     .build();
//!
//! assert_eq!(options.template_type, TemplateType::Verbose);
//! ```
//!
//! ## Validating Skill Names
//!
//! ```
//! use mcp_skill_generator::SkillName;
//!
//! // Valid names
//! let name = SkillName::new("vkteams-bot").unwrap();
//! assert_eq!(name.as_str(), "vkteams-bot");
//!
//! // Invalid names fail validation
//! assert!(SkillName::new("123invalid").is_err());
//! assert!(SkillName::new("Invalid-Name").is_err());
//! ```
//!
//! # Skill Naming Rules
//!
//! Skill names must follow Claude Code requirements:
//! - 1-64 characters
//! - Only lowercase letters, numbers, hyphens, underscores
//! - Start with a letter
//! - End with a letter or number
//!
//! # Error Handling
//!
//! All operations return `Result<T, Error>` where `Error` provides
//! `is_xxx()` methods for error classification:
//!
//! ```
//! use mcp_skill_generator::{Error, SkillName};
//!
//! match SkillName::new("invalid-") {
//!     Ok(_) => unreachable!(),
//!     Err(e) => {
//!         assert!(e.is_validation_error());
//!         println!("Validation failed: {}", e);
//!     }
//! }
//! ```
//!
//! # Thread Safety
//!
//! All public types implement `Send + Sync + Debug` for use with
//! Tokio and multi-threaded environments.

#![warn(missing_docs)]

pub mod skill_template;
pub mod types;

// Re-export key types for convenience
pub use skill_template::{create_skill_template_engine, render_skill};
pub use types::{
    Error, GeneratedSkill, ParameterContext, Result, SkillContext, SkillGenerationOptions,
    SkillGenerationOptionsBuilder, SkillMetadata, SkillName, TemplateType, ToolContext,
    sanitize_string, validate_no_template_syntax,
};

#[cfg(test)]
mod tests;
