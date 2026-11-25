---
tags:
  - sdd
  - delta
  - skill
created: 2026-11-26
status: merged
related:
  - "[[../../spec]]"
---

# Delta: device names and pre-1.0 hardening (skill)

Issues: #701. Merged into [[../../spec]].

## ADDED

No new requirement ids; additions are inside the modified sections below.

## MODIFIED

### §10 consumed from `mcp-core` (#701)

Old:

>   `ServerSlug`, `ServerId`, `ServerIdSlugError`, `MAX_SERVER_ID_LENGTH`,

New:

>   `ServerSlug`, `ServerId`, `ServerIdSlugError`, `MAX_SERVER_ID_LENGTH`,
>   `is_reserved_device_name` (workspace-internal, saved scripts),

### §13 `RecipeName` rule (#701)

Old:

>   no leading hyphen; `parse_header

New:

>   no leading hyphen and is not a Windows device name (`ScriptError::InvalidName`, #701); `parse_header

### §13 persisted device-named scripts (#701)

Old:

>   uninspectable `scripts/` is reported as one invalid entry `scripts`.

New:

>   uninspectable `scripts/` is reported as one invalid entry `scripts`. A previously saved script with a device name
>   (for example `nul.ts`) is therefore skipped by `list` and reported by `check`; `run --save nul` is rejected.

### §13 open items

Old:

>   (follow-ups).

New:

>   (follow-ups). The report of a device-named script does not say why it is rejected (#712).

## REMOVED

None.

## Deviations

- `RecipeName` rejects the 24 device names (user decision); an existing `nul.ts` is skipped by `list` and reported by `check`.
- `CONIN$` and `CONOUT$` are out of scope.
