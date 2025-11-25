//! Core types, traits, and errors for MCP Code Execution.
//!
//! This crate provides the foundational types and abstractions used across
//! all other crates in the MCP execution workspace.
//!
//! # Architecture
//!
//! The core consists of:
//! - Strong domain types (`ServerId`, `ToolName`)
//! - Error hierarchy with contextual information
//! - Server configuration with security validation
//! - Command validation utilities
//!
//! # Examples
//!
//! ```
//! use mcp_core::{ServerConfig, ServerId};
//!
//! // Create a server configuration
//! let config = ServerConfig::builder()
//!     .command("docker".to_string())
//!     .arg("run".to_string())
//!     .env("LOG_LEVEL".to_string(), "debug".to_string())
//!     .build();
//!
//! // Server ID
//! let server_id = ServerId::new("github").unwrap();
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

mod command;
mod error;
mod server_config;
mod types;

pub mod cli;

// Re-export error types
pub use error::{Error, Result};

// Re-export domain types
pub use types::{ServerId, ToolName};

// Re-export server configuration types
pub use server_config::{ServerConfig, ServerConfigBuilder, TransportType};

// Re-export command validation
pub use command::validate_server_config;
