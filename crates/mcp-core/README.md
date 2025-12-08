# mcp-core

Foundation types, traits, and error handling for MCP Code Execution.

[![Crates.io](https://img.shields.io/crates/v/mcp-core.svg)](https://crates.io/crates/mcp-core)
[![Documentation](https://docs.rs/mcp-core/badge.svg)](https://docs.rs/mcp-core)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

## Overview

`mcp-core` provides foundational types and abstractions used across all crates in the mcp-execution workspace. Following [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/), it emphasizes strong typing, safety, and clear error handling.

## Features

- **Strong Domain Types**: `ServerId`, `ToolName` instead of raw strings
- **Error Hierarchy**: Contextual errors with `thiserror`
- **Server Configuration**: Type-safe config with security validation
- **Command Validation**: Prevents command injection attacks
- **Thread-Safe**: All types are `Send + Sync`
- **Zero Unsafe**: No `unsafe` code blocks

## Installation

```toml
[dependencies]
mcp-core = "0.6"
```

## Usage

### Server Configuration

```rust
use mcp_core::{ServerConfig, ServerId};

// Create a server configuration with builder pattern
let config = ServerConfig::builder()
    .command("docker".to_string())
    .arg("run".to_string())
    .arg("-i".to_string())
    .arg("--rm".to_string())
    .arg("ghcr.io/org/mcp-server".to_string())
    .env("LOG_LEVEL".to_string(), "debug".to_string())
    .env("API_KEY".to_string(), "secret".to_string())
    .build();

// Strong-typed server identifier
let server_id = ServerId::new("github");
println!("Server: {}", server_id);
```

### Domain Types

```rust
use mcp_core::{ServerId, ToolName};

// Type-safe identifiers prevent mixing up strings
let server = ServerId::new("github");
let tool = ToolName::new("create_issue");

// Compare and use as HashMap keys
assert_eq!(server.as_str(), "github");
assert_eq!(tool.as_str(), "create_issue");
```

### Error Handling

```rust
use mcp_core::{Error, Result};

fn process_server(id: &str) -> Result<()> {
    if id.is_empty() {
        return Err(Error::InvalidConfiguration {
            message: "Server ID cannot be empty".to_string(),
        });
    }
    Ok(())
}
```

### Command Validation

```rust
use mcp_core::{ServerConfig, validate_server_config};

let config = ServerConfig::builder()
    .command("npx".to_string())
    .arg("-y".to_string())
    .arg("@modelcontextprotocol/server-github".to_string())
    .build();

// Validates against command injection and other security issues
validate_server_config(&config)?;
```

## Types Reference

| Type | Description |
|------|-------------|
| `ServerId` | Unique server identifier (newtype over String) |
| `ToolName` | MCP tool name (newtype over String) |
| `ServerConfig` | Server configuration with command, args, env |
| `TransportType` | Transport type enum (Stdio, Http, Sse) |
| `Error` | Error type with contextual information |
| `Result<T>` | Alias for `std::result::Result<T, Error>` |

## Architecture

```
mcp-core/
├── types.rs         # ServerId, ToolName domain types
├── server_config.rs # ServerConfig, ServerConfigBuilder
├── command.rs       # Command validation utilities
├── error.rs         # Error enum with thiserror
├── cli.rs           # CLI-related utilities
└── lib.rs           # Public API re-exports
```

## Related Crates

This crate is the foundation for:

- [`mcp-introspector`](../mcp-introspector) - MCP server analysis
- [`mcp-codegen`](../mcp-codegen) - TypeScript code generation
- [`mcp-files`](../mcp-files) - Virtual filesystem
- [`mcp-server`](../mcp-server) - MCP server implementation
- [`mcp-cli`](../mcp-cli) - Command-line interface

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE.md))
- MIT license ([LICENSE-MIT](../../LICENSE.md))

at your option.
