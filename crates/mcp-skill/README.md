# mcp-execution-skill

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-skill.svg)](https://crates.io/crates/mcp-execution-skill)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-skill)](https://docs.rs/mcp-execution-skill)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

Skill generation for MCP progressive loading. Generates Claude Code skill files (SKILL.md) from TypeScript tool files.

## Installation

```bash
cargo add mcp-execution-skill
```

Or add to your `Cargo.toml`:

```toml
[dependencies]
mcp-execution-skill = "0.6"
```

> [!IMPORTANT]
> Requires Rust 1.89 or later.

## Usage

```rust
use mcp_execution_skill::{scan_tools_directory, build_skill_context};
use std::path::Path;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Scan TypeScript tool files from generated server directory
    let tools = scan_tools_directory(Path::new("~/.claude/servers/github")).await?;

    // Build skill generation context
    let context = build_skill_context("github", &tools, None);

    // Use context.generation_prompt with LLM to generate SKILL.md
    println!("Skill: {}", context.skill_name);
    println!("Tools: {}", context.tool_count);
    println!("Prompt:\n{}", context.generation_prompt);

    Ok(())
}
```

> [!TIP]
> For optimal results, use the MCP server (`mcp-execution-server`) for skill generation. It leverages LLM capabilities to summarize tool descriptions, resulting in more concise skill files.

## Features

- **JSDoc Parsing** - Extract metadata (`@tool`, `@server`, `@category`, `@keywords`) from TypeScript files
- **Context Building** - Structure tool information into categories for skill generation
- **Template Rendering** - Generate prompts using pre-compiled Handlebars templates
- **Async Directory Scanning** - Non-blocking file operations via `tokio::fs`

## Architecture

```
TypeScript Files → Parser → Context Builder → Template Renderer → SKILL.md Prompt
```

1. **Parser** (`parser.rs`) - Extracts JSDoc metadata using pre-compiled regexes
2. **Context Builder** (`context.rs`) - Groups tools by category, generates examples
3. **Template Renderer** (`template.rs`) - Renders Handlebars prompt template

## Examples

### Parse a Single Tool File

```rust
use mcp_execution_skill::parse_tool_file;

let content = r#"
/**
 * @tool create_issue
 * @server github
 * @category issues
 * @keywords create,issue,new
 * @description Create a new GitHub issue
 */
"#;

let parsed = parse_tool_file(content, "createIssue.ts")?;
assert_eq!(parsed.name, "create_issue");
assert_eq!(parsed.category, Some("issues".to_string()));
```

### Build Context with Hints

```rust
use mcp_execution_skill::build_skill_context;

// Add use-case hints for better context
let hints = vec!["managing pull requests", "code review"];
let context = build_skill_context("github", &tools, Some(&hints));

println!("Categories: {:?}", context.categories.len());
```

## Types

| Type | Description |
|------|-------------|
| `ParsedToolFile` | Metadata extracted from TypeScript file |
| `ParsedParameter` | TypeScript parameter information |
| `SkillCategory` | Tools grouped by category |
| `SkillTool` | Tool metadata for skill generation |
| `GenerateSkillResult` | Complete context for SKILL.md generation |

## Error Handling

```rust
use mcp_execution_skill::{ParseError, ScanError};

// ParseError - JSDoc parsing failures
// ScanError - Directory scanning issues (permissions, limits)
```

## Security

> [!NOTE]
> Built-in DoS protection for untrusted input.

- **File Size Limit** - Max 1MB per file
- **File Count Limit** - Max 500 files per directory
- **Path Validation** - Only scans `.ts` files, excludes `_runtime/`

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-execution-core`](../mcp-core) - Foundation types and traits
- [`mcp-execution-codegen`](../mcp-codegen) - TypeScript code generation
- [`mcp-execution-files`](../mcp-files) - Virtual filesystem
- [`mcp-execution-cli`](../mcp-cli) - CLI with `skill` command

## MSRV Policy

Minimum Supported Rust Version: **1.89**

MSRV increases are considered minor version bumps.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
