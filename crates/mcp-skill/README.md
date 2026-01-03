# mcp-skill

> **Skill generation for MCP progressive loading**

[![Crate](https://img.shields.io/crates/v/mcp-skill.svg)](https://crates.io/crates/mcp-skill)
[![Docs](https://docs.rs/mcp-skill/badge.svg)](https://docs.rs/mcp-skill)

## Overview

The `mcp-skill` crate provides functionality to generate Claude Code skill files (SKILL.md) from generated progressive loading TypeScript files.

A skill file teaches Claude Code agents how to use MCP server tools efficiently through progressive loading - discovering tools via `ls`, loading them via `cat`, and executing via `node`.

## Features

- **JSDoc Parsing** - Extract metadata from TypeScript tool files
- **Context Building** - Structure tool information for skill generation
- **Template Rendering** - Generate skill prompts using Handlebars
- **Directory Scanning** - Process all tools in a server directory

## Architecture

The skill generation flow:

1. **Parser** (`parser.rs`) - Extracts JSDoc metadata from TypeScript files
2. **Context Builder** (`context.rs`) - Structures parsed tools into categories
3. **Template Renderer** (`template.rs`) - Renders Handlebars prompt template

## Usage

```rust
use mcp_skill::{scan_tools_directory, build_skill_context};
use std::path::Path;

// Scan TypeScript tool files
let tools = scan_tools_directory(Path::new("~/.claude/servers/github")).await?;

// Build skill generation context
let context = build_skill_context("github", &tools, None);

// Use context.generation_prompt with LLM to generate SKILL.md
println!("Generated prompt: {}", context.generation_prompt);
```

## Types

### Core Types

- **`ParsedToolFile`** - Metadata extracted from TypeScript file
- **`ParsedParameter`** - TypeScript parameter information
- **`SkillCategory`** - Tools grouped by category
- **`SkillTool`** - Tool metadata for skill generation

### Error Types

- **`ParseError`** - Errors during TypeScript file parsing
- **`ScanError`** - Errors during directory scanning
- **`TemplateError`** - Errors during template rendering

## Examples

### Parse a Single Tool File

```rust
use mcp_skill::parse_tool_file;

let content = r#"
/**
 * @tool create_issue
 * @server github
 * @category issues
 * @keywords create,issue,new
 * @description Create a new issue
 */
"#;

let parsed = parse_tool_file(content, "createIssue.ts")?;
assert_eq!(parsed.name, "create_issue");
```

### Scan Directory

```rust
use mcp_skill::scan_tools_directory;
use std::path::Path;

let tools = scan_tools_directory(Path::new("/path/to/server")).await?;
println!("Found {} tools", tools.len());
```

### Build Context

```rust
use mcp_skill::build_skill_context;

let context = build_skill_context("github", &tools, None);
println!("Skill name: {}", context.skill_name);
println!("Tool count: {}", context.tool_count);
```

## Security

- **File Size Limits** - Max 1MB per file (DoS protection)
- **File Count Limits** - Max 500 files per directory
- **Path Validation** - Only scans `.ts` files, excludes `_runtime/`

## Performance

- **Pre-compiled Regexes** - Using `LazyLock` for efficiency
- **Streaming Parsing** - No full AST parsing overhead
- **Async I/O** - Non-blocking file operations via `tokio::fs`

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE) or <http://www.apache.org/licenses/LICENSE-2.0>)
- MIT license ([LICENSE-MIT](../../LICENSE-MIT) or <http://opensource.org/licenses/MIT>)

at your option.
