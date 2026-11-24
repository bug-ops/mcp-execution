# Usage Guide

Complete guide for generating a typed TypeScript SDK from an MCP server and calling its tools from scripts.

## Overview

`generate` writes one TypeScript file per MCP tool, a type-only `catalog.ts` with one signature per tool, and a runtime bridge that talks to the server. A model reads the signatures it needs, writes a script that calls the tools, and reads only what the script prints. Loops, joins and large results stay in the script.

Only text a model reads costs context. Importing a module when a script runs puts nothing into a model's context.

### Script or Direct Call

| Task | Better choice |
|------|---------------|
| Single lookup, or one or two sequential calls | Direct tool call |
| Small server catalog | Direct tool call |
| Many calls over a list, pagination, joins across tools | Script |
| Large results filtered or aggregated before use | Script |

### Requirements

Generated files are TypeScript that Node.js runs directly, which needs Node.js 22.18.0 or newer (on the 23 line, 23.6.0 or newer). Run `mcp-execution-cli setup` to check the installed version.

## Quick Start

### 1. Generate the SDK

```bash
# Generate TypeScript files for GitHub MCP server
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=-e --arg=GITHUB_PERSONAL_ACCESS_TOKEN \
  --arg=ghcr.io/github/github-mcp-server \
  --env=GITHUB_PERSONAL_ACCESS_TOKEN=github_pat_YOUR_TOKEN \
  --name=github

# Files are created in: ~/.claude/servers/github/
```

### 2. Configure MCP Servers

Create `~/.claude/mcp.json` with your server configurations:

```json
{
  "mcpServers": {
    "github": {
      "command": "docker",
      "args": [
        "run",
        "-i",
        "--rm",
        "-e",
        "GITHUB_PERSONAL_ACCESS_TOKEN",
        "ghcr.io/github/github-mcp-server"
      ],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "github_pat_YOUR_TOKEN_HERE"
      }
    }
  }
}
```

See [`mcp.json.example`](./mcp.json.example) for more server examples.

### 3. Use Generated Tools

Run one tool directly. The single argument is a JSON object:

```bash
node ~/.claude/servers/github/createIssue.ts '{"owner":"myorg","repo":"myrepo","title":"Bug: Login button not working"}'
```

On success the result is printed to stdout as one JSON line. On failure one JSON line with `error` and `kind` goes to stderr and the exit status is 1.

Or write a script that calls several tools. Keep it in a working directory of your own, for example `~/scripts/`, never inside `~/.claude/servers/<id>/`: `generate` replaces that whole directory on every run, so anything saved there is deleted. Import the tools by a relative path from the script, with the `.ts` extension:

```typescript
// ~/scripts/my-script.mts
import { createIssue } from '../.claude/servers/github/createIssue.ts';
import { listIssues } from '../.claude/servers/github/listIssues.ts';

const issue = await createIssue({
  owner: 'myorg',
  repo: 'myrepo',
  title: 'Bug: Login button not working',
  body: 'Steps to reproduce:\n1. Navigate to /login\n2. Click login button\n3. Nothing happens'
});

console.log(`Created issue #${issue.number}`);
```

```bash
node ~/scripts/my-script.mts
```

## Generated File Structure

```
~/.claude/servers/github/
├── _runtime/
│   ├── mcp-bridge.ts       # Runtime bridge for MCP communication
│   └── cli.ts              # Single-call entry point shared by the tool files
├── index.ts                # Re-exports all tools
├── catalog.ts              # One type-only signature per tool
├── createIssue.ts          # One file per tool
├── updateIssue.ts
├── listIssues.ts
├── package.json
├── tsconfig.json
├── _meta.json              # Metadata sidecar
└── ...                     # One file per remaining tool
```

> **Warning:** `generate` replaces the whole server directory on every run. Do not save your own scripts or edits inside it.

## Best Practices

### DO: Read the Catalog First

```bash
cat ~/.claude/servers/github/catalog.ts
```

The catalog is the compact way to see what a server offers. Open a tool file only when you need its full types and documentation.

### DO: Import Only the Tools a Script Uses

```typescript
import { createIssue } from '../.claude/servers/github/createIssue.ts';
```

### Avoid: Reading Every Tool File Into a Model's Context

Reading `index.ts` or every tool file puts all of that text into the context of the model that reads it. Importing `index.ts` from a script costs nothing in context, but it adds nothing over importing the tools you call.

## Server Configuration

### GitHub MCP Server (Docker)

```json
{
  "github": {
    "command": "docker",
    "args": [
      "run", "-i", "--rm",
      "-e", "GITHUB_PERSONAL_ACCESS_TOKEN",
      "ghcr.io/github/github-mcp-server"
    ],
    "env": {
      "GITHUB_PERSONAL_ACCESS_TOKEN": "github_pat_..."
    }
  }
}
```

### Filesystem MCP Server (npx)

```json
{
  "filesystem": {
    "command": "npx",
    "args": [
      "-y",
      "@modelcontextprotocol/server-filesystem",
      "/path/to/allowed/directory"
    ]
  }
}
```

### PostgreSQL MCP Server

```json
{
  "postgres": {
    "command": "npx",
    "args": [
      "-y",
      "@modelcontextprotocol/server-postgres",
      "postgresql://localhost/mydb"
    ]
  }
}
```

## Runtime Bridge

The `_runtime/mcp-bridge.ts` file provides the connection between generated TypeScript and MCP servers.

### Key Features

1. **Connection Caching**: Reuses MCP client connections across multiple tool calls
2. **Automatic Configuration**: Reads from `~/.claude/mcp.json`
3. **Error Handling**: Clear error messages for debugging
4. **Type Safety**: Full TypeScript type checking

### API

#### `callMCPTool(serverId, toolName, params)`

Calls an MCP tool with given parameters.

```typescript
import { callMCPTool } from '../.claude/servers/github/_runtime/mcp-bridge.ts';

