---
aliases:
  - ADR-405
  - adopt serde-saphyr
tags:
  - sdd
  - decision
  - security
  - skill
created: 2026-08-10
status: accepted
supersedes: ADR-341
review-date: 2027-02-10
related:
  - "[[../constitution]]"
  - "[[../skill/spec]]"
  - "[[ADR-341-serde-saphyr-vs-serde-norway]]"
---

# ADR-405: Adopt `serde-saphyr` as `mcp-execution-skill`'s YAML parser

> [!important]
> This record documents an **owner override** of [[ADR-341-serde-saphyr-vs-serde-norway]]'s
> decision (c) ("monitor and revisit at a measurable gate"), not a passed gate. ADR-341's §7
> gate stood at **1 of 3** criteria met at override time — see §1 below. The override is a
> deliberate risk acceptance, not a claim that the risk this project previously declined to
> take on has gone away.

## 1. Gate status at override time (ADR-341 §7)

ADR-341 §7 required all three of the following before revisiting decision (c):

1. `granit-parser` reaches **>=1.0.0 GA** (non-rc) — **met**: `granit-parser` 1.0.1 and
   `serde-saphyr` 1.0.1 are both published, non-rc releases.
2. **>=2 identity-resolved contributors with >=10 commits each** in a rolling 6-month window —
   **not met**. Per ADR-341 §4's identity resolution (`bourumir-wyngs` /
   `bourumir.wyngs@gmail.com` and `audrius.meskauskas@ethz.ch` are the same human, confirmed via
   the committer's own GitHub profile bio, not by trusting `.author.login` or raw email
   uniqueness), post-fork `granit-parser` is **126 of 127 commits by one person** — stated
   plainly, not as "effectively single-maintainer." This has not changed since ADR-341.
