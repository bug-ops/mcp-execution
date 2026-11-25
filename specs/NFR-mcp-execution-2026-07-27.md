---
aliases:
  - mcp-execution NFR
  - mcp-execution Quality Requirements
tags:
  - nfr
  - requirements/non-functional
  - mcp
  - codegen
  - status/draft
created: 2026-07-27
project: "mcp-execution"
status: draft
standard: "ISO/IEC 25010:2011"
related:
  - "[[BRD-mcp-execution-2026-07-27]]"
  - "[[SRS-mcp-execution-2026-07-27]]"
---

# mcp-execution: Non-Functional Requirements Specification

> [!abstract]
> Quality attribute requirements for **mcp-execution**, workspace version
> `0.11.0`. Based on ISO/IEC 25010:2011. Traceable to
> [[BRD-mcp-execution-2026-07-27]]. Every target below is either (a) a
> literal constant already enforced in the codebase, (b) a measured figure
> already published in `README.md`, or (c) explicitly marked as **not
> enforced/not measured** where the repository asserts an intention (a doc
> comment target) without a corresponding CI gate. No numeric target in this
> document was invented for the occasion.
>
> Two exceptions: [[#11. Planned Non-Functional Requirements]] records
> requirements of the direction adopted on 2026-11-24 (planned, unenforced and
> unmeasured, some values open), and [[#12. 1.0 Contract Requirements]] records
> the API, wire-format, command-line, MSRV and release-pipeline contract that
> 1.0.0 promises (target requirements, with explicit open decisions).

## 1. Introduction

### 1.1 Purpose

This document specifies the non-functional (quality) requirements for
`mcp-execution`. It complements [[SRS-mcp-execution-2026-07-27]], which
covers functional requirements.

### 1.2 Scope

Covered: Performance Efficiency, Reliability, Security, Maintainability,
Portability/Compatibility (CLI cross-platform + protocol interoperability).
Explicitly out of scope, with justification: Usability (accessibility/
internationalization — no GUI, no localization exists), Compatibility's
browser/mobile sub-areas (not applicable to a CLI/library), and any
service-level Availability target (no hosted component exists).

### 1.3 Definitions

| Term | Definition |
|------|-----------|
| SLA | Service Level Agreement |
| RTO | Recovery Time Objective |
| RPO | Recovery Point Objective |
| P95 | 95th percentile |
| CWE-400 | Common Weakness Enumeration 400, Uncontrolled Resource Consumption |
| MSRV | Minimum Supported Rust Version |

### 1.4 References

- [[BRD-mcp-execution-2026-07-27]] — Business Requirements Document
- [[SRS-mcp-execution-2026-07-27]] — Software Requirements Specification
- ISO/IEC 25010:2011 — Systems and software Quality Requirements and Evaluation
- [[constitution]] — reverse-engineered project principles (source for most
  Security and Maintainability targets below)

### 1.5 Priority and Trade-offs

> [!tip] Quality Attribute Priority
> Inferred from the *volume and depth of documented engineering effort*
> actually found in the codebase (doc comments, dedicated tests, issue
> references), not from a stated priority order — no such order is written
> down anywhere in the repository.
> 1. **Security** — by far the most heavily invested area; see
>    [[constitution#V. Security]] ("the dominant concern of this codebase").
> 2. **Reliability** (specifically: crash-safety of filesystem export, and
>    fail-closed behavior under resource-bound violations).
> 3. **Performance** — the project's stated value proposition (token
>    savings), and the only area with numeric targets published in
>    `README.md`.
> 4. **Maintainability** — extensive test/doc-comment discipline, but
>    without an enforced numeric coverage gate (see
>    [[#7.2 Testability]]).

## 2. Performance Efficiency

### 2.1 Time Behaviour

| ID | Requirement | Target | Measurement | Conditions |
|----|------------|--------|-------------|-----------|
| NFR-PERF-001 | Progressive-loading code generation latency | <100ms for 10 tools; <20ms for 50 tools | `criterion` benchmark (`crates/mcp-codegen/benches/code_generation.rs`); reported achieved: 0.19ms (10 tools, 526x under target), 0.97ms (50 tools, 20.6x under target) | Local benchmark run, single process, per `README.md`'s Performance table |
| NFR-PERF-002 | Virtual filesystem export latency | <10ms (README); <50ms for a 30-file export (crate doc comment, `crates/mcp-files/src/filesystem.rs`) | `criterion` benchmark (`crates/mcp-files/benches/filesystem_export.rs`); reported achieved: 1.2ms (8.3x under the README target) | Local benchmark run |
| NFR-PERF-003 | Per-tool token footprint of a loaded generated file | ~500–1,500 tokens/tool | Not an automated measurement — a documented estimate in `README.md`/`specs/README.md`, not re-derived by any test or benchmark in this repository. **Revised 2026-11-24:** this estimate is not a requirement. **Measured 2026-11-24 (SDD-3, #562):** the `stats` command reports bytes and estimated tokens (bytes / 4, rounded up, labelled an estimate; [[decisions/ADR-562-context-cost-estimator]]). Fixture server, 9 tools: recorded definitions 1898 B / 475 tokens, all tool files 5250 B / 1313 tokens, one tool file (mean) 583 B / 146 tokens, `catalog.ts` 1899 B / 475 tokens, output schema declared for 1 of 9. The harness `crates/mcp-cli/tests/context_cost_harness.rs` compares K calls as one `call` process each (A) against one `run` script (B); bytes A / B at K = 1, 2, 3, 5, 10, 20: 68 / 275, 136 / 283, 204 / 291, 340 / 307, 680 / 348, 1380 / 438. B is smaller from K = 5 (A is about 68 B per call, B about 8 B per call plus about 267 B fixed). The script text is fixed, so the cost of a model writing the script is not counted; the figures come from one synthetic fixture and do not generalise | `stats` command and the context-cost harness, both offline and deterministic |

> [!warning] Assumptions
> The headline "98% token savings" figure (`README.md`) and the "83%
> asymptotic maximum" figure (`CHANGELOG.md` `[0.2.0]` "Notes") are **not**
> reconciled anywhere in the repository, and neither is backed by an
> automated, reproducible benchmark measuring actual end-to-end token
> consumption in a real Claude Code session. Both NFR-PERF-003 and the
> savings percentage itself should be treated as documented estimates, not
> verified NFRs. **Decision (2026-11-24):** the 98% and 83% figures are
> retired; a reproducible measurement is planned under #562 — see
> [[BRD-mcp-execution-2026-07-27#Open Questions]].

### 2.2 Resource Utilization

| ID | Requirement | Target | Measurement |
|----|------------|--------|-------------|
| NFR-PERF-010 | Memory usage generating 1000 tools | < 256 MB (README target) | Reported achieved: ~2 MB (`README.md` Performance table); also profiled via a dedicated `dhat`-based heap-profiling example (`crates/mcp-files/examples/profile_memory.rs`) |
| NFR-PERF-011 | Bounded resource consumption per untrusted input | See CWE-400 bound table in [[#4.1 Confidentiality]]/[[#4.3 Integrity]] below | Unit tests at each bound's exact boundary value (`MAX_TOOL_COUNT`, `MAX_SCHEMA_SIZE_BYTES`, `MAX_GENERATED_FILES`/`MAX_GENERATED_BYTES`, `MAX_EXPORT_FILES`/`MAX_EXPORT_BYTES`, `MAX_PENDING_SESSIONS`/`MAX_TOTAL_PENDING_BYTES`, `MAX_REQUEST_LINE_SIZE`/`MAX_RESPONSE_LINE_SIZE`) |

### 2.3 Capacity

| ID | Requirement | Target | Growth |
|----|------------|--------|--------|
| NFR-PERF-020 | Tools discoverable per server | ≤ 1000 (`MAX_TOOL_COUNT`) | Fixed constant, not currently configurable at runtime |
| NFR-PERF-021 | Concurrent in-flight MCP requests (server binary) | ≤ 8 (`MAX_CONCURRENT_REQUESTS`) admitted at once; a bounded decode-ahead queue (also 8) lets notifications/responses keep flowing behind an unadmitted request | Fixed constant; chosen because `rmcp` 2.2.0 spawns an unbounded task per inbound request with no concurrency knob of its own |
| NFR-PERF-022 | Concurrently pending introspection→categorization sessions | ≤ 1000 (`MAX_PENDING_SESSIONS`), further bounded by aggregate estimated bytes (`MAX_TOTAL_PENDING_BYTES`) | Fixed constants; no growth/scaling policy exists (this is a single local process, not a scaled service) |

> [!note] Not Applicable
> There is no "requests per second" or "data volume per year" NFR — this is
> a single-user, locally-run tool invoked per developer action, not a
> service under sustained external load. Applying an RPS-style capacity
> target would misrepresent the system's actual operating model.

## 3. Reliability

### 3.1 Availability

> [!note] Not Applicable
> No hosted, always-on component exists. `mcp-execution-server` runs for the
> lifetime of a single local Claude Code session over stdio; there is no
> uptime SLA, no monitoring window, and no planned-maintenance concept to
> specify.

### 3.2 Fault Tolerance

| ID | Requirement | Behaviour |
|----|------------|-----------|
| NFR-REL-010 | A hung/unreachable target MCP server during introspection | Bounded by `connect_timeout`/`discover_timeout` (default 30s each, max 600s); fails with `Error::Timeout`, never blocks indefinitely; a per-server-id lock ensures only operations against that *same* server id are affected, never unrelated ones |
| NFR-REL-011 | A malformed/oversized/skipped line on a bounded stdio stream (either direction this project controls) | Dropped and logged at WARN, not treated as a fatal stream error (`RecoveringCodec`); the request it was answering then times out normally rather than surfacing a distinct, uncorrelatable size-limit error |
| NFR-REL-012 | An MCP server's own tool metadata drifting from a previously generated sidecar | Detected and surfaced (fails loudly for a missing referenced file, warns non-fatally for an unreferenced extra file) rather than silently generating documentation from stale data — see SRS FR-016 |
| NFR-REL-013 | A single component (one target MCP server, one output directory) failing or being slow | Failure/slowness is scoped to that one identity via per-resource locking (introspector cache keyed by `ServerId`, export lock keyed by output directory); does not degrade unrelated concurrent operations |

### 3.3 Recoverability

| ID | Requirement | Target |
|----|------------|--------|
| NFR-REL-020 | Filesystem export crash recovery | A process killed at any point before a staged export is published leaves the previous target directory intact or a recoverable staging/displaced-backup artifact — never a partially-written target. A later export's stale-artifact sweep (age-gated at `STALE_ARTIFACT_MIN_AGE` = 5 minutes) reclaims true crash leftovers without touching a genuinely concurrent sibling export |
| NFR-REL-021 | Export atomicity granularity | Per-top-level-group (one server's own subtree), not whole-batch across multiple servers sharing one output root — a documented, accepted scope, not a defect |
| NFR-REL-022 | Backup / data-loss window | No conventional backup exists (this is generated, reproducible output, not user-authored data); the one accepted residual risk is a process killed **between** the two renames of `swap_into_place`, leaving the target transiently absent until the next export's sweep reclaims the displaced original — accepted as a louder failure (a visibly missing directory) than the silent broken-import bug this design replaces |

> [!note]
> Traditional RTO/RPO framing (minutes-based recovery targets, scheduled
> backup frequency) does not map cleanly onto a tool whose "data" is
> regenerable, deterministic build output. The entries above restate the
> actual, code-enforced guarantees in the closest applicable ISO 25010
> sub-characteristic rather than forcing an artificial RTO/RPO number.

## 4. Security

### 4.1 Confidentiality

| ID | Requirement | Implementation |
|----|------------|---------------|
| NFR-SEC-001 | Command/environment/URL/header values are validated before use | `validate_server_config` (shell metacharacters `;|&><\`$()`/CR/LF in command+args; forbidden env var names `LD_PRELOAD`, `LD_LIBRARY_PATH`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`, `DYLD_LIBRARY_PATH`, `DYLD_FRAMEWORK_PATH`, `PATH`, `NODE_OPTIONS`, `BASH_ENV`, `PYTHONPATH`, `PYTHONSTARTUP`, `RUBYOPT`, `PERL5OPT`, `JAVA_TOOL_OPTIONS`, plus any `DYLD_`-prefixed name; URL scheme restricted to `http`/`https`; header names checked against RFC 7230 `tchar`, values checked for control characters; duplicate header names rejected case-insensitively) — runs unconditionally inside the config builder, and again at the point of use (defense in depth) |
| NFR-SEC-002 | Secret values never appear in debug/log output | Hand-written (never derived) `Debug` impls using `RedactedItems`/`RedactedMapValues`/`RedactedUrl`/`sanitize_path_for_error`, applied to every type from `ServerConfig` up through the CLI's `Commands` enum; `Serialize` is deliberately exempt (config persistence needs real values) — this asymmetry is documented at every layer that repeats it, not accidental |
| NFR-SEC-003 | No secret/PII handling policy beyond the above | Not applicable in the traditional sense (this project handles infrastructure credentials — API tokens, connection strings — not end-user PII); no masking/pseudonymization/deletion policy exists because no PII is collected or stored |

### 4.2 Authentication & Authorization

> [!note] Not Applicable
> This project has no user accounts, login, session-based user identity, or
> role model. The one session concept that exists (`mcp-execution-server`'s
> `PendingGeneration`) is a short-lived workflow token bridging two MCP tool
> calls within a single already-trusted MCP client connection, not an
> authentication mechanism — see [[#4.5 Compliance]] for the closest
> applicable control (session validation, not identity).

### 4.3 Integrity

| ID | Requirement | Implementation |
|----|------------|---------------|
| NFR-SEC-020 | Every attacker-influenced input is validated server-side (no client-trust assumption) | `validate_server_config` (connection details), `validate_server_id`/`validate_path_segment` (identifiers), size/count bounds at every pipeline stage (introspection, codegen, export, session store, wire framing) — see the CWE-400 bound table below |
| NFR-SEC-021 | Resource-exhaustion (CWE-400) bounds at every layer, each derived from the layer below rather than independently chosen | See table below; this cascading-by-value relationship is itself a tested invariant (data that already cleared a lower layer's bound must never be deterministically rejected by a higher layer for merely being as large as already allowed) |
| NFR-SEC-022 | Prompt-injection / Markdown-injection defense for LLM-facing or document-embedded untrusted text | `sanitize_untrusted_text` (control-character, line-separator, and bidi-override/isolate flattening to spaces, plus bidi-mark removal, plus — #425 — removal of the Unicode Tags block U+E0000-U+E007F, U+FEFF, and the U+2060-U+2064 invisible-operator run, and flattening of U+200B to a space; closes an invisible-payload smuggling channel a tokenizer reads but a human reviewer can't see, for that specific enumerated character set; plus — #431 — on the already-filtered text, a whole-value variation-selector total (drops every selector once the total exceeds 16, regardless of distribution across runs/base characters) combined with a per-run threshold (drops a run over 2 when the total stays under that bound) — the whole-value total specifically closes the case a per-run-only check does not: a payload spread as many short runs across many base characters; plus length truncation) plus `wrap_untrusted_block` (`<untrusted-data>...</untrusted-data>`, `&`/`<`/`>`-escaped body) — applied by `mcp-skill` (tool metadata, `skill_name`, and — #429 — `use_case_hints`, itself also count-bounded via `MAX_USE_CASE_HINTS`) and `mcp-server`, regression-tested against an explicit boundary-forgery attempt |
| NFR-SEC-023 | The generated TypeScript runtime bridge cannot silently drift from the Rust source of truth for its own security rules | `FORBIDDEN_CHARS`/`FORBIDDEN_ENV_NAMES`/`FORBIDDEN_ENV_PREFIX`/`env_name_charset_pattern()`/`env_name_charset_desc()` (#467) and the DoS size/count ceilings `MAX_ARG_COUNT`/`MAX_ARG_LEN`/`MAX_ENV_COUNT`/`MAX_ENV_VALUE_LEN`/`MAX_URL_LEN`/`MAX_HEADER_COUNT`/`MAX_HEADER_VALUE_LEN` (#471) are rendered into `_runtime/mcp-bridge.ts` directly from `mcp_execution_core`'s Rust constants/accessors at generation time (via `BridgeContext::default()`, hand-written with no `derive(Default)` so it can never render a fail-open/empty bridge), not hand-copied; the string-valued renders that participate in matching (not just message text) additionally go through `sanitize_ts_string_literal` so a future Rust-side value containing `'`/`\` cannot silently change what the generated code matches |
| NFR-SEC-024 | Log-injection (forged log lines via embedded control characters/newlines in attacker-influenced text) defense-in-depth for the plain-text log format | `mcp-execution-server`'s `SanitizedCodecError` reuses `sanitize_untrusted_text` (the same primitive NFR-SEC-022 uses for a different purpose) to guard a `JsonRpcMessageCodecError`'s `Display` before it reaches the `tracing::warn!` for a dropped malformed/oversized stdin line. Defense-in-depth, not a fix for a currently reachable path: with the pinned `rmcp` 3.1.2, `RxJsonRpcMessage`'s request/notification payload types are `#[serde(untagged)]`, so a mismatched inner variant's error — including any attacker-controlled text — is discarded before it can reach a `Serde` error here; the guard exists because that error-swallowing is an `rmcp` implementation detail this project does not control and `JsonRpcMessageCodecError` is `#[non_exhaustive]` (#415). `--log-format json` needs no equivalent guard — `serde_json` already escapes whatever string ends up in the field |
| NFR-SEC-025 | Visual-spoofing ("Trojan Source"-style) defense, plus invisible-payload-smuggling defense against a specific enumerated character set plus the variation-selector channel (with documented residual limits), for untrusted text shown to a human or an LLM reader | Same mechanism as NFR-SEC-022 (`sanitize_untrusted_text`'s bidi-character flattening, its #425 handling of the Unicode Tags block and the enumerated zero-width characters, and its #431 whole-value-total-plus-per-run variation-selector mitigation, total threshold 16 — a global count, not a semantic check, so it cannot distinguish several independent legitimate emoji from an equal count of payload-carrying selectors once the total is crossed, and the allowance is per sanitized field, so many fields can aggregate a larger surviving total in a full introspection response; see `specs/core/spec.md`'s "Known limitation" notes) — listed as its own requirement because it addresses a distinct threat (text that misrepresents itself, or is entirely invisible, to the human reviewer even though an LLM reader sees it in full) rather than structural injection, even though one function implements both. Extends, at a construction-time validation boundary rather than a display-time repair, to `ToolName::new`/`ServerId::new` (#432, #431, #444): the UTS #39 `Identifier_Status=Allowed` allowlist gate (`ToolNameError`/`ServerIdError::DisallowedCharacter`, issue #444) rejects a name/id carrying a Tags-block, bidi, or zero-width-operator payload, or *any* variation selector (stricter than the display-text thresholds above, since an identifier has no rendering to protect), as a side effect of accepting only allowlisted identifier characters — none of those characters carry that status. `sanitize_ts_string_literal` independently neutralizes the same character classes plus the same variation-selector thresholds NFR-SEC-022 uses, at the codegen boundary (tool name and server id embedded in generated TypeScript). That codegen-boundary function truncates its **raw** input to `MAX_UNTRUSTED_FIELD_LEN` *before* escaping (not the escaped output — an earlier version that capped post-escape could split a multi-character escape sequence and leave the generated string literal unterminated, critic finding C3), since neither newtype enforces a length bound — so the defense holds regardless of which code path produced the string. An earlier draft of #432/#431 added denylist predicates (`contains_invisible_payload_char`/`contains_variation_selector`) as a `ToolName::new` construction-time gate; these were removed before merge once #444's allowlist gate landed on the same constructor and left them with no in-tree caller |

**CWE-400 bound cascade** (root constants and their direct, by-value
derivations):

| Root (mcp-introspector) | Derived (mcp-codegen) | Derived (mcp-files) | Derived (mcp-server) |
|---|---|---|---|
| `MAX_TOOL_COUNT` = 1000 | `MAX_GENERATED_FILES` = `MAX_TOOL_COUNT` + 5 | `MAX_EXPORT_FILES` = `MAX_GENERATED_FILES` | (session count independently bounded, `MAX_PENDING_SESSIONS` = 1000) |
| `MAX_TOOL_NAME_LEN` = 256 B, `MAX_TOOL_DESCRIPTION_LEN` = 8 KiB, `MAX_SCHEMA_SIZE_BYTES` = 64 KiB | `MAX_GENERATED_BYTES` = 2 × `MAX_TOOL_COUNT` × (name+desc+schema) | `MAX_EXPORT_BYTES` = `MAX_GENERATED_BYTES` | `MAX_TOTAL_PENDING_BYTES` = 4 × per-session estimate built from the same three root constants |

Independent, non-derived bounds enforced elsewhere in the same category:
`MAX_ARG_COUNT` (256), `MAX_ARG_LEN` (4096 B), `MAX_ENV_COUNT` (256),
`MAX_ENV_VALUE_LEN` (32 KiB), `MAX_HEADER_COUNT` (128),
`MAX_HEADER_VALUE_LEN` (8 KiB), `MAX_URL_LEN` (8 KiB) — all in
`mcp-execution-core::command`, and (#471) mirrored a second time into the
generated TypeScript runtime bridge (see NFR-SEC-023); `MAX_REQUEST_LINE_SIZE`/
`MAX_RESPONSE_LINE_SIZE` (4 MiB each, stdio framing, both directions this
project controls); `MAX_CONCURRENT_REQUESTS` (8, server binary);
`MAX_TOOL_FILES` (500, skill scanning); `MAX_FILE_SIZE` (1 MiB, `_meta.json`);
`MAX_FRONTMATTER_SIZE` (8 KiB, SKILL.md YAML block); `MAX_SKILL_CONTENT_SIZE`
(100 KiB, `save_skill`).

### 4.4 Non-repudiation / Accountability

> [!note] Not Applicable
> No audit-log, user-identity-attributed action log, or non-repudiation
> mechanism exists or is claimed. `tracing` structured logs exist for
> operational diagnosis (see [[#7.3 Analysability]]), not as an
> accountability control.

### 4.5 Compliance

> [!note] Not Applicable
> No regulatory compliance requirement (GDPR/CCPA/HIPAA/PCI DSS/SOC 2) is
> evidenced or claimed anywhere in the repository — consistent with this
> being a locally-run developer tool that stores no end-user personal data.
> The closest analogous control actually present is **supply-chain policy**,
> enforced via `cargo-deny` (`deny.toml`): dependency licenses restricted to
> an explicit allow-list (MIT, Apache-2.0, BSD-2/3-Clause, ISC,
> CDLA-Permissive-2.0, Unicode-3.0, Zlib, BSL-1.0, MPL-2.0), unknown
> registries/git sources denied, only `crates.io` allowed as a dependency
> source, and security advisories scanned (RUSTSEC/CVE) — enforced in CI
> (`security` job, `EmbarkStudios/cargo-deny-action`).
> Dependabot version updates for `cargo` and `github-actions` carry a 7 day
> `cooldown` (`.github/dependabot.yml`, #644); security updates are exempt.
> Cargo's own `registry.global-min-publish-age` is deliberately **not** set
> (user decision 2026-11-25): it blocks `cargo update --precise` on a fresh
> release and makes cargo 1.98, the current MSRV, warn on every command.
> Revisit when the MSRV reaches 1.100.

## 5. Usability

> [!note] Not Applicable (mostly)
> This is a developer-facing CLI/library with no GUI; the ISO 25010
> Usability sub-characteristics of UI aesthetics and accessibility do not
> apply. The two sub-areas below are the only ones with real, evidenced
> content.

### 5.1 Operability

| ID | Requirement | Target |
|----|------------|--------|
| NFR-USE-010 | Shell completion availability | bash, zsh, fish, powershell, elvish (`completions` subcommand) |
| NFR-USE-011 | Error messages must not leak secrets while still being actionable | Every rejection path omits the offending value when it could be secret-shaped (e.g. a misparsed `--api-key sk-...`), while still naming the *kind* of failure (e.g. "stdio server entry must not set \"url\"") |

### 5.2 Accessibility / Internationalization

> [!note] Not Applicable
> No GUI exists to apply WCAG/accessibility standards to. No localization
> or multi-language output exists — all CLI/error/log text is English-only,
> consistent with this repository's own English-only documentation
> convention.

## 6. Compatibility

### 6.1 Interoperability

| ID | Requirement | Standard/Protocol |
|----|------------|-------------------|
| NFR-COM-001 | MCP protocol compliance | Model Context Protocol via the official `rmcp` SDK (both client and server roles); `mcp-execution-server`'s `get_info()` pins `2025-06-18` as the negotiation fallback, but the server advertises and can negotiate up to every protocol version the SDK knows (`ProtocolVersion::KNOWN_VERSIONS`, currently through `2026-07-28`) since `supported_protocol_versions()`/`discover()` are not overridden — see `specs/server/spec.md` and issue #381 |
| NFR-COM-002 | Generated output executes without depending on this project's own runtime | Generated TypeScript is a self-contained package (own `package.json`/`tsconfig.json`) runnable via `tsx`, `deno`, or Node's native TypeScript stripping — not merged into a consumer's own TypeScript build |

### 6.2 Co-existence

| ID | Requirement | Details |
|----|------------|---------|
| NFR-COM-010 | OS support (build/run this project itself) | Linux, macOS, Windows — pre-built binaries published for macOS (arm64/amd64), Linux (amd64/arm64); Windows built from source/release zip per `README.md` |
| NFR-COM-011 | Runtime dependency for executing generated output | Node.js ≥ 22.18.0 (23.6.0 on the 23 line), checked by `setup` |
| NFR-COM-012 | Coexistence with other MCP-aware tools sharing `~/.claude/mcp.json` | `McpServerEntry` deserialization warns (not fails) on unrecognized top-level keys (e.g. another tool's `disabled`/`alwaysAllow` keys), so this project's CLI can read a config file shared with tools it doesn't otherwise know about |

> [!note] Not Applicable
> No browser or mobile compatibility requirement applies — there is no
> browser-rendered or mobile-app surface anywhere in this project.

## 7. Maintainability

### 7.1 Modularity & Modifiability

| ID | Requirement | Details |
|----|------------|---------|
| NFR-MNT-001 | Architecture style | Layered workspace (Cargo workspace, 7 crates), strict low→high dependency direction, zero circular or upward dependencies (`mcp-core` has zero intra-workspace dependencies) — see [[README#Block Structure]] |
| NFR-MNT-002 | API versioning | Semantic Versioning. Before 1.0.0 breaking changes are permitted without shims and are documented under `[Unreleased]`/`Breaking` in `CHANGELOG.md` rather than deferred. From 1.0.0 the policy of [[#12.2 Semver policy]] binds (`NFR-COM-040` to `NFR-COM-046`) |
| NFR-MNT-003 | Lint policy as a modularity/quality gate | Clippy `all`/`cargo`/`nursery`/`pedantic` set to `deny` at workspace level; any `#[allow(...)]` narrower than workspace scope requires a justification comment (enforced by convention, backfilled per `CHANGELOG.md` `[Unreleased]`/`Documentation`, #186) |

### 7.2 Testability

| ID | Requirement | Details |
|----|------------|---------|
| NFR-MNT-010 | Test count | README states "657 tests with 100% pass rate" — a point-in-time claim as of the README's last edit, not independently re-verified while writing this document |
| NFR-MNT-011 | Coverage measurement vs. enforcement | Coverage is generated and uploaded to Codecov on every CI run (`coverage` job, `cargo llvm-cov nextest`), but **`fail_ci_if_error: false`** is set on the upload step and no numeric coverage threshold gate exists in `.github/workflows/ci.yml` — coverage is tracked, not enforced. No specific percentage target should be asserted as a requirement without adding such a gate. |
| NFR-MNT-012 | Doc-tests as executable documentation | `#![warn(missing_docs)]` workspace-wide; every `pub` item requires a doc comment with a runnable `# Examples` section, verified by `cargo test --doc --all-features --workspace` in CI |
| NFR-MNT-013 | CI/CD pipeline | Build + lint (`fmt --check`, `clippy -D warnings`) + test (`nextest`) + doc-test + coverage + MSRV check + `cargo-deny` security/license/source audit + benchmark build, on every push/PR (`.github/workflows/ci.yml`). The release pipeline requirements (tests before publishing, publish dry-run, release notes from the curated changelog, `cargo semver-checks`) extend it in [[#12.6 Release pipeline]] (`NFR-MNT-040` to `NFR-MNT-049`) |
| NFR-MNT-014 | Regression traceability | Tests are named and commented to cite the exact issue number they guard against (e.g. `test_build_and_export_rejects_empty_top_level_component`), making the test suite itself a changelog of past correctness/security incidents |

### 7.3 Analysability

| ID | Requirement | Details |
|----|------------|---------|
| NFR-MNT-020 | Structured logging | `tracing` used throughout; `mcp-execution-server`'s tool handlers carry structured spans (e.g. a `server_id` field, deliberately left empty on early validation failure rather than logging unvalidated input) |
| NFR-MNT-021 | Observability stack | Logs only (`tracing`/`tracing-subscriber`, `env-filter`); no metrics or distributed-tracing export exists or is claimed — consistent with this being a locally-run tool, not a monitored service |
| NFR-MNT-022 | Health check endpoints | Not applicable — no long-running network service with a health-check surface exists (`mcp-execution-server` communicates over stdio to a single local client, not a service with `/health`/`/ready`) |

## 8. Portability

### 8.1 Adaptability

| ID | Requirement | Details |
|----|------------|---------|
| NFR-POR-001 | Deployment environments | Local developer machine only (Linux/macOS/Windows); no cloud/on-prem/hybrid hosted deployment target exists or is claimed |
| NFR-POR-002 | Containerization | Not evidenced — no `Dockerfile`/container manifest exists in this repository |

### 8.2 Installability

| ID | Requirement | Details |
|----|------------|---------|
| NFR-POR-010 | Installation methods | `cargo install mcp-execution-cli` (crates.io, recommended); pre-built release binaries (GitHub Releases, per-platform archives); build from source (`cargo install --path crates/mcp-cli`); individual library crates via `cargo add` (all seven are stable libraries from 1.0.0, `NFR-COM-030`). Every channel carries the tagged version, and nothing is published before the gate of `NFR-MNT-041` passes |
| NFR-POR-011 | Configuration management | A single JSON file, `~/.claude/mcp.json`, read directly from the filesystem, plus one narrow environment-variable exception: `MCP_EXECUTION_LOG_FORMAT` (issue #399) selects diagnostic log format (`text`/`json`) for `mcp-execution-cli` and the `mcp-execution` server binary — an operational logging-output switch, not a secrets-manager-style configuration path, with no secret-shaped value ever accepted or echoed. No other environment-variable-based or secrets-manager-based configuration path exists for this project's *own* configuration (as opposed to the *target* MCP servers it introspects, whose env vars it explicitly validates — see [[#4.1 Confidentiality]]) |
| NFR-POR-012 | MSRV policy | Rust 1.98 minimum, enforced by a dedicated CI job (`msrv`); rolling policy: MSRV = stable minus two, raised in a minor release (README states it). How a raise relates to semver for the library crates is an open decision (OD-3, `NFR-COM-070` to `NFR-COM-072`) |

## 9. Verification Matrix

| ID | Method | Environment | Frequency |
|----|--------|-------------|-----------|
| NFR-PERF-001, NFR-PERF-002 | `criterion` benchmark | Local dev machine / CI (`cargo bench --no-run --profile bench-fast` build-only in CI; full run is manual) | Ad hoc / per performance-sensitive change |
| NFR-PERF-010 | `dhat`-based heap profiling (`profile_memory` example) | Local dev machine | Ad hoc |
| NFR-SEC-001–025 | Unit + integration test suite (`cargo nextest run --all-features --workspace`) | CI (`test` job) | Every push/PR |
| NFR-SEC-021 (advisories/licenses/sources) | `cargo-deny` | CI (`security` job) | Every push/PR |
| NFR-MNT-010–014 | `cargo nextest run`, `cargo test --doc` | CI | Every push/PR |
| NFR-MNT-011 (coverage) | `cargo llvm-cov nextest` → Codecov upload | CI (`coverage` job, Linux only) | Every push/PR; **not gating** (`fail_ci_if_error: false`, no threshold check) |
| NFR-POR-012 (MSRV) | `cargo check`/`build` pinned to Rust 1.98 | CI (`msrv` job) | Every push/PR |
| NFR-PERF-030 to NFR-PERF-032, NFR-SEC-050 to NFR-SEC-055, NFR-MNT-030, NFR-COM-020, NFR-POR-020 | Planned, not yet implemented | Not applicable until the owning capability lands | Not applicable |
| NFR-COM-030 to NFR-COM-072, NFR-MNT-040 to NFR-MNT-049 | Per-requirement methods in [[#12.8 Verification and traceability]]; target requirements binding from the 1.0.0 tag | CI and the release workflow | See that table |

## 10. Open Questions

> [!question] Unresolved Quality Requirements
> - [ ] Should coverage become a gating CI check (a numeric threshold), or
>   remain tracked-only as it is today?
> - [ ] Is there an intended target for closing the introspector's HTTP/SSE
>   response-size gap (see [[SRS-mcp-execution-2026-07-27#FR-002]]), and
>   should a quantitative NFR (e.g. a maximum buffered-response size) be set
>   for it once `rmcp` exposes the necessary knob?
> - [x] Should the 98% token-savings claim be backed by an automated,
>   reproducible benchmark (making it a verifiable NFR rather than a
>   documented estimate)? **Answered 2026-11-24:** the 98% and 83% figures are
>   retired; measurement is planned under #562 and no published figure may be
>   unreproducible (#560).
> - [ ] Is any future hosted/multi-tenant deployment mode planned (which
>   would reintroduce Availability/Scalability as real, specifiable NFR
>   categories rather than "not applicable")?
> - [ ] `GenerationProvenance` capture (`mcp-core::provenance`, #468/#497)
>   landed and is stamped into every `_meta.json` at generation time, but no
>   comparison/staleness-detection mechanism consumes it yet. Should a
>   drift-detection NFR (and CLI/server surface) be specified once that
>   comparison ships?
> - [ ] What value should the output budget of a script run take
>   (NFR-PERF-031), and what runner start-up overhead is acceptable
>   (NFR-PERF-032)?
> - [ ] Do successful tool results printed to the caller fall under the
>   untrusted-text boundary rule, or are they an accepted exception
>   (NFR-SEC-054)?
> - [ ] The five open decisions of the 1.0 contract (OD-1 to OD-5): the
>   `progressive` name, the wire-format compatibility window, MSRV against
>   semver, foreign types in public signatures and the intended public surface
>   of the `cli` and `server` libraries — see [[#12.7 Open decisions (Ask First)]].

## 11. Planned Non-Functional Requirements

> [!warning] Planned, not implemented
> Nothing in this section is enforced or measured today. These requirements
> belong to the direction adopted on 2026-11-24
> ([[BRD-mcp-execution-2026-07-27#Direction (adopted 2026-11-24)]]) and use
> the same ID scheme as above. Values marked provisional or open are
> undecided; no figure here is a measurement.

### 11.1 Performance Efficiency (planned)

| ID | Requirement | Target | Measurement |
|----|------------|--------|-------------|
| NFR-PERF-030 | Context cost of discovery: size of one entry of the typed catalog (FR-032) | Provisional: median entry at most about 250 bytes without parameter documentation, and the whole catalog of the 16-tool baseline server at most about 10% of the bytes of its tool files. The target is a plan figure, not an achieved one | The measurement command planned under #562, on reference fixtures |
| NFR-PERF-031 | Output budget of a script run (FR-036): the amount a run may print before output is spilled to a file | Decided in #571: stdout 16 KiB, stderr 4 KiB, hard maximum 1 MiB; the head within the budget is shown, the rest is spilled | `acceptance_run.rs` (budget head, spill path, byte count) |
| NFR-PERF-032 | Runner overhead: start-up cost of running a script through the runner compared with running it directly | Measured and stated; the acceptable value is open. Measured 2026-11-24 ([[decisions/ADR-590-persistent-connections-across-runs]]): a `run` without calls takes a median 229.2 ms wall (release build, workspace release profile, Apple M3 Pro, node v26.13.1, stdio fixture, N = 15 sequential runs after one warm-up), of which `node` alone is about 96 ms (ADR-569); one extra server start-up adds about 19.4 ms and is paid once per run, not per call (K = 10 calls: first call 4.9 ms, calls 2 to 10 0.2 ms each, one initialize per run). The first measurement on a debug build gave 223.5 ms and 35.3 ms and is kept only as a secondary figure in the ADR | The measurement harness (ADR-590 method, ADR-569 table) |

### 11.2 Security and resource bounds (planned)

| ID | Requirement | Details |
|----|------------|---------|
| NFR-SEC-050 | Spill files, call journals and saved scripts are bounded by named constants, each derived from an existing bound where one sits below it | Count, size, retention and listing size are bounded (CWE-400). The bound on a saved script does not depend on a constant private to a higher crate; where a shared bound lives is open (FR-036, FR-037, FR-044) |
| NFR-SEC-051 | Generated-file and generated-byte bounds stay consistent when output grows | A change that adds or removes a fixed generated file follows the fixed-file rule in [[codegen/spec]] (planned changes). The generated-bytes bound is re-derived so that a server that passed introspection is never rejected at generation, counting one input schema and one output schema per tool. A named byte bound for the catalog derives from the generated-bytes budget (value open) (FR-032, FR-030) |
| NFR-SEC-052 | The call journal never trusts a server's read-only hint | Every completed call is reported as possibly state-changing unless the user's policy classes it as a read; the failure summary is delivered through the runner's notice channel, which script output cannot forge; deleting expired journal and spill files is confined exactly like creating them (FR-037) |
| NFR-SEC-053 | A dry run cannot be defeated by a wrong or hostile hint | Classification uses the annotations recorded at generation; a wrong or hostile read-only hint must not cause a write to execute (mechanism open); a skipped write never returns a fabricated result (FR-038) |
| NFR-SEC-054 | Tool results printed to the caller and the untrusted-text rule | Open maintainer decision: whether results printed by single-call mode, the runner and the one-shot call fall under the boundary rule in [[constitution#V. Security]] or are an accepted exception. No sanitisation and no exception is claimed until it is decided. Error text from a server is sanitised, bounded and marked as coming from the server. An argument is a single JSON object and a malformed one is reported without repeating its text (FR-035, FR-036, FR-039) |
| NFR-SEC-055 | Text from an MCP server stays inert data in a rendered skill: the host that loads the skill treats none of it as anything but text | A requirement on every generation path of the skill (FR-043), under the injection-defence rule of the constitution's security section |

### 11.3 Maintainability and documentation (planned)

| ID | Requirement | Details |
|----|------------|---------|
| NFR-MNT-030 | Documentation claims policy | Every number in user-facing documentation comes from a command a reader can rerun or is labelled as an illustration with its source; documentation says when a script is not worth it; examples run as written (#560, #562, #563) |

### 11.4 Compatibility and portability (planned)

| ID | Requirement | Details |
|----|------------|---------|
| NFR-COM-020 | The `_meta.json` version 3 bump is a breaking change | Existing version 2 directories are rejected with a message that says to regenerate; the bump happens once, with one registry of fields; the change is recorded in `CHANGELOG.md` and no compatibility shim is added before 1.0 (FR-034, #568) |
| NFR-POR-020 | Minimum Node.js version for running generated files and scripts | Node.js 22.18.0 (23.6.0 on the 23 line) or later, the first versions that run `.ts` files without a flag; `setup` enforces it (NFR-COM-011, #564). A script runner may need a higher floor (#571) |

## 12. 1.0 Contract Requirements

> [!abstract]
> What the 1.0.0 release promises and how that promise is verified: the set of stable library crates, the
> public-API and semver policy, wire-format compatibility, the command-line contract, the MSRV rule for
> libraries and the release pipeline. Source: issue #670 (parent #669, API freeze #673 with sub-issues #674 to
> #680). Requirement groups are numbered inside the existing ISO/IEC 25010 categories: Compatibility
> (`NFR-COM-030` to `NFR-COM-072`) and Maintainability (`NFR-MNT-040` to `NFR-MNT-049`). Business need:
> [[BRD-mcp-execution-2026-07-27#1.0 contract (target release 1.0.0)|BRD FR-047 to FR-049]]; per-crate public
> surface: the section "1.0 Public API Contract" of each crate spec (`FR-047.1` to `FR-047.7`); index:
> [[README#1.0 Contract]]; decision record: [[decisions/ADR-670-1-0-api-wire-release-contract]].

> [!warning] Status of this section
> Requirements stated without a marker are **decided** (the stable set is decided by the maintainer) or are
> direct consequences of the issues that cite them. Five points are **not decided** and are written as
> `Open decision / Ask First` items in [[#12.7 Open decisions (Ask First)]]: the `progressive` name (OD-1), the
> wire-format compatibility window (OD-2), MSRV against semver (OD-3), foreign types in public signatures
> (OD-4) and the intended public surface of the `cli` and `server` libraries (OD-5). A requirement that depends on
> one says so and states what holds **until** it is decided. Until 1.0.0 is tagged, `NFR-MNT-002` (breaking
> changes allowed, no shims) stays in force; this section binds from the tag.

### 12.1 Stable library set and public surface

Traces to: BRD FR-047, SRS FR-047, issues #673, #675, #676, #677.

| ID | Requirement | Priority | Acceptance criteria |
|----|-------------|----------|---------------------|
| NFR-COM-030 | THE SYSTEM SHALL publish exactly seven library crates as stable 1.x libraries: `mcp-execution-core`, `-introspector`, `-codegen`, `-files`, `-skill`, `-server`, `-cli`. THE SYSTEM SHALL release them at one shared workspace version (lockstep, as today through `workspace.package`) | must | `Cargo.toml` `workspace.package.version` is the only version source, and every crate inherits it. The release gate of `NFR-MNT-040` fails when a crate or an intra-workspace dependency version differs |
| NFR-COM-031 | THE SYSTEM SHALL define a crate's public API as the items reachable by a `pub` path from the crate root that are not `#[doc(hidden)]`, together with its documented cargo features, its documented wire and file formats (section 12.3) and, for `mcp-execution-cli` and the `mcp-execution` server binary, the contract of section 12.4. Everything else is internal and MAY change in any release | must | Each crate spec lists its public surface (section "1.0 Public API Contract"). The `cargo public-api` output or the rustdoc item list of each crate equals that listing, checked in the review of every PR that changes a `pub` item |
| NFR-COM-032 | THE SYSTEM SHALL keep an internal module, field or helper out of the public API: `pub(crate)`, private, or `#[doc(hidden)]` only where an integration test or a sibling crate needs the item, and in the latter case the item is listed as "workspace-internal" in the crate spec and excluded from the semver promise | must | The `pub` items of `mcp-execution-cli` and `mcp-execution-server` equal the listing of the crate spec (the surface proposed in OD-5, or the decided one). No `pub` field exists in a type that is not on the listing |
| NFR-COM-033 | THE SYSTEM SHALL NOT expose a cargo feature that exists only for tests or fixtures (today `test-fixtures` of `mcp-execution-cli` and `mcp-execution-introspector`) as a documented feature of a published crate | must | `cargo doc --all-features` for the published crates shows no fixture-only item; the feature, if kept, is absent from crate docs and READMEs and its items are `#[doc(hidden)]` and unlisted, or the fixtures move to an unpublished (`publish = false`) workspace member |
| NFR-COM-034 | THE SYSTEM SHALL define a concept that crosses crates once, in the lowest crate that needs it, and reuse it above (today `ResultSource` is defined in `mcp-execution-introspector` and in `mcp-execution-codegen`, and `ResultKind` in `mcp-execution-cli` expresses the same idea) | must | A search over `crates/*/src` finds one public definition of each cross-crate concept; the two former `ResultSource` types are one type; the crate specs name the owner |
| NFR-COM-035 | THE SYSTEM SHALL carry a server id in public signatures as a validated type built once at the boundary, never as `String` or `&str` (today `String` ids in `mcp-execution-codegen`, `mcp-execution-skill` and `mcp-execution-server` structs, and a slug check that returns `()`) | must | No public struct field or function parameter named for a server id has type `String` or `&str`; the slug rule is enforced by a constructor, with a rejection test per forbidden character class |

### 12.2 Semver policy

Traces to: BRD FR-047, SRS FR-047, issues #673, #674, #678, #679.

| ID | Requirement | Priority | Acceptance criteria |
|----|-------------|----------|---------------------|
| NFR-COM-040 | THE SYSTEM SHALL treat each of these as a breaking change, released only in a major version: removing or renaming a public item; changing a signature, a type, a bound or an `async`, `const` or `unsafe` qualifier of a public item; adding a variant to, or a public field to, a type that is not `#[non_exhaustive]`; adding a required method to a public trait that downstream code implements; removing a documented feature; changing a documented default or a documented error condition in a way a correct caller can observe; changing the major version of, or removing, a dependency whose types appear in a public signature (`NFR-COM-044`); and a breaking change to a wire format or to the command-line contract (sections 12.3 and 12.4) | must | `cargo semver-checks` (`NFR-MNT-046`) reports no major-level lint on a minor or patch release; the remaining items are checked by the reviewer against this list, and a breaking PR cites the clause |
| NFR-COM-041 | THE SYSTEM SHALL treat each of these as compatible, allowed in a minor version: adding a public item; adding a variant to or a field of a `#[non_exhaustive]` type; adding a defaulted trait method; adding a cargo feature that is off by default; adding a command, a flag or an optional JSON field (`NFR-COM-060`, `NFR-COM-062`); and tightening an internal limit that is not documented as part of the contract. A patch version SHALL contain only fixes that change no public item | must | A minor release diff contains only these categories; the `CHANGELOG.md` section lists each under "Added" or "Changed" |
| NFR-COM-042 | THE SYSTEM SHALL mark `#[non_exhaustive]` every public enum and every public struct with public fields that may grow without a major release: error enums, kind and status enums, outcome enums, option and configuration structs and result structs. A struct that callers must create SHALL offer a constructor or builder, because `#[non_exhaustive]` forbids struct-literal construction outside the crate | must | Every public enum of the published crates carries `#[non_exhaustive]` unless it is listed as closed (`NFR-COM-043`); each `#[non_exhaustive]` struct has a documented constructor or builder; the `ResourceKind` case recorded as breaking in the changelog is closed by it |
| NFR-COM-043 | THE SYSTEM SHALL keep a set exhaustive only when it is closed by definition, SHALL say so in the type's documentation ("closed set: adding a variant is a major change") and SHALL list it in the crate spec | must | Each exhaustive public enum has that sentence in its rustdoc and an entry in its crate spec; an exhaustive enum with no such entry is a defect found by the check of `NFR-COM-042` |
| NFR-COM-044 | WHERE a type of a third-party crate appears in a public signature, trait implementation or public field, THE SYSTEM SHALL list it in the crate spec with the crate and its version range, and SHALL treat a major-version change of that crate as a breaking change of this crate. THE SYSTEM SHALL NOT add a new foreign type to a public signature without the Ask First decision of OD-4. **Open decision (OD-4):** whether `rmcp` and `tracing-subscriber` types may stay, and under what rule. Known exposures at 9473243: `rmcp::ServerHandler` implemented by `GeneratorService` (`mcp-execution-server`), `EnvFilter` in `cap_rmcp_log_level` (`mcp-execution-core`, feature `tracing-filter`), `chrono::DateTime<Utc>` in `GenerationProvenance::generated_at` (`mcp-execution-core`), `serde_json::Value` in `ToolInfo` (`mcp-execution-introspector`) and `ToolDefinition` (`mcp-execution-codegen`), `serde_json::Error` in `MetadataReadError::Malformed` (`mcp-execution-core`), `handlebars::RenderError` and `handlebars::TemplateError` in `mcp-execution-skill::TemplateError`, `chrono::DateTime<Utc>` in the `Clock` trait (`mcp-execution-server`), `clap::Parser` and `clap::Subcommand` implementations on `Cli` and `Commands` (`mcp-execution-cli`), and `serde` and `schemars::JsonSchema` implementations on public types | must | The crate specs carry the exposure lists above; a review check compares the list with the public signatures; the OD-4 outcome replaces the list with either "none" or the accepted list and its coupling rule |
| NFR-COM-045 | WHEN a change is breaking per `NFR-COM-040`, THE SYSTEM SHALL ship it only with a major version bump and record it under `### Breaking` in `CHANGELOG.md`; from 1.0.0 THE SYSTEM SHOULD announce a removal first with `#[deprecated]` (stating the replacement) in an earlier minor release, unless the change fixes a defect that makes the old behavior unsafe | must (deprecation: should) | Each `Breaking` entry names the clause; each removal after 1.0.0 is preceded by a `#[deprecated]` release or carries a recorded safety exception |
| NFR-COM-046 | THE SYSTEM SHALL treat the serialized form (field names, value types, enum spellings) of a public type as public API only when the type documents it as persisted or wire (`_meta.json`, helper protocol, JSON command output); for any other type the serialized form MAY change in a minor release | should | Types that are wire or persisted carry that statement in rustdoc and are listed in section 12.3; other types do not |

### 12.3 Wire-format compatibility

Traces to: BRD FR-047, SRS FR-047, FR-034, FR-040, issues #670, #683.

| ID | Requirement | Priority | Acceptance criteria |
|----|-------------|----------|---------------------|
| NFR-COM-050 | THE SYSTEM SHALL treat these formats as the wire contract, each owned by one crate and one spec: `_meta.json` (`schema_version` 3, owned by `mcp-execution-core::metadata`, [[core/spec]]); the helper protocol (`HELPER_PROTOCOL_VERSION` 1, line bounds, hello, environment names and channel key, owned by `mcp-execution-core::helper_protocol`, [[core/spec]] section 6); the names exported by a generated tool file (`{name}`, `{Name}Params`, `{Name}Result`) and by `_runtime/mcp-bridge.ts` (`callMCPTool`, `McpToolError`, [[codegen/spec]] sections 14 and 15); and the saved-script header (`mcp-script`, [[skill/spec]] section 13) | must | Each format is listed in its owner's spec with its version constant or "unversioned"; adding a format requires adding it to this list |
| NFR-COM-051 | THE SYSTEM SHALL give each versioned wire format one integer version, SHALL have a producer write exactly one current version, and SHALL treat a version bump as a breaking change recorded under `### Breaking` in `CHANGELOG.md` with the regeneration step the user must take | must | `METADATA_SCHEMA_VERSION` and `HELPER_PROTOCOL_VERSION` are the only definitions of their versions; the bridge template renders the helper version from the core constant (as built); a PR that changes either constant carries a `Breaking` entry |
| NFR-COM-052 | WHEN a consumer reads a version it does not support, THE SYSTEM SHALL reject it with a value-free message that names the found and expected versions and the action (regenerate, upgrade), and SHALL NOT parse it on a best-effort basis. As built: `read_server_metadata` accepts exactly version 3 and returns `MetadataReadError::SchemaMismatch { found, expected }`; the helper closes the channel without a reply on a version mismatch | must | The existing mismatch tests (a `_meta.json` of another version is rejected, a helper hello with a wrong version closes the channel) stay green; a new version adds one rejection test for the previous one |
| NFR-COM-053 | THE SYSTEM SHALL evolve a wire format inside one version only by adding optional fields that every consumer of that version ignores when unknown. THE SYSTEM SHALL record, per format, whether unknown fields are ignored or rejected: the `_meta.json` types do not use `deny_unknown_fields` (unknown fields are ignored), while the saved-script header and the policy file use `#[serde(deny_unknown_fields)]`, so a new field there breaks an older reader and is a version-level change | must | The wire-format listing of each owner spec carries the unknown-field rule; a test per format shows an extra field is ignored or rejected as stated |
| NFR-COM-054 | THE SYSTEM SHALL state how long an older version stays readable and how a bump is signalled to a user who has output from the previous release. **Open decision (OD-2):** the exact compatibility window ("read old, write new", exact match with regeneration, or a fixed N-1 window). Until it is decided the as-built rule of `NFR-COM-052` (exact match, regenerate) holds and no format changes its version | must | The ADR records the chosen window; the spec of each owner states it; a test pins it with a committed fixture of the previous version |
| NFR-COM-055 | THE SYSTEM SHOULD pin each wire format with committed golden fixtures that the current code reads or rejects as specified: a valid `_meta.json` of the current version, one of another version, and a helper hello transcript | should | The fixtures live under the owning crate's `tests/`; CI runs them; a change to a fixture is a reviewed change of the `Breaking` class |
| NFR-COM-056 | THE SYSTEM SHALL decide before the 1.0.0 tag whether the per-tool digest granularity left out of #580 (tracked by #683) changes the `_meta.json` shape; a change that alters the shape lands before the tag, as an optional field or with a version bump per `NFR-COM-051` | must | #683 is closed, or explicitly deferred with a note in [[core/spec#7. Planned Changes (script-first direction, adopted 2026-11-24)]], before the release gate of `NFR-MNT-049` runs |

### 12.4 Command-line and server-tool contract

Traces to: BRD FR-048, SRS FR-048, issues #670, #675, #680.

| ID | Requirement | Priority | Acceptance criteria |
|----|-------------|----------|---------------------|
| NFR-COM-060 | THE SYSTEM SHALL keep the documented command set, flag names, flag meanings, default values and positional arguments of `mcp-execution-cli` stable within a major version: removing, renaming or changing the meaning of a command or flag is breaking; adding a command or an optional flag is compatible. The hidden `helper` command is internal and covered by the helper protocol (`NFR-COM-050`), not by this clause. **Open decision (OD-1):** whether the `--progressive-output` flag and the `progressive` names are renamed before 1.0.0 | must | The command and flag listing of [[cli/spec]] section 2 and sections 16 to 19 equals the `--help` output of every command (checked by the `docs_examples` test or its successor); a PR that changes a flag edits both |
| NFR-COM-061 | THE SYSTEM SHALL keep the exit-code values and their meanings stable: 0 success, 1 other error, 2 invalid input, 3 remote server fault, 4 timeout, 5 check failed (drift or saved-script problems), 6 script failed, and 129, 130, 131, 143 for a `run` stopped by SIGHUP, SIGINT, SIGQUIT, SIGTERM. Moving a condition from one code to another is breaking. Introducing a new code value is **Ask First** | must | The `ExitCode` constants in `mcp-execution-core::cli` and the classification table of [[cli/spec]] section 5 agree; each code has a test that reaches it through the binary |
| NFR-COM-062 | THE SYSTEM SHALL keep the `--format json` output of each command stable in field names, value types and meaning within a major version: removing, renaming or retyping a field is breaking; adding an optional field is compatible and consumers are told to ignore unknown fields. The `run` output framing (budget head, spill path notice, `script_status` trailer and, with `--format json`, its stream objects) is part of this contract | must | Each command's JSON shape is documented in [[cli/spec]]; a golden test per command compares the field set; the `run` framing has the existing acceptance tests |
| NFR-COM-063 | THE SYSTEM SHALL write the command result to stdout and diagnostics, logs and error text to stderr, in every `--format` | must | A test per command with `--format json` parses stdout as the documented shape while stderr carries the diagnostics |
| NFR-COM-064 | THE SYSTEM SHALL keep the documented environment variables stable: `MCP_EXECUTION_LOG_FORMAT` and `MCP_EXECUTION_CLI`. The helper channel variables (`MCP_EXECUTION_HELPER`, `MCP_EXECUTION_HELPER_TOKEN`, `MCP_EXECUTION_RUN`) are wire-level (`NFR-COM-050`) | should | The variable list of [[cli/spec]] equals the constants of `mcp-execution-core::cli` and `helper_protocol` |
| NFR-COM-065 | THE SYSTEM SHALL NOT promise the text of `--format text` and `--format pretty` output or of log lines; a change to them is not a breaking change. **Assumption, confirm:** the maintainer may instead freeze `text` | should | The rustdoc and the CLI spec say "human-readable, not a stable interface" for both formats |
| NFR-COM-066 | THE SYSTEM SHALL keep the tool names and the parameter and result schemas of the `mcp-execution` server binary ([[server/spec]]) stable within a major version, with the same removal, rename and retype rule as `NFR-COM-062` | should | The tool list and schemas of [[server/spec]] equal what `tools/list` returns, pinned by the existing schema drift-guard tests |

### 12.5 MSRV and semver

Traces to: BRD FR-047, SRS FR-047, issue #670; extends `NFR-POR-012`.

| ID | Requirement | Priority | Acceptance criteria |
|----|-------------|----------|---------------------|
| NFR-COM-070 | THE SYSTEM SHALL declare the MSRV once in `workspace.package.rust-version`, inherit it in every crate, enforce it by the CI `msrv` job, and SHALL NOT raise it in a patch release | must | Each crate manifest inherits `rust-version` from the workspace; the `msrv` job pins the declared version |
| NFR-COM-071 | WHEN the MSRV is raised, THE SYSTEM SHALL change `rust-version`, the `msrv` job, the documentation and `CHANGELOG.md` in one pull request and SHALL NOT raise it to a version newer than stable minus two at that time (the rule of `NFR-POR-012`) | must | The PR diff touches all four; the new MSRV is at most stable minus two on the PR date |
| NFR-COM-072 | THE SYSTEM SHALL state whether an MSRV raise is a breaking change of the library crates. **Open decision (OD-3):** the constitution says the MSRV is raised "in a minor release"; semver for libraries from 1.0.0 may require a major release. Until it is decided the rules of `NFR-COM-070` and `NFR-COM-071` hold and the raise is recorded under `### Changed` | must | The ADR records the chosen reading; `NFR-POR-012` and [[constitution#II. Technology Stack]] quote it |

### 12.6 Release pipeline

Traces to: BRD FR-049, SRS FR-049, issue #669 exit criteria; extends `NFR-MNT-013`, `NFR-POR-010` and `NFR-POR-012`.

> [!info] State at 9473243 (as built, not a requirement)
> `.github/workflows/release.yml` runs on a `v*.*.*` tag. It builds six targets, then creates the GitHub Release
> (notes from `git log` subjects plus GitHub-generated notes, not from `CHANGELOG.md`), and only then, in the
> `publish-crates` job, builds each crate, runs `cargo test --workspace` (default features, not the CI feature
> set) and publishes through trusted publishing. There is no publish dry-run and no semver check in CI or in the
> release workflow. The requirements below close those gaps.

| ID | Requirement | Priority | Acceptance criteria |
|----|-------------|----------|---------------------|
| NFR-MNT-040 | WHEN a release tag `vX.Y.Z` is pushed, THE SYSTEM SHALL verify before any build or publish step that `X.Y.Z` equals the workspace version and every intra-workspace dependency version, and that `CHANGELOG.md` has a section `[X.Y.Z]` with no entry left under `[Unreleased]` | must | A tag whose version differs from the manifest, or with no changelog section, fails the first job; a run on a throwaway tag shows both failures |
| NFR-MNT-041 | THE SYSTEM SHALL run, as a gate that every artifact-producing job depends on (`needs:`), the full check set on the tagged commit with the exact CI invocations: `cargo +nightly fmt --check`, `cargo +stable clippy --all-targets --all-features --workspace -- -D warnings`, `cargo nextest run --all-features --workspace --no-fail-fast`, `cargo test --doc --all-features --workspace`, `cargo-deny`, the MSRV check and the docs build. Nothing public (GitHub Release, archives, crates) SHALL be created before the gate passes | must | In the workflow graph no job that uploads a release asset or calls `cargo publish` can start unless the gate job succeeded; the gate job's commands match `.github/workflows/ci.yml` |
| NFR-MNT-042 | THE SYSTEM SHALL run `cargo publish --workspace --dry-run` as a gate before the real publish, and SHALL run it on every pull request that changes a manifest, the lockfile or the release workflow | must | The dry-run step exists in the release workflow before the publish step and in CI behind a path filter; a manifest error (missing metadata, an unpublishable path dependency) fails the PR |
| NFR-MNT-043 | THE SYSTEM SHALL publish the seven crates in dependency order and SHALL be safe to re-run after a partial failure: a crate version already on crates.io is skipped, and a crate version different from the tag is never published | must | Re-running the workflow on a tag where the first crates are already published completes the rest; the job log lists published, skipped and failed crates |
| NFR-MNT-044 | THE SYSTEM SHALL take the GitHub Release body from the `[X.Y.Z]` section of `CHANGELOG.md` verbatim and SHALL fail when that section is missing or empty; it SHALL NOT use `git log` subjects or auto-generated notes as the body | must | The release body of a tag equals the changelog section byte for byte; a tag with an empty section fails before the release is created |
| NFR-MNT-045 | THE SYSTEM SHALL list every change that is breaking per `NFR-COM-040` under `### Breaking` of the version's changelog section, one line ending with the PR link | must | The `semver-checks` result and the review are compared against the section before the tag; a breaking PR without the entry blocks the release |
| NFR-MNT-046 | THE SYSTEM SHALL run `cargo semver-checks` in CI for each of the seven published crates against the last published version on crates.io, on every pull request and in the release gate, with the action pinned to a full commit SHA | must (from the 1.0.0 baseline) | The job exists, covers all seven crates and fails on a major-level lint; before 1.0.0 is published there is no meaningful baseline, so the job is added with the 1.0.0 release (#679) and until then runs without failing the build |
| NFR-MNT-047 | WHEN `cargo semver-checks` reports a lint on a change intended as compatible, THE SYSTEM SHALL fix the change or reclassify it as breaking with a major version; a suppression of a lint SHALL carry a comment naming the issue and the reason and is **Ask First** | must | No exclusion, lint allow or baseline override exists without that comment. `cargo semver-checks` cannot see MSRV, behavior, wire or CLI changes, so those stay covered by the `NFR-COM-040` review, the `NFR-COM-055` fixtures and the `NFR-COM-060` to `NFR-COM-062` tests |
| NFR-MNT-048 | THE SYSTEM SHALL keep crates.io publication on trusted publishing (OIDC, no long-lived token) and every workflow action pinned to a full commit SHA with a trailing version comment | must | `.github/workflows/*.yml` contains no `uses:` with a tag or branch; the publish job holds no secret token |
| NFR-MNT-049 | BEFORE the first 1.0.0 tag, THE SYSTEM SHALL complete one rehearsal release (a 0.12.0 trial as in #669) that passes every gate above and leaves the latest version of all seven crates on crates.io | must (one time) | The rehearsal run links to the passing gate, dry run and publish; crates.io lists the trial version for the seven crates |

### 12.7 Open decisions (Ask First)

> [!question] OD-1: rename "progressive" before 1.0.0? (#680)
> Today `ProgressiveGenerator`, the `progressive` module of `mcp-execution-codegen`, the `--progressive-output`
> flag and `examples/progressive-loading-usage.md` carry the old name of the product direction; 45 files under
> `crates/` mention it. After 1.0.0 a rename is a major release.
> **Options.** (a) Rename before 1.0.0 (type, module, flag, example file) and record it as `Breaking`.
> (b) Keep the names deliberately and say why in the spec. (c) Rename the Rust items and keep the flag.
> **Recommendation.** (a): it is the only moment the rename costs nothing, and the project already describes
> itself as script-first. The new names are part of the decision and are not proposed here. No shim, per
> `NFR-MNT-002`. Affects `NFR-COM-031`, `NFR-COM-060`.

> [!question] OD-2: wire-format compatibility window (#670)
> `_meta.json` v3 and helper protocol v1 are read by exact match today (`NFR-COM-052`).
> **Options.** (a) Exact match for the whole 1.x line: the version never changes inside a major, a bump happens in
> a major release only, and regeneration is the migration. (b) "Read old, write new": a consumer accepts the
> current and the previous version, a producer writes the current. (c) Accept any version at or above a floor.
> **Recommendation.** (a), with additive optional fields allowed inside the version (`NFR-COM-053`): the output is
> reproducible from a live server, the helper and the bridge are produced and consumed by one release family, and
> (b) doubles the read paths and the fixtures. A bump then needs the major release that `NFR-COM-051` already
> demands. Open inside it: the saved-script header has no version field; add one before 1.0.0 or document it as
> unversioned. Affects `NFR-COM-051` to `NFR-COM-056`.

> [!question] OD-3: MSRV against semver for libraries (#670)
> **Options.** (a) An MSRV raise is not breaking: a minor release only, never a patch, at most stable minus two,
> always in the changelog under `Changed`. (b) It is breaking: a major release. (c) As (a), plus an advance notice
> in the changelog one minor release before the raise.
> **Recommendation.** (a): it matches the constitution and common practice for Rust libraries, a major per MSRV bump
> would mean a major every few months under the rolling policy, and Cargo's resolver can select a compatible
> version. Affects `NFR-COM-070` to `NFR-COM-072`, `NFR-POR-012`.

> [!question] OD-4: foreign types in public signatures (#678)
> **Options.** (a) Forbid: wrap `EnvFilter` (or drop `cap_rmcp_log_level` from the public API) and hide the
> `ServerHandler` implementation. (b) Accept all, list them, and tie each crate's major version to the foreign
> crate's major. (c) Split: accept `rmcp::ServerHandler` on `mcp-execution-server` only, because being an `rmcp`
> server is that crate's purpose, and keep `tracing-subscriber` out of `mcp-execution-core`.
> **Recommendation.** (c). `EnvFilter` couples the foundation crate, which every other crate depends on, to a
> logging crate's major version for a helper only the CLI uses; the `rmcp` implementation is inherent to the server
> crate and its coupling is recorded by `NFR-COM-044` (see also
> [[decisions/ADR-529-rmcp-3-5-latest-promotion-revisit]]). Also decide the other exposures found in the code: `chrono::DateTime`, `serde_json::Value` and `serde_json::Error`,
> `handlebars` errors in `mcp-execution-skill`, and the `serde` and `schemars` implementations (recommendation: accept
> `serde`, `serde_json`, `schemars` and `chrono` and list them; wrap the two `handlebars` error types, since the
> template engine is an implementation detail). Affects `NFR-COM-044`.

> [!question] OD-5: intended public surface of the `cli` and `server` libraries (#675)
> The listings in [[cli/spec#20. 1.0 Public API Contract]] and [[server/spec#15. 1.0 Public API Contract]] are
> proposals derived from the code (what the binaries and tests need), not decisions.
> **Options.** (a) Minimal: the library exposes only what an embedder needs (the `Cli` and `Commands` parse types and
> an entry point for `cli`; `GeneratorService` with its parameter and result types for `server`), and everything
> else is `pub(crate)` or `#[doc(hidden)]`. (b) Keep the modules public as today. (c) Publish `cli` and `server` as
> binaries only.
> **Recommendation.** (a); (c) contradicts the maintainer's decision that all seven crates are stable libraries.
> Affects `NFR-COM-032`, `FR-047.6`, `FR-047.7`.

### 12.8 Verification and traceability

| ID | Method | Environment | Frequency |
|----|--------|-------------|-----------|
| NFR-COM-030 to NFR-COM-035 | Rustdoc item list or `cargo public-api` compared with the crate spec listing; the checks of the acceptance column | CI and review | Every PR that changes a `pub` item |
| NFR-COM-040 to NFR-COM-046 | `cargo semver-checks` plus review against the clause list | CI (`NFR-MNT-046`) and review | Every PR and release |
| NFR-COM-050 to NFR-COM-056 | Golden fixtures and mismatch tests under `crates/*/tests/` | CI (`test` job) | Every push/PR |
| NFR-COM-060 to NFR-COM-066 | `docs_examples` test, golden JSON tests and binary-level exit-code tests | CI (`test` job) | Every push/PR |
| NFR-COM-070 to NFR-COM-072 | `msrv` job and diff review | CI | Every push/PR |
| NFR-MNT-040 to NFR-MNT-049 | Release workflow gates, a throwaway-tag run and the rehearsal release | Release workflow | Every tag, and once before 1.0.0 |

| BRD | SRS | NFR | Block specs |
|-----|-----|-----|-------------|
| FR-047 | FR-047 | NFR-COM-030 to NFR-COM-035, NFR-COM-040 to NFR-COM-046, NFR-COM-050 to NFR-COM-056, NFR-COM-070 to NFR-COM-072 | `FR-047.1` to `FR-047.7` in the seven crate specs |
| FR-048 | FR-048 | NFR-COM-060 to NFR-COM-066 | [[cli/spec]], [[server/spec]] |
| FR-049 | FR-049 | NFR-MNT-040 to NFR-MNT-049 | [[README#1.0 Contract]] |

## See Also

- [[BRD-mcp-execution-2026-07-27]] — business requirements (source)
- [[SRS-mcp-execution-2026-07-27]] — functional requirements
- [[README]] — project knowledge base / cross-block index
