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

pub mod actions;
pub mod commands;
pub mod formatters;

// Re-export action types for convenience
pub use actions::{ConfigAction, ServerAction};
