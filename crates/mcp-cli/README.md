# mcp-execution-cli

Command-line interface for MCP Code Execution.

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-cli.svg)](https://crates.io/crates/mcp-execution-cli)
[![Documentation](https://docs.rs/mcp-execution-cli/badge.svg)](https://docs.rs/mcp-execution-cli)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE-MIT)

## Overview

`mcp-execution-cli` provides a comprehensive command-line interface for MCP Code Execution, enabling server introspection, code generation, WASM execution, and skill management. It implements the [Code Execution pattern for MCP](https://www.anthropic.com/engineering/code-execution-with-mcp), achieving 60-80% token savings through progressive tool loading.

## Features

- **9 Subcommands**: introspect, generate, execute, skill, server, stats, debug, config, completions
- **Multiple Output Formats**: JSON, text, pretty-printed
- **Shell Completions**: bash, zsh, fish, PowerShell
- **Security Profiles**: Strict/Moderate/Permissive configurations
- **Skill Management**: Save, load, list, test, and remove skills
- **Type-Safe**: Strong types throughout with validation

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

### Introspect an MCP Server

```bash
# Discover server capabilities
mcp-execution-cli introspect github --output json
```

### Generate Code and Skills

```bash
# Generate TypeScript code and save as skill
mcp-execution-cli generate github --save-skill

# Custom skill directory
mcp-execution-cli generate github --save-skill --skill-dir ~/.mcp-skills
```

### Execute WASM Modules

```bash
# Execute with default security profile
mcp-execution-cli execute module.wasm --entry main

# Execute with strict security profile
mcp-execution-cli execute module.wasm --entry main --profile strict

# Custom memory and timeout limits
mcp-execution-cli execute module.wasm --entry main --memory 512 --timeout 30
```

### Manage Skills

```bash
# List all saved skills
mcp-execution-cli skill list

# Load a skill
mcp-execution-cli skill load github -o pretty

# Show skill details
mcp-execution-cli skill info github

# Test skill validity
mcp-execution-cli skill test github

# Test all skills with strict validation
mcp-execution-cli skill test --all --strict

# Remove a skill
mcp-execution-cli skill remove github -y
```

### Shell Completions

```bash
# Generate completions for your shell
mcp-execution-cli completions bash > /etc/bash_completion.d/mcp-execution-cli
mcp-execution-cli completions zsh > ~/.zsh/completions/_mcp-execution-cli
mcp-execution-cli completions fish > ~/.config/fish/completions/mcp-execution-cli.fish
```

## Commands

### `introspect`

Analyze MCP servers and discover capabilities:

```bash
mcp-execution-cli introspect <SERVER_NAME> [OPTIONS]
```

**Options**:
- `--output, -o`: Output format (json, text, pretty)

### `generate`

Generate TypeScript code or skills:

```bash
mcp-execution-cli generate <SERVER_NAME> [OPTIONS]
```

**Options**:
- `--save-skill`: Save as reusable skill
- `--skill-dir`: Custom skill directory
- `--skill-name`: Custom skill name
- `--output, -o`: Output format

### `execute`

Execute WASM modules:

```bash
mcp-execution-cli execute <MODULE> [OPTIONS]
```

**Options**:
- `--entry, -e`: Entry point function (default: main)
- `--args, -a`: Function arguments
- `--profile`: Security profile (strict, moderate, permissive)
- `--memory`: Memory limit in MB
- `--timeout`: Timeout in seconds
- `--list-exports`: List module exports
- `--output, -o`: Output format

### `skill`

Manage saved skills:

```bash
# List skills
mcp-execution-cli skill list [--output json]

# Load skill
mcp-execution-cli skill load <NAME> [--output pretty]

# Show info
mcp-execution-cli skill info <NAME> [--output json]

# Test skill
mcp-execution-cli skill test <NAME> [--strict]

# Test all skills
mcp-execution-cli skill test --all [--strict] [--format json]

# Remove skill
mcp-execution-cli skill remove <NAME> [--yes]
```

### `server`, `stats`, `debug`, `config`

Additional utilities for server management, statistics, debugging, and configuration.

### `completions`

Generate shell completions:

```bash
mcp-execution-cli completions <SHELL>
```

**Shells**: bash, zsh, fish, powershell

## Integration with Claude Code

`mcp-execution-cli` generates Claude Agent Skills that Claude Code and Claude Desktop can use directly. Skills are Markdown files that teach Claude how to interact with MCP servers, achieving 60-80% token savings.

```bash
# Generate skill from your MCP server
mcp-execution-cli generate github --skill-name github

# Skills automatically appear in Claude Code
# Files created:
# ~/.claude/skills/github/SKILL.md       (Main documentation)
# ~/.claude/skills/github/REFERENCE.md   (Detailed API reference)
# ~/.claude/skills/github/metadata.json  (Server metadata)
```

## Performance

- **Fast**: <50ms for most operations
- **Efficient**: Minimal memory usage
- **Cached**: WASM modules cached for reuse
- **Lightweight**: Single binary, no runtime dependencies

## Security

- **Command Injection Prevention**: All user input validated
- **Path Validation**: Rejects malicious paths
- **Security Profiles**: Three-tier security system
- **Checksum Verification**: Blake3 integrity checking
- **Atomic Operations**: Prevents race conditions

## Documentation

For detailed documentation, see:
- [Project Documentation](https://github.com/bug-ops/mcp-execution)
- [Getting Started Guide](https://github.com/bug-ops/mcp-execution/blob/master/GETTING_STARTED.md)
- [Architecture](https://github.com/bug-ops/mcp-execution/blob/master/docs/ARCHITECTURE.md)

## License

Licensed under either of:

- Apache License, Version 2.0 ([LICENSE-APACHE](../../LICENSE-APACHE))
- MIT license ([LICENSE-MIT](../../LICENSE-MIT))

at your option.

## Contributing

Contributions are welcome! Please see the [project repository](https://github.com/bug-ops/mcp-execution) for guidelines.
