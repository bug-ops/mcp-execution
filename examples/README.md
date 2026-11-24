# Examples

Complete examples and guides for generating a typed TypeScript SDK from an MCP server and scripting against it.

## Quick Navigation

| Document | Description | Best For |
|----------|-------------|----------|
| [progressive-loading-usage.md](./progressive-loading-usage.md) | Complete usage tutorial | Learning how the generated SDK is used |
| [mcp.json.example](./mcp.json.example) | MCP server configurations | Configuring your MCP servers |
| [host-code-mode.md](./host-code-mode.md) | What the host documents about its own code execution mode | Deciding how it relates to `run` |

## Getting Started

### 1. For Claude Code Users

Skills can be generated dynamically using the `mcp-execution-server` MCP tools:

```bash
# Start the mcp-execution-server and use generate_skill/save_skill tools
mcp-execution

# Or use the mcp-execution-cli to generate files first
mcp-execution-cli generate --from-config github
```

Then ask Claude Code to call the generated tools, directly or from a script.

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

#### Overriding connect/discover timeouts

Slow-starting servers can override the default 30-second connect/discover
timeouts (bounded to `0 < timeout <= 600` seconds) per server entry in
`mcp.json`:

```json
{
  "mcpServers": {
    "slow-server": {
      "command": "npx",
      "args": ["-y", "@example/slow-mcp-server"],
      "connectTimeoutSecs": 60,
      "discoverTimeoutSecs": 120
    }
  }
}
```

For a manually-configured server (not `--from-config`), pass the same
overrides as CLI flags instead:

```bash
mcp-execution-cli introspect slow-mcp-server \
  --connect-timeout-secs 60 --discover-timeout-secs 120
```

#### HTTP and SSE servers in `mcp.json`

Remote servers can be defined alongside stdio servers in the same file by
setting `"type"` and `"url"` instead of `"command"`:

```json
{
  "mcpServers": {
    "local-server": {
      "command": "npx",
      "args": ["-y", "@example/local-mcp-server"]
    },
    "remote-http": {
      "type": "http",
      "url": "https://api.example.com/mcp",
      "headers": {
        "Authorization": "Bearer YOUR_TOKEN_HERE"
      }
    },
    "remote-sse": {
      "type": "sse",
      "url": "https://example.com/sse"
    }
  }
}
```

`"type"` defaults to `"stdio"` when `"command"` is set, so existing stdio-only
configs keep working unchanged.

## What Does Generation Produce?

`generate` writes one TypeScript file per MCP tool, a type-only `catalog.ts` with one signature per tool, and a runtime bridge. A model reads the signatures it needs, then writes a script that calls the tools and prints only the answer. Loops, joins and large results stay in the script.

For a single lookup or one or two sequential calls, a direct tool call is cheaper than a script; see [progressive-loading-usage.md](./progressive-loading-usage.md#script-or-direct-call).

## From Generation to a Saved Script

Every server below is read from `~/.claude/mcp.json` (see [mcp.json.example](./mcp.json.example)). The commands and their flags are described in [progressive-loading-usage.md](./progressive-loading-usage.md).

```bash
# 1. Generate, then read the catalog: one type-only signature per tool
mcp-execution-cli generate --from-config github
cat ~/.claude/servers/github/catalog.ts

# 2. One lookup: call the tool directly (the tool name follows `--`)
mcp-execution-cli call --from-config github -- list_issues '{"owner":"myorg","repo":"myrepo"}'

# 3. Many calls: write a script in ~/scripts/ (imports use `mcp:github`) and run it
mcp-execution-cli run ~/scripts/triage.mts

# 4. Optional checks before a run that changes things
mcp-execution-cli run ~/scripts/triage.mts --typecheck
mcp-execution-cli run ~/scripts/triage.mts --dry-run --policy ~/policy.json
mcp-execution-cli run ~/scripts/triage.mts --restrict          # Unix only

# 5. Keep a script that worked (it needs an `mcp-script` header), then find it again
mcp-execution-cli run ~/scripts/triage.mts --save triage
mcp-execution-cli scripts list --server github
mcp-execution-cli scripts check --server github

# 6. Measure the context cost and check the server has not changed
mcp-execution-cli stats github
mcp-execution-cli drift --from-config github
```

`~/policy.json` for the dry run lists the calls allowed to execute, for example `{"reads":["github/list_issues"]}`; every other call is skipped.

> **Warning:** `run` is not a sandbox. Scripts run with your user privileges, they cannot read their own stdin under `run`, and `--restrict` narrows network access only on Node.js 25 and later.

After upgrading `mcp-execution-cli`, regenerate every server (`generate` again): the generated bridge and the metadata schema changed. A tool file run directly with `node` finds the helper through `MCP_EXECUTION_CLI` or `PATH`.

## File Structure After Generation

```
~/.claude/servers/github/
├── index.ts                    # Re-exports all tools
├── catalog.ts                  # One type-only signature per tool
├── createIssue.ts              # One file per tool
├── createPullRequest.ts
├── getAuthenticatedUser.ts
├── ... (one file per remaining tool)
├── _meta.json                  # Metadata sidecar (schema version 3)
├── package.json
├── tsconfig.json
└── _runtime/
    ├── mcp-bridge.ts           # Thin client of the mcp-execution-cli helper
    └── cli.ts                  # Single-call entry point shared by the tool files
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
# Claude reads catalog.ts, then uses `mcp-execution-cli call` for one tool
# or `mcp-execution-cli run` for a script
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

The `mcp-execution` binary provides an MCP server with 5 tools for SDK generation:

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
User: "Generate the SDK for GitHub"

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

Generation works with any MCP-compliant server:

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

## Performance

Generation and export timings come from the Criterion benchmarks:

```bash
cargo bench --profile bench-fast -p mcp-execution-codegen -p mcp-execution-files
```

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
- Ensure Node.js is 22.18.0 or newer (23.6.0 or newer on the 23 line): `node --version`
- Run `mcp-execution-cli setup`: it also reports whether the helper binary is found; set `MCP_EXECUTION_CLI` to its path if it is not on `PATH`
- A "schema version ... regenerate" error means the directory predates this release: run `generate` again

**"Exit code 6 from `run`"**
- The script exited non-zero or was killed by a signal; read the `script_status` line and the call summary. Exit 4 is the runner's own timeout

### Getting Help

1. Check [progressive-loading-usage.md](./progressive-loading-usage.md) for detailed guide
2. Open an issue on GitHub

## Contributing Examples

Have a useful example or workflow? Contributions welcome:

1. Create a new markdown file in `examples/`
2. Follow the existing format and style
3. Add it to this README's navigation table
4. Submit a pull request

## See Also

- [Project README](../README.md) - Project overview and installation
- [MCP Specification](https://github.com/modelcontextprotocol/specification) - Protocol specification
