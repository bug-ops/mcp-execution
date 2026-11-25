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

### 3. Find Tools in the Catalog

```bash
cat ~/.claude/servers/github/catalog.ts
```

`catalog.ts` has one type-only signature per tool. Read a single tool file (`cat ~/.claude/servers/github/createIssue.ts`) only for a tool you need in full.

### 4. Call One Tool

```bash
mcp-execution-cli call --from-config github -- create_issue '{"owner":"myorg","repo":"myrepo","title":"Bug: Login button not working"}'
```

`call` takes the server flags of `generate` (`--from-config NAME`, `--http URL`, `--sse URL`, or a command with `-a`/`-e`), then `--`, the tool name and an optional JSON object. On success the result is printed as one compact JSON line (an empty success prints nothing); a tool error exits 3 and a non-object or malformed JSON argument exits 2 without echoing it.

A generated tool file runs on its own as well, one JSON object as the argument:

```bash
node ~/.claude/servers/github/createIssue.ts '{"owner":"myorg","repo":"myrepo","title":"Bug: Login button not working"}'
```

On success one JSON line goes to stdout. On failure one JSON line with `error` and `kind` goes to stderr and the exit status is 1. Outside `run` the tool file starts a hidden `mcp-execution-cli helper` process: the binary is `$MCP_EXECUTION_CLI` when set, otherwise `mcp-execution-cli` on `PATH`. A set but unusable `MCP_EXECUTION_CLI` fails with a message naming it and never falls back to `PATH`; `mcp-execution-cli setup` reports whether a helper binary is found.

### 5. Run a Script

For many calls, a join or a large result, write one script. Keep it in a working directory of your own, for example `~/scripts/`, never inside `~/.claude/servers/<id>/`: `generate` replaces that whole directory on every run, so anything saved there is deleted. Import servers as `mcp:<server-id>`:

```typescript
// ~/scripts/my-script.mts
import { createIssue } from 'mcp:github';

const issue = await createIssue({
  owner: 'myorg',
  repo: 'myrepo',
  title: 'Bug: Login button not working',
  body: 'Steps to reproduce:\n1. Navigate to /login\n2. Click login button\n3. Nothing happens'
});

console.log(JSON.stringify(issue));
```

```bash
mcp-execution-cli run ~/scripts/my-script.mts
```

