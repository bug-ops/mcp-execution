# MCP Code Execution

> **Autonomous MCP Tool Execution with 98% Token Savings**
>
> Transform any MCP server into executable, type-safe TypeScript tools using progressive loading pattern. Load only what you need, when you need it.

[![CI](https://github.com/bug-ops/mcp-execution/actions/workflows/ci.yml/badge.svg)](https://github.com/bug-ops/mcp-execution/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/bug-ops/mcp-execution/branch/master/graph/badge.svg)](https://codecov.io/gh/bug-ops/mcp-execution)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://blog.rust-lang.org/2025/01/23/Rust-1.89.0.html)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE)

## The Problem

Traditional MCP integration loads ALL tools from a server (~30,000 tokens), even when you only need one or two. This wastes context window space and slows down AI agents.

## The Solution

Progressive loading generates one TypeScript file per tool (~500-1,500 tokens each). AI agents discover and load only what they need via simple `ls` and `cat` commands.

**Result**: **98% token savings** + **autonomous execution** + **type safety**

> Inspired by [Anthropic's engineering blog post](https://www.anthropic.com/engineering/code-execution-with-mcp) on Code Execution with MCP.

## Quick Start

### Installation

```bash
# Pre-built binaries
curl -L https://github.com/bug-ops/mcp-execution/releases/latest/download/mcp-execution-cli-macos-arm64.tar.gz | tar xz

# From source
cargo install --path crates/mcp-cli
```

### Generate TypeScript Tools

```bash
# 1. Configure MCP server in ~/.config/claude/mcp.json
# 2. Generate tools
mcp-execution-cli generate --from-config github

# Output: ~/.claude/servers/github/
#   - createIssue.ts
#   - updateIssue.ts
#   - ... (one file per tool)
```

### Use Progressive Loading

```bash
# List available tools
ls ~/.claude/servers/github/

# Load only what you need (98% token savings!)
cat ~/.claude/servers/github/createIssue.ts

# Execute autonomously
node ~/.claude/servers/github/createIssue.ts --repo="owner/repo" --title="Bug"
```

## Key Features

| Feature | Description |
|---------|-------------|
| **98% Token Savings** | Load 1 tool (~500 tokens) vs all tools (~30,000 tokens) |
| **Autonomous Execution** | Generated files run directly via Node.js CLI |
| **Type-Safe** | Full TypeScript interfaces from MCP JSON schemas |
| **Lightning Fast** | 526x faster than target (0.19ms for 10 tools) |
| **100% MCP Compatible** | Works with all MCP servers via [rmcp SDK](https://docs.rs/rmcp) |

## Workspace Crates

| Crate | Description |
|-------|-------------|
| [mcp-core](crates/mcp-core) | Foundation types, traits, and error handling |
| [mcp-introspector](crates/mcp-introspector) | MCP server analysis using rmcp SDK |
| [mcp-codegen](crates/mcp-codegen) | TypeScript code generation with progressive loading |
| [mcp-files](crates/mcp-files) | Virtual filesystem for code organization |
| [mcp-server](crates/mcp-server) | MCP server for progressive loading generation |
| [mcp-cli](crates/mcp-cli) | Command-line interface |

**Dependency Graph** (no circular dependencies):
```
mcp-cli → {mcp-codegen, mcp-introspector, mcp-files, mcp-core}
mcp-server → {mcp-codegen, mcp-introspector, mcp-files, mcp-core}
mcp-codegen → {mcp-files, mcp-core}
mcp-introspector → {rmcp, mcp-core}
mcp-files → mcp-core
```

## CLI Commands

See [mcp-cli README](crates/mcp-cli) for full reference.

```bash
# Generate TypeScript tools
mcp-execution-cli generate --from-config github

# Introspect MCP server
mcp-execution-cli introspect --from-config github

# View cache statistics
mcp-execution-cli stats

# Shell completions
mcp-execution-cli completions bash
```

## Performance

| Metric | Target | Achieved |
|--------|--------|----------|
| 10 tools generation | <100ms | **0.19ms** (526x faster) |
| 50 tools generation | <20ms | **0.97ms** (20.6x faster) |
| VFS export | <10ms | **1.2ms** (8.3x faster) |
| Memory (1000 tools) | <256MB | **~2MB** |

## Documentation

- [Progressive Loading Tutorial](examples/progressive-loading-usage.md) - Complete guide
- [Claude Code Integration](examples/SKILL.md) - Skill setup
- [Architecture Decisions](docs/adr/) - ADRs explaining design choices
- [API Documentation](https://docs.rs/mcp-execution) - Rust API docs

## Development

```bash
# Build
cargo build --workspace

# Test
cargo nextest run --workspace

# Lint
cargo clippy --workspace -- -D warnings

# Format
cargo +nightly fmt --workspace
```

All development follows [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/).

## Security

- **No code execution during generation**: Generated TypeScript is static
- **Input validation**: All parameters validated before use
- **Path safety**: Validated to prevent directory traversal
- **Template security**: Handlebars escapes all user input

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## License

Licensed under either of:

- **Apache License, Version 2.0** ([LICENSE-APACHE](LICENSE))
- **MIT license** ([LICENSE-MIT](LICENSE))

at your option.

## Resources

- [MCP Specification](https://spec.modelcontextprotocol.io/) - Protocol specification
- [rmcp Documentation](https://docs.rs/rmcp) - Official Rust MCP SDK
- [Code Execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp) - Original inspiration
