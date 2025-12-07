//! Skill generation module for creating SKILL.md files.
//!
//! This module provides functionality to:
//! - Parse TypeScript tool files for metadata
//! - Build context for skill generation
//! - Render prompt templates for LLM generation
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
//! use mcp_server::skill::{scan_tools_directory, build_skill_context};
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

pub use context::build_skill_context;
pub use parser::{
    ParseError, ParsedParameter, ParsedToolFile, ScanError, parse_tool_file, scan_tools_directory,
};
pub use template::{TemplateError, render_generation_prompt};
