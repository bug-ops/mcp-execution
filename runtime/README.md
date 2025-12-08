# MCP Execution Runtime

Runtime bridge for autonomous MCP tool execution via Node.js.

## Overview

This module provides the bridge between generated TypeScript tool definitions and MCP servers. It handles:

- Server connection management with caching
- JSON-RPC 2.0 protocol communication
- Error handling and reporting
- Process lifecycle management

## Requirements

- Node.js 18.0.0 or higher
- No external dependencies (uses Node.js built-ins only)

## Usage

This module is automatically used by generated tool files. When you generate tools via:

```bash
mcp-execution-cli generate github
```

Each tool file imports `callMCPTool` from this module:

```typescript
import { callMCPTool } from '../_runtime/mcp-bridge.js';

export async function createIssue(params) {
  return callMCPTool('github', 'create_issue', params);
}
```

## Autonomous Execution

Tool files can be executed directly via Node.js:

```bash
node ~/.claude/servers/github/createIssue.ts '{"owner":"user","repo":"repo","title":"Bug"}'
```

This enables Claude Code to execute MCP tools autonomously using its Bash tool.

## API

### `callMCPTool(serverId, toolName, params)`

Execute an MCP tool on a server.

**Parameters:**
- `serverId` (string): Server identifier (e.g., "github")
- `toolName` (string): Tool name as defined by MCP server
- `params` (object): Tool parameters

**Returns:** Promise resolving to tool execution result

**Throws:** Error if tool execution fails

**Example:**
```typescript
const result = await callMCPTool('github', 'create_issue', {
  owner: 'myorg',
  repo: 'myrepo',
  title: 'Bug report'
});
```

### `closeAllConnections()`

Close all active server connections.

Call during graceful shutdown to clean up processes. This is automatically called on process exit.

**Example:**
```typescript
process.on('SIGINT', async () => {
  await closeAllConnections();
  process.exit(0);
});
```

## Configuration

Server configurations are loaded from `~/.claude/mcp.json`:

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
        "GITHUB_PERSONAL_ACCESS_TOKEN": "github_pat_YOUR_TOKEN"
      }
    }
  }
}
```

See `examples/mcp.json.example` in the main repository for more examples.

## Connection Caching

The runtime automatically caches server connections:

- **First call**: 500ms (server startup + execution)
- **Subsequent calls**: 50ms (execution only)

This provides 10x performance improvement for repeated tool calls to the same server.

## Debug Mode

Enable debug logging:

```bash
MCPBRIDGE_DEBUG=1 node tool.ts '{"param":"value"}'
```

Debug output includes:
- Connection status
- JSON-RPC requests/responses
- Timing information
- Error stack traces

## Error Handling

All errors are returned as JSON:

```json
{
  "error": "Tool execution failed: Invalid parameters",
  "stack": "Error: Tool execution failed..."
}
```

This makes errors easy to parse and debug.

## Architecture

```
Generated Tool → callMCPTool() → Server Connection Cache
                                         ↓
                                  JSON-RPC Request
                                         ↓
                                  MCP Server Process
                                         ↓
                                  JSON-RPC Response
                                         ↓
                                  Parse & Return
```

## Performance

- **Connection caching**: Reuses processes across calls
- **Process spawning**: ~10ms overhead
- **Cached calls**: ~50ms total execution time
- **Memory**: <10MB per server process

## Security

- Server configs from trusted location (`~/.claude/`)
- Environment variables passed securely
- No arbitrary code execution
- Process isolation per server
- Clean termination on error

## Troubleshooting

### "MCP configuration file not found"

Create `~/.claude/mcp.json` with server configurations.

### "Server 'xyz' not found in config"

Add the server to your `mcp.json` under the `mcpServers` key.

### "Server initialization failed"

Check that:
- Server command is correct
- Server is executable
- Environment variables are set
- Node.js 18+ is installed

## See Also

- [Progressive Loading Usage Guide](../examples/progressive-loading-usage.md)
