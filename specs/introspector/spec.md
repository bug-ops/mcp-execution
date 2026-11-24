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
  - "[[../runtime/spec]]"
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
pub struct ToolInfo { pub name: ToolName, pub description: String, pub input_schema: serde_json::Value, pub output_schema: Option<serde_json::Value>, pub annotations: Option<ToolAnnotations> /* #[serde(default, skip_serializing_if = "Option::is_none")] */ }
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

The whole method carries a `#[tracing::instrument(skip_all, fields(server_id =
%server_id))]` span (issue #211): every `tracing` call made during the steps
below runs inside that span, so under the default `fmt` subscriber each log
line gets a `discover_server{server_id=...}:` prefix. No existing log
message's text changed — only this added span context, which lets
concurrently-discovered servers' log output be correlated.

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
     with caller headers converted to `http::HeaderValue`/`HeaderName`. **SSE
     events are bounded** by `rmcp`'s `max_sse_event_size`, set explicitly to
     match rmcp's own default; **JSON bodies (success and error) are
     unbounded** — see [[#Known gap HTTP JSON response size]].

   Both branches converge on a single private `connect_and_list_tools`
   helper, generic over the connect future each transport builds (issue
   #294): `discover_via_stdio` and `discover_via_http` used to copy-paste the
   same connect/list-tools/timeout/error-mapping pipeline for their shared
   `RunningService<RoleClient, ()>` client, and now both delegate to it.
   Steps 3–5 below are that shared pipeline — timeout durations, error
   types/messages, and success output are unchanged from before the dedup.
3. `client.serve(transport)` bounded by `config.connect_timeout()` →
   `Error::Timeout { operation: "connect to {id}", .. }` on expiry. The
   handshake is a legacy `initialize` request declaring rmcp's
   `ProtocolVersion::LATEST` (`2026-07-28` as of rmcp 3.5.0, taken from
   `().serve`'s default client config); `server/discover` and
   `ClientLifecycleMode::Auto` are not used (deferred, ADR-529). Accepted
   failure classes: a strict server that rejects the unknown requested
   version instead of counter-offering yields `ConnectionFailed`; a server
   that echoes `2026-07-28` via `initialize` makes rmcp switch to
   per-request `_meta` semantics, which a legacy server may reject at
   `tools/list`; an initialize-less server yields `ConnectionFailed`.
   Guarded by `test_adr_529_protocol_version_latest_gate` (`lib.rs`) and
   `test_discover_server_http_handshake_is_initialize_at_latest`
   (`tests/http_transport_test.rs`); stdio shares the identical
   `().serve` call and is not separately tested.
4. `list_tools_bounded(&client)` — pages via `list_tools`, bailing out as
   soon as the running total exceeds `MAX_TOOL_COUNT` **without** buffering
   every page first (unlike `rmcp`'s own `list_all_tools`, which would hold
   an arbitrarily large response in memory before any bound is checked).
   Bounded overall by `config.discover_timeout()` →
   `Error::Timeout { operation: "list_all_tools for {id}", .. }`.
5. `extract_peer_meta` — pulls server name/version/capability flags from the
   handshake `InitializeResult` into a private `PeerMeta` struct
   (`server_name`, `server_version`, `has_resources`, `has_prompts`),
   replacing a positional `(String, String, bool, bool)` tuple whose two
   trailing `bool`s could previously be transposed without a type error
   (issue #207); falls back to `config.command`/`config.url()` and
   `"unknown"` version if the server sent no peer info. The tool list and
   `PeerMeta` are combined into a private `DiscoveryResult { tools, peer_meta
   }`, also replacing a positional tuple, and threaded from
   `discover_via_stdio_process`/`discover_via_stdio`/`discover_via_http` up
   through `discover_server`.
6. `build_server_info` → per-tool `build_tool_info`, enforcing
   `MAX_TOOL_NAME_LEN`/`MAX_TOOL_DESCRIPTION_LEN`/`MAX_SCHEMA_SIZE_BYTES` and
   the overall `MAX_TOOL_COUNT` — returns `Error::ResourceLimitExceeded` on
   any violation, naming the specific tool. Also enforces
   [`ToolName::new`]'s invariant on every tool's `name` — see the error table
   below.
7. Cache result in `self.servers`, keyed by the just-built `ServerInfo`'s own
   `info.id.clone()` — **not** the `server_id` parameter threaded through
   steps 1–6 — so the map key is structurally derived from the value's own
   identity and cannot drift from it, even though both are sourced from the
   same identifier in every call today (issue #317, no public API change).
   Return `ServerInfo`.

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

## 5. Known Gap: HTTP JSON Response Size

As of `rmcp` 3.1.2, SSE events are bounded: `discover_via_http` sets
`StreamableHttpClientTransportConfig::max_sse_event_size` explicitly (16 MiB,
matching `rmcp`'s own default, pinned locally so an upstream default change
cannot silently loosen this crate's bound) — an oversized SSE event is
rejected instead of buffered (issue #226, narrowed by #390).

The plain JSON response path remains unbounded: `rmcp` deserializes it via
`reqwest::Response::json`, buffering the body **fully in memory** before this
crate's own `MAX_TOOL_COUNT`/`MAX_SCHEMA_SIZE_BYTES` checks ever run, with no
`rmcp`-provided config knob to bound it. A non-2xx error response is read
the same unbounded way and folded into the resulting error's message, so a
hostile server can force a large buffer (plus a copy of it in the error
value) by answering with an oversized error body — same root cause, no
separate tracking needed. The only mitigation is
`ServerConfig::discover_timeout` (bounds *how long* an unbounded body can be
read, not *how large* it can grow).

This is a known upstream limitation, not something fixable from this crate:
the unbounded `.json()`/`.text()` calls live inside `rmcp`'s own
`impl StreamableHttpClient for reqwest::Client`, and the only override point
(`StreamableHttpClientTransport::with_client`) requires reimplementing
`post_message`/`get_stream`/`delete_session` against helpers (`bounded_sse_stream`,
`DEFAULT_MAX_SSE_EVENT_SIZE`, `validate_custom_header`, `RESERVED_HEADERS`)
that `rmcp` keeps `pub(crate)`. Doing so would trade the JSON bound for the
loss of the already-working SSE bound and of reserved-header validation — a
net regression — while adding a direct `reqwest` dependency this crate
deliberately avoids. A `Content-Length` pre-check is also unsound: the
header is absent for chunked responses, exactly what an attacker would send.
There is no version-gated condition left to revisit this under; it stays
open until `rmcp` exposes a JSON-body size knob.

## 6. Error Conditions

| Condition | `Error` variant |
|---|---|
| `validate_server_config` fails | `SecurityViolation` / `ValidationError` (from `mcp-core`) |
| Process spawn fails (stdio) | `ConnectionFailed { server, source }` |
| Connect handshake exceeds `connect_timeout` | `Timeout { operation: "connect to {id}", duration_secs }` |
| `tools/list` exceeds `discover_timeout` | `Timeout { operation: "list_all_tools for {id}", duration_secs }` |
| Accumulated tool count > `MAX_TOOL_COUNT` during paging | `ResourceLimitExceeded { resource: ResourceKind::ToolCount { server_id }, .. }` |
| Single tool's name/description/schema exceeds its bound | `ResourceLimitExceeded { resource: ResourceKind::ToolNameLength \| DescriptionLength { tool_name } \| InputSchemaSize { tool_name } \| OutputSchemaSize { tool_name }, .. }` |
| Tool name fails `ToolName::new`'s invariant (e.g. contains `/`, empty) | `ValidationError { field: "tool name", reason }` — hard-fails the *entire* `discover_server` call via `?`-propagation, exactly like an oversized name/description/schema above: a single malformed tool name is not skipped-with-a-warning while the rest of the server's tools are returned (#287) |
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
- `ServerInfo`/`ToolInfo` derive `Deserialize` and hold `ServerId`/`ToolName`
  fields directly. Both newtypes are constructible only through their
  fallible `new`, enforced even through their `#[serde(try_from = "String")]`-
  backed `Deserialize` impl (see [[../core/spec]]) — so deserializing a
  `ServerInfo`/`ToolInfo` from untrusted JSON with a hostile `id`/tool `name`
  (e.g. containing `..` or a path separator) now fails outright instead of
  silently producing an unvalidated value, closing a gap that predates this
  invariant.

## 8. Cross-Crate Contracts

- **Consumes** `mcp-core`: `ServerConfig`, `ServerId`, `ToolName`,
  `Transport`, `validate_server_config`, `Error`/`Result`.
- **Produced for** `mcp-codegen`: `ServerInfo`/`ToolInfo` are the direct
  input to `ProgressiveGenerator::generate`/`generate_with_categories` — see
  [[../codegen/spec#Input contract]]. `MAX_TOOL_COUNT`,
  `MAX_TOOL_NAME_LEN`, `MAX_TOOL_DESCRIPTION_LEN`, `MAX_SCHEMA_SIZE_BYTES`
  are referenced **by value** (not independently re-chosen) in
  `mcp-codegen`'s and `mcp-files`'s own derived resource bounds.

## 9. Annotation Capture (#568)

`ToolInfo.annotations: Option<mcp_execution_core::annotations::ToolAnnotations>` (serde
`default`, omitted when `None`) carries what the server advertised for a tool; the types are in
[[../core/spec#`annotations` module (`src/annotations.rs`, #568)]]. `build_tool_info` captures them
through a private `capture_annotations(title, wire_annotations)`:

- The title is `Tool.title`, falling back to `annotations.title`, and passes through
  `AnnotationTitle::new` (sanitized, capped at `MAX_UNTRUSTED_FIELD_LEN`); a title that sanitizes to
  nothing counts as absent.
- Each of `readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint` becomes a three-valued
  `Hint` (`From<Option<bool>>`); the SDK's documented defaults are never applied.
- The result is `None` when the server stated no hint and sent no usable title, so omitting
  `annotations` and sending an empty object capture identically; a title-only tool keeps its title
  with four `NotStated` hints.
- Contradictory hints (read-only with destructive) are stored as advertised.

| ID | Requirement | Status |
|---|---|---|
| FR-034.8 | A tool's title and its four hints are captured into `ToolInfo` | implemented |
| FR-034.9 | Each hint is a typed three-valued value; SDK defaults are not applied | implemented |
| FR-034.10 | A tool with no annotations and no title is recorded as absent | implemented |
| FR-034.11 | Values are recorded as advertised, not reinterpreted | implemented |
| FR-034.12 | The captured payload is bounded: the title is capped by the sanitizer and hints carry no text | implemented in part: the payload is not added to the per-tool size checks at introspection (`MAX_SCHEMA_SIZE_BYTES` and the description cap apply to the schemas and description only); the bound is the title cap |
| FR-034.13 | No code in this crate branches on an annotation; a missing annotation is never read as safe | implemented |

**Constraints.** Annotations are untrusted hints from a possibly hostile server; the rmcp SDK's own
documentation says clients should never make tool-use decisions from annotations received from
untrusted servers. Nothing in this crate relaxes a check because a hint is present or has a given
value. The sidecar field that persists the values is in
[[../core/spec#`metadata` module (`src/metadata.rs`)]].

Ask first: making any consumer read a hint for a decision (that belongs to the dry-run policy, #573).

## 10. Planned Changes (script-first direction, adopted 2026-11-24)

> [!warning] Not implemented
> The annotation capture of #568 is implemented and described in
> [[#9. Annotation Capture (#568)]]. Nothing in this section exists in the code yet. Each subsection
> names its tracking issue. When a change lands, its content moves into the as-built sections above
> and the subsection is removed in the same pull request.

### Planned: keep the freshness hints of a tools listing (#580)

**Why.** The drift check (FR-045) compares a live listing with the recorded provenance and should
report how long the server says the listing stays fresh and who may cache it. The pinned SDK's
tools-listing result carries `ttl_ms: Option<u64>` and `cache_scope: Option<CacheScope>`
(`rmcp-3.10.0/src/model.rs:1894-1960`, the `paginated_result!` macro, instantiated for tools at
`model.rs:4398-4402`); both are optional because they are required only from protocol version
`2026-07-28`. This crate reads neither: `list_tools_bounded` extends a vector with each page's
tools and keeps only the next cursor (`crates/mcp-introspector/src/lib.rs:738-744`), and
`ServerInfo` has no field for them (`lib.rs:196-209`).

**Requirements.**

| ID | Requirement | Priority |
|---|---|---|
| FR-045.5 | WHEN a tools listing carries `ttl_ms` or `cache_scope` THE SYSTEM SHALL expose them as typed values on the discovery result, and WHEN absent THE SYSTEM SHALL expose them as absent | must |
| FR-045.6 | THE SYSTEM SHALL type `ttl_ms` as a duration and `cache_scope` as an enum, not as a number and a string | must |
| FR-045.7 | WHEN a listing spans several pages THE SYSTEM SHALL define which page's hints are exposed and SHALL document the rule | must |
| FR-045.8 | THE SYSTEM SHALL NOT use either hint to skip, shorten or cache a request; it only reports them | must |

**Constraints.** The hints are server-supplied and untrusted; they describe the server's claim, not
a guarantee. Exposing them changes public fields of `ServerInfo`, which derives `Deserialize` and
is constructed by struct literal in downstream crates, so the change is ask-first. Paging stays
bounded exactly as in [[#3. Discovery Flow (`discover_server`)]].

**Open decisions.**

- [NEEDS CLARIFICATION: Which page's hints are kept when pages differ: first page, last page, or the most conservative combination (smallest freshness, most restrictive scope).]
- [NEEDS CLARIFICATION: Whether the hints are fields of `ServerInfo` or of a separate listing-metadata value returned alongside it.]
- Ask first: changing any public field of `ServerInfo` or `ToolInfo`; adding a dependency.

**Depends on / blocks.** Blocks the freshness part of #580. Independent of the v3 bump.

### Planned: a shared connect step for tool calls (#574)

**Why.** Today the client connects, lists tools and is dropped: `connect_and_list_tools` builds the
`RunningService`, pages the listing under the discover timeout and returns only the tool list and
peer metadata (`crates/mcp-introspector/src/lib.rs:1009-1050`); on the HTTP path the client is
dropped when it returns (`lib.rs:1137-1141`). No call-tool request is issued anywhere in the
Rust crates; calling a tool is done only by the generated TypeScript bridge. A one-shot call
command (FR-039) needs a live client. The quick way, a small separate client beside this crate,
would be a second connection path with its own validation, timeouts and bounds, which the
project avoids.

**Requirements.**

| ID | Requirement | Priority |
|---|---|---|
| FR-039.30 | THE SYSTEM SHALL provide one connect step, shared by tool listing and tool calling, covering config validation, transport selection (stdio, HTTP, SSE), the connect timeout, the bounded stdio response decoder, HTTP header handling and protocol negotiation | must |
| FR-039.31 | THE SYSTEM SHALL NOT introduce a second connection path, a lighter client or a second copy of the config validation for calling tools | must |
| FR-039.32 | WHEN the one-shot call connects THE SYSTEM SHALL run `validate_server_config` through the same code as discovery, so an unsafe config is rejected with the same classified error before any connection is made | must |
| FR-039.33 | THE SYSTEM SHALL leave `Introspector::discover_server`'s public behaviour, errors and tests unchanged; the shared step is extracted, not duplicated | must |
| FR-039.34 | WHEN the call ends, fails or times out THE SYSTEM SHALL terminate any server process it started | must |

**Constraints.** The call path itself, its output bounds and its exit codes are specified in the
new block [[../runtime/spec]] and in [[../cli/spec]]; this block owns only the shared connect
step. The known unbounded HTTP JSON response gap ([[#5. Known Gap: HTTP JSON Response Size]]) is
inherited by the call path and stated there, not hidden. The `2024-11-05` handshake pin of the
generated bridge is not copied; negotiation follows the existing connect step and the
protocol-version decision record.

**Open decisions.**

- [NEEDS CLARIFICATION: Shape of the shared step: a public type or function in this crate, or crate-private with the call path inside this crate.]
- [NEEDS CLARIFICATION: Whether the connection is returned to the caller for reuse across several calls (needed by the per-script helper, #575) or consumed by one call.]
- Ask first: new public API on this crate; moving the step into another crate; any change to timeouts, bounds or handshake behaviour; adding a dependency.

**Depends on / blocks.** Gated by the runtime decision record #569 (if the generated bridge stays
the only client, a Rust call path would be a second way to call tools). Blocks #574 and #575.

## 10. See Also

- [[../core/spec]] — `ServerConfig`/security validation this crate re-checks
- [[../codegen/spec]] — consumer of `ServerInfo`/`ToolInfo`
- [[../decisions/ADR-529-rmcp-3-5-latest-promotion-revisit]] — handshake
  protocol-version decision and revisit gate
- [[../server/spec#introspect_server]] — MCP-tool-level wrapper around this crate
- [[#10. Planned Changes (script-first direction, adopted 2026-11-24)|Planned changes]] — freshness hints, shared connect step; [[#9. Annotation Capture (#568)]] — implemented
- [[../runtime/spec]] — the planned tool-call path that reuses the shared connect step
