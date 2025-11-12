//! Code generation for MCP tools.
//!
//! Transforms MCP tool schemas into executable TypeScript or Rust code
//! using Handlebars templates.

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

pub mod generator;
pub mod typescript;
pub mod template_engine;
pub mod types;

pub use generator::CodeGenerator;
pub use types::GeneratedCode;
