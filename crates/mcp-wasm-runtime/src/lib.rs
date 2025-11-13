//! WASM execution runtime with security sandbox.
//!
//! Provides secure WASM-based execution environment with memory/CPU limits,
//! isolated filesystem access, and validated host functions.
//!
//! # Architecture
//!
//! The runtime consists of four main components:
//!
//! - **Security**: Configurable security boundaries and limits
//! - **Host Functions**: Controlled interface for WASM to interact with host
//! - **Sandbox**: Wasmtime-based execution environment with resource limits
//! - **Compiler**: TypeScript to WASM compilation with caching
//!
//! # Examples
//!
//! ```no_run
//! use mcp_wasm_runtime::{Runtime, security::SecurityConfig};
//! use mcp_bridge::Bridge;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let bridge = Bridge::new(1000);
//! let config = SecurityConfig::default();
//! let runtime = Runtime::new(Arc::new(bridge), config)?;
//!
//! let wasm_bytes = vec![/* compiled WASM */];
//! let result = runtime.execute(&wasm_bytes, "main", &[]).await?;
//! # Ok(())
//! # }
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

pub mod compiler;
pub mod host_functions;
pub mod sandbox;
pub mod security;

pub use compiler::{CompilationBackend, Compiler};
pub use host_functions::HostContext;
pub use sandbox::Runtime;
pub use security::{SecurityConfig, SecurityConfigBuilder};
