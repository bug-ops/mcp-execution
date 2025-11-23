//! Core types, traits, and errors for MCP Code Execution.
//!
//! This crate provides the foundational types and abstractions used across
//! all other crates in the MCP execution workspace.
//!
//! # Architecture
//!
//! The core consists of:
//! - Strong domain types (`ServerId`, `ToolName`, `SessionId`, `MemoryLimit`, `CacheKey`)
//! - Error hierarchy with contextual information
//! - Core traits for execution, caching, and state storage
//! - Configuration types with security policies
//!
//! # Examples
//!
//! ```
//! use mcp_core::{RuntimeConfig, SecurityPolicy, MemoryLimit};
//! use std::time::Duration;
//!
//! // Create a runtime configuration
//! let config = RuntimeConfig::builder()
//!     .memory_limit(MemoryLimit::from_mb(512).unwrap())
//!     .execution_timeout(Duration::from_secs(60))
//!     .enable_cache(true)
//!     .security(SecurityPolicy::strict())
//!     .build();
//!
//! // Validate configuration
//! assert!(config.validate().is_ok());
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

mod command;
mod config;
mod error;
mod types;

pub mod cli;
pub mod stats;
pub mod traits;

// Re-export error types
pub use error::{Error, Result};

// Re-export domain types
pub use types::{
    CacheKey, MemoryLimit, ServerId, SessionId, SkillDescription, SkillName, ToolName,
};

// Re-export configuration types
pub use config::{RuntimeConfig, RuntimeConfigBuilder, SecurityPolicy};

// Re-export traits for convenience
pub use traits::{CacheProvider, CodeExecutor, StateStorage};

// Re-export command validation
pub use command::validate_command;
