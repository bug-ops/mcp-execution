---
aliases:
  - mcp-execution-cli spec
  - CLI spec
tags:
  - sdd
  - spec
  - cli
  - security
created: 2026-07-27
status: documented
related:
  - "[[../constitution]]"
  - "[[../core/spec]]"
  - "[[../introspector/spec]]"
  - "[[../codegen/spec]]"
  - "[[../files/spec]]"
  - "[[../skill/spec]]"
  - "[[../runtime/spec]]"
  - "[[../decisions/ADR-569-runtime-strategy-single-mcp-client]]"
---

# Block: Command-Line Interface (`mcp-execution-cli`)

> [!abstract]
> Path: `crates/mcp-cli`. `clap`-derived CLI binary (`mcp-execution-cli`)
> that drives the whole pipeline directly (introspect → generate → skill),
> plus server-config management, environment setup, and shell completions.
> Depends on `mcp-execution-core`, `mcp-execution-introspector`,
> `mcp-execution-codegen`, `mcp-execution-files`, `mcp-execution-skill`.

## 1. Responsibility

Provide the human/scriptable entry point to everything the other crates
do, without going through the MCP-server layer at all: read
`~/.claude/mcp.json` or accept transport flags directly, introspect a
server, generate progressive-loading TypeScript, render SKILL.md, and
manage/validate server configuration and the local runtime environment.

