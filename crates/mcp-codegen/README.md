# mcp-execution-codegen

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-codegen.svg)](https://crates.io/crates/mcp-execution-codegen)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-codegen)](https://docs.rs/mcp-execution-codegen)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

Progressive loading TypeScript code generation for MCP tools. Achieves **98% token savings** by generating one file per tool.

## Installation

```toml
[dependencies]
mcp-execution-codegen = "0.6"
```

Or with cargo-add:

```bash
cargo add mcp-execution-codegen
```

> [!IMPORTANT]
> Requires Rust 1.89 or later.

## Usage

### Progressive Loading Generation

```rust
use mcp_execution_codegen::progressive::ProgressiveGenerator;
use mcp_execution_introspector::Introspector;
use mcp_execution_core::{ServerId, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Introspect MCP server
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("github");
    let config = ServerConfig::builder()
        .command("github-mcp-execution-server".to_string())
        .build();
    let info = introspector.discover_server(server_id, &config).await?;

    // 2. Generate progressive loading files
    let generator = ProgressiveGenerator::new()?;
    let code = generator.generate(&info)?;

    println!("Generated {} files", code.file_count());
    Ok(())
}
```

> [!TIP]
> Generated files include: one `.ts` file per tool, `index.ts` re-exports, and `_runtime/mcp-bridge.ts` helper.

### Token Savings

| Approach | Tokens | Savings |
|----------|--------|---------|
| Traditional (all tools) | ~30,000 | - |
| Progressive (1 tool) | ~500-1,500 | **98%** |

## Generated TypeScript Structure

Each tool file includes full TypeScript interfaces:

```typescript
/**
 * Creates a new issue in a GitHub repository
 * @param params - Tool parameters
 * @returns Tool execution result
 */
export async function createIssue(
  params: CreateIssueParams
): Promise<CreateIssueResult> {
  return await callMCPTool('github', 'create_issue', params);
}

export interface CreateIssueParams {
  /** Repository in format "owner/repo" */
  repo: string;
  /** Issue title */
  title: string;
  /** Issue body (optional) */
  body?: string;
}
```

## Features

- **One File Per Tool**: Separate TypeScript file for each MCP tool
- **Type-Safe Interfaces**: Full TypeScript parameter and result types
- **JSDoc Documentation**: Complete documentation from MCP schemas
- **98% Token Savings**: Load only the tools you need
- **Handlebars Templates**: Customizable code generation

## Type Conversion

JSON Schema types are converted to TypeScript:

| JSON Schema | TypeScript |
|-------------|------------|
| `string` | `string` |
| `number` | `number` |
| `boolean` | `boolean` |
| `array` | `T[]` |
| `object` | `{ [key: string]: T }` |

> [!NOTE]
> Optional parameters use `?` suffix in TypeScript interfaces.

## Performance

| Metric | Target | Achieved |
|--------|--------|----------|
| 10 tools | <100ms | **0.19ms** (526x faster) |
| 50 tools | <20ms | **0.97ms** (20.6x faster) |
| VFS export | <10ms | **1.2ms** (8.3x faster) |

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-execution-core`](../mcp-execution-core) - Foundation types
- [`mcp-execution-introspector`](../mcp-execution-introspector) - MCP server analysis
- [`mcp-execution-files`](../mcp-execution-files) - Virtual filesystem for output

## MSRV Policy

Minimum Supported Rust Version: **1.89**

MSRV increases are considered minor version bumps.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