See [Running Scripts](#running-scripts) for the output budget, checks and limits.

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
import { createIssue } from 'mcp:github';
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

## Running Scripts

`mcp-execution-cli run [SCRIPT|-]` runs a TypeScript script with Node.js against the generated servers. The script imports `mcp:<server-id>` for a server's tools and `mcp:helpers` for `paginate` and `mapBounded` (bounded pages, items and concurrency; each reports whether it truncated). `run -` reads the script from stdin and arguments after `--` go to the script.

```bash
mcp-execution-cli run ~/scripts/my-script.mts
mcp-execution-cli run --timeout 120 --max-output 8192 ~/scripts/my-script.mts -- first-arg
```

`run` keeps one connection per server for the whole run (up to 16 servers), so a loop of calls to one server connects once. The runner owns the connections; stdio, HTTP and SSE servers all work.

### What `run` Prints

- The first stdout line is `[mcp-execution <token>] run started`. The runner marks every line it adds with the same token, so text a script prints cannot pass for a runner line.
- Stdout of the script is cut at `--max-output` bytes (default 16384) and stderr at `--max-stderr` bytes (default 4096), at a character boundary. The whole stream is saved to a private file under the per-user state directory and a notice names the file and the cut.
- The last stdout line is `[mcp-execution <token>] script_status exit 0`; the other statuses are `exit N`, `signal N` and `timed out`.
- After a failed run a call summary lists each tool call with its state (`ok`, `tool_error`, `skipped`, or `unknown` for a call still in flight) and a 12-hex digest of its arguments, never a value or server text, followed by `completed calls may have changed state`.
- `--timeout` is 1 to 600 seconds (default 60). `--format json` prints one envelope with the token, status, exit code and both streams.

Exit codes: 0 success, 1 other error, 2 invalid input, 3 server or tool error, 4 timeout, 5 a check found problems (`drift`, `scripts check`), 6 the script exited non-zero or was killed by a signal.

### Type Check, Dry Run and Restrict

| Flag | Effect |
|------|--------|
| `--typecheck` | Runs `tsc --noEmit` over the script first; a type error exits 2 before node starts (at most 20 lines, 4 KiB shown). Skipped with a note, and the script runs, when `tsc` is missing, runs over 60 seconds, Node type definitions are not installed, or the dependencies of the generated SDK are not installed (run `npm install` in the server directory to enable the check). Real type errors in your script still exit 2. |
| `--dry-run` | Executes only the tools listed under `reads` in the policy file; every other call is skipped and rejects with `write_skipped`. The report lists the skipped writes (at most 50) and says the result is incomplete where later steps depend on them. Tool annotations never classify a call. |
| `--policy FILE` | The policy for `--dry-run`: JSON `{"reads":["github/list_issues"]}`, at most 64 KiB. Default: `mcp-execution/policy.json` in the per-user config directory. The default policy is empty, so every call counts as a write. A missing explicit file or an invalid one exits 2 without echoing its content. |
| `--restrict` | Unix only. Limits file reads to the script, the servers directory and the run directory and grants no file writes, child processes or workers, so reads of other locations fail by design. Network access is narrowed only on Node.js 25 and later. Windows refuses the flag with exit 2. |

### Privilege Level

> **Warning:** `run` is not a sandbox. The script runs with your user privileges. Read a script before running it and never run one that came from tool output. The journal, `--dry-run` and `--restrict` are diagnostics and a narrowing, not a security boundary.

Known residuals: a script cannot read its own stdin under `run`, and a raw read of file descriptor 0 breaks the run; a child process that inherits stdin and reads it can take responses meant for the script; grandchildren a script starts can outlive the run.

## Saved Scripts

A working script can be kept with the skill of its server and found again. It starts with a header block, JSON inside a comment, that is read without running anything:

```typescript
/*! mcp-script
{"v":1,"name":"open-issues","description":"List open issues of a repository","parameters":[{"name":"repo","description":"owner/name"}],"tools":["github/list_issues"],"writes":"read_only"}
*/
import { listIssues } from 'mcp:github';
// ...
```

Header fields: `v` (always 1), `name` (1 to 64 characters of `a-z`, `0-9` and `-`, equal to the file name), `description`, `parameters` (a name and a description each), `tools` (`server/tool`; the first one chooses the skill) and `writes` (`read_only` or `writes`).

```bash
# After a successful run, save to ~/.claude/skills/github/scripts/open-issues.ts
mcp-execution-cli run ~/scripts/open-issues.mts --save open-issues

# Replace an existing recipe
mcp-execution-cli run ~/scripts/open-issues.mts --save open-issues --overwrite

# Signatures only, no bodies
mcp-execution-cli scripts list --server github

# Compare saved scripts with the server's current tools; exit 5 on problems, changes nothing
mcp-execution-cli scripts check --server github
```

`--save` validates the header and name before node starts (exit 2 and no tool call when they are invalid, or the name exists without `--overwrite`) and saves only when the script exits 0. A skill holds at most 50 scripts of up to 64 KiB each. `generate` and `skill` never touch the `scripts/` directory and no command runs a saved script on its own. A custom skill name given to `skill --skill-name` must equal its directory name.

## Measuring Context Cost and Drift

```bash
mcp-execution-cli stats github
mcp-execution-cli drift --from-config github
```

`stats` reads `_meta.json` (schema version 3) and the sizes of the generated files, never their contents, and prints for four levels the bytes and the estimated tokens (bytes divided by 4, rounded up: an estimate, never a real token count): the server's own definitions as recorded at generation, all tool files, one tool file (the mean) and `catalog.ts`. It also prints how many tools declare an output schema. A figure that is missing, for example a deleted `catalog.ts`, prints `not available`, never zero. The output is identical on every run. A directory generated with an older schema exits 2 with no figures and asks for regeneration. For the in-repo fixture server (9 tools):

| Level | Bytes | Tokens (estimate) |
|-------|-------|-------------------|
| server definitions (recorded) | 1898 | 475 |
| all tool files | 5250 | 1313 |
| one tool file (mean) | 583 | 146 |
| `catalog.ts` | 1899 | 475 |

The figures are sizes of text a model reads if it reads that level in full. They do not show that a model read it, and a host that already defers tool definitions starts from a lower baseline.

The scenario harness in `crates/mcp-cli/tests/context_cost_harness.rs` runs offline against the same fixture. It compares the bytes reaching the context (command text, stdout and stderr) for K calls made one `call` per shell command against one `run` script making them:

| K | one `call` per command (bytes) | one `run` script (bytes) |
|---|--------------------------------|--------------------------|
| 1 | 68 | 275 |
| 2 | 136 | 283 |
| 3 | 204 | 291 |
| 5 | 340 | 307 |
| 10 | 680 | 348 |
| 20 | 1380 | 438 |

With this script the single `run` is smaller from K = 5. These figures do not prove more than that: the script is fixed, not written by a model, so the cost of writing it is not counted; the server is the in-repo fixture only; tokens are an estimate (bytes/4), not a real count; turn overhead and prompt caching are not modelled. The harness prints the same statement under its table. It shows where a fixed script stops costing more, not what a model session costs.

`drift` connects to the live server, compares its tool list and endpoint with the metadata recorded at generation, and writes nothing. Exit 0 means no drift and exit 5 means they differ. If you generated with `--name` or `--progressive-output`, pass the same flags to `drift`.

## Regenerate After Upgrading

The generated bridge in `_runtime/mcp-bridge.ts` is a thin client of `mcp-execution-cli`, and the metadata schema is version 3. After upgrading, run `generate` again for every server: old directories do not parse (`stats`, `skill` and `scripts check` ask for regeneration). A tool file run directly with `node` needs an `mcp-execution-cli` that matches the generated bridge, found through `MCP_EXECUTION_CLI` or `PATH`.

## Runtime Bridge

The `_runtime/mcp-bridge.ts` file is the client that tool files call. It holds no MCP connection of its own: it forwards each call over a newline-delimited JSON channel to the `mcp-execution-cli` helper, which owns every connection, the validation, the result extraction and the limits. The channel is either the one `run` registered for the script or, for a tool file run directly with `node`, a hidden `mcp-execution-cli helper` process.

### API

#### `callMCPTool(serverId, toolName, params, resultSource?)`

Calls an MCP tool and resolves its result (`undefined` for an empty success). `resultSource` is `'content'` (default) or `'structured'` for tools that declare an output schema; generated tool files choose it for you.

```typescript
import { callMCPTool } from '../.claude/servers/github/_runtime/mcp-bridge.ts';

const result = await callMCPTool('github', 'create_issue', {
  owner: 'myorg',
  repo: 'myrepo',
  title: 'Bug report',
  body: 'Description'
});
```

It throws only `McpToolError`, whose `kind` is `tool_error`, `rpc_error`, `call_failed` or `write_skipped`. What the server said is in `serverMessage` (sanitized and cut), never in `message`.

#### Removed

`closeAllConnections`, the bridge's SIGINT and SIGTERM handlers and the `MCPBRIDGE_REQUEST_TIMEOUT_MS` variable no longer exist: the helper owns the connections and ends them with the run. The time limit of a run is `run --timeout`.

### Environment Variables

| Variable | Meaning |
|----------|---------|
| `MCP_EXECUTION_CLI` | Binary to start as the helper for a tool file run directly with `node`; default `mcp-execution-cli` on `PATH` |
| `MCPBRIDGE_DEBUG` | `1` prints bridge diagnostics to stderr |
| `MCPBRIDGE_OBSERVE_SHAPES` | File that receives the shapes of successful results, for `generate --observed-shapes` |

## Examples

The scripts below live in `~/scripts/`, outside the generated directory, which `generate` replaces on every run. Run each with `mcp-execution-cli run ~/scripts/<file>.mts`. The result shapes are those of the GitHub server; check the types in `catalog.ts` for yours.

### Create GitHub Issue

```typescript
import { createIssue } from 'mcp:github';

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
import { listPullRequests } from 'mcp:github';

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
import { searchCode } from 'mcp:github';

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

### Error: helper channel closed (running a tool file with `node`)

**Solution**: A tool file run directly needs the `mcp-execution-cli` helper. Put a current `mcp-execution-cli` on `PATH` or set `MCP_EXECUTION_CLI` to its path; a set but unusable value is an error and never falls back to `PATH`. After upgrading, regenerate the server directory.

### Error: Invalid argument: ... schema version ... regenerate

**Solution**: The directory was generated by an older release. Run `mcp-execution-cli generate --from-config <SERVER>` again.

### `run --typecheck` prints a skip note instead of checking

**Solution**: The note says "Node type definitions not installed" or "dependencies of the generated SDK are not installed (run npm install in the server directory)". Run `npm install` once in the generated server directory to enable the check; until then the script runs unchecked.

### Exit code 6 from `run`

**Solution**: The script exited non-zero or was killed by a signal; the `script_status` line and the call summary say which. Exit 4 is the runner's own timeout.

## Performance

Generation and export timings come from the Criterion benchmarks:

```bash
cargo bench --profile bench-fast -p mcp-execution-codegen -p mcp-execution-files
```

Under `run`, repeated calls in one script reuse one connection per server for the whole run. A tool file started directly with `node` starts its own helper, so it connects again on every start. For context cost, see [Measuring Context Cost and Drift](#measuring-context-cost-and-drift).

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
- `--from-config <NAME>` - Load the server from `~/.claude/mcp.json`
- `--dry-run` - Preview the files without writing them

### Other Commands

| Command | Purpose |
|---------|---------|
| `call <server flags> -- TOOL [JSON]` | Call one tool and print the result within the output budget |
| `run [SCRIPT\|-] [flags] [-- ARGS]` | Run a script with bounded output (`--timeout`, `--max-output`, `--max-stderr`, `--typecheck`, `--dry-run`, `--policy`, `--restrict`, `--save`, `--overwrite`) |
| `scripts list\|check --server ID` | List saved scripts by signature; check them against the live tools (exit 5 on problems) |
| `stats SERVER_ID` | Bytes and estimated tokens per level of a generated server |
| `drift <server flags>` | Compare the live tool list with the recorded one (exit 5 on drift; `--name`, `--progressive-output` as for `generate`) |
| `introspect`, `skill`, `server`, `setup`, `completions` | Inspect a server, write a skill, list configured servers, check the environment, shell completions |

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
│ Runtime Bridge      │  Thin client, Node.js built-ins only
│ (mcp-bridge.ts)     │  Newline-delimited JSON to the helper
└──────────┬──────────┘
           │
           ↓
┌─────────────────────┐
│ mcp-execution-cli   │  `run` channel or hidden `helper`
│ (helper)            │  Connections, validation, limits
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
