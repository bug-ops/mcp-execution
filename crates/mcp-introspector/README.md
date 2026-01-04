# mcp-execution-introspector

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-introspector.svg)](https://crates.io/crates/mcp-execution-introspector)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-introspector)](https://docs.rs/mcp-execution-introspector)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

MCP server introspection using the official [rmcp](https://docs.rs/rmcp) SDK.

## Installation

```toml
[dependencies]
mcp-execution-introspector = "0.6"
```

Or with cargo-add:

```bash
cargo add mcp-execution-introspector
```

> [!IMPORTANT]
> Requires Rust 1.89 or later.

## Usage

### Basic Introspection

```rust
use mcp_execution_introspector::Introspector;
use mcp_execution_core::{ServerId, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut introspector = Introspector::new();

    let server_id = ServerId::new("github");
    let config = ServerConfig::builder()
        .command("npx".to_string())
        .arg("-y".to_string())
        .arg("@modelcontextprotocol/server-github".to_string())
        .env("GITHUB_TOKEN".to_string(), "ghp_xxx".to_string())
        .build();

    let info = introspector.discover_server(server_id, &config).await?;

    println!("Server: {} v{}", info.name, info.version);
    println!("Tools found: {}", info.tools.len());

    for tool in &info.tools {
        println!("  - {}: {}", tool.name, tool.description);
    }

    Ok(())
}
```

### Accessing Tool Schemas

```rust
let info = introspector.discover_server(server_id, &config).await?;

for tool in &info.tools {
    println!("Tool: {}", tool.name);
    println!("Schema: {}", serde_json::to_string_pretty(&tool.input_schema)?);
}
```

> [!TIP]
> Tool schemas are JSON Schema objects that can be used for TypeScript type generation.

### Transport Support

```rust
// npx-based servers
let config = ServerConfig::builder()
    .command("npx".to_string())
    .arg("-y".to_string())
    .arg("@modelcontextprotocol/server-github".to_string())
    .build();

// Docker-based servers
let config = ServerConfig::builder()
    .command("docker".to_string())
    .arg("run".to_string())
    .arg("-i".to_string())
    .arg("--rm".to_string())
    .arg("ghcr.io/org/mcp-execution-server".to_string())
    .build();
```

> [!NOTE]
> Currently supports **stdio** transport, which is the most common for MCP servers.

## Features

- **Official SDK**: Uses [rmcp](https://docs.rs/rmcp) for MCP communication
- **Tool Discovery**: Extract all tools from any MCP server
- **Schema Extraction**: Get JSON schemas for tool parameters
- **Capability Detection**: Discover tools, resources, prompts support
- **Caching**: Store discovered server information
- **Security**: Command validation prevents injection attacks

## Types Reference

| Type | Description |
|------|-------------|
| `Introspector` | Main introspection service with caching |
| `ServerInfo` | Discovered server metadata and tools |
| `ToolInfo` | Tool name, description, and JSON schema |
| `ServerCapabilities` | Flags for tools, resources, prompts support |

## How It Works

1. **Validate Config** — Check server configuration for security issues
2. **Spawn Process** — Start MCP server via stdio transport
3. **Connect** — Establish rmcp client connection
4. **Query** — Use `ServiceExt::list_all_tools()` to get tools
5. **Extract** — Parse tool definitions and schemas
6. **Cache** — Store information for later retrieval

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-execution-core`](../mcp-core) - Foundation types (`ServerId`, `ServerConfig`)
- [`mcp-execution-codegen`](../mcp-codegen) - Uses introspection results for code generation
- [`rmcp`](https://docs.rs/rmcp) - Official Rust MCP SDK

## MSRV Policy

Minimum Supported Rust Version: **1.89**

MSRV increases are considered minor version bumps.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
