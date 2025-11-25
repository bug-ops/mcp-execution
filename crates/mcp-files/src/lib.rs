//! Virtual filesystem for MCP tools.
//!
//! Provides an in-memory, read-only virtual filesystem for storing and
//! accessing generated MCP tool definitions. Files are organized in a
//! hierarchical structure like `/mcp-tools/servers/<server-id>/...`.
//!
//! # Features
//!
//! - **In-memory storage**: All files stored in memory for fast access
//! - **Strong types**: Type-safe paths and error handling
//! - **Builder pattern**: Fluent API for VFS construction
//! - **Integration**: Works seamlessly with `mcp-codegen` output
//! - **Thread-safe**: All types are `Send + Sync`
//!
//! # Examples
//!
//! ## Basic usage
//!
//! ```
//! use mcp_files::{Vfs, FilesBuilder};
//!
//! // Create VFS using builder
//! let vfs = FilesBuilder::new()
//!     .add_file("/mcp-tools/manifest.json", "{}")
//!     .add_file("/mcp-tools/types.ts", "export type Params = {};")
//!     .build()
//!     .unwrap();
//!
//! // Read files
//! let content = vfs.read_file("/mcp-tools/manifest.json").unwrap();
//! assert_eq!(content, "{}");
//!
//! // Check existence
//! assert!(vfs.exists("/mcp-tools/types.ts"));
//! assert!(!vfs.exists("/missing.ts"));
//! ```
//!
//! ## Integration with code generation
//!
//! ```
//! use mcp_files::FilesBuilder;
//! use mcp_codegen::{GeneratedCode, GeneratedFile};
//!
//! let mut code = GeneratedCode::new();
//! code.add_file(GeneratedFile {
//!     path: "manifest.json".to_string(),
//!     content: r#"{"version": "1.0"}"#.to_string(),
//! });
//! code.add_file(GeneratedFile {
//!     path: "tools/sendMessage.ts".to_string(),
//!     content: "export function sendMessage() {}".to_string(),
//! });
//!
//! let vfs = FilesBuilder::from_generated_code(code, "/mcp-tools/servers/github")
//!     .build()
//!     .unwrap();
//!
//! assert!(vfs.exists("/mcp-tools/servers/github/manifest.json"));
//! assert!(vfs.exists("/mcp-tools/servers/github/tools/sendMessage.ts"));
//! ```
//!
//! ## Directory operations
//!
//! ```
//! use mcp_files::FilesBuilder;
//!
//! let vfs = FilesBuilder::new()
//!     .add_file("/mcp-tools/servers/test/file1.ts", "")
//!     .add_file("/mcp-tools/servers/test/file2.ts", "")
//!     .build()
//!     .unwrap();
//!
//! let files = vfs.list_dir("/mcp-tools/servers/test").unwrap();
//! assert_eq!(files.len(), 2);
//! ```

#![deny(unsafe_code)]
#![warn(missing_docs, missing_debug_implementations)]

pub mod builder;
pub mod filesystem;
pub mod types;
pub mod vfs;

// Re-export main types
pub use builder::FilesBuilder;
pub use filesystem::ExportOptions;
pub use types::{FileEntry, FilePath, FilesError, Result};
pub use vfs::FileSystem;
