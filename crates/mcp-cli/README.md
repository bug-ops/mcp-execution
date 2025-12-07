# mcp-execution-cli

Command-line interface for MCP Code Execution progressive loading.

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-cli.svg)](https://crates.io/crates/mcp-execution-cli)
[![Documentation](https://docs.rs/mcp-execution-cli/badge.svg)](https://docs.rs/mcp-execution-cli)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

## Overview

`mcp-execution-cli` provides a command-line interface for generating TypeScript files from MCP servers using progressive loading pattern, achieving 98% token savings.

## Features

- **Progressive Loading**: Generate one TypeScript file per MCP tool
- **Type-Safe**: Full TypeScript interfaces from MCP schemas
- **Multiple Transports**: stdio, HTTP, SSE, Docker
- **Multiple Output Formats**: JSON, text, pretty-printed
- **Shell Completions**: bash, zsh, fish, PowerShell
- **Fast**: ~2-3ms generation time per server

## Installation

### From crates.io

```bash
cargo install mcp-execution-cli
```

### From Source

```bash
git clone https://github.com/bug-ops/mcp-execution
cd mcp-execution
cargo install --path crates/mcp-execution-cli
```

## Quick Start

### Generate Progressive Loading Files

```bash
# Generate for stdio MCP server
mcp-execution-cli generate github-mcp-server --env GITHUB_TOKEN=ghp_xxx

# Output: Files written to ~/.claude/servers/github/
#   - createIssue.ts (one tool)
#   - updateIssue.ts (another tool)
#   - ... (45 more tools)
#   - index.ts (re-exports all)
#   - _runtime/mcp-bridge.ts (runtime helper)
```

### Discover Available Tools

```bash
# List available MCP servers
ls ~/.claude/servers/

# List tools in a server
ls ~/.claude/servers/github/

# Load one tool (progressive loading - 98% token savings!)
cat ~/.claude/servers/github/createIssue.ts
```

### Shell Completions

```bash
# Generate completions for your shell
mcp-execution-cli completions bash > /etc/bash_completion.d/mcp-execution-cli
mcp-execution-cli completions zsh > ~/.zsh/completions/_mcp-execution-cli
mcp-execution-cli completions fish > ~/.config/fish/completions/mcp-execution-cli.fish
```

## Commands

### `generate`

Generate TypeScript files with progressive loading:

```bash
mcp-execution-cli generate <SERVER_OR_HTTP_URL> [OPTIONS]
```

**Transport Options**:
- `--http <URL>`: Use HTTP transport
- `--sse <URL>`: Use SSE transport
- `--arg <ARG>`: Server command argument (repeatable)
- `--env <KEY=VALUE>`: Environment variable (repeatable)
- `--header <KEY=VALUE>`: HTTP header (repeatable)
- `--cwd <PATH>`: Working directory

**Output Options**:
- `--progressive-output <PATH>`: Custom output directory (default: ~/.claude/servers/)
- `--format <FORMAT>`: Output format (json, text, pretty)

**Examples**:

```bash
# Stdio transport
mcp-execution-cli generate github-mcp-server \
  --env GITHUB_TOKEN=ghp_xxx

# HTTP transport
mcp-execution-cli generate --http https://api.example.com/mcp \
  --header "Authorization=Bearer token"

# SSE transport
mcp-execution-cli generate --sse https://api.example.com/mcp/events

# Docker container
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/org/server \
  --env=API_KEY=xxx

# Custom output directory
mcp-execution-cli generate github-mcp-server \
  --progressive-output /custom/path
```

### `introspect`

Analyze MCP servers and discover capabilities:

```bash
mcp-execution-cli introspect <SERVER_NAME> [OPTIONS]
```

**Options**:
- `--output, -o`: Output format (json, text, pretty)
- `--env <KEY=VALUE>`: Environment variables
- `--arg <ARG>`: Server arguments

**Example**:

```bash
mcp-execution-cli introspect github-mcp-server \
  --env GITHUB_TOKEN=ghp_xxx \
  --output json
```

### `stats`

View cache statistics:

```bash
mcp-execution-cli stats [--output json]
```

Shows MCP Bridge cache statistics (size, hits, capacity).

### `completions`

Generate shell completions:

```bash
mcp-execution-cli completions <SHELL>
```

**Shells**: bash, zsh, fish, powershell

## Integration with Claude Code

`mcp-execution-cli` generates TypeScript files that Claude Code can discover and use through progressive loading pattern.

### How It Works

1. **Generate**: Creates TypeScript files in `~/.claude/servers/{server-id}/`
2. **Discover**: Claude uses `ls` to discover available servers and tools
3. **Load**: Claude loads only the specific tools it needs (~500-1,500 tokens each)
4. **Savings**: 98% reduction vs loading all tools (~30,000 tokens)

### Instruction Skill

An instruction skill guides Claude Code:
- **Location**: `~/.claude/skills/mcp-progressive-loading/SKILL.md`
- **Purpose**: Teaches Claude how to discover and use generated TypeScript files
- **Pattern**: Discovery via `ls`, loading via `cat`

See [examples/progressive-loading-usage.md](../../examples/progressive-loading-usage.md) for complete tutorial.

## Token Savings Example

**Traditional approach** (load all tools):
```bash
cat ~/.claude/servers/github/index.ts  # ~30,000 tokens for 45 tools
```

**Progressive loading** (load one tool):
```bash
cat ~/.claude/servers/github/createIssue.ts  # ~500-1,500 tokens for 1 tool
# Savings: 98%! 🎉
```

## Generated TypeScript Structure

Each tool file includes:

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

/** Parameters for createIssue tool */
export interface CreateIssueParams {
  repo: string;           // Required (no ?)
  title: string;          // Required (no ?)
  body?: string;          // Optional (has ?)
  labels?: string[];      // Optional (has ?)
}

/** Result type for createIssue tool */
export interface CreateIssueResult {
  [key: string]: unknown;
}
```

## Performance

- **Generation**: ~2-3ms per server
- **Token Savings**: 98% (30,000 → 500-1,500 tokens per tool)
- **Memory**: Minimal (~2MB for 1000 tools)
- **Lightweight**: Single binary, no runtime dependencies

## Security

- **No Code Execution**: Generated TypeScript is for type information only
- **Command Injection Prevention**: All user input validated
- **Path Validation**: Rejects malicious paths (no directory traversal)
- **Template Security**: Handlebars templates escape all user input

## Documentation

For detailed documentation, see:
- [Progressive Loading Tutorial](../../examples/progressive-loading-usage.md)
- [Project README](../../README.md)
- [Architecture Decision](../../docs/adr/010-simplify-to-progressive-only.md)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! Please see the [project repository](https://github.com/bug-ops/mcp-execution) for guidelines.
