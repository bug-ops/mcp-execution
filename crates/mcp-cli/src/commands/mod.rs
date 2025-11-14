//! Command implementations for the MCP CLI.
//!
//! This module contains all subcommand implementations, organized by functionality.
//! Each command module is responsible for parsing its arguments, executing the
//! operation, and formatting output according to the requested format.

pub mod config;
pub mod debug;
pub mod execute;
pub mod generate;
pub mod introspect;
pub mod server;
pub mod stats;
