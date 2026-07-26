# mcp-execution-codegen

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-codegen.svg)](https://crates.io/crates/mcp-execution-codegen)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-codegen)](https://docs.rs/mcp-execution-codegen)
[![codecov](https://codecov.io/gh/bug-ops/mcp-execution/graph/badge.svg?token=2UEW36O9AN&flag=mcp-codegen)](https://codecov.io/gh/bug-ops/mcp-execution)
[![MSRV](https://img.shields.io/badge/MSRV-1.91-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

Progressive loading TypeScript code generation for MCP tools. Achieves **98% token savings** by generating one file per tool.

## Installation

```toml
[dependencies]
mcp-execution-codegen = "0.8"
```

Or with cargo-add:

```bash
cargo add mcp-execution-codegen
```

> [!IMPORTANT]
> Requires Rust 1.91 or later.

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

### Generated Files: `tsconfig.json` is a Leaf Configuration

The generator produces `package.json`, `tsconfig.json`, and one `.ts` file per tool. Both JSON files are regenerated on every `generate` call — **manual edits will be lost.**

**Important:** The generated `tsconfig.json` is a **leaf configuration** not intended to be `extends`-ed by other TypeScript configs. If your own `tsconfig.json` extends the generated one, `"noEmit": true` will be inherited, which silently prevents your TypeScript build from emitting output.

**Do not extend the generated `tsconfig.json`.** The generated TypeScript files are a standalone package meant to be executed or type-checked separately from your own build:

- Execute directly via a TS-aware runtime: `tsx`, `deno`, or Node.js's native type-stripping.
- Or type-check independently: run `tsc -p <generated-dir>` as a separate build step.
- If using a bundler (esbuild, swc, Vite, etc.) that doesn't enforce TypeScript's `noEmit` constraint, consult your bundler's documentation for mixing `noEmit` and emitting configurations.

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
  return (await callMCPTool('github', 'create_issue', params)) as CreateIssueResult;
}

export type CreateIssueParams = {
  /** Repository in format "owner/repo" */
  repo: string;
  /** Issue title */
  title: string;
  /** Issue body (optional) */
  body?: string;
};
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

## Security

> [!IMPORTANT]
> All server-controlled strings are sanitized before interpolation into generated TypeScript.

The code generator applies JSDoc sanitization to every field that originates from an MCP server:

- Tool `name` and `description` (truncated to 256 chars, `*/` escaped, CR/LF stripped)
- Server `name` and `version` (same rules, `version` truncated to 64 chars)
- Every `description` field inside `input_schema` JSON recursively
- Categorization fields: `category` (128 chars), `keywords`, `short_description` (256 chars each)
- Non-string `description` values in schemas are replaced with `null`

This prevents malicious MCP servers from injecting arbitrary TypeScript by embedding `*/` in their metadata.

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-execution-core`](../mcp-core) - Foundation types
- [`mcp-execution-introspector`](../mcp-introspector) - MCP server analysis
- [`mcp-execution-files`](../mcp-files) - Virtual filesystem for output

## MSRV Policy

Minimum Supported Rust Version: **1.91**

MSRV increases are considered minor version bumps.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
