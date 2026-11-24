//! Command implementations for the MCP CLI.
//!
//! This module contains all subcommand implementations, organized by functionality.
//! Each command module is responsible for parsing its arguments, executing the
//! operation, and formatting output according to the requested format.

pub mod call;
pub mod common;
pub mod completions;
pub mod drift;
pub mod generate;
pub mod helper;
pub mod introspect;
pub mod run;
pub mod scripts;
pub mod server;
pub mod setup;
pub mod skill;
pub mod stats;
