---
aliases:
  - ADR-369
  - rmcp stateless discover lifecycle adoption
tags:
  - sdd
  - decision
  - introspector
  - rmcp
created: 2026-08-10
status: accepted
review-date: 2026-11-10
related:
  - "[[../constitution]]"
  - "[[../introspector/spec]]"
  - "[[../server/spec]]"
---

# ADR-369: Evaluate Adopting `rmcp`'s SEP-2575 Stateless Discover Lifecycle and SEP-2549 Cache Hints

> [!important]
> This is a decision record, not a feature spec. It documents a research
> evaluation and its outcome; it authorizes no code change. All citations
> below were independently re-measured against vendored `rmcp` 3.1.2 (and
> 2.2.0 where explicitly noted) across two rounds of architect/critic
> review.

## 1. Context

[[#369]] asks whether this workspace should adopt two client-side MCP SEPs
that `rmcp` 3.x exposes but this project does not yet use:

- **SEP-2575** — a stateless discover lifecycle (`server/discover` +
  `ClientLifecycleMode`) that lets a client learn server capabilities
  without a full `initialize` handshake.
- **SEP-2549** — cache-hint metadata (`ttlMs`) that lets a client know how
  long discovered/introspected data stays valid.

This ADR covers only those two findings, referred to below as **A**
(SEP-2575, client side) and **B** (SEP-2549, cache hints). A third finding
surfaced during evaluation — that `mcp-server`'s `GeneratorService` already
accidentally advertises and serves SEP-2575 discover on the *server* side —
is deliberately **out of scope for this decision**; the user confirmed it
should be filed as an independent follow-up issue rather than folded into
this ADR. It is retained here only as an appendix (§6) with enough detail
for that follow-up to be filed accurately.

Relevant call sites: `Introspector::discover_server()`
(`crates/mcp-introspector/src/lib.rs`, see
[[../introspector/spec]]) is the only place this workspace would wire A;
`GeneratorService` (`crates/mcp-server/src/service.rs`, see
[[../server/spec]]) is where C's already-live behavior lives.

## 2. Options Considered

### A. Stateless discover lifecycle (SEP-2575), client side

**(a) Adopt now** (`ClientServiceExt::serve_with_lifecycle(t,
ClientLifecycleMode::Auto { .. })` at both `().serve(...)` call sites,
`crates/mcp-introspector/src/lib.rs:1060` and `:1121`)

- **Pros:** forward-compatible with any future server that implements
  discover; the mechanism itself is already vendored and stable.
- **Cons:** measured as a **net latency regression** against the actual
  server population (§4.1), not a saving; introduces a new silent-failure
  mode (§4.1, risk 1); a narrow fallback trigger (§4.1, risk 2); and
  compresses the existing timeout budget (§4.1, risk 3) — all against a
  population that is overwhelmingly legacy-only servers today.

**(b) Adopt now behind an opt-in flag**

- **Pros:** zero blast radius until enabled.
- **Cons:** an untested path nobody enables is exactly the
  "capability wired to no caller" anti-pattern [[#369]] itself is
  investigating (see issues #180/#185/#191/#195/#199/#202,
  [[../constitution]]). Rejected on that basis alone.

**(c) Adopt later, behind an executable CI gate — recommended**

- **Pros:** costs nothing now; ties the revisit trigger to an rmcp-version
  signal that changes automatically via dependabot, rather than to a
  manually-tracked date; keeps the integration shape (§5) documented so
  the swap stays cheap whenever taken.
- **Cons:** requires a gate specific enough not to rot silently (§5) — a
  bare "wait and see" was rejected for the same reason ADR-341 rejected it
  for its own monitored decision.

**Decision: (c).**

### B. Cache hints (SEP-2549) in `Introspector`

**(a) Add TTL persistence to the `Introspector` cache now**

- **Pros:** would align with the SEP once `rmcp` negotiates protocol
  2026-07-28.
- **Cons:** the premise is false today (§4.2) — `ttlMs` is populated only
  from protocol 2026-07-28 (`model.rs:1601-1608`), this project currently
  negotiates 2025-11-25, and the `Introspector` cache this would extend
  has **zero production readers** to begin with. Persisting the field
  today would persist an always-absent value onto a structure nothing
  reads.

**(b) Reject as framed; re-scope the real gap — recommended**

- **Pros:** correctly identifies that the actual gap `B`'s premise was
  reaching for — "nothing signals whether generated output is stale" — is
  real, but lives in `~/.claude/servers/{id}/_meta.json`
  (`crates/mcp-core/src/metadata.rs`), not in `Introspector`'s cache.
  `_meta.json` records no timestamp, config fingerprint, or tool-list
  digest today, so nothing can even *ask* whether it is stale — a
  protocol-independent problem `ttlMs` would only partially and
  conditionally solve.
- **Cons:** reframing risks reading as scope-dodging if not paired with
  filing the re-scoped follow-up (§7, item 1) in the same pass.

**Decision: (b).**

## 3. Decision

- **A: adopt later**, gated on the CI assertion in §5. Not adopted now,
  not adopted behind a dormant opt-in flag.
- **B: reject as framed.** No cache/TTL work lands against `Introspector`.
  The real gap — generation provenance in `_meta.json` — is re-scoped into
  a separate follow-up (§7, item 1).
- **Structure**: `workspace`, unchanged. No new crates, no new public
  types, no files created by this decision beyond this ADR itself.

## 4. Evidence Ledger

All citations below were independently re-verified against vendored
`rmcp-3.1.2` sources (and `rmcp-2.2.0` where explicitly marked) across two
rounds of architect/critic review; two citation errors and one framing gap
caught in the second round are corrected in place below rather than left
for a reader to trip over.

### 4.1 Finding A — cost/benefit and risks

**Round-trip cost, by peer type**, against the actual `~/.claude/mcp.json`
population:

| Peer | Round trips under `Auto` | vs. legacy today |
|---|---|---|
| Legacy server (the entire real population today) | discover → `-32601`, then `initialize` → result | **+1** |
| Discover-capable server | discover → result | ~0 (saves only the `initialized` notification write) |
| Server rejecting the declared version | + one extra discover per version retry (`service/client.rs:884-914`) | **+N** |

Adopting A today buys forward compatibility at the price of an extra round
trip on every discovery, against a population where the fallback path is
the only path actually taken.

**Risks, in weight order:**

1. **New silent-output failure mode.** `DiscoverResult` carries no
   required `server_info` field; it is read from optional
   `_meta["io.modelcontextprotocol/serverInfo"]` (`model.rs:1202-1294`).
   Under today's legacy handshake this cannot happen —
   `From<InitializeResult> for ServerPeerInfo` sets `server_info: Some(..)`
   unconditionally (`model.rs:1136-1145`) from a required field — so
   `extract_peer_meta`'s inner fallback
   (`crates/mcp-introspector/src/lib.rs:1306`) is **unreachable in
   production today**. Adopting A is what makes it reachable: a
   discover-capable non-`rmcp` server that omits that `_meta` key silently
   degrades `_meta.json`'s `server_name`/`server_version` to the raw
   command string and `"unknown"`, which then flows into generated
   `SKILL.md`. This is a genuinely new defect introduced by adoption, not
   a pre-existing bug it would merely expose.
2. **Narrow fallback trigger.** `ClientLifecycleMode::Auto` falls back to
   the legacy handshake **only** on JSON-RPC `-32601` (`METHOD_NOT_FOUND`),
   at `service/client.rs:743`. Any server that answers an unknown
   pre-initialize request with a different error code, no response, or by
   closing stdout turns what would have been working discovery into
   `ConnectionFailed`/`Timeout` instead. The target population is
   arbitrary user-configured servers from `~/.claude/mcp.json`, not a
   controlled fleet.
3. **Timeout budget compression.** The single 30 s `connect_timeout`
   (`crates/mcp-core/src/server_config.rs:62`) would need to cover
   discover + N version retries + the legacy `initialize` fallback, where
   today it covers one `initialize` call. Same wall-clock budget, strictly
   more work inside it.
4. `discover_startup` errs `NoPreferredProtocolVersion` on an empty
   version list — a footgun only if the lifecycle knob is ever made
   user-configurable, not a risk of the integration shape below as
   proposed.

**Integration shape, if and when A is taken:** a `ServerConfig` lifecycle
knob defaulting to legacy, threaded into `connect_and_list_tools`; only the
two `().serve(...)` call sites change (`lib.rs:1060`, `lib.rs:1121`),
swapping to `ClientServiceExt::serve_with_lifecycle(t,
ClientLifecycleMode::Auto { .. })`, plus a `tracing::warn` when discover
yields no `server_info` so risk 1 above is observable instead of silent.

### 4.2 Finding B — cache hints

- The premise — "a staleness signal `ServerInfo`/`ToolInfo` caching lacks"
  — rests on a cache with no production reader. The write path is live
  (`lib.rs:431` inserts on every discover), so the accurate framing is a
  **write-only cache**: `get_server`/`list_servers`/`server_count`/
  `remove_server`/`clear` have zero non-test callers workspace-wide. Every
  production `Introspector` is construct → one `discover_server` → drop
  (CLI: `introspect.rs:199`, `generate.rs:180`; `mcp-server`:
  `service.rs:356/499/665`, which evicts unconditionally at
  `service.rs:341`).
- `rmcp` 3.1.2 already implements SEP-2549 client-side
  (`service/client/cache.rs`: a per-`Peer` cache, `ClientCacheConfig`
  defaulting `enabled: true`, TTL-driven, invalidated on
  `notifications/tools/list_changed`). This project gets that
  implementation for free via the dependency bump, and it is equally inert
  for the same structural reason as this project's own cache.
- `ttlMs: Option<u64>` is populated only from protocol 2026-07-28
  (`model.rs:1601-1608`); this project negotiates 2025-11-25;
  `ClientCacheConfig.default_ttl` is `ZERO`. Persisting it today would
  persist an always-absent field.
- The real underlying gap: `~/.claude/servers/{id}/` is frozen after
  `generate`, and `_meta.json` (`crates/mcp-core/src/metadata.rs`) records
  no timestamp, no config fingerprint, no tool-list digest — nothing can
  even *ask* whether it is stale. The protocol-independent fix is
  generation provenance in `_meta.json` plus a comparison command;
  `ttlMs` would later become one optional input to that, not the
  mechanism itself. Out of scope here → §7, item 1.

## 5. Measurable Gate for Revisiting A

The gate must be an **executable, CI-failing assertion**, not an inferred
or manually-tracked condition: `rmcp` version bumps arrive via dependabot
with zero source changes to this workspace, and no workspace code
currently references `ProtocolVersion::LATEST` or `KNOWN_VERSIONS`, so a
protocol promotion would land green and go unnoticed without one.

**Gate condition** (lives in `mcp-introspector`, A's concern): a test
asserting

```rust
assert_eq!(rmcp::model::ProtocolVersion::LATEST, rmcp::model::ProtocolVersion::V_2025_11_25);
```

fails the moment `rmcp` promotes `LATEST` to `V_2026_07_28`.

**What a failing assertion means — and does not mean.** A red assertion is
a **trigger to re-open the adoption discussion for A**, nothing more. It
does not itself authorize implementing A, and it must not be read as
"gate condition met, proceed." The benefit side of A's cost/benefit (§4.1)
— whether servers in the actual population have started answering discover
— is unmeasured by this assertion and must be re-assessed at that point,
not assumed. Treat a red CI run as "schedule the re-evaluation," not as a
green light to ship the integration shape in §4.1 unreviewed.

**Placement note — why the gate contains only this one assertion.** An
earlier draft of this decision also considered pinning
`supported_protocol_versions()` — the exact version set `mcp-server`'s
`GeneratorService` advertises — as a second gate condition, since it is
equally cheap and would double as a characterization test. That was
rejected: `KNOWN_VERSIONS` changing does not make A more or less worth
adopting, so including it would make the gate non-atomic — a failure could
fire the "revisit A" signal for a reason unrelated to A. It would also
reintroduce, inside this ADR's gate, exactly the A/C scope split the user
already resolved by keeping C's server-side finding out of this document
(§6): prescribing a characterization test for `mcp-server` is a C
recommendation regardless of which document it lives in. That assertion
belongs in C's own follow-up issue instead (§7, item 2). If that follow-up
is deprioritized, the server-side drift stays unguarded in the interim —
an accepted, explicitly-noted gap, not an oversight.

`review-date: 2026-11-10` (three months from this ADR's creation date) is
recorded as a weak backup to the assertion above, not a substitute for it.

## 6. Appendix — Finding C (informational only; not part of this ADR's decision)

Retained solely so a follow-up issue can be filed accurately (§7, item 2).
**No recommendation here is part of this ADR's decision content**, per the
user-confirmed scope split in §1.

- **Mechanism.** `GeneratorService` implements `ServerHandler`
  (`crates/mcp-server/src/service.rs:1086`) without overriding
  `supported_protocol_versions()`, so `rmcp`'s default returns
  `KNOWN_VERSIONS` — all five entries including `V_2026_07_28`
  (`model.rs:181-187`) — and `rmcp`'s default
  `ServerHandler::discover` (`handler/server.rs:343`) answers
  `server/discover` requests with that full list.
- **Corrected dating.** "Live since the recent dependency bumps" is false
  for the version-echo half of this behavior: `rmcp` **2.2.0** already
  carried `V_2026_07_28` in `KNOWN_VERSIONS`
  (`rmcp-2.2.0/model.rs:170-176`), and its `negotiate_protocol_version` was
  hardcoded to `KNOWN_VERSIONS` with no override hook —
  `supported_protocol_versions` has **zero** occurrences anywhere in
  `rmcp` 2.2.0. What is new in 3.x is the override point itself, plus
  `server/discover` and inline pre-initialize handling.
- **`initialize` is the larger surface, not discover.** `get_info()`'s
  `V_2025_06_18` (`service.rs:1089`) is a **fallback**, used only when a
  client requests a version `rmcp` doesn't recognize
  (`service/server.rs:469-484`, `rmcp` 3.1.2) — not a cap. Any client
  requesting protocol 2026-07-28 gets it echoed back today via
  `initialize`, entirely independent of whether discover is ever used.
- **Narrowing `supported_protocol_versions()` is not a valid opt-out.**
  `handle_request` dispatches `DiscoverRequest` unconditionally
  (`handler/server.rs:108-110`; the neighboring `104-107` range is the
  `InitializeRequest` arm, not this one). The version gate
  (`handler/server.rs:65-71`) rejects only an unsupported *declared*
  version — after which an `Auto` client simply retries at a lower
  version and proceeds statelessly regardless.
- **A behavior-change fix would actively brick sessions.** The only way to
  force an `Auto` client to fall back today is overriding `discover()` to
  return `-32601`. But `require_request_metadata()` is a sticky
  `AtomicBool` (`service.rs:1032-1035`, no clearing path anywhere) set
  before the first non-initialize dispatch (`service/server.rs:541`).
  After the client's legacy `initialize` fallback succeeds, every
  subsequent request trips `requires_request_metadata`
  (`handler/server.rs:75-97`) → `invalid_params`: a self-inflicted bricked
  session. This rules out "just make discover fail" as a remedy.
- **Therefore the only sound remedy is characterize-and-test.** Pin the
  current `server/discover` response and the `initialize` negotiation
  outcomes in tests; do not change the advertised protocol version set
  without a separate, affirmative decision of its own. Suggested severity:
  `P2` — live, unaudited protocol drift, not a broken behavior.

## 7. Out-of-Scope Follow-Ups (file separately, per this project's convention)

1. **`_meta.json` generation provenance + staleness check** — the real
   content of finding B (§4.2): add a timestamp, config fingerprint, and
   tool-list digest to `crates/mcp-core/src/metadata.rs`'s `_meta.json`
   schema, plus a comparison command. `_meta.json` `schema_version: 1` may
   be bumped for this, pre-1.0.
2. **Finding C** — characterize-and-test `mcp-server`'s protocol
   advertisement (§6). Includes the `supported_protocol_versions()` pin
   that was deliberately excluded from this ADR's gate (§5). Suggested
   label `P2`. File before or together with this ADR's commit, so the
   caveat noted in §5 does not go unaddressed.
3. **Issue #226 revisit is now actionable, and its docs are stale.**
   `crates/mcp-introspector/src/lib.rs:369-378` and
   [[../introspector/spec]] (its `rmcp`-version-gated HTTP transport
   section) still describe `rmcp` 2.2.0 and say "revisit once 3.0.0 stable
   ships," while the lockfile is at 3.1.2 and
   `StreamableHttpClientTransportConfig::max_sse_event_size` now exists
   and is **not** set by `discover_via_http` (`lib.rs:1113-1115`).
4. **Spec drift.** [[../server/spec]]'s statement that `get_info()`
   "advertises protocol version `2025-06-18`" is misleading per §6's
   corrected understanding — it is a fallback value, not the advertised
   or negotiated ceiling. `crates/mcp-server/tests/integration_tests.rs:27`
   (`test_service_info_has_correct_capabilities`) pins `get_info()` in
   isolation — a green test guarding an incomplete picture of what the
   server actually negotiates.
5. **`Introspector`'s five dead accessors** (§4.2:
   `get_server`/`list_servers`/`server_count`/`remove_server`/`clear`) —
   wire a caller or delete outright; pre-1.0, no deprecation cycle needed.

## 8. Open Questions

1. **Whether any server in `~/.claude/mcp.json` actually answers
   `server/discover`** was not surveyed and is **not blocking** this
   decision: the live exposure this evaluation cares about is server-side
   (finding C, out of scope here), and the gate in §5 is an assertion
   tied to `rmcp`'s own version promotion rather than a one-time survey.
   Keep any such survey as optional supporting evidence only, not a
   prerequisite to revisiting A.

## 9. See Also

- [[../constitution]] — layered-workspace and capability-wired-to-no-caller
  principles this evaluation applies (§2, option A(b); §4.2)
- [[../introspector/spec]] — `Introspector::discover_server()`, the call
  site A would modify, and the `rmcp`-version-gated HTTP transport section
  flagged as stale in §7, item 3
- [[../server/spec]] — `GeneratorService::get_info()`, the subject of §6
  and the spec drift noted in §7, item 4
- ADR-341 — prior art for this project's "monitor and revisit at a
  measurable gate" decision shape, reused here for finding A
