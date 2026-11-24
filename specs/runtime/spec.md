---
aliases:
  - script runtime spec
  - Runtime block spec
tags:
  - sdd
  - spec
  - runtime
  - security
created: 2026-11-24
status: planned
related:
  - "[[../constitution]]"
  - "[[../cli/spec]]"
  - "[[../codegen/spec]]"
  - "[[../core/spec]]"
  - "[[../introspector/spec]]"
  - "[[../decisions/ADR-569-runtime-strategy-single-mcp-client]]"
---

# Block: Script Runtime (planned)

> [!abstract]
> This block does not exist in code yet. It specifies the planned runtime
> around model-written TypeScript scripts that call generated servers: a runner
> with bounded output, a call journal, a dry run with a write policy, a Rust
> tool-call path, a per-script helper, reduced-privilege execution and a pre-run
> typecheck. It has no crate of its own; where each piece lands is an open
> decision. Every requirement below is planned, none is
> verified against code, and every statement about today's behaviour cites its
> source. Requirement IDs (FR-036 to FR-042) are defined in the SRS.

> [!warning] Not implemented
> Nothing in this block exists in the code yet. Each section names its tracking
> issue. When a component lands, its content moves into the spec of the crate
> that owns it and the section here is removed in the same pull request.

## 1. Responsibility (planned)

Make a multi-call script a supported way to use a generated server, without
letting the script flood the model's context, hide partial failure, or perform
unreviewed writes:

