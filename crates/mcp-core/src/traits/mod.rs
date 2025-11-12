//! Core traits for MCP Code Execution.
//!
//! This module defines the foundational traits that all components implement,
//! providing a consistent interface for execution, caching, and state management.
//!
//! # Module Structure
//!
//! - `executor` - Code execution trait
//! - `cache` - Cache provider trait
//! - `state` - State storage trait
//!
//! # Examples
//!
//! ```
//! use mcp_core::traits::CodeExecutor;
//! use mcp_core::MemoryLimit;
//! use std::time::Duration;
//!
//! // Implementing a custom executor
//! # use async_trait::async_trait;
//! # use serde_json::Value;
//! # use mcp_core::{Result, Error};
//! #
//! struct MyExecutor {
//!     memory_limit: MemoryLimit,
//!     timeout: Duration,
//! }
//!
//! #[async_trait]
//! impl CodeExecutor for MyExecutor {
//!     async fn execute(&mut self, code: &str) -> Result<Value> {
//!         // Execute code
//!         Ok(Value::String("executed".to_string()))
//!     }
//!
//!     fn set_memory_limit(&mut self, limit: MemoryLimit) {
//!         self.memory_limit = limit;
//!     }
//!
//!     fn set_timeout(&mut self, timeout: Duration) {
//!         self.timeout = timeout;
//!     }
//!
//!     fn memory_limit(&self) -> MemoryLimit {
//!         self.memory_limit
//!     }
//!
//!     fn timeout(&self) -> Duration {
//!         self.timeout
//!     }
//! }
//! ```

mod cache;
mod executor;
mod state;

pub use cache::CacheProvider;
pub use executor::CodeExecutor;
pub use state::StateStorage;
