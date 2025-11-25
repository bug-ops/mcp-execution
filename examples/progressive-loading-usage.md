# Progressive Loading Usage Example

This example demonstrates how to use progressive loading to achieve 98% token savings when working with MCP servers in Claude Code.

## Prerequisites

- `mcp-cli` installed (`cargo install --path crates/mcp-cli`)
- An MCP server (e.g., `github-mcp-server`)
- Required environment variables (e.g., `GITHUB_TOKEN` for GitHub server)

## Step 1: Generate TypeScript Files for an MCP Server

Generate progressive loading files for the GitHub MCP server:

```bash
mcp-cli generate github-mcp-server \
  --env GITHUB_TOKEN=ghp_your_token_here
```

**Output:**

```
✓ Introspecting MCP server 'github-mcp-server'...
✓ Discovered 45 tools
✓ Generating progressive loading code...
✓ Exported 47 files to ~/.claude/servers/github/

Server: GitHub MCP Server
Tools: 45
Output: /Users/username/.claude/servers/github
```

**Files generated:**

- 45 tool files (e.g., `createIssue.ts`, `updateIssue.ts`, ...)
- 1 index file (`index.ts`)
- 1 runtime bridge (`_runtime/mcp-bridge.ts`)
- **Total**: 47 files

## Step 2: Claude Code Discovers Available MCP Servers

Claude Code can now discover what MCP servers have been generated:

```bash
$ ls ~/.claude/servers/
```

**Output:**

```
github/
```

If you generate more servers, you'll see them all:

```bash
$ mcp-cli generate slack-mcp-server --env SLACK_TOKEN=xoxb-...
$ mcp-cli generate google-drive-mcp-server --env GOOGLE_CREDENTIALS=...

$ ls ~/.claude/servers/
```

**Output:**

```
github/
google-drive/
slack/
```

## Step 3: Discover Tools in a Specific Server

List all tools available in the GitHub server:

```bash
$ ls ~/.claude/servers/github/
```

**Output:**

```
addComment.ts
createIssue.ts
createPullRequest.ts
deleteFile.ts
getFile.ts
getIssue.ts
getPullRequest.ts
listBranches.ts
listCommits.ts
listIssues.ts
listPullRequests.ts
searchCode.ts
searchIssues.ts
searchRepositories.ts
updateIssue.ts
updatePullRequest.ts
... (29 more files)
_runtime/
index.ts
```

## Step 4: Progressive Loading - Load Only What You Need

This is where token savings happen!

### Traditional Approach (Load Everything)

```bash
# ❌ Loading entire server definition
$ cat ~/.claude/servers/github/index.ts

# Result: ~30,000 tokens for all 45 tools
```

### Progressive Loading (Load One Tool)

```bash
# ✅ Load only the createIssue tool
$ cat ~/.claude/servers/github/createIssue.ts

# Result: ~500-1,500 tokens
# Savings: 98%! 🎉
```

## Step 5: Understanding Tool Structure

When you load a single tool file, you see its complete API:

```bash
$ cat ~/.claude/servers/github/createIssue.ts
```

**Output:**

```typescript
import { callMCPTool } from './_runtime/mcp-bridge.js';

/**
 * Creates a new issue in a GitHub repository
 *
 * @param params - Tool parameters
 * @returns Tool execution result
 * @throws {Error} If tool execution fails
 */
export async function createIssue(
  params: CreateIssueParams
): Promise<CreateIssueResult> {
  return await callMCPTool('github', 'create_issue', params);
}

/**
 * Parameters for createIssue tool.
 */
export interface CreateIssueParams {
  /**
   * Repository in format "owner/repo"
   */
  repo: string;

  /**
   * Issue title
   */
  title: string;

  /**
   * Issue body
   */
  body?: string;

  /**
   * Labels to apply
   */
  labels?: string[];

  /**
   * Assignees
   */
  assignees?: string[];
}

/**
 * Result type for createIssue tool.
 *
 * The structure of the result depends on the specific tool implementation.
 * Refer to the MCP server documentation for details.
 */
export interface CreateIssueResult {
  [key: string]: unknown;
}
```

### What Claude Code Learns

From this TypeScript file, Claude Code can see:

1. **Function signature**: `createIssue(params: CreateIssueParams): Promise<CreateIssueResult>`
2. **Required parameters**: `repo` and `title` (no `?` mark)
3. **Optional parameters**: `body`, `labels`, `assignees` (has `?` mark)
4. **Parameter types**: All clearly typed (`string`, `string[]`, etc.)
5. **Documentation**: JSDoc comments explain each parameter

## Step 6: Loading Multiple Tools (Still Efficient)

If Claude Code needs multiple tools, it loads them individually:

```bash
# Load createIssue
$ cat ~/.claude/servers/github/createIssue.ts
# ~500 tokens

# Load updateIssue
$ cat ~/.claude/servers/github/updateIssue.ts
# ~600 tokens

# Load getIssue
$ cat ~/.claude/servers/github/getIssue.ts
# ~400 tokens

# Total: ~1,500 tokens for 3 tools
# vs ~30,000 tokens for all 45 tools
# Savings: 95% even when loading multiple tools!
```

## Token Savings Analysis

### Scenario: Creating a GitHub Issue

**Traditional approach** (load entire server):

