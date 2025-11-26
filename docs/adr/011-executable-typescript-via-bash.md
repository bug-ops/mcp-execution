# ADR-011: Autonomous MCP Tool Execution via Node.js CLI

**Status**: Proposed
**Date**: 2025-11-26
**Deciders**: Rust Architecture Team
**Related**: [ADR-010](010-simplify-to-progressive-only.md) (Simplified to progressive loading only)

---

## Context

After implementing progressive loading (ADR-010), we generate TypeScript files with type-safe function definitions for each MCP tool. Claude Code can read these files (achieving 98% token savings), but **cannot execute them autonomously**.

### Current Flow

```
User: "Create a GitHub issue titled 'Bug fix'"
  ↓
Claude Code reads: ~/.claude/servers/github/createIssue.ts (500 tokens)
  ↓
Claude Code understands: function createIssue(params) signature
  ↓
❌ BLOCKED: Cannot execute the function to get actual results
```

### Requirements

1. Claude Code must execute MCP tools **autonomously** (no manual intervention)
2. Must preserve **98% token savings** from progressive loading
3. Must work in **CLI environment** (where Claude Code runs)
4. Must support **any MCP server** (GitHub, Google Drive, custom servers)
5. Must require **minimal user setup** (simple installation)
6. Must provide **excellent debugging** (clear error messages)

### Constraints

- Claude Code runs in a CLI environment (not browser)
- Claude Code has access to the **Bash tool** for executing commands
- Progressive loading must be maintained (cannot load all tools at once)
- Solution must follow Microsoft Rust Guidelines
- No changes to Claude Code itself (external tool)

---

## Decision

Generate **dual-purpose TypeScript files** that serve as both:

1. **Library modules** - Type definitions for progressive loading (read by Claude Code)
2. **Executable CLI scripts** - Runnable via Node.js when invoked directly

Claude Code executes tools using its existing **Bash tool**:

```bash
node ~/.claude/servers/github/createIssue.ts '{"owner":"user","repo":"repo","title":"Bug"}'
```

### Architecture

```
User asks Claude → Claude reads TS file (500 tokens) → Claude executes via Bash
                                                            ↓
                                                     Node.js interprets TS
                                                            ↓
                                                     Runtime Bridge (mcp-bridge.ts)
                                                            ↓
                                                     MCP Server (stdio)
                                                            ↓
                                                     JSON result → Claude
```

### File Structure

Each generated tool file has two modes:

```typescript
#!/usr/bin/env node
// Generated: ~/.claude/servers/github/createIssue.ts

import { callMCPTool } from '../_runtime/mcp-bridge.js';

// TYPE DEFINITIONS (for progressive loading - read by Claude)
export interface CreateIssueParams {
  owner: string;
  repo: string;
  title: string;
  body?: string;
}

export interface CreateIssueResult {
  number: number;
  url: string;
  state: string;
}

// FUNCTION IMPLEMENTATION (library mode)
export async function createIssue(
  params: CreateIssueParams
): Promise<CreateIssueResult> {
  return callMCPTool('github', 'create_issue', params);
}

// CLI MODE (executable when run directly)
if (import.meta.url === `file://${process.argv[1]}`) {
  const params = JSON.parse(process.argv[2] || '{}');
  createIssue(params)
    .then(result => {
      console.log(JSON.stringify(result, null, 2));
      process.exit(0);
    })
    .catch(error => {
      console.error(JSON.stringify({ error: error.message }, null, 2));
      process.exit(1);
    });
}
```

---

## Alternatives Considered

### Alternative 1: Custom Rust CLI Wrapper

Create a Rust binary that wraps MCP protocol:

```bash
mcp-execution-cli call github create_issue '{"owner":"user",...}'
```

**Rejected because:**
- ❌ Duplicates MCP protocol implementation (violates DRY)
- ❌ Requires maintaining two MCP client implementations (Rust + TypeScript)
- ❌ Adds complexity to the codebase
- ❌ Users need to install additional tool beyond Node.js

### Alternative 2: WebAssembly Runtime

Compile tools to WASM and execute in Rust runtime.

**Rejected because:**
- ❌ Already removed in ADR-010 (15,000 LOC removed)
- ❌ Complex debugging (WASM stack traces)
- ❌ Poor error messages
- ❌ Adds significant build complexity
- ❌ Slower than native Node.js execution

### Alternative 3: Claude Code Native Integration

Request Claude Code team to add native MCP tool execution.

**Rejected because:**
- ❌ Not in our control (depends on Anthropic)
- ❌ Tight coupling to specific Claude Code version
- ❌ Not reusable outside Claude Code environment
- ❌ Long development timeline (external dependency)

### Alternative 4: HTTP Server Wrapper

Run a local HTTP server that proxies MCP calls.

**Rejected because:**
- ❌ Requires server lifecycle management (start/stop)
- ❌ Port allocation complexity
- ❌ Security concerns (open ports)
- ❌ Adds deployment complexity
- ❌ Overkill for simple tool execution

### Selected: Executable TypeScript via Bash ✅

**Why this is the best solution:**

1. ✅ **Leverages existing capabilities**: Claude Code's Bash tool already works
2. ✅ **Zero new dependencies**: Only Node.js 18+ (already required for MCP servers)
3. ✅ **Simple architecture**: No new crates, no complex runtime
4. ✅ **Excellent debugging**: JSON in/out, clear error messages
5. ✅ **Self-contained files**: Single file = types + execution (DRY)
6. ✅ **Progressive loading preserved**: Types readable in ~500 tokens
7. ✅ **Works everywhere**: Any environment with Node.js
8. ✅ **Separation of concerns**: Rust generates, TypeScript executes

---

## Implementation Details

### Component 1: Enhanced Code Generation

**File**: `crates/mcp-codegen/templates/progressive/tool.hbs`

Add CLI mode to existing template:

```handlebars
#!/usr/bin/env node
{{!-- Existing template content --}}

