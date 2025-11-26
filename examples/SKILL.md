---
name: mcp-progressive-loading
description: Generates TypeScript files for MCP server tools with progressive loading (98% token savings). Use when setting up MCP tools, configuring servers, or executing MCP operations autonomously via Node.js CLI.
---

# MCP Progressive Loading

Generates TypeScript tool definitions from MCP servers with progressive loading (load only what you need).

**Key capabilities:**
- Generate tools from any MCP server (GitHub, Google Drive, Slack, etc.)
- Execute tools autonomously via Node.js
- 98% token savings: ~500 tokens/tool vs ~30,000 for all tools

## Quick Start

**1. Generate tools** (loads config from `~/.claude/mcp.json`):
```bash
mcp-execution-cli generate --from-config github
```

**2. Use tools** (autonomous execution via Node.js):
```bash
node ~/.claude/servers/github/createIssue.ts '{"owner":"...", "repo":"...", "title":"Bug fix"}'
```

Natural language examples:
- "Generate progressive loading for GitHub server"
- "Create an issue in myorg/myrepo about the login bug"
- "List all available GitHub tools"

## How It Works

**Traditional:** Load all 40 tools = 30,000 tokens
**Progressive:** Load 1 tool = 500 tokens (98% savings)

Each tool is a separate `.ts` file in `~/.claude/servers/{server}/`:
- `createIssue.ts` - Individual tool (load on demand)
- `_runtime/mcp-bridge.ts` - Runtime bridge to MCP server
- `index.ts` - Exports all tools (optional)

## Common Tasks

### Setup (first time only)
Validates Node.js 18+ and MCP configuration:
```bash
mcp-execution-cli setup
```

### Generate Tools

**From mcp.json (recommended):**
```bash
mcp-execution-cli generate --from-config github
```

**Manual configuration:**
```bash
mcp-execution-cli generate docker --arg=... --name=my-server
```

### Discover Tools
```bash
ls ~/.claude/servers/github/*.ts          # List all tools
cat ~/.claude/servers/github/createIssue.ts  # Inspect parameters
```

### Execute Tools
Natural language works automatically:
- "Create a GitHub issue titled 'Fix login bug'"
- "Search for pull requests in myorg/myrepo"
- "Get my GitHub profile"

Claude will:
1. Read tool definition (~500 tokens)
2. Execute via Node.js: `node tool.ts '{"params":"..."}'`
3. Return JSON result

## Configuration

Required: `~/.claude/mcp.json` with server definitions:

```json
{
  "mcpServers": {
    "github": {
      "command": "docker",
      "args": ["run", "-i", "--rm", "-e", "GITHUB_PERSONAL_ACCESS_TOKEN",
               "ghcr.io/github/github-mcp-server"],
      "env": {"GITHUB_PERSONAL_ACCESS_TOKEN": "github_pat_..."}
    }
  }
}
```

Works with any MCP server: GitHub, Google Drive, Slack, PostgreSQL, custom servers.

## Command Reference

```bash
# From mcp.json (recommended)
mcp-execution-cli generate --from-config <NAME>

# Manual configuration
mcp-execution-cli generate <COMMAND> --arg=... --env=KEY=VALUE --name=<NAME>

# Common options
--progressive-output <DIR>  # Custom output directory (default: ~/.claude/servers/)
--http <URL>                # Use HTTP transport
--format json|text|pretty   # Output format
```

**Examples:**
```bash
mcp-execution-cli generate --from-config github
mcp-execution-cli generate node --arg=./server.js --name=local
mcp-execution-cli generate docker --arg=run --arg=-i --name=gdrive
```

## Benefits

- **98% token savings**: 500 tokens/tool vs 30,000 for all tools
- **Type safety**: Full TypeScript interfaces from MCP schemas
- **Autonomous execution**: Works via Node.js CLI
- **Zero dependencies**: Uses Node.js built-ins only
- **IDE support**: Full autocomplete and type checking

## Troubleshooting

**Tools not generated?**
- Verify server running: `docker ps` or check logs with `--verbose`
- Ensure MCP protocol compliance

**Module not found?**
- Re-run generation: `mcp-execution-cli generate --from-config <server>`
- Check `~/.claude/servers/` exists

**Authentication errors?**
- Validate tokens in `~/.claude/mcp.json`
- Check network access to server

## Reference

- [Complete Usage Guide](./progressive-loading-usage.md)
- [MCP Specification](https://github.com/modelcontextprotocol/specification)
- [Project Documentation](../README.md)
