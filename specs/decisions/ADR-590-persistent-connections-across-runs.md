---
aliases:
  - ADR-590
  - persistent connections across runs
tags:
  - sdd
  - decision
  - runtime
  - performance
created: 2026-11-24
status: accepted
decided: 2026-11-24
decided-by: user
decided-option: A
review-date: 2027-02-24
related:
  - "[[../constitution]]"
  - "[[../cli/spec]]"
  - "[[ADR-569-runtime-strategy-single-mcp-client]]"
---

# ADR-590: Should Upstream Connections Persist Between Script Runs

> [!important]
> Record for #590 (part of #559).
>
> **Status: accepted (2026-11-24), option A, by the user.** No persistent connections across runs; the helper pool
> within one run stays. Revisit after a measured containerised target exceeds a threshold the maintainer names. #590
> closes by this decision, with no code. The threshold was never named before the numbers were read, and the user
> decided without it.

## 1. Context

Within one script run, connections are pooled by the helper (`MAX_POOLED_SERVERS` 16). Across runs each `run` starts
every server again. For a server launched through a container runtime that costs seconds per run. Comparable tools keep
a session or daemon alive between invocations. The issue says the work is worth doing only if measured start-up cost
justifies a long-lived process.

## 2. Options

| Option | Description | Cost |
|--------|-------------|------|
| A. No persistence; the per-run helper is final | Nothing new | Start-up cost repeats per run |
| B. Opt-in session daemon, started and stopped explicitly | A long-lived process, a socket, an auth model, shutdown rules | Largest security surface: a persistent process holding upstream credentials, reachable by other local processes |
| C. Reuse only within a run chain via an explicit `--session` handle with an idle timeout | A bounded middle form of B | Still a persistent process; smaller lifetime, same class of risk |

## 3. Method (to be run in wave 3, task T304)

- Fixture: `tools_mcp_server` over stdio, cold start versus warm second call, repeated N times; report the median and
  the spread and state the command and machine class.
- Container-launched server: measured only if a local image is already present; otherwise stated as unmeasured and not
  extrapolated.
- Decision threshold stated before the numbers are read: B or C is considered only if the measured per-run start-up of
  a real target exceeds a figure the maintainer names. The figure was not named, so the rule could not be evaluated
  numerically.

## 4. Data (developer-f; primary figures from a release build, re-measured after the performance review)

**Setup.** Apple M3 Pro (arm64), macOS, node v26.13.1; release build of the integrated branch
(`cargo build --release --all-features -p mcp-execution-cli --bins`, workspace release profile: opt-level 3, thin LTO,
codegen-units 1, no extra flags); fixture
`fixture-tools-mcp-server` over stdio with `MCP_FIXTURE_LOG` per run; fresh temporary HOME with `mcp.json` and a package
from `generate --from-config fixture`. Command per run: `mcp-execution-cli run <script> --servers-dir <dir> --timeout
60`, wall time measured from process spawn to exit. N = 15 sequential runs per case after one discarded warm-up run;
K = 10 calls. Scripts: `no_call` prints a line, `one_call` does one `echo`, `k_calls` does 10 sequential `echo` with
`performance.now()` per call. The harness is a throwaway script outside the repository; it takes the server command
from `mcp.json`, so a container entry can be substituted and the run repeated.

| Case | Wall median (ms) | Min | Max | Fixture initializations per run | Calls per run |
|------|------------------|-----|-----|---------------------------------|---------------|
| no_call | 229.2 | 226.1 | 241.3 | 0 | 0 |
| one_call | 248.6 | 238.7 | 258.0 | 1 | 1 |
| k_calls (K=10) | 248.4 | 243.4 | 259.1 | 1 | 10 |

- Per-run server start-up (one_call minus no_call, medians): about 19.4 ms, including server spawn, initialize and the
  helper-side connect.
- Inside the K-call run: first call (spawn, initialize and call) median 4.9 ms (4.8 to 5.2); calls 2 to 10 median
  0.2 ms each (0.1 to 0.3). The first-call time excludes node and helper start-up.
- Each run initializes the server exactly once, however many calls it makes (the pool shares one session); across N
  runs the fixture starts N times (15 initializations in 15 runs).
- About 229 ms of each run is fixed node and runner start-up, independent of build optimisation, which persistence
  would not remove.

Secondary, debug build (first measurement, same harness and N = 15, kept for comparison only): no_call 223.5 ms
(218.3 to 231.0), one_call 258.8 (248.9 to 292.0), k_calls K=10 251.7 (242.6 to 263.8); server start-up about 35.3 ms
per run; first call 5.9 ms, calls 2 to 10 0.3 ms. A debug build overstates the server start-up cost by about 16 ms.

**Container-launched server: not measured.** `docker` is installed but its daemon was not running ("failed to connect
to the docker API"), so the presence of a local image could not be established, and `podman` is absent. Nothing is
extrapolated; this is the missing input for any future revisit.

## 5. Decision

Option A, by the user. Persistence across runs is not added; the helper pool inside one run stays (it already turns K
calls into one initialize). The only measured target, the stdio fixture, costs about 19 ms per run (release build) against about 229 ms
of fixed start-up, so it cannot justify a persistent process that holds upstream credentials. Revisit trigger: a
measured containerised target whose per-run start-up exceeds a threshold the maintainer names; until then B and C stay
unbuilt. The threshold was not named before the numbers were read and the user decided without one.

## 6. See Also

- [[../cli/spec]] — pool, limits and lifecycle
