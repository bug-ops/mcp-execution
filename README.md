# MCP Code Execution

[![Crates.io](https://img.shields.io/crates/v/mcp-execution-cli.svg)](https://crates.io/crates/mcp-execution-cli)
[![docs.rs](https://img.shields.io/docsrs/mcp-execution-core)](https://docs.rs/mcp-execution-core)
[![CI](https://github.com/bug-ops/mcp-execution/actions/workflows/ci.yml/badge.svg)](https://github.com/bug-ops/mcp-execution/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/bug-ops/mcp-execution/graph/badge.svg?token=2UEW36O9AN)](https://codecov.io/gh/bug-ops/mcp-execution)
[![MSRV](https://img.shields.io/badge/MSRV-1.98-blue.svg)](https://blog.rust-lang.org/2025/01/23/Rust-1.91.0.html)
[![License](https://img.shields.io/badge/license-MIT%20OR%20Apache--2.0-blue.svg)](LICENSE-MIT)

Turn any MCP server into a typed TypeScript SDK that a model programs against. Loops, joins and large results stay in code, and only the answer reaches the model's context.

> [!TIP]
> Inspired by [Anthropic's engineering blog post](https://www.anthropic.com/engineering/code-execution-with-mcp) on Code Execution with MCP.

## Why Script Against MCP Tools

Calling MCP tools one at a time has costs besides loading tool definitions (which many hosts now defer on their own):

- **Intermediate results** pass through the model's context even when only a filtered or joined answer is needed.
- **One model round trip per call**, including calls whose arguments depend on nothing the model needs to see.

`mcp-execution-cli generate` introspects a server and writes one TypeScript file per tool, a type-only `catalog.ts` with one signature per tool, and a small runtime bridge. A model reads the signatures it needs, writes a script that calls the tools, and reads only what the script prints.

Only text a model reads costs context. Importing a module when a script runs puts nothing into the context.

### When a Script Pays Off

| Task | Better choice |
|------|---------------|
| Single lookup, or one or two sequential calls | Direct tool call: nothing to combine |
| Small server catalog | Direct tool call: the definitions are cheap to hold |
| Many calls over a list, pagination, joins across tools | Script: the loop runs in code and one answer comes back |
| Large results that are filtered or aggregated before use | Script: the full result never enters the context |

### Relation to a Host's Code Execution Mode

Some hosts offer their own code execution mode for MCP tools. This project does not replace it: it produces the typed SDK (parameter and result types, a catalog and a bridge to the server) that a script, run by any means you have, imports. The generated files run with plain Node.js; no host feature is required.

### Measured Context Cost

`mcp-execution-cli stats <server-id>` prints, from a generated directory, the size in bytes and the estimated tokens (bytes divided by 4, always labelled an estimate) of each level a model can read: the server's own definitions as recorded at generation, all tool files, one tool file and `catalog.ts`. For the in-repo fixture server (9 tools):

| Level | Bytes | Tokens (estimate) |
|-------|-------|-------------------|
| server definitions (recorded) | 1898 | 475 |
| all tool files | 5250 | 1313 |
| one tool file (mean) | 583 | 146 |
| `catalog.ts` | 1899 | 475 |

These are sizes of text a model reads if it reads that level in full. They do not show that a model did, they are not real token counts, and a host that already defers tool definitions starts from a lower baseline.

The scenario harness (`crates/mcp-cli/tests/context_cost_harness.rs`, offline, part of `cargo nextest run --all-features -p mcp-execution-cli`) compares the bytes that reach the context when K calls are made one `call` per shell command and when one `run` script makes them, against the same fixture. With a fixed script, the single script is the smaller of the two from K = 5 (for K = 1, 2 and 3 one call per command is smaller; at K = 10 it is 680 against 348 bytes, at K = 20 it is 1380 against 438). The figures prove only these scenarios. They do not prove more than that: the script is fixed, not written by a model, so the cost of writing it is not counted; the server is the in-repo fixture only; tokens are an estimate (bytes/4), not a real count; turn overhead and prompt caching are not modelled.

## Installation

### From crates.io (recommended)

```bash
cargo install mcp-execution-cli
```

<details>
<summary><h3>Pre-built binaries</h3></summary>

Download from [GitHub Releases](https://github.com/bug-ops/mcp-execution/releases/latest):

```bash
# macOS (Apple Silicon)
curl -L https://github.com/bug-ops/mcp-execution/releases/latest/download/mcp-execution-cli-macos-arm64.tar.gz | tar xz

# macOS (Intel)
curl -L https://github.com/bug-ops/mcp-execution/releases/latest/download/mcp-execution-cli-macos-amd64.tar.gz | tar xz

# Linux (x86_64)
curl -L https://github.com/bug-ops/mcp-execution/releases/latest/download/mcp-execution-cli-linux-amd64.tar.gz | tar xz

# Linux (ARM64)
curl -L https://github.com/bug-ops/mcp-execution/releases/latest/download/mcp-execution-cli-linux-arm64.tar.gz | tar xz

# Windows (x86_64)
# Download mcp-execution-cli-windows-amd64.zip from releases page
```

</details>

<details>
<summary><h3>From source</h3></summary>

```bash
git clone https://github.com/bug-ops/mcp-execution
cd mcp-execution
cargo install --path crates/mcp-cli
```

</details>

<details>
<summary><h3>As a library</h3></summary>

Add individual crates to your project:

```bash
cargo add mcp-execution-core          # Foundation types
cargo add mcp-execution-introspector  # MCP server analysis
cargo add mcp-execution-codegen       # TypeScript generation
cargo add mcp-execution-files         # Virtual filesystem
cargo add mcp-execution-skill         # SKILL.md generation
```

</details>

> [!IMPORTANT]
> Requires Rust 1.98 or later for building from source.

## Usage

### Generate TypeScript Tools

```bash
# 1. Configure MCP server in ~/.claude/mcp.json
# 2. Generate tools
mcp-execution-cli generate --from-config github

# Output: ~/.claude/servers/github/
#   - createIssue.ts
#   - updateIssue.ts
#   - ... (one file per tool)
```

### Find Tools in the Catalog

```bash
# Read the compact signatures of every tool
cat ~/.claude/servers/github/catalog.ts

# Read one tool in full (types and JSDoc)
cat ~/.claude/servers/github/createIssue.ts
```

`catalog.ts` is the cheap level to read first: one type-only signature per tool. Open a tool file only for a tool you need in full.

### Call One Tool

```bash
# The tool name and an optional JSON object follow `--`
mcp-execution-cli call --from-config github -- create_issue '{"owner":"myorg","repo":"myrepo","title":"Bug"}'
```

`call` accepts the same server flags as `generate` (`--from-config`, `--http`, `--sse` or a command) and prints the result as one compact JSON line within the output budget. An empty success prints nothing. A tool error exits 3, a non-object or malformed JSON argument exits 2 without echoing it.

A generated tool file also runs directly with Node.js, one JSON object as the argument:

```bash
node ~/.claude/servers/github/createIssue.ts '{"owner":"myorg","repo":"myrepo","title":"Bug"}'
```

Outside `run` this starts a hidden `mcp-execution-cli helper` process. The binary is `$MCP_EXECUTION_CLI` when set, otherwise `mcp-execution-cli` on `PATH`; a set but unusable `MCP_EXECUTION_CLI` is an error and never falls back to `PATH`.

> [!NOTE]
> Generated files are TypeScript that Node.js runs directly. This needs Node.js 22.18.0 or newer (on the 23 line, 23.6.0 or newer), or Bun or Deno (see [Supported Runtimes](#supported-runtimes)); `mcp-execution-cli setup` checks for one, and reports whether the helper binary is found.

### Run a Script

For many calls, a join or a large result, write one script and run it. A script imports servers as `mcp:<server-id>`:

```typescript
// ~/scripts/open-issues.mts (adapt the result handling to the types in catalog.ts)
import { listIssues } from 'mcp:github';

const { items } = await listIssues({ owner: 'myorg', repo: 'myrepo', state: 'open' });
console.log(items.map((issue) => `#${issue.number} ${issue.title}`).join('\n'));
```

```bash
mcp-execution-cli run ~/scripts/open-issues.mts
```

Keep scripts outside `~/.claude/servers/<id>/`: `generate` replaces that directory on every run. `run -` reads the script from stdin and arguments after `--` go to the script.

What `run` prints:

- The first stdout line is `[mcp-execution <token>] run started`; the same token marks every line the runner adds, so text printed by the script cannot pass for it.
- The script's stdout is cut at `--max-output` bytes (default 16384) and its stderr at `--max-stderr` bytes (default 4096). The remainder is saved to a private file under the per-user state directory and a notice names it.
- The last line is `[mcp-execution <token>] script_status exit 0` (or `exit N`, `signal N`, `timed out`).
- On failure a summary lists the tool calls made and their state (start and end only, never values); `completed calls may have changed state`.
- `--timeout` is 1 to 600 seconds (default 60). `--format json` prints one envelope with the same fields.

Exit codes: 0 success, 1 other error, 2 invalid input, 3 server or tool error, 4 timeout, 5 a check found problems (`drift`, `scripts check`), 6 the script itself exited non-zero or was killed by a signal.

Optional checks and limits:

| Flag | Effect |
|------|--------|
| `--typecheck` | Runs `tsc --noEmit` first and exits 2 before node starts on a type error. Skipped with a note, and the script runs, when `tsc` is missing, when Node type definitions are not installed, or when the dependencies of the generated SDK are not installed (run `npm install` in the server directory to enable the check). Real type errors in your script still exit 2. |
| `--dry-run` | Executes only the tools listed under `reads` in the policy file and skips every other call, rejecting it with `write_skipped`; the report lists the skipped writes and says the result is incomplete. |
| `--policy FILE` | Policy file for `--dry-run`: JSON `{"reads":["github/list_issues"]}`. Default: `mcp-execution/policy.json` in the per-user config directory. The default policy is empty, so every call counts as a write. |
| `--restrict` | Unix only. Limits file reads to the script, the servers directory and the run directory, and grants no file writes, child processes or workers. Network access is narrowed only on Node.js 25 and later. Windows refuses the flag with exit 2. |
| `--save NAME` | Saves the script as a named recipe after it exits 0 (see below). `--overwrite` replaces an existing recipe. |

> [!WARNING]
> `run` is not a sandbox. The script runs with your user privileges, so read a script before running it and never run one that came from tool output. The journal, `--dry-run` and `--restrict` are diagnostics and a narrowing, not a security boundary. A script cannot read its own stdin under `run`; a raw read of file descriptor 0 breaks the run. Child processes a script starts can outlive the run.

### Saved Scripts

A script that worked can be kept with the skill of its server. It starts with a header block (JSON in a comment) that is read without running anything:

```typescript
/*! mcp-script
{"v":1,"name":"open-issues","description":"List open issues of a repository","parameters":[],"tools":["github/list_issues"],"writes":"read_only"}
*/
import { listIssues } from 'mcp:github';
// ...
```

```bash
# Save after a successful run: ~/.claude/skills/github/scripts/open-issues.ts
mcp-execution-cli run ~/scripts/open-issues.mts --save open-issues

# List saved scripts by signature (no bodies), then check them against the live tool list
mcp-execution-cli scripts list --server github
mcp-execution-cli scripts check --server github
```

`scripts check` exits 5 when a saved script references a tool the server no longer has or has an invalid header, and changes nothing. `generate` and `skill` never touch the `scripts/` directory, and no command runs a saved script on its own. A skill holds at most 50 scripts of up to 64 KiB each.

### Check for Drift

```bash
# Compare the live tool list with the metadata recorded at generation (writes nothing)
mcp-execution-cli drift --from-config github
```

Exit 0: no drift. Exit 5: the tool list or endpoint differs from the recorded one. When the server was generated under another name or into another directory, pass the same `--name` and `--servers-dir` you gave `generate`.

### Supported Runtimes

| Runtime | Generated tool files (`<runtime> <tool>.ts '<json>'`) | `run`, `--typecheck`, `--restrict` |
|---------|------------------------------------------------------|-------------------------------------|
| Node.js 22.18+ (on the 23 line, 23.6+) | Supported | Supported; on 22.18 name a script with a top-level `for await` `.mts` |
| Bun | Supported (tested with `bun <tool>.ts '<json>'`) | Not supported: `run` starts Node.js |
| Deno | Supported (tested with `deno run -A <tool>.ts '<json>'`) | Not supported: `run` starts Node.js |

The Bun and Deno rows are exercised by `acceptance_runtime_compat` against the fixture server; CI fails when either runtime is missing from its job because it sets `MCP_EXECUTION_REQUIRE_ALT_RUNTIMES=1`. Without that variable the tests skip a missing runtime.

`setup` accepts a suitable Node.js. Without one it runs a one-line `.ts` file with Bun, then Deno, and reports the first that works together with a caveat: `run`, `--typecheck` and `--restrict` still need Node.js. With `--format json` the result names it in `runtime` (`name`, `version`) and carries `caveat` (`null` for Node.js).

- **Node.js below 22.18:** run a tool file through a TypeScript loader, for example `npx tsx <tool>.ts '<json>'`; `run`, `typecheck` and `--restrict` still need Node.js 22.18 or newer.

## Key Features

| Feature | Description |
|---------|-------------|
| **Typed SDK** | TypeScript parameter and result types converted from MCP JSON schemas |
| **Compact Catalog** | `catalog.ts` lists one type-only signature per tool |
| **One-Shot Calls** | `call` runs one tool of any stdio, HTTP or SSE server and prints a bounded result |
| **Script Runner** | `run` executes a script with bounded output, a journal, optional type check, dry run and `--restrict` |
| **Saved Scripts** | `run --save`, `scripts list` and `scripts check` keep and verify working scripts per skill |
| **Measured Cost** | `stats` prints bytes and estimated tokens per level; `drift` compares the live tool list with generation |
| **Direct Execution** | Each generated tool file runs with Node.js and takes one JSON argument |
| **Any MCP Server** | stdio, HTTP and SSE servers via the [rmcp SDK](https://docs.rs/rmcp) |

## Workspace Crates

| Crate | Description |
|-------|-------------|
| [mcp-execution-core](crates/mcp-core) | Foundation types, traits, and error handling |
| [mcp-execution-introspector](crates/mcp-introspector) | MCP server analysis using rmcp SDK |
| [mcp-execution-codegen](crates/mcp-codegen) | TypeScript SDK generation from MCP tool definitions |
| [mcp-execution-files](crates/mcp-files) | Virtual filesystem for code organization |
| [mcp-execution-skill](crates/mcp-skill) | SKILL.md generation for Claude Code |
| [mcp-execution-server](crates/mcp-server) | MCP server exposing SDK generation |
| [mcp-execution-cli](crates/mcp-cli) | Command-line interface |

**Dependency Graph**:

```text
mcp-execution-cli → {mcp-execution-codegen, mcp-execution-introspector, mcp-execution-files, mcp-execution-core, mcp-execution-skill}
mcp-execution-server → {mcp-execution-codegen, mcp-execution-introspector, mcp-execution-files, mcp-execution-core, mcp-execution-skill}
mcp-execution-codegen → {mcp-execution-introspector, mcp-execution-core}
mcp-execution-files → mcp-execution-codegen
mcp-execution-introspector → {rmcp, mcp-execution-core}
mcp-execution-skill → mcp-execution-core
```

## CLI Commands

See [mcp-cli README](crates/mcp-cli) for full reference.

```bash
# Generate TypeScript tools from config (recommended)
mcp-execution-cli generate --from-config github

# Preview what would be generated without writing to disk
mcp-execution-cli generate --from-config github --dry-run

# Call one tool, run a script, inspect saved scripts
mcp-execution-cli call --from-config github -- list_issues '{"state":"open"}'
mcp-execution-cli run script.mts
mcp-execution-cli scripts list --server github

# Context cost of a generated server, and drift against the live server
mcp-execution-cli stats github
mcp-execution-cli drift --from-config github

# Introspect MCP server from config
mcp-execution-cli introspect --from-config github

# Introspect with detailed schemas
mcp-execution-cli introspect --from-config github --detailed

# Manual configuration (alternative)
mcp-execution-cli introspect docker --arg=run --arg=-i --env=TOKEN=xxx

# Shell completions
mcp-execution-cli completions bash
```

> [!TIP]
> Use `--from-config` to load server settings from `~/.claude/mcp.json` instead of manual arguments.

## Upgrading to 1.0

Regenerate every server directory when upgrading to 1.0, because the generated bridge and the `_meta.json` schema changed in 0.x releases and older output no longer parses or runs. Within 1.x the `_meta.json` and helper protocol versions do not change; they change only in a major release.

```bash
mcp-execution-cli generate --from-config <SERVER>
```

Breaking changes to act on when coming from 0.x:

- **Generated output**: `_meta.json` is at schema version 3, and the bridge is a thin client of the `mcp-execution-cli` helper. A tool file run directly with `node` needs a matching `mcp-execution-cli` (`MCP_EXECUTION_CLI` or `PATH`). `closeAllConnections`, the bridge's SIGINT/SIGTERM handlers and `MCPBRIDGE_REQUEST_TIMEOUT_MS` are removed.
- **CLI**: `generate` and `introspect` accept exactly one of `--from-config`, a positional command, or `--http`/`--sse`. `call` takes the tool after `--`. Exit code 6 means a script run by `run` failed. The `--progressive-output` flag of `generate` and `drift` is now `--servers-dir`. The JSON result of `setup` replaces `node_version` with `runtime` and `caveat`.
- **Names**: server IDs and tool names consist only of Unicode `Identifier_Status=Allowed` characters, so a server exposing a tool named with a space or `@`/`+`/`(`/`&`/`<`/`>` fails introspection instead of being accepted. The `server_id` of the MCP tools must be 1-64 lowercase letters, digits or hyphens; any other value is a tool result with `isError: true`. Windows reserved device names (`con`, `prn`, `aux`, `nul`, `com0`-`com9`, `lpt0`-`lpt9`) are not valid server directory names on any platform: `--name con` is rejected, and a name derived from a command or URL gets `-server` appended. In any letter case, these names also give a generated tool file and its export a `_2`-style suffix, and are rejected as saved script names (`--save`).
- **npm package**: the standalone `mcp-execution-runtime` package was removed in v0.9.0; use the TypeScript bridge written by `mcp-execution-cli generate`.

The complete list is in the `### Breaking` section of each release in the [changelog](CHANGELOG.md).

## Performance

Generation and export timings come from the Criterion benchmarks; reproduce them with:

```bash
cargo bench --profile bench-fast -p mcp-execution-codegen -p mcp-execution-files
```

## Documentation

- [Usage Guide](examples/generated-package-usage.md) - Complete guide
- [Host Code Mode notes](examples/host-code-mode.md) - What the host documents about its own code execution mode
- [Claude Code Integration](crates/mcp-skill) - SKILL.md generation (`mcp-execution-cli skill`)

## Development

```bash
cargo build --workspace        # Build
cargo nextest run --workspace  # Test
cargo clippy --workspace       # Lint
cargo +nightly fmt --workspace # Format
```

> [!NOTE]
> All development follows [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/).

## MSRV Policy

Minimum Supported Rust Version: **1.98**

MSRV = stable minus two, raised in a minor release.

## License

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT license](LICENSE-MIT) at your option.

Unless you explicitly state otherwise, any contribution intentionally submitted for inclusion in this project by you shall be dual-licensed as above, without any additional terms or conditions.