```
Load all 45 tools: ~30,000 tokens
Use 1 tool: 0 additional tokens
Total: 30,000 tokens
```

**Progressive loading**:

```
Load 1 tool (createIssue): ~500 tokens
Use 1 tool: 0 additional tokens
Total: 500 tokens
Savings: 98.3%! 🎉
```

### Scenario: Working with Issues (5 tools)

**Traditional approach**:

```
Load all 45 tools: ~30,000 tokens
Use 5 tools: 0 additional tokens
Total: 30,000 tokens
```

**Progressive loading**:

```
Load createIssue: ~500 tokens
Load updateIssue: ~600 tokens
Load getIssue: ~400 tokens
Load listIssues: ~700 tokens
Load addComment: ~450 tokens
Total: 2,650 tokens
Savings: 91.2%! 🎉
```

## Current Limitations

### callMCPTool() Is Not Yet Implemented

The generated TypeScript shows the API structure, but the `callMCPTool()` function in `_runtime/mcp-bridge.ts` is currently a stub:

```typescript
// Current implementation (stub)
export async function callMCPTool(
  serverId: string,
  toolName: string,
  params: unknown
): Promise<unknown> {
  throw new Error(
    `callMCPTool not yet implemented. ` +
    `Attempted to call ${serverId}:${toolName} with params: ${JSON.stringify(params)}`
  );
}
```

### Workarounds

Until `callMCPTool()` is implemented (planned for Phase 2.3):

**Option 1: Use MCP Server Directly**

```bash
# Call GitHub MCP server directly using its native protocol
# (requires MCP protocol knowledge)
```

**Option 2: Use for Discovery Only**

```bash
# Use progressive loading to discover and understand tools
# Then manually construct MCP requests based on TypeScript interfaces
```

**Option 3: Wait for Phase 2.3**

Implementation of `mcp-cli bridge` command will make `callMCPTool()` functional:

```bash
# Future: mcp-cli bridge will execute MCP calls
mcp-cli bridge call github create_issue '{"repo":"owner/repo","title":"Bug report"}'
```

## Advanced Usage

### Custom Output Directory

Generate tools to a custom location:

```bash
mcp-cli generate github-mcp-server \
  --progressive-output /custom/path/github \
  --env GITHUB_TOKEN=ghp_xxx
```

### HTTP/SSE Transport

For MCP servers using HTTP or SSE:

```bash
# HTTP transport
mcp-cli generate --http https://api.example.com/mcp \
  --header "Authorization=Bearer token"

# SSE transport
mcp-cli generate --sse https://api.example.com/mcp/events \
  --header "Authorization=Bearer token"
```

### Docker-Based MCP Servers

For MCP servers running in Docker:

```bash
mcp-cli generate docker \
  --arg run --arg -i --arg --rm \
  --arg ghcr.io/org/mcp-server \
  --env API_KEY=xxx
```

### JSON Output (for Scripting)

```bash
mcp-cli generate github-mcp-server \
  --env GITHUB_TOKEN=ghp_xxx \
  --format json
```

**Output:**

```json
{
  "server_id": "github",
  "server_name": "GitHub MCP Server",
  "tool_count": 45,
  "output_path": "/Users/username/.claude/servers/github"
}
```

## Best Practices

### 1. Generate Once, Use Many Times

```bash
# Generate tools once
mcp-cli generate github-mcp-server --env GITHUB_TOKEN=ghp_xxx

# Use them in many Claude Code sessions
# No need to regenerate unless server changes
```

### 2. Organize by Server

Keep different servers in separate directories:

```
~/.claude/servers/
├── github/        # GitHub tools
├── slack/         # Slack tools
└── google-drive/  # Google Drive tools
```

### 3. Load Tools Progressively

```bash
# ❌ Don't load index.ts (loads all tools)
cat ~/.claude/servers/github/index.ts

# ✅ Load individual tools as needed
cat ~/.claude/servers/github/createIssue.ts
cat ~/.claude/servers/github/updateIssue.ts
```

### 4. Use TypeScript Interfaces

The generated interfaces show exactly what parameters are required:

```typescript
export interface CreateIssueParams {
  repo: string;        // Required (no ?)
  title: string;       // Required (no ?)
  body?: string;       // Optional (has ?)
  labels?: string[];   // Optional (has ?)
}
```

## Summary

Progressive loading provides:

- ✅ **98% token savings** vs traditional all-in-one approach
- ✅ **Type-safe interfaces** for all MCP tools
- ✅ **On-demand loading** - load only what you need
- ✅ **Simple discovery** - `ls` and `cat` to explore tools
- ✅ **Full documentation** - JSDoc comments in every file

Current limitation:

- ⚠️ **Execution not yet implemented** - waiting for Phase 2.3 (`mcp-cli bridge`)

For now, use progressive loading for **tool discovery and understanding**. Execution will be added in future releases.

## Next Steps

1. Generate tools for your favorite MCP servers
2. Explore the TypeScript files to understand available tools
3. Achieve 98% token savings through progressive loading
4. Watch for Phase 2.3 updates to enable full execution

## Related Documentation

- **CLI Reference**: See `mcp-cli --help`
- **SKILL.md**: `~/.claude/skills/mcp-progressive-loading/SKILL.md`
- **ADR-010**: Decision to focus on progressive loading only
- **Project README**: `/path/to/mcp-execution/README.md`
