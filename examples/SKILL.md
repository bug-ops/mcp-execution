---
name: mcp-progressive-loading
description: Generate TypeScript files for MCP tools using progressive loading pattern. Achieves 98% token savings by loading only the tools you need instead of all tools at once.
---

# MCP Progressive Loading Skill

Generate and use MCP server tools with progressive loading - load only what you need, when you need it.

## What This Skill Does

This skill helps you:
1. **Generate** TypeScript files for any MCP server's tools
2. **Configure** progressive loading for optimal token usage
3. **Use** MCP tools with 98% token savings (30,000 → 500 tokens per tool)

## Quick Start

### Generate Tools for a Server

Ask Claude Code to generate progressive loading files:

```
"Generate progressive loading files for the GitHub MCP server"
```

Claude will run:
```bash
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=-e --arg=GITHUB_PERSONAL_ACCESS_TOKEN \
  --arg=ghcr.io/github/github-mcp-server \
  --env=GITHUB_PERSONAL_ACCESS_TOKEN=github_pat_YOUR_TOKEN \
  --name=github
```

Output: `~/.claude/servers/github/*.ts`

### Use Generated Tools

Once generated, ask Claude Code to use specific tools:

```
"Use the GitHub createIssue tool to report a bug"
"List all available GitHub tools"
"Show me what parameters createPullRequest needs"
```

## How Progressive Loading Works

### Traditional Approach (Without Progressive Loading)
```
Load all 40 tools → 30,000 tokens → Every request
```

### Progressive Loading Approach
```
Load 1 specific tool → 500 tokens → 98% savings
```

### Directory Structure

```
~/.claude/servers/github/
├── index.ts                    # Re-exports all tools
├── createIssue.ts              # Individual tool (loaded on-demand)
├── createPullRequest.ts        # Individual tool (loaded on-demand)
├── getAuthenticatedUser.ts     # Individual tool (loaded on-demand)
└── _runtime/
    └── mcp-bridge.ts           # Runtime helper for MCP calls
```

## Common Tasks

### 1. Generate for Local MCP Server

```
"Generate progressive loading for my local server at ./server.js"
```

Result:
```bash
mcp-execution-cli generate node --arg=./server.js --name=my-server
```

### 2. Generate for Docker-based Server

```
"Generate progressive loading for the Google Drive MCP server"
```

Result:
```bash
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=mcp/gdrive \
  --name=gdrive
```

### 3. Generate with Custom Output Directory

```
"Generate GitHub tools to /tmp/test-github"
```

Result:
```bash
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/github/github-mcp-server \
  --name=github \
  --progressive-output=/tmp/test-github
```

### 4. List Available Tools

```
"Show me all available GitHub tools"
```

Claude will:
```bash
ls ~/.claude/servers/github/*.ts
```

### 5. Inspect Tool Parameters

```
"What parameters does createIssue need?"
```

Claude will:
```bash
cat ~/.claude/servers/github/createIssue.ts
```

## Configuration

### MCP Server Configuration

After generating tools, configure your MCP server in `~/.claude/mcp.json`:

```json
{
  "mcpServers": {
    "github": {
      "command": "docker",
      "args": [
        "run", "-i", "--rm",
        "-e", "GITHUB_PERSONAL_ACCESS_TOKEN",
        "ghcr.io/github/github-mcp-server"
      ],
      "env": {
        "GITHUB_PERSONAL_ACCESS_TOKEN": "github_pat_YOUR_TOKEN_HERE"
      }
    }
  }
}
```

### Supported MCP Servers

Any MCP-compliant server:
- **GitHub** - Repository management, issues, PRs, code search
- **Google Drive** - File operations, search, sharing
- **Slack** - Messaging, channels, users
- **PostgreSQL** - Database queries and management
- **Custom servers** - Any server implementing MCP protocol

## Command Reference

### Generate Command

```bash
mcp-execution-cli generate [OPTIONS] <SERVER>

Arguments:
  <SERVER>  Server command (e.g., "docker", "node", "npx")

Options:
  --arg <ARGS>              Server arguments (repeatable)
  --env <KEY=VALUE>         Environment variables
  --name <NAME>             Custom server name for directory
  --progressive-output <DIR> Custom output directory
  --http <URL>              Use HTTP transport
  --sse <URL>               Use SSE transport
  --format <FORMAT>         Output format (json|text|pretty)
```

### Examples

**Simple Node.js server:**
```bash
mcp-execution-cli generate node --arg=./server.js
```

