//! MCP CLI library.
//!
//! This library provides the core functionality for the MCP CLI tool,
//! exposing the CLI argument definitions, command implementations, output
//! formatters, and command runner, so the binary crate and tests can share
//! a single compiled module tree.
#![warn(missing_docs, missing_debug_implementations)]
// `commands::completions::run` and `commands::server::list_servers` are `async fn` to match
// the signature every other command handler needs (they're all dispatched uniformly from
// `runner::execute_command`), even though these two happen to do no `.await`ing themselves.
#![allow(clippy::unused_async)]
// `commands::generate` tests collect into a `Vec` before an `is_empty()`/`len()` check for
// readability at the assertion site; the iterator itself isn't reused afterward.
#![allow(clippy::needless_collect)]

pub mod actions;
pub mod cli;
pub mod commands;
pub mod formatters;
pub mod runner;

// Re-export action types for convenience
pub use actions::ServerAction;
