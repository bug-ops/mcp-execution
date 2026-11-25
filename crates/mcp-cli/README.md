# mcp-execution-cli

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-cli.svg)](https://crates.io/crates/mcp-execution-cli)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-cli)](https://docs.rs/mcp-execution-cli)
[![codecov](https://codecov.io/gh/bug-ops/mcp-execution/graph/badge.svg?token=2UEW36O9AN&flag=mcp-cli)](https://codecov.io/gh/bug-ops/mcp-execution)
[![MSRV](https://img.shields.io/badge/MSRV-1.98-blue.svg)](https://github.com/bug-ops/mcp-execution)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](../../LICENSE.md)

Command-line interface that turns an MCP server into a typed TypeScript SDK a model programs against, so loops, joins and large results stay in code and only the answer reaches the context.

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
> Requires Rust 1.98 or later for building from source.

## Usage

### Generate the SDK

```bash
# From config file (recommended)
mcp-execution-cli generate --from-config github

# With manual configuration
mcp-execution-cli generate github-mcp-server --env GITHUB_TOKEN=ghp_xxx
```

> [!TIP]
> Use `--from-config` to load server configuration from `~/.claude/mcp.json`.

### Discover and Call Tools

```bash
# List available servers
ls ~/.claude/servers/

# List tools in a server
ls ~/.claude/servers/github/

# Read the compact signature of every tool
cat ~/.claude/servers/github/catalog.ts

# Read one tool in full
cat ~/.claude/servers/github/createIssue.ts

# Call one tool: the single argument is a JSON object
node ~/.claude/servers/github/createIssue.ts '{"owner":"myorg","repo":"myrepo","title":"Bug"}'
```

A tool file run with `node` starts a hidden `mcp-execution-cli helper` process: the binary is `$MCP_EXECUTION_CLI` when set, otherwise `mcp-execution-cli` on `PATH`. A set but unusable `MCP_EXECUTION_CLI` is an error and never falls back to `PATH`.

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

Generate the TypeScript SDK (one file per tool, a catalog and a runtime bridge):

```bash
mcp-execution-cli generate <SERVER> [OPTIONS]
```

**Options**:

- `--from-config <NAME>`: Load config from mcp.json
- `--arg <ARG>`: Server command argument (repeatable)
- `--env <KEY=VALUE>`: Environment variable (repeatable)
- `--cwd <CWD>`: Working directory for the server process
- `--http <URL>`: Use HTTP transport
- `--sse <URL>`: Use SSE transport
- `--header <KEY=VALUE>`: HTTP headers (repeatable; conflicts with `--from-config`)
- `--connect-timeout-secs <SECS>`: Connection timeout (1-600s; conflicts with `--from-config`)
- `--discover-timeout-secs <SECS>`: Tool discovery timeout (1-600s; conflicts with `--from-config`)
- `--name <NAME>`: Custom server name for output directory
- `--progressive-output <PATH>`: Custom output directory
- `--dry-run`: Preview files that would be generated without writing to disk
- `--format <FORMAT>`: Output format (json, text, pretty)
- `--log-format <FORMAT>`: Diagnostic log format (text, json); falls back to
  `MCP_EXECUTION_LOG_FORMAT` when unset

**Examples**:

```bash
# From config
mcp-execution-cli generate --from-config github

# Preview without writing
mcp-execution-cli generate --from-config github --dry-run

# Docker container with custom name
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/org/server \
  --env=API_KEY=xxx \
  --name=myserver
```

### `call`

Call one tool of one server and print its result within the output budget:

```bash
mcp-execution-cli call <SERVER FLAGS> -- <TOOL> [JSON]
```

Server flags are those of `generate` (`--from-config`, `--http`, `--sse`, or a command). The tool name and an optional JSON object follow `--`. The result is printed as one compact JSON line; an empty success prints nothing; a tool error exits 3; a non-object or malformed JSON argument exits 2 without echoing it.

```bash
mcp-execution-cli call --from-config github -- create_issue '{"title":"x"}'
mcp-execution-cli call --http https://api.example.com/mcp -- echo
```

### `run`

Run a TypeScript script against the generated servers with bounded output:

```bash
mcp-execution-cli run [SCRIPT|-] [OPTIONS] [-- ARGS...]
```

The script imports `mcp:<server-id>` (a server's tools) and `mcp:helpers` (`paginate`, `mapBounded`).

**Options**:

- `--servers-dir <PATH>`: Base directory of generated servers (default: `~/.claude/servers`)
- `--timeout <SECS>`: Run timeout, 1-600 (default 60)
- `--max-output <BYTES>`: Stdout printed before the rest is spilled to a private file (default 16384)
- `--max-stderr <BYTES>`: Stderr printed before the rest is spilled (default 4096)
- `--typecheck`: Run `tsc --noEmit` first; a type error exits 2 before node starts. Skipped with a note, and the script runs, when `tsc` is missing or when Node type definitions or the generated SDK's dependencies are not installed (run `npm install` in the server directory to enable the check); real type errors in the script still exit 2
- `--dry-run`: Execute only the tools listed under `reads` in the policy file; skip and report every other call
- `--policy <FILE>`: Policy for `--dry-run`, JSON `{"reads":["github/list_issues"]}` (default: `mcp-execution/policy.json` in the per-user config directory)
- `--restrict`: Unix only. Narrow file reads to the script, the servers directory and the run directory; no file writes, child processes or workers. Network is narrowed on Node.js 25 and later only; Windows refuses the flag (exit 2)
- `--save <NAME>`: Save the script as a recipe in `~/.claude/skills/<server>/scripts/` after it exits 0; `--overwrite` replaces an existing one

**Output**: the first stdout line is `[mcp-execution <token>] run started`, every runner line carries the same token, and the last is `[mcp-execution <token>] script_status exit 0` (`exit N`, `signal N`, `timed out`). A cut stream is saved to a file and a notice names it. After a failed run a call summary lists the calls made and their state, never values. `--format json` prints one envelope.

**Exit codes**: 0 success, 1 other error, 2 invalid input, 3 server or tool error, 4 timeout, 5 a check found problems, 6 the script exited non-zero or was killed.

> [!WARNING]
> `run` is not a sandbox: the script runs with your user privileges. A script cannot read its own stdin under `run`.

### `scripts`

Inspect the saved scripts of a skill:

```bash
mcp-execution-cli scripts list --server github    # signatures only, no bodies
mcp-execution-cli scripts check --server github   # exit 5 on problems, changes nothing
```

`--skills-dir <PATH>` selects another skills directory, and `scripts check --servers-dir <PATH>` another servers directory; a leading `~/` is expanded in both. `scripts check` exits 2 when the server's `_meta.json` has another schema version, and a symlinked `_meta.json` is refused. A saved script starts with an `mcp-script` header (JSON in a leading block comment); `generate` and `skill` never touch the `scripts/` directory.

### `stats`

Print deterministic size figures of a generated server:

```bash
mcp-execution-cli stats github [--servers-dir <PATH>]
```

For the server's recorded definitions, all tool files, one tool file (mean) and `catalog.ts`, it prints bytes and estimated tokens (bytes divided by 4, always labelled an estimate) and how many tools declare an output schema. Missing data prints `not available`, never zero; the output is identical on every run; a `_meta.json` older than schema version 3 exits 2 with no figures.

### `drift`

Compare a live server's tool list with the metadata recorded at generation:

```bash
mcp-execution-cli drift --from-config github
```

Takes the server flags of `generate` plus `--name` and `--progressive-output` (pass the same values you gave `generate`). Exit 0: no drift. Exit 5: the lists differ. Writes nothing.

### `introspect`

Analyze MCP servers and discover capabilities:

```bash
mcp-execution-cli introspect <SERVER> [OPTIONS]
```

**Configuration Modes**:

1. Load from `~/.claude/mcp.json` (recommended):
   ```bash
   mcp-execution-cli introspect --from-config github
   ```

2. Manual configuration:
   ```bash
   mcp-execution-cli introspect github-mcp-server --arg=stdio
   ```

**Options**:

- `--from-config <NAME>`: Load config from mcp.json
- `--arg <ARG>`: Server command argument (repeatable)
- `--env <KEY=VALUE>`: Environment variable (repeatable)
- `--cwd <CWD>`: Working directory for the server process
- `--http <URL>`: Use HTTP transport
- `--sse <URL>`: Use SSE transport
- `--header <KEY=VALUE>`: HTTP headers (repeatable; conflicts with `--from-config`)
- `--connect-timeout-secs <SECS>`: Connection timeout (1-600s; conflicts with `--from-config`)
- `--discover-timeout-secs <SECS>`: Tool discovery timeout (1-600s; conflicts with `--from-config`)
- `--detailed`: Show full input/output schemas
- `--format <FORMAT>`: Output format (json, text, pretty)
- `--log-format <FORMAT>`: Diagnostic log format (text, json); falls back to
  `MCP_EXECUTION_LOG_FORMAT` when unset

**Examples**:

```bash
# From config with detailed schemas
mcp-execution-cli introspect --from-config github --detailed

# Manual with Docker
mcp-execution-cli introspect docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/github/github-mcp-server \
  --env=GITHUB_TOKEN=ghp_xxx

# HTTP transport with headers
mcp-execution-cli introspect --http https://api.example.com/mcp \
  --header "Authorization=Bearer token" \
  --header "X-Custom-Header=value"

# With custom timeouts
mcp-execution-cli introspect docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/slow-server \
  --connect-timeout-secs 60 \
  --discover-timeout-secs 120
```

### `skill`

Generate Claude Code skill files (SKILL.md) from generated tools:

```bash
mcp-execution-cli skill --server <SERVER_ID> [OPTIONS]
```

**Options**:

- `--server, -s <ID>`: Server identifier (required)
- `--servers-dir <PATH>`: Custom servers directory
- `--output, -o <PATH>`: Custom output path
- `--skill-name <NAME>`: Custom skill name
- `--hint <HINT>`: Use case hints (repeatable)
- `--overwrite`: Overwrite existing SKILL.md

**Examples**:

```bash
# Generate skill for GitHub server
mcp-execution-cli skill --server github

# With custom output and hints
mcp-execution-cli skill --server github \
  --output ~/.claude/skills/github/SKILL.md \
  --hint "managing pull requests" \
  --hint "code review"

# Overwrite existing skill
mcp-execution-cli skill --server github --overwrite
```

> [!TIP]
> For optimal results, prefer using the MCP server (`mcp-execution-server`) for skill generation.
> The MCP server can leverage LLM capabilities to summarize tool descriptions and reduce
> context size, resulting in more concise and effective skill files.

### `server`

Manage MCP server configurations from `~/.claude/mcp.json`:

```bash
# List all configured servers
mcp-execution-cli server list

# Show detailed information about a server
mcp-execution-cli server info github

# Validate a server command
mcp-execution-cli server validate docker
```

### `setup`

Validate the runtime environment for generated MCP tool execution:

```bash
mcp-execution-cli setup

# Output:
# ✓ Node.js v26.13.1 detected
# ✓ MCP configuration found: ~/.claude/mcp.json
# ✓ Made 13 TypeScript files executable
# ✓ Helper binary found: /usr/local/bin/mcp-execution-cli
# ✓ Runtime setup complete
```

Checks that Node.js 22.18.0 or newer is installed (23.6.0 or newer on the 23 line; generated tools run as `.ts` files), `~/.claude/mcp.json` exists, that the helper binary (`$MCP_EXECUTION_CLI`, else `mcp-execution-cli` on `PATH`) is found, and makes generated TypeScript files executable (Unix only).

### `completions`

Generate shell completions:

```bash
mcp-execution-cli completions <SHELL>
```

> [!NOTE]
> Supported shells: bash, zsh, fish, powershell

## Features

- **Typed SDK**: One TypeScript file per MCP tool, plus a type-only catalog
- **Script Runner**: `run` with bounded output, call journal, type check, dry run and `--restrict`
- **Saved Scripts and Measurement**: `scripts`, `stats` and `drift`
- **Type-Safe**: Full TypeScript interfaces from MCP schemas
- **Multiple Transports**: stdio, HTTP, SSE, Docker
- **Shell Completions**: bash, zsh, fish, PowerShell

## Security

- **No Code Execution by the Generator**: `generate` writes files and never runs them; running a generated tool is your decision
- **No Sandbox for `run`**: a script runs with your privileges; the journal, `--dry-run` and `--restrict` narrow and report but are not a security boundary
- **Command Injection Prevention**: All user input validated
- **Path Validation**: Rejects malicious paths
- **Template Security**: Handlebars escapes all user input

> [!WARNING]
> Never pass untrusted input directly to `--arg` or `--env` options.

## Related Crates

This crate is part of the [mcp-execution](https://github.com/bug-ops/mcp-execution) workspace:

- [`mcp-execution-core`](../mcp-core) - Foundation types
- [`mcp-execution-introspector`](../mcp-introspector) - MCP server analysis
- [`mcp-execution-codegen`](../mcp-codegen) - TypeScript code generation
- [`mcp-execution-files`](../mcp-files) - Virtual filesystem
- [`mcp-execution-skill`](../mcp-skill) - Skill generation
- [`mcp-execution-server`](../mcp-server) - MCP server

## MSRV Policy

Minimum Supported Rust Version: **1.98**

MSRV = stable minus two, raised in a minor release.

## License

Licensed under either of [Apache License 2.0](../../LICENSE.md) or [MIT license](../../LICENSE.md) at your option.
