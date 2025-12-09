# mcp-files

[![Crates.io](https://img.shields.io/crates/v/mcp-files.svg)](https://crates.io/crates/mcp-files)
[![docs.rs](https://img.shields.io/docsrs/mcp-files)](https://docs.rs/mcp-files)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

In-memory virtual filesystem for MCP tools organization and export.

## Installation

```toml
[dependencies]
mcp-files = "0.6"
```

Or with cargo-add:

```bash
cargo add mcp-files
```

> [!IMPORTANT]
> Requires Rust 1.89 or later.

## Usage

### Basic Usage

```rust
use mcp_files::{FileSystem, FilesBuilder};

let fs = FilesBuilder::new()
    .add_file("/mcp-tools/manifest.json", "{\"version\": \"1.0\"}")
    .add_file("/mcp-tools/createIssue.ts", "export function createIssue() {}")
    .build()
    .unwrap();

// Read files
let content = fs.read_file("/mcp-tools/manifest.json").unwrap();

// Check existence
assert!(fs.exists("/mcp-tools/createIssue.ts"));
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

let files = fs.list_dir("/servers/github").unwrap();
assert_eq!(files.len(), 3);
```

### Integration with Code Generation

```rust
use mcp_files::FilesBuilder;
use mcp_codegen::{GeneratedCode, GeneratedFile};

let mut code = GeneratedCode::new();
code.add_file(GeneratedFile {
    path: "createIssue.ts".to_string(),
    content: "export function createIssue() {}".to_string(),
});

let vfs = FilesBuilder::from_generated_code(code, "/servers/github")
    .build()
    .unwrap();

assert!(vfs.exists("/servers/github/createIssue.ts"));
```

> [!TIP]
> Use `from_generated_code` to seamlessly integrate with `mcp-codegen` output.

### Export to Disk

```rust
use mcp_files::{FilesBuilder, ExportOptions};
use std::path::Path;

let fs = FilesBuilder::new()
    .add_file("/github/createIssue.ts", "// code")
    .build()
    .unwrap();

let options = ExportOptions::default();
fs.export_to_disk(Path::new("~/.claude/servers"), &options)?;
```

> [!NOTE]
> Export validates paths to prevent directory traversal attacks.

## Features

- **In-Memory Storage**: Fast access without disk I/O during generation
- **Builder Pattern**: Fluent API for VFS construction
- **Strong Types**: Type-safe paths and error handling
- **Disk Export**: Write VFS contents to filesystem
- **Thread-Safe**: All types are `Send + Sync`

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

| Operation | Target | Achieved |
|-----------|--------|----------|
| VFS export | <10ms | **1.2ms** (8.3x faster) |
| Memory (1000 files) | <256MB | **~2MB** |

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-core`](../mcp-core) - Foundation types used by this crate
- [`mcp-codegen`](../mcp-codegen) - Generates code that this crate organizes

## MSRV Policy

Minimum Supported Rust Version: **1.89**

MSRV increases are considered minor version bumps.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