1. Run a script from a file or standard input, with a time limit and a budget
   on what reaches the model (#571).
2. Record which tool calls a run made, and report them when it fails (#572).
3. Run a script in a mode where writes are reported and not sent (#573).
4. Call one tool from the shell through the same connection code introspection
   uses (#574), and reuse one connection per script (#575).
5. Narrow what a script may do on the user's machine (#576) and check it before
   it runs (#577).

Working names used here (`run`, `call`, "helper", "journal") are placeholders;
names, flags, file formats and algorithms are open decisions unless a section
fixes them.

## 2. What Exists Today

| Fact | Evidence |
|------|----------|
| Tools are called only by the generated TypeScript bridge; no non-test Rust code calls a tool on any client path (the one `call_tool` match in `crates/` is a comment about a macro) | `crates/mcp-server/src/service.rs:1200` |
| The Rust client connects, lists tools page by page under the connect and discover timeouts, then drops the client | `crates/mcp-introspector/src/lib.rs:419` (`discover_server`), `:1009` (`connect_and_list_tools`), `:1077` and `:1141` (the two `serve` call sites) |
| Every generated tool file imports the bridge by a relative path and ends with `runIfMain(import.meta.url, tool)`, which runs the tool when the file is the process entry point (shared `_runtime/cli.ts`, #567/#583) | `crates/mcp-codegen/templates/progressive/tool.ts.hbs`, `cli.ts.hbs` |
| The bridge speaks stdio only: a validated http or sse entry is then rejected as unsupported | `crates/mcp-codegen/templates/progressive/runtime-bridge.ts.hbs:644-649` |
| The bridge handshake is pinned to protocol `2024-11-05` | `runtime-bridge.ts.hbs:966` |
| The bridge reads `~/.claude/mcp.json` itself at call time and spawns the server inside the process that runs the script | `runtime-bridge.ts.hbs:680-681`, `:906` |
| The bridge mirrors the Rust configuration validation, with constants rendered from Rust | `runtime-bridge.ts.hbs:634-665`; [[../codegen/spec]] section 8 |
| An empty successful result resolves to `undefined` (no error; #482); `callMCPTool` throws only `McpToolError` with server text kept in `serverMessage` (#570) | `runtime-bridge.ts.hbs` (`extractResult`, `McpToolError`); [[../codegen/spec#8. Runtime Bridge (`_runtime/mcp-bridge.ts`)]] |
| The bridge has no output bound, no call record and no write policy (it only has the opt-in observed-shape recorder of #587, which records types and never values) | `runtime-bridge.ts.hbs` (`callMCPTool`) |
| The CLI has no subcommand that runs a script or calls a tool | [[../cli/spec]] section 2 |
| Exit codes: success 0, error 1, invalid input 2, server error 3, timeout 4 | `crates/mcp-core/src/cli.rs:139-183`; [[../cli/spec]] section 5 |
| `setup` accepts Node.js 18 and later | `crates/mcp-cli/src/commands/setup.rs:229` |

The consequence is that the project has two MCP clients with different
validation, transports and protocol negotiation, and the second is the one that
runs next to model-written code. The decision record
[[../decisions/ADR-569-runtime-strategy-single-mcp-client]] (#569) evaluates
which of them should be the single client.

## 3. Gating by the Runtime Decision Record

Resolution of which components wait for #569:

| Component | Issue | Waits for #569 | Reason |
|-----------|-------|----------------|--------|
| Quiet single-call output | #570 | No | Changes only the generated tool file's output; implemented, specified in [[../codegen/spec#13.6 Single-call mode (#567, #570, #583)]] |
| Script runner with bounded output | #571 | No | Built on today's generated bridge and does not add a client |
| Call journal and failure report | #572 | Yes | Where it is written, and by which process, differs between the bridge and a Rust process |
| Dry run and write policy | #573 | Yes | Where policy is enforced, and how hard, depends on whether tool traffic leaves the script process |
| One-shot tool call | #574 | Yes | If the generated bridge stays the only client, a Rust call path would be a second way to call tools |
| Per-script helper and bridge retirement | #575 | Yes | Exists only if the Rust binary becomes the single client |
| Reduced-privilege execution | #576 | Yes | Depends on tool traffic leaving the script process |
| Pre-run typecheck | #577 | No | Independent of who calls tools; sequenced after the typed SDK (#556 epic) |

> [!danger] Gate
> Implementation of every component marked "Yes" must not start until a
> maintainer accepts #569. No agent may mark the record accepted.

## 4. Planned: Script Runner With Bounded Output (#571)

### Why

A model that wants to loop, filter or join over tool results has no supported
way to do it: the documented use is one `node <tool>.ts '<json>'` per shell
command, and generated files import the bridge by a relative path
(`tool.ts.hbs:15`). Nothing bounds what a script prints, so a long result is
pasted into the context; the introspector's response cap protects against a
hostile server and is not a context budget ([[../introspector/spec]] sections
4 and 5). Experiments on Node 26 showed that a TypeScript script from standard
input with top-level `await` runs, that an absolute-path import of a `.ts`
module works, and that `module.registerHooks` exists; `setup` still accepts
Node 18 (`setup.rs:229`), so the Node floor is open.

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-036.1 | WHEN the user invokes the run command with a script file or standard input THE SYSTEM SHALL execute the script on the local runtime and return its exit status | must |
| FR-036.2 | WHEN a script imports a generated server through the documented stable specifier THE SYSTEM SHALL resolve it to that server's generated files regardless of the script's own location | must |
| FR-036.3 | THE SYSTEM SHALL count the bytes the script writes to stdout and, WHEN the count exceeds the output budget, show only the head within the budget and write the full output to a spill file | must |
| FR-036.4 | WHEN output is spilled THE SYSTEM SHALL report the head, the spill file path and the total byte count in a notice written by the runner | must |
| FR-036.5 | THE SYSTEM SHALL deliver runner notices in a form that script output cannot forge, so neither script nor server text can pass for a notice that names a path | must |
| FR-036.6 | THE SYSTEM SHALL apply the budget to stderr as well | should |
| FR-036.7 | WHEN the run exceeds the time limit THE SYSTEM SHALL terminate the script and exit with the timeout code | must |
| FR-036.8 | THE SYSTEM SHALL apply a default time limit and a default budget, each overridable per run within a hard maximum | must |
| FR-036.9 | WHEN the runner fails before or around the script THE SYSTEM SHALL exit with the CLI classification and a one-line message that does not echo script text | must |
| FR-036.10 | WHEN the script exits non-zero by itself THE SYSTEM SHALL exit non-zero and SHALL NOT remap the script's codes onto the CLI's classified codes | must |
| FR-036.11 | THE SYSTEM SHALL provide a pagination helper and a bounded-parallelism helper, each reporting when its limit stops it | should |
| FR-036.12 | WHEN a spill file is created THE SYSTEM SHALL create it under a runner-owned location with a runner-generated name, confined by symlink-aware validation and readable only by the user | must |
| FR-036.13 | THE SYSTEM SHALL bound the number and total size of spill files and remove expired ones, confined exactly as creation is | must |
| FR-036.14 | THE SYSTEM SHALL start the script without a shell and SHALL pass script arguments as data | must |
| FR-036.15 | THE SYSTEM SHALL state in help text and generated guidance that scripts run with the user's privileges and without a sandbox | must |

### Constraints

- Constitution [[../constitution]] section V applies: path confinement and
  symlink-aware validation, resource bounds as named constants (a bound above an
  existing one derives from it), redacting `Debug` on any type that carries
  script text, arguments or environment values, and the untrusted-text rule for
  error text and for tool and server names that originate from a server (printed
  results are the open decision in section 11).
- Reuse the CLI's `ExitCode` and classification; reuse existing confinement
  helpers instead of writing new ones.
- Works against servers generated by the current bridge. If generation must
  change (for example to add a fixed helper file), that is a breaking change
  and follows the fixed-file rule in [[../codegen/spec]].
- Scripts are never interpreted, linted or restricted by the runner.

### Open decisions

- [NEEDS CLARIFICATION: command name, flags, and how file versus standard input is selected (working name `run`).]
- [NEEDS CLARIFICATION: the stable import specifier and its mechanism (registered resolution hook, generated alias directory, rewriting); only `module.registerHooks` on Node 26 was verified.]
- [NEEDS CLARIFICATION: minimum Node version, and whether `.ts` execution works at all on Node 18 and 20 (not verified).]
- [NEEDS CLARIFICATION: spill location, retention, count and size caps, permission bits; whether a failed spill write fails the run or continues truncated.]
- [NEEDS CLARIFICATION: how notices stay off the script's channel (stderr, a delimiter with a per-run unpredictable token, a file descriptor).]
- [NEEDS CLARIFICATION: default budget and time limit, and whether the budget is bytes or estimated tokens.]
- [NEEDS CLARIFICATION: whether the bridge's classified failures map onto exit codes 3 and 4.]
- [NEEDS CLARIFICATION: helper API shape and whether the helpers are a fixed generated file or ship with the CLI.]

Ask first: the Node floor and any change to `setup`'s accepted versions; a new
dependency (process groups, temporary files); any generation change that forces
regeneration; default budget and time limit.

### Depends on / blocks

Not gated. Builds on #570 for the failure shape of a single call. Blocks #572,
#573, #577; consumed by the script-first skill (#578) and saved scripts (#579).
Tracked under epic #557.

## 5. Planned: Call Journal and Partial-Failure Report (#572)

### Why

A script that makes several calls and fails midway leaves no record of what
succeeded: the failure is a thrown error from the bridge, and only in debug mode
a log line per call, which prints the call's parameters
(`runtime-bridge.ts.hbs:1090`). A model that sees "script failed" cannot tell
whether three issues were created or none, and a rerun repeats the writes. A
record of calls must not become another place for secrets, so it follows the
provenance discipline of storing digests, not values ([[../core/spec]]).

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-037.1 | WHEN a tool call is made during a run THE SYSTEM SHALL append one journal entry with server id, tool name, argument digest, argument size, status, duration and result size | must |
| FR-037.2 | THE SYSTEM SHALL NOT write any argument value, result value, error text or server-supplied text to the journal | must |
| FR-037.3 | WHEN a call is sent THE SYSTEM SHALL record its start before the response arrives, so a call in flight at failure is present with outcome unknown | must |
| FR-037.4 | WHEN a run ends with a failure, timeout or kill THE SYSTEM SHALL print a bounded summary of completed calls, the failed call and unknown-outcome calls | must |
| FR-037.5 | THE SYSTEM SHALL deliver the summary through the runner's notice channel (FR-036.5) and account for it in the output budget | must |
| FR-037.6 | THE SYSTEM SHALL report every completed call as possibly state-changing, unless the user's policy (FR-038.5) classes it as a read | must |
| FR-037.7 | THE SYSTEM SHALL bound entries per run, bytes per file, retained journals and their age, count dropped entries and mark the journal truncated | must |
| FR-037.8 | THE SYSTEM SHALL create journals at a system-derived location and name, confined by symlink-aware validation and readable only by the user | must |
| FR-037.9 | THE SYSTEM SHALL remove expired journals under the same confinement as creation | must |
| FR-037.10 | THE SYSTEM SHALL flush each entry as it is written, and record concurrent calls without interleaving corruption | must |
| FR-037.11 | WHEN the run succeeds THE SYSTEM SHOULD NOT print the summary by default | should |

No requirement in this section may rely on a server's read-only
hint. Tool and server names in the summary come from an untrusted server and are
sanitised before a model sees them.

### Constraints

- The journal is diagnostic, not an audit log: a script runs with the user's
  privileges and can alter or delete its own journal. Tamper resistance belongs
  to FR-041. No document may call the journal tamper-evident.
- Types: `ServerId`, `ToolName`, a digest newtype, a status enum with an
  explicit unknown variant; hand-written redacting `Debug` on any type holding
  argument bytes before digesting.
- All bounds are named constants; per-entry size is bounded by construction.

### Open decisions

- [NEEDS CLARIFICATION: owner and location of the journal (the bridge inside the script process, or a Rust process outside it); blocked on #569.]
- [NEEDS CLARIFICATION: format, location, naming, retention values and bounds.]
- [NEEDS CLARIFICATION: digest algorithm and keying. An unkeyed hash of a low-entropy argument can be reversed by guessing; a per-run salt prevents that but defeats comparing digests across runs, which is what spotting a repeated write needs.]
- [NEEDS CLARIFICATION: whether the summary shows a digest prefix; whether "timed out" is distinct from "unknown"; whether a successful run's journal is kept.]

Ask first: the digest construction; whether the journal also covers single-call
mode and the one-shot call; starting implementation before #569 is accepted.

### Depends on / blocks

Gated by #569 (section 3). Builds on #571. Shares the notion of write-capable
calls with #573. Annotations from the metadata registry ([[../core/spec]]) are
optional input, never trusted (FR-037.6). Tracked under epic #557.

## 6. Planned: Dry Run and Write Policy (#573)

### Why

When a model calls tools one at a time, the host asks the user to approve each
call; a script collapses many calls into one approved action, so a script that
deletes or overwrites does so without per-call review. Nothing in this project
distinguishes a read from a write: tool annotations are discarded at
introspection (`ToolInfo` has no annotations field,
`crates/mcp-introspector/src/lib.rs:237-246`). The planned metadata change
(#568) records them; they are claims by the server, not facts.

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-038.1 | WHEN a run is started in dry-run mode THE SYSTEM SHALL execute only calls classified as reads and SHALL NOT send calls classified as writes | must |
| FR-038.2 | WHEN a tool has no recorded annotations, or is absent from the recorded metadata, THE SYSTEM SHALL classify it as a write | must |
| FR-038.3 | THE SYSTEM SHALL classify using the annotations recorded in `_meta.json` at generation, not the live server's | must |
| FR-038.4 | A wrong or hostile read-only hint SHALL NOT cause a write to execute during a dry run | must |
| FR-038.5 | THE SYSTEM SHALL apply a user policy classifying tools by server and tool | must |
| FR-038.6 | WHEN regeneration changes the annotations recorded for a tool THE SYSTEM SHALL NOT silently widen what a dry run executes; how such a change is detected is part of the open mechanism for FR-038.4 | must |
| FR-038.7 | WHEN the run ends THE SYSTEM SHALL report each skipped write (server, tool, order) in a bounded report | must |
| FR-038.8 | A skipped write SHALL NOT return a fabricated result; WHEN later steps depend on it THE SYSTEM SHALL say the report is incomplete | must |
| FR-038.9 | WHEN the policy file is missing, unreadable or invalid THE SYSTEM SHALL fail before any call with a classified error that does not echo file contents | must |
| FR-038.10 | THE SYSTEM SHALL state the guarantee level in help and guidance: protection against mistakes in honest scripts and against wrong hints, not against a script written to evade it | must |

Comparison between recorded and live annotations belongs to the
drift check (FR-045, #580), not to this component. The mechanism that satisfies
FR-038.4 is open.

### Constraints

- Fail safe: any classification error, missing metadata or internal failure
  resolves to "write" and is never sent as a read.
- Report text from the server is sanitised, bounded and delimited as untrusted;
  policy and report paths are confined and never derived from script or server
  input; types validate at construction (`ServerId`, `ToolName`, a
  classification enum).
- A mock server that lies about annotations is part of the test fixtures, and
  the hostile-annotation test comes first.
- Dry run does not cover effects outside MCP calls (shell commands, direct file
  writes, direct network access by the script).

### Open decisions

- [NEEDS CLARIFICATION: mechanism for FR-038.4. Candidates, none chosen: hints advisory and only a user policy entry lets a read-classed tool execute; a hint honoured only for a server the user marked trusted; a hint honoured only for tools whose recorded definition the user pinned.]
- [NEEDS CLARIFICATION: where enforcement sits (inside the script process or in a Rust process) and what that does to bypassability; blocked on #569.]
- [NEEDS CLARIFICATION: what a script receives for a skipped write (a thrown marker, `undefined`, a typed skipped marker) and whether the run stops at the first dependent step.]
- [NEEDS CLARIFICATION: whether the report shows argument values (needed to review, may carry secrets and reaches the model) or only digests and sizes.]
- [NEEDS CLARIFICATION: policy file format and location; whether a policy may only tighten or may also relax a classification (a relaxation is shown in the report); whether dry run is the default for a script that contains a write.]

Ask first: the mechanism for FR-038.4; whether dry run is on by default; the
policy format and location; starting before #569 and #568 land.

### Depends on / blocks

Gated by #569. Blocked by #568 (annotations in `_meta.json`) and builds on
#571. Shares write classification with #572. Tracked under epic #557.

## 7. Planned: One-Shot Tool Call Through the Authoritative Client (#574)

### Why

The Rust crates can reach a server over stdio, HTTP and SSE but only to list
tools (`lib.rs:419`, `:1009`). The generated tool file can call a server only
over stdio and cannot call an HTTP or SSE server that the same project can
introspect (`runtime-bridge.ts.hbs:644-649`). A quick Rust caller beside the
introspector would create a second connection path with its own validation and
timeouts, which the project avoids, and which is the reason this component waits
for #569 (section 3).

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-039.1 | WHEN the call command is run THE SYSTEM SHALL resolve the server with the existing server-resolution machinery and the same `ServerConfig` validation as `introspect` | must |
| FR-039.2 | THE SYSTEM SHALL connect through the introspector's connection code for stdio, HTTP and SSE; the connect and validation steps shared by listing and calling SHALL exist once | must |
| FR-039.3 | THE SYSTEM SHALL NOT introduce a second connection path, transport implementation or validation copy | must |
| FR-039.4 | WHEN a call succeeds THE SYSTEM SHALL return the result with the value semantics of FR-030 ([[../codegen/spec]]); the printed form of an empty success is the one decided there | must |
| FR-039.5 | WHEN the server reports a tool error THE SYSTEM SHALL return the one-line structured failure of single-call mode, with server text sanitised, bounded and marked as coming from the server | must |
| FR-039.6 | WHEN the output exceeds the budget THE SYSTEM SHALL apply the runner's budget and spill rules | must |
| FR-039.7 | WHEN the JSON argument is not a single object THE SYSTEM SHALL exit with invalid input and SHALL NOT repeat any part of the argument | must |
| FR-039.8 | THE SYSTEM SHALL validate the tool name as a `ToolName` and the server as a `ServerId` before connecting | must |
| FR-039.9 | WHEN the call exceeds its time limit THE SYSTEM SHALL fail with the timeout code and shut the connection down | must |
| FR-039.10 | WHEN the call ends THE SYSTEM SHALL terminate any server process it started | must |
| FR-039.11 | THE SYSTEM SHALL map failures to the CLI's exit codes; the classification of a tool-reported error is stated, not implied | must |

### Constraints

- The shared connect step lives in the introspector crate; the dependency
  direction forbids the CLI owning it. The introspector's public behaviour and
  tests are unchanged: the step is extracted, not copied.
- Protocol negotiation is the existing connection code's; the bridge's
  `2024-11-05` pin is not copied.
- Result size is bounded by the layer below (stdio line cap, SSE event cap); the
  plain HTTP JSON body is unbounded upstream, so the call's time limit is the
  only bound there, and that is stated, not hidden ([[../introspector/spec]]
  section 5).
- Redacting `Debug` on any type holding configuration, headers or arguments;
  log the `ServerConfig` wrapper, not `Transport`.

### Open decisions

- [NEEDS CLARIFICATION: command name, flags and server selectors (working name `call`).]
- [NEEDS CLARIFICATION: shape of the shared step (new public method on `Introspector`, a new type, or a lower-level function).]
- [NEEDS CLARIFICATION: exit code for a tool-reported error (3 or 1).]
- [NEEDS CLARIFICATION: per-call time limit versus the connect and discover timeouts; new field versus reuse.]
- [NEEDS CLARIFICATION: whether to verify the tool name against generated metadata before connecting; printing rules for non-text content.]
- [NEEDS CLARIFICATION: whether results printed to the caller fall under the constitution's untrusted-text rule or are an accepted exception (open maintainer decision, section 11).]

Ask first: any change to the introspector's public API; a new dependency; the
command name and flags; starting before #569 is accepted.

### Depends on / blocks

Gated by #569. Builds on #570 and #571. Blocks #575. Tracked under epic #557.

## 8. Planned: Per-Script Helper and Retirement of the Generated Bridge (#575)

### Why

Spawning or reconnecting for every call is wasteful for a script that makes ten
calls to one server, and the bridge's own connection code, mirrored validation
and protocol pin are the second client described in section 2. If the Rust
binary becomes the single client, one process per script can hold pooled
connections and own policy and the journal, and the generated TypeScript shrinks
to typed wrappers that call it.

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-040.1 | WHERE #569 accepts the Rust single client THE SYSTEM SHALL provide a per-script helper holding pooled connections, bounded in count and lifetime, with the connect, validation and result semantics of FR-039 | must |
| FR-040.2 | WHEN the run ends, whether it succeeds, fails or times out, THE SYSTEM SHALL close every pooled connection and leave no server process running | must |
| FR-040.3 | THE SYSTEM SHALL route generated wrappers' tool calls through the helper | must |
| FR-040.4 | THE SYSTEM SHALL remove from the generated bridge its own connection code, its mirrored configuration validation and the `2024-11-05` handshake pin (`runtime-bridge.ts.hbs:966`) | must |
| FR-040.5 | THE SYSTEM SHALL list the generated-output changes and the regeneration requirement as breaking changes in the changelog, with no compatibility shim (pre-1.0) | must |
| FR-040.6 | THE SYSTEM SHALL state the behaviour when the binary cannot be found by a script, and `setup` SHALL verify what that requires | must |

This component owns what retires the bridge. No other component
removes bridge code; until #575 lands, the bridge stays and is not extended
beyond what #570 and #571 need.

### Constraints

- Single connection path: the helper uses the shared connect step of FR-039.2.
- Bounds on pool size, lifetime and per-call time are named constants.
- The inter-process protocol, binary discovery and reconnect policy follow from
  the decision and are open here.

### Open decisions

- [NEEDS CLARIFICATION: protocol between script and helper, how a script reaches it, lifetime, reconnect policy, pool size.]
- [NEEDS CLARIFICATION: how a script finds the binary (PATH, a path recorded at generation, a variable set by the runner).]
- [NEEDS CLARIFICATION: handling of cache hints and list-change notifications on a long-lived connection (likely ignored at first).]

Ask first: starting before #569 is accepted; any change to the generated file
layout; adding a dependency.

### Depends on / blocks

Gated by #569; builds on #574 and #571. Blocks #572 and #573 enforcement in a
Rust process if option B is accepted, and #576. Tracked under epic #557.

## 9. Planned: Reduced-Privilege Script Execution (#576)

### Why

A script runs with the user's privileges. Dry run and the journal are
diagnostic and cannot resist a script written to evade them. Narrowing what a
script may do requires that tool traffic leaves the script process, which is
what the helper (#575) provides.

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-041.1 | WHERE the helper carries all tool traffic THE SYSTEM SHALL be able to run a script with its filesystem, process-spawn and network access narrowed to what the user permits | should |
| FR-041.2 | WHEN reduced-privilege execution is not active THE SYSTEM SHALL say so wherever it describes a run, and SHALL NOT describe the runner as sandboxed | must |
| FR-041.3 | THE SYSTEM SHALL state what the narrowing covers and what it does not | must |

Candidate mechanism: the Node.js permission model. Checked locally on Node 26:
file writes and process spawning are denied by default under it.
Not verified: network restriction, behaviour on the Node versions `setup`
accepts, and how the helper's channel is permitted.

### Constraints and open decisions

- Depends on #575 (helper) and therefore on #569.
- [NEEDS CLARIFICATION: mechanism, defaults, how a user widens permissions, effect on dry-run and journal guarantees (FR-038.10, FR-037 constraints).]

Ask first: any default that denies something scripts commonly need.
Tracked under epic #557.

## 10. Planned: Pre-Run Typecheck (#577)

### Why

A script with a type error against the generated SDK fails after it has made
some calls. Checking it first, when a TypeScript compiler is available, turns
that into a failure before any call.

### Requirements

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-042.1 | WHERE a TypeScript compiler is available THE SYSTEM SHOULD typecheck a script against the generated SDK before executing it | should |
| FR-042.2 | WHEN the compiler is absent THE SYSTEM SHALL run the script and say the check was skipped | must |
| FR-042.3 | THE SYSTEM SHALL bound the diagnostics it shows and treat their text as untrusted | must |

Open: how the compiler is found, whether a type error blocks the run or only
warns, and the cost against runner start-up. Optional, after the typed SDK;
not gated. Tracked under epic #557.

## 11. Security Posture

> [!danger] Scripts are not sandboxed
> Until FR-041 lands, a script run by the runner has the user's privileges,
> exactly as a shell command does. It reads the user's files, spawns processes
> and reaches the network, and it can read `~/.claude/mcp.json`
> (`runtime-bridge.ts.hbs:681`). No document, help text or generated guidance
> may describe the runner as sandboxed or safe against hostile scripts. Dry run
> and the journal are diagnostics, not boundaries.

| Concern | Rule |
|---------|------|
| Untrusted text | Error text from a server is sanitised, bounded and marked as coming from the server; tool and server names shown to a model are sanitised |
| Notices | The runner's notices travel on a channel script output cannot forge (FR-036.5) |
| Arguments | An argument to single-call mode and to the one-shot call is a single JSON object; a malformed argument is reported without repeating its text |
| Secrets | Journals, reports and logs hold digests, sizes and names, never values; hand-written redacting `Debug` |
| Confinement | Spill files, journals, policy and reports live in system-chosen locations, never derived from script or server input |
| Skill content | Text that originates from an MCP server stays inert data in a rendered skill; owned by the skill block |

> [!question] Open maintainer decision
> Whether successful tool results printed to the caller by single-call mode, the
> runner and the one-shot call fall under the constitution's section V rule for
> untrusted text, or are an accepted exception, is open. Wherever results are
> printed, the documents state this as open; they claim neither sanitisation
> nor an exception.

## 12. Planned Cross-Block Contracts

| Block | Planned contract |
|-------|------------------|
| [[../cli/spec]] | Front doors for the runner (#571) and the one-shot call (#574), exit-code classification (section 5 there), `setup`'s Node check (#564) |
| [[../codegen/spec]] | Generated wrappers calling the helper and bridge retirement (#575), file-count bound chain. Implemented already: quiet single-call output (#570) and the printed form of an empty success (nothing, exit 0; [[../codegen/spec#13.6 Single-call mode (#567, #570, #583)]]) |
| [[../core/spec]] | The one registry of `_meta.json` fields (v3, implemented with #568); the stored annotations feed #573 and #572; this block defines no metadata field of its own |
| [[../introspector/spec]] | The shared connect step used by listing and calling (#574, #575) |
| [[../decisions/ADR-569-runtime-strategy-single-mcp-client]] | The gate in section 3 |

## 13. Open Decisions (consolidated)

| Decision | Section | Owner |
|----------|---------|-------|
| Runtime strategy: who is the MCP client at run time | 3 | Maintainer, ADR-569 |
| Run command name, flags, import specifier, Node floor | 4 | Maintainer, ask first |
| Spill and journal locations, retention and bounds | 4, 5 | Implementer, documented |
| Notice channel that scripts cannot forge | 4 | Implementer, reviewed |
| Digest construction and keying | 5 | Maintainer, ask first |
| Mechanism for FR-038.4, policy format, default of dry run | 6 | Maintainer, ask first |
| Shape of the shared connect step; exit code of a tool error | 7 | Implementer, ask first |
| Helper protocol, binary discovery, pool bounds | 8 | After ADR-569 |
| Reduced-privilege mechanism and its verification | 9 | After helper |
| Printed results versus the untrusted-text rule | 11 | Maintainer |

## 14. See Also

- [[../constitution]] — resource bounds, confinement, redacting `Debug`, untrusted text
- [[../decisions/ADR-569-runtime-strategy-single-mcp-client]] — runtime strategy record (proposed)
- [[../cli/spec]] — subcommands and exit codes
- [[../codegen/spec]] — generated bridge and wrappers
- [[../core/spec]] — metadata registry and provenance
- [[../introspector/spec]] — connection path and response bounds