3. **>=5 reverse dependencies**, excluding crates by the same identity-resolved maintainer —
   **not met**. ADR-341 §3.6 recorded exactly 2 independent reverse dependencies (`ryl`,
   `cwl-lsp`; `serde_yaml_bw` shares `granit-parser`'s maintainer identity).

**Gate score: 1 of 3.** Proceeding with the swap is therefore an explicit project-owner
override of ADR-341's own recorded decision, made before the gate's own stated conditions were
satisfied — a deliberate risk acceptance, not a technical finding that the risk retired itself.

### ADR-341 §7's identity-resolution instruction carries forward, unretired

ADR-341 §7 is explicit that a naive contributor count (grouping by `.author.login` or raw
commit email) **false-passes** criterion 2 today, and that whoever re-runs the gate must repeat
the identity-resolution step, not just the commit count. This override does **not** retire that
instruction: `granit-parser`'s maintainer-concentration risk is being accepted now, not
resolved, so a future re-assessment of this crate (e.g. at this ADR's review date) must still
apply ADR-341 §4's identity-resolution method rather than trusting a naive contributor count.

## 2. Resolutions to the three owner rulings

These three rulings were made directly by the project owner and are binding on this
implementation; they are recorded here, not re-litigated:

1. **Block/folded YAML scalar trailing newline: do NOT `trim_end()`.** The previous
   `serde_norway`-based parser silently stripped the YAML-1.2 clip-chomped trailing newline;
   `serde-saphyr` is YAML-1.2-correct and keeps it. This implementation accepts the correct
   value rather than normalizing it away — a **BREAKING** behavior change, recorded in
   `CHANGELOG.md`.
2. **`num-traits <=0.2.19` dependency ceiling: accepted, closed.** This workspace already sits
   exactly at this ceiling (ADR-341 §3.6); `serde-saphyr` freezes it there. No CI/`deny.toml`
   rule enforces this — `cargo update -p num-traits` simply refuses to move past the ceiling on
   its own, so there is nothing meaningful to encode in tooling. This is closed by acceptance,
   not by enforcement.
3. **`SkillMetadataError` gains `#[non_exhaustive]` in this same change.** Verified risk-free:
   zero references to this enum outside `crates/mcp-skill` (re-confirmed by grep during
   implementation), and the workspace is pre-1.0 (`0.9.0`). Applied alongside the new
   `FrontmatterTooComplex` variant so future variants are additive, not breaking.

## 3. What changed in the implementation

Single production call site, unchanged from ADR-341 §1: `crates/mcp-skill/src/parser.rs` —
`extract_skill_metadata`. `RawFrontmatter` (`name`/`description`, both `Option<String>`, no
serde attributes) is unchanged. `serde_norway::from_str` is replaced by
`serde_saphyr::from_str_with_options(block, frontmatter_options())`, where `frontmatter_options`
builds an explicit `serde_saphyr::Options`/`Budget` — every field set individually via the
`options!`/`budget!`/`alias_limits!` macros (struct-literal construction is not available;
`Options`, `Budget`, and `AliasLimits` are all `#[non_exhaustive]`).

`SkillMetadataError` gains one new fieldless variant, `FrontmatterTooComplex`, for a
budget/alias-limit breach; `InvalidYaml(String)`'s shape is unchanged, but the `String` is now
always constructed by this crate from `serde_saphyr::Error::location()` plus a small fixed
vocabulary of failure kinds (`yaml_error_kind`) — never from the parser's own rendered
`Display`/`to_string()` text.

### Budget table (implemented values, `frontmatter_options` in `crates/mcp-skill/src/parser.rs`)

| Setting | Value | Basis |
|---|---|---|
| `max_nodes` | `8_192` | 2 B/node is the densest valid construction (`[a,a,...]`, confirmed — `[,,,,]` is invalid YAML); <=4096 nodes from 8 KiB. 2x margin. |
| `max_events` | `16_384` | 2 events/node, aligned with `max_nodes`. |
| `max_anchors` | `8_192` | `&a` is >=2 B; <=4096 anchors from 8 KiB, 2x margin. Must stay non-zero or `merge_keys` becomes dead configuration. |
| `max_aliases` | `8_192` | `*a` is >=2 B; <=4096 aliases from 8 KiB, 2x margin. |
| `max_total_scalar_bytes` | `65_536` | 8x the input cap; bounds scalar bytes materialized by alias replay. |
| `max_merge_keys` | `4_096` | `<<:` is >=3 B; <=2730 merge keys from 8 KiB, ~1.5x margin. |
| `max_documents` | `2` | This crate's single-document call path never lets the budget's own `max_documents` counter observe a second document: `from_str_with_options` fully parses (and budget-checks) the first document, then peeks for trailing content and raises `Error::MultipleDocuments` directly if any is found — a second document's own `DocumentStart` event is never budget-checked through this entry point. Kept above the theoretical minimum of 1 anyway, matching this table's general margin convention, even though it is not reachable here. |
| `max_depth` | `64` (default, set explicitly) | Deliberate exception to the size-derived sizing rule: 8 KiB of `[[[[...` nests thousands deep, so no size-derived value is meaningful. Only matters on the unknown-key/buffering path — a declared `Option<String>` field short-circuits on the same type mismatch regardless of depth (§4, C3); `max_depth` does not protect that path. |
| `enforce_alias_anchor_ratio` / `alias_anchor_min_aliases` / `alias_anchor_ratio_multiplier` | `true` / `100` / `10` (defaults, set explicitly) | Built-in heuristic; does not fire on this crate's 57-alias reference fixture (57 < 100) — `max_nodes` fires first. |
| `max_total_comment_bytes` | 64 MiB (default, set explicitly) | Unreachable from an 8 KiB input. |
| `max_reader_input_bytes` | 256 MB (default, set explicitly) | Reader-only; `from_str_with_options` never consults it for a `&str` input. |
| `max_inclusion_depth` | `24` (default, set explicitly) | The `include` feature is not enabled. |
| `AliasLimits::max_total_replayed_events` | `16_384` | Aligned with `max_events`; the direct billion-laughs bound. Other two `AliasLimits` fields left at their defaults. |
| `merge_keys` | `MergeKeyPolicy::AsOrdinary` | `<<: *anchor` must not inject fields from an anchored map; verified live (`test_merge_key_treated_as_ordinary_key`). |
| `duplicate_keys` | `DuplicateKeyPolicy::Error` | Equals `serde-saphyr`'s own default; set explicitly so an upstream default change cannot loosen it. |
| `with_snippet` | `false` | Load-bearing for the no-untrusted-source-echo requirement: `Error::WithSnippet` is never constructed. |

**Two settings from this crate's original design table do not exist as real `serde-saphyr`
1.0.1 API surface**, verified against the published source rather than assumed:
`max_buffered_comment_events` and `simple_key_max_lookahead` are not fields on `Budget`,
`Options`, or anywhere else in the crate (confirmed by exhaustive grep over the vendored
source). They are omitted from the implementation rather than guessed at.

## 4. Corrections to ADR-341's Evidence Ledger (C1-C4)

These four corrections were found during implementation, reproduced independently against real
`serde-saphyr`/`granit-parser` 1.0.1 behavior (not derived from documentation), and are recorded
here because they change what ADR-341 asserted, not just how the swap was executed:

- **C1 — `Options::budget` defaults to `Some(Budget::default())`, not `None`.** ADR-341's
  premise that a dropped `budget:` line yields no budget is false; it yields the *default*
  budget (`max_nodes: 250_000`), a ~30x looser bound than this ADR's `8_192`, not a disabled
  one.
- **C2 — a budget breach on the reference alias-bomb fixture surfaces as `Error::AliasError`,
  not `Error::Budget`, but `AliasError` is a generic wrapper, not a budget-specific signal.**
  `AliasError` is what `serde-saphyr` attaches to *any* error raised while deserializing a value
  reached through an alias — including an ordinary, non-amplifying type mismatch that merely
  happens to occur under an alias (e.g. `base: &a [1, 2]\nname: *a`). An earlier draft of this
  implementation matched `Error::AliasError` unconditionally as a budget-breach signal and
  misclassified that case as `FrontmatterTooComplex`; caught in review, fixed by classifying on
  the authoritative `serde_saphyr::budget::BudgetReport::breached` (observed via a registered
  `budget_report` callback) instead, with the direct `Error::Budget` and four
  `AliasReplay*`/`AliasExpansion*` variants kept as a fallback (`is_budget_breach` in
  `crates/mcp-skill/src/parser.rs`; regression test
  `test_alias_wrapped_type_mismatch_is_not_a_budget_breach`).
- **C3 — the budget is NOT shape-independent; ADR-341 §3.3's "shape-independent by
  construction" claim does not hold.** A bomb placed under a *declared* `Option<String>` field
  still short-circuits on serde's type mismatch before the budget accumulates anything, exactly
  as the previous `serde_norway`-based parser did. The budget only defends the *undeclared-key*
  and buffering-field paths, not the declared-field path — see §5 below and
  `RawFrontmatter`'s doc comment.
- **C4 — ADR-341 §3.2's redesigned-budget latency figure (~1.5-1.6 ms) did not reproduce.**
  The correct dense-node floor is 2 bytes/node (`[a,a,...]`), not 1 byte/node (`[,,,,]`, which
  is not valid YAML). This halves the safe `max_nodes` ceiling this ADR settles on (`8_192`
  rather than `16_384`) and changes the measured worst case — see §6.

## 5. C3 in practice: the budget is not shape-independent

An alias bomb placed under a key `RawFrontmatter` does not declare is materialized by the
generic visitor and reaches the budget — rejected as `FrontmatterTooComplex`
(`test_extract_skill_metadata_alias_bomb_under_unknown_key_rejected_by_budget`, **BREAKING**:
this input was accepted as `Ok` under the previous parser). The same bomb placed directly under
a *declared* field instead short-circuits on a type mismatch before the budget accumulates
anything — unchanged from the previous parser, surfacing as `InvalidYaml`
(`test_extract_skill_metadata_alias_bomb_under_declared_field_short_circuits`). A third test,
`test_alias_bomb_rejection_survives_a_buffering_field_shape`, proves the budget *does* defend a
buffering field shape (a `#[serde(deserialize_with)]` that buffers through `serde_json::Value` —
`serde_saphyr::Value` does not exist under this crate's `deserialize`-only feature set): it calls
`serde_saphyr::from_str_with_options` directly against a local test-only struct and asserts
`is_budget_breach` on the resulting error, rather than going through `extract_skill_metadata`
and a `SkillMetadataError`.

## 6. Measured latency

Best-of-20, both debug and release, on the reference alias-bomb fixture (445 B), the densest
legitimate 8 KiB flow sequence (8190 B), and a typical frontmatter block (58 B). Three
independent measurements, all the same order of magnitude, on different machines/probes:

| Measurement | typical | alias bomb (reject) | densest legitimate 8 KiB (accept) |
|---|---|---|---|
| Architect probe (release) | 2.4 us | 2.08 ms | 1.60 ms |
| Critic probe (release) | 2.3 us | 1.42 ms | 1.19 ms |
| **Implementation probe (release)** | **2.1 us** | **1.31 ms** | **1.15 ms** |
| Implementation probe (debug) | 19.1 us | 13.0 ms | 12.8 ms |

**Accepted worst case: <=3 ms release** on `save_skill`'s synchronous request-handling task.
**Reject/rework threshold: >10 ms release**, or any measured non-bomb input exceeding 3 ms.
Every measurement above clears the accepted bar with margin. Measured on this implementation's
machine via a standalone probe outside the repository, mirroring `frontmatter_options` and the
in-repo alias-bomb fixture exactly (probe not committed).

## 7. Corrections to ADR-341's stated pros/cons

- **M8 — ADR-341 §2(a)'s pro "closes the `deny.toml` `unsound = "all"` special case entirely"
  does NOT materialize.** `unsound = "all"` is kept deliberately (see `deny.toml`'s rewritten
  header comment) — cargo-deny's `unsound` default (`"workspace"`) still silently drops
  advisories against *any* transitive dependency, not specifically the previous YAML parser, so
  removing that parser does not make the setting redundant.
- **M7 — rollback is NOT "cheap."** The code revert is a single commit (two manifests plus
  `parser.rs` and `deny.toml`). The **doc footprint is not revertible in kind**: this ADR marks
  ADR-341 `superseded`; undoing the swap requires a *third* ADR superseding this one and
  reinstating ADR-341's status, plus re-editing constitution §II/§V, `specs/skill/spec.md` §7,
  and the SRS reference. "Cheap" describes the code path only.
- **Unsafe-code delta: `-181` `unsafe fn`, confirmed by independent re-measurement.** Removed:
  `unsafe-libyaml-norway` 0.2.15 (228 `unsafe fn`, re-counted via `grep -c` over the vendored
  source, matching ADR-341 §3.1 exactly) + `serde_norway` 0.9.42 (8). Added: `encoding_rs`
  0.8.35 (40) + `arraydeque` 0.5.1 (15); `granit-parser`/`serde-saphyr` themselves are genuinely
  `#![forbid(unsafe_code)]` (0 added), re-confirmed. Net: 236 − 55 = **-181**, reproducing
  ADR-341 §3.1's figure exactly. `cargo tree -p mcp-execution-skill` resolves the predicted
  11-crate addition with no `base64`/`zmij`/`nohash-hasher` (confirming `default-features =
  false, features = ["deserialize"]` is doing its job — `zmij` is pulled into the workspace by
  `serde_json` independently and is unrelated to this swap).
- **`encoding_rs_io` remains non-optional** inside `serde-saphyr`'s `deserialize` feature
  despite this crate only ever calling the `from_str`/`&str` path, never `from_reader` — an
  upstream feature-gating request, unresolved by this ADR (ADR-341 §8.1, carried forward as a
  follow-up, not filed as part of this change's scope).

## 8. Verification performed

- `cargo +nightly fmt --check`, `cargo +stable clippy --all-targets --all-features --workspace
  -- -D warnings`, `cargo nextest run --all-features --workspace --no-fail-fast`, `cargo test
  --doc --all-features --workspace` — all pass.
- `cargo tree -i num-traits -e normal` and `cargo tree -p mcp-execution-skill` — resolved tree
  matches the predicted shape (§7).
- `cargo deny check licenses bans advisories sources` — passes; `encoding_rs`'s
  `(Apache-2.0 OR MIT) AND BSD-3-Clause` license is already covered by the existing allow-list.
- `RUSTDOCFLAGS="--deny rustdoc::broken_intra_doc_links" cargo doc --no-deps --workspace` —
  passes.
- Zero references to `SkillMetadataError` outside `crates/mcp-skill` (re-confirmed by grep,
  see ruling 3 in §2) — the `#[non_exhaustive]` addition plus new variant is not breaking for
  any in-repo caller.

## See Also

- [[ADR-341-serde-saphyr-vs-serde-norway]] — the superseded decision this ADR overrides, kept as
  historical record (Evidence Ledger not rewritten; corrections recorded separately in §4 above)
- [[../constitution#V. Security]] — YAML parse-time bound, explicit parse `Budget`, and
  no-untrusted-source-echo principles this ADR implements
- [[../skill/spec#7. `extract_skill_metadata` — Frontmatter Parsing]] — the current
  `serde-saphyr`-based implementation this ADR documents
