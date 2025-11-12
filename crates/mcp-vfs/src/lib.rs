//! Virtual filesystem for MCP tools.
//!
//! Provides a virtual filesystem structure with progressive loading
//! of MCP tool definitions.

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

pub mod vfs;
pub mod builder;
pub mod types;

pub use vfs::VirtualFS;
pub use types::{FileEntry, DirEntry};
