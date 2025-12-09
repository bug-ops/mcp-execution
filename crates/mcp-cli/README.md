# mcp-execution-cli

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-cli.svg)](https://crates.io/crates/mcp-execution-cli)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-cli)](https://docs.rs/mcp-execution-cli)
[![MSRV](https://img.shields.io/badge/MSRV-1.89-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

Command-line interface for MCP Code Execution progressive loading. Achieves **98% token savings** by generating one TypeScript file per MCP tool.

## Installation

### Pre-built binaries

Download from [GitHub Releases](https://github.com/bug-ops/mcp-execution/releases/latest):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/bug-ops/mcp-execution/releases/latest/download/mcp-execution-cli-macos-arm64.tar.gz | tar xz

# macOS (Intel)
curl -L https://github.com/bug-ops/mcp-execution/releases/latest/download/mcp-execution-cli-macos-amd64.tar.gz | tar xz

# Linux (x86_64)
curl -L https://github.com/bug-ops/mcp-execution/releases/latest/download/mcp-execution-cli-linux-amd64.tar.gz | tar xz
```

### From crates.io

```bash
cargo install mcp-execution-cli
```

### From source

```bash
git clone https://github.com/bug-ops/mcp-execution
cd mcp-execution
cargo install --path crates/mcp-cli
```

> [!IMPORTANT]
> Requires Rust 1.89 or later for building from source.

## Usage

### Generate Progressive Loading Files

```bash
# From config file (recommended)
mcp-execution-cli generate --from-config github

# With manual configuration
mcp-execution-cli generate github-mcp-server --env GITHUB_TOKEN=ghp_xxx
```

> [!TIP]
> Use `--from-config` to load server configuration from `~/.config/claude/mcp.json`.

### Discover Available Tools

```bash
# List available servers
ls ~/.claude/servers/

# List tools in a server
ls ~/.claude/servers/github/

# Load one tool (98% token savings!)
cat ~/.claude/servers/github/createIssue.ts
```

### Shell Completions

```bash
# Bash
mcp-execution-cli completions bash > /etc/bash_completion.d/mcp-execution-cli

# Zsh
mcp-execution-cli completions zsh > ~/.zsh/completions/_mcp-execution-cli

# Fish
mcp-execution-cli completions fish > ~/.config/fish/completions/mcp-execution-cli.fish
```

## Commands

### `generate`

Generate TypeScript files with progressive loading:

```bash
mcp-execution-cli generate <SERVER> [OPTIONS]
```

**Options**:

- `--from-config <NAME>`: Load config from mcp.json
- `--arg <ARG>`: Server command argument (repeatable)
- `--env <KEY=VALUE>`: Environment variable (repeatable)
- `--progressive-output <PATH>`: Custom output directory
- `--format <FORMAT>`: Output format (json, text, pretty)

**Examples**:

```bash
# From config
mcp-execution-cli generate --from-config github

# Docker container
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/org/server \
  --env=API_KEY=xxx
```

### `introspect`

Analyze MCP servers and discover capabilities:

```bash
mcp-execution-cli introspect --from-config github --output json
```

### `stats`

View cache statistics:

```bash
mcp-execution-cli stats
```

### `completions`

Generate shell completions:

```bash
mcp-execution-cli completions <SHELL>
```

> [!NOTE]
> Supported shells: bash, zsh, fish, powershell

## Features

- **Progressive Loading**: One TypeScript file per MCP tool
- **Type-Safe**: Full TypeScript interfaces from MCP schemas
- **Multiple Transports**: stdio, HTTP, SSE, Docker
- **Shell Completions**: bash, zsh, fish, PowerShell
- **Fast**: ~2-3ms generation time per server

## Token Savings

| Approach | Tokens | Savings |
|----------|--------|---------|
| Traditional (all tools) | ~30,000 | - |
| Progressive (1 tool) | ~500-1,500 | **98%** |

## Security

- **No Code Execution**: Generated TypeScript is for type information only
- **Command Injection Prevention**: All user input validated
- **Path Validation**: Rejects malicious paths
- **Template Security**: Handlebars escapes all user input

> [!WARNING]
> Never pass untrusted input directly to `--arg` or `--env` options.

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-core`](../mcp-core) - Foundation types
- [`mcp-introspector`](../mcp-introspector) - MCP server analysis
- [`mcp-codegen`](../mcp-codegen) - TypeScript code generation
- [`mcp-files`](../mcp-files) - Virtual filesystem
- [`mcp-server`](../mcp-server) - MCP server

## MSRV Policy

Minimum Supported Rust Version: **1.89**

MSRV increases are considered minor version bumps.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
