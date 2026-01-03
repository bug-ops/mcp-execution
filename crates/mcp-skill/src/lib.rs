//! Skill generation for MCP progressive loading.
//!
//! This crate provides functionality to generate Claude Code skill files (SKILL.md)
//! from generated progressive loading TypeScript files.
//!
//! # Architecture
//!
//! The skill generation flow:
//! 1. `parser` - Extracts `JSDoc` metadata from TypeScript files
//! 2. `context` - Builds structured context from parsed tools
//! 3. `template` - Renders Handlebars template with context
//!
//! # Examples
//!
//! ```no_run
//! use mcp_skill::{scan_tools_directory, build_skill_context};
//! use std::path::Path;
//!
//! # async fn example() -> anyhow::Result<()> {
//! let tools = scan_tools_directory(Path::new("~/.claude/servers/github")).await?;
//! let context = build_skill_context("github", &tools, None);
//! # Ok(())
//! # }
//! ```

mod context;
mod parser;
mod template;
pub mod types;

pub use context::build_skill_context;
pub use parser::{
    MAX_FILE_SIZE, MAX_TOOL_FILES, ParseError, ParsedParameter, ParsedToolFile, ScanError,
    extract_skill_metadata, parse_tool_file, scan_tools_directory,
};
pub use template::{TemplateError, render_generation_prompt};
pub use types::{
    GenerateSkillParams, GenerateSkillResult, SaveSkillParams, SaveSkillResult, SkillCategory,
    SkillMetadata, SkillTool, ToolExample, validate_server_id,
};
