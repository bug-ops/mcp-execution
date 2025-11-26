//! Progressive loading code generation module.
//!
//! Generates TypeScript code for progressive loading where each tool
//! is in a separate file. This enables Claude Code to load only the
//! tools it needs, achieving significant token savings.
//!
//! # Architecture
//!
//! Progressive loading differs from the traditional WASM approach:
//!
//! **Traditional (WASM)**:
//! - All tools in one large file
//! - Loaded upfront (all 30 tools at once)
//! - ~30,000 tokens per server
//!
//! **Progressive Loading**:
//! - One file per tool
//! - Loaded on-demand (`ls`, `cat` specific tool)
//! - ~500-1,500 tokens per tool (98% savings)
//!
//! # File Structure
//!
//! For a server with tools `create_issue`, `update_issue`:
//!
//! ```text
//! ~/.claude/servers/github/
//! ├── index.ts                    # Re-exports all tools
//! ├── createIssue.ts              # Individual tool (loaded on-demand)
//! ├── updateIssue.ts              # Individual tool (loaded on-demand)
//! └── _runtime/
//!     └── mcp-bridge.ts           # Runtime helper for MCP calls
//! ```
//!
//! # Examples
//!
//! ## Basic Usage
//!
//! ```no_run
//! use mcp_codegen::progressive::ProgressiveGenerator;
//! use mcp_introspector::{Introspector, ServerInfo};
//! use mcp_core::{ServerId, ServerConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Introspect server
//! let mut introspector = Introspector::new();
//! let server_id = ServerId::new("github");
//! let config = ServerConfig::builder()
//!     .command("/path/to/github-server".to_string())
//!     .build();
//! let info = introspector.discover_server(server_id, &config).await?;
//!
//! // Generate progressive loading files
//! let generator = ProgressiveGenerator::new()?;
//! let code = generator.generate(&info)?;
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
//! This achieves 98% token savings compared to loading all tools upfront.
//!
//! # Feature Flag
//!
//! This module is only available when the `progressive` feature is enabled:
//!
//! ```toml
//! [dependencies]
//! mcp-codegen = { version = "0.1", features = ["progressive"] }
//! ```

pub mod generator;
pub mod types;

// Re-export main types
pub use generator::ProgressiveGenerator;
pub use types::{BridgeContext, IndexContext, PropertyInfo, ToolContext, ToolSummary};
