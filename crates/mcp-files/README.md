# mcp-files

Virtual filesystem for MCP tools organization and export.

[![Crates.io](https://img.shields.io/crates/v/mcp-files.svg)](https://crates.io/crates/mcp-files)
[![Documentation](https://docs.rs/mcp-files/badge.svg)](https://docs.rs/mcp-files)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

## Overview

`mcp-files` provides an in-memory virtual filesystem for storing and organizing generated MCP tool definitions. Files are organized hierarchically and can be exported to disk for use by AI agents.

## Features

- **In-Memory Storage**: Fast access without disk I/O during generation
- **Builder Pattern**: Fluent API for VFS construction
- **Strong Types**: Type-safe paths and error handling
- **Disk Export**: Write VFS contents to filesystem
- **Thread-Safe**: All types are `Send + Sync`
- **Integration**: Works seamlessly with `mcp-codegen` output

## Installation

```toml
[dependencies]
mcp-files = "0.6"
```

## Usage

### Basic Usage

```rust
use mcp_files::{FileSystem, FilesBuilder};

// Create filesystem using builder
let fs = FilesBuilder::new()
    .add_file("/mcp-tools/manifest.json", "{\"version\": \"1.0\"}")
    .add_file("/mcp-tools/createIssue.ts", "export function createIssue() {}")
    .add_file("/mcp-tools/updateIssue.ts", "export function updateIssue() {}")
    .build()
    .unwrap();

// Read files
let content = fs.read_file("/mcp-tools/manifest.json").unwrap();
assert_eq!(content, "{\"version\": \"1.0\"}");

// Check existence
assert!(fs.exists("/mcp-tools/createIssue.ts"));
assert!(!fs.exists("/missing.ts"));
```

### Directory Operations

```rust
use mcp_files::FilesBuilder;

let fs = FilesBuilder::new()
    .add_file("/servers/github/createIssue.ts", "// code")
    .add_file("/servers/github/updateIssue.ts", "// code")
    .add_file("/servers/github/getIssue.ts", "// code")
    .build()
    .unwrap();

// List directory contents
let files = fs.list_dir("/servers/github").unwrap();
assert_eq!(files.len(), 3);
```

### Integration with Code Generation

```rust
use mcp_files::FilesBuilder;
use mcp_codegen::{GeneratedCode, GeneratedFile};

// Code generation produces GeneratedCode
let mut code = GeneratedCode::new();
code.add_file(GeneratedFile {
    path: "createIssue.ts".to_string(),
    content: "export function createIssue() {}".to_string(),
});
code.add_file(GeneratedFile {
    path: "index.ts".to_string(),
    content: "export * from './createIssue';".to_string(),
});

// Convert to VFS under specific path
let vfs = FilesBuilder::from_generated_code(code, "/servers/github")
    .build()
    .unwrap();

assert!(vfs.exists("/servers/github/createIssue.ts"));
assert!(vfs.exists("/servers/github/index.ts"));
```

### Export to Disk

```rust
use mcp_files::{FilesBuilder, ExportOptions};
use std::path::Path;

let fs = FilesBuilder::new()
    .add_file("/github/createIssue.ts", "// code")
    .build()
    .unwrap();

// Export to filesystem
let options = ExportOptions::default();
fs.export_to_disk(Path::new("~/.claude/servers"), &options)?;

// Files written to ~/.claude/servers/github/createIssue.ts
```

## Types Reference

| Type | Description |
|------|-------------|
| `FileSystem` | In-memory virtual filesystem |
| `FilesBuilder` | Builder for constructing FileSystem |
| `FilePath` | Validated file path (newtype) |
| `FileEntry` | File entry with path and content |
| `ExportOptions` | Options for disk export |
| `FilesError` | Error type for file operations |

## Performance

From benchmarks (M1 MacBook Pro):

| Operation | Target | Achieved |
|-----------|--------|----------|
| VFS export | <10ms | **1.2ms** |
| Memory (1000 files) | <256MB | **~2MB** |

## Architecture

```
mcp-files/
├── filesystem.rs  # FileSystem implementation
├── builder.rs     # FilesBuilder with fluent API
├── types.rs       # FilePath, FileEntry, FilesError
└── lib.rs         # Public API re-exports
```

## Related Crates

- [`mcp-core`](../mcp-core) - Foundation types used by this crate
- [`mcp-codegen`](../mcp-codegen) - Generates code that this crate organizes

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE.md))
- MIT license ([LICENSE-MIT](../../LICENSE.md))

at your option.
