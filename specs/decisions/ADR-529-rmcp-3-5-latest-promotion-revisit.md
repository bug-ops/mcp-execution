---
aliases:
  - ADR-529
  - rmcp 3.5 LATEST promotion revisit
tags:
  - sdd
  - decision
  - introspector
  - rmcp
created: 2026-09-30
status: accepted
supersedes-in-part: ADR-369
review-date: 2027-01-30
related:
  - "[[../constitution]]"
  - "[[../introspector/spec]]"
  - "[[../server/spec]]"
  - "[[ADR-369-rmcp-stateless-lifecycle-adoption]]"
---

# ADR-529: Revisit ADR-369 Finding A After rmcp 3.5.0 Promotes `ProtocolVersion::LATEST`

> [!important]
> This is a decision record, not a feature spec. It replaces the gate defined
> in [[ADR-369-rmcp-stateless-lifecycle-adoption]] §5 (and only that part) and
> records a deliberate acceptance of a wire-behavior change. Citations were
> checked against vendored `rmcp` 3.4.0 and 3.5.0 sources.

## 1. Context

The Dependabot group bump in #527 is titled `rmcp` 3.4.1, but its lockfile
resolves `rmcp`/`rmcp-macros` to **3.5.0** (with `thiserror` 2.0.21). rmcp
3.5.0 (upstream `modelcontextprotocol/rust-sdk#1105`) moves
`ProtocolVersion::LATEST` from `2025-11-25` to `2026-07-28` and adds
`NO_INITIALIZE`, `LATEST_WITH_INITIALIZE` (`2025-11-25`) and
`has_initialize()`. This turns
`test_adr_369_protocol_version_latest_gate` red, which ADR-369 §5 defines as
the trigger to re-open finding A (adopting the SEP-2575 discover lifecycle).
Tracked in #529.

Separately, the bump silently changes the wire: `Introspector` connects with
`().serve(...)`, whose default client config uses `ProtocolVersion::default()`
(= `LATEST`), so every `initialize` now declares `2026-07-28` instead of
`2025-11-25`. No test observed this.

## 2. Options

- **(a) Keep A deferred; accept the new `initialize` default; re-gate —
  chosen.**
- **(b) Adopt A now** (`ClientLifecycleMode::Auto`): zero measured benefit
  (§4 item 5) and a worse timeout cost (§4 item 4).
- **(c) Pin the handshake version** to `LATEST_WITH_INITIALIZE` via an explicit
  `ClientConfig` at both `serve` sites: restores the pre-bump request but adds
  connection-code changes and a permanent divergence from rmcp's default.
  Rejected by the owner; revisit under the triggers in §6.
