---
aliases:
  - mcp-execution-server spec
  - MCP Server Exposure spec
tags:
  - sdd
  - spec
  - server
  - security
created: 2026-07-27
status: documented
related:
  - "[[../constitution]]"
  - "[[../introspector/spec]]"
  - "[[../codegen/spec]]"
  - "[[../files/spec]]"
  - "[[../skill/spec]]"
---

# Block: MCP Server Exposure (`mcp-execution-server`)

> [!abstract]
> Path: `crates/mcp-server`. Binary + library that exposes progressive-loading
> generation and skill generation **as an MCP server itself** — i.e. Claude
> Code can drive this project's own functionality over MCP, using the calling
> Claude session's own natural-language understanding for tool
> categorization instead of a second, separately-billed LLM call. Depends on
> `mcp-execution-core`, `mcp-execution-introspector`, `mcp-execution-codegen`,
> `mcp-execution-files`, `mcp-execution-skill`.
>
> Not merely a "progressive loading" server as CLAUDE.md's crate table
> summarizes it — see [[../README#Discrepancies vs CLAUDE.md]].

## 1. Responsibility

Expose five MCP tools via `rmcp`'s `#[tool_router]`/`#[tool_handler]`
machinery, backed by an in-process session store
(`StateManager`) that bridges a two-call protocol
(`introspect_server` → `save_categorized_tools`) so Claude can inspect tool
metadata, categorize it via its own language understanding, and hand the
categorization back for code generation — without this project needing its
own LLM API key.

## 2. Public API Surface (Types)

```rust
// crate root re-exports
pub use clock::{Clock, SystemClock};
pub use service::GeneratorService;
pub use state::StateManager;
pub use types::{CategorizedTool, GeneratedServerInfo, IntrospectServerParams,
    IntrospectServerResult, IntrospectedToolSummary, ListGeneratedServersParams,
    ListGeneratedServersResult, PendingGeneration, SaveCategorizedToolsParams,
    SaveCategorizedToolsResult, ToolGenerationError};
// re-exported from mcp_execution_skill for the generate_skill/save_skill tools:
pub use mcp_execution_skill::{GenerateSkillParams, GenerateSkillResult,
    SaveSkillParams, SaveSkillResult, SkillCategory, SkillMetadata, SkillTool, ToolExample};

pub trait Clock: Send + Sync + Debug { fn now(&self) -> DateTime<Utc>; }
pub struct SystemClock; // production
#[cfg(test)] pub struct TestClock; // injectable fake, Arc<Mutex<DateTime<Utc>>>

pub struct GeneratorService {
    state: Arc<StateManager>,
    introspectors: Arc<Mutex<HashMap<ServerId, Arc<Mutex<Introspector>>>>>,
    exports: Arc<Mutex<HashMap<PathBuf, Arc<Mutex<()>>>>>,
    clock: Arc<dyn Clock>,
    skills_base_dir: Option<PathBuf>,  // test-only override; production = ~/.claude/skills
    servers_base_dir: Option<PathBuf>, // test-only override; production = ~/.claude/servers
    tool_router: ToolRouter<Self>,
}
impl GeneratorService { pub fn new() -> Self; } // Default too

pub struct StateManager { /* pending: Arc<RwLock<PendingTable>>, clock */ }
impl StateManager {
    pub fn new() -> Self; // real clock
    pub async fn store(&self, generation: PendingGeneration) -> Result<Uuid, StateError>;
    pub async fn take(&self, session_id: Uuid) -> Option<PendingGeneration>;
    pub async fn get(&self, session_id: Uuid) -> Option<PendingGeneration>;
    pub async fn take_if<T, E>(
        &self,
        session_id: Uuid,
        validate: impl FnOnce(&PendingGeneration) -> Result<T, E>,
    ) -> Option<Result<(PendingGeneration, usize, T), E>>; // usize = the entry's known size_bytes
    pub async fn pending_count(&self) -> usize;
    pub async fn cleanup_expired(&self) -> usize;
    // pub(crate) async fn restore(&self, session_id: Uuid, generation: PendingGeneration,
    //     size_bytes: usize) -> Result<(), StateError>; -- crate-internal, see below
}
pub const MAX_PENDING_SESSIONS: usize = 1000;
pub const MAX_TOTAL_PENDING_BYTES: usize; // = 4 * per-session-estimate (see below)

pub struct PendingGeneration {
    pub server_id: ServerId, pub server_info: ServerInfo, pub config: ServerConfig,
    pub output_dir_override: Option<PathBuf>, pub created_at: DateTime<Utc>, pub expires_at: DateTime<Utc>,
}
impl PendingGeneration {
    pub const DEFAULT_TIMEOUT_MINUTES: i64 = 30;
    pub fn new(server_id, server_info, config, output_dir_override, clock: &dyn Clock) -> Self;
    pub fn is_expired(&self, clock: &dyn Clock) -> bool; // strict `>`, not `>=`
}
```

## 3. MCP Tools Exposed

| Tool | Purpose | Cancellation-aware? |
|---|---|---|
| [[#introspect_server]] | Connect to a target server, discover tools, return a session id | Yes |
| [[#save_categorized_tools]] | Generate + export TypeScript from a prior session's categorization | No (deliberately) |
| [[#list_generated_servers]] | Enumerate previously-generated servers under `~/.claude/servers` | No (single bounded scan) |
| [[#generate_skill]] | Scan a generated server's tools, return an LLM-facing SKILL.md generation prompt | Yes |
| [[#save_skill]] | Write Claude-composed SKILL.md content, confined to `~/.claude/skills/{server_id}/` | No (deliberately) |

`get_info()` (`ServerHandler`) pins `protocol_version` to `2025-06-18` as
the fallback used for negotiation against clients requesting an
unrecognized version, enables the tools capability, and provides an
`instructions` string describing the introspect→categorize→save workflow.
`supported_protocol_versions()` and `discover()` are not overridden, so
rmcp's defaults apply: the server advertises every protocol version the
SDK knows (`ProtocolVersion::KNOWN_VERSIONS`), not just `2025-06-18`
(characterized by tests in `crates/mcp-server/tests/integration_tests.rs`,
issue #381).

### `introspect_server`

`IntrospectServerParams { server_id, command, args, env, output_dir: Option<PathBuf>, connect_timeout_secs: Option<u64>, discover_timeout_secs: Option<u64> }`
→ `IntrospectServerResult { server_id, server_name, tools_found, tools: Vec<IntrospectedToolSummary>, session_id: Uuid, expires_at }`.

- **Always builds a stdio `ServerConfig`** (`build_stdio_server_config`) —
  `IntrospectServerParams` has **no** field capable of selecting HTTP/SSE
  transport (`url`/`http`/`sse`/`headers`), and a dedicated test
  (`introspect_server_params_shape_is_pinned`) uses an exhaustive struct
  destructure with no `..` rest pattern so **adding such a field is a
  compile error** unless the destructure is updated too — a structural
  guard against reintroducing SSRF risk, since `ServerConfig`'s own docs
  note this crate is exactly the kind of server-context embedder expected
  to add SSRF allowlisting before ever setting Http/Sse transport.
- `server_id` validated via `mcp_execution_skill::validate_server_id`
  before anything else; the tracing span's `server_id` field is left empty
  on early validation failure (never logging unvalidated attacker input
  into a structured field).
- `output_dir`, if given, is checked with the cheap, I/O-free
  `relative_subpath` (rejects absolute/`..`) — **not** the full
  filesystem-touching confinement walk, which is deliberately deferred to
  `save_categorized_tools` (see [[#Output directory resolution]]).
- Introspection itself races `Introspector::discover_server` against the
  request's `CancellationToken` via `tokio::select! { biased; ... }` (see
  [[#Per-resource locking]]).
- Result is stored as a `PendingGeneration` in `StateManager`; response is
  wrapped via `mcp_execution_core::untrusted::wrap_untrusted_block`
  (`wrap_introspect_result`) since the tool summaries it contains are
  server-reported, attacker-controlled text now shown to Claude.
- Errors from building the `ServerConfig` or from discovery are classified via
  `caller_or_internal_error`: a `ValidationError` **or** `SecurityViolation`
  (shell metacharacters, a forbidden env var, etc.) reports as `invalid_params`
  — the caller's fault — while anything else reports as `internal_error`.
  `SecurityViolation` previously fell through to `internal_error`, misreporting
  hostile caller input as a server-side fault; it is now handled identically to
  `ValidationError`.

### `save_categorized_tools`

`SaveCategorizedToolsParams { session_id: Uuid, categorized_tools: Vec<CategorizedTool> }`
→ `SaveCategorizedToolsResult { success, files_generated, output_dir, categories: HashMap<String,usize>, errors: Vec<ToolGenerationError> }`.

1. `state.take_if(session_id, validate)` — validates in place and consumes the session
   only if `validate` succeeds, without ever deep-cloning it (issue #378; see
   [[#State Management (StateManager)]]). `validate` is `validate_categorized_tools`
   (step 2 below), a synchronous closure that runs while `take_if` holds the state
   table's write lock — deliberately restricted to fast, in-memory checks with no I/O.
   Returns `None` if the session doesn't exist, has expired, or is currently checked
   out by another in-flight `save_categorized_tools` call for the same `session_id`
   (`invalid_params` — wording covers all three, since a concurrent caller can't tell
   which applies); `Some(Err(e))` if `validate` rejected the entry, leaving the
   session in place at its original expiry for the caller to retry with the same
   `session_id` (issue #371); `Some(Ok((pending, size_bytes, (categorization,
   categories))))` if `validate` accepted it — the session is removed from
   `StateManager` at this point and this call is its sole owner from here on.
   `size_bytes` is the entry's already-known size (computed once, at `store` time),
   threaded through so a later `restore` (step 4) never has to re-serialize the
   session to re-derive it (issue #378 S2 follow-up).
2. `validate_categorized_tools` builds a display-name→raw-name lookup
   (`display_to_raw`) before validating any entry, since a caller can only ever echo
   back the *display* form of a tool name `introspect_server` showed it, never the
   raw one. For each introspected tool, both plausible display forms are computed
   (`display_forms`): the fully escaped form actually shown (`sanitize_untrusted_text`
   followed by `&`/`<`/`>` → `&amp;`/`&lt;`/`&gt;` entity-escaping, mirroring
   `wrap_untrusted_block`'s own escaping) and the same text with those entities
   decoded back, since `wrap_untrusted_block`'s preamble explicitly invites the
   reader to do so. If two **distinct** raw tool names collide on the same
   display key under either form, that key is dropped from the lookup
   entirely — genuinely ambiguous, so a caller using it hits "not found" rather
   than silently having one raw tool's categorization misattributed to another's.
   Each `categorized_tools` entry's `name` is resolved through this lookup to a
   raw tool name once; both the duplicate check and the codegen categorization
   map are keyed by that **resolved raw name**, not the submitted string. This
   fixes a categorization-lookup desync (issue #307): an earlier version built
   the categorization map keyed by the submitted display string while codegen
   looked up tools by their raw name, silently dropping category/keywords/
   description for any tool name containing a control character, line
   terminator, or `&`/`<`/`>`. Rejects: more entries than `min(introspected
   count, MAX_TOOL_FILES)` (reusing `mcp_execution_skill::MAX_TOOL_FILES` so this
   stage can never generate more tool files than `generate_skill` will later
   accept — bounded by the true introspected tool count, not by the lookup's
   size, since one raw tool can legitimately own two display keys and an
   ambiguous key is excluded from the map); a name that doesn't resolve to any
   raw tool (unknown, or an ambiguous display key); a name that resolves to a
   raw tool a previous entry in the same call already claimed; any field
   (`name`/`category`/`keywords`/`short_description`) over its own byte cap
   (`MAX_CATEGORIZED_TOOL_NAME_LEN`=128, `MAX_CATEGORY_LEN`=100,
   `MAX_KEYWORDS_LEN`=500, `MAX_SHORT_DESCRIPTION_LEN`=320 — each checked via a
   single private `check_categorized_field_length` helper called once per field).
3. `generate_and_export` runs the rest of the pipeline against the now-owned
   `pending`, **borrowed** (not consumed) so the caller can hand it back on failure:
   resolves and confines `output_dir` fresh, right here — not from any value cached
   on the session — via `output_dir::resolve_output_dir` (see [[#Output directory
   resolution]]); `ProgressiveGenerator::generate_with_categories` →
   `FilesBuilder::from_generated_code(code, "/")` → `vfs.file_count()` captured;
   exports inside `spawn_blocking`, holding a **per-`output_dir`** lock for the
   duration (see [[#Per-resource locking]]).
4. **Any `Err` returned from step 3 - `output_dir` resolution, codegen, VFS build,
   the `spawn_blocking` join, or export - calls `state.restore(session_id, pending,
   size_bytes)` before returning that same error** (issue #379); a result-
   serialization failure after a successful step 3 is treated as unreachable
   (`SaveCategorizedToolsResult`'s fields cannot fail to serialize) and simply
   propagates via `?`, matching this project's convention against handling
   scenarios that can't happen. `restore` re-inserts the session under its original
   `session_id`, `expires_at` (not a fresh TTL), and `size_bytes`, so a transient
   failure anywhere in step 3 - a symlink planted after `take_if` ran, a momentary
   disk-full or permission error during export - is retriable with the same
   `session_id`, the same guarantee step 1 already gives a pre-consume validation
   failure. This covers every ordinary error return, not literally everything that
   could lose the session: a panic inside `generate_and_export`, or the request
   future being dropped mid-await (process shutdown, transport teardown), still
   loses it with no restore, same as any pre-#379 code path. `restore` itself
   enforces the same `MAX_PENDING_SESSIONS`/`MAX_TOTAL_PENDING_BYTES` bounds
   `store` does (issue #379 S1 — see [[#State Management (StateManager)]]); if the
   table is already back at capacity by the time this call's checkout window ends,
   `restore` returns `Err` and the session is genuinely lost — that failure is
   logged (`GeneratorService::restore_or_log`), not surfaced to the client, since
   the pipeline's own error is always the more relevant one to report. Only once
   step 3 fully succeeds, or `restore` runs (successfully or not), is the session's
   fate settled.
5. Does **not** observe request cancellation — a documented, deliberate
   choice: an earlier version raced the *lock wait* (not the export
   itself, which was already excluded) against cancellation, but that
   produced two successive correctness bugs (a leaked lock-table entry, or
   an evicted entry pulled out from under the still-running holder,
   reopening the exact data-loss race the lock exists to prevent) for
   little benefit, since the export itself was never interruptible anyway.

### `list_generated_servers`

`ListGeneratedServersParams { base_dir: Option<String> }` → `ListGeneratedServersResult { servers: Vec<GeneratedServerInfo>, total_servers }`.

`base_dir`, if given, is confined to `servers_base_dir` via
`resolve_list_base_dir` (issue #236's fix — previously an absolute or
escaping `base_dir` silently fell back to the default instead of being
rejected). Confinement is lexical first (`Path::join` + `starts_with`, which
catches even a non-existent target and a Windows root-without-prefix path
like `\pwn\evil`), then, if the joined path exists, re-checked via
canonicalization against the canonicalized root (catches a symlink planted
inside it). The scan itself (`spawn_blocking`, nested `read_dir`) counts
`.ts` files per subdirectory (excluding `_`-prefixed and `_runtime`) and the
subdirectory's mtime as `generated_at`.

### `generate_skill`

Thin MCP-tool wrapper around `mcp_execution_skill::scan_tools_directory` +
`build_skill_context`, observing cancellation via the same
`tokio::select! { biased; ... }` pattern as `introspect_server` (the scan
walks every tool file, so a large directory can take a while). A missing
server directory or scan failure (`MissingMetadata`/`UnsupportedSchema`/
`StaleMetadata`) is reported as `invalid_params` (caller's fault: "run
`generate` first"), not `internal_error`. Non-fatal drift warnings from the
scan (`ScanResult::warnings`) are copied into the structured
`GenerateSkillResult::warnings` field, not just logged.

### `save_skill`

Thin MCP-tool wrapper around `mcp_execution_skill::resolve_skill_output_path`
+ `extract_skill_metadata`. Validates `server_id`, content size
(`MAX_SKILL_CONTENT_SIZE` = 100 KiB), and that content starts with `---`
before the more expensive frontmatter parse. Rejects overwriting an existing
file unless `overwrite: true`. **Deliberately does not observe
cancellation**: `tokio::fs::write` on the blocking-pool cannot actually be
interrupted once queued — racing it against `ct.cancelled()` would make the
response lie (telling a cancelled client the write never happened while it
still lands on disk moments later), which is worse than not attempting
cancellation. The content-size bound (100 KiB) is not what actually caps the
synchronous parse cost — `extract_skill_metadata`'s own
`MAX_FRONTMATTER_SIZE` (8 KiB) on the extracted block is what keeps that
bounded regardless of overall content size, since YAML parsing isn't
linear-time on pathological input.

## 4. State Management (`StateManager`)

Sessions expire after `PendingGeneration::DEFAULT_TIMEOUT_MINUTES` = 30
minutes and are swept **lazily** — only as a side effect of `store`/`take`
(no background timer). Two independent resource-exhaustion bounds, both
required:

- `MAX_PENDING_SESSIONS` = 1000 — structural backstop against unbounded
  `HashMap` growth (and its iteration cost), independent of session size.
- `MAX_TOTAL_PENDING_BYTES` = 4 × a derived per-session estimate
  (`MAX_TOOL_COUNT × (MAX_TOOL_NAME_LEN + MAX_TOOL_DESCRIPTION_LEN + 2 ×
  MAX_SCHEMA_SIZE_BYTES)`, from `mcp-introspector`'s constants) — the count
  cap alone doesn't bound memory, since a single session's real footprint
  can vary by orders of magnitude with tool count; 1000 sessions at a
  worst-case few-hundred-MB each could otherwise reach hundreds of GB.
  Session size is estimated via `serde_json::to_vec(&server_info).len()`
  (a serialization failure is treated as `usize::MAX`, i.e. always
  exceeding any bound, rather than silently under-counting).

`StateManager::store`/`take`/`get`/`take_if`/`restore`/`pending_count`/
`cleanup_expired` all consult the **injected** `Clock`, not `Utc::now()`
directly — verified by a dedicated test that jumps a shared `TestClock` far
past the TTL and asserts every one of those methods observes the same jump.

`take_if` (issue #378) validates a session in place and removes it from the
table only if the caller-supplied closure returns `Ok` — run synchronously
while the write lock is held, so it never pays `get`'s full deep-clone cost
(up to `MAX_SINGLE_SESSION_BYTES`) on a failed attempt, the common case for a
caller retrying a rejected `categorized_tools` payload. On success it hands
back the removed session, its already-known `size_bytes`, and the closure's
output; on failure the session is left untouched at its original expiry,
identically to what a `get`-then-validate-then-`take` sequence would leave
behind, but without ever cloning to get there.

`restore` (`pub(crate)`, issue #379) re-inserts a previously-removed session
(via `take` or a successful `take_if`) under its original `session_id` and
`size_bytes`, preserving its original `expires_at` rather than granting a
fresh TTL. Used by `save_categorized_tools` to undo a `take_if` consumption
when the pipeline step that was supposed to follow it (`output_dir`
resolution, codegen, VFS build, export) fails for a retriable reason — see
step 4 in [[#save_categorized_tools]]. `size_bytes` must be the value
`take_if` (or `store`) already computed for this exact session, so `restore`
never re-serializes it to re-derive a size its caller already had (issue #378
S2). Enforces the identical `MAX_PENDING_SESSIONS`/`MAX_TOTAL_PENDING_BYTES`
bounds `store` does, returning `Err` and dropping `generation` rather than
inserting it if either would be exceeded (issue #379 S1): a session removed
by `take_if` is briefly unaccounted for while its caller's pipeline runs
(the data is still resident in the handler's memory, just not reflected in
`total_bytes`), so without this check a client could refill the freed budget
with concurrent `store`s during that window and have `restore` land on top
of it, parking the table above its configured caps for the rest of the
original session's TTL. `pub(crate)` rather than `pub` because no caller
outside this crate can supply a valid `size_bytes` for an arbitrary
`generation` — `estimate_size_bytes` (private) is the only way to produce
one — and an external caller could otherwise pass an arbitrary `session_id`,
bypassing `store`'s minted-`Uuid` invariant entirely. If `session_id` already
names an entry (only possible via a colliding UUID from a concurrent
`store`), it is silently overwritten rather than rejected, since restoring
the caller's own already-consumed session takes priority over that
astronomically unlikely edge case.

## 5. Per-Resource Locking

Two independent lock tables, both following the identical pattern:

- `introspectors: HashMap<ServerId, Arc<Mutex<Introspector>>>` — a slow or
  hung downstream server only blocks `introspect_server` calls for that
  *same* server id, never unrelated ids. The outer map lock is released
  before the per-id handle is awaited.
- `exports: HashMap<PathBuf, Arc<Mutex<()>>>` — an export for one
  `output_dir` never blocks a different one; holding the per-target lock
  across `FileSystem::export_to_filesystem` serializes two concurrent
  `save_categorized_tools` calls targeting the **same** `output_dir`,
  narrowing (though not eliminating across 3+ overlapping calls) the
  data-loss race `mcp-files`'s own age-gated artifact sweep is the final
  backstop against.

Both tables use **identity-checked eviction** (`Arc::ptr_eq`) after use —
`server_id`/`output_dir` are caller-supplied, so without eviction the map
grows unboundedly; an unconditional `remove` keyed by value alone is a
TOCTOU bug (it could evict a fresh handle a *third*, concurrent caller
already inserted after a *second* caller's own eviction).

Every lock helper (`introspector_for`/`evict_introspector`/`export_lock_for`/
`evict_export_lock`) and every tool handler except `list_generated_servers`
(`introspect_server`/`save_categorized_tools`/`generate_skill`/`save_skill`)
carries a `#[tracing::instrument(skip_all, fields(server_id = ...))]` (or
`output_dir = ...` for the lock helpers) span (issue #211) — this changes the
shape of stderr output (nested spans, structured `server_id`/`output_dir`
fields) but not log message text.

## 6. Output Directory Resolution (`output_dir.rs`)

Mirrors `mcp-skill`'s `resolve_skill_output_path` for a directory target
rather than a file:

- `relative_subpath` (I/O-free) is all `introspect_server` runs — reject
  fast, commit to nothing, create nothing.
- `resolve_output_dir` (filesystem-touching: creates and confinement-checks
  every intermediate directory, symlink-strict at `server_id`'s own
  directory) is called **only from `save_categorized_tools`**, as the first
  step of the post-consume pipeline (see step 3 in
  [[#save_categorized_tools]]) — not once at `introspect_server` time with
  the result cached on the session. Caching it would leave a window (up to
  the full 30-minute session lifetime) in which a symlink planted after
  resolution is never re-checked, and would create directories for any
  `introspect_server` call regardless of whether a matching
  `save_categorized_tools` ever follows. Running it after the session is
  already consumed (rather than before, as an earlier version of this fix
  had it) is safe specifically because a failure here now triggers
  `state.restore` (issue #379) exactly like any other post-consume pipeline
  failure, so it costs nothing in retriability.
- The **final** target directory is confinement-checked but deliberately
  **not created** here — `FileSystem::export_to_filesystem` publishes it
  atomically via a staged rename, and pre-creating it would defeat that
  atomicity on a first-time `generate`.

## 7. Prompt Injection Defense

Any server-reported tool metadata surfaced back to the calling Claude
session — `introspect_server`'s tool summaries — is wrapped via
`mcp_execution_core::untrusted::wrap_untrusted_block` before being returned
as `CallToolResult` text (issue #292's fix), the same primitive
`mcp-skill` uses for its LLM-facing generation prompt.

## 8. Transport & Framing (`main.rs`, binary only)

The binary wires stdio through a **size-bounded** codec
(`JsonRpcMessageCodec::new_with_max_length`) instead of `rmcp`'s default
`stdio()`, since that default reads lines via an unbounded
`BufReader::read_until`. `MAX_REQUEST_LINE_SIZE` = 4 MiB (headroom over the
largest legitimate payload, both well under 1 MiB); actual peak buffer
capacity can reach ~4x that under attack due to `tokio_util`'s
doubling-growth buffer.

Additionally gates **request** admission (not notifications/responses,
which always flow) behind a `Semaphore` of `MAX_CONCURRENT_REQUESTS` = 8,
since `rmcp` 2.2.0 spawns an unbounded `tokio::spawn` per inbound request
with no concurrency knob of its own. A bounded decode-ahead queue (also
capped at 8) lets the stream keep decoding notifications/responses behind
an as-yet-unadmitted request rather than stalling the whole connection
behind it (FIFO admission order preserved). `RecoveringCodec` folds a
malformed/oversized/skipped line into a logged-and-continued outcome rather
than a terminal stream error, closing a `tokio_util` "stranded buffered
request" stall (issue #273) that could otherwise leave a valid,
already-buffered request undelivered forever if it shared a chunk with a
preceding bad line.

The acquired permit for an admitted request is attached to that request's
`Extensions` (`attach_permit`) and released only when `rmcp`'s
`RequestContext` for it is dropped — on handler completion or panic, not on
cancellation alone, since `rmcp`'s own cancel path never aborts the handler
task (issue #227). `RecoveringCodec`'s blank-line handling was later fixed
(issue #284) to peek the next buffered line and silently fold a
blank/whitespace-only line to `DecodedFrame::Skipped` rather than
`Malformed`, avoiding a `tracing::warn!` per blank line — the same
log-volume-amplification class already fixed for the introspector's
symmetric decoder (#275/#282); a genuinely malformed non-blank line still
warns.

## 9. Error Conditions

`StateError`: `AtCapacity { limit }`, `MemoryBudgetExceeded { limit }`.

`OutputDirError`: `InvalidServerId`, `AbsolutePath`, `ParentTraversal`,
`ServerDirIsSymlink`, `Escape`, `NotADirectory`, `CreateDir`, `Io`.

Every tool method maps its internal errors to `rmcp::ErrorData` via either
`McpError::invalid_params` (caller's fault — bad input, missing session,
confinement violation) or `McpError::internal_error` (this project's/the
environment's fault — I/O failure, task join error, serialization failure).

## 10. Cross-Crate Contracts

- **Consumes**: `mcp-introspector::Introspector`/`ServerInfo`,
  `mcp-codegen::ProgressiveGenerator`, `mcp-files::FilesBuilder`/
  `FileSystem`, `mcp-skill::{scan_tools_directory, build_skill_context,
  resolve_skill_output_path, extract_skill_metadata, validate_server_id,
  MAX_TOOL_FILES}`, `mcp-core::{ServerConfig, ServerId,
  sanitize_path_for_error, validate_path_segment, untrusted::*}`.
- **Schema drift guards**: `service.rs`'s own tests assert the `schemars`-
  derived schema for `CategorizedTool`/`SaveCategorizedToolsParams`/
  `IntrospectServerParams` matches the real runtime constants
  (`MAX_CATEGORIZED_TOOL_NAME_LEN` etc., `mcp_execution_skill::MAX_TOOL_FILES`,
  `mcp_execution_core::MAX_ARG_COUNT`/`MAX_ARG_LEN`) byte-for-byte.

## 11. Edge Cases & Notable Behaviors

- `test_introspect_server_concurrent_calls_do_not_cross_contaminate_server_id`
  guards a subtle interaction between rmcp's async-fn span-instrumentation
  heuristic and `#[tool]`'s macro-generated boxing — if that heuristic ever
  stops matching, the `server_id` tracing field silently stops being
  recorded, and this test is the only thing that would catch it.
- `PendingGeneration::is_expired` uses **strict** `>`, so a session checked
  at exactly its `expires_at` instant is *not* yet expired.
- `resolve_list_base_dir`'s confinement check runs even when the joined
  path doesn't exist yet (lexical `starts_with`), matching every other
  confinement check in this crate rather than being the one path that
  skips it.

## 12. See Also

- [[../introspector/spec]] — wrapped by `introspect_server`
- [[../codegen/spec]] / [[../files/spec]] — wrapped by `save_categorized_tools`
- [[../skill/spec]] — wrapped by `generate_skill`/`save_skill`
- [[../cli/spec]] — the CLI-driven alternative path to the same underlying crates
