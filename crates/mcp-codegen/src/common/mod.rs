//! Common code shared by the generators.
//!
//! This module contains types and utilities shared by the generators: template
//! context types, the JSON Schema to TypeScript converter and TypeScript helpers.
//! Code here has no feature gates.

pub mod ts_types;
pub mod types;
pub mod typescript;

// Re-export common types
pub use types::{GeneratedCode, GeneratedFile, TemplateContext, ToolDefinition};
