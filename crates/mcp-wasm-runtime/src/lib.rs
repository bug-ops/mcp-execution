//! WASM execution runtime with security sandbox.
//!
//! Provides secure WASM-based execution environment with memory/CPU limits,
//! isolated filesystem access, and validated host functions.

#![warn(missing_docs, missing_debug_implementations)]

pub mod sandbox;
pub mod compiler;
pub mod host_functions;
pub mod security;

pub use sandbox::Runtime;
