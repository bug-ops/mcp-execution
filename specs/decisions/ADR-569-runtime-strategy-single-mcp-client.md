---
aliases:
  - ADR-569
  - runtime strategy single MCP client
tags:
  - sdd
  - decision
  - runtime
  - architecture
created: 2026-11-24
status: accepted
decided: 2026-11-24
decided-option: B
review-date: 2027-02-24
related:
  - "[[../constitution]]"
  - "[[../cli/spec]]"
  - "[[../codegen/spec]]"
  - "[[../introspector/spec]]"
  - "[[ADR-369-rmcp-stateless-lifecycle-adoption]]"
  - "[[ADR-529-rmcp-3-5-latest-promotion-revisit]]"
---

# ADR-569: Who Is the MCP Client at Script Run Time, the Generated Bridge or the Rust Binary

> [!important]
> **Status: accepted (2026-11-24), option B.** The maintainer decided in-session
> during the #569 work: the Rust binary is the single MCP client at script run
> time. The decision is recorded in section 5a. Sections 1 to 5 are the
> evaluation as written before the decision and are kept unchanged.
>
> The latency requirement of section 6 was accepted on a proxy: option B's per-call
> figure cannot be measured before B exists, so the cold `introspect` time stands
> in for it at acceptance, and the real per-call figures are measured during
> implementation (section 5a, "Measurements owed").

## 1. Context

Issue #569 asks which process calls MCP tools while a model-written script runs.
Today two clients exist.

| | Rust client | Generated bridge |
|---|---|---|
| Where | `Introspector` in `crates/mcp-introspector/src/lib.rs:419` (`discover_server`), `:1009` (`connect_and_list_tools`) | `_runtime/mcp-bridge.ts`, rendered from `crates/mcp-codegen/templates/progressive/runtime-bridge.ts.hbs` (1,240 lines) |
| Purpose | Lists tools only; no non-test Rust code calls a tool (the single `call_tool` match in `crates/` is a comment, `crates/mcp-server/src/service.rs:1200`) | Calls tools; the only call path in the project (`runtime-bridge.ts.hbs:1085`) |
| Transports | stdio, HTTP, SSE through `rmcp` | stdio only; a validated http or sse entry is rejected afterwards (`:644-649`) |
| Protocol | negotiated by `rmcp` | handshake pinned to `2024-11-05` (`:966`) |
| Validation | `validate_server_config` | a line-for-line mirror with constants rendered from Rust (`:634-665`; [[../codegen/spec]] section 8) |
| Configuration | resolved by the CLI | reads `~/.claude/mcp.json` itself at call time (`:680-681`) and spawns the server in the process that runs the script (`:906`) |
| Bounds | response line cap, SSE event cap, connect and discover timeouts ([[../introspector/spec]] sections 4-5) | none on output; no call record; no write policy |

The script runtime ([[../cli/spec]]) adds a runner, a call journal,
a dry run with a write policy and a Rust call path. Each can be built in the
bridge or around one Rust client; choosing late means building twice. The
two clients also have to be kept in agreement by hand: the bridge mirrors the
Rust validation line for line ([[../constitution]] section V). This record does
not reopen the `rmcp` lifecycle decisions of
[[ADR-369-rmcp-stateless-lifecycle-adoption]] and
[[ADR-529-rmcp-3-5-latest-promotion-revisit]].

## 2. Options

### A. Keep growing the generated bridge

The TypeScript bridge stays the only client used by scripts. Add HTTP and SSE,
a current protocol handshake, output and time budgets, the journal and the
policy to the template.

- Everything runs inside the script's own process, so a script can read or
  bypass any of it.
- Keeps generated servers self-contained: no binary needed at run time.
- Every capability is implemented a second time, in TypeScript, next to the
  Rust one, with constants mirrored from Rust.

### B. The Rust binary is the single client

