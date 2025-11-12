//! MCP bridge for proxying WASM calls to real MCP servers.
//!
//! Provides connection pooling, caching, rate limiting, and security
//! validation for MCP tool calls.

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

pub mod bridge;
pub mod connection;
pub mod cache;
pub mod security;

pub use bridge::Bridge;
pub use cache::CacheProvider;
