---
aliases:
  - ADR-588
  - multi-host output targets
tags:
  - sdd
  - decision
  - cli
  - skill
created: 2026-11-24
status: accepted
decided: 2026-11-24
decided-by: user
review-date: 2027-02-24
related:
  - "[[../constitution]]"
  - "[[../cli/spec]]"
  - "[[../skill/spec]]"
---

# ADR-588: Support for Agent Hosts Other Than Claude Code

> [!important]
> Decision-only record for #588 (part of #559).
>
> **Status: accepted (2026-11-24), option A, by the user.** Claude Code stays the only target; path overrides remain
> the supported route. #588 closes by this record, with no code.

## 1. Context

Generated servers go to `~/.claude/servers/{id}/`, skills to `~/.claude/skills/{id}/`, configuration is read from
`~/.claude/mcp.json`. `--progressive-output` and `--servers-dir` override paths; there is no notion of a target host.
The generated TypeScript is host-neutral and the skill format is an open specification. The issue itself notes the
current host stays first until the script-first work lands, which is this pass.

## 2. Options

| Option | What changes | Cost |
|--------|--------------|------|
| A. Claude Code stays the only target; path overrides remain the supported route for other hosts | Nothing; docs state the overrides | None; a user of another host keeps hand-set paths and gets no host-format instructions |
| B. A `--target <host>` enum with a per-host table (output paths, config path, instruction format), starting with one named additional host | New type, new flag on `generate` and `skill`, a tested table per host | Each host adds a maintained, externally changing surface that cannot be verified offline |
| C. B for a documented list of hosts, driven by a user-supplied table file | New file format and loader | Largest surface; a second configuration source to validate |

## 3. Recommendation

Option A for this pass, with a review trigger: a named second host whose output paths and instruction format are
documented by that host and which a maintainer asks for. Rationale: the script-first runtime is the first time the
output depends on a host-reachable binary, and the helper-protocol and `run` surfaces should settle before a per-host
matrix is built on them. Rejecting this record changes no code in the PR.

## 4. Consequences

- Choosing A closes #588 with this record as the decision; the host-neutral guidance is already covered by the
  `--servers-dir` and `--progressive-output` documentation.
- Choosing B or C is a new feature needing its own spec before implementation.

## 5. See Also

- [[../cli/spec]] — current path options
- [[../skill/spec]] — skill output location
