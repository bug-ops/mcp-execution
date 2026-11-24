---
aliases:
  - ADR-591
  - ranked catalog search
tags:
  - sdd
  - decision
  - skill
  - measurement
created: 2026-11-24
status: accepted
decided: 2026-11-24
decided-by: user
decided-option: A
review-date: 2027-02-24
related:
  - "[[../constitution]]"
  - "[[../skill/spec]]"
  - "[[../cli/spec]]"
---

# ADR-591: Is a Ranked Search Across Generated Catalogs Needed

> [!important]
> Method record for #591 (part of #559).
>
> **Status: accepted (2026-11-24), option A, by the user.** Text search over compact catalogs; no new command now;
> revisit as catalogs grow. #591 closes by this decision, with no code. The decision rule of section 3 had no stated
> share, and the user decided without one (section 5).

## 1. Context

With several generated servers, finding a tool means a text search over each server's files. Published experiments
with code-first tool use report a stale near-duplicate returned first and hundreds of matches for a broad query. The
typed compact catalog (#578) exists after this pass. The issue says a text search over compact catalogs may suffice and
to measure before building.

## 2. Options

| Option | Description |
|--------|-------------|
| A. No new command; the skill teaches a bounded text search over compact catalogs | Zero code |
| B. A ranked search command returning a bounded number of signatures | New command, a ranking function, tests |
| C. B exposed through `mcp-execution-server` | Depends on [[ADR-589-shell-less-client-meta-tools]] |

## 3. Method (data task, wave 1, owner developer-f)

- Corpus: the in-repo fixture catalogs plus at least three generated servers with overlapping vocabulary (including a
  near-duplicate tool pair and a broad keyword shared by many descriptions).
- Queries: a fixed list written before measuring (exact name, synonym, broad keyword, misspelling).
- Arms: text search over compact catalogs versus the candidate ranking; metrics are rank of the intended tool and the
  number of matches returned for the broad query.
- Everything is offline and deterministic; commands are stated so the numbers can be rerun.
- Decision rule stated before the numbers are read: B only if A returns the intended tool outside the top five, or more
  than a bounded number of matches for the broad query, in a stated share of queries. The share and the bound were not
  named before the numbers were read, so the rule could not be evaluated to a verdict.

## 4. Data (developer-d, recorded at SDD-1; wave-1 handoff 2026-11-24T20-28-25)

**Method as run.** Corpus: 48 catalog entries from `generate`: the in-repo fixture (9) plus three servers served by a
throwaway stdio MCP script (tracker 14, wiki 12, chat 13) with a near-duplicate pair (`searchIssues` /
`searchIssuesLegacy`, "Deprecated ... use search_issues"; `createIssue` / `createTicket`), `addComment` and
`listComments` in two servers, and broad words (list, create, search, user). Catalog size 10120 bytes (about 2.5k
estimated tokens): chat 2696, fixture 1899, tracker 2982, wiki 2543. Queries: 17 written to `queries.json` before the
first measurement (13 with an intended tool: 4 exact, 4 synonym, 2 near-duplicate, 3 misspelling; 4 broad, counted
only). Arm A: case-insensitive fixed-string match of the whole query over `catalog.ts` doc and signature lines, ordered
by server name then catalog order. Arm A2: any non-stopword word of the query matches. Arm B (candidate, throwaway):
split camelCase and underscore names; name words equal 3, prefix 2, edit distance 1 for words of 4 or more letters 2,
description word 1; deprecated or legacy entries -2; stopwords; ties by id; at most 5 returned. Deterministic. Rerun:
`sh .local/adr591/gen.sh` (after `mk.py`, paths edited) then `python3 -I .local/adr591/measure.py`; the scripts are
untracked files in the developer's worktree.

**Results over the 13 intended queries.**

| Arm | Top 1 | Top 5 | Found outside top 5 | Not found |
|-----|-------|-------|---------------------|-----------|
| A literal grep | 4 | 4 | 0 | 9 |
| A2 any-word grep | 4 | 9 | 2 | 2 |
| B ranking | 11 | 12 | 0 | 1 (a pure synonym with no shared word) |

Near-duplicate (`search issues`): A literal returns only `searchIssues` (the legacy description reads "search of
issues"); A2 returns 5 entries with the intended one third; B ranks the intended tool first and the legacy tool below
it. Broad queries (matches): list 11, user 4, search 4, create 7; B returns at most 5.

**Decision rule against the numbers** (share and bound undefined; counts given so any reading can be applied):
outside top five, found-but-low only: A 0 of 13, A2 2 of 13 (15%); counting "not found" as outside: A 9 of 13 (69%), A2
4 of 13 (31%). Broad matches over a bound: bound 5, 2 of 4 queries exceed; bound 10, 1 of 4; bound 11 or more, 0.

**What the method cannot measure.** The scale of the published reports (hundreds of matches, real stale duplicates):
this corpus is small enough to read in full, so neither arm has a context-cost problem at this size. What a model types
into grep, and refinement across turns. Real server vocabulary: the corpus and queries are synthetic and written by one
author, and arm B was written with misspellings in mind, so its misspelling and exact-name results are optimistic. The
upkeep cost of B (code, tests, synonym handling; it has no thesaurus). Catalog entries are camelCase while MCP names are
snake_case: a literal search by the real tool name misses (Q02), and the ranking handles it only because it splits
names.

## 5. Decision

Option A, by the user. No ranked-search command is added now. The corpus is below the size where text search costs
context (about 2.5k estimated tokens for 48 entries), and the decision rule had no stated threshold. Revisit when
catalogs grow: a named trigger is a catalog set large enough that reading it in full is no longer cheap, together with
a measured miss rate from real queries. The numbers above are the baseline for that review. Arm B's results are the
evidence that a small token-overlap ranking works on this corpus (12 of 13 in the top five), without a verdict on
building it.

## 6. See Also

- [[../skill/spec]] — catalog and skill text
- [[../cli/spec]] — the commands the skill text teaches
