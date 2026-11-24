//! Typed TypeScript SDK generation module.
//!
//! Generates one TypeScript file per tool, a type-only catalog and a runtime
//! bridge, so a model can read only the signatures it needs and script
//! against the server.
//!
//! # Architecture
//!
//! - One file per tool, with parameter and result types
//! - `catalog.ts` lists one type-only signature per tool
//! - Files are read on demand (`ls`, `cat` a specific tool)
//! - Importing a module when a script runs puts nothing into a model's context; only text the
//!   model reads does
//!
//! # File Structure
//!
//! For a server with tools `create_issue`, `update_issue`:
//!
//! ```text
//! ~/.claude/servers/github/
//! ├── index.ts                    # Re-exports all tools
//! ├── catalog.ts                  # One type-only signature per tool
//! ├── createIssue.ts              # Individual tool (loaded on-demand)
//! ├── updateIssue.ts              # Individual tool (loaded on-demand)
//! └── _runtime/
//!     ├── mcp-bridge.ts           # Runtime helper for MCP calls
//!     └── cli.ts                  # Single-call entry point shared by every tool file
//! ```
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```no_run
//! use mcp_execution_codegen::progressive::ProgressiveGenerator;
//! use mcp_execution_introspector::{Introspector, ServerInfo};
//! use mcp_execution_core::{ServerId, ServerConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Introspect server
//! let introspector = Introspector::new();
//! let server_id = ServerId::new("github").unwrap();
//! let config = ServerConfig::builder()
//!     .command("/path/to/github-server".to_string())
//!     .build()?;
//! let info = introspector.discover_server(server_id, &config).await?;
//!
//! // Generate progressive loading files
//! let generator = ProgressiveGenerator::new()?;
//! let code = generator.generate(&info, &config)?;
//!
//! // Files are generated, ready to write to disk
//! for file in &code.files {
//!     println!("Generated: {}", file.path);
//!     // Write to ~/.claude/servers/github/
//! }
//! # Ok(())
//! # }
//! ```
//!
//! ## Progressive Loading Pattern
//!
//! Once files are written to disk, Claude Code can discover and load tools progressively:
//!
//! ```bash
//! # Step 1: Discover available servers
//! $ ls ~/.claude/servers/
//! github/  google-drive/
//!
//! # Step 2: Discover available tools in github server
//! $ ls ~/.claude/servers/github/
//! index.ts  createIssue.ts  updateIssue.ts  getIssue.ts
//!
//! # Step 3: Load ONLY the tool you need
//! $ cat ~/.claude/servers/github/createIssue.ts
//! // TypeScript code for createIssue tool
//! ```
//!
//! # Feature Flag
//!
//! This module is only available when the `progressive` feature is enabled:
//!
//! ```toml
//! [dependencies]
//! mcp-codegen = { version = "0.1", features = ["progressive"] }
//! ```

mod annotations;
mod catalog;
pub mod generator;
mod observed;
mod tool_types;
pub mod types;

// Re-export main types
pub use generator::ProgressiveGenerator;
pub use types::{
    BridgeContext, CategoryInfo, IndexContext, ResultSource, ToolCategorization, ToolContext,
    ToolSummary,
};
