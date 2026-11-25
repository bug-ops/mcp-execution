//! Common code shared by the generators.
//!
//! This module contains the generator output types and the JSON Schema to TypeScript
//! converter.
//! Code here has no feature gates.

pub mod ts_types;
pub mod types;
pub(crate) mod typescript;

// Re-export common types
pub use types::{GeneratedCode, GeneratedFile};
