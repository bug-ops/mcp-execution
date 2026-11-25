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
status: proposed
decided: 2026-11-25
decided-by: user (stable crate set only)
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
> **Status: proposed.** One decision is taken (2026-11-25, by the user): **all seven workspace crates are stable
> 1.x libraries.** Five points are open and need the user's decision before the 1.0.0 tag. The numbered
> requirements are in [[../NFR-mcp-execution-2026-07-27#12. 1.0 Contract Requirements]].

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

## 3. Open decisions (Ask First)

| ID | Question | Options | Recommendation |
|----|----------|---------|----------------|
| OD-1 | Rename "progressive" before 1.0.0 (#680)? | (a) rename the Rust items, the flag and the example; (b) keep deliberately; (c) rename items, keep the flag | (a): free now, a major release later |
| OD-2 | Compatibility window of `_meta.json` v3 and helper protocol v1 | (a) exact match for all of 1.x, a bump only in a major, regeneration is the migration; (b) read the current and previous version, write the current; (c) accept any version at or above a floor | (a), with additive optional fields inside a version; decide whether the unversioned saved-script header gets a version before 1.0.0 |
| OD-3 | Is an MSRV raise breaking for the libraries? | (a) no: minor only, never patch, at most stable minus two, changelog `Changed`; (b) yes: major; (c) as (a) with one minor of advance notice | (a) |
| OD-4 | Foreign types in public signatures (#678) | (a) forbid, wrap or hide all; (b) accept all, tie majors; (c) accept `rmcp::ServerHandler` on the server crate only, keep `tracing-subscriber` out of `mcp-execution-core` | (c), accept and list `serde`, `serde_json`, `schemars`, `chrono`; wrap the `handlebars` errors |
| OD-5 | Intended public surface of the `cli` and `server` libraries (#675) | (a) minimal embedder surface, the rest `pub(crate)` or hidden; (b) keep all public; (c) binaries only | (a); (c) contradicts the stable-set decision |

Details, the affected requirement IDs and the exposures found in the code (`serde_json::Value`, `serde_json::Error`,
`handlebars` errors, `chrono::DateTime`, `clap` derives) are in the NFR section 12.7 and `NFR-COM-044`.

## 4. Consequences

- Until the open decisions are made, the requirements that depend on them state what holds meanwhile: the as-built
  exact-match rule for wire versions, the rolling MSRV rule recorded under `Changed`, and no new foreign type.
- #673 and #674 to #680 cite the requirement IDs; #679 adds the `cargo semver-checks` job with its baseline at the
  1.0.0 release.
- #683 (per-tool digests, left out of #580) changes `_meta.json` if implemented, so it is decided before the tag
  (`NFR-COM-056`).
- Rejecting a recommendation changes the affected requirement text only; no code changes in the specification PR.

## 5. See Also

- [[../NFR-mcp-execution-2026-07-27]] — section 12, the requirements
- [[../README]] — the 1.0 contract index
- [[../constitution]] — the MSRV rule (II. Technology Stack)
