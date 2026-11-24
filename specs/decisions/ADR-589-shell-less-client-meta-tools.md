---
aliases:
  - ADR-589
  - shell-less client meta-tools
tags:
  - sdd
  - decision
  - server
  - security
created: 2026-11-24
status: accepted
decided: 2026-11-24
decided-by: user
review-date: 2027-02-24
related:
  - "[[../constitution]]"
  - "[[../server/spec]]"
  - "[[../cli/spec]]"
  - "[[ADR-569-runtime-strategy-single-mcp-client]]"
---

# ADR-589: Search and Execute Tools for MCP Clients Without a Shell

> [!important]
> Decision-only record for #589 (part of #559).
>
> **Status: accepted (2026-11-24), option A, by the user.** Neither a search nor an execute tool is added to
> `mcp-execution-server` now. #589 closes by this record, with no code.

## 1. Context

`mcp-execution-server` exposes generation tools only: `introspect_server`, `save_categorized_tools`,
`list_generated_servers`, `generate_skill`, `save_skill`. A shell-less MCP client cannot run `run`. Gateways for such
clients converge on two meta-tools: search tools, then execute a script whose only capability is calling them.
This pass delivers the Rust call path (`Session`), the helper and `--restrict`, which the issue lists as prerequisites.

Facts that bear on the decision (from [[../cli/spec]]):

- `--restrict` is Unix only; on Windows it is refused. It narrows filesystem reads and child processes; network is
  narrowed only on Node 25+.
- Dry run and the journal are diagnostics, not boundaries.
- The caller of a server tool is remote from the user's shell, so a script written by a remote model runs with the
  server's privileges.

## 2. Options

| Option | Exposes | Security position |
|--------|---------|-------------------|
| A. Neither tool now; record the prerequisites and revisit | Nothing new | No new attack surface |
| B. Search only (read-only, bounded signatures from generated catalogs) | One read-only tool | Low; depends on the catalog search decision (ADR-591) |
| C. Search and execute, executing under `--restrict` and the dry-run policy | Two tools | High: remote model code on the user's machine; restriction is not a sandbox, absent on Windows, network not narrowed below Node 25 |

## 3. Recommendation

Option A. The security bar the issue itself sets (higher than the local runner, since the caller is remote) is not met
by the isolation this pass delivers: no network narrowing on current Node lines and no Windows restriction. Reconsider with a named trigger: a restriction mechanism that covers
network and Windows, and a maintainer-approved threat model for a remote caller. Option B can follow ADR-591 without
this one.

## 4. Consequences

- Choosing A closes #589 with this record as the decision; no `mcp-execution-server` tool is added.
- Choosing C requires its own spec, threat model and security review before any code.

## 5. See Also

- [[../server/spec]] — current tool list
- [[../cli/spec]] — `--restrict` contract and its limits