The crate is a library (`src/lib.rs`) with a thin binary entry point
(`src/main.rs`: parse `Cli`, call `runner::init_logging`, call
`runner::execute_command`, then `std::process::exit` on the returned
`ExitCode`). `lib.rs` exposes `pub mod cli`, `pub mod runner`, `pub mod
actions`, `pub mod commands`, and `pub mod formatters` (plus a re-exported
`ServerAction`) — `cli::{Cli, Commands}` and `runner`'s
command-execution/exit-code-classification entry points are genuine public
library API, not merely an implementation detail of the compiled binary, so
an external crate or integration test can drive command parsing/execution
in-process (issue #188).

## 2. Subcommands (`Commands` enum, `src/cli.rs`)

| Subcommand | Purpose | Key flags |
|---|---|---|
| `introspect` | Connect + display server capabilities/tools | `--from-config`, `server` (positional), `--arg`/`-a`, `--env`/`-e`, `--cwd`, `--http`/`--sse`, `--header`, `--detailed`/`-d`, `--connect-timeout-secs`, `--discover-timeout-secs` |
| `generate` | Introspect + emit progressive-loading TypeScript to `~/.claude/servers/{id}/` | identical transport flags as `introspect` (`--arg`/`-a`, `--env`/`-e`, `--cwd`, `--http`/`--sse`, `--header` — both commands flatten the same `ServerFlags`, so the `-a`/`-e` short aliases apply here too), plus `--name`, `--progressive-output`, `--observed-shapes <FILE>`, `--dry-run` |
| `skill` | Render SKILL.md directly from a generated server's tools (no LLM) | `-s/--server`, `--servers-dir`, `-o/--output`, `--skill-name`, `--hint` (repeatable), `--overwrite` |
| `server` | Manage `~/.claude/mcp.json` entries | subcommand: `list`, `info <server>`, `validate <command>` |
| `setup` | Validate the local runtime (Node.js version, executable bits, config presence) | none |
| `completions` | Emit a shell completion script | `<shell>` (bash/zsh/fish/powershell/elvish) |

Global flags on `Cli` (apply to every subcommand): `-v/--verbose` (DEBUG log
level), `--format {json,text,pretty}` (default `pretty`, case-insensitive).
`--format` is typed as `mcp_execution_core::cli::OutputFormat` directly via
clap's `PossibleValuesParser` (mapped through `OutputFormat::from_str`),
not a raw `String` parsed post-hoc: `--help` lists the three possible
values, an invalid value (e.g. `--format xml`) is rejected by clap itself
before any command handler runs (routed through the same
`runner::report_and_classify`/`ExitCode::INVALID_INPUT` path as a
handler-level failure — see [[#5. runner.rs]]), and
`completions`-generated shell scripts complete `--format` from the same
three values (issue #206).

`--log-format {text,json}` (issue #399) is a second, independent global
flag: it selects the *diagnostic log* format (text vs. structured JSON,
written to stderr via `runner::init_logging`), not the command *result*
format `--format` controls. Typed as `Option<mcp_execution_core::cli::LogFormat>`
via the same `PossibleValuesParser`/`ignore_case = true` pattern as
`--format` (so `--help` lists both possible values and case-insensitivity
comes from clap, not a manual `to_lowercase`), deliberately with **no**
`default_value` — `None` means "flag not passed, consult the
`MCP_EXECUTION_LOG_FORMAT` environment variable" (see
[[#5. runner.rs|init_logging]]). An invalid `--log-format` value is
rejected by clap itself, exactly like `--format`; an invalid
`MCP_EXECUTION_LOG_FORMAT` value is handled leniently (falls back to text
with a `WARN` log line) since it is consulted only after the flag, deep
inside `init_logging`, not at clap-parse time.

`introspect`/`generate` flatten a shared `ServerFlags` (`#[derive(Args)]`,
private fields, `cli.rs`) holding `from_config`/`server`/`args`/`env`/`cwd`/
`http`/`sse`/`headers`/`connect_timeout_secs`/`discover_timeout_secs`.
Exclusivity is a single clap `ArgGroup` named `server_source`
(`required(true)`, default `multiple(false)`) over
`[from_config, server, http, sse]` — i.e. exactly one of "load from config"
or "pick a transport" must be chosen, enforced before any command handler
runs. `from_config` additionally `conflicts_with_all` the non-selector args
(`args`/`env`/`cwd`/`headers`/the two timeout overrides). `ServerFlags`
converts into the closed `ServerSource` domain enum via `TryFrom` (see
below) once parsing has run — its private fields make the "no selector" /
"multiple selectors" states unconstructible from outside `cli.rs`, not just
runtime-checked.

## 3. `common.rs` — Shared Server-Resolution Machinery

This module is the single place `introspect` and `generate` (which accept
an identical flag surface) resolve "how do I reach this server":

```rust
pub enum ServerSource {
    Config { name: String },
    Flags { transport: TransportArgs, connect_timeout_secs: Option<u64>, discover_timeout_secs: Option<u64> },
}
pub(crate) fn resolve_server_config(source: ServerSource) -> Result<(ServerId, ServerConfig)>;
```
`ServerSource` is the output of `TryFrom<ServerFlags> for ServerSource`
(`cli.rs`, needs `ServerFlags`'s private fields). Every value of this type
is a legal state: `--from-config` and the timeout overrides are folded into
the same enum because they share one exclusivity group, which also means a
`Config` source can never carry a meaningless timeout override — unlike the
former `RawServerArgs` landing zone (`pub` fields, all-`Option`/`Vec`
shape), where that combination was constructible but silently ignored.
Named fields (rather than positional parameters) still prevent transposing
two same-typed fields (e.g. `http`/`sse`, both `Option<String>`) — issue
#286's original fix, preserved here.

```rust
pub struct McpConfig { pub mcp_servers: HashMap<String, McpServerEntry> }
pub struct McpServerEntry { pub transport: McpTransport, pub connect_timeout_secs, pub discover_timeout_secs }
pub enum McpTransport { Stdio{command,args,env,cwd}, Http{url,headers}, Sse{url,headers} }
pub enum TransportArgs { Stdio{...}, Http{...}, Sse{...} } // raw, unparsed CLI-flag mirror of McpTransport; every variant is a legal state by construction (no all-`None`/"both http and sse" shape exists). `pub` with `pub` fields, so directly constructible by any caller; the real CLI path only ever produces one via `TryFrom<ServerFlags>`, which enforces "exactly one transport" at the ServerFlags -> ServerSource boundary
```

`McpServerEntry`'s `Deserialize` is **hand-written** (via a
`RawMcpServerEntry` landing zone + `TryFrom`), not derived, so it can:
- Infer `stdio` vs `http` when `"type"` is absent (via `command` vs `url`
  presence) — an `mcp.json` entry can omit `"type"` for a bare `url`-only
  http entry.
- Reject cross-field violations with precise messages (`"http server entry
  must not set \"command\""`, `"stdio server entry must not set
  \"url\""`).
- Silently accept-and-warn on unrecognized top-level keys (`extra:
  HashMap<String, Value>`), since `~/.claude/mcp.json` is shared with other
  MCP clients (e.g. Claude Code's own `disabled`/`alwaysAllow` keys) this
  project doesn't model.

`derive_server_id_from_url(url)` — the `ServerId` used for an Http/Sse
config resolved from CLI flags (not `mcp.json`, which uses the config-file
key as the id). Uses **only `host` + `path`** from the parsed URL, never
`userinfo`, structurally excluding embedded credentials from the derived
directory name; lowercased, runs of non-`[a-z0-9-]` collapsed to a single
`-`, trimmed, truncated to `MAX_SERVER_ID_LENGTH`, falls back to
`"http-server"` if the result would be empty (e.g. a bare `https://` with
no host) or the URL fails to parse. This matters beyond cosmetics: the id
becomes a directory name under `~/.claude/servers/{id}/` and is embedded in
generated `.ts` literals, so a raw URL there would break
`mcp-skill::validate_server_id`'s charset, could smuggle a `..` path
segment, and (if not credential-excluded) would leak a token into a
directory name and generated source.

`parse_key_value(s, kind)` — parses a single `--env`/`--header` `KEY=VALUE`
CLI argument. **Never echoes the raw input on failure** in any of its
error paths (no `=` at all; empty key; key containing whitespace/`:`/
control chars — the last case specifically catches a `Name: Value`-style
mistake where the real `=` matched inside a secret value) — because the
whole string, or the "key" half, may itself be the secret.

## 4. Debug-Redaction Discipline

Every CLI-facing type that can carry a secret (`Cli`/`Commands` themselves,
`ServerFlags`, `McpTransport`, `RawMcpServerEntry`, `TransportArgs`) hand-
writes `Debug` rather than deriving it, applying `mcp-core`'s
`RedactedItems`/`RedactedMapValues`/`RedactedUrl`/`sanitize_path_for_error`
consistently — because `runner::report_and_classify` prints
`format!("Error: {}", sanitized_error_report(err))` to stderr on any
failure, and a `Commands` value routinely ends up inside that error's
`anyhow::Context`. Regression
tests assert specific secret substrings (e.g. `sk-verySECRETtoken...`)
never appear in `format!("{:?}", cli.command)` for `--env`/`--header`/
`--http`/`--sse` inputs.

This discipline extends to `tracing` log lines, not just the error path:
`commands::introspect::run` logs the resolved `ServerConfig` under
`--verbose` (INFO level) by formatting `config` itself
(`mcp_execution_core::ServerConfig`, whose own hand-written `Debug` impl
applies the same redaction) rather than `config.transport()`
(`&mcp_execution_core::Transport`). `Transport` is `mcp-core`'s one
secret-bearing type that still derives a plain `Debug` — formatting it
directly, here or anywhere else, reproduces the leak this log line was
fixed for (#336; tracked for a structural fix, redacting `Debug` on
`Transport` itself, in #345). Any new log line touching a `ServerConfig`
must format the `ServerConfig`/`TransportArgs`/`McpTransport` wrapper, never
`Transport` directly.

Neither of the above covers a *dependency's own* log lines (issue #353):
`rmcp::transport::worker` logs an `ERROR` line on connection failure that
formats a `reqwest::Error` whose `Display` embeds the full request URL,
query string included — this project's `Debug` impls never see that text,
since `rmcp` builds it internally. `runner::init_logging` closes this by
wrapping the fmt layer's writer: `fmt::layer().with_writer(|| RedactingWriter(io::stderr()))`,
where `RedactingWriter<W: io::Write>` runs every buffer through
`mcp_execution_core::redact_urls_in_text` before forwarding it to `W`. This
is viable because `tracing-subscriber`'s fmt layer formats each event into a
buffer and issues exactly one `write_all` per event, so `write` always
receives one whole formatted line to scan. This makes the writer
target-agnostic — it covers every dependency's `tracing` output, not just
`rmcp`'s, and survives `rmcp` renaming or relocating the log line that
leaked in the first place.

The same issue's second leak was in this crate's own error path:
`escape_error_text`'s contract has broadened from "neutralize control
characters" to "make error text safe for stderr" — it now redacts embedded
URLs (via `mcp_execution_core::redact_urls_in_text`) *before* truncating,
then sanitizes control characters as before. This one chokepoint covers
`sanitized_error_report`'s whole chain (where `CoreError::ConnectionFailed`'s
boxed `source` — `rmcp`'s error, for an http/sse transport — otherwise
leaked the same URL into the visible `Error:` report) and every `warn!`
call site in `commands::server` that already routed through it, present and
future.

## 5. `runner.rs` — Dispatch and Exit-Code Classification

```rust
pub fn init_logging(verbose: bool, log_format: Option<LogFormat>) -> Result<()>;
pub async fn execute_command(command: Commands, output_format: OutputFormat) -> Result<ExitCode>;
pub fn report_and_classify(err: &anyhow::Error) -> ExitCode;
```

`init_logging`'s `log_format` parameter (issue #399) is `cli.log_format`
verbatim — `None` when `--log-format` was not passed. Two private
module-level functions (mirroring `mcp-execution-server`'s
`resolve_log_format`/`log_format_env_is_invalid`) are extracted out of
`init_logging` rather than inlined, specifically so a test can assert
`MCP_EXECUTION_LOG_FORMAT` is actually consulted — see
`resolve_log_format_reads_env_var_when_flag_unset` — not only that the pure
resolvers they delegate to work given a hand-built input:
- `resolve_log_format(log_format: Option<LogFormat>) -> LogFormat` reads
  `MCP_EXECUTION_LOG_FORMAT` itself (`std::env::var(LOG_FORMAT_ENV_VAR).ok()`)
  and resolves the effective format via the pure, unit-tested
  `mcp_execution_core::cli::LogFormat::resolve(log_format, env_value)`: the
  flag wins unconditionally when set (the environment variable is not even
  inspected in that case); otherwise an empty/whitespace env value is
  treated as unset, a valid one (case-insensitive) is used, and an
  unrecognized one falls back to `LogFormat::Text`. `resolve` returns only
  the resolved format — no caller logs the rejected raw value (an earlier
  design threading it through `resolve`'s return type as `(LogFormat,
  Option<String>)` was reworked away: neither production call site
  consumed it, exactly the "capability designed, caller never wired"
  pattern issue #399 itself targets).
- `log_format_env_is_invalid(log_format: Option<LogFormat>) -> bool`
  independently reads the same environment variable and answers "should
  `init_logging` warn about it", via
  `mcp_execution_core::cli::LogFormat::is_invalid_env_value(raw)`: `false`
  whenever the flag was passed (matching `resolve`'s own precedence — a
  bad env value must not warn once the flag has already decided) or the
  env value is unset/valid, `true` only for a non-empty rejected value.

`init_logging` calls both, then emits a fixed-message `tracing::warn!`
*after* subscriber init (a warn before init would go nowhere) when
`log_format_env_is_invalid` returns `true` — the rejected raw value is
never interpolated into that log line, to avoid a log-injection vector
from external process environment input.

Building the layer itself: `fmt::layer()` and `fmt::layer().json()` are
different types (`Format<Full>` vs. `Format<Json>`), so they cannot share
one binding across an `if`/`match`. `init_logging` instead builds the
writer-configured layer once — `fmt::layer().with_writer(|| RedactingWriter(io::stderr()))`
— then branches only on the terminal `.json()`/no-op call, `.boxed()`-ing
each arm to a common `Box<dyn Layer<_> + Send + Sync>`:

```rust
let fmt_layer = tracing_subscriber::fmt::layer().with_writer(|| RedactingWriter(io::stderr()));
let layer = match format {
    LogFormat::Json => fmt_layer.json().boxed(),
    LogFormat::Text => fmt_layer.boxed(),
};
tracing_subscriber::registry().with(filter).with(layer).init();
```

This proves `RedactingWriter` is present in both arms by construction — the
writer is configured once, before the format branch, so no copy-paste
divergence between two full `registry()...init()` calls can silently drop
it from one arm. `RedactingWriter` itself is formatter-independent (it
wraps the byte sink, not the event formatter), so it applies identically to
text and JSON output — with one caveat: `redact_urls_in_text`'s trailing-
punctuation trim set includes `\` (backslash) specifically so a redacted
URL sitting inside a JSON-escaped string (`serde_json` escaping `"` to
`\"`) does not absorb and delete that escaping backslash, which would
otherwise leave an unescaped `"` and invalid JSON — see
[[../core/spec#redact module|redact_urls_in_text]].

Issue #421: this crate is a *client* of third-party MCP servers
(`mcp_execution_introspector::Introspector`), and `rmcp` 3.1.2's transport layer logs raw,
unsanitized peer input at `debug` level. `RedactingWriter` only rewrites embedded URLs — it does
not neutralize this, so without a cap, `--verbose` alone (`filter = EnvFilter::new("debug")`, no
`RUST_LOG` involved at all) streams an untrusted server's raw stdout lines into stderr. `init_logging`
closes this specific path — *debug-level, raw-line* logging — via a private pure function,
`cap_rmcp_log_level(EnvFilter) -> EnvFilter`, applied to *both* branches of the `verbose` `if`/`else`
— not just the non-verbose branch's `try_from_default_env().unwrap_or_else(...)` fallback, since a
directive folded only into that fallback string would never apply to the verbose branch (which
never calls `try_from_default_env` at all) and is dead code in the non-verbose branch whenever
`RUST_LOG` parses successfully:

```rust
let filter = cap_rmcp_log_level(if verbose {
    EnvFilter::new("debug")
} else {
    EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
});
```

The cap is level-based, not a content filter: `rmcp` also logs a `Debug`-formatted peer
notification at `info` (`service.rs`'s `tracing::info!(?notification, ...)`), which `rmcp=info`
does not and cannot suppress. That site is mitigated (`?` renders via `Debug`, which escapes
control characters, unlike the raw-`Display` `message`-field sites this cap targets), not
eliminated — the full notification content still reaches stderr. Closing it would require
content-level sanitization of `rmcp`'s own event, not a level directive, and is out of scope here.

`cap_rmcp_log_level` adds an `rmcp=info` directive via `EnvFilter::add_directive`.
`tracing_subscriber` orders directives by target specificity, so this directive (more specific
than a bare global `debug`) wins over it. An operator who explicitly sets a *more specific*
directive, e.g. `RUST_LOG=rmcp::transport=debug`, still wins over this one — intentional, since
the goal is closing the accidental broad-`debug` case (including plain `--verbose`), not blocking
a deliberate request for `rmcp` transport debug logs; this is the escape hatch for an operator who
needs one. Specificity is a different axis from level, though: an *equally* specific
`RUST_LOG=rmcp=debug` (same `rmcp` target this cap sets, different level) is not merged with this
cap's `rmcp=info` — `tracing_subscriber`'s `Directive` ordering does not compare level, so
`EnvFilter::add_directive` on a same-target directive *replaces* the existing entry, silently
downgrading an operator's explicit `rmcp=debug` to `info`. Both the escape hatch and the
replace-not-merge behavior are pinned by dedicated tests rather than assumed. Being a pure
function (no `std::env` access), `cap_rmcp_log_level` is unit-tested directly with a scoped
subscriber rather than by mutating `RUST_LOG` in-process (parallel test threads share one
process) — mirroring the `SharedBuf`-based scoped-subscriber pattern `runner.rs`'s
`RedactingWriter` tests already establish.

`execute_command` **never propagates a handler failure as `Err`** — it
always resolves to `Ok(classified_exit_code)` (issue #195's fix, so `main`
can always reach `std::process::exit` with a semantic code instead of
falling back to anyhow's blanket exit-code-1 default). `main.rs` itself
also routes pre-dispatch failures (e.g. clap already rejects an invalid
`--format` before any command runs, but a hypothetical future pre-dispatch
failure) through the same `report_and_classify`.

`classify_exit_code`/`classify_core_error` walk the `anyhow::Error`'s cause
chain (not just the top) looking first for a `mcp_execution_core::Error`,
then (fallback) a `mcp_execution_files::FilesError` — the latter exists
specifically because `generate`'s export step wraps `FilesError` via
`anyhow::Context` rather than converting it to `CoreError`, so it would
otherwise always fall through to the generic `ExitCode::ERROR` (issue #198
M6's fix). `CoreError::ScriptGenerationError`'s wrapped `source` is
recursed into, so e.g. a `ResourceLimitExceeded` wrapped inside it still
classifies as `SERVER_ERROR`, not the generic fallback:

| `Error` variant | `ExitCode` |
|---|---|
| `Timeout` | `TIMEOUT` (4) |
| `ConnectionFailed`, `ResourceLimitExceeded` (core or files) | `SERVER_ERROR` (3) — "the remote MCP server is at fault," not the CLI caller |
| `ValidationError`, `SecurityViolation`, `InvalidArgument` | `INVALID_INPUT` (2) |
| `SerializationError`, everything else | `ERROR` (1) |
| any `FilesError` other than `ResourceLimitExceeded` | `ERROR` (1) |

## 6. `introspect` Command (`commands/introspect.rs`)

`run(source: ServerSource, detailed: bool, output_format: OutputFormat) -> Result<ExitCode>`.
Resolves config via `resolve_server_config`, runs
`Introspector::discover_server` once, formats an `IntrospectionResult`
(`ServerMetadata` + `Vec<ToolDisplay>`, schemas included only when
`detailed`). No output-directory writes at all — read-only, display-only
command.

## 7. `generate` Command (`commands/generate.rs`)

`run(source: ServerSource, name: Option<String>, output_dir: Option<PathBuf>, observed_shapes: Option<PathBuf>, dry_run: bool, output_format) -> Result<ExitCode>`:

1. `resolve_server_config` → `discover_server_info`: if `--name` is given,
   it is validated via `mcp_execution_skill::validate_server_id` **before**
   the connection attempt and only overrides `ServerInfo.id` once valid —
   an invalid `--name` (traversal shape, absolute path, or simply outside
   `validate_server_id`'s `[a-z0-9-]` charset, e.g. `My Server!`) is now a
   hard `INVALID_INPUT` error instead of being silently slugified into a
   different id. **Breaking behavior change** from the pre-#311 CLI, which
   constructed `ServerId::new(custom_name)` directly with no validation.
2. If the server has zero tools: logs a warning and returns
   `ExitCode::SUCCESS` (not an error) without generating anything.
3. `resolve_server_dir_name` turns `server_info.id` into the directory
   name, re-validating it via `validate_server_id` regardless of which arm
   produced the id: `derive_server_id_from_path_or_name` (stdio command),
   `derive_server_id_from_url` (http/sse — see [[#3. common.rs]]), or the
   already-validated `--name` override. This check is a redundant backstop
   for those three arms, but the **sole** enforcement point when the id
   came straight from an unvalidated `--from-config` `mcp.json` key with
   no `--name` override —
   the error message differs accordingly (names `mcp.json` and suggests a
   ready-to-use `--name` slug vs. framing the failure as an internal error
   for the other arms, since reaching it there would mean one of their own
   checks has a bug) (issue #311).
4. `ProgressiveGenerator::generate` (uncategorized — no LLM step in this
   path, unlike `mcp-server`'s `save_categorized_tools`). When `--observed-shapes <FILE>` is given
   (#587), the file is read **before** the connection attempt by `read_observed_tail` and its text
   is passed to `ProgressiveGenerator::with_observed_shapes`
   ([[../codegen/spec#13.8 Observed result shapes (#587)]]). `read_observed_tail` reads **only the last
   4 MiB** (`OBSERVED_TAIL_BYTES`) of the file: when the read starts past offset 0 it drops the first,
   possibly partial, line; bytes that are not valid UTF-8 are replaced (the line parser then rejects
   such a line like any malformed one). A file of any size therefore never fails the command; only an
   unreadable or missing file does (`anyhow` error "failed to read observed shapes from <path>"). The
   flag changes nothing but the result-type hint of tools without an output schema.
5. `resolve_base_dir(output_dir)` — defaults to `~/.claude/servers`.
6. **`--dry-run`**: renders a `DryRunResult` (`FilePreview` per file: path +
   size, human-readable `format_size`) **without writing anything to
   disk** — the only place in this workspace that previews generated
   output without ever touching the filesystem.
7. Otherwise: `FilesBuilder::from_generated_code(code, "/").build()`, then
   `FileSystem::export_to_filesystem_with_options(output_path,
   &ExportOptions::new().with_confine_to(base_dir))` — **not**
   `FilesBuilder::build_and_export`, which treats its target as a
   shared multi-server root; `generate` instead publishes one server's
   whole directory (`output_path = base_dir.join(server_dir_name)`) per
   call, getting `export_to_filesystem_with_options`'s own per-call atomic
   staging/swap (a re-run with fewer tools deletes stale tool files in that
   server's own directory, but never touches sibling servers under the
   same `base_dir`). `with_confine_to(base_dir)` is a second,
   defense-in-depth layer behind the id-sanitization in step 3: a future
   caller that skipped that sanitization fails loudly instead of writing
   outside `base_dir`. See [[../files/spec#5. Atomic Export
   (`export_to_filesystem_with_options`)]] and
   [[../files/spec#8. Cross-Crate Contracts]].
8. Success output names the required post-export step
   (`NPM_INSTALL_HINT`: run `npm install` before type-checking the
   generated package — issue #257's fix, since the generated `package.json`
   declares `@types/node` as a `devDependency` that isn't installed by
   `generate` itself).

`Text`/`Pretty` output (both the success report and the `--dry-run`
preview) escapes the MCP server's handshake-supplied `server_name` via
`formatters::escape_display` before interpolating it into a freeform
`"Server: {name} ({id})"` line — always JSON-quoting the value, even when
it contains no control characters, so a benign name like `Test Server`
renders as `Server: "Test Server" (id)`, not just a malicious one (issue
#299). `Json` output is unaffected, since `serde_json` already escapes
string values.

## 8. `skill` Command (`commands/skill.rs`)

`run(server, servers_dir, output_path, skill_name, hints, overwrite, output_format) -> Result<ExitCode>`:
validates `server` via `mcp_execution_skill::validate_server_id`
(mapped to `CoreError::InvalidArgument` for correct exit-code
classification), resolves the tool directory (default
`~/.claude/servers/{server}`), scans via `scan_tools_directory`, then calls
the private `prepare_skill_context(server, tools, hints, skill_name,
output_path) -> Result<(GenerateSkillResult, PathBuf)>`, and **renders
`SKILL.md` directly** (`render_skill_md`) — no LLM/prompt round-trip, unlike
`mcp-server`'s `generate_skill`/`save_skill` split. The crate's own doc
comment recommends preferring the MCP server path for "optimal results,"
since it can leverage Claude's own summarization instead of the mechanical
template-only rendering this command does. Refuses to overwrite an existing
output file unless `--overwrite`.

`prepare_skill_context` validates a custom `skill_name` (if any) via
`validate_skill_name` up front, then passes it straight into
`build_skill_context` as that function's own `custom_name: Option<&str>`
parameter — not patched onto the result afterward — so `generation_prompt`
reflects a custom name the same way `mcp-server`'s `generate_skill` handler
does (issues #435, #436). It also validates a custom `--output` path (if
supplied) via `validate_output_path`, returning it as `Some(PathBuf)`;
`None` means the caller wants the default path. Resolving the confined
default requires an async filesystem walk this (synchronous) function
cannot perform itself, so `run` resolves it separately.

`run` resolves the actual write path after `prepare_skill_context` returns
(issue #501/#509): a custom `--output` path is used as-is (a CLI operator's
own flag, evaluated with the operator's own filesystem permissions —
confining it further would not defend against anything a symlink swapped
in by a racing local process could not already do); the default path is
resolved via `resolve_default_output_path(skills_dir, server)`, which
confines and creates the `{server}/` segment directory through
`mcp_execution_core::resolve_confined_path`, **rejecting it outright if it
already exists as a symlink**, mirroring `mcp-server`'s `save_skill`
default-path confinement. This has a side effect: the segment directory is
created *before* the `--overwrite` check and before rendering, so a
refused (existing file, no `--overwrite`) or otherwise failed run can leave
an empty `{server}/` directory behind — the same side effect `save_skill`'s
own default-path resolution has. A custom `--output` path's parent
directory, by contrast, is created only *after* the `--overwrite` gate and
rendering succeed.

`write_skill_md` writes `SKILL.md` atomically (write-temp-file then
`rename`). The temp file (`{output}.tmp`, a predictable path) is written
through `mcp_execution_core::write_confined_file`, which opens it with
`O_NOFOLLOW` on Unix, so a symlink pre-planted there is rejected instead of
followed (issue #501/#509) — the same primitive `save_skill` uses for the
equivalent race on its own write step (issue #496). The final
`std::fs::rename` needs no equivalent guard: it replaces whatever directory
entry sits at the final `SKILL.md` path rather than following it, so a
pre-existing symlink there (e.g. a dotfiles setup symlinking `SKILL.md`
into a repo) is safely replaced rather than followed or rejected, on both
the default and a custom `--output` path.

The resolved path is kept separate from `GenerateSkillResult` rather than
written back into its `default_output_path_hint` field: that field is
`build_skill_context`'s own non-authoritative display hint (see
[[../skill/spec#2. Public API Surface]]), and overwriting it here would
reintroduce the same field-reuse-across-semantics pattern issue #436
eliminated from the MCP `generate_skill`/`save_skill` tool pair.

`--hint` values are threaded into `GenerateSkillResult`'s `use_case_hints`
field (sanitized; blank/whitespace-only entries filtered; capped at
`mcp_execution_skill::types::MAX_USE_CASE_HINTS` entries) and render as a
`## Use Cases` bullet list in the written `SKILL.md` — omitted entirely
when no `--hint` is given, preserving byte-identical output for hint-less
runs (issue #481).

## 9. `server` Command (`commands/server.rs`)

Three actions (`ServerAction`), all reading `~/.claude/mcp.json` as the
single source of truth:

- `list` — enumerates every entry, checking a **time-boxed** availability
  signal per entry (`LIST_AVAILABILITY_TIMEOUT` = 3s, deliberately shorter
  than — and independent of — the entry's own configured
  `connect_timeout_secs`): stdio = PATH lookup only; http/sse = URL
  well-formedness + a bounded real `Introspector::discover_server` attempt.
  Checks run concurrently across entries so one slow/firewalled server
  doesn't visibly hang the whole listing. Because http/sse `list` and
  `info`/`validate` now share the **exact same connection path**
  (`Introspector::discover_server`), they can disagree only about *how
  long* the check is allowed to run, not *how* the transport is reached —
  a server merely slower than 3s but within its own configured timeout can
  legitimately show `unavailable` in `list` and `available` in
  `validate`/`info` (an intentional, documented trade-off, distinct from
  the unconditional-wrong-answer bug it replaced).
- `info <server>` / `validate <command>` — perform a **full** introspection
  handshake (the entry's own full configured timeout applies), the
  authoritative single-target check.

`ServerEntry.status` is typed as a closed
`ServerStatus` enum (`Available`/`Unavailable`,
`#[serde(rename_all = "lowercase")]`), not a bare `String` — a **breaking**
type change from the pre-#318 CLI, though it serializes identically
(`"available"`/`"unavailable"`) so `--format json` consumers are
unaffected.

`info`/`validate` distinguish "entry absent from `mcp.json`" from "entry
present but invalid" via `get_mcp_server_entry` (looks up the raw
`McpServerEntry` only) rather than `get_mcp_server` (which also eagerly ran
`build_core_config`'s security validation — see [[#3. common.rs]] —
making the two cases indistinguishable through one `with_context` "not
found" wrapping):

- `server info` on an entry that is present but fails `build_core_config`
  (e.g. an invalid URL scheme) or fails introspection now reports a
  structured `ServerInfo` with `"status": "unavailable"` through the
  normal `output_format` path — not a raw, unformatted `anyhow` error —
  while still returning `ExitCode::ERROR` (issue #305). Only a genuinely
  absent entry propagates as `Err` (and thus a raw error report). The
  `ServerInfo.status` is a `ServerAvailability` enum (`Available`, or `Unavailable { reason }`), so a reason
  cannot exist without the unavailable status and vice versa (#480). It is flattened into the output as
  `"status": "available"`, or `"status": "unavailable"` plus `"reason": {"kind":"invalid_configuration"|"introspection_failed","message":"..."}`,
  whose message went through `escape_error_text`; `ServerEntry` is unchanged.
- `server validate` on a present-but-invalid entry reports the actual
  `build_core_config`/precheck failure message in `ValidationResult` (e.g.
  `"Server '{name}' has an invalid configuration: {e}"`), not the generic
  `"Server not found"` message reserved for a genuinely absent entry (issue
  #304).

`build_command_string` (feeds `list`/`info`/`validate` output, printed
unconditionally, never gated behind `--verbose`) redacts the same way
`ServerConfig`'s own `Debug` impl does (issue #346): stdio `command` is
routed through `sanitize_path_for_error` (home directory/username scrub);
stdio `args` are replaced **wholesale** with
`mcp_execution_core::REDACTED_PLACEHOLDER` per entry, rendered as a
space-joined shell-shaped string (`"docker <redacted> <redacted>"`), since
a single argument routinely holds an entire secret with no key/value half
worth preserving — unlike `mcp_execution_core::RedactedItems`'s
Rust-`Debug`-list rendering, which would be awkward to embed in
`--format json` output; http/sse `url` is redacted via `RedactedUrl`
(strips userinfo credentials and any query string, keeps scheme/host/path
readable), falling back to redacting the whole string if it fails to
parse. `validate_command`'s own "URL is not well-formed" precheck message
(`url_precheck_message`, built *before* `build_command_string` runs) is
redacted the same way, closing a gap where a malformed-but-credentialed
URL could leak via the precheck message even though `build_command_string`
itself was already safe (issue #346, S1).

## 10. `setup` Command (`commands/setup.rs`)

Validates the local runtime is ready to execute generated tools:
1. Checks `node --version` is a version that runs `.ts` files without a flag (hard error if missing or
   older): 22.18.0 or later on the 22 line, 23.6.0 or later on the 23 line, and any later major. Node.js 18 and
   20, 22.6 to 22.17 (which need `--experimental-strip-types`) and 23.0 to 23.5 are rejected, and the error names
   the minimum (`NodeVersion`, #564). The same minimum appears in the README and the usage guide.
2. On Unix only: makes every `.ts` file executable (`files_made_executable`
   count) anywhere under `~/.claude/servers/`, at any depth — the walk
   recurses into nested subdirectories (e.g. `{server-id}/_runtime/`) rather
   than stopping at the server-id level — skipping symlinked entries rather
   than following them (`skipped_entries` count), and reports whether a
   servers directory exists at all.
3. Reports whether `~/.claude/mcp.json` exists, printing a starter example
   if not.

Non-Unix platforms always report `servers_dir_found: false`,
`files_made_executable: 0`, `skipped_entries: 0` (permission bits aren't a
concept there).

**Symlink rejection** (issue #476, extended to arbitrary depth by #489): the
walk is confined to `~/.claude/servers/` by rejecting symlinked entries
outright rather than resolving and re-checking them, at every recursion
level — mirroring the guard semantics of
`mcp_execution_core::confinement::resolve_confined_path` (not reused
directly, since that function creates directories for caller-supplied paths
and this walk enumerates existing entries and must create nothing):
- A symlinked server-id entry, or a symlinked intermediate subdirectory at
  any depth, is skipped and never descended into (would otherwise let the
  walk `chmod` into an arbitrary directory elsewhere on disk).
- A symlinked `.ts` file, at any depth, is skipped (would otherwise `chmod`
  its target anywhere the process can reach).

The symlink check runs before the `.ts` extension filter, so a symlinked
entry that is *not* a `.ts` file (a symlinked `README.md`, a symlinked
`_meta.json`, etc.) is also skipped and counted in `skipped_entries` with a
warning, even though the walk would never have touched it otherwise. This
ordering is required for directories (their extension is irrelevant to
whether they should be descended into) and is applied uniformly to files too
for one consistent rule, rather than special-casing "symlinked but harmless."

Because a symlinked directory is never descended into, the recursion cannot
cycle back to an ancestor through a symlink; ordinary (non-symlink)
directories cannot form cycles on their own, so the walk is bounded by the
real directory depth.

Entry kind is checked via `DirEntry::file_type()` (does not traverse
symlinks), and each skipped entry increments `SetupResult::skipped_entries`
and emits a `tracing::warn!` with the sanitized path, so a planted symlink is
surfaced to the user instead of silently ignored. This is a check against
pre-existing state, not a concurrency guarantee: it does not defend against a
symlink planted by a racing process between the kind check and the
subsequent `set_permissions` call (same non-guarantee documented for
`resolve_confined_path`), nor against a hardlink inside the servers tree
pointing outside it, which `file_type()` cannot distinguish from a regular
file — both are accepted as out of scope.

**Error tolerance during the walk** (issue #490): below the root, a directory
that fails to open, or a read error encountered mid-iteration over its
entries (e.g. a transient I/O error from `next_entry()`), is logged as a
`tracing::warn!` and treated as exhausted rather than aborting the command —
the walk resumes with the next sibling entry instead of propagating the
error via `?`. This applies uniformly at every recursion depth below the
root, not only to the server-id level. The same skip-and-warn rule covers a
single entry's failures (#503): an entry whose `file_type()` fails is skipped, and a `.ts` file whose
`metadata()` or `set_permissions()` call fails (`make_executable`) is skipped with a warning and not counted in
`files_made_executable`, so one problematic file never aborts the walk.

The walk's root (`~/.claude/servers/` itself) is the one exception: failing
to *open* the root is still fatal and propagates as an error. Unlike a
nested directory, the root has no sibling directories whose processing needs
protecting, so tolerating a root-open failure would only convert a real
error (e.g. the servers directory exists but lost its permissions, or was
replaced by a non-directory) into a silent `Ok((true, 0, 0))` — a `setup`
run that reports success while having done nothing. A read error
encountered *mid-iteration* over the root's own entries, by contrast, is
still skip-and-warn like everywhere else: any `chmod`s already applied
before the failure are on disk and stand regardless of what the function
returns, and a read error after a directory has already been opened
successfully is transient enough (e.g. a stale NFS handle) that best-effort
completion of the walk is preferred over failing the whole command over it.

The core walk is exposed as `check_files_executable_in(servers_dir: &Path)`
for testing without `HOME` mutation; the public, no-argument
`check_files_executable()` resolves `~/.claude/servers/` and delegates to it.
`check_files_executable_in` opens the root directory itself (propagating on
failure) and hands the resulting iterator to `walk_entries`, the shared
per-entry loop also used — via the `walk_and_chmod` wrapper, which supplies
the skip-and-warn open — for every recursive (non-root) directory.

## 11. `completions` Command (`commands/completions.rs`)

`generate_completions(shell, cmd)` — thin wrapper over `clap_complete::generate`,
writing the script to stdout. Cannot fail (always returns
`ExitCode::SUCCESS`); all error handling is internal to `clap_complete`.

## 12. Output Formatting (`formatters.rs`)

```rust
pub fn format_output<T: Serialize>(data: &T, format: OutputFormat) -> Result<String>;
pub fn emit<T: Serialize>(data: &T, format: OutputFormat, exit_code: ExitCode) -> Result<ExitCode>;
pub fn escape_display(s: &str) -> String; // wraps s as a JSON string literal (quotes + backslash-escapes, including control chars)
pub mod json; pub mod text; pub mod pretty;
```
`escape_display` exists for commands that build **freeform** lines (e.g.
`"Server: {name} ({id})"`) rather than serializing a whole struct through
`format_output` — a malicious MCP server could otherwise inject raw
ANSI/control escape sequences into the user's terminal via handshake or
tool-metadata text. `pretty`'s own internal value formatter delegates to
this same function for `String` values, so both call sites share one
implementation and one guarantee.

`emit` collapses the format/print/return-exit-code sequence every command
handler branch repeated (`introspect`, `server`, `setup`, `skill`) into one
call site: it formats `data` per `format`, prints it to stdout, and returns
the caller-supplied `exit_code` unchanged (issue #368/#375). `generate`
does not use it, since its `--dry-run`/success paths build freeform
`Text`/`Pretty` lines via `escape_display` rather than serializing one
struct through `format_output`.

## 13. Error Conditions

CLI commands surface errors as `anyhow::Error` (wrapping
`mcp_execution_core::Error`/`mcp_execution_files::FilesError` via `?`/
`.context(...)`), classified at the `runner` layer per [[#5. runner.rs]]
into a process exit code — no command handler calls `std::process::exit`
itself.

## 14. Cross-Crate Contracts

- **Consumes**: `mcp-core` (`ServerConfig`, `cli::{OutputFormat,ExitCode}`,
  redaction helpers, `Error`), `mcp-introspector::Introspector`,
  `mcp-codegen::progressive::ProgressiveGenerator`,
  `mcp-files::FilesBuilder`, `mcp-skill::{scan_tools_directory,
  build_skill_context, render_skill_md, validate_server_id,
  MAX_SERVER_ID_LENGTH}`.
- Shares the exact `ServerFlags`/`ServerSource`/`resolve_server_config` code
  path between `introspect` and `generate` — any transport-resolution fix
  applies to both simultaneously by construction.

## 15. Edge Cases & Notable Behaviors

- `generate` on a tool-less server is a **success**, not an error — a
  deliberate "nothing to do" outcome, distinct from every failure path.
- `--dry-run` and `server list` are the only two places in the CLI that
  intentionally avoid a real filesystem write / a full-timeout network
  call, respectively, in favor of a fast, bounded preview.
- `mcp.json`'s `McpServerEntry` deserialization warns (not fails) on
  unrecognized keys, so this project's CLI can coexist with a config file
  shared by other MCP-aware tools.

## 16. Planned Changes (script-first direction, adopted 2026-11-24)

> [!warning] Not implemented
> Nothing in this section exists in the code yet. Each subsection names its tracking issue.
> When a change lands, its content moves into the as-built sections above and the subsection is
> removed in the same pull request.

Command, flag and subcommand names below (`stats`, `run`, `call`, `recipe`, `--save`, the drift command) are
working names; choosing them is an open decision of each subsection. The `skill` command also changes through
the script-first skill package (default name, name validation, body shape); that content lives in
[[../skill/spec#12. Planned Changes (script-first direction, adopted 2026-11-24)]] and is not repeated here.

### Planned: context-cost measurement command (#562)

**Why.** The project's headline number has no measurement behind it. `README.md:10`, `:23` and `:133` claim 98%
token savings; the business requirements document records the disagreement with the later "~83% maximum"
note as an open question, and the performance requirement on tokens per tool is stated as not an automated
measurement. Before #568 `ToolMetadata` recorded no size for the server's own definition of a tool, so once
generation was over that size could not be recovered. Since #568 (`METADATA_SCHEMA_VERSION` 3) it
records `manifest_size_bytes` (the compact serialized `ToolInfo`) and `result_type_origin` (the
output-schema flag), which the static report reads. A single
"savings" percentage also misleads, because it depends on which files a model reads and on what the host already
defers.

**Requirements.**

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-046.10 | WHEN the user runs the measurement command on a generated directory THE SYSTEM SHALL print, for each discovery level (the server's own definitions as recorded, all tool files, one tool file, and the catalog once it exists), the size in bytes and the estimated tokens, the name of the estimator, and the number and share of tools that declare an output schema | must |
| FR-046.11 | WHEN a quantity is not available (catalog not generated, size absent from an older directory) THE SYSTEM SHALL print "not available" and SHALL NOT print zero or an estimate | must |
| FR-046.12 | WHEN the directory carries an older metadata schema version THE SYSTEM SHALL exit non-zero, state that the directory must be regenerated and print no figures, as the skill scan does for an unsupported schema (`ScanError::UnsupportedSchema`, `crates/mcp-skill/src/parser.rs:95`) | must |
| FR-046.13 | THE SYSTEM SHALL provide a deterministic scenario harness that compares, for K calls in a bounded list that always includes 1 and 2, the bytes reaching the context when the K calls are made one per shell command and when one script makes them, against an in-repo mock server, and reports the K at which the two arms cross | must |
| FR-046.14 | THE SYSTEM SHALL run the harness with no network and no model, in CI | must |
| FR-046.15 | THE SYSTEM SHALL NOT label any output "savings"; a ratio MAY be shown only between two named quantities that are both shown | should |
| FR-046.16 | THE SYSTEM SHALL state in the command help and in the report header what each measurement proves and does not prove | should |

**What each measurement proves.**

| Measurement | Proves | Does not prove |
|-------------|--------|----------------|
| Static report | The size of the text a model would read if it read that level in full; how that size splits between fixed boilerplate and tool-specific content; how many tools declare an output schema | That a model did read it; savings against a host that already defers definitions; the success of any task; true token counts |
| Scenario harness | For the defined scenarios, the bytes that reach the context under each arm, deterministically, and where scripting stops paying off | The cost of writing the script (model-authored in reality); turn overhead and prompt caching; correctness; behaviour with real servers or tokenizers |

**Constraints.**

- [[../constitution#V. Security]]: the command reads a bounded set of files, with count and bytes derived from
  the generation and export bounds (`MAX_GENERATED_FILES`, `MAX_GENERATED_BYTES`, `MAX_EXPORT_FILES`,
  `MAX_EXPORT_BYTES`), not an independent number; a directory beyond the bound is rejected, not truncated.
  Tool names and text read from generated files are untrusted: the report prints names, sizes and counts only,
  never file contents, sanitised through `mcp_execution_core::untrusted` and the output rules of §12.
- Writes nothing; any later output file is confined with symlink-aware validation.
- Strong types: sizes and token estimates are distinct newtypes, the estimator is an enum; the same input and
  version give byte-identical output (no timestamps, stable order).
- The per-tool size field and the output-schema flag are fields of the single `_meta.json` v3 registry owned by
  [[../core/spec]]; this subsection defines no metadata field and never bumps the version on its own.

**Open decisions.**

- [NEEDS CLARIFICATION: command name and placement (working name `stats`), and whether the harness is a subcommand, a test target or a benchmark.]
- [NEEDS CLARIFICATION: how a tool definition is serialised for sizing (canonical JSON of the tool object, with or without annotations and output schema; before or after the introspector's bounds).]
- [NEEDS CLARIFICATION: the token estimator and its documented error band; recorded in a decision record.]
- [NEEDS CLARIFICATION: whether `index.ts` and the runtime files count in "all tool files"; where the mock servers live so this crate can use them; whether CI asserts exact figures or relations and bounds.]

Ask first: choosing the estimator; a tokenizer dependency (check current versions); any machine-readable output
format or output file.

**Depends on / blocks.** The static report is blocked by #568 (per-tool size and the output-schema flag in the
`_meta.json` v3 registry); the scenario harness is not. Consumed by the documentation rewrite (#560), which may
quote only figures this command prints. Tracked under epic #555.

### Planned: command-line front doors for the script runner (#571) and the one-shot tool call (#574)

The behaviour of both commands (execution, output budget and spill, notices, the connection path, result
semantics) is specified in [[../runtime/spec#4. Planned: Script Runner With Bounded Output (#571)]] and
[[../runtime/spec#7. Planned: One-Shot Tool Call Through the Authoritative Client (#574)]]. This subsection holds
only what belongs to the command-line layer.

**Why.** The subcommands today are `introspect`, `generate`, `skill`, `server`, `setup` and `completions` (§2);
none runs a script or calls a tool, and the exit-code classification (§5) has no class for a script's own exit
status. `introspect` resolves a server through `resolve_server_config` and `Introspector::discover_server`
(`crates/mcp-cli/src/commands/introspect.rs:185-203`); a one-shot call reuses that resolution instead of adding
a second one.

**Requirements.**

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-036.20 | WHEN the run command is invoked THE SYSTEM SHALL take the script from a file or from standard input, take the time limit and output budget as options bounded by a hard maximum, and pass script arguments to the script as data, never interpolated into a command line | must |
| FR-036.21 | WHEN the runner itself fails (unreadable script, invalid option, missing runtime, unusable spill location, timeout) THE SYSTEM SHALL exit with the classification of §5 (invalid input 2, timeout 4, other 1) and a one-line message that does not echo script text; WHEN the script exits non-zero by itself THE SYSTEM SHALL propagate its status without remapping it onto the classified codes | must |
| FR-036.22 | THE SYSTEM SHALL give every new type that carries script text, script arguments or environment values a hand-written redacting `Debug`, and the `Commands` value SHALL never print them (§4) | must |
| FR-039.20 | WHEN the call command is invoked THE SYSTEM SHALL select the server with the same selectors as `introspect` (`ServerFlags`, `ServerSource`, §2, §3), take the arguments as a single JSON object, and validate the server as a `ServerId` and the tool as a `ToolName` before connecting | must |
| FR-039.21 | WHEN the JSON argument is malformed or is not a single object THE SYSTEM SHALL exit with invalid input and report the failure without repeating any part of the argument | must |
| FR-039.22 | THE SYSTEM SHALL map call failures onto §5 (connection failure and resource limit 3, timeout 4, invalid input 2) and state the class of a tool-reported error | must |
| FR-039.23 | WHEN the call prints a result or an error THE SYSTEM SHALL apply the global `--format`, sanitise and bound server text in error output and mark it as coming from the server, and build any freeform line from untrusted text through `escape_display` (§12) | must |

**Constraints.**

- The call command is a client of the shared connect step that the introspector owns; this crate owns no
  connection code ([[../runtime/spec#7. Planned: One-Shot Tool Call Through the Authoritative Client (#574)]]).
- `execute_command` keeps resolving to a classified exit code and never propagates a handler failure as `Err`
  (§5); `main.rs` still calls `std::process::exit` only after classification.
- Help text of the run command states that scripts run with the user's privileges and without a sandbox.

**Open decisions.**

- [NEEDS CLARIFICATION: command names, flags, and how a file or standard input is chosen (working names `run`, `call`).]
- [NEEDS CLARIFICATION: exit code of a tool-reported error (3 or 1), and whether the runner's classified tool failures map onto 3 and 4.]
- [NEEDS CLARIFICATION: whether results printed to the caller fall under the constitution's untrusted-text rule or are an accepted exception; a maintainer decision stated here and in the runtime block, with neither sanitisation nor an exception claimed until it is made.]
- [NEEDS CLARIFICATION: how the global `--format` interacts with runner notices.]

Ask first: the command names and flags; any change to `ExitCode` or to the classification table; starting the
one-shot call before the runtime decision record is accepted.

**Depends on / blocks.** The run command (#571) is not gated and builds on #570. The one-shot call (#574) is gated
by the runtime decision record #569 ([[../decisions/ADR-569-runtime-strategy-single-mcp-client]]): if the
generated bridge stays the only client, a Rust call path would be a second way to call tools. #574 also depends
on the shared connect step in [[../introspector/spec]]. Tracked under epic #557.

### Planned: saved-script commands (#579)

The storage layout, survival across regeneration, header validation and bounds are specified in
[[../skill/spec#12. Planned Changes (script-first direction, adopted 2026-11-24)]]; this subsection holds the
command-line side.

**Why.** Nothing in the CLI saves, lists or checks a script. The `skill` command writes `SKILL.md` through a
temporary file and `rename` (`commands/skill.rs:336-352`) into a segment directory it creates and never replaces
(`:376-389`), whereas `generate` replaces the whole server directory (`commands/generate.rs:441`, §7 step 7);
saved scripts therefore belong beside `SKILL.md`, and the commands that touch them must respect that.

**Requirements.**

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-044.14 | WHEN the user lists saved scripts THE SYSTEM SHALL print each signature (name, one-line description, declared inputs, referenced tools) and no script body, with header text sanitised as untrusted and the listing bounded | must |
| FR-044.15 | WHEN the user checks saved scripts THE SYSTEM SHALL report per script an invalid header, an over-bound file and each referenced tool absent from the generated server's `_meta.json`, SHALL NOT connect to any server, and SHALL NOT modify or delete anything | must |
| FR-044.16 | WHEN the check finds a problem THE SYSTEM SHALL exit non-zero with a code distinct from usage and I/O errors | must |
| FR-044.17 | WHEN the run command is given a save option THE SYSTEM SHALL save exactly the executed text under an explicit script name and header, refuse an existing name unless overwrite is requested, and write nothing when the header or name is invalid | should |
| FR-044.18 | THE SYSTEM SHALL NOT execute a saved script as a side effect of saving, listing or checking | must |

**Constraints.**

- [[../constitution#V. Security]]: writes go through the confined no-follow primitives of §8; script bodies and
  parser source excerpts never appear in output, errors or `Debug`; output text follows §12.
- `thiserror` in `mcp-skill`, `anyhow` only here (§13).

**Open decisions.**

- [NEEDS CLARIFICATION: subcommand and flag names (working names `recipe list`, `recipe check`, `--save`).]
- [NEEDS CLARIFICATION: the exit code for "problems found": a new `ExitCode` constant or a documented use of an existing one.]
- [NEEDS CLARIFICATION: whether saving happens only after a successful run, and how it behaves after a partial failure ([[../runtime/spec#5. Planned: Call Journal and Partial-Failure Report (#572)]]).]
- [NEEDS CLARIFICATION: a check of a skill whose server directory is missing or stale: reported as unreadable, distinct from a missing tool.]

Ask first: adding an `ExitCode` constant; the recipe header format (decision record, see the skill block).

**Depends on / blocks.** Depends on the script runner (#571) for the save hook and on the skill-block storage
rules. Tracked under epic #558.

### Planned: drift check against recorded provenance (#580)

**Why.** Every `generate` run records provenance in `_meta.json` and nothing reads it back: `tool_digest` and
`config_fingerprint` are written by `crates/mcp-codegen/src/progressive/generator.rs:866-876` and referenced
otherwise only by tests. The question provenance exists to answer, whether the server's exposed surface or the
endpoint changed since generation, is unmet, so a server that adds, removes or rewords a tool is invisible
until a script misbehaves. The record is one aggregate digest, not one per tool (`GenerationProvenance`,
`crates/mcp-core/src/provenance.rs:650-657`), so exact attribution of a changed tool needs a per-tool digest
recorded at generation. The locked `rmcp` 3.10.0 carries `ttl_ms` and `cache_scope` on the tools listing result
(`rmcp-3.10.0/src/model.rs:1920-1936`), but the introspector keeps only `page.tools` and `page.next_cursor`
(`crates/mcp-introspector/src/lib.rs:738-744`) and no code in `crates/` reads either field.

**Requirements.**

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-045.10 | WHEN the check runs THE SYSTEM SHALL resolve the server as `generate` does (`ServerSource`, §3) and obtain the live tool list through `Introspector::discover_server`, adding no second connection path, and SHALL derive the generated directory as `generate` derives it (§7 step 3) | must |
| FR-045.11 | WHEN the check compares THE SYSTEM SHALL compute the live digest with the one mapping and function `generate` uses, and WHEN the config fingerprint differs from the recorded one SHALL report that the endpoint differs rather than present a tool difference as the server changing | must |
| FR-045.12 | WHEN the digests differ THE SYSTEM SHALL name the tools present only in the live list and only in `_meta.json`; WHEN the names are identical but the digests differ and no per-tool digests are recorded THE SYSTEM SHALL report that the tool list differs and that which tool changed cannot be determined from the recorded data | must |
| FR-045.13 | WHERE per-tool digests are recorded THE SYSTEM SHALL also name tools present on both sides whose digest differs, and a change confined to tool annotations SHALL be reported as drift once the digest decision of the `_meta.json` v3 registry allows it | should |
| FR-045.14 | THE SYSTEM SHALL exit with distinct codes usable in CI for no drift, drift detected, and check could not complete (connection, timeout, unreadable or unsupported sidecar, invalid input) | must |
| FR-045.15 | THE SYSTEM SHALL phrase every message as "the server changed" or "the tool list differs from the one recorded at generation", never "your files are out of date byte for byte" ([[../core/spec]], provenance: what provenance does and does not answer) | must |
| FR-045.16 | THE SYSTEM SHALL always issue the live request, SHALL report `ttl_ms` and `cache_scope` when the listing carries them, SHALL state that the list may depend on credentials when the scope is not public, and SHALL NOT use `ttl_ms` to skip the request | must |
| FR-045.17 | WHEN printing tool or script names THE SYSTEM SHALL sanitise them as untrusted text, bound the number printed with an "and N more" count, and SHALL NOT print descriptions or schema text of changed tools; THE SYSTEM SHALL write no file, `_meta.json` included | must |
| FR-045.18 | WHERE saved scripts exist THE SYSTEM SHOULD list, by name only, those that reference a removed or changed tool | should |

**Constraints.**

- [[../constitution#V. Security]]: names come from an untrusted server and from model-written files; the
  fingerprint and digests are printed as hex only and config values never; redacting `Debug` on config types
  is unchanged.
- The outcome is a closed enum (no drift, endpoint differs, drift with added, removed, changed and undetermined),
  not strings or booleans; same inputs give identical output and ordering.
- The tool-to-digest mapping exists once and is shared with `generate`; it is private to codegen today, so
  where it lives is open and decided with the registry in [[../core/spec]].

**Open decisions.**

- [NEEDS CLARIFICATION: record per-tool digests in the `_meta.json` v3 registry (makes "changed" exact) or accept name-level reporting only.]
- [NEEDS CLARIFICATION: exit code values and names (existing: success 0, error 1, invalid input 2, server error 3, timeout 4).]
- [NEEDS CLARIFICATION: command name and placement, a new subcommand or a flag on an existing one.]
- [NEEDS CLARIFICATION: whether honouring `ttlMs` means more than reporting it.]

Ask first: changing `ServerInfo` public fields to carry `ttl_ms` and `cache_scope`; recording per-tool digests;
the exit code values.

**Depends on / blocks.** Needs the exposure of the cache hints in [[../introspector/spec]] and, for exact
attribution, the per-tool digest decision in the `_meta.json` v3 registry owned by #568. Independent of the
runtime decision record. Tracked under epic #558.

## 17. See Also

- [[../core/spec]] — shared config/redaction/exit-code primitives
- [[../introspector/spec]], [[../codegen/spec]], [[../files/spec]], [[../skill/spec]] — crates this CLI drives directly, without going through [[../server/spec]]
- [[#16. Planned Changes (script-first direction, adopted 2026-11-24)]] — measurement, runner and call front doors, saved scripts, drift check; not implemented
- [[../runtime/spec]] — behaviour behind the runner and call commands (planned)
- [[../decisions/ADR-569-runtime-strategy-single-mcp-client]] — gates the one-shot call
