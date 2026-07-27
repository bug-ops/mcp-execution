---
aliases:
  - mcp-execution-introspector spec
  - Introspection spec
tags:
  - sdd
  - spec
  - introspector
  - security
created: 2026-07-27
status: documented
related:
  - "[[../constitution]]"
  - "[[../core/spec]]"
  - "[[../codegen/spec]]"
---

# Block: MCP Server Introspection (`mcp-execution-introspector`)

> [!abstract]
> Path: `crates/mcp-introspector`. Connects to a target MCP server (stdio
> subprocess, or Streamable HTTP for both `Http`/`Sse` transports) using the
> official `rmcp` SDK, discovers its tools/capabilities, and returns a bounded
> `ServerInfo`. Depends only on `mcp-execution-core` within the workspace.

## 1. Responsibility

Given a `ServerId` + `ServerConfig`, spawn/connect to the described server,
run the MCP handshake, page through `tools/list`, and produce a `ServerInfo`
that downstream `mcp-codegen` can turn into TypeScript. Also caches discovered
servers in-process (`Introspector` holds a `HashMap<ServerId, ServerInfo>`).

## 2. Public API Surface

```rust
pub struct Introspector { /* private: HashMap<ServerId, ServerInfo> */ }
impl Introspector {
    pub fn new() -> Self;
    pub async fn discover_server(&mut self, server_id: ServerId, config: &ServerConfig) -> Result<ServerInfo>;
    pub fn get_server(&self, server_id: &ServerId) -> Option<&ServerInfo>;
    pub fn list_servers(&self) -> Vec<&ServerInfo>;
    pub fn server_count(&self) -> usize;
    pub fn remove_server(&mut self, server_id: &ServerId) -> bool;
    pub fn clear(&mut self);
}
// Default: same as new()

pub struct ServerInfo { pub id: ServerId, pub name: String, pub version: String, pub tools: Vec<ToolInfo>, pub capabilities: ServerCapabilities }
pub struct ToolInfo { pub name: ToolName, pub description: String, pub input_schema: serde_json::Value, pub output_schema: Option<serde_json::Value> }
pub struct ServerCapabilities { pub supports_tools: bool, pub supports_resources: bool, pub supports_prompts: bool }
```

Resource-limit constants (all `pub`):

| Constant | Value | Guards |
|---|---|---|
| `MAX_TOOL_COUNT` | 1000 | total tools returned by `tools/list` (paged) |
| `MAX_TOOL_NAME_LEN` | 256 | bytes, per tool name |
| `MAX_TOOL_DESCRIPTION_LEN` | 8 KiB | bytes, per tool description |
| `MAX_SCHEMA_SIZE_BYTES` | 64 KiB | serialized bytes, per input **or** output schema |

`MAX_SCHEMA_SIZE_BYTES` is the dominant term multiplied through every
downstream derived budget (`mcp-codegen::MAX_GENERATED_BYTES`,
`mcp-files::MAX_EXPORT_BYTES`, `mcp-server::state::MAX_TOTAL_PENDING_BYTES`)
— it was deliberately shrunk 4x (from 256 KiB) specifically to shrink those
derived budgets proportionally without touching their own formulas.

## 3. Discovery Flow (`discover_server`)