const result = await callMCPTool('github', 'create_issue', {
  owner: 'myorg',
  repo: 'myrepo',
  title: 'Bug report',
  body: 'Description'
});
```

#### `closeAllConnections()`

Closes all cached connections (call during shutdown).

```typescript
import { closeAllConnections } from '../.claude/servers/github/_runtime/mcp-bridge.ts';

// Cleanup on exit
process.on('SIGINT', async () => {
  await closeAllConnections();
  process.exit(0);
});
```

## Examples

The scripts below live in `~/scripts/`, outside the generated directory, which `generate` replaces on every run. The import paths are relative to that location.

### Create GitHub Issue

```typescript
import { createIssue } from '../.claude/servers/github/createIssue.ts';

const issue = await createIssue({
  owner: 'modelcontextprotocol',
  repo: 'specification',
  title: 'Feature request: Support for streaming responses',
  body: 'It would be great to support streaming for long-running operations...',
  labels: ['enhancement', 'discussion']
});

console.log(`Issue created: ${issue.html_url}`);
```

### List Pull Requests

```typescript
import { listPullRequests } from '../.claude/servers/github/listPullRequests.ts';

const prs = await listPullRequests({
  owner: 'modelcontextprotocol',
  repo: 'specification',
  state: 'open',
  perPage: 10
});

prs.forEach(pr => {
  console.log(`#${pr.number}: ${pr.title} by ${pr.user.login}`);
});
```

### Search Code

```typescript
import { searchCode } from '../.claude/servers/github/searchCode.ts';

const results = await searchCode({
  query: 'language:rust mcp',
  perPage: 5
});

results.items.forEach(item => {
  console.log(`${item.repository.full_name}:${item.path}`);
});
```

## Troubleshooting

### Error: MCP configuration file not found

**Solution**: Create `~/.claude/mcp.json` with your server configurations.

```bash
cp examples/mcp.json.example ~/.claude/mcp.json
# Edit with your tokens and paths
```

### Error: Server 'xyz' not found in config

**Solution**: Add the server to your `mcp.json`:

```json
{
  "mcpServers": {
    "xyz": {
      "command": "...",
      "args": [...]
    }
  }
}
```

### Error: Unsupported file extension ".ts" (or similar from Node.js)

**Solution**: Your Node.js is too old to run TypeScript directly. Install Node.js 22.18.0 or newer (23.6.0 or newer on the 23 line) and run `mcp-execution-cli setup`.

## Performance

Generation and export timings come from the Criterion benchmarks:

```bash
cargo bench --profile bench-fast -p mcp-execution-codegen -p mcp-execution-files
```

The runtime bridge caches connections, so repeated calls in one script reuse the same server process.

## CLI Commands

### Generate the SDK

```bash
mcp-execution-cli generate <server> [OPTIONS]
```

Options:
- `--name <NAME>` - Custom directory name (default: server command)
- `--arg <ARG>` - Server command argument (repeatable)
- `--env <KEY=VALUE>` - Environment variable (repeatable)
- `--progressive-output <DIR>` - Custom output directory

### Examples

```bash
# GitHub via Docker with custom name
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/github/github-mcp-server \
  --name=github

# Filesystem via npx
mcp-execution-cli generate npx \
  --arg=-y \
  --arg=@modelcontextprotocol/server-filesystem \
  --arg=/path/to/dir \
  --name=filesystem

# Custom output directory
mcp-execution-cli generate docker \
  --arg=... \
  --progressive-output=/tmp/my-tools
```

## Architecture

```
┌─────────────────────┐
│ Your Script         │  Loops, joins, filtering
│ (my-script.mts)     │  Prints only the answer
└──────────┬──────────┘
           │
           ↓
┌─────────────────────┐
│ Generated Tools     │  One file per tool
│ (createIssue.ts)    │  Type-safe interfaces
└──────────┬──────────┘
           │
           ↓
┌─────────────────────┐
│ Runtime Bridge      │  Connection management
│ (mcp-bridge.ts)     │  JSON-RPC over stdio, built on Node.js built-ins
└──────────┬──────────┘
           │
           ↓
┌─────────────────────┐
│ MCP Server          │  GitHub, Filesystem, etc.
│ (Docker/npx)        │  Tool execution
└─────────────────────┘
```

## Further Reading

- [MCP Specification](https://github.com/modelcontextprotocol/specification)
- [GitHub MCP Server](https://github.com/github/github-mcp-server)
