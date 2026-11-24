---
aliases:
  - ADR-581
  - code mode interoperability
tags:
  - sdd
  - decision
  - skill
  - research
  - claude-code
created: 2026-11-24
status: proposed
review-date: 2027-02-24
related:
  - "[[../constitution]]"
  - "[[../skill/spec]]"
  - "[[../cli/spec]]"
---

# ADR-581: Interoperating With the Host's Experimental Code Mode

> [!important]
> This is a decision record, not a feature spec. It states what is settled now, lays out the options and
> criteria for what is not, and defines the investigation whose results will inform the open decision. It
> decides nothing about a native target and authorises no code change. Its status stays `proposed` until a
> maintainer accepts it.

## 0. Status of the Script-First Pass (2026-11-24)

- Status stays `proposed`; option (a) is the posture in effect for this pass because nothing generated depends on
  the undocumented surface.
- The section 5 investigation is **deferred by the maintainer** and tracked as a follow-up (human-run, interactive
  terminal only; agents cannot run it). Facts 3 and 9 to 13 therefore stay unobserved.
- This pass publishes `examples/host-code-mode.md` (#581): documented facts only (1, 2, 4 to 8), each re-fetched
  from its source and dated at publication, per section 4. It makes no token claim and describes nothing from
  facts 3 or 9 to 13.
- The `run` command of [[../cli/spec]] is the runner the guidance describes; the shell helper (fact 2) is the
  only documented way to reach it from the host's mode.

## 1. Context

The host (Claude Code) has an experimental mode in which the model writes JavaScript that calls the host's
tools instead of emitting one tool call per turn. That is the pattern this project's script-first direction
([[../skill/spec#14. Script-First Requirements]],
[[../cli/spec]]) is built on, so users will ask how the two fit together and the project must say
something true. The mode's script API for MCP tools is largely undocumented, which makes it an unstable
surface: building on it risks breaking with any host release, and writing guidance about it risks stating
things that are not so.

The facts below were researched on 2026-11-24. Each carries one label:

- **documented**: read from the host's documentation;
- **changelog-only**: named in the host changelog but not in the documentation;
- **undocumented**: no source found.

None was re-verified when this record was written; every statement must be re-read from its source, and dated,
before it is published anywhere (section 4, rule 1).

| # | Fact | Label |
|---|------|-------|
| 1 | Opt-in through the environment variable `CLAUDE_CODE_EXPERIMENTAL_CODE_MODE=1`, set in the shell or in the `env` block of user settings; project settings cannot enable it; organisations can block it by setting the variable's value to `0` in managed settings (there is no separate block flag). Re-read 2026-11-24 | documented |
| 2 | Code runs through a tool named `REPL`; `$` (used as a tagged template) and `edit()` appear in the page's examples only, not as a documented helper API. Re-read 2026-11-24 | documented as examples only |
| 3 | The changelog dates the introduction to v2.1.323 and also names `tools`, `log`, `preview`, `sleep`, `write()`, top-level `await` and `return`, and variables that persist between scripts | changelog-only |
| 4 | Nested tool calls are permission-checked and hooked under their usual names (`Bash`, `Edit`, `mcp__server__tool`); in auto mode a classifier reviews a script once, not each nested call | documented |
| 5 | Limits: 10 minutes in the foreground, 2 hours in the background. The page states no general rule that long output is cut with a path returned; it names only `uncutOutputNote`, from v2.1.331. Re-read 2026-11-24 | documented (limits); the output-cut rule is not stated beyond `uncutOutputNote` |
| 6 | Sandbox settings cover shell commands only; the JavaScript and its file, web and MCP calls run outside the sandbox | documented |
| 7 | Surface: interactive terminal sessions only (not the desktop app, the VS Code panel, cloud sessions or Remote Control); a fixed list of models: Opus 5.5, Sonnet 5.5, Haiku 5.5 and Fable 5.2. The page names no provider exclusions; Bedrock, Vertex and Foundry are supported from v2.1.331. Re-read 2026-11-24 | documented |
| 8 | Stated benefits: parallel independent commands, chaining results, returning only what is needed, filtering before the model reads; no token figures are published | documented |
| 9 | How MCP tools are invoked from a script | undocumented |
| 10 | The shape of MCP results inside a script | undocumented |
| 11 | Whether local modules or npm packages can be imported | undocumented |
| 12 | Behaviour in headless mode and in the Agent SDK | undocumented |
| 13 | Any guidance for MCP server authors | undocumented |

Facts 9 to 13 are exactly what a generator would need in order to target the mode natively, and exactly what
the host does not document.

> [!note] Corrections of 2026-11-24 (SDD-2)
> Facts 1, 2, 5 and 7 were corrected against the page re-fetched on 2026-11-24 (source:
> `https://code.claude.com/docs/en/code-mode.md`, fetched by developer-e for `examples/host-code-mode.md`). The
> earlier wording was not re-verified when this record was first written. Consequences: the shell helper `$` is an
> example, not a documented API, so option (a)'s "documented shell helper" is weaker than first stated; and there is no
> documented general output-cut rule, so the guidance does not claim one (section 3, rule 4). Facts 3, 4, 6, 8 and
> 9 to 13 were not re-read in this pass.

## 2. The Decision To Make

Whether anything this project generates (tool files, catalog, skill text, saved scripts) should target the
host's code mode natively, that is, call the host's own script API for MCP tools instead of the project's
runner. This record does not make that decision.

### 2.1 Options

**(a) No native target; interoperate through documented pieces only — recommended.** Guidance explains how the
project's runner and saved scripts are used from inside the host's mode through the documented shell helper
(fact 2), within facts 1, 2, 4, 5, 6, 7 and 8. Nothing generated depends on facts 3 or 9 to 13.

- **Pros:** costs nothing in stability; every statement is checkable; keeps one execution path (the project's
  runner), so the single-path principle of the runtime block is not weakened; fully reversible.
- **Cons:** does not benefit from whatever native typed access the host may later provide; users in the mode
  go through a shell helper rather than a native call.

**(b) Generate code that targets the host's script API for MCP tools.**

- **Pros:** the most direct use of the host's mode, if the API turns out to be stable.
- **Cons:** rests on facts that are undocumented today; any host release can break the output silently; adds a
  second way to call tools beside the project's runner; guidance about it would assert behaviour nobody can
  cite.

**(c) A thin adapter, shipped separately from the generated output, that maps the generated modules onto the
host's API.**

- **Pros:** confines the dependency on the undocumented surface to one replaceable piece.
- **Cons:** still depends on the same undocumented facts; still a second way to call tools; its maintenance
  cost is unknown until the API is characterised.

### 2.2 Criteria

| Criterion | What is asked |
|-----------|---------------|
| Documentation | Is every fact the option relies on documented, with a source page and a date? |
| Stability | Has the surface stayed the same across consecutive host releases, and is the mode still marked experimental? |
| Single path | Does the option add a second way to call tools beside the runner? |
| Portability | Does the result still work in hosts other than this one? |
| Reversibility | Can the option be withdrawn without breaking saved scripts or generated servers? |
| Honesty | Can every published sentence be traced to a documented source? |

By these criteria only option (a) is satisfiable today.

## 3. What Is Settled Now

These rules hold regardless of how the open decision ends:

1. Published guidance and examples cover documented behaviour only (facts 1, 2, 4, 5, 6, 7 and 8). Anything
   observed only during the investigation of section 5 stays in this record and is never relabelled
   documented.
2. Nothing shipped depends on the undocumented API: no generated file, skill text, saved script or command
   calls, documents or teaches it, while the decision is "no native target".
3. The guidance makes no token-saving claim about the host's mode; none is published (fact 8).
4. The guidance states the runner's own output budget and, for the host, only what fact 5 documents (the time limits
   and `uncutOutputNote` from v2.1.331); it makes no claim of a general host output cut (corrected 2026-11-24). It also
   tells the reader that scripts in the mode run outside the sandbox except shell commands (fact 6) while nested
   calls remain permission-checked (fact 4).
5. If the decision ever becomes "native target", a separate specification is written first; this record
   authorises no implementation.

## 4. Rules for the Guidance

1. Every statement carries its source page and the date it was last verified, and is re-verified before each
   publication.
2. Facts 3 and 9 to 13 are not described.
3. An example that invokes the runner or a saved script through the shell helper is published only after it is
   confirmed against the documentation or by the investigation of section 5.
4. Placement (a section of the user documentation, a separate page, or a mention in the skill) is decided with
   the documentation rewrite (#560).

## 5. Investigation Protocol

A time-boxed investigation, run by a person in an interactive terminal session, characterises facts 3 and 9 to
13. It is not run by an agent: it needs an interactive terminal, and the JavaScript it runs sits outside the
sandbox (fact 6).

**Preconditions.**

- The mode is enabled by hand, in the shell or in user settings; no agent edits a settings file to enable it.
  If managed settings block it, record "blocked", stop, and leave the decision unchanged.
- A local test server with no credentials and no write-capable tools, offering at least: a text tool, a tool
  with an output schema returning structured content, a tool that succeeds with empty content, a tool that
  reports an error, and a tool with a large result. Which server is used is an open decision.
- A scratch directory holding nothing sensitive.
- The time box and the stop rule are written down before the first step (values open).

**Questions.**

| ID | Question | Related fact |
|----|----------|--------------|
| Q1 | By what form does a script invoke an MCP tool, and is the form stable across two consecutive host builds? | 9 |
| Q2 | What shape do results have inside a script for text, structured, empty and error results? | 10 |
| Q3 | What does a failed call surface: its form, whether exception text carries server-supplied text, whether it carries stack traces? | 10, 13 |
| Q4 | Can a script import a local TypeScript module, a Node built-in module and an npm package? | 11 |
| Q5 | Do skills load inside the mode, and do their bundled files and scripts resolve from a script? | 11 |
| Q6 | What is the numeric threshold of the long-output cut, and what form does the returned path take? | 5 |
| Q7 | How are permissions and hooks applied when the project's runner is called through the shell helper? | 4 |
| Q8 | Which of the names in the changelog (`tools`, `log`, `preview`, `sleep`, `write()`, persistent variables) exist in the running build? | 3 |

Headless mode and the Agent SDK (fact 12) are out of scope: the host documents neither for this mode, and the
investigation does not extend to them.

**What to record for every observation.**

1. The host version and the date.
2. The exact script, as run.
3. The exact output.
4. The label "observed".

An observation never replaces a documented statement: if it contradicts the documentation, both are recorded,
the documentation stays authoritative for published claims, and the contradiction is a finding for the host's
maintainers. If the host version changes mid-run, the affected questions restart, and each record carries its
own version.

**Handling of transcripts.** Scrub credentials, home-directory paths and any server-supplied text beyond what
the finding needs before a transcript leaves the person's machine. Truncate large or hostile output before
recording it.

**Stop rule.** The investigation ends when every question has either an observation or an explicit "unknown",
or when the time box ends. Questions left unanswered are recorded as unknown, and the decision assumes no
answer for them.

**Where results go.** Results are findings for a maintainer to append to this record, as a dated section, or to
file; an agent does not create tracked files from them.

## 6. Exit Criteria for the Open Decision

The decision "no native target" is revisited when all of the following hold:

1. The host's documentation states the invocation form of an MCP tool from a script, the shape of results for
   text, structured, empty and error cases, and the error surface (fact 9, fact 10 and the error part of fact
   13 move to documented).
2. The surface has stayed unchanged across a number of consecutive host releases (the number is open).
3. The mode has left experimental status, or a maintainer records why that is not required (open).
4. The investigation has an observation or an explicit "unknown" for every question, and the observations agree
   with the documentation.
5. A measurement from the context-cost command ([[../cli/spec#19. Script-First Requirements and Contracts]])
   shows a benefit the runner does not already give; without one, the criterion of a second path is not met.

A maintainer decides whether to reopen the decision when these hold. The new decision is recorded as a new
section or a successor record, with the host version and the date.

## 7. Recommendation

Option (a) is the recommendation of the adopted direction, recorded as a recommendation: complement the host's mode through the
documented shell helper, run the investigation of section 5, and keep the decision on a native target open
against the exit criteria of section 6. Nothing in sections 3 and 4 depends on the outcome.

## 8. Open Questions

- [NEEDS CLARIFICATION: the time box of the investigation, and the age after which a verified fact must be re-read.]
- [NEEDS CLARIFICATION: which local test server the investigation uses, and where it lives.]
- [NEEDS CLARIFICATION: whether the return value and quoting rules of the shell helper are documented well enough to write an example without the investigation.]
- [NEEDS CLARIFICATION: where the guidance lives, and whether the skill mentions the mode at all (see the skill block's open decision).]
- [NEEDS CLARIFICATION: thresholds for exit criteria 2 and 3: the number of consecutive releases, and whether leaving experimental status is required.]

## 9. See Also

- [[../constitution]] — the rules on secrets and untrusted text this record applies
- [[../skill/spec#14. Script-First Requirements]] — the skill text that may mention the mode
- [[../cli/spec]] — the runner the guidance describes
- [[../cli/spec#19. Script-First Requirements and Contracts]] — the context-cost command used by exit criterion 5
