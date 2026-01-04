# mcp-execution-core

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-core.svg)](https://crates.io/crates/mcp-execution-core)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-core)](https://docs.rs/mcp-execution-core)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

Foundation types, traits, and error handling for MCP Code Execution.

## Installation

```toml
[dependencies]
mcp-execution-core = "0.6"
```

Or with cargo-add:

```bash
cargo add mcp-execution-core
```

> [!IMPORTANT]
> Requires Rust 1.89 or later.

## Usage

### Server Configuration

```rust
use mcp_execution_core::{ServerConfig, ServerId};

let config = ServerConfig::builder()
    .command("docker".to_string())
    .arg("run".to_string())
    .arg("-i".to_string())
    .arg("--rm".to_string())
    .arg("ghcr.io/org/mcp-execution-server".to_string())
    .env("LOG_LEVEL".to_string(), "debug".to_string())
    .build();

let server_id = ServerId::new("github");
```

### Domain Types

```rust
use mcp_execution_core::{ServerId, ToolName};

// Type-safe identifiers prevent mixing up strings
let server = ServerId::new("github");
let tool = ToolName::new("create_issue");

assert_eq!(server.as_str(), "github");
assert_eq!(tool.as_str(), "create_issue");
```

> [!TIP]
> Strong types prevent accidentally passing a `ToolName` where a `ServerId` is expected.

### Error Handling

```rust
use mcp_execution_core::{Error, Result};

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
use mcp_execution_core::{ServerConfig, validate_server_config};

let config = ServerConfig::builder()
    .command("npx".to_string())
    .arg("-y".to_string())
    .arg("@modelcontextprotocol/server-github".to_string())
    .build();

// Validates against command injection
validate_server_config(&config)?;
```

> [!WARNING]
> Always validate server configurations before execution to prevent command injection attacks.

## Features

- **Strong Domain Types**: `ServerId`, `ToolName` instead of raw strings
- **Error Hierarchy**: Contextual errors with `thiserror`
- **Server Configuration**: Type-safe config with security validation
- **Command Validation**: Prevents command injection attacks
- **Thread-Safe**: All types are `Send + Sync`
- **Zero Unsafe**: No `unsafe` code blocks

## Types Reference

| Type | Description |
|------|-------------|
| `ServerId` | Unique server identifier (newtype over String) |
| `ToolName` | MCP tool name (newtype over String) |
| `ServerConfig` | Server configuration with command, args, env |
| `TransportType` | Transport type enum (Stdio, Http, Sse) |
| `Error` | Error type with contextual information |
| `Result<T>` | Alias for `std::result::Result<T, Error>` |

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-execution-introspector`](../mcp-introspector) - MCP server analysis
- [`mcp-execution-codegen`](../mcp-codegen) - TypeScript code generation
- [`mcp-execution-files`](../mcp-files) - Virtual filesystem
- [`mcp-execution-skill`](../mcp-skill) - SKILL.md generation
- [`mcp-execution-server`](../mcp-server) - MCP server implementation
- [`mcp-execution-cli`](../mcp-cli) - Command-line interface

## MSRV Policy

Minimum Supported Rust Version: **1.89**

MSRV increases are considered minor version bumps.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
