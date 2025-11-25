// TODO(phase-7.3): Review and reduce clippy allows added during Phase 7.1 rapid development.
// Each suppressed lint should either be fixed in code or documented as intentional.
#![allow(clippy::uninlined_format_args)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::option_if_let_else)]
#![allow(clippy::similar_names)]
#![allow(clippy::missing_const_for_fn)]
#![allow(clippy::unused_self)]
#![allow(clippy::unnecessary_wraps)]
#![allow(clippy::needless_lifetimes)]
#![allow(clippy::elidable_lifetime_names)]

//! Code generation for MCP tools.
//!
//! Transforms MCP tool schemas into executable TypeScript or Rust code
//! using Handlebars templates.
//!
//! # Features
//!
//! This crate supports multiple code generation targets via feature flags:
//!
//! - **`wasm`** (default): Generate TypeScript for WebAssembly execution
//! - **`skills`**: Generate executable scripts for Claude Code Skills
//! - **`progressive`**: Generate progressive loading files (one file per tool)
//! - **`all`**: Enable all generation modes
//!
//! # Examples
//!
//! ## WASM Code Generation (default)
//!
//! ```toml
//! [dependencies]
//! mcp-codegen = "0.1"  # wasm feature enabled by default
//! ```
//!
//! ```no_run
//! use mcp_codegen::CodeGenerator;
//! use mcp_introspector::ServerInfo;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let generator = CodeGenerator::new()?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Skills Code Generation
//!
//! ```toml
//! [dependencies]
//! mcp-codegen = { version = "0.1", features = ["skills"], default-features = false }
//! ```
//!
//! ## Progressive Loading Code Generation
//!
//! Progressive loading generates one file per tool, enabling Claude Code to load only what it needs:
//!
//! ```toml
//! [dependencies]
//! mcp-codegen = { version = "0.1", features = ["progressive"], default-features = false }
//! ```
//!
//! ```no_run
//! use mcp_codegen::progressive::ProgressiveGenerator;
//! use mcp_introspector::ServerInfo;
//!
//! # fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let generator = ProgressiveGenerator::new()?;
//! # Ok(())
//! # }
//! ```
//!
//! This achieves 98% token savings compared to loading all tools upfront.

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

// Common module (no feature gates)
pub mod common;
pub mod template_engine;

// WASM module (feature-gated)
#[cfg(feature = "wasm")]
pub mod wasm;

// Skills module (feature-gated)
#[cfg(feature = "skills")]
pub mod skills;

// Progressive module (feature-gated)
#[cfg(feature = "progressive")]
pub mod progressive;

// Re-export common types (always available)
pub use common::types::{GeneratedCode, GeneratedFile, TemplateContext, ToolDefinition};
pub use template_engine::TemplateEngine;

// Re-export WASM-specific types
#[cfg(feature = "wasm")]
pub use wasm::CodeGenerator;

// Re-export Progressive-specific types
#[cfg(feature = "progressive")]
pub use progressive::ProgressiveGenerator;

// Feature check: at least one feature must be enabled
#[cfg(not(any(feature = "wasm", feature = "skills", feature = "progressive")))]
compile_error!("At least one feature must be enabled: 'wasm', 'skills', or 'progressive'");
