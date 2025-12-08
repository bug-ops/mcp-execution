# Progressive Loading Usage Guide

Complete guide for using progressive loading pattern with MCP tools.

## Overview

Progressive loading generates one TypeScript file per MCP tool, achieving **98% token savings** by loading only the tools you need. Instead of loading 30,000 tokens for all tools, you load 500-1,500 tokens per tool.

## Quick Start

### 1. Generate Progressive Loading Files

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

### 3. Install Dependencies

The runtime bridge requires the MCP SDK:

```bash
npm install @modelcontextprotocol/sdk
```

### 4. Use Generated Tools

Load only the tools you need:

```typescript
// Load only the specific tools needed
import { createIssue } from '~/.claude/servers/github/createIssue.js';
import { listIssues } from '~/.claude/servers/github/listIssues.js';

// Call tools with type-safe parameters
const issue = await createIssue({
  owner: 'myorg',
  repo: 'myrepo',
  title: 'Bug: Login button not working',
  body: 'Steps to reproduce:\n1. Navigate to /login\n2. Click login button\n3. Nothing happens'
});

console.log(`Created issue #${issue.number}`);
```

## Generated File Structure

```
~/.claude/servers/github/
├── _runtime/
│   └── mcp-bridge.ts       # Runtime bridge for MCP communication
├── index.ts                 # Re-exports all tools (not recommended)
├── createIssue.ts          # Individual tool (500 tokens)
├── updateIssue.ts          # Individual tool (500 tokens)
├── listIssues.ts           # Individual tool (500 tokens)
└── ...                      # 37 more tools
```

## Best Practices

### ✅ DO: Load Specific Tools

```typescript
// GOOD: Load only what you need (500 tokens)
import { createIssue } from '~/.claude/servers/github/createIssue.js';
```

### ❌ DON'T: Load All Tools

```typescript
// BAD: Loads everything (30,000 tokens)
import * as github from '~/.claude/servers/github/index.js';
```

## Token Savings Comparison

| Pattern | Tools Loaded | Tokens | Savings |
|---------|--------------|--------|---------|
| **Traditional** | All 40 tools | ~30,000 | 0% |
| **Progressive** | 1 tool | ~500 | 98% |
| **Progressive** | 3 tools | ~1,500 | 95% |
| **Progressive** | 10 tools | ~5,000 | 83% |

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
// Cleanup on exit
process.on('SIGINT', async () => {
  await closeAllConnections();
  process.exit(0);
});
```

## Examples

### Create GitHub Issue

```typescript
import { createIssue } from '~/.claude/servers/github/createIssue.js';

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
import { listPullRequests } from '~/.claude/servers/github/listPullRequests.js';

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
import { searchCode } from '~/.claude/servers/github/searchCode.js';

const results = await searchCode({
  query: 'language:rust progressive loading',
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

### Error: Cannot find module '@modelcontextprotocol/sdk'

**Solution**: Install the MCP SDK:

```bash
npm install @modelcontextprotocol/sdk
```

## Performance

Progressive loading is optimized for:

- **Generation**: ~2ms to generate 42 files (TypeScript code)
- **Loading**: 500-1,500 tokens per tool vs 30,000 for all tools
- **Runtime**: Connections cached and reused across calls

## CLI Commands

### Generate Progressive Loading Files

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
│ Generated Tools     │  500-1,500 tokens each
│ (createIssue.ts)    │  Type-safe interfaces
└──────────┬──────────┘
           │
           ↓
┌─────────────────────┐
│ Runtime Bridge      │  Connection management
│ (mcp-bridge.ts)     │  Parameter serialization
└──────────┬──────────┘
           │
           ↓
┌─────────────────────┐
│ MCP SDK             │  Official @modelcontextprotocol/sdk
│ (StdioTransport)    │  stdio communication
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
- [MCP TypeScript SDK](https://github.com/modelcontextprotocol/typescript-sdk)
- [GitHub MCP Server](https://github.com/github/github-mcp-server)
