//! MCP server library for progressive loading generation.
//!
//! This crate provides an MCP server that helps generate progressive loading
//! TypeScript files for other MCP servers. It leverages Claude's natural
//! language understanding for tool categorization - no separate LLM API needed.
//!
//! # Architecture
//!
//! The server implements three main tools:
//!
//! 1. **`introspect_server`** - Connect to a target MCP server and discover its tools
//! 2. **`save_categorized_tools`** - Generate TypeScript files with Claude's categorization
//! 3. **`list_generated_servers`** - List all servers with generated files
//!
//! # Workflow
//!
//! 1. User asks Claude to generate progressive loading for an MCP server
//! 2. Claude calls `introspect_server` to discover tools
//! 3. Claude analyzes tool metadata and assigns categories, keywords, descriptions
//! 4. Claude calls `save_categorized_tools` with categorization
//! 5. Server generates TypeScript files with discovery headers
//!
//! # Examples
//!
//! ```no_run
//! use mcp_server::service::GeneratorService;
//! use rmcp::transport::stdio;
//! use rmcp::ServiceExt;
//!
//! # async fn example() -> anyhow::Result<()> {
//! // Create and run the service
//! let service = GeneratorService::new().serve(stdio()).await?;
//! service.waiting().await?;
//! # Ok(())
//! # }
//! ```
//!
//! # State Management
//!
//! The server maintains temporary session state between `introspect_server` and
//! `save_categorized_tools` calls. Sessions expire after 30 minutes and are
//! cleaned up lazily.
//!
//! # Key Benefits
//!
//! - **No LLM API**: Claude (the conversation LLM) does categorization
//! - **Human-in-the-loop**: User can review and adjust categories
//! - **Progressive loading**: 98% token savings (30,000 → 500-1,500 tokens)
//! - **Type-safe**: Full TypeScript types from MCP schemas
//! - **Discoverable**: grep-friendly headers for tool discovery

pub mod service;
pub mod state;
pub mod types;

pub use service::GeneratorService;
pub use state::StateManager;
pub use types::{
    CategorizedTool, GeneratedServerInfo, IntrospectServerParams, IntrospectServerResult,
    ListGeneratedServersParams, ListGeneratedServersResult, PendingGeneration,
    SaveCategorizedToolsParams, SaveCategorizedToolsResult, ToolGenerationError, ToolMetadata,
};

// Re-export skill types from mcp-skill crate
pub use mcp_skill::{
    GenerateSkillParams, GenerateSkillResult, SaveSkillParams, SaveSkillResult, SkillCategory,
    SkillMetadata, SkillTool, ToolExample,
};
