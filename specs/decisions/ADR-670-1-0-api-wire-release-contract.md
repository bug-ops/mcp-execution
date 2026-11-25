---
aliases:
  - ADR-670
  - 1.0 contract
tags:
  - sdd
  - decision
  - release
  - api
created: 2026-11-25
status: accepted
decided: 2026-11-25
decided-by: user (stable crate set, OD-1, OD-2, OD-4, OD-5, per-tool digest deferral); OD-3 open
related:
  - "[[../constitution]]"
  - "[[../README]]"
  - "[[../NFR-mcp-execution-2026-07-27]]"
  - "[[../core/spec]]"
  - "[[../cli/spec]]"
  - "[[ADR-529-rmcp-3-5-latest-promotion-revisit]]"
---

# ADR-670: The 1.0 API, Wire-Format and Release Contract

> [!important]
> Decision record for #670 (parent #669, API freeze #673 with #674 to #680).
>
> **Status: accepted, one point open.** Decided on 2026-11-25 by the user: all seven workspace crates are stable
> 1.x libraries, and the decisions of section 3. Only OD-3 (MSRV against semver) is open. The numbered requirements
> are in [[../NFR-mcp-execution-2026-07-27#12. 1.0 Contract Requirements]].

## 1. Context

Anything public at 1.0.0 becomes a semver commitment. At 9473243 the specs say nothing about what 1.0 promises:
`#[non_exhaustive]` exists on one type; the `cli` and `server` libraries export their internals; one concept is
defined three times; foreign types (`rmcp`, `tracing-subscriber`) appear in public signatures; server ids are
`String`; CI has no semver check; the release workflow creates the GitHub Release before it runs tests, builds its
notes from `git log` and has no publish dry-run. The constitution says the MSRV is raised "in a minor release",
which sits unclearly beside library semver.

## 2. Decision taken

All seven crates (`mcp-execution-core`, `-introspector`, `-codegen`, `-files`, `-skill`, `-server`, `-cli`) are
declared stable 1.x libraries at one workspace version. Consequences: each needs a listed public surface
(`NFR-COM-031`, `NFR-COM-032`), a `#[non_exhaustive]` pass (`NFR-COM-042`), and `cargo semver-checks`
(`NFR-MNT-046`).

Not a user decision but recorded here as an assumption to confirm: lockstep versioning (`NFR-COM-030`), the
deprecation window of one minor release (`NFR-COM-045`), and that `--format text` and `--format pretty` are not
part of the CLI contract (`NFR-COM-065`).

## 3. Decisions on the Ask First points

| ID | Plan id | Question | Decision (2026-11-25) |
|----|---------|----------|------------------------|
| OD-5 | D1 | Public surface of `cli` and `server` (#675) | (a), narrower than the first proposal: `cli` exposes only `Cli` and `run` (`runtime` is `#[doc(hidden)]`); `server` hides `clock`, `state` and the output-directory re-exports |
| OD-4 | D2 | Foreign types (#678) | (c): `rmcp::ServerHandler` on the server only; core drops `log_filter`, the `tracing-filter` feature and `tracing-subscriber`; the cap policy is the `#[doc(hidden)]` `core::cli::RmcpLogCap` (workspace-internal, no copied helper); `handlebars` errors are boxed; `serde`, `serde_json`, `schemars`, `chrono`, and `clap`/`anyhow` on `cli`, are accepted |
| OD-1 | D3 | Rename "progressive" (#680) | (a): module `package`, `PackageGenerator`, `templates/package/`, flag `--servers-dir`, `examples/generated-package-usage.md`; no shim |
| none | D4 | Per-tool digests (#683) | Deferred past 1.0; if added, an optional `_meta.json` v3 field (`NFR-COM-053`); post-1.0 work in #688 |
| OD-2 | OD-2 | Wire compatibility window | (a): exact match for 1.x, a bump only in a major, additive optional fields inside a version. Not decided: whether the saved-script header gets a version before 1.0.0 |
| OD-3 | none | MSRV raise against semver | **Open.** The recommendation stays (a); meanwhile the rolling rule recorded under `Changed` holds |

Also decided with the API freeze: the server-id slug rule `[a-z0-9-]{1,64}` is frozen with no hyphen-position rule
(#677, [[../core/spec]]).

## 4. Consequences

- The decisions above are implemented with the API freeze (#673, #674 to #680) and the release pipeline (#672).
- Still open before the 1.0.0 tag include #686 (the `semver` job becomes blocking at the baseline), #687 (remaining
  public surface), #691 and #696.
- The `cargo semver-checks` job of #679 exists; its step is non-blocking until the 1.0.0 baseline.

## 5. See Also

- [[../NFR-mcp-execution-2026-07-27]] — section 12, the requirements
- [[../README]] — the 1.0 contract index
- [[../constitution]] — the MSRV rule (II. Technology Stack)
