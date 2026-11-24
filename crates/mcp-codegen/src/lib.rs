//! TypeScript SDK generation for MCP tools.
//!
//! This crate turns an MCP server's tool definitions into a typed TypeScript
//! SDK: one file per tool, a type-only catalog and a runtime bridge. A model
//! programs against the SDK, so loops, joins and large results stay in code
//! and only the answer reaches its context.
//!
//! # Architecture
//!
//! 1. **Discovery**: a model lists or reads `catalog.ts` in `~/.claude/servers/{server-id}/`
//! 2. **Selection**: it reads only the tool files it needs
//! 3. **Execution**: generated TypeScript calls MCP tools through the runtime bridge, either
//!    as a single call (`node tool.ts '{"param":"value"}'`) or from a script
//!
//! # Example
//!
//! ```no_run
//! use mcp_execution_codegen::progressive::ProgressiveGenerator;
//! use mcp_execution_introspector::ServerInfo;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let generator = ProgressiveGenerator::new()?;
//! // generator.generate(&server_info, &server_config)?;
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
//! ├── catalog.ts            # One type-only signature per tool
//! ├── createIssue.ts        # Individual tool file
//! ├── updateIssue.ts        # Individual tool file
//! └── _runtime/
//!     ├── mcp-bridge.ts     # Runtime helper
//!     └── cli.ts            # Single-call entry point
//! ```

#![warn(missing_docs, missing_debug_implementations)]

// Core modules (always available)
pub mod common;
pub mod progressive;
pub mod template_engine;

// Re-export main types
pub use common::types::{GeneratedCode, GeneratedFile, TemplateContext, ToolDefinition};
pub use progressive::{ProgressiveGenerator, TSCONFIG_JSON};
pub use template_engine::TemplateEngine;
