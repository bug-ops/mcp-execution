# mcp-introspector

MCP server introspection using the official rmcp SDK.

[![Crates.io](https://img.shields.io/crates/v/mcp-introspector.svg)](https://crates.io/crates/mcp-introspector)
[![Documentation](https://docs.rs/mcp-introspector/badge.svg)](https://docs.rs/mcp-introspector)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

## Overview

`mcp-introspector` discovers MCP server capabilities, tools, and schemas using the official [rmcp](https://docs.rs/rmcp) SDK. It extracts tool definitions for code generation, enabling the progressive loading pattern.

## Features

- **Official SDK**: Uses rmcp v0.8 for MCP communication
- **Tool Discovery**: Extract all tools from any MCP server
- **Schema Extraction**: Get JSON schemas for tool parameters
- **Capability Detection**: Discover tools, resources, prompts support
- **Caching**: Store discovered server information
- **Security**: Command validation prevents injection attacks

## Installation

```toml
[dependencies]
mcp-introspector = "0.6"
```

## Usage

### Basic Introspection

```rust
use mcp_introspector::Introspector;
use mcp_core::{ServerId, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut introspector = Introspector::new();

    // Configure MCP server connection
    let server_id = ServerId::new("github");
    let config = ServerConfig::builder()
        .command("npx".to_string())
        .arg("-y".to_string())
        .arg("@modelcontextprotocol/server-github".to_string())
        .env("GITHUB_TOKEN".to_string(), "ghp_xxx".to_string())
        .build();

    // Discover server capabilities
    let info = introspector.discover_server(server_id, &config).await?;

    println!("Server: {} v{}", info.name, info.version);
    println!("Tools found: {}", info.tools.len());

    for tool in &info.tools {
        println!("  - {}: {}", tool.name, tool.description);
    }

    Ok(())
}
```

### Multiple Servers

```rust
use mcp_introspector::Introspector;
use mcp_core::{ServerId, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut introspector = Introspector::new();

    // Discover GitHub server
    let github_config = ServerConfig::builder()
        .command("npx".to_string())
        .arg("-y".to_string())
        .arg("@modelcontextprotocol/server-github".to_string())
        .build();
    introspector.discover_server(ServerId::new("github"), &github_config).await?;

    // Discover Postgres server
    let postgres_config = ServerConfig::builder()
        .command("npx".to_string())
        .arg("-y".to_string())
        .arg("@modelcontextprotocol/server-postgres".to_string())
        .build();
    introspector.discover_server(ServerId::new("postgres"), &postgres_config).await?;

    // List all discovered servers
    for server in introspector.list_servers() {
        println!("{}: {} tools", server.id, server.tools.len());
    }

    Ok(())
}
```

### Accessing Tool Schemas

```rust
use mcp_introspector::Introspector;
use mcp_core::{ServerId, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("github");
    let config = ServerConfig::builder()
        .command("github-server".to_string())
        .build();

    let info = introspector.discover_server(server_id.clone(), &config).await?;

    // Access tool schemas for code generation
    for tool in &info.tools {
        println!("Tool: {}", tool.name);
        println!("Description: {}", tool.description);
        println!("Input Schema: {}", serde_json::to_string_pretty(&tool.input_schema)?);
    }

    Ok(())
}
```

## Types Reference

| Type | Description |
|------|-------------|
| `Introspector` | Main introspection service with caching |
| `ServerInfo` | Discovered server metadata and tools |
| `ToolInfo` | Tool name, description, and JSON schema |
| `ServerCapabilities` | Flags for tools, resources, prompts support |

## How It Works

1. **Validate Config**: Check server configuration for security issues
2. **Spawn Process**: Start MCP server via stdio transport
3. **Connect**: Establish rmcp client connection
4. **Query**: Use `ServiceExt::list_all_tools()` to get tools
5. **Extract**: Parse tool definitions and schemas
6. **Cache**: Store information for later retrieval

## Transport Support

Currently supports **stdio** transport (most common for MCP servers):

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
    .arg("ghcr.io/org/mcp-server".to_string())
    .build();
```

## Related Crates

- [`mcp-core`](../mcp-core) - Foundation types (`ServerId`, `ServerConfig`)
- [`mcp-codegen`](../mcp-codegen) - Uses introspection results for code generation
- [`rmcp`](https://docs.rs/rmcp) - Official Rust MCP SDK

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE.md))
- MIT license ([LICENSE-MIT](../../LICENSE.md))

at your option.
