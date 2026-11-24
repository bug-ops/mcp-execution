---
aliases:
  - ADR-562
  - context cost estimator
tags:
  - sdd
  - decision
  - cli
  - measurement
created: 2026-11-24
status: accepted
decided: 2026-11-24
decided-by: user
review-date: 2027-02-24
related:
  - "[[../constitution]]"
  - "[[../cli/spec]]"
---

# ADR-562: How the Context-Cost Command Estimates Tokens

> [!important]
> Decision record for #562. The `stats` command prints byte counts and token estimates; this record fixes the one
> estimation method.
>
> **Status: accepted (2026-11-24), option A, by the user.** Bytes divided by 4, labelled as an estimate; the gate on
> T103 is satisfied.

## 1. Context

Documentation states 98% (`README.md:10`, `:133`) and about 83% (`CHANGELOG.md`, `[0.2.0]`) savings; neither is
measured, and NFR-PERF-003 says "Not an automated measurement". The command reports bytes and estimated tokens for the
raw manifest, all tool files, one tool file and the compact catalog, plus how many tools declare an output schema.
Missing data prints "not available", never zero; nothing is labelled "savings".

## 2. Options

### A. Bytes divided by 4, labelled as an estimate (recommended)

- Deterministic, offline, no dependency, byte-identical across runs.
- A rough ratio for English and JSON; wrong for other scripts and for any specific tokenizer.
- Bytes are always printed beside the estimate, so a reader can apply their own ratio.

### B. A tokenizer crate

- Closer to one model's count. Adds a dependency (ask-first), is specific to one vendor and version, and changes figures
  when the crate updates, which breaks reproducibility.

### C. Bytes only, no token figure

- Nothing to disagree with. Does not answer the question the documentation asks (context cost).

## 3. Rules Common to All Options

- One method, stated in the help text and in the output header; the label is "estimate".
- No percentages named "savings"; the harness reports a crossover, not a saving.
- A `_meta.json` from before schema v3 exits 2 with no figures.

## 4. Recommendation

Option A. A single-variant estimator type is not introduced; the method is a function with the divisor as a named
constant, and a second method can be added when one exists.

## 5. Consequences

- Rejecting A in favour of B needs a dependency decision and a re-baseline of every published figure.
- Choosing C removes the token column; the harness crossover is then in bytes.

## 6. See Also

- [[../cli/spec]] — `stats` command contract after SDD-4 (as built in section 16)
- [[../core/spec]] — `_meta.json` v3 fields the report reads

## 7. Data (fixture server, 9 tools; recorded at SDD-1 from the wave-1 `stats` implementation)

Estimate is `bytes.div_ceil(4)`: it rounds up, so a non-empty text never reads as 0 tokens ("divided by 4" in
section 2 means this).

| Level | Bytes | Tokens (estimate) |
|-------|-------|-------------------|
| server definitions (recorded, sum of `manifest_size_bytes`) | 1898 | 475 |
| all tool files (`{typescript_name}.ts`, no `index.ts`, no runtime files) | 5250 | 1313 |
| one tool file (mean, rounded down) | 583 | 146 |
| `catalog.ts` | 1899 | 475 |

Output schema declared (Declared or Degraded): 1 of 9. Observation: the catalog is about the size of the raw
definitions and about 36% of all tool files by bytes (1899 of 5250; the developer handoff states about 28%, which does
not match these figures and should be re-checked before it is published).

## 8. Notes

- Help text of `Commands::Stats` (`cli.rs`) does not yet carry the FR-046.16 "proves / does not prove" note that the
  report header has.
- No machine-readable output: the spec marks it ask-first; `--format` is ignored.
