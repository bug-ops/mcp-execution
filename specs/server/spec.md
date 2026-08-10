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
    pub async fn pending_count(&self) -> usize;
    pub async fn cleanup_expired(&self) -> usize;
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

`get_info()` (`ServerHandler`) advertises protocol version `2025-06-18`,
tools capability enabled, and an `instructions` string describing the
introspect→categorize→save workflow.

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

1. `state.get(session_id)` — a non-consuming peek. The session must exist and not be
   expired, or `invalid_params`. Nothing is removed from `StateManager` yet: both
   validation steps below (2 and 3) run against this peeked copy, so a failure in
   either leaves the session in place, at its original expiry, for the caller
   to retry with the same `session_id` (issue #371). Only step 4 actually consumes
   it, via `state.take(session_id)` — which re-checks existence/expiry against the
   live table and can itself still miss (e.g. a concurrent call for the same
   `session_id` already consumed it between steps 1 and 4), reporting the identical
   `invalid_params` error as step 1's miss.
2. Builds a display-name→raw-name lookup (`display_to_raw`) before validating
   any entry, since a caller can only ever echo back the *display* form of a
   tool name `introspect_server` showed it, never the raw one. For each
   introspected tool, both plausible display forms are computed (`display_forms`):
   the fully escaped form actually shown (`sanitize_untrusted_text` followed by
   `&`/`<`/`>` → `&amp;`/`&lt;`/`&gt;` entity-escaping, mirroring
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
3. **Resolves `output_dir` fresh, right here, still before consuming the session** —
   not from any value cached on the session — via `output_dir::resolve_output_dir`
   (see [[#Output directory resolution]]). Moved ahead of step 4 as part of #371: a
   `ServerDirIsSymlink`/`NotADirectory`/`Escape` failure here is environment-dependent
   and thus retriable, so it gets the same session-preserving treatment as step 2's
   validation instead of destroying the session the way it did before #371.
4. Drops the peeked session (and `display_to_raw`/`seen_raw_names`, which borrow it)
   now that steps 2-3 have both passed, then calls `state.take(session_id)` to
   actually consume it (see step 1). Dropping first bounds peak memory to one live
   session copy instead of two across the codegen/export work below —
   `state.get`'s peek in step 1 is a full deep clone of the session, including every
   introspected tool's schema.
5. `ProgressiveGenerator::generate_with_categories` → `FilesBuilder::from_generated_code(code, "/")` → `vfs.file_count()` captured.
6. Exports inside `spawn_blocking`, holding a **per-`output_dir`** lock for
   the duration (see [[#Per-resource locking]]).
7. Does **not** observe request cancellation — a documented, deliberate
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

`StateManager::store`/`take`/`get`/`pending_count`/`cleanup_expired` all
consult the **injected** `Clock`, not `Utc::now()` directly — verified by a
dedicated test that jumps a shared `TestClock` far past the TTL and asserts
every one of those methods observes the same jump.

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
  directory) is called **only from `save_categorized_tools`**, as the last
  validation step before the session is consumed (see step 3 in
  [[#save_categorized_tools]]) — not once at `introspect_server` time with
  the result cached on the session. Caching it would leave a window (up to
  the full 30-minute session lifetime) in which a symlink planted after
  resolution is never re-checked, and would create directories for any
  `introspect_server` call regardless of whether a matching
  `save_categorized_tools` ever follows.
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
