---
tags:
  - sdd
  - delta
  - cli
created: 2026-11-26
status: merged
related:
  - "[[../../spec]]"
---

# Delta: device names and pre-1.0 hardening (cli)

Issues: #702, #703, #705, #707. Merged into [[../../spec]].

## ADDED

No new requirement ids; additions are inside the modified sections below.

## MODIFIED

### `rmcp` log cap (#421, #702)

Old:

> closes this specific path — *debug-level, raw-line* logging — by adding the directive of
> `mcp_execution_core::cli::RmcpLogCap` (the one statement of the cap policy, also used by `mcp-server`, see
> [[../core/spec]]; #608, #678) in the private `runner::log_filter`, applied to *every* base filter — not just the
> `try_from_default_env()` fallback, since a directive folded only into that fallback string would never apply to the
> verbose branch (which never calls `try_from_default_env` at all) and is dead code whenever `RUST_LOG` parses
> successfully:
>
> ```rust
> Ok(base.add_directive(cap.directive().parse()?)) // cap: RmcpLogCap::Info, or Warn for the quiet profile
> ```
>
> The cap is level-based, not a content filter: `rmcp` also logs a `Debug`-formatted peer
> notification at `info` (`service.rs`'s `tracing::info!(?notification, ...)`), which `rmcp=info`
> does not and cannot suppress. That site is mitigated (`?` renders via `Debug`, which escapes
> control characters, unlike the raw-`Display` `message`-field sites this cap targets), not
> eliminated — the full notification content still reaches stderr. Closing it would require
> content-level sanitization of `rmcp`'s own event, not a level directive, and is out of scope here.
>
> `log_filter` adds an `rmcp=<cap>` directive (`info` here; `warn` for the quiet profile) via `EnvFilter::add_directive`.
> `tracing_subscriber` orders directives by target specificity, so this directive (more specific
> than a bare global `debug`) wins over it. An operator who explicitly sets a *more specific*
> directive, e.g. `RUST_LOG=rmcp::transport=debug`, still wins over this one — intentional, since
> the goal is closing the accidental broad-`debug` case (including plain `--verbose`), not blocking
> a deliberate request for `rmcp` transport debug logs; this is the escape hatch for an operator who
> needs one. Specificity is a different axis from level, though: an *equally* specific
> `RUST_LOG=rmcp=debug` (same `rmcp` target this cap sets, different level) is not merged with this
> cap's `rmcp=info` — `tracing_subscriber`'s `Directive` ordering does not compare level, so
> `EnvFilter::add_directive` on a same-target directive *replaces* the existing entry, silently
> downgrading an operator's explicit `rmcp=debug` to `info`. Both the escape hatch and the
> replace-not-merge behavior are pinned by dedicated tests rather than assumed. Being a pure
> function (no `std::env` access), `log_filter` is unit-tested directly with a scoped
> subscriber rather than by mutating `RUST_LOG` in-process (parallel test threads share one
> process) — mirroring the `SharedBuf`-based scoped-subscriber pattern `runner.rs`'s
> `RedactingWriter` tests already establish.

New:

> closes this specific path — *debug-level, raw-line* logging — with the cap of
> `mcp_execution_core::cli::RmcpLogCap` (the one statement of the cap policy, also used by `mcp-server`, see
> [[../core/spec]]; #608, #678), built in the private `runner::log_filter(verbose, profile, env) ->
> Result<(EnvFilter, Targets), ParseError>` and applied to *every* base filter (`--verbose`, `RUST_LOG`, either profile
> default). The subscriber applies base, then cap, then the format layer.
>
> **AND filter (#702).** An event is emitted only if the base filter enables it **and** the cap does, so the cap only
> lowers verbosity. The cap is a `Targets` filter parsed from `cap_filter_directives(&base.to_string())`: `rmcp` at
> `info` (`warn` for the quiet profile default), plus the base filter's `rmcp::` target directives (without span or
> field parts) as exemptions. `cap_filter_directives` returns a `trace` default, which the binary replaces with the base
> filter's max level hint. With the `info` cap:
>
> | Base filter | `rmcp` events emitted |
> |---|---|
> | `RUST_LOG=error` | errors only |
> | `--verbose` or `RUST_LOG=debug` | up to `info` |
> | `RUST_LOG=rmcp=debug` | up to `info` |
> | `RUST_LOG=rmcp=error` | errors only |
> | `RUST_LOG=rmcp::transport=debug` | `rmcp::transport` up to `debug`, nothing else under `rmcp` |
>
> The last row is the escape hatch: a directive naming an `rmcp::` submodule is a deliberate request and is not capped.
> Directives with a span or field part are not exempt.
>
> The cap is level-based, not a content filter: `rmcp` also logs a `Debug`-formatted peer
> notification at `info` (`service.rs`'s `tracing::info!(?notification, ...)`), which an `info` cap
> does not suppress. That site is mitigated (`?` renders via `Debug`, which escapes
> control characters, unlike the raw-`Display` `message`-field sites this cap targets), not
> eliminated — the full notification content still reaches stderr. Closing it would require
> content-level sanitization of `rmcp`'s own event, not a level directive, and is out of scope here.
>
> Being a pure function (no `std::env` access), `log_filter` is unit-tested directly with a scoped
> subscriber rather than by mutating `RUST_LOG` in-process (parallel test threads share one
> process) — mirroring the `SharedBuf`-based scoped-subscriber pattern `runner.rs`'s
> `RedactingWriter` tests already establish.

### logging profile for `call`, `run`, `helper` (#702)

Old:

> `log_filter(verbose, profile, env)` (adds the `RmcpLogCap` directive); `run` passes

New:

> `log_filter(verbose, profile, env)` (returns the base filter and the `RmcpLogCap` filter, section 4); `run` passes

### §7 steps 1-3 (#703)

Old:

> 1. `resolve_server_config` → `discover_server_info`: if `--name` is given,
>    it is validated with `ServerSlug::new` **before**
>    the connection attempt and only overrides `ServerInfo.id` once valid —
>    an invalid `--name` (traversal shape, absolute path, or simply outside
>    the `[a-z0-9-]` slug charset, e.g. `My Server!`, or a Windows reserved device name, #696) is a
>    hard error with exit 2 (`INVALID_INPUT`, `CoreError::InvalidArgument`; the rejected name is sanitized and quoted;
>    `drift` shares the check) instead of being silently slugified into a
>    different id. **Breaking behavior change** from the pre-#311 CLI, which
>    constructed `ServerId::new(custom_name)` directly with no validation.
> 2. If the server has zero tools: logs a warning and returns
>    `ExitCode::SUCCESS` (not an error) without generating anything.
> 3. `resolve_server_dir_name` turns `server_info.id` into the directory
>    name, re-validating it with `ServerSlug::new` regardless of which arm
>    produced the id: `derive_server_id_from_path_or_name` (stdio command),
>    `derive_server_id_from_url` (http/sse — see [[#3. common.rs]]), or the
>    already-validated `--name` override. This check is a redundant backstop
>    for those three arms, but the **sole** enforcement point when the id
>    came straight from an unvalidated `--from-config` `mcp.json` key with
>    no `--name` override —
>    the error message differs accordingly (names `mcp.json` and suggests a
>    ready-to-use `--name` slug vs. framing the failure as an internal error
>    for the other arms, since reaching it there would mean one of their own
>    checks has a bug) (issue #311). The two derivations append `-server` to a reserved device name (`aux` gives
>    `aux-server`, #696), so a derived id always passes. A `--from-config` key that is not a slug is rejected only
>    after introspection (open follow-up #703).

New:

> 1. `resolve_server_config`, then `name_override`: if `--name` is given,
>    it is validated with `ServerSlug::new` and only overrides `ServerInfo.id` once valid —
>    an invalid `--name` (traversal shape, absolute path, or simply outside
>    the `[a-z0-9-]` slug charset, e.g. `My Server!`, or a Windows reserved device name, #696) is a
>    hard error with exit 2 (`INVALID_INPUT`, `CoreError::InvalidArgument`; the rejected name is sanitized and quoted;
>    `drift` shares the check) instead of being silently slugified into a
>    different id. **Breaking behavior change** from the pre-#311 CLI, which
>    constructed `ServerId::new(custom_name)` directly with no validation.
> 2. `generated_location(<--name or the server id>, ..)` runs **before any server process is started or connection
>    opened** (#703). It resolves the base directory with `ServersDir::resolve(output_dir)` (section 3, replaces
>    `resolve_base_dir`, #618; default `~/.claude/servers`, a leading `~` component is expanded, a bare `~` included),
>    and `resolve_server_dir_name` turns the id into the directory
>    name, re-validating it with `ServerSlug::new` regardless of which arm
>    produced the id: `derive_server_id_from_path_or_name` (stdio command),
>    `derive_server_id_from_url` (http/sse — see [[#3. common.rs]]), or the
>    already-validated `--name` override. This check is a redundant backstop
>    for those three arms, but the **sole** enforcement point when the id
>    came straight from an unvalidated `--from-config` `mcp.json` key with
>    no `--name` override —
>    the error message differs accordingly (names `mcp.json` and suggests a
>    ready-to-use `--name` slug vs. framing the failure as an internal error
>    for the other arms, since reaching it there would mean one of their own
>    checks has a bug) (issue #311). The two derivations append `-server` to a reserved device name (`aux` gives
>    `aux-server`, #696), so a derived id always passes. A bad key or an unusable home or `--servers-dir` therefore fails
>    without starting the server, also for a server that has no tools. A valid `--name` bypasses the key check.
> 3. `discover_server_info` (connect, introspect, apply the validated override). If the server has zero tools: logs a
>    warning and returns `ExitCode::SUCCESS` (not an error) without generating anything.

### §7 base directory step moved into step 2 (#703)

Old:

> 5. `ServersDir::resolve(output_dir)` (section 3, replaces `resolve_base_dir`, #618) — defaults to `~/.claude/servers`; a leading `~` component is expanded (a bare `~` included).
> 6. **`--dry-run`**

New:

> 5. **`--dry-run`**

### §7 export step number

Old:

> 7. Otherwise: `FilesBuilder::from_generated_code

New:

> 6. Otherwise: `FilesBuilder::from_generated_code

### §7 id-sanitization reference

Old:

> id-sanitization in step 3: a future

New:

> id-sanitization in step 2: a future

### saved-script rationale step reference

Old:

> §7 step 7);

New:

> §7 step 6);

### FR-045.10 step reference

Old:

> (§7 step 3) | must |

New:

> (§7 step 2) | must |

### `setup` runtime detection (#705)

Old:

>    unsuitable Node.js is not fatal by itself: Bun and then Deno are probed (below).

New:

>    unsuitable Node.js is not fatal by itself: Bun and then Deno are probed (below). `node --version` is bounded
>    (#705, see **Node.js version check**).

### Node.js version check (#705)

Old:

> **Runtime probe (#685, `runtime/probe.rs`).**

New:

> **Node.js version check (#705, `runtime/node.rs`, shared by `setup` and `run`).** `check_node_version` runs
> `node --version` as a process tree through the same capture helper as the runtime probe (null stdin and stderr, stdout
> capped at 4096 bytes) with a 10 s limit. The tree is killed on timeout and when the check is dropped. Failures: a
> spawn failure gives the "Node.js not found in PATH" message; a read or wait failure or a non-zero exit gives
> "Node.js is installed but not working correctly"; a timeout gives "Node.js did not answer `node --version` within
> 10 s". In `setup` each of these falls back to the Bun and Deno probe.
>
> **Runtime probe (#685, `runtime/probe.rs`).**

### `setup` signals (#705)

Old:

> ignored at startup stays ignored; SIGTERM is always handled. A running probe and its whole process tree are stopped,
> on timeout as well. Residuals: SIGKILL of `setup` can leave a hanging probe, and a signal in the instant a shim forks
> can miss that child. The `node --version` check has no timeout and is not stopped with a cancelled `setup` (open
> follow-up #705).

New:

> ignored at startup stays ignored; SIGTERM is always handled. A running probe or `node --version` check and its whole
> process tree are stopped, on timeout as well. Residuals: SIGKILL of `setup` can leave a hanging probe, and a signal in
> the instant a shim forks can miss that child.

### `run` signals cover the Node.js check (#705)

Old:

> reading a stdin script and writing its snapshot, the type check, a pre-spawn check, and the script process.

New:

> reading a stdin script and writing its snapshot, the Node.js version check (bounded to 10 s, see **Node.js version
>   check**), the type check, a pre-spawn check, and the script process.

### `run` cancel notice (#705)

Old:

> `...; the script was not started` (type check or pre-spawn)

New:

> `...; the script was not started` (Node.js version check, type check or pre-spawn)

### FR-045.18 (#707)

Old:

> - FR-045.18 (wave 2): after a `ToolsDiffer` with removed tools, `scripts_using()` lists the saved scripts
>   (`scripts::list`) that name a removed tool of this server, through the same bounded and sanitised `NameList` (report
>   field `scripts_using_removed_tools`, text heading "saved scripts using a removed tool"). Best effort: a missing
>   skill directory or a `list` error gives no names. Only removed tools count; scripts that use a changed tool are
>   not listed.

New:

> - FR-045.18 (wave 2, #707): after a `ToolsDiffer` with removed or changed tools, `scripts_using()` makes one
>   `scripts::list` call and returns two lists of saved scripts of this server: those that name a removed tool (report
>   field `scripts_using_removed_tools`, text heading "saved scripts using a removed tool") and those that name a changed
>   tool (`scripts_using_changed_tools`, heading "saved scripts using a changed tool"). Both fields are always present,
>   each a bounded and sanitised `NameList` (`{names, more}`) in listing order; a script naming both kinds is in both.
>   Best effort: a missing skill directory or a `list` error gives no names.

### resolved note for FR-045.13 and FR-045.18 (#707)

Old:

> FR-045.18 covers removed tools only.

New:

> FR-045.18 covers removed and changed tools (#707).

## REMOVED

None.

## Deviations

- `run` races the Node.js version check against termination signals, in addition to the 10 s bound.
- `generate` also resolves the servers directory before connecting, so its errors precede the zero-tools exit.
- The binary rebuilds the parsed cap filter to replace its `trace` default with the base filter's max level hint.
- #704 is not part of this change; the test environment note is unchanged.