Staged: first a one-shot `call` command through the introspector's connection
path (#574), then a per-script helper process holding pooled upstream
connections over stdio, HTTP and SSE through `rmcp`, owning policy and journal
(#575). Generated TypeScript shrinks to typed wrappers that call the helper; the
bridge's connection code, mirrored validation and protocol pin are removed.
Daemon and gateway tools are later steps and are out of scope.

- One connection path, one validation, one protocol negotiation.
- Tool traffic leaves the script process, which makes enforcement and, later,
  reduced-privilege execution meaningful.
- Scripts and generated wrappers depend on the binary being reachable at run
  time.

### Variant: adopt a TypeScript MCP library inside the bridge

Keep option A's shape but replace the hand-written handshake and transports with
an existing TypeScript MCP client library.

- Removes the protocol pin and adds transport coverage without writing them.
- Adds a runtime dependency to generated output, which today imports only Node
  built-ins, and keeps two clients and two validations.
- Whether this is a variant of A or a separate option is left to the
  maintainer; adopting a new runtime dependency is ask-first.

A hybrid (bridge for stdio, helper for HTTP and SSE) is not recommended by
anyone but should be evaluated if proposed: it fails the single-policy-point
criterion by construction.

## 3. Evaluation Criteria

| Criterion | What is assessed | Evidence type |
|-----------|------------------|---------------|
| Credential isolation | Which process holds secrets from `mcp.json` relative to model-written code; only meaningful once scripts cannot read `mcp.json` themselves, which needs reduced-privilege execution (#576) | citation |
| Policy enforcement point | Inside or outside the script process, and how bypassable | citation |
| Protocol and transport coverage | stdio, HTTP, SSE; negotiated versus pinned protocol | citation |
| Maintenance | Lines and duplicated validation to keep in step | citation, line counts |
| Runtime dependency on the binary | How a script finds it, behaviour when absent, what `setup` must verify | analysis |
| Latency | Cold start of one call; cost of the second and tenth call in one script | measurement |
| Portability | Hosts and operating systems; Windows support of the bridge is unverified | analysis |
| Sandboxing prospects | Whether tool traffic can leave the script process | analysis |
| Migration cost | Regeneration of existing servers, breaking changes | analysis |

Every figure must be reproducible from a stated command or fixture; claims about
other tools are labelled as secondary.

## 4. What Each Option Implies for the Gated Components

Gating was defined in the runtime block, since folded into [[../cli/spec]]; this decision satisfies the gate. Not gated and unaffected
under any option: quiet single-call output (#570), the script runner (#571) and
the pre-run typecheck (#577).

| Component | Option A | Option B |
|-----------|----------|----------|
| Call journal (#572) | Written by the bridge inside the script process; can be altered by the script | Written by the helper outside the script process |
| Dry run and write policy (#573) | Enforced in TypeScript inside the script process; bypassable by the script | Enforced in the helper; still bypassable by a script that talks to servers itself until #576 |
| One-shot call (#574) | A separate Rust path beside the bridge: a second way to call tools, which must be justified against the single-path rule | The first stage of the single client |
| Per-script helper and bridge retirement (#575) | Not built; the bridge is extended instead | Built; removes the bridge's connection code, mirrored validation and protocol pin |
| Reduced-privilege execution (#576) | Little to narrow, since tool traffic stays in the script process | Possible, with the helper as the only permitted channel |

If the decision is later revisited, the journal, policy and one-shot call are
the pieces whose owner changes.

## 5. Recommendation of the Adopted Direction

> [!tip] Recommendation, not a decision
> The adopted direction recommends option B: the Rust binary as the single MCP
> client, staged as a one-shot call and then a per-script helper. The stated
> reasons are one connection path, protocol and transport coverage through the
> client the project already trusts, a policy and journal that live outside the
> script process, and removal of the mirrored validation. The recommendation is
> an input to this evaluation and must survive it; option A and the library
> variant are to be assessed with equal rigor.

## 5a. Decision

- **Adopted: option B.** One Rust MCP client; the runner process hosts the
  per-script helper; generated TypeScript becomes a protocol client of it.
  Staging, carriers and the protocol are in [[../cli/spec]] sections 16 to 18 and [[../core/spec]] section 6.
- **Not adopted:** option A, the TypeScript-library variant and the hybrid.
- **Latency evidence accepted at decision time:** the cold `introspect` time of
  the in-repo fixture, as a proxy for the helper's cold connect (the helper
  uses the same connection code).
- **Measurements (recorded at SDD-1 from the #574 handoff):** release builds,
  macOS, Node v26.13.1, fixture `echo`, 10 sequential runs each, milliseconds.
  Every run is a separate process, so there is no warm state. Script:
  `bench.py` in the developer's scratchpad (not tracked); commands as in the
  table.

  | Measurement | first | 2nd | 10th | median |
  |---|---|---|---|---|
  | A: `node echo.ts '{}'` (generated wrapper, existing TypeScript bridge) | 206 | 198 | 136 | 160 |
  | B: `mcp-execution-cli call --from-config fixture -- echo '{}'` | 9 | 8 | 8 | 8 |
  | B: `mcp-execution-cli introspect --from-config fixture` (cold) | 9 | 10 | 8 | 8 |
  | reference: `node --version` | 96 | | | |

  Reading: A includes about 96 ms of bare node start-up, so the bridge's own
  cost is roughly 40 to 110 ms per process. B's per-call figure equals its cold
  introspect figure, which supports the cold-introspect proxy accepted above.
  The figures compare one process per call; the helper, which pays the connect
  once per run, is measured by the wave-3 harness (K calls in one script
  versus K `call` processes).
- **Review trigger:** the review date in the frontmatter, or a measured per-call
  regression of option B against the pre-change bridge figure.
- **Breaking changes implied** (changelog form, pre-1.0, no shim): generated
  bridge loses its connection code, mirrored validation and the `2024-11-05`
  pin; `BridgeContext` fields change; regeneration of every existing server is
  required; `FIXED_FILE_COUNT` stays 7.

## 6. Required Before Acceptance

> [!note] Status of this checklist
> Acceptance was given by the maintainer; the items below are not all
> satisfied. The latency item is satisfied by the proxy of section 5a and
> completed by the owed measurements. The criteria assessment (section 3) is
> carried by [[../cli/spec]] and the architect's evaluation. The item on
> referencing the blocked specs is done in the runtime plan and tasks.


- [ ] A latency measurement on an in-repo mock server: per-call and per-script
      overhead for each option, with the method stated so it can be rerun (or an
      explicit statement of why option A's figure cannot be obtained).
- [ ] The list of breaking changes the chosen option implies (generated output
      layout, bridge removal or shrinkage, regeneration), in the form the
      changelog needs; pre-1.0, no compatibility shim.
- [ ] An assessment of every criterion in section 3 for each option, each backed
      by a `file:line` citation or a reproducible measurement, with line numbers
      re-checked at the time of acceptance.
- [ ] The maintainer's decision, its status, and a review trigger (an
      observable condition such as a new protocol revision or a measured
      regression).
- [ ] After acceptance: the blocked specs and issues (#572, #573, #574, #575,
      #576) reference this record.

## 7. Open Questions

- [NEEDS CLARIFICATION: Is the TypeScript-library variant part of option A or a separate option?]
- [NEEDS CLARIFICATION: Under option B, how does a script find the binary (PATH, a path recorded at generation, a variable set by the runner), and what must `setup` verify?]
- [NEEDS CLARIFICATION: Which hosts and platforms count for portability in the first wave? The adopted direction keeps Claude Code first.]
- [NEEDS CLARIFICATION: Is a latency threshold needed for "acceptable", or is the measurement informative only?]
- [NEEDS CLARIFICATION: How does the stateless protocol revision lower the value of connection pooling for HTTP servers, and is pooling for stdio servers assessed separately?]

## 8. See Also

- [[../constitution]] — mirrored validation, resource bounds
- [[../cli/spec]] — the components this record gated, now implemented
- [[../codegen/spec]] — generated bridge responsibilities
- [[../introspector/spec]] — the authoritative connection path
- [[ADR-369-rmcp-stateless-lifecycle-adoption]], [[ADR-529-rmcp-3-5-latest-promotion-revisit]] — protocol lifecycle decisions this record must not contradict
