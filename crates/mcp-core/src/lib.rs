//! Core types, traits, and errors for MCP Code Execution.
//!
//! This crate provides the foundational types and abstractions used across
//! all other crates in the MCP execution workspace.
//!
//! # Architecture
//!
//! The core consists of:
//! - Strong domain types (`ServerId`, `ToolName`, `SessionId`)
//! - Error hierarchy with contextual information
//! - Core traits for execution, bridging, caching, and storage
//! - Configuration types

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

mod error;
mod types;
mod config;

pub mod traits;

pub use error::{Error, Result};
pub use types::{ServerId, ToolName, SessionId, MemoryLimit};
pub use config::{RuntimeConfig, SecurityPolicy};
