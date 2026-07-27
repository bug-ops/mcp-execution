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
//! use mcp_execution_core::{ServerConfig, ServerId};
//!
//! // Create a server configuration
//! let config = ServerConfig::builder()
//!     .command("docker".to_string())
//!     .arg("run".to_string())
//!     .env("LOG_LEVEL".to_string(), "debug".to_string())
//!     .build()
//!     .unwrap();
//!
//! // Server ID
//! let server_id = ServerId::new("github").unwrap();
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

mod command;
mod error;
mod path;
mod redact;
mod server_config;
mod types;

pub mod cli;
pub mod metadata;
pub mod untrusted;

// Re-export error types
pub use error::{Error, ResourceKind, Result};

// Re-export domain types
pub use types::{ServerId, ServerIdError, ToolName, ToolNameError};

// Re-export server configuration types
pub use server_config::{ServerConfig, ServerConfigBuilder, Transport};

// Re-export command validation
pub use command::{
    MAX_ARG_COUNT, MAX_ARG_LEN, MAX_ENV_COUNT, MAX_ENV_VALUE_LEN, MAX_HEADER_COUNT,
    MAX_HEADER_VALUE_LEN, MAX_URL_LEN, forbidden_chars, forbidden_env_names, forbidden_env_prefix,
    validate_server_config, validate_url_scheme,
};

// Re-export path helpers shared by confinement checks
pub use path::{contains_parent_dir, sanitize_path_for_error, validate_path_segment};

// Re-export Debug-redaction helpers shared by secret-shaped fields
pub use redact::{REDACTED_PLACEHOLDER, RedactedItems, RedactedMapValues, RedactedUrl};
