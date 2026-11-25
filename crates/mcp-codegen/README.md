# mcp-execution-codegen

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-codegen.svg)](https://crates.io/crates/mcp-execution-codegen)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-codegen)](https://docs.rs/mcp-execution-codegen)
[![codecov](https://codecov.io/gh/bug-ops/mcp-execution/graph/badge.svg?token=2UEW36O9AN&flag=mcp-codegen)](https://codecov.io/gh/bug-ops/mcp-execution)
[![MSRV](https://img.shields.io/badge/MSRV-1.98-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

TypeScript SDK generation for MCP tools: one typed file per tool, a type-only catalog and a runtime bridge, so a model can script against a server.

## Installation

```bash
cargo add mcp-execution-codegen
```

> [!IMPORTANT]
> Requires Rust 1.98 or later.

## Usage

### SDK Generation

```rust
use mcp_execution_codegen::package::PackageGenerator;
use mcp_execution_introspector::Introspector;
use mcp_execution_core::{ServerId, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Introspect MCP server
    let introspector = Introspector::new();
    let server_id = ServerId::new("github");
    let config = ServerConfig::builder()
        .command("github-mcp-server".to_string())
        .build();
    let info = introspector.discover_server(server_id, &config).await?;

    // 2. Generate the SDK files
    let generator = PackageGenerator::new()?;
    let code = generator.generate(&info, &config)?;

    println!("Generated {} files", code.file_count());
    Ok(())
}
```

> [!TIP]
> Generated files include: one `.ts` file per tool, `index.ts` re-exports, `catalog.ts` (one type-only signature per tool), and the `_runtime/mcp-bridge.ts` and `_runtime/cli.ts` helpers.

### Generated Files: `tsconfig.json` is a Leaf Configuration

The generator produces `package.json`, `tsconfig.json`, and one `.ts` file per tool. Both JSON files are regenerated on every `generate` call — **manual edits will be lost.**

**Important:** The generated `tsconfig.json` is a **leaf configuration** not intended to be `extends`-ed by other TypeScript configs. If your own `tsconfig.json` extends the generated one, `"noEmit": true` will be inherited, which silently prevents your TypeScript build from emitting output.

**Do not extend the generated `tsconfig.json`.** The generated TypeScript files are a standalone package meant to be executed or type-checked separately from your own build:

- Execute directly via a TS-aware runtime: `tsx`, `deno`, or Node.js's native type-stripping.
- Or type-check independently: run `tsc -p <generated-dir>` as a separate build step.
- If using a bundler (esbuild, swc, Vite, etc.) that doesn't enforce TypeScript's `noEmit` constraint, consult your bundler's documentation for mixing `noEmit` and emitting configurations.

## Generated TypeScript Structure

Each tool file includes full TypeScript types:

```typescript
/**
 * @tool create_issue
 * @server github
 * @description Creates a new issue in a GitHub repository
 */
export async function createIssue(
  params: CreateIssueParams
): Promise<CreateIssueResult> {
  return (await callMCPTool('github', 'create_issue', params, 'content')) as CreateIssueResult;
}

export type CreateIssueParams = {
  /** Repository in format "owner/repo" */
  repo: string;
  /** Issue title */
  title: string;
  /** Issue body (optional) */
  body?: string;
};

export type CreateIssueResult = Record<string, unknown> | unknown[] | string | undefined;
```

When a server declares an output schema, `CreateIssueResult` is its converted type followed by
`| undefined` (an empty success), and the function asks the bridge for the result's
`structuredContent`.

## Runtime Bridge

`_runtime/mcp-bridge.ts` is a thin client of the `mcp-execution-cli` helper, not an MCP client. It frames each `callMCPTool(serverId, toolName, params, resultSource?)` call as a line of JSON on a channel and maps the response, so connections, validation, result extraction and limits live in one Rust implementation. The channel is the one `mcp-execution-cli run` registers for a script or, for a tool file run directly with `node`, a hidden `mcp-execution-cli helper` process (binary: `$MCP_EXECUTION_CLI`, else `mcp-execution-cli` on `PATH`; a set but unusable value is an error with no fallback).

The bridge throws only `McpToolError` (`kind`: `tool_error`, `rpc_error`, `call_failed`, `write_skipped`); server text is in `serverMessage`. `closeAllConnections`, the SIGINT and SIGTERM handlers and `MCPBRIDGE_REQUEST_TIMEOUT_MS` are gone. `MCPBRIDGE_DEBUG=1` prints diagnostics to stderr and `MCPBRIDGE_OBSERVE_SHAPES=<file>` records result shapes for `generate --observed-shapes`. Generated output from an earlier release must be regenerated.

## Features

- **One File Per Tool**: Separate TypeScript file for each MCP tool
- **Type-Safe Types**: Full TypeScript parameter and result types, with unions, intersections and local references
- **JSDoc Documentation**: Complete documentation from MCP schemas
- **Compact Catalog**: One type-only signature per tool in `catalog.ts`
- **Handlebars Templates**: Customizable code generation

## Type Conversion

JSON Schema is converted by one converter, `common::ts_types::convert_schema`, for parameter
types, result types and the catalog:

| JSON Schema | TypeScript |
|-------------|------------|
| `string`, `number`, `integer`, `boolean`, `null` | `string`, `number`, `number`, `boolean`, `null` |
| `enum`, `const` | union of literals, in declared order without duplicates |
| `type: [..]`, `oneOf`, `anyOf` | union |
| `allOf` | intersection (up to 8 members) |
| `array` | `T[]` (`unknown[]` for tuples) |
| `object` | members, plus an index signature unless `additionalProperties` is `false` |
| local `$ref` | resolved against the same document, cycles cut with `unknown` |

Property keys are never renamed: a key that is not a plain identifier renders as a quoted
literal with its raw value (`'per-page'?: number`). Anything outside the supported subset, and
every bound hit, renders `unknown` for the smallest enclosing node and records a diagnostic.

> [!NOTE]
> Optional parameters use `?` suffix in TypeScript types.

## Performance

Generation timings come from the Criterion benchmark; reproduce them with `cargo bench --profile bench-fast -p mcp-execution-codegen`.

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

Minimum Supported Rust Version: **1.98**

MSRV = stable minus two, raised in a minor release.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.
