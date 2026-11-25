---
tags:
  - sdd
  - delta
  - core
created: 2026-11-26
status: merged
related:
  - "[[../../spec]]"
---

# Delta: device names and pre-1.0 hardening (core)

Issues: #701, #702. Merged into [[../../spec]].

## ADDED

No new requirement ids; additions are inside the modified sections below.

## MODIFIED

### Windows reserved device names (#696, #701)

Old:

> valid. The published JSON schema is unchanged and does not express the exclusion. File names of generated tools and
> saved scripts are not covered (open follow-up #701).

New:

> valid. The published JSON schema is unchanged and does not express the exclusion.
>
> The rule is stated once, in `is_reserved_device_name(name: &str) -> bool` (`src/path.rs`, `#[doc(hidden)]`,
> workspace-internal, #701): true when the whole string equals one of the 24 names in any ASCII letter case; no
> extension stripping (`con.ts` and `con-1` are false). Server slugs, saved script names ([[../skill/spec]]) and
> generated tool file names ([[../codegen/spec]]) share it.

### `path` module

Old:

> pub fn contains_parent_dir(path: &Path) -> bool; // true if any component is `..`
> ```

New:

> pub fn contains_parent_dir(path: &Path) -> bool; // true if any component is `..`
> #[doc(hidden)]
> pub fn is_reserved_device_name(name: &str) -> bool; // workspace-internal, see `ServerSlug` (#701)
> ```

### `rmcp` log cap API (#702)

Old:

> impl RmcpLogCap { pub const fn directive(self) -> &'static str; } // "rmcp=info" | "rmcp=warn"

New:

> impl RmcpLogCap {
>     pub const fn directive(self) -> &'static str; // "rmcp=info" | "rmcp=warn"
>     pub fn cap_filter_directives(self, base_filter: &str) -> String;
> }

### `rmcp` log cap behaviour (#702)

Old:

> `tracing-subscriber`. It is workspace-internal (`NFR-COM-032`): not part of the documented 1.0 API. Each binary
> parses the directive and adds it to its own `EnvFilter`: `mcp-cli` uses `Info`, or `Warn` for the quiet profile of
> `call`, `run` and `helper`; `mcp-server` uses `Info`. The cap replaces an equally specific `RUST_LOG=rmcp=...`
> directive rather than merging with it. The former `log_filter` module (`cap_rmcp_log_level`) and the
> `tracing-filter` feature were removed (#678).

New:

> `tracing-subscriber`. It is workspace-internal (`NFR-COM-032`): not part of the documented 1.0 API. `mcp-cli` uses
> `Info`, or `Warn` for the quiet profile of `call`, `run` and `helper`; `mcp-server` uses `Info`. The former
> `log_filter` module (`cap_rmcp_log_level`) and the `tracing-filter` feature were removed (#678).
>
> The cap is a second filter combined with the base filter by logical AND, so it only lowers verbosity.
> `cap_filter_directives(base_filter)` returns `trace`, then `directive()`, then every comma-separated directive of
> `base_filter` that starts with `rmcp::` and has no span or field part, comma-joined. Those repeated directives keep
> their targets exempt from the cap; an `rmcp::` directive with a span or field part is not repeated, so the cap applies
> to it. Each binary parses the result into a `Targets` filter and replaces the `trace` default with the base filter's
> max level hint, so the cap does not raise the subscriber's max level for targets outside `rmcp`.

### consumer row `mcp-codegen` (#701)

Old:

> | `mcp-codegen` | `Error`/`Result`, `metadata::*`

New:

> | `mcp-codegen` | `Error`/`Result`, `is_reserved_device_name` (workspace-internal), `metadata::*`

### consumer row `mcp-skill` (#701)

Old:

> | `mcp-skill` | `sanitize_path_for_error`, `contains_parent_dir`, `ServerSlug`,

New:

> | `mcp-skill` | `sanitize_path_for_error`, `contains_parent_dir`, `is_reserved_device_name` (workspace-internal), `ServerSlug`,

### NFR-COM-032 workspace-internal listing

Old:

> `cli::RmcpLogCap` (`#[doc(hidden)]`), excluded from the semver promise.

New:

> `cli::RmcpLogCap` (with `directive` and `cap_filter_directives`) and `is_reserved_device_name` (both
> `#[doc(hidden)]`), excluded from the semver promise.

## REMOVED

None.

## Deviations

- `is_reserved_device_name` is exported `#[doc(hidden)]` (workspace-internal), not added to the 1.0 public-items table (user decision).
- `cap_filter_directives` returns a `trace` default that each binary replaces with the base filter's max level hint.