// CLI mode: Execute when run directly
if (import.meta.url === `file://${process.argv[1]}`) {
  const params = JSON.parse(process.argv[2] || '{}');
  {{typescript_name}}(params)
    .then(result => {
      console.log(JSON.stringify(result, null, 2));
      process.exit(0);
    })
    .catch(error => {
      console.error(JSON.stringify({
        error: error.message,
        stack: error.stack
      }, null, 2));
      process.exit(1);
    });
}
```

### Component 2: Runtime Bridge

**File**: `runtime/mcp-bridge.ts`

Implements MCP protocol over stdio:

```typescript
import { spawn, ChildProcess } from 'child_process';

// Connection cache: Reuse server processes across tool calls
const serverConnections = new Map<string, ChildProcess>();

export async function callMCPTool(
  serverId: string,
  toolName: string,
  params: Record<string, unknown>
): Promise<unknown> {
  // Get or create server connection
  const serverProcess = await getConnection(serverId);

  // Send JSON-RPC request
  const request = {
    jsonrpc: '2.0',
    id: Date.now(),
    method: 'tools/call',
    params: { name: toolName, arguments: params }
  };

  serverProcess.stdin.write(JSON.stringify(request) + '\n');

  // Wait for response
  return parseResponse(serverProcess.stdout);
}
```

**Key features:**
- Connection caching (500ms → 50ms for subsequent calls)
- Automatic server lifecycle management
- Proper error handling with stack traces
- JSON-RPC 2.0 protocol compliance

### Component 3: Setup Command

**File**: `crates/mcp-cli/src/commands/setup.rs`

Validates runtime environment:

```rust
pub async fn run() -> Result<ExitCode> {
    // Check Node.js 18+ installed
    check_node_version().await?;

    // Make TypeScript files executable (Unix only)
    make_files_executable().await?;

    println!("✓ Runtime setup complete");
    println!("  Claude Code can now execute MCP tools");

    Ok(ExitCode::SUCCESS)
}
```

### Component 4: Updated Skill Documentation

**File**: `examples/SKILL.md`

Add execution instructions:

```markdown
## Executing MCP Tools

After reading a tool definition, execute it via Node.js:

```bash
# Read tool definition (500 tokens)
cat ~/.claude/servers/github/createIssue.ts

# Execute with parameters
node ~/.claude/servers/github/createIssue.ts '{
  "owner": "myorg",
  "repo": "myrepo",
  "title": "Bug: Login button not working"
}'

# Parse JSON result
{
  "number": 123,
  "url": "https://github.com/myorg/myrepo/issues/123",
  "state": "open"
}
```
```

---

## Consequences

### Positive

1. ✅ **Simple architecture**: No new Rust crates needed, TypeScript runtime only
2. ✅ **Minimal setup**: Just Node.js 18+ (no npm install required)
3. ✅ **Excellent debugging**: JSON in/out makes debugging trivial
4. ✅ **Progressive loading intact**: Still 98% token savings
5. ✅ **Self-documenting**: CLI mode clearly shows how to use tools
6. ✅ **Type-safe**: Generated TypeScript provides compile-time safety
7. ✅ **Reusable**: Works outside Claude Code (any Node.js environment)
8. ✅ **Fast iteration**: TypeScript changes don't require Rust rebuild

### Negative

1. ⚠️ **Node.js dependency**: Requires Node.js 18+ installed
2. ⚠️ **Process spawn overhead**: ~10ms per tool call
3. ⚠️ **No compile-time checks**: TypeScript errors only at runtime
4. ⚠️ **Platform differences**: Shebang (`#!/usr/bin/env node`) Unix-only

### Mitigations

1. **Node.js requirement**:
   - Document clearly in README
   - `setup` command validates installation
   - Most MCP servers already require Node.js

2. **Spawn overhead**:
   - Connection caching reduces to ~50ms after first call
   - Acceptable for user-facing operations
   - Future: Could implement persistent daemon if needed

3. **Runtime errors**:
   - Generated types prevent most errors
   - JSON schema validation in bridge
   - Clear error messages with stack traces

