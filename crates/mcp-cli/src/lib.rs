//! MCP CLI library.
//!
//! This library provides the core functionality for the MCP CLI tool,
//! exposing modules for commands and formatters that can be tested.

#![allow(clippy::format_push_string)]
#![allow(clippy::unused_async)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::needless_collect)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::unnecessary_literal_unwrap)]

use clap::ValueEnum;

pub mod actions;
pub mod commands;
pub mod formatters;

// Re-export action types for convenience
pub use actions::ServerAction;

/// Output format for code generation.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, ValueEnum)]
pub enum GeneratorFormat {
    /// Progressive loading format (one file per tool, for Claude Code).
    #[default]
    Progressive,
    /// Claude Agent SDK format (with Zod schemas, for SDK integration).
    ClaudeAgent,
}
