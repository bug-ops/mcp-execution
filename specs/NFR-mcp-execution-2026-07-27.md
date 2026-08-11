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
> `0.8.0`. Based on ISO/IEC 25010:2011. Traceable to
> [[BRD-mcp-execution-2026-07-27]]. Every target below is either (a) a
> literal constant already enforced in the codebase, (b) a measured figure
> already published in `README.md`, or (c) explicitly marked as **not
> enforced/not measured** where the repository asserts an intention (a doc
> comment target) without a corresponding CI gate. No numeric target in this
> document was invented for the occasion.

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
| NFR-PERF-003 | Per-tool token footprint of a loaded generated file | ~500–1,500 tokens/tool | Not an automated measurement — a documented estimate in `README.md`/`specs/README.md`, not re-derived by any test or benchmark in this repository | N/A |

> [!warning] Assumptions
> The headline "98% token savings" figure (`README.md`) and the "83%
> asymptotic maximum" figure (`CHANGELOG.md` `[0.2.0]` "Notes") are **not**
> reconciled anywhere in the repository, and neither is backed by an
> automated, reproducible benchmark measuring actual end-to-end token
> consumption in a real Claude Code session. Both NFR-PERF-003 and the
> savings percentage itself should be treated as documented estimates, not
> verified NFRs, until such a benchmark exists — see
> [[BRD-mcp-execution-2026-07-27#Open Questions]].

### 2.2 Resource Utilization

| ID | Requirement | Target | Measurement |
|----|------------|--------|-------------|
| NFR-PERF-010 | Memory usage generating 1000 tools | < 256 MB (README target) | Reported achieved: ~2 MB (`README.md` Performance table); also profiled via a dedicated `dhat`-based heap-profiling example (`crates/mcp-files/examples/profile_memory.rs`, `dhat-heap` feature) |
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
| NFR-SEC-023 | The generated TypeScript runtime bridge cannot silently drift from the Rust source of truth for its own security rules | `FORBIDDEN_CHARS`/`FORBIDDEN_ENV_NAMES`/`FORBIDDEN_ENV_PREFIX`/`ENV_NAME_CHARSET_REGEX`/`ENV_NAME_CHARSET_DESC` (#467) and the DoS size/count ceilings `MAX_ARG_COUNT`/`MAX_ARG_LEN`/`MAX_ENV_COUNT`/`MAX_ENV_VALUE_LEN`/`MAX_URL_LEN`/`MAX_HEADER_COUNT`/`MAX_HEADER_VALUE_LEN` (#471) are rendered into `_runtime/mcp-bridge.ts` directly from `mcp_execution_core`'s Rust constants/accessors at generation time (via `BridgeContext::default()`, hand-written with no `derive(Default)` so it can never render a fail-open/empty bridge), not hand-copied; the string-valued renders that participate in matching (not just message text) additionally go through `sanitize_ts_string_literal` so a future Rust-side value containing `'`/`\` cannot silently change what the generated code matches |
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
| NFR-COM-011 | Runtime dependency for executing generated output | Node.js ≥ 18.0.0, checked by `setup` |
| NFR-COM-012 | Coexistence with other MCP-aware tools sharing `~/.claude/mcp.json` | `McpServerEntry` deserialization warns (not fails) on unrecognized top-level keys (e.g. another tool's `disabled`/`alwaysAllow` keys), so this project's CLI can read a config file shared with tools it doesn't otherwise know about |

> [!note] Not Applicable
> No browser or mobile compatibility requirement applies — there is no
> browser-rendered or mobile-app surface anywhere in this project.

## 7. Maintainability

### 7.1 Modularity & Modifiability

| ID | Requirement | Details |
|----|------------|---------|
| NFR-MNT-001 | Architecture style | Layered workspace (Cargo workspace, 7 crates), strict low→high dependency direction, zero circular or upward dependencies (`mcp-core` has zero intra-workspace dependencies) — see [[README#Block Structure]] |
| NFR-MNT-002 | API versioning | Semantic Versioning (workspace version `0.8.0`, pre-1.0 — breaking changes are permitted and are documented under `[Unreleased]`/`Breaking` in `CHANGELOG.md` rather than deferred) |
| NFR-MNT-003 | Lint policy as a modularity/quality gate | Clippy `all`/`cargo`/`nursery`/`pedantic` set to `deny` at workspace level; any `#[allow(...)]` narrower than workspace scope requires a justification comment (enforced by convention, backfilled per `CHANGELOG.md` `[Unreleased]`/`Documentation`, #186) |

### 7.2 Testability

| ID | Requirement | Details |
|----|------------|---------|
| NFR-MNT-010 | Test count | README states "657 tests with 100% pass rate" — a point-in-time claim as of the README's last edit, not independently re-verified while writing this document |
| NFR-MNT-011 | Coverage measurement vs. enforcement | Coverage is generated and uploaded to Codecov on every CI run (`coverage` job, `cargo llvm-cov nextest`), but **`fail_ci_if_error: false`** is set on the upload step and no numeric coverage threshold gate exists in `.github/workflows/ci.yml` — coverage is tracked, not enforced. No specific percentage target should be asserted as a requirement without adding such a gate. |
| NFR-MNT-012 | Doc-tests as executable documentation | `#![warn(missing_docs)]` workspace-wide; every `pub` item requires a doc comment with a runnable `# Examples` section, verified by `cargo test --doc --all-features --workspace` in CI |
| NFR-MNT-013 | CI/CD pipeline | Build + lint (`fmt --check`, `clippy -D warnings`) + test (`nextest`) + doc-test + coverage + MSRV check + `cargo-deny` security/license/source audit + benchmark build, on every push/PR (`.github/workflows/ci.yml`) |
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
| NFR-POR-010 | Installation methods | `cargo install mcp-execution-cli` (crates.io, recommended); pre-built release binaries (GitHub Releases, per-platform archives); build from source (`cargo install --path crates/mcp-cli`); individual library crates via `cargo add` |
| NFR-POR-011 | Configuration management | A single JSON file, `~/.claude/mcp.json`, read directly from the filesystem, plus one narrow environment-variable exception: `MCP_EXECUTION_LOG_FORMAT` (issue #399) selects diagnostic log format (`text`/`json`) for `mcp-execution-cli` and the `mcp-execution` server binary — an operational logging-output switch, not a secrets-manager-style configuration path, with no secret-shaped value ever accepted or echoed. No other environment-variable-based or secrets-manager-based configuration path exists for this project's *own* configuration (as opposed to the *target* MCP servers it introspects, whose env vars it explicitly validates — see [[#4.1 Confidentiality]]) |
| NFR-POR-012 | MSRV policy | Rust 1.91 minimum, enforced by a dedicated CI job (`msrv`); README states MSRV increases are treated as minor version bumps |

## 9. Verification Matrix

| ID | Method | Environment | Frequency |
|----|--------|-------------|-----------|
| NFR-PERF-001, NFR-PERF-002 | `criterion` benchmark | Local dev machine / CI (`cargo bench --no-run --profile bench-fast` build-only in CI; full run is manual) | Ad hoc / per performance-sensitive change |
| NFR-PERF-010 | `dhat`-based heap profiling (`profile_memory` example, `dhat-heap` feature) | Local dev machine | Ad hoc |
| NFR-SEC-001–025 | Unit + integration test suite (`cargo nextest run --all-features --workspace`) | CI (`test` job) | Every push/PR |
| NFR-SEC-021 (advisories/licenses/sources) | `cargo-deny` | CI (`security` job) | Every push/PR |
| NFR-MNT-010–014 | `cargo nextest run`, `cargo test --doc` | CI | Every push/PR |
| NFR-MNT-011 (coverage) | `cargo llvm-cov nextest` → Codecov upload | CI (`coverage` job, Linux only) | Every push/PR; **not gating** (`fail_ci_if_error: false`, no threshold check) |
| NFR-POR-012 (MSRV) | `cargo check`/`build` pinned to Rust 1.91 | CI (`msrv` job) | Every push/PR |

## 10. Open Questions

> [!question] Unresolved Quality Requirements
> - [ ] Should coverage become a gating CI check (a numeric threshold), or
>   remain tracked-only as it is today?
> - [ ] Is there an intended target for closing the introspector's HTTP/SSE
>   response-size gap (see [[SRS-mcp-execution-2026-07-27#FR-002]]), and
>   should a quantitative NFR (e.g. a maximum buffered-response size) be set
>   for it once `rmcp` exposes the necessary knob?
> - [ ] Should the 98% token-savings claim be backed by an automated,
>   reproducible benchmark (making it a verifiable NFR rather than a
>   documented estimate)?
> - [ ] Is any future hosted/multi-tenant deployment mode planned (which
>   would reintroduce Availability/Scalability as real, specifiable NFR
>   categories rather than "not applicable")?

## See Also

- [[BRD-mcp-execution-2026-07-27]] — business requirements (source)
- [[SRS-mcp-execution-2026-07-27]] — functional requirements
- [[README]] — project knowledge base / cross-block index