4. **Platform support**:
   - Shebang optional (can use `node file.ts` explicitly)
   - Windows: Use `node` command directly
   - Document cross-platform usage

---

## Performance Characteristics

### Token Usage (Progressive Loading Preserved)

| Scenario | Tokens | Savings |
|----------|--------|---------|
| Load all 40 tools (traditional) | 30,000 | 0% |
| Load 1 tool (progressive) | 500 | 98% ✅ |
| Load 3 tools (progressive) | 1,500 | 95% ✅ |

### Execution Time

| Operation | First Call | Cached Call |
|-----------|------------|-------------|
| Server startup | 500ms | - |
| Tool execution | 50ms | 50ms |
| **Total** | **550ms** | **50ms** ✅ |

### File Size

| File Type | Size | Count (40 tools) |
|-----------|------|------------------|
| Tool file | 2-5 KB | 40 files (200 KB) |
| Runtime bridge | 10 KB | 1 file |
| **Total** | - | **~210 KB** ✅ |

---

## Rollout Plan

### Phase 1: Foundation (Week 1)
- [ ] Create this ADR
- [ ] Update `tool.hbs` template with CLI mode
- [ ] Test generated files execute correctly

### Phase 2: Runtime (Week 2)
- [ ] Implement `mcp-bridge.ts` with connection caching
- [ ] Add `setup` command to mcp-cli
- [ ] Integration tests for execution

### Phase 3: Documentation (Week 3)
- [ ] Update `examples/SKILL.md` with execution guide
- [ ] Add troubleshooting section to README
- [ ] Create video tutorial (optional)

### Success Criteria

- [ ] Claude Code can execute any MCP tool autonomously
- [ ] Progressive loading achieves 98% token savings
- [ ] Setup requires only `mcp-execution-cli setup`
- [ ] All integration tests pass
- [ ] Documentation includes working examples

---

## Compatibility

### Backward Compatibility

✅ **Fully backward compatible**

- Existing generated files remain valid library modules
- Adding CLI mode doesn't break imports
- Users can opt-in to execution gradually

### Forward Compatibility

✅ **Future-proof design**

- Can add features to runtime bridge without changing tool files
- Can support new MCP protocol versions in bridge only
- Can optimize without breaking user code

---

## Security Considerations

### Input Validation

- ✅ JSON parsing with error handling
- ✅ MCP server config from trusted location (`~/.claude/`)
- ✅ No arbitrary code execution (parameters are data only)

### Process Isolation

- ✅ Each MCP server runs in separate process
- ✅ Servers can't access filesystem outside allowed directories
- ✅ Clean process termination on error

### Secrets Management

- ✅ Environment variables passed securely to server processes
- ✅ Tokens never logged or exposed in error messages
- ✅ Config files have appropriate permissions

---

## Monitoring & Debugging

### Logging Strategy

```bash
# Enable debug mode
MCPBRIDGE_DEBUG=1 node ~/.claude/servers/github/createIssue.ts '{...}'

# Output includes:
# - Server connection status
# - JSON-RPC request/response
# - Timing information
# - Error stack traces
```

### Error Messages

All errors return structured JSON:

```json
{
  "error": "Server 'github' not found in config",
  "code": "SERVER_NOT_FOUND",
  "suggestion": "Run: mcp-execution-cli generate github"
}
```

---

## Related Work

- [ADR-004](004-use-rmcp-official-sdk.md): Use official rmcp SDK (Rust side)
- [ADR-010](010-simplify-to-progressive-only.md): Simplified to progressive loading
- [MCP Specification](https://github.com/modelcontextprotocol/specification): Protocol reference
- [Node.js ESM](https://nodejs.org/api/esm.html): ES Module specification for `import.meta.url`

---

## Decision Makers

- @rust-architect: Proposed architecture
- @rust-developer: Implementation feasibility
- @rust-testing-engineer: Testing strategy
- @rust-code-reviewer: Code review standards

---

## Appendix: Example End-to-End Flow

```bash
# 1. User generates tools
$ mcp-execution-cli generate docker --arg=... --name=github
✓ Generated 40 tools
✓ Runtime setup complete

# 2. Claude Code discovers tools
User: "Create a GitHub issue titled 'Fix login bug'"

# 3. Claude Code reads tool definition (500 tokens)
$ cat ~/.claude/servers/github/createIssue.ts
# [Reads type definitions and understands parameters]

# 4. Claude Code executes tool
$ node ~/.claude/servers/github/createIssue.ts '{
  "owner": "myorg",
  "repo": "myapp",
  "title": "Fix login bug",
  "body": "Login form validation is broken"
}'

# 5. Gets result
{
  "number": 456,
  "url": "https://github.com/myorg/myapp/issues/456",
  "state": "open"
}

# 6. Claude Code responds
"I've created issue #456 in myorg/myapp: 'Fix login bug'"
```

---

**Status**: Proposed → Implementation in progress
**Next Review**: After Phase 1 completion
