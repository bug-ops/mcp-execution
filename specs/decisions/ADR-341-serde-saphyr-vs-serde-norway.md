---
aliases:
  - ADR-341
  - serde-saphyr vs serde_norway evaluation
tags:
  - sdd
  - decision
  - security
  - skill
created: 2026-07-27
status: accepted
review-date: 2026-10-27
related:
  - "[[../constitution]]"
  - "[[../skill/spec]]"
---

# ADR-341: Evaluate `serde-saphyr` as a replacement for `serde_norway`

> [!important]
> This is a decision record, not a feature spec. It documents a research
> evaluation and its outcome; it authorizes no code change. All figures
> below were independently measured against real crate sources (not cited
> from documentation) across two rounds of architect/critic review.

## 1. Context

[[#293]] found that `unsafe-libyaml-norway` — pulled in transitively via
`serde_norway`, this workspace's sole YAML parser (see
[[../constitution#II. Technology Stack]]) — is by far the largest
unsafe-code concentration in the dependency tree (213/222 unsafe
functions, ~14,500 unsafe LOC). #293 scoped a parser swap out and shipped
advisory monitoring only (`deny.toml` `unsound = "all"`).

[[#341]] asks the follow-up question #293 deliberately deferred: is
`saphyr`/`serde-saphyr` — a from-scratch, pure-Rust, YAML-1.2 parser —
mature enough to replace `serde_norway` outright?

**Issue #341's premise is stale as of `serde-saphyr` >= 0.0.27 (2026-05-26).**
The issue names `saphyr-parser` (328 stars, 8 contributors, active since
2024-04-02) as the backend. From 0.0.27 onward, `serde-saphyr`'s actual
parser dependency is **`granit-parser`**, the `serde-saphyr` author's own
fork of `saphyr-parser` 0.0.6 — created 2026-04-30 (~12 weeks old at
evaluation time), 11 stars, whose own README states it "has since diverged
significantly and is now maintained as an independent project." Adopting
`serde-saphyr` today does not buy into the mature `saphyr` project; it
buys into a ~12-week-old, effectively single-maintainer fork. This
correction is load-bearing for every option below.

Usage surface in this workspace is a single call site:
`crates/mcp-skill/src/parser.rs` — `extract_skill_metadata` deserializes
an 8 KiB-capped frontmatter block (`RawFrontmatter`: two `Option<String>`
fields, no serde attributes) via `serde_norway::from_str`, plus
`describe_yaml_error` for file-relative line/column correction. Its
public error type (`SkillMetadataError::InvalidYaml(String)`) already
stores a rendered string, so no caller is pinned to either parser's error
type — a swap would be API-invisible outside this one file. See
[[../skill/spec#7. `extract_skill_metadata` — Frontmatter Parsing]].

## 2. Options Considered

### (a) Swap now to `serde-saphyr` (caret `"1.0.0-rc.1"`, auto-adopts GA)

- **Pros:** −181 `unsafe fn` net; closes the `deny.toml` `unsound = "all"`
  special case entirely rather than monitoring it; more accurate error
  locations; a semantically bounded parse budget replaces an opaque byte
  cap; `serde-saphyr`/`granit-parser` are themselves `#![forbid(unsafe_code)]`.
  A caret requirement (verified via `cargo generate-lockfile`) resolves
  `1.0.0-rc.1` today and rolls forward to `1.0.0` GA unattended — no `=`
  pin and no follow-up PR needed, so this option is not blocked on GA
  timing the way an earlier pass of this evaluation assumed.
- **Cons:** the real backend, `granit-parser`, is a 12-week-old,
  bus-factor-1 project (§4). Three behavioral diffs must be absorbed
  (§4). Requires a redesigned parse `Budget` shipped in the same change —
  `serde-saphyr`'s *default* settings are a measured DoS regression on
  this exact axis #341 raises (§4). `num-traits` sits exactly at
  `serde-saphyr`'s declared ceiling (`<=0.2.19`), freezing that upgrade.
- **This is the only option `granit-parser`'s maturity currently rules out** —
  every other technical objection raised against it in this evaluation
  (error-rendering, untrusted-source echo, budget false-rejections,
  version pinning) turned out to be solvable, not fundamental. See §6.

### (b) Not warranted — no further action

- **Pros:** zero churn. `serde_norway`'s unsafe surface is libyaml, a
  ~20-year field-hardened C library; the parse surface here is a single
  ≤8 KiB in-memory string extracted from a local file, not
  network-reachable untrusted input.
- **Cons:** ignores 19 months of `serde_norway` upstream silence (default
  branch's last commit 2024-12-21; `pushed_at` 2025-08-04, so "dormant,"
  not "abandoned") and #293's own finding. Leaves the incidental
  alias-bomb resistance of `RawFrontmatter` undocumented and untested
  (§5) — a future field-type change could silently remove it.

### (c) Monitor and revisit at a measurable gate — **recommended**

- **Pros:** defers the swap until `granit-parser`'s single biggest
  objection (bus-factor) is actually resolved or proven false by a
  RUSTSEC advisory against the status quo; costs nothing now; the
  one-call-site surface means the swap stays cheap whenever taken.
- **Cons:** requires an explicit review date and a gate specific enough
  not to false-pass (§7) — a generic "wait and see" is not itself
  actionable and was rejected in favor of the criteria in §7.

**Decision: (c).** What the decision reduces to, once every removable
objection to (a) is stripped away (§6): a 12-week-old, bus-factor-1
parser (`granit-parser`) versus a 19-month-dormant but ~20-year
field-hardened one (libyaml, via `serde_norway`), for a single call site
parsing one ≤8 KiB string from a local file. The risk being traded away
(libyaml's unsafe surface) is real but field-hardened and narrow in this
usage; the risk being taken on (a fork with 126 of 127 post-fork commits
from one human, per §4) is not yet retired. Maintainer concentration in
`granit-parser` is the only remaining substantive objection to (a), and
it is not one that can be engineered around — it can only be waited out
or falsified.

## 3. Evidence Ledger

All figures measured directly against vendored/built crate sources, not
taken from documentation. Two rounds of independent critic re-measurement
confirmed every figure below except where marked *(illustrative)*.

### 3.1 Unsafe-code delta: **−181 `unsafe fn`**, not near-zero

- Removed: `unsafe-libyaml-norway` 0.2.15 (228 `unsafe fn`) + `serde_norway`
  0.9.42's own (8 `unsafe fn`) ≈ 236.
- Added: `serde-saphyr`/`granit-parser` are genuinely
  `#![forbid(unsafe_code)]` (0 added), but the transitive set pulls in
  `encoding_rs` 0.8.35 (40 `unsafe fn`, 137,786 LOC) and `arraydeque` 0.5.1
  (15 `unsafe fn`) — 55 added.
- Net: 236 − 55 = **−181**, not −236 and not to zero. Net crate count
  change is −2/+8 = **+6**.
- `encoding_rs` arrives via `encoding_rs_io`, which is **non-optional**
  inside `serde-saphyr`'s `deserialize` feature even though it is used
  only by the `from_reader` path (`de/buffered_input.rs`,
  `de/safe_resolver.rs`) — a path `extract_skill_metadata` never calls.
  This is the single largest unsafe item the swap would *add*, for code
  this project does not use. Worth an upstream feature-gating request if
  the swap is ever taken (§8).
- With `default-features = false, features = ["deserialize"]` (this
  project's actual build shape), `base64`, `zmij`, and `nohash-hasher` are
  never compiled at all, so `serde-saphyr`'s declared `base64 <=0.22.1`
  ceiling **does not bind**. The one ceiling that does bind is
  `num-traits <=0.2.19` — this workspace sits exactly at it today.
  `smallvec <1.16.0` is close but not binding (workspace is at 1.15.2).

### 3.2 Alias-bomb benchmark — bounded at every configuration, not an unbounded blowup

| Configuration | Latency | Scaling |
|---|---|---|
| `serde_norway` (status quo) | ~20-28 us | flat |
| `serde-saphyr`, default `Budget` | ~47-49 ms | flat across 286-576 B input *(bounded worst case: the fixed cost of eagerly expanding to the default `max_nodes` 250,000 before bailing — not an unbounded blowup, though ~2000x `serde_norway` on a synchronous request-handling path)* |
| `serde-saphyr`, redesigned `Budget` (§6) | ~1.5-1.6 ms | flat across 286-576 B, ~50x `serde_norway` |

`Budget` is the **sole** control here — `Options::alias_limits` left at
its default is *not* an effective bound (`budget = None` measures
120-167 ms, worse than the default-`Budget` case). Any future
simplification must not drop `Budget` on the assumption `AliasLimits`
still covers it.

### 3.3 `#[serde(flatten)]` short-circuit asymmetry (corrected)

`serde_norway`'s alias-bomb resistance today is **incidental to
`RawFrontmatter`'s field shape**, not a designed property: its lazy
deserializer short-circuits on a `sequence`-into-`String` type mismatch
before expanding. Measured effect of changing that shape:

- Adding a plain `Vec<String>` field: **no effect** — 15-25 us, identical
  to today. (An earlier pass of this evaluation claimed this would break
  the short-circuit; that claim was measured and found wrong.)
- Adding a `#[serde(flatten)]`-style buffering field is what actually
  breaks it (note: only when the alias bomb uses undeclared/unknown YAML
  keys — `flatten` buffers these, not declared fields): `serde_norway`
  then degrades to **4.2 / 5.1 / 6.0 / 7.0 ms** at alias-nesting levels
  8/10/12/14 — **scaling with input**, unlike its current flat ~20-28 us.
- `serde-saphyr` under the redesigned `Budget` (§6) stays **flat at
  1.78 ms** on the identical flattened shape.

This is a genuine asymmetry worth stating plainly: `serde_norway`'s
current protection is *shape-and-input-dependent* (it holds only for
today's field types and degrades if they change), while `serde-saphyr`'s
budget is *shape-independent* by construction. This does not change the
§2 recommendation, but it is an argument for (a) that this evaluation
would otherwise have failed to make, and it is the reason §5's regression
test is the recommended near-term deliverable — to catch exactly this
kind of silent regression.

### 3.4 Behavioral differences (3 of 31 differential cases)

1. **Block/folded scalar trailing newline.** `description: |` yields
   `"...lines."` under `serde_norway` (strips trailing newline) vs
   `"...lines.\n"` under `serde-saphyr` (YAML-1.2-correct "clip"
   chomping; `|-` explicit-strip is unaffected either way). This would
   change the observable `SkillMetadata.description` value flowing into
   `SKILL.md`. The only exact-equality description assertions in the
   workspace (`crates/mcp-server/src/service.rs:4335,4364,4399`) all use
   plain scalars — **no existing test would catch this regression** if
   the swap were made without an explicit decision (normalize via
   `trim_end`, or accept and document). Left open for a human ruling (§8).
2. **Merge-key (`<<`) expansion.** `serde_norway` does not expand merge
   keys (an error, not silently-wrong data). `serde-saphyr`'s default
   `MergeKeyPolicy::Merge` expands them, flipping an error case into a
   success. Resolved (if the swap is taken) via
   `MergeKeyPolicy::AsOrdinary`, which treats `<<` as an ordinary unknown
   key and reproduces `serde_norway`'s exact observed result — verified
   as exact parity, and confirmed *live* under the redesigned `Budget`
   (anchors are permitted there, so this policy actually runs).
3. **Error location drift.** On some malformed inputs (e.g. tab-indent),
   reported line/column differs between parsers; the existing
   `test_extract_skill_metadata_invalid_yaml` assertion on `"line 2"`
   would need rebasing if the swap were made.

### 3.5 Error-rendering security finding

`serde-saphyr`'s default `Display` renders a 4-6 line
`annotate-snippets`-style block that **echoes the frontmatter source
line verbatim**, with the location repeated 2-3 times. Since
`SkillMetadataError::InvalidYaml` is client/LLM-facing,
[[../constitution#V. Security]]'s prompt-injection-defense principle
applies: untrusted source text must not reach LLM-facing error text
unfiltered. Concretely demonstrated: a frontmatter block containing
`description: SUPER_SECRET_TOKEN_abc123` reproduces that literal string
into the error the MCP client/LLM receives, under default `Display`.

Resolution (if the swap is taken): `Error::without_snippet().render()`
produces a single-line message with the location stated once and no
source echo (e.g. `unexpected event: expected string scalar at line 1,
column 7`) — confirmed across duplicate-key, tab-indent, and
wrong-type cases. No `Options` field suppresses the snippet; suppression
lives on the error value, not the deserializer configuration.

This also invalidates today's `describe_yaml_error` approach of
correcting a rendered string in place (`replacen("line L column C", …)`):
`serde-saphyr`'s snippet-free format uses a comma
(`at line 1, column 7`) where `serde_norway` has none (`line 1 column
7`), so the existing substring-replace would silently no-op even with
the snippet removed. The correct fix is to stop string-replacing
altogether and build the message directly from the structured
`Error::location()` value already retrieved today — simpler than the
current code, not more complex.

**Known cleanup item, not a blocker:** the duplicate-key render still
carries the attacker-controlled key name plus an internal-API hint
(`"set DuplicateKeyPolicy in Options if acceptable"`). The no-source-echo
bar is met regardless, but if the regression-test deliverable (§5) is
later implemented, it should assert on key names too, and the config
hint should be treated as noise to strip from an MCP-client-facing error.

### 3.6 Dependency-ceiling and reverse-dependency findings

- `num-traits <=0.2.19`: workspace sits **exactly** at this ceiling today
  — a real, binding freeze if the swap is taken.
- `smallvec <1.16.0`: workspace is at 1.15.2, one minor release below —
  close but not currently binding.
- `base64 <=0.22.1`: does **not** bind under this project's actual build
  (`default-features = false, features = ["deserialize"]` never compiles
  `base64`) — an earlier pass of this evaluation overstated this as a
  binding constraint.
- `granit-parser` reverse dependencies: exactly 4 (`serde-saphyr`,
  `serde_yaml_bw`, `ryl`, `cwl-lsp`). Of these, `serde_yaml_bw` appears to
  share the same author as `granit-parser`; `ryl` (owenlamont) and
  `cwl-lsp` (JensKrumsieck) are genuinely independent — **2** independent
  consumers, thin but non-zero.

Figures marked with byte counts are approximate/illustrative unless
stated with an exact reproducible input: an earlier pass cited "1000
unknown keys (7,913 B)," which does not reproduce with the obvious
`kN: vN` encoding (that construction is 10,803 B, over the 8 KiB cap).
The reproducible, re-verified figure is **500 unknown keys / 5,303 B**,
which covers the same counterexample and produced 0 false rejections
under the redesigned `Budget` (§6).

## 4. Maturity Signals and the Bus-Factor Finding

| | `serde-saphyr` | `granit-parser` | `saphyr` (upstream, not actually used) | `serde_norway` |
|---|---|---|---|---|
| latest | 1.0.0-rc.1 (2026-07-18) | 1.0.0-rc.1 | 0.0.11 | 0.9.42 (2024-12-21) |
| repo created | 2025-09-27 | 2026-04-30 | 2024-04-02 | — |
| stars | 207 | 11 | 328 | 55 |
| downloads (recent) | — | 1.09 M (essentially all via serde-saphyr) | — | 8.24 M total / 1.74 M recent |

`granit-parser` bus-factor is **proven, not estimated**: the GitHub
profile for `bourumir-wyngs` (the fork's primary committer) states "My
real name is Audrius Meškauskas" — confirming that
`audrius.meskauskas@ethz.ch` (a second-looking commit identity, including
some commits with no linked GitHub login) and `bourumir-wyngs` /
`bourumir.wyngs@gmail.com` are **the same human**. Under identity
resolution, post-fork `granit-parser` is **126 of 127 commits by one
person** (the sole exception: 1 commit by Owen Lamont).

`serde_norway` is not risk-free either: its default branch's last commit
is 2024-12-21 (19 months dormant at evaluation time, though `pushed_at`
2025-08-04 means it is not literally untouched) — the same kind of
single-maintainer risk that led #293's parser choice in the first place.
Staying on (b) indefinitely is not a zero-risk baseline; it is why (c)
is a monitored decision rather than a permanent one.

## 5. Recommended Near-Term Deliverable (in scope for this issue)

Land a **regression test** that pins `RawFrontmatter`'s current
lazy-deserializer short-circuit behavior against alias-bomb-shaped input
— asserting that parsing completes in bounded time/does not expand a
crafted alias bomb — so that a future change to `RawFrontmatter`'s field
types (specifically, any `#[serde(flatten)]`-style buffering field; see
§3.3) cannot silently reopen the amplification path `serde_norway`
currently resists only incidentally.

This is deliberately **not** the `Budget` redesign or the swap itself:
`serde_norway` has no equivalent budget knob, there is no present DoS to
fix (the bomb is already rejected in ~20-28 us today), and the tightened
`Budget` (§3.2, §6) only becomes relevant if/when the swap in §2(a) is
actually taken. The regression test is the one piece of this evaluation
that stands on its own regardless of what happens to §7's gate.

## 6. What Changed Between the First and Final Pass

Two rounds of adversarial review corrected the following claims that an
earlier pass of this evaluation got wrong; recorded here so the
corrected reasoning, not just the conclusion, survives:

- The `=1.0.0-rc.1` pin requirement was **false** — a caret requirement
  resolves and auto-upgrades to GA — removing the original "wait for GA
  timing" argument for (c) entirely. The gate in §7 is reframed
  accordingly: the actual reason to wait is `granit-parser`'s maturity,
  not `serde-saphyr`'s version status.
- The `base64 <=0.22.1` ceiling was overstated as binding; it is not,
  under this project's actual feature set (§3.6).
- The error-rendering and untrusted-source-echo concerns were
  initially raised as unresolved blockers; both have a concrete
  resolution (`Error::without_snippet()` + structured `location()`) and
  are downgraded to bounded implementation work, not blockers (§3.5).
- The first-pass `Budget` design produced false rejections on legitimate
  input (≥500 unknown keys, any anchor without an alias, flow depth ≥8).
  The redesigned rule — size every limit so that non-amplifying input
  which already cleared the 8 KiB byte cap cannot reach it — produces
  **0 false rejections across every case tested**, so the budget fires
  only on genuine alias amplification.
- `MergeKeyPolicy::Error` (first-pass fix for finding 2 in §3.4) was
  **dead configuration** under the first-pass budget, since
  `max_anchors = 0` would have fired first. The redesigned budget permits
  anchors (`max_anchors = 8192`, since anchors alone cannot amplify
  without aliases) so `MergeKeyPolicy::AsOrdinary` is live.

None of these corrections change the §2 decision. They make (a) more
technically defensible than the first pass concluded — every objection
except `granit-parser`'s bus factor turned out to be solvable.

## 7. Measurable Gate for Revisiting (a)

Revisit **only when all three hold**, or immediately on the trigger
below:

1. `granit-parser` reaches **>=1.0.0 GA** (non-rc), **and**
2. **>=2 identity-resolved contributors with >=10 commits each** in a
   rolling 6-month window, **and**
3. **>=5 reverse dependencies**, excluding crates by the same
   identity-resolved maintainer.

**Immediate trigger, ahead of the gate:** any RUSTSEC advisory against
`unsafe-libyaml-norway` or `serde_norway`.

### Criterion 2 requires explicit identity resolution — this is not optional

Run naively — grouping commits by `.author.login` or by raw commit
email — criterion 2 **false-passes today**: `bourumir.wyngs@gmail.com`
(85 commits) and `audrius.meskauskas@ethz.ch` (41 commits) both clear the
`>=10` bar within the current 6-month window, and grouping by
`.author.login` fails to catch this because the ETH-email commits carry
no linked login. A naive read of this gate would report "second
sustained committer achieved" on a repository that is, per §4, 126 of 127
commits from one human.

The gate **must** resolve identity before counting: cross-reference each
commit author's email/display name against known aliases (starting with
the committer's own GitHub profile bio, which is what surfaced this
exact case) rather than trusting `.author.login` or raw email uniqueness
alone. Applying that resolution to the current data collapses the
naive count of 2 down to **1**, which correctly does not clear
criterion 2. Whoever re-runs this gate at the review date must repeat
the identity-resolution step, not just the commit count.

**Review date: 2026-10-27.**

## 8. Out-of-Scope Follow-Ups (not resolved by this evaluation)

The following are noted here but deliberately not resolved in this
decision record; each should become its own issue if pursued, per this
project's convention of not silently folding unrelated findings into an
issue's stated scope:

1. An upstream feature-gating request to `serde-saphyr`: `encoding_rs_io`
   is non-optional inside the `deserialize` feature despite being used
   only by the (here, unused) `from_reader` path. Only relevant if/when
   §7's gate is met and (a) is actually taken.
2. Whether the no-verbatim-untrusted-source-echo requirement (§3.5)
   should be generalized in [[../constitution#V. Security]] as a rule
   for *any* future parser error text reaching an LLM, rather than
   documented as specific to this evaluation. Both review passes agreed
   this generalization is warranted; it was not applied here since no
   parser swap is being made yet. (Implemented as a constitution principle,
   issue #351.)
3. `granit-parser`'s inherited exposure (if any) to upstream `saphyr`
   issue #109 (a billion-laughs report open since 2026-03-28, fix PR
   #120 unmerged upstream) was not verified. `serde-saphyr`'s `Budget`
   masks this at the serde layer regardless, so it does not affect this
   project even if inherited; noted for completeness only.
4. The duplicate-key error path's leftover key-name/config-hint artifact
   (§3.5) — a cleanup item for whenever the regression test in §5 is
   implemented, not before.

## 9. Open Questions Requiring a Human Ruling

These do not block accepting (c); they matter only if/when (a) is
eventually taken:

1. Block/folded scalar trailing-newline behavior (§3.4, item 1):
   normalize via `trim_end`, or accept the YAML-1.2-correct value and
   document the change?
2. Is freezing `num-traits` at `<=0.2.19` (§3.6) an acceptable tradeoff
   at the time the swap is made?

## See Also

- [[../constitution#V. Security]] — YAML parse-time bound and
  prompt-injection-defense principles this evaluation applies
- [[../skill/spec#7. `extract_skill_metadata` — Frontmatter Parsing]] —
  the current `serde_norway`-based implementation this ADR evaluates
  replacing
