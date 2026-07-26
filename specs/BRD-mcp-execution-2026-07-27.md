---
aliases:
  - mcp-execution BRD
  - MCP Code Execution BRD
tags:
  - brd
  - mcp
  - codegen
  - status/draft
created: 2026-07-27
project: "mcp-execution"
status: draft
related:
  - "[[README]]"
  - "[[constitution]]"
  - "[[SRS-mcp-execution-2026-07-27]]"
  - "[[NFR-mcp-execution-2026-07-27]]"
---

# mcp-execution: Business Requirements Document

> [!abstract]
> This BRD was **reverse-engineered from an already-built, released system**
> (workspace version `0.8.0`, 18 tags from `v0.1.0` to `v0.8.0`), not authored
> before implementation. Every statement below is grounded in `README.md`,
> `CHANGELOG.md`, `Cargo.toml` package metadata, `SECURITY.md`,
> `CONTRIBUTING.md`, and the per-crate specs in this `specs/` package. There
> is no stream-of-consciousness input document to preserve — the "problem
> statement" and "target users" sections below are derived from what the
> project's own public-facing documents say about themselves, not from an
> interview or a founding brief, because no such artifact exists in this
> repository. Where the repo is silent on a normally-expected BRD element
> (market sizing, named stakeholders, budget, timeline), this is stated
> explicitly rather than invented — see [[#Open Questions]].

## Executive Summary

**mcp-execution** is a Rust workspace (CLI binary, MCP server binary, and five
library crates) that converts any [MCP (Model Context
Protocol)](https://spec.modelcontextprotocol.io/) server's tool definitions
into a self-contained package of TypeScript files using a "progressive
loading" code-generation pattern — one file per tool, discoverable via plain
`ls`/`cat` — so that an AI coding agent (concretely, Claude Code) loads only
the token budget of the tools it actually invokes in a session instead of an
entire server's tool manifest. The project explicitly credits [Anthropic's
"Code Execution with
MCP"](https://www.anthropic.com/engineering/code-execution-with-mcp)
engineering blog post as its inspiration (`README.md`). It ships both as a
standalone CLI (`mcp-execution-cli`) and as an MCP server in its own right
(`mcp-execution-server`), the latter letting Claude Code drive the same
generation pipeline using its own natural-language tool-categorization
ability instead of a second, separately-billed LLM call.

## Problem Statement

- **What problem exists today?** Per `README.md`'s own "The Problem" section:
  "Traditional MCP integration loads ALL tools from a server (~30,000
  tokens), even when you only need one or two. This wastes context window
  space and slows down AI agents."
- **Who experiences this problem?** Any developer or AI-agent host (Claude
  Code, by name, per the generated-file target path
  `~/.claude/servers/{id}/` and the `~/.claude/mcp.json` config format
  consumed by `--from-config`) that connects to one or more MCP servers with
  more tools than a given task needs.
- **What is the impact of not solving it?** Context-window/token-budget
  consumption for the AI agent session, quantified in `README.md` as up to
  **98% token savings** achieved by not solving it the traditional way — this
  is the project's own claimed number, not an independently verified
  benchmark. `CHANGELOG.md`'s `[0.2.0]` "Notes" section additionally records
  that an original internal estimate of "90%+ savings" was revised down to an
  asymptotic "~83% maximum" as the codebase matured, then the current
  `README.md` states 98% — the two documents disagree numerically and the
  repo does not reconcile them; see [[#Open Questions]].
- **What are current workarounds (if any)?** The de-facto workaround this
  project replaces is the standard MCP client behavior of loading a server's
  full tool manifest up front; the project's own solution *is* the
  documented alternative (Anthropic's code-execution-with-MCP pattern).

> [!warning] Assumptions
> - No user research, customer interviews, or support-ticket history is
>   present anywhere in this repository. The problem statement above is
>   taken at face value from the project's own README, not independently
>   validated against real user complaints.
> - "AI agents" and "Claude Code" are used interchangeably in project
>   documentation; the repo does not evidence testing against, or explicit
>   support for, any other MCP-capable agent host, even though the
>   generation output (plain TypeScript executed via `tsx`/`deno`/Node) is
>   not intrinsically Claude-specific.

## Target Users

> [!warning] Assumptions
> No persona research, user interviews, or usage telemetry exist in this
> repository. The user categories below are inferred solely from *who the
> software's interfaces are built for* (CLI ergonomics, config file format,
> crates.io publishing), not from any documented research.

### Primary Users

Individual developers who run **Claude Code** locally with one or more MCP
servers configured in `~/.claude/mcp.json`, and who use
`mcp-execution-cli generate --from-config <server>` (or the equivalent MCP
server tools) to pre-generate progressive-loading TypeScript tools instead of
letting their agent host load the server's full tool manifest. Evidenced by:
the CLI's `--from-config` flag reading `~/.claude/mcp.json` directly
(`specs/cli/spec.md`), the fixed output path `~/.claude/servers/{id}/`, and
the `setup` subcommand that specifically validates a local Node.js
installation and `~/.claude/mcp.json`'s presence.

### Secondary Users

Rust developers embedding individual workspace crates
(`mcp-execution-core`, `-introspector`, `-codegen`, `-files`, `-skill`) as
libraries in their own tooling, rather than using the CLI at all. Evidenced
by: each crate being independently published to crates.io with its own
`description`/`keywords`/`categories` in `Cargo.toml`, and `README.md`'s "As a
library" installation section.

### Stakeholders

The repository's sole recorded author/maintainer (`Andrei G`, per
`Cargo.toml`'s `workspace.package.authors` and `git config`), and open-source
contributors following `CONTRIBUTING.md`. No product owner, company,
customer, or sponsor distinct from the maintainer is evidenced anywhere in
the repository (no `NOTICE`, no named organization in `Cargo.toml`, no
enterprise-support document). `CHANGELOG.md`'s earliest entries (`[0.2.0]`,
2025-11-23) credit "Rust Project Architect, Performance Engineer, and
Security Engineer agents" for development — i.e. the project's own history
records AI-agent-assisted development, not a human team roster.

## Functional Requirements

Feature areas below map 1:1 to the workspace crates documented in
[[README#Block Structure]] and are expanded into formal `SHALL` requirements
with acceptance criteria in [[SRS-mcp-execution-2026-07-27]]. FR numbering is
shared between this BRD and the SRS for traceability.

### Server Introspection

- **FR-001**: As a developer, I need to connect to any MCP server (stdio
  subprocess, or HTTP/SSE endpoint) and discover its tools, so that I can
  generate code for a server without knowing its schema in advance.
  - *Acceptance criteria*: `introspect` (CLI) / `introspect_server` (MCP
    tool) returns tool count, names, descriptions, and (optionally) full
    JSON schemas for a reachable server; a hung or unreachable server fails
    within its configured timeout rather than blocking indefinitely.
  - *Priority*: Must
- **FR-002**: As a developer, I need introspection to reject a server that
  returns an excessive number of tools, or tools with oversized
  names/descriptions/schemas, so that a misbehaving or hostile server cannot
  exhaust memory or disk during code generation.
  - *Acceptance criteria*: introspection enforces documented, fixed bounds
    (tool count, name/description length, schema size) and fails with a
    named-resource error rather than degrading silently.
  - *Priority*: Must
- **FR-003**: As a developer, I need the tool that introspects a server on my
  behalf to validate the connection details (command, arguments, environment
  variables, URL, headers) before using them, so that a hand-edited or
  malicious server config cannot be used to run arbitrary shell commands or
  leak environment variables.
  - *Acceptance criteria*: connection details containing shell metacharacters
    or a forbidden/dangerous environment variable name are rejected before
    any process is spawned or connection opened.
  - *Priority*: Must
- **FR-004**: As a developer, I need repeated introspection of the same
  server within one process to be cached, so that redundant network/process
  round-trips are avoided.
  - *Acceptance criteria*: `Introspector` exposes `get_server`/`list_servers`
    over an in-process cache keyed by server id.
  - *Priority*: Should

### Progressive Loading Code Generation

- **FR-005**: As a developer, I need each discovered tool converted into its
  own self-contained TypeScript file (types, JSDoc, an executable CLI
  entry point), so that an agent can load exactly one tool's definition
  instead of a whole server's manifest.
  - *Acceptance criteria*: for a server with N tools, generation produces one
    `.ts` file per tool plus a fixed set of supporting files (index,
    runtime bridge, package/tsconfig, metadata sidecar).
  - *Priority*: Must
- **FR-006**: As a developer, I need generated tool metadata (name,
  description, parameter descriptions) recoverable without re-parsing the
  generated TypeScript, so that downstream tooling (SKILL.md generation) can
  read structured data instead of scraping source code.
  - *Acceptance criteria*: a `_meta.json` sidecar is always emitted alongside
    the generated tools, versioned, and consumed by `mcp-execution-skill`
    directly.
  - *Priority*: Must
- **FR-007**: As a developer, I need Claude Code's own natural-language
  understanding to categorize tools during generation, so that this project
  does not need its own separately-billed LLM API integration.
  - *Acceptance criteria*: `mcp-execution-server` exposes a two-call protocol
    (`introspect_server` then `save_categorized_tools`) in which the calling
    Claude session supplies the categorization; the CLI's plain `generate`
    path produces uncategorized output without contacting any LLM.
  - *Priority*: Must
- **FR-008**: As a developer, I need generation to reject a tool set that
  would produce an excessive number of files or bytes, so that a large or
  hostile server cannot be used to fill disk space via generated output.
  - *Acceptance criteria*: generation enforces fixed file-count and byte
    bounds, derived from introspection's own bounds, and fails loudly rather
    than partially writing output.
  - *Priority*: Must
- **FR-009**: As a developer, I need to preview what generation would produce
  without writing anything to disk, so that I can inspect the output of a
  new/unfamiliar server safely.
  - *Acceptance criteria*: `generate --dry-run` renders a file list with
    sizes and writes nothing to the filesystem.
  - *Priority*: Should

### Virtual Filesystem & Atomic Export

- **FR-010**: As a developer, I need generated output published to
  `~/.claude/servers/{id}/` such that a process killed mid-write never leaves
  a half-written tree, so that a crashed generation run cannot corrupt a
  previously working tool set.
  - *Acceptance criteria*: export stages content in a sibling temporary
    directory and publishes it via an atomic rename; a killed process leaves
    the previous, complete tree in place or a recoverable staging artifact,
    never a partial target directory.
  - *Priority*: Must
- **FR-011**: As a developer, I need re-generating one server's tools to
  never affect a different server's already-generated directory, so that
  managing many MCP servers' generated output side by side is safe.
  - *Acceptance criteria*: export treats the shared servers directory as a
    set of independently-published per-server groups; a failure publishing
    one server's group does not roll back or corrupt a sibling group.
  - *Priority*: Must
- **FR-012**: As a developer, I need re-generating a server with fewer tools
  than before to remove the now-stale tool files, so that deleted/renamed
  tools don't leave orphaned files behind.
  - *Acceptance criteria*: exporting a server's group replaces that group's
    directory contents wholesale rather than merging.
  - *Priority*: Must
- **FR-013**: As a developer, I need export to reject a batch that would
  exceed fixed file-count/byte bounds before writing anything, so that
  export cannot be used to exhaust disk space independent of the
  code-generation bound already checked upstream.
  - *Acceptance criteria*: `FilesBuilder`/`FileSystem` export enforces
    `MAX_EXPORT_FILES`/`MAX_EXPORT_BYTES`, equal to codegen's own derived
    bounds.
  - *Priority*: Must

### SKILL.md Generation

- **FR-014**: As a developer, I need a Claude Code `SKILL.md` integration
  file generated from a server's already-generated tools, so that Claude
  Code can discover and describe the tool set without me hand-writing
  documentation.
  - *Acceptance criteria*: `mcp-execution-cli skill` renders a `SKILL.md`
    directly from a generated server's `_meta.json`, with no LLM call
    required.
  - *Priority*: Must
- **FR-015**: As a developer, I need an alternative path where Claude itself
  composes the `SKILL.md` body (better summaries than a fixed template), so
  that I can trade a template-only render for LLM-quality prose when using
  the MCP server.
  - *Acceptance criteria*: `mcp-execution-server`'s `generate_skill` returns
    an LLM-facing generation prompt; `save_skill` writes Claude's composed
    content back to disk.
  - *Priority*: Should
- **FR-016**: As a developer, I need `SKILL.md` generation to detect drift
  between the metadata sidecar and the tool files actually on disk, so that
  I'm warned if the two have gone out of sync (e.g. manual edits, partial
  regeneration).
  - *Acceptance criteria*: scanning fails loudly if a sidecar entry has no
    matching `.ts` file, and reports (non-fatally) any `.ts` file with no
    sidecar entry.
  - *Priority*: Must
- **FR-017**: As a developer, I need `save_skill`/CLI `skill` writes confined
  to the intended output location, so that a malicious or malformed
  `server_id`/output path cannot write outside the intended skills
  directory.
  - *Acceptance criteria*: output path resolution is confined to
    `base_dir/server_id`, symlink-aware, and rejects path traversal.
  - *Priority*: Must

### CLI Server Configuration Management

- **FR-018**: As a developer, I need to list, inspect, and validate the MCP
  servers configured in `~/.claude/mcp.json` without running full generation,
  so that I can check my configuration quickly.
  - *Acceptance criteria*: `mcp-execution-cli server list/info/validate`
    read `~/.claude/mcp.json` as the single source of truth and report each
    entry's availability.
  - *Priority*: Should
- **FR-019**: As a developer, I need to verify my local machine is ready to
  execute generated tools (Node.js present and new enough, generated files
  executable, config present), so that I can diagnose setup problems before
  first use.
  - *Acceptance criteria*: `mcp-execution-cli setup` checks Node.js version,
    marks generated `.ts` files executable (Unix), and reports config
    presence.
  - *Priority*: Should
- **FR-020**: As a developer, I need shell completion scripts for the CLI, so
  that I can use it efficiently from an interactive shell.
  - *Acceptance criteria*: `mcp-execution-cli completions <shell>` emits a
    valid completion script for bash/zsh/fish/powershell/elvish.
  - *Priority*: Could
- **FR-021**: As a developer, I need every CLI output format (JSON/text/
  pretty) to escape server-reported values before printing them, so that a
  malicious MCP server cannot inject terminal control sequences into my
  shell.
  - *Acceptance criteria*: server-supplied names/text are escaped/quoted in
    every output format, not only JSON.
  - *Priority*: Must

### MCP Server Exposure (Session-Based Workflow)

- **FR-022**: As Claude Code (or another MCP client), I need to introspect a
  server and receive a session id representing that introspection, so that I
  can categorize its tools in a follow-up call rather than in one giant
  request.
  - *Acceptance criteria*: `introspect_server` returns a session id and
    expiry; the session can be redeemed exactly once by
    `save_categorized_tools`.
  - *Priority*: Must
- **FR-023**: As Claude Code, I need my categorization submitted in
  `save_categorized_tools` validated against what was actually introspected,
  so that I cannot (accidentally or otherwise) cause generation for tools
  that were never discovered.
  - *Acceptance criteria*: submitted tool names must match the session's own
    introspected set exactly (after identical sanitization); extra, missing,
    or duplicate names are rejected.
  - *Priority*: Must
- **FR-024**: As the operator of this MCP server, I need the number of
  in-flight (pending) introspection sessions and their aggregate memory
  footprint bounded, so that many concurrent or abandoned sessions cannot
  exhaust server memory.
  - *Acceptance criteria*: a fixed cap on pending session count and a
    separate fixed cap on aggregate estimated session bytes both apply
    independently; exceeding either is rejected before the session is
    stored.
  - *Priority*: Must
- **FR-025**: As Claude Code, I need to list previously generated servers
  under the default (or a caller-specified, confined) output location, so
  that I can check what's already been generated without re-introspecting.
  - *Acceptance criteria*: `list_generated_servers` enumerates subdirectories
    with tool counts and generation timestamps; a caller-specified
    `base_dir` is confined to the server's own base directory.
  - *Priority*: Should
- **FR-026**: As the operator of this MCP server, I need a long-running or
  hung request against one target server to never block requests targeting
  a different server, so that one slow/unreachable MCP server doesn't
  degrade the whole session for unrelated servers.
  - *Acceptance criteria*: per-server-id and per-output-directory locking is
    scoped narrowly enough that concurrent operations against different
    targets proceed independently.
  - *Priority*: Should

### Security Validation & Hardening

- **FR-027**: As any user of this software, I need any value that could be a
  secret (env var value, HTTP header value, URL credentials, CLI argument)
  excluded from debug/log output, so that a crash report, verbose log, or
  error message never leaks a token.
  - *Acceptance criteria*: every type capable of carrying such a value
    implements a redacting `Debug`; regression tests assert specific secret
    substrings never appear in debug-formatted output.
  - *Priority*: Must
- **FR-028**: As any user of this software, I need any text an introspected
  (untrusted) MCP server supplies — before it reaches me as an LLM prompt or
  rendered document — sanitized and clearly marked as untrusted data, so
  that a hostile server cannot smuggle instructions into what I read as
  trusted output.
  - *Acceptance criteria*: server-reported tool names/descriptions/keywords
    are sanitized (control characters removed/flattened) and, where
    LLM-facing, wrapped in an explicit, unforgeable untrusted-data boundary.
  - *Priority*: Must
- **FR-029**: As any user of this software, I need every filesystem write
  whose target path is influenced by caller/server input confined to its
  intended base directory, so that a crafted `server_id` or path cannot
  write outside the intended location, even via a planted symlink.
  - *Acceptance criteria*: path confinement is checked component-by-component
    against a canonicalized base, rejects a symlink at the confinement
    boundary outright, and is re-checked immediately before each write
    (never cached).
  - *Priority*: Must

## Non-Functional Requirements

Detailed, measurable non-functional requirements are specified separately in
[[NFR-mcp-execution-2026-07-27]] (this project has more than five quality
attributes with concrete, code-enforced targets, meeting this pipeline's own
threshold for a standalone NFR document). Summary below.

### Performance

Token-budget reduction is the project's core value proposition (claimed 98%
savings, `README.md`), backed by measured code-generation/export timings far
inside documented targets. See [[NFR-mcp-execution-2026-07-27#2. Performance Efficiency]].

### Scalability

Not applicable in the traditional server-scaling sense — this is a
locally-run CLI/library/single-process MCP server, not a hosted multi-tenant
service. Resource-exhaustion bounds (tool count, file count, session count)
exist instead to bound a *single* process's worst case; see
[[NFR-mcp-execution-2026-07-27#3. Reliability]].

### Security & Privacy

The dominant, most heavily-invested engineering concern in this codebase per
the reverse-engineered [[constitution#V. Security]]. Fully detailed in
[[NFR-mcp-execution-2026-07-27#4. Security]].

### Availability

Not applicable — no hosted/always-on component exists; the MCP server binary
runs for the lifetime of a local Claude Code session over stdio, not as a
managed service with an uptime SLA.

### Usability

Developer-CLI usability only (help text, shell completions, structured exit
codes); no GUI, no accessibility, no internationalization requirements are
evidenced. See [[NFR-mcp-execution-2026-07-27#5. Usability]].

## Scope & Boundaries

### In Scope

- CLI (`mcp-execution-cli`): introspect, generate, skill, server, setup,
  completions subcommands.
- MCP server (`mcp-execution-server`): `introspect_server`,
  `save_categorized_tools`, `list_generated_servers`, `generate_skill`,
  `save_skill` tools.
- Progressive-loading TypeScript code generation from MCP tool schemas.
- SKILL.md generation for Claude Code integration.
- In-memory VFS with atomic, crash-safe filesystem export.
- Security hardening: command-injection defense, resource-exhaustion (CWE-400)
  bounds, prompt-injection defense, debug-redaction, path confinement.
- Independent publishing of five library crates to crates.io.

### Out of Scope

> [!danger] Explicit Exclusions
> Confirmed by direct evidence (removed code, documented non-goals, or
> structural guards against the excluded thing), not assumed.

- **A standalone runtime package.** `mcp-execution-runtime` (an npm package
  duplicating the generated bridge) was removed entirely (`CHANGELOG.md`
  `[Unreleased]`, #261) for being an unhardened, structurally-unsynchronizable
  fork of the generated bridge template.
- **A WASM-based execution runtime and a disk-based plugin/persistence
  store.** Early `CHANGELOG.md` history (`[0.2.0]`–`[0.5.0]`) documents a
  `mcp-wasm-runtime` crate, a `mcp-plugin-store` crate (Blake3-verified
  plugin persistence), a `mcp-bridge` crate, and a `mcp-examples` crate —
  none of these exist in the current `crates/*` workspace members. The
  project's architecture moved from a WASM-execution model to the current
  plain-TypeScript progressive-loading model; the WASM/plugin-persistence
  direction is abandoned, not merely undocumented.
- **This project's own LLM API integration for tool categorization.** By
  explicit design (`constitution.md`#VII): categorization is delegated to the
  *calling* Claude Code session's own language understanding via the
  `introspect_server`/`save_categorized_tools` split, specifically to avoid
  a second, separately-billed LLM call.
- **HTTP/SSE transport for the MCP server's own introspection tool.**
  `IntrospectServerParams` has no field capable of selecting HTTP/SSE
  transport, and a dedicated test pins its exact field set so a future
  change re-adding one is a compile error unless updated deliberately — a
  structural SSRF guard, not an oversight (`specs/server/spec.md`
  `introspect_server`).
- **Sandboxing untrusted commands.** The forbidden-environment-variable list
  is explicitly documented as an "accidental-misconfiguration guard, not a
  sandbox boundary" (`specs/core/spec.md`) — it does not protect against a
  malicious command/binary itself being executed.
- **A response-size bound on the HTTP/SSE introspection transport.**
  Documented as a known, currently-unfixable upstream (`rmcp`) limitation,
  not a deliberate scope decision, and not yet resolved
  (`specs/introspector/spec.md#Known Gap: HTTP Response Size`).

## Integrations & Dependencies

| System | Direction | Data | Status |
|--------|-----------|------|--------|
| MCP servers (any, via `rmcp` SDK) | Read | Tool schemas, server handshake metadata | Exists |
| `~/.claude/mcp.json` | Read | Server connection configuration | Exists |
| `~/.claude/servers/{id}/` (Claude Code) | Write | Generated TypeScript tools | Exists |
| `~/.claude/skills/{id}/` (Claude Code) | Write | Generated `SKILL.md` | Exists |
| Node.js / `tsx` / `deno` (generated-code execution runtime) | N/A (consumed externally) | Executes generated `.ts` files | Exists, not bundled |
| crates.io / docs.rs | Write (publish) | Library crate distribution | Exists |
| GitHub Actions CI, Codecov | N/A | Build/test/lint/coverage automation | Exists |
| `cargo-deny` advisory/license/source database | Read | Supply-chain policy enforcement | Exists |

## Constraints & Assumptions

### Technical Constraints

- Rust, edition 2024, MSRV 1.91 (`Cargo.toml`, CI-enforced `msrv` job).
- `tokio` async runtime throughout; `rmcp` as the sole MCP protocol
  implementation (client and server).
- No `unsafe` code anywhere in the workspace (`#![deny(unsafe_code)]` in
  every crate except the two binaries, which don't need it).
- Workspace-wide Clippy `all`/`cargo`/`nursery`/`pedantic` set to `deny`.
- Generated output executes via Node.js 18+ (checked by `setup`), `tsx`, or
  `deno` — not bundled with this project, and not verified by this
  project's own CI beyond generating syntactically valid TypeScript.

### Business Constraints

> [!question] Ungrounded
> No document in this repository records a timeline, budget, funding, team
> size, or roadmap. `CHANGELOG.md`'s early phase history attributes
> development to named AI-agent roles ("Rust Project Architect, Performance
> Engineer, Security Engineer") rather than a human team roster, which is
> unusual for a BRD's "Business Constraints" section and is reported here
> exactly as found, not reinterpreted.

### Assumptions

> [!warning] Assumptions
> - The primary and near-exclusive consumer of generated output is Claude
>   Code specifically (not a generic "any MCP client"), based on the fixed
>   `~/.claude/*` output paths. If this assumption is wrong (e.g. the project
>   intends broader agent-host support), several functional requirements
>   above (FR-005 through FR-017) would need a configurable output root
>   rather than a hardcoded one.
> - The claimed "98% token savings" (README) is the project's own estimate,
>   not an independently reproduced benchmark; `CHANGELOG.md`'s own revised
>   "~83% asymptotic maximum" note (`[0.2.0]`) is never reconciled with the
>   current README figure anywhere in the repo.

## Success Criteria

- [x] Generated code-generation/export latency stays within documented
  targets — README's own performance table reports all four measured
  scenarios exceeding target by 8x–526x (generation: 0.19ms vs. <100ms
  target for 10 tools; export: 1.2ms vs. <10ms target).
- [x] Test suite passes with zero known failures — README states "657 tests
  with 100% pass rate" (a point-in-time claim tied to the README's own last
  edit, not a live-verified figure as of this document's date).
- [ ] Token-savings claim is reconciled and independently reproducible — not
  currently demonstrated by an executable benchmark in this repository (the
  93–98% figures live only in prose).
- [ ] A defined process exists for resolving the introspector's documented
  HTTP/SSE response-size gap — currently tracked only as a "known
  limitation" note, with no committed target release.

## Open Questions

> [!question] Unresolved Items
> These cannot be answered by reading the codebase — they require input
> from the maintainer or a governing body that doesn't appear to exist yet.

- [ ] What is the actual target user base size, and is there a support
  channel/SLA beyond "file a GitHub issue"?
- [ ] Is there a business model (this is dual MIT/Apache-2.0 licensed with no
  evidence of commercial licensing, hosting, or paid support)?
- [ ] Which figure is authoritative for the headline savings claim — 98%
  (current README) or the ~83% asymptotic maximum documented in
  `CHANGELOG.md` `[0.2.0]`?
- [ ] Is broader (non-Claude-Code) MCP-host support an intended future
  direction, given the hardcoded `~/.claude/*` output paths?
- [ ] Is there a roadmap beyond the `[Unreleased]` section of
  `CHANGELOG.md`?

## Glossary

| Term | Definition |
|------|-----------|
| MCP | Model Context Protocol — the open protocol this project introspects servers over and exposes a server for, via the `rmcp` SDK |
| Progressive loading | This project's code-generation pattern: one TypeScript file per tool, discoverable and loadable independently, instead of one large manifest |
| VFS | Virtual filesystem — `mcp-execution-files`'s in-memory staging area for generated output before it is exported to disk |
| `_meta.json` | The structured metadata sidecar emitted by codegen and consumed by skill generation, replacing a historical regex-based TypeScript re-parser |
| SKILL.md | A Claude Code integration file describing a generated server's tools, produced either by direct template rendering or by an LLM-composed path |
| CWE-400 | Common Weakness Enumeration 400 (Uncontrolled Resource Consumption) — the security category this project's resource bounds (tool count, file count, byte totals, session count) defend against |

## See Also

- [[README]] — reverse-engineered cross-block architecture and data flow
- [[constitution]] — reverse-engineered project principles
- [[SRS-mcp-execution-2026-07-27]] — formal functional requirements derived from this BRD and the per-crate specs
- [[NFR-mcp-execution-2026-07-27]] — formal non-functional/quality requirements