1. `validate_server_config(config)` — defense in depth, even though a
   builder-constructed `ServerConfig` is already validated (see
   [[../core/spec#Defense in depth]]).
2. Dispatch on `config.transport()`:
   - `Stdio` → `discover_via_stdio_process`: spawns the child
     (`kill_on_drop(true)` as a backstop against a dropped future leaking the
     process — deliberately *not* relying on `rmcp`'s own `TokioChildProcess`
     cleanup, whose `tokio::spawn`-based `Drop` can be starved under a
     short-lived runtime); wires stdout through a **size-bounded** decoder
     (`bounded_response_stream`, see [[#Response-line bounding (stdio)]])
     instead of `rmcp`'s default unbounded `AsyncRwTransport`; always kills
     the child afterward regardless of outcome.
   - `Http`/`Sse` → `discover_via_http`: builds a `StreamableHttpClientTransport`
     with caller headers converted to `http::HeaderValue`/`HeaderName`. **No
     response-size bound exists on this path** — see
     [[#Known gap HTTP response size]].
3. `client.serve(transport)` bounded by `config.connect_timeout()` →
   `Error::Timeout { operation: "connect to {id}", .. }` on expiry.
4. `list_tools_bounded(&client)` — pages via `list_tools`, bailing out as
   soon as the running total exceeds `MAX_TOOL_COUNT` **without** buffering
   every page first (unlike `rmcp`'s own `list_all_tools`, which would hold
   an arbitrarily large response in memory before any bound is checked).
   Bounded overall by `config.discover_timeout()` →
   `Error::Timeout { operation: "list_all_tools for {id}", .. }`.
5. `extract_peer_meta` — pulls server name/version/capability flags from the
   handshake `InitializeResult`; falls back to
   `config.command`/`config.url()` and `"unknown"` version if the server
   sent no peer info.
6. `build_server_info` → per-tool `build_tool_info`, enforcing
   `MAX_TOOL_NAME_LEN`/`MAX_TOOL_DESCRIPTION_LEN`/`MAX_SCHEMA_SIZE_BYTES` and
   the overall `MAX_TOOL_COUNT` — returns `Error::ResourceLimitExceeded` on
   any violation, naming the specific tool.
7. Cache result in `self.servers`, return `ServerInfo`.

## 4. Response-Line Bounding (stdio)

`rmcp`'s default `(ChildStdout, ChildStdin)` transport reads lines via an
unbounded `BufReader::read_until`, bypassing `JsonRpcMessageCodec`'s own
`max_length` entirely. This crate replaces that with an explicit
`FramedRead` over a custom `BoundedResponseDecoder` wrapping
`JsonRpcMessageCodec`, capped at `MAX_RESPONSE_LINE_SIZE` = 4 MiB (private
constant, matches `mcp-server`'s `MAX_REQUEST_LINE_SIZE` for consistency,
not derivation — this bounds an **untrusted server's** responses, that one
bounds an already-trusted local client's requests).

- An oversized/malformed/skipped line is **dropped, logged at WARN**, not
  treated as a hard error — the request it was answering then runs out its
  timeout and surfaces as `Error::Timeout` rather than a distinct
  size-limit error (there is no request id to correlate a dropped, unparsed
  line back to).
- A genuine I/O error still ends the session immediately (not retried).
- Blank/whitespace-only lines are silently skipped without a WARN log
  (avoids log-volume amplification from noisy stdout).
- `tokio_util`'s internal buffer grows by doubling, so peak buffer capacity
  during an oversized-line attack can reach ~4x `MAX_RESPONSE_LINE_SIZE`
  before rejection — still strictly bounded, just not 1:1 with the cap.

## 5. Known Gap: HTTP Response Size

`rmcp` 2.2.0's Streamable HTTP client transport buffers each JSON response
body and each SSE event **fully in memory** before this crate's own
`MAX_TOOL_COUNT`/`MAX_SCHEMA_SIZE_BYTES` checks ever run, with no
`rmcp`-provided config knob to bound that buffering. The only mitigation is
`ServerConfig::discover_timeout` (bounds *how long* an unbounded response
can be read, not *how large* it can grow). This is documented as a known
upstream limitation, not something fixable without reimplementing a large
part of `rmcp`'s HTTP transport client-side; `rmcp` 3.0.0-beta.2 adds a
`max_sse_event_size` knob (SSE only, not JSON bodies) — revisit once a
3.0.0 stable ships.

## 6. Error Conditions

| Condition | `Error` variant |
|---|---|
| `validate_server_config` fails | `SecurityViolation` / `ValidationError` (from `mcp-core`) |
| Process spawn fails (stdio) | `ConnectionFailed { server, source }` |
| Connect handshake exceeds `connect_timeout` | `Timeout { operation: "connect to {id}", duration_secs }` |
| `tools/list` exceeds `discover_timeout` | `Timeout { operation: "list_all_tools for {id}", duration_secs }` |
| Accumulated tool count > `MAX_TOOL_COUNT` during paging | `ResourceLimitExceeded { resource: ResourceKind::ToolCount { server_id }, .. }` |
| Single tool's name/description/schema exceeds its bound | `ResourceLimitExceeded { resource: ResourceKind::ToolNameLength \| DescriptionLength { tool_name } \| InputSchemaSize { tool_name } \| OutputSchemaSize { tool_name }, .. }` |
| HTTP header name/value invalid (introspection-time) | `ConnectionFailed` (header construction failure) |

## 7. Edge Cases & Notable Behaviors

- A server that sends no `peer_info` on handshake still produces a usable
  `ServerInfo` (fallback name from `config`, version `"unknown"`,
  capabilities all `false` except `supports_tools` which is derived from
  whether any tools were found).
- Two tools sharing an identical raw name are *not* deduplicated at this
  layer — collision handling (via `resolve_typescript_names`) is
  `mcp-codegen`'s responsibility.
- Dropping the `Introspector::discover_server` future (e.g. a caller racing
  it against its own cancellation via `tokio::select!`) still reliably
  kills the spawned stdio child, via `kill_on_drop(true)` set at spawn time
  — not via any code inside `discover_server` itself.
- `list_tools_bounded` bails out **as soon as** the page that first pushes
  the running total over `MAX_TOOL_COUNT` is fetched — at most one page's
  worth of tools beyond the limit is ever held in memory at once, not the
  server's entire (potentially huge) full response.

## 8. Cross-Crate Contracts

- **Consumes** `mcp-core`: `ServerConfig`, `ServerId`, `ToolName`,
  `Transport`, `validate_server_config`, `Error`/`Result`.
- **Produced for** `mcp-codegen`: `ServerInfo`/`ToolInfo` are the direct
  input to `ProgressiveGenerator::generate`/`generate_with_categories` — see
  [[../codegen/spec#Input contract]]. `MAX_TOOL_COUNT`,
  `MAX_TOOL_NAME_LEN`, `MAX_TOOL_DESCRIPTION_LEN`, `MAX_SCHEMA_SIZE_BYTES`
  are referenced **by value** (not independently re-chosen) in
  `mcp-codegen`'s and `mcp-files`'s own derived resource bounds.

## 9. See Also

- [[../core/spec]] — `ServerConfig`/security validation this crate re-checks
- [[../codegen/spec]] — consumer of `ServerInfo`/`ToolInfo`
- [[../server/spec#introspect_server]] — MCP-tool-level wrapper around this crate
