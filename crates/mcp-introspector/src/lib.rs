//! MCP server introspection and analysis.
//!
//! Connects to MCP servers, discovers available tools, and extracts
//! schema information for code generation.

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

pub mod discovery;
pub mod analyzer;
pub mod types;

pub use discovery::Introspector;
pub use types::{ServerInfo, ToolInfo};
