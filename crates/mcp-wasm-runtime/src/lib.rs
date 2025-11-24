// TODO(phase-7.3): Review and reduce clippy allows added during Phase 7.1 rapid development.
// Each suppressed lint should either be fixed in code or documented as intentional.
#![allow(clippy::missing_panics_doc)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::cast_precision_loss)]
#![allow(clippy::significant_drop_tightening)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::cast_sign_loss)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::unused_self)]
#![allow(clippy::option_option)]
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::match_like_matches_macro)]
#![allow(clippy::manual_let_else)]
#![allow(clippy::redundant_closure_for_method_calls)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_fields_in_debug)]
#![allow(clippy::single_match_else)]

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

pub mod cache;
pub mod compiler;
pub mod host_functions;
pub mod monitor;
pub mod sandbox;
pub mod security;

pub use cache::{CacheKey, ModuleCache};
pub use compiler::{CompilationBackend, Compiler};
pub use host_functions::HostContext;
pub use monitor::ResourceMonitor;
pub use sandbox::Runtime;
pub use security::{SecurityConfig, SecurityConfigBuilder, SecurityProfile};