**Docker container with environment:**
```bash
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=-e --arg=DATABASE_URL \
  --arg=postgres-mcp-server \
  --env=DATABASE_URL=postgresql://localhost/mydb \
  --name=postgres
```

**HTTP transport:**
```bash
mcp-execution-cli generate \
  --http=https://api.example.com/mcp \
  --header=Authorization=Bearer_TOKEN \
  --name=api-server
```

## File Structure

Each generated tool file contains:

```typescript
/**
 * Create a new issue in a GitHub repository
 *
 * @param params - Issue creation parameters
 * @returns Promise resolving to created issue details
 */
export async function createIssue(params: CreateIssueParams): Promise<CreateIssueResult> {
  // Type-safe parameter validation
  // Automatic MCP connection
  // Error handling
}

export interface CreateIssueParams {
  owner: string;
  repo: string;
  title: string;
  body?: string;
  labels?: string[];
  assignees?: string[];
}

export interface CreateIssueResult {
  number: number;
  url: string;
  state: string;
}
```

## Benefits

### Token Savings
- **Before**: Load all 40 tools = 30,000 tokens per request
- **After**: Load 1 tool = 500 tokens per request
- **Savings**: 98% reduction in token usage

### Performance
- **Generation**: ~3ms per tool
- **Loading**: Only load what you need
- **Type Safety**: Full TypeScript types from JSON schemas

### Developer Experience
- **Discovery**: `ls ~/.claude/servers/` shows all servers
- **Exploration**: `cat tool.ts` shows tool details
- **Documentation**: JSDoc comments in every file
- **IDE Support**: Full autocomplete and type checking

## Troubleshooting

### "No tools generated"
- Check server is running: `docker ps` or `node server.js`
- Verify server responds to MCP protocol
- Check logs with `--verbose` flag

### "Cannot find module"
- Ensure `~/.claude/servers/` exists
- Run generation command again
- Check file permissions

### "Authentication failed"
- Verify environment variables in `~/.claude/mcp.json`
- Check API tokens are valid
- Ensure server has network access

## Advanced Usage

### Custom Output Directory

```bash
mcp-execution-cli generate github \
  --progressive-output=/custom/path
```

Result: `/custom/path/github/*.ts`

### Multiple Servers

Generate for multiple servers:

```bash
# GitHub
mcp-execution-cli generate docker --arg=... --name=github

# Google Drive
mcp-execution-cli generate docker --arg=... --name=gdrive

# Slack
mcp-execution-cli generate docker --arg=... --name=slack
```

All available at:
```
~/.claude/servers/
├── github/
├── gdrive/
└── slack/
```

### Server Introspection

Inspect server capabilities without generating:

```bash
mcp-execution-cli introspect docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/github/github-mcp-server \
  --detailed
```

## See Also

- [Progressive Loading Usage Guide](./progressive-loading-usage.md) - Complete usage guide
- [MCP Specification](https://github.com/modelcontextprotocol/specification) - Protocol details
- [rmcp SDK](https://docs.rs/rmcp) - Rust MCP implementation
- [Project README](../README.md) - Project overview

## Tips

1. **Generate once, use many times** - Generated files are cached locally
2. **Use `--name` for clarity** - `--name=github` is better than `docker`
3. **Check `index.ts`** - Lists all available tools with descriptions
4. **Update regularly** - Re-run generate when server adds new tools
5. **Version control** - Consider committing generated files for team sharing

## Example Session

```
User: "Set up GitHub progressive loading"

Claude: I'll generate progressive loading files for the GitHub MCP server.

[Runs generation command]

✓ Successfully generated progressive loading files
  Server: GitHub (github)
  Tools: 40
  Location: ~/.claude/servers/github

User: "What can I do with GitHub now?"

Claude: You have 40 GitHub tools available. Here are the main categories:

[Lists: createIssue, createPullRequest, searchCode, etc.]

User: "Create an issue in myorg/myrepo about fixing the login bug"

Claude: I'll use the createIssue tool.

[Loads only createIssue.ts (~500 tokens)]
[Creates the issue]

✓ Issue created: #123 "Fix login bug"
```

## Performance Metrics

- **Generation Time**: 2-3ms per tool
- **Token Usage**: 500-1,500 tokens per tool (vs 30,000 for all)
- **Token Savings**: 98%
- **File Size**: ~2-5KB per tool file
- **Type Safety**: 100% (full TypeScript types)

---

**Ready to use progressive loading?** Ask Claude Code to generate tools for your MCP server!
