---
aliases:
  - ADR-579
  - saved script format
tags:
  - sdd
  - decision
  - skill
  - security
created: 2026-11-24
status: accepted
decided: 2026-11-24
decided-by: user
review-date: 2027-02-24
related:
  - "[[../constitution]]"
  - "[[../skill/spec]]"
  - "[[../cli/spec]]"
  - "[[ADR-569-runtime-strategy-single-mcp-client]]"
---

# ADR-579: Saved-Script Format and Location

> [!important]
> Decision record for #579.
>
> **Status: accepted (2026-11-24), option A, by the user.** JSON header in a leading block comment of `<name>.ts`; the
> proposed bounds (about 64 KiB per script, 50 scripts) are accepted as the starting values. The gate on T202
> is satisfied.

## 1. Context

A saved script is model-written code that runs again later. It must be validated without executing it, listed by
signature without its body, checked against the server's current tools, and survive regeneration of both the skill and
the server directory. A server directory is replaced wholesale on every `generate`
(`crates/mcp-files/src/filesystem.rs:816-840`), so it cannot hold scripts. The skill directory receives one file,
`SKILL.md` (`crates/mcp-cli/src/commands/skill.rs:336-352`), so `<skill-dir>/scripts/` is free and is the location
the Agent Skills format provides.

## 2. Options

### A. JSON header in a leading block comment of `<name>.ts` (recommended)

```ts
/*! mcp-script
{"v":1,"name":"triage-issues","description":"...","parameters":[...],"tools":["github/list_issues"],"writes":"read_only"}
*/
```

- One file per script; `save` is one atomic write; no executable code needed to read the header.
- Parsed by `serde_json` into typed `ScriptHeader`; no new dependency; `v` allows a later format.
- Header size is bounded by construction (read at most N bytes up to the closing marker).
- Cost: JSON in a comment is less pleasant to hand-edit.

### B. Leading `//` lines with `key: value`

- Human-friendly. Needs a hand-written parser with quoting rules for parameters; every quirk is a validation gap.

### C. Sidecar `<name>.json` next to `<name>.ts`

- Clean separation. Two files break atomic save, can drift apart, and double the confinement and bound surface.

### D. `export const meta = {...}` in the script

- Typed in TypeScript. Reading it requires parsing or executing TypeScript, which contradicts "saved scripts are never
  executed automatically".

## 3. Decision Inputs

| Criterion | A | B | C | D |
|-----------|---|---|---|---|
| Read without executing | yes | yes | yes | no |
| Atomic save | yes | yes | no | yes |
| No new dependency or parser | yes | no | yes | no |
| Typed validation at construction | yes | partial | yes | n/a |

## 4. Rules Common to All Options

- Location `<skills-dir>/<skill>/scripts/<name>.ts`; the file name equals `RecipeName` (validated newtype, charset and
  length bounded); a header whose `name` differs from the file name is refused.
- `save` refuses an existing name unless `Overwrite::Replace`; writes are atomic and confined with symlink-aware
  validation; a symlinked `scripts/` is rejected.
- `ToolRef` (`server/tool`) and `WriteIntent{ReadOnly,Writes}` are types, not strings.
- Bounds as named constants: header bytes, script bytes, scripts per skill, parameters per script (values chosen by the
  implementer and stated in the skill spec; proposed starting point is small, for example 64 KiB per script and 50
  scripts).
- `generate` and `skill` never touch `scripts/`; nothing in `scripts/` is executed by any command.
- `check` never modifies anything and reports scripts that reference tools the server no longer has.

## 5. Recommendation

Option A. It is the only option that is atomic, executes nothing to read, and needs no new parser or dependency.

## 6. Consequences

- A rejected or amended format changes only the header codec in `mcp-skill/src/scripts.rs`; the types, confinement,
  bounds and commands are unaffected.
- #579 and its parent #558 close only if this record is accepted in some form.

## 7. See Also

- [[../skill/spec]] — skill block that owns the saved-script contract after SDD-4
- [[../skill/spec#13. Saved Scripts (as built)]] — contracts for `scripts::{save,list,check}`
- [[../constitution]] — confinement, bounds, untrusted text
