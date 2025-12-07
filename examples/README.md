# Examples

Complete examples and guides for using MCP Code Execution with progressive loading.

## Quick Navigation

| Document | Description | Best For |
|----------|-------------|----------|
| [progressive-loading-usage.md](./progressive-loading-usage.md) | Complete usage tutorial | Learning how progressive loading works |
| [mcp.json.example](./mcp.json.example) | MCP server configurations | Configuring your MCP servers |

## Getting Started

### 1. For Claude Code Users

Skills can be generated dynamically using the `mcp-server` MCP tools:

```bash
# Start the mcp-server and use generate_skill/save_skill tools
mcp-execution

# Or use the mcp-execution-cli to generate files first
mcp-execution-cli generate --from-config github
```

Then ask Claude Code to execute MCP tools autonomously.

### 2. For CLI Users

If you're using the CLI directly, start with [progressive-loading-usage.md](./progressive-loading-usage.md):

```bash
# Generate TypeScript files for GitHub server
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/github/github-mcp-server \
  --name=github
```

### 3. For Configuration

Use [mcp.json.example](./mcp.json.example) as a template for your MCP server configuration:

```bash
# Copy example to Claude Code directory
cp examples/mcp.json.example ~/.claude/mcp.json
# Edit with your credentials
```

## What is Progressive Loading?

Progressive loading generates one TypeScript file per MCP tool, enabling AI agents to load only the tools they need:

**Before (Traditional):**
```
Load all 40 tools → 30,000 tokens → Every request
```

**After (Progressive Loading):**
```
Load 1 specific tool → 500 tokens → 98% savings
```

## File Structure After Generation

```
~/.claude/servers/github/
├── index.ts                    # Re-exports all tools
├── createIssue.ts              # Individual tool (loaded on-demand)
├── createPullRequest.ts        # Individual tool (loaded on-demand)
├── getAuthenticatedUser.ts     # Individual tool (loaded on-demand)
├── ... (40+ more tool files)
└── _runtime/
    └── mcp-bridge.ts           # Runtime helper for MCP calls
```

## Example Workflows

### Workflow 1: Setting Up GitHub

```bash
# 1. Generate tools
mcp-execution-cli generate docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=-e --arg=GITHUB_PERSONAL_ACCESS_TOKEN \
  --arg=ghcr.io/github/github-mcp-server \
  --env=GITHUB_PERSONAL_ACCESS_TOKEN=github_pat_YOUR_TOKEN \
  --name=github

# 2. Configure server (create ~/.claude/mcp.json)
# See mcp.json.example

# 3. Use with Claude Code
# "Create an issue in myorg/myrepo about fixing the login bug"
```

### Workflow 2: Multiple Servers

```bash
# Generate for GitHub
mcp-execution-cli generate docker --arg=... --name=github

# Generate for Google Drive
mcp-execution-cli generate docker --arg=... --name=gdrive

# Generate for Slack
mcp-execution-cli generate docker --arg=... --name=slack

# All available at ~/.claude/servers/
```

### Workflow 3: Introspection

```bash
# Inspect server capabilities before generating
mcp-execution-cli introspect docker \
  --arg=run --arg=-i --arg=--rm \
  --arg=ghcr.io/github/github-mcp-server \
  --detailed

# Shows all available tools, their parameters, and schemas
```

### Workflow 4: Using MCP Server Tools

The `mcp-execution` binary provides an MCP server with 5 tools for progressive loading generation:

```bash
# Add to your mcp.json
{
  "mcpServers": {
    "mcp-execution": {
      "command": "mcp-execution"
    }
  }
}
```

**Available MCP Tools:**

| Tool | Description |
|------|-------------|
| `introspect_server` | Connect to MCP server and discover tools |
| `save_categorized_tools` | Generate TypeScript files with categorization |
| `list_generated_servers` | List all servers with generated files |
| `generate_skill` | Scan tools directory and generate skill content |
| `save_skill` | Save generated skill to `~/.claude/skills/` |

**Example: Generate GitHub Tools via MCP**

```
User: "Generate progressive loading files for GitHub"

Claude uses:
1. introspect_server(server_id: "github") → Discovers 40 tools
2. save_categorized_tools(session_id: "...", categorizations: [...]) → Writes files
3. generate_skill(server_id: "github") → Creates skill context
4. save_skill(server_id: "github", content: "...") → Saves SKILL.md
```

**Example: List Generated Servers**

```
User: "What MCP servers have I generated?"

Claude uses:
1. list_generated_servers() → Returns list of servers in ~/.claude/servers/
```

## Supported MCP Servers

Progressive loading works with any MCP-compliant server:

### Official Servers
- **GitHub** - Repository management, issues, PRs, code search
- **Google Drive** - File operations, search, sharing
- **Slack** - Messaging, channels, users

### Database Servers
- **PostgreSQL** - Database queries and management
- **MySQL** - Database operations
- **MongoDB** - Document database operations

### Custom Servers
Any server implementing the [MCP specification](https://github.com/modelcontextprotocol/specification)

## Token Savings Calculation

### Example: GitHub MCP Server (40 tools)

**Traditional Approach:**
```
Load all tools: 40 × 750 tokens = 30,000 tokens
Every request:  30,000 tokens
```

**Progressive Loading:**
```
Load 1 tool:    1 × 500 tokens = 500 tokens
Savings:        29,500 tokens (98%)
```

### Real-World Impact

| Server | Tools | Traditional | Progressive | Savings |
|--------|-------|-------------|-------------|---------|
| GitHub | 40 | 30,000 tokens | 500 tokens | 98% |
| Google Drive | 25 | 18,750 tokens | 500 tokens | 97% |
| Slack | 30 | 22,500 tokens | 500 tokens | 98% |

## Performance Metrics

- **Generation Time**: 2-3ms per tool
- **File Size**: 2-5KB per tool file
- **Type Safety**: 100% (full TypeScript types from JSON schemas)
- **Compatibility**: Works with all MCP-compliant servers

## Troubleshooting

### Common Issues

**"No tools generated"**
- Check server is running: `docker ps` or verify process
- Use `--verbose` flag for detailed logs
- Verify server implements MCP protocol correctly

**"Cannot find generated files"**
- Check output directory: `ls ~/.claude/servers/`
- Verify generation completed without errors
- Check file permissions

**"Tool execution fails"**
- Verify `~/.claude/mcp.json` configuration
- Check environment variables are set correctly
- Ensure MCP SDK is installed: `npm list @modelcontextprotocol/sdk`

### Getting Help

1. Check [progressive-loading-usage.md](./progressive-loading-usage.md) for detailed guide
2. See [../docs/ARCHITECTURE.md](../docs/ARCHITECTURE.md) for technical details
3. Open an issue on GitHub

## Contributing Examples

Have a useful example or workflow? Contributions welcome:

1. Create a new markdown file in `examples/`
2. Follow the existing format and style
3. Add it to this README's navigation table
4. Submit a pull request

## See Also

- [Project README](../README.md) - Project overview and installation
- [Architecture](../docs/ARCHITECTURE.md) - System architecture and design
- [ADRs](../docs/adr/) - Architecture decision records
- [MCP Specification](https://github.com/modelcontextprotocol/specification) - Protocol specification
