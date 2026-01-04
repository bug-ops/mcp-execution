# mcp-execution-server

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-server.svg)](https://crates.io/crates/mcp-execution-server)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-server)](https://docs.rs/mcp-execution-server)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

MCP server for generating progressive loading TypeScript files. Achieves **98% token savings** by leveraging Claude's natural language understanding for tool categorization.

## Installation

```bash
# Build from workspace root
cargo build --release -p mcp-execution-server

# Binary: target/release/mcp-execution
```

> [!IMPORTANT]
> Requires Rust 1.89 or later.

## Usage

### Running the Server

```bash
# Direct execution
mcp-execution

# Or via cargo
cargo run -p mcp-execution-server
```

### Claude Code Configuration

Add to `~/.config/claude/mcp.json`:

```json
{
  "mcpServers": {
    "mcp-execution": {
      "command": "mcp-execution"
    }
  }
}
```

> [!TIP]
> The server exposes 5 MCP tools for the complete workflow from introspection to skill generation.

### Programmatic Usage

```rust
use mcp_execution_server::GeneratorService;
use rmcp::ServiceExt;
use rmcp::transport::stdio;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let service = GeneratorService::new().serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
```

## MCP Tools

### `introspect_server`

Connect to an MCP server and discover its tools.

```json
{
  "server_id": "github",
  "command": "npx",
  "args": ["-y", "@anthropic/mcp-execution-server-github"],
  "env": { "GITHUB_TOKEN": "..." }
}
```

### `save_categorized_tools`

Generate TypeScript files using Claude's categorization.

```json
{
  "session_id": "uuid-from-introspect",
  "categorized_tools": [
    {
      "name": "create_issue",
      "category": "issues",
      "keywords": "create,issue,new,bug",
      "short_description": "Create a new issue"
    }
  ]
}
```

### `list_generated_servers`

List all servers with generated progressive loading files.

### `generate_skill`

Analyze generated files and return context for SKILL.md generation.

### `save_skill`

Save generated SKILL.md content to the filesystem.

> [!NOTE]
> Sessions expire automatically after 30 minutes with lazy cleanup.

## Workflow

```text
introspect_server → Claude categorizes → save_categorized_tools → TypeScript files
                                                                        ↓
                                        SKILL.md ← save_skill ← generate_skill
```

## Features

- **5 MCP Tools**: Complete workflow from introspection to skill generation
- **No LLM API Required**: Claude handles categorization in conversation
- **98% Token Savings**: Progressive loading pattern reduces context usage
- **Type-Safe**: Full TypeScript types from MCP JSON schemas
- **Session Management**: Automatic 30-minute session expiry

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-execution-core`](../mcp-core) - Foundation types and traits
- [`mcp-execution-introspector`](../mcp-introspector) - MCP server analysis
- [`mcp-execution-codegen`](../mcp-codegen) - TypeScript code generation
- [`mcp-execution-files`](../mcp-files) - Virtual filesystem
- [`mcp-execution-skill`](../mcp-skill) - SKILL.md generation
- [`mcp-execution-cli`](../mcp-cli) - Command-line interface

## MSRV Policy

Minimum Supported Rust Version: **1.89**

MSRV increases are considered minor version bumps.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