- **(d) Pin `rmcp` below 3.5.0**: blocks upstream fixes (#1281/#1295/#1300)
  for no gain; the gate did its job.

## 3. Decision

- A stays **deferred**; no discover lifecycle is adopted.
- The `initialize` handshake now declaring `2026-07-28` is **accepted
  deliberately**, tracking rmcp's default, with the failure classes in §5. No
  pin, no source change to `Introspector` connection code.
- The ADR-369 §5 gate is replaced by the gate in §6.

## 4. Evidence

1. `ProtocolVersion::LATEST = V_2026_07_28`; `NO_INITIALIZE = V_2026_07_28`;
   `LATEST_WITH_INITIALIZE = V_2025_11_25` (`rmcp-3.5.0/src/model.rs:177-232`).
2. Silent wire change: `Default for ProtocolVersion` returns `LATEST`
   (`model.rs:157`), `ClientConfig::default()` uses it (`model.rs:1440-1448`, an
   `impl Default for InitializeRequestParams`, which `ClientConfig` aliases), and `impl ClientHandler for ()` uses `ClientConfig::default()`
   (`handler/client.rs:277,296`); `serve_client` hardcodes
   `ClientLifecycleMode::Initialize` (`service/client.rs:693-709`).
3. rmcp servers, including `mcp-server`'s `GeneratorService`, never echo
   `2026-07-28` via `initialize`: `negotiate_protocol_version` requires
   `has_initialize` (`service/server.rs:486-497`; the same logic existed in
   3.4.0 as `is_legacy_version`). ADR-369 §6's claim that a client requesting
   `2026-07-28` gets it echoed via `initialize` held at 3.1.2 and is false
   since 3.2.0. `mcp-server` behavior is unchanged (`get_info` pins
   `2025-06-18`, `service.rs:1207`).
4. Since rmcp 3.2.0 (unchanged through 3.5.0), `Auto` classifies any
   correlated non-modern error as legacy (`is_modern_rejection_code`,
   `service/client.rs:861` in 3.5.0) and has a 10 s
   `DEFAULT_AUTO_DISCOVER_TIMEOUT` fallback (`service/client.rs:657,833-840`).
   rmcp 3.4.1 (#1288) adds only an HTTP-transport change: JSON 4xx discover
   rejections keep the server's JSON-RPC error and are re-correlated
   (`transport/streamable_http_client.rs:365-401`). Hence ADR-369 §4.1 risk 2 (fallback only on `-32601`) is obsolete, risk 3
   (budget compression) worsens, risk 1 (optional `serverInfo`) is unchanged.
5. Benefit side, measured against the only real server in `~/.claude.json`
   (`@sveltejs/mcp`): `server/discover` returns `-32602` (not
   discover-capable); `initialize(2026-07-28)` returns `2025-06-18`.
   Discover-capable servers: 0 of 1. A remains a pure +1 round-trip cost.

## 5. Accepted Cost

Per server class, for the `initialize` declaring `2026-07-28`:

- Spec-compliant server that does not know it and answers with its own
  version: no change (`@sveltejs/mcp`; every rmcp >= 3.2 server).
- Strict server that rejects an unknown requested version instead of
  counter-offering: `ConnectionFailed`. A regression vs rmcp 3.4 only for
  strict servers that know `2025-11-25` but not `2026-07-28`; strict servers
  older than that already failed.
- **Known exposure (not live-probed):** rmcp 3.0.0-3.1.x servers list
  `2026-07-28` in `KNOWN_VERSIONS` and echo any supported requested version
  (`rmcp-3.1.2` `service/server.rs:469-483`), including our own
  `mcp-execution-server` v0.10.0 (locked to rmcp 3.1.2). Against such a
  server this build's `initialize` is answered with `2026-07-28`; rmcp then
  seeds per-request `_meta` and uses modern-lifecycle semantics for the rest
  of the session (`service/client.rs:924-934,274-286,327`). A server that
  ignores that `_meta` works; one that rejects it fails at `tools/list`.
- Initialize-less server (`>= 2026-07-28` only): `ConnectionFailed`, the same
  as before; the cost of deferring A.
- 10 s `Auto` discover cost: not applicable while A is deferred.

## 6. Gate and Re-open Triggers

- `test_adr_529_protocol_version_latest_gate`
  (`crates/mcp-introspector/src/lib.rs`) asserts
  `ProtocolVersion::LATEST == V_2026_07_28` and fires on the next promotion.
- `test_discover_server_http_handshake_is_initialize_at_latest`
  (`crates/mcp-introspector/tests/http_transport_test.rs`) asserts the
  handshake is an `initialize` request declaring `ProtocolVersion::LATEST`
  (not a literal, so it stays atomic with the gate). stdio is covered only by
  the identical `().serve` call; accepted for MVP.
- Re-open A, or the handshake-version pin, when: (1) the gate goes red; (2) a
  server in the configured population rejects our `initialize` (either failure
  class in §5) or advertises only no-initialize versions; (3) any server in the
  population answers `server/discover` (re-run the probe at the review date).
- `review-date: 2027-01-30` is a weak backup, not a substitute.

## 7. See Also

- [[ADR-369-rmcp-stateless-lifecycle-adoption]] — original evaluation; its §5
  gate is superseded here, the rest stands as amended
- [[../introspector/spec]] — handshake description (§3)
- [[../server/spec]] — `get_info()` and version negotiation
