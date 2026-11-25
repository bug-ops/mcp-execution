---
tags:
  - sdd
  - delta
  - codegen
created: 2026-11-26
status: merged
related:
  - "[[../../spec]]"
---

# Delta: device names and pre-1.0 hardening (codegen)

Issues: #701. Merged into [[../../spec]].

## ADDED

No new requirement ids; additions are inside the modified sections below.

## MODIFIED

### §5 collision checks (#701)

Old:

>   combines two collision checks with different case sensitivity (numeric

New:

>   combines these collision checks, with different case sensitivity (numeric

### §5 device names (#701)

Old:

>     left as-is.
>   - The fixed output filenames

New:

>     left as-is.
>   - Windows device names (`mcp_execution_core::is_reserved_device_name`, the 24 names of
>     [[../core/spec]]) are checked **case-insensitively**, because `<name>.ts` cannot be a file on
>     Windows: a tool named `nul` becomes `nul_2`, `CON` becomes `CON_2` (#701). File name, export name,
>     catalog `@see` and `_meta.json` `typescript_name` follow, since they share `typescript_name`. As
>     with other suffixes, a later tool resolving to the literal `con_2` moves on to `con_2_2`.
>   - The fixed output filenames

### §11 consumed from `mcp-core` (#701)

Old:

> - **Consumes**: `mcp-core::metadata`/error types/forbidden-char constants;

New:

> - **Consumes**: `mcp-core::metadata`/error types/forbidden-char constants, `is_reserved_device_name`
>   (workspace-internal);

## REMOVED

None.

## Deviations

- A device-name tool taking a `_2` suffix can push a later tool resolving to that literal name to `_2_2`.
