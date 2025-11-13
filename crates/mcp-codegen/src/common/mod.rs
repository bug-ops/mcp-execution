//! Common code shared between all feature modes.
//!
//! This module contains types and utilities used by both WASM and Skills
//! generation modes. Code here has no feature gates.

pub mod types;
pub mod typescript;

// Re-export common types
pub use types::{GeneratedCode, GeneratedFile, TemplateContext, ToolDefinition};
