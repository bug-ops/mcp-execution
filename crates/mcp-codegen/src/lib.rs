//! Progressive loading code generation for MCP tools.
//!
//! This crate generates TypeScript files for progressive loading pattern,
//! where each MCP tool is a separate file. This enables Claude Code to
//! discover and load tools on-demand, achieving 98% token savings.
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::elidable_lifetime_names)]
#![allow(clippy::unused_self)]
#![allow(clippy::unnecessary_wraps)]
//!
//! # Architecture
//!
//! The progressive loading pattern works as follows:
//!
//! 1. **Tool Discovery**: Claude Code lists files in `~/.claude/servers/{server-id}/`
//! 2. **Selective Loading**: Claude Code reads only the tools it needs
//! 3. **Execution**: Generated TypeScript code calls MCP tools via bridge
//!
//! # Example
//!
//! ```no_run
//! use mcp_codegen::progressive::ProgressiveGenerator;
//! use mcp_introspector::ServerInfo;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let generator = ProgressiveGenerator::new()?;
//! // generator.generate(&server_info)?;
//! # Ok(())
//! # }
//! ```
//!
//! # Generated Structure
//!
//! For a server with 3 tools, generates:
//! ```text
//! ~/.claude/servers/github/
//! ├── index.ts              # Re-exports all tools
//! ├── createIssue.ts       # Individual tool file
//! ├── updateIssue.ts       # Individual tool file
//! └── _runtime/
//!     └── mcp-bridge.ts    # Runtime helper
//! ```
//!
//! # Token Savings
//!
//! - **Traditional**: Load all 30 tools upfront = 30,000 tokens
//! - **Progressive**: Load on-demand = ~2,000 tokens per tool
//! - **Savings**: 93-98%

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

// Core modules (always available)
pub mod common;
pub mod progressive;
pub mod template_engine;

// Re-export main types
pub use common::types::{GeneratedCode, GeneratedFile, TemplateContext, ToolDefinition};
pub use progressive::ProgressiveGenerator;
pub use template_engine::TemplateEngine;
