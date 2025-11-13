//! WASM code generation module.
//!
//! Generates TypeScript code for WebAssembly execution with host functions.
//! Only available when the `wasm` feature is enabled.

pub mod generator;

// Re-export main generator
pub use generator::CodeGenerator;
