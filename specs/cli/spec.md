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

### Shared directories and the metadata reader (#618)

```rust
// `ServersDir` and `SkillsDir` live in `mcp-execution-core` (`home` module) and are re-exported by `commands::common`:
// resolve(Option<PathBuf>) -> Result<Self, HomeDirError>; server(&ServerId) / skill(&ServerId) -> PathBuf; as_path()
pub fn read_generated_metadata(dir: &Path, server: &ServerId) -> Result<ServerMetadata>;
pub async fn read_generated_metadata_async(dir: PathBuf, server: &ServerId) -> Result<ServerMetadata>;
```

- `ServersDir::resolve` and `SkillsDir::resolve` are the only way a command resolves a directory option. `None` gives
  `~/.claude/servers` or `~/.claude/skills`; `Some` gives the path unchanged, except that a leading `~` component (`~`, `~/x`) is expanded
  against the home directory through the core `expand_home` (so `--servers-dir '~/custom'` and `--servers-dir '~'` work
  when the shell did not expand them; #637). A first component `~name` (anything starting with `~` that is not exactly `~`) is rejected with `HomeDirError::UnsupportedUserHome`, whose text never echoes the path ("a '~name' path (another user's home directory) is not supported; use '~/...' or an absolute path, or './~name' for a directory with that literal name"), `./~name` passes `expand_home` unchanged, and which the runner classifies as `INVALID_INPUT` for the directory options the CLI expands itself (exit 2; `Unknown` and `NotAbsolute` keep exit 1, #647). Home comes from the core
  `HomeDir` (core spec, `home` module), never from `dirs`; when it is needed and unknown or relative the command fails
  with the `HomeDirError` text ("cannot determine the home directory" or "the home directory '<p>' is not an absolute
  path"). `resolve_base_dir` and `read_recorded` no longer exist. The `mcp.json` path is built by one `mcp_config_path()`
  helper in `commands::common` (also used by `setup`).
- Users: `generate` (`--output-dir`, `--progressive-output`), `skill` (`--servers-dir`, the default skills directory),
  `stats` (`--servers-dir`), `scripts list` and `scripts check` (`--skills-dir`; `check` also `--servers-dir`), `drift`
  (the skills directory of `scripts_using`), `run` (`--servers-dir` and the `--save` skill directory), and `setup`. `ServerId` rejects separators and `..`, so `server()` and
  `skill()` cannot leave the base directory.
- `read_generated_metadata[_async]` wraps the single core reader ([[../core/spec#Reader (`read_server_metadata`, #618)]])
  for `drift`, `scripts check`, `stats` and the dry-run policy suggestions. `skill` reaches the same reader through
  `mcp-skill`'s scan ([[../skill/spec]]), so it refuses the same sidecars but keeps `ScanError`'s messages and exit 1 for a
  schema mismatch. Mapping: `NotFound` is "Metadata not found for
  '<server>'. Run 'mcp-execution-cli generate' for this server first." (exit 1); `SchemaMismatch` is
  `CoreError::InvalidArgument` naming both versions and asking to regenerate (exit 2); every other refusal (a symlinked or
  non-regular sidecar, over 1 MiB, malformed, unreadable) is the reader's message with the context "Cannot use
  `_meta.json` of '<server>'" (exit 1). A symlinked `_meta.json` is refused everywhere, including when its target lies
  inside the directory.

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
closes this specific path — *debug-level, raw-line* logging — via the shared pure function
`mcp_execution_core::log_filter::cap_rmcp_log_level(EnvFilter, LevelFilter) -> EnvFilter` (#608; one copy for `mcp-cli` and
`mcp-server`, see [[../core/spec#`log_filter` module (`src/log_filter.rs`, feature `tracing-filter`, #608)]]), applied to *both* branches of the `verbose` `if`/`else`
— not just the non-verbose branch's `try_from_default_env().unwrap_or_else(...)` fallback, since a
directive folded only into that fallback string would never apply to the verbose branch (which
never calls `try_from_default_env` at all) and is dead code in the non-verbose branch whenever
`RUST_LOG` parses successfully:

```rust
let filter = cap_rmcp_log_level(
    if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    },
    LevelFilter::INFO,
);
```

The cap is level-based, not a content filter: `rmcp` also logs a `Debug`-formatted peer
notification at `info` (`service.rs`'s `tracing::info!(?notification, ...)`), which `rmcp=info`
does not and cannot suppress. That site is mitigated (`?` renders via `Debug`, which escapes
control characters, unlike the raw-`Display` `message`-field sites this cap targets), not
eliminated — the full notification content still reaches stderr. Closing it would require
content-level sanitization of `rmcp`'s own event, not a level directive, and is out of scope here.

`cap_rmcp_log_level` adds an `rmcp=<cap>` directive (`info` here; `warn` for the quiet profile) via `EnvFilter::add_directive`.
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
| `FilesError::HomeDirectory` (a `HomeDirError` source, #657) | `INVALID_INPUT` (2) for `UnsupportedUserHome`; `ERROR` (1) for `Unknown` and `NotAbsolute`, as for a bare `HomeDirError` (shared `classify_home_error`) |
| any other `FilesError` other than `ResourceLimitExceeded` | `ERROR` (1) |

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
5. `ServersDir::resolve(output_dir)` (section 3, replaces `resolve_base_dir`, #618) — defaults to `~/.claude/servers`; a leading `~` component is expanded (a bare `~` included).
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
`SkillName::new` up front, then passes it straight into
`build_skill_context` as that function's own `custom_name: Option<&SkillName>`
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
   than following them (`skipped_entries` count), counting every entry it
   could not process (`failed_entries`, below), and reports whether a
   servers directory exists at all. The directory is resolved through the
   shared `ServersDir::resolve(None)` (section 3).
3. Reports whether `~/.claude/mcp.json` exists, printing a starter example
   if not.

Non-Unix platforms always report `servers_dir_found: false`,
`files_made_executable: 0`, `skipped_entries: 0`, `failed_entries: 0` (permission bits aren't a
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

**No silent partial success** (#610): below the root, each failure above that leaves part of the tree untouched also
increments `SetupResult::failed_entries` (JSON field; Unix only, 0 elsewhere), once per event, in addition to the warning:
(a) an entry whose `file_type()` fails; (b) a `.ts` file whose `make_executable` fails (`metadata()` or
`set_permissions()`); (c) a subdirectory whose `read_dir` fails (the whole subtree is unlisted, counted once); (d) a
`next_entry()` error mid-directory (the rest of that directory is unread, counted once, the directory is then treated as
exhausted). `skipped_entries` stays symlink-only. The pretty summary adds `Could not make N entr(y|ies) executable (see
warnings above)` and closes with `Runtime setup finished with warnings` instead of `Runtime setup complete` when
`failed_entries > 0`. The exit code stays 0 (`ExitCode::SUCCESS`): a skipped symlink is already a warning-class condition
at exit 0 in this CLI, and a failed entry is reported in the output rather than by the exit code. Internally the walk
returns a private `WalkReport { made_executable, skipped_entries, failed_entries }` (`None` when there is no servers
directory), replacing the earlier tuple.

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
`check_files_executable_in` (returns `Result<Option<WalkReport>>`) opens the root directory itself (propagating on
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

## 16. Script-First Commands (as built)

### `call` (#574, `commands/call.rs`)

- Syntax: `call <ServerFlags> -- TOOL [JSON]` (clap `last = true`), for example
  `call --from-config github -- create_issue '{"title":"x"}'` and `call --http URL -- echo`. `ServerFlags` is
  unchanged, so `introspect`, `generate` and `drift` keep their positional server.
- Flow: the tool name is validated as `ToolName` and the argument as a single JSON object before the server config is
  resolved (exit 2; the error carries only the JSON error class and line and column, never argument text); then
  `Introspector::connect`, one `Session::call_tool` with `CALL_TIMEOUT` 60 s, then `close`.
- Output: result within the 16 KiB budget, over-budget output spilled with a notice naming path and byte count. A
  tool-reported or JSON-RPC error is one stderr line and exit 3; an empty success prints nothing and exits 0. Exit
  codes otherwise come from the classifier (invalid input 2, connection failure 3, timeout 4).
- `call` uses `ResultSource::Structured` (structured content if present, else content); it does not use the helper
  protocol. `CallArgs` has a redacting `Debug`.
- Print form (fixed in wave 2, architect ruling): every value, strings included, is printed as compact JSON through
  `serde_json::to_string` (a string is quoted); `--format` is ignored. This matches the generated wrapper's
  `JSON.stringify`. The earlier raw-string form is withdrawn. `DEFAULT_CALL_TIMEOUT` (60 s) now lives in
  `runtime/helper.rs` and is shared by `call`, `helper` and `run`. The wave-1 raw-string form was rejected by the architect.

### `run` (#571, `commands/run.rs`, `runtime/{exec,notice,import_hook}.rs`)

- Flow: validate flags and script, resolve node (version check), prune old run dirs, print the token anchor on stdout,
  start node with `--import <data:...> --disable-warning=ExperimentalWarning <abs script> <args...>`, capture stdout
  and stderr with async readers (capped at `MAX_SPILL_FILE_BYTES`, 8 MiB per stream), apply budgets, spill the rest,
  then print head, notices and trailer, or one JSON envelope. Text output is buffered, not streamed.
- Preload (`runtime/preload.js`, `runtime/helpers.js`): `module.registerHooks` resolves `mcp:<id>` to
  `<servers>/<id>/index.ts` and provides a virtual `mcp:helpers` with `paginate` and `mapBounded`, each returning
  `{items|results, truncated}`. `MCP_EXECUTION_HELPER*` is removed from the child environment. Script stdin is the
  null device in wave 1.
- Script source: a file runs in place by absolute path; missing, non-file or over `MAX_SCRIPT_BYTES` (1 MiB) files
  are invalid input (exit 2, one stderr line, node not started). A stdin script is written to `runs/<id>/script.ts`
  (0600 in a 0700 dir) and removed after the run.
- Constants: `DRAIN_GRACE` 2 s; timeout 1..=600 s, default 60 s; the script runs through `ProcessTree` (below), which kills on timeout, cancel and drop.
- Exit mapping: script exit 0 gives 0; non-zero exit or signal gives 6 (`SCRIPT_FAILED`); runner timeout 4; runner
  input errors 2; other runner failures 1; the runner stopped by SIGINT gives 130 (`INTERRUPTED`) and by SIGTERM 143
  (`TERMINATED`) (#627); on Unix SIGHUP gives 129 (`HANGUP`) and SIGQUIT 131 (`QUIT`) (#646).
- Signals (#627, `runtime/signal.rs`, `commands/run.rs`, `runtime/exec.rs`): `Termination::install` registers SIGINT, SIGTERM,
  SIGHUP and SIGQUIT on Unix (`RunnerSignal::{Interrupt, Terminate, Hangup, Quit}`), and Ctrl-C and Ctrl-Break on Windows (both report as SIGINT); a failed registration logs a warning and
  that source stays silent. The handle is installed right after `refuse_inside_run` and is raced against every phase:
  reading a stdin script and writing its snapshot, the type check, a pre-spawn check, and the script process. The first
  signal wins, and cancellation is checked **before** the script's own exit status (`biased` select), so a script that traps
  SIGINT and exits 0 still gives 130 when the signal is delivered to the runner (a `kill -INT <runner>`, or a terminal Ctrl-C
  off a terminal-owned run; the script is in its own group on Unix and does not receive that SIGINT itself, the runner kills its
  group). On a terminal after the foreground hand-over below, a Ctrl-C reaches only the script's group and the runner sees no
  signal: the exit code is 130, 131 or 129 only when the script itself dies of that signal (`terminal_cancellation`), while a
  script that traps SIGINT and exits gives its own exit code (0 stays 0), as a foreground job does in a shell. The script (node) with its descendants and a running `tsc` are killed. Every cancel path returns `Ok(130 | 143 | 129 | 131)`, never `Err`: tokio's stdin read cannot
  be cancelled, and only an `Ok` result reaches `process::exit`, so an `Err` would leave the runtime waiting for stdin EOF.
  For the same reason a failed write of the cancel notice or of the report (a closed or broken stderr or stdout) is only
  logged. A cancel before the script starts writes `run cancelled by <SIGNAL>; the script was not read` (stdin script not yet
  read) or `...; the script was not started` (type check or pre-spawn) to stderr, and the run directory (a stdin snapshot included) is
  removed best-effort; the pre-spawn `now_or_never` check is best-effort, a signal that slips past it is caught by the cancel-first
  select and kills the freshly spawned node (same exit code). A cancelled script reports the trailer status `cancelled by
  SIGINT|SIGTERM|SIGHUP|SIGQUIT` and JSON `script_status` the same text; the call journal summary is printed (the status is not `exit 0`).
- Wrap-up (`wrap_up` in `commands/run.rs`): after the script ends, `run` waits for `helper.finish()` (closing the helper and
  the MCP sessions) **without a bound** while no signal has arrived, so a normal run always reports its call summary, the
  `--dry-run` report and the journal marker. The bound `WRAP_UP_BOUND` (4 s) applies only once a signal has arrived: the
  script was already cancelled, or a signal arrives during the wrap-up (the status then becomes `Cancelled(signal)`, exit
  the signal's code). Under the bound, a further signal cuts the wrap-up at once; when the bound expires or the wrap-up is cut, the
  call summary is dropped and the notice `call summary dropped: closing the MCP sessions was cut short` is added.
  A session that does not answer its close request is abandoned after the introspector's `CLOSE_GRACE` (2 s) and its child
  killed, so the exit after a signal is bounded by `WRAP_UP_BOUND` plus the closes already in flight (6 hanging HTTP
  sessions took 12 s unbounded; the #636 test `acceptance_run_wrap_up` asserts exit 130 within 6 s and the notice).
- Test hook (`test-fixtures` builds only, absent from release binaries, #638): when `MCP_EXECUTION_TEST_READY_FILE` names
  a path, `run` writes that file after the termination handlers are armed and right before it reads a stdin script, so a
  test waits for readiness instead of sleeping. A write error is only logged. The fixture server also honours
  `MCP_FIXTURE_HANG_ON_CLOSE` (HTTP only: a session `DELETE` never answers).
- Residual (documented in `run --help`): SIGKILL cannot be handled. If the runner is killed that way, including `kill -9 -- -<pgid>`
  on its group, the script process it started can keep running, and, after a terminal hand-over, the terminal stays with the orphaned script group, so the shell may report an input/output error when it reads the terminal. On Unix the tsc process group is killed on timeout or cancel
  (`ProcessTree`, #633, see `--typecheck`). Since #646 the script is spawned through `ProcessTree` as well: it leads its own
  process group, and the processes that remain in that group get SIGKILL on timeout, cancel and drop. A descendant that created
  its own session or process group (`setsid`, detached spawn) is not stopped. After a normal exit of the script (#656)
  `ProcessTree::wait` waits for the exit without reaping (`waitid(EXITED | NOWAIT)` on a blocking thread), then sends SIGKILL to
  the group and reaps the leader, so background descendants that remain in the group are killed (the script only; after a normal
  `tsc` exit nothing is signalled). The group is signalled only while the leader is unreaped, so a recycled pgid is never hit.
  Residuals: on darwin `waitid(EXITED | NOWAIT)` also reports a stopped or continued leader, so the exit wait polls every 20 ms
  while the leader stays stopped (Linux reports only exits), and the group is killed and reaped only after a real exit; killing
  the group safely assumes nothing else reaps the script (an `ECHILD` result sends no signal); `kill_leftovers` ignores `EPERM`,
  so a leftover owned by another uid survives. Because the script is in its own group, the
  runner handles SIGHUP and SIGQUIT itself as cancellation (exit 129 and 131) instead of relying on group delivery, except when
  the runner inherited either signal as ignored (#655): `signal.rs` reads the startup dispositions (`sigaction` query, before
  any registration) and registers no cancel handler for a signal that was `SIG_IGN` at startup, so it stays ignored by the
  runner (a debug log only) and a foreground `nohup` on a terminal still ends the run on hang-up. Node resets an inherited
  ignore of SIGHUP, so the script itself still ends on SIGHUP. SIGINT and SIGTERM are unchanged (tokio overrides an inherited
  `SIG_IGN`; follow-up #664). Residual: SIGQUIT is ignored by a non-interactive shell for a background `&` job, so the runner
  then ignores SIGQUIT too; a background run needs `setsid` or `tmux`.
  On Windows (#649) the script and `tsc` are assigned to a job object (`JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`, no breakaway) in
  `ProcessTree` (`runtime/exec.rs`, private `job` module): the job is terminated on timeout, cancel and drop, and after a normal
  exit of the script or `tsc`, which kills leftovers on Windows (Unix does not signal after a normal `tsc` exit). When the job
  cannot be created or the child cannot be assigned, only the direct child is stopped (`kill_on_drop`). Residuals: a process
  started before the assignment can escape the job; whether libuv's own job object nests with this one for a grandchild of a
  non-detached script child has no Windows test (#665).
- Terminal foreground (#646, `runtime/tty.rs`, Unix): the script's group would be a background group on a terminal, and a
  descendant reading `/dev/tty` (an `ssh`, `sudo` or git credential prompt) would be stopped by SIGTTIN. Right after the spawn
  `ForegroundGroup::hand_over` opens `/dev/tty` and, only when `tcgetpgrp` equals the runner's own group, makes the script's
  group the terminal's foreground group (`tcsetpgrp` with SIGTTOU blocked on the calling thread through `pthread_sigmask`, since
  the runner is a background group at restore time), then sends the group SIGCONT in case SIGTTIN had already stopped it. The terminal goes back to the runner's group on a normal end, timeout,
  cancel and drop, but only when the foreground group is the script's own or a dead group (#659): the check is `tcgetpgrp`, and a group counts as dead only when `kill(-pgid, 0)` answers exactly `ESRCH` (`EPERM` counts as alive), so a terminal that a shell job-control step or another process took over is left alone. Nothing changes when the runner has no
  controlling terminal, is itself a background job, or the hand-over fails (a debug log only); non-Unix has no hand-over. As the
  script's group then receives the terminal-generated SIGINT, SIGQUIT and SIGHUP instead of the runner, `terminal_cancellation`
  maps a script that died of signal 2, 3 or 1 to `Cancelled(Interrupt | Quit | Hangup)` (exit 130, 131, 129), applied only
  after a hand-over; so a script killed by one of those signals on its own account is reported as cancelled too, and a script
  that traps SIGINT and survives keeps running, as a foreground job does in a shell. In short, on a terminal Ctrl-C reaches only the script's group: a script that handles SIGINT is not force-stopped and keeps its own exit code or keeps running, while Ctrl-\ (SIGQUIT), a terminal hang-up and a `kill` aimed at the runner still stop the run when the script does not handle them. Ctrl-Z would stop the script's group and leave the runner, now a background group, waiting
  with no shell prompt, so the run never suspends: `ForegroundGroup::keep_running`, a third arm of the `select!` in
  `run_script_process` (it never completes; without a terminal it stays pending; no thread), sends the script's group SIGCONT on
  every 100 ms tick unconditionally (#659, #660; the former stopped-leader probe is gone, because `waitid` sees only the caller's
  children and cannot see a stopped descendant), so Ctrl-Z is a no-op for a running script while Ctrl-C and the timeout keep
  working. Residuals: a node SIGCONT handler, if any, fires about ten times a second, and a group member stopped on purpose or by a
  user's `kill -STOP` is resumed. On darwin a group whose only member is an unreaped zombie answers `EPERM`, so the terminal is
  not taken back. Polling is used instead of SIGCHLD; `waitid` is unavailable on
  OpenBSD and similar targets, which the crate does not support. The `runtime::tty` module is `pub(crate)`. Dependencies (`mcp-cli`, `cfg(unix)`
  only): `nix` 0.32 (`default-features = false`, feature `signal`) for the safe `pthread_sigmask`, because the workspace denies
  `unsafe`; `rustix` gains the `termios` feature.
- Trailer: `[mcp-execution <token>] script_status exit N | signal N | timed out | cancelled by SIGINT|SIGTERM|SIGHUP|SIGQUIT`. Stdout notices (cut, drain
  abandoned, timeout) go to stdout and the stderr cut notice to stderr, all carrying the token. JSON mode prints no
  anchor and no stderr; the envelope holds `token`, `script_status`, `exit_code`, `stdout`/`stderr` (`text`, `bytes`,
  `truncated`, `spill`, `spill_complete`) and `notices`. A spill failure degrades to a notice, never a runner failure.
- Spill notice wording (#625): `<stream> cut at <shown> of <total> bytes; <kept> saved to <path>`, where `<kept>` is `full
  output` when the capture held every byte and `first <N> bytes` when the 8 MiB capture limit cut the stream (the file then
  holds only `N` bytes, `MAX_SPILL_FILE_BYTES` = 8388608), or the notice ends `could not be saved` when no spill file
  could be written. `--format json` carries the same notice text in `notices`; its stream objects state it through
  `spill_complete` (#635): `null` exactly when `spill` is `null`, `false` when the spill file holds fewer bytes than `bytes`
  (the capture limit cut the stream), otherwise `true`. The field is always serialized (additive change).
- Deviations (see the runtime plan): `ScriptStatus::TimedOut` for a runner kill; the wave-1 refusal of unwired flags
  (`reject_unwired_flags`) is gone: `--restrict`, `--typecheck` and `--save` landed in wave 2, `--dry-run` and
  `--policy` in wave 3 (section 18).
- The wave-1 limitation (a script that made a call never exited) is gone: the bridge is now a helper-protocol client
  that `unref`s the channel when idle (section 17).

### `stats` (#562, `commands/stats.rs`)

- Reads `_meta.json` (schema v3) and file sizes only, never file contents. Types: `Bytes`, `TokenEstimate`,
  `Measure{Available,NotAvailable}`, `StatsReport`. Estimate is `bytes.div_ceil(4)` (`BYTES_PER_TOKEN_ESTIMATE`);
  bytes are always printed ([[../decisions/ADR-562-context-cost-estimator]]).
- The sidecar is read through `read_generated_metadata` (section 3): a schema mismatch is invalid input (exit 2,
  "regenerate", nothing on stdout), a symlinked or non-regular sidecar is refused. A missing tool file or catalog is "not
  available", and the total over tool files is then also unavailable. Symlinked or non-regular files count as not
  available. Bounds: `MAX_METADATA_FILE_BYTES`, `MAX_TOOL_COUNT`. One text form for every `--format`. `--servers-dir`
  expands a leading `~`. All file measurement for one run happens in a single `spawn_blocking` (#621), not one hop per
  tool, bounded by `MAX_TOOL_COUNT`.
- "All tool files" excludes `index.ts` and runtime files; "one tool file" is the mean, rounded down; the output-schema
  share counts `Declared` and `Degraded`.

### `drift` (#580, `commands/drift.rs`)

- Flow: `ServerFlags`, `resolve_server_config`, `~/.claude/servers/<server id>/_meta.json` read through
  `read_generated_metadata_async` (bounded, regular non-symlink file, schema v3; section 3), `Introspector::discover()`, fingerprint first, then `ToolDigest::compute` over
  `ToolInfo::digest_entry`, then a name diff. Types: `DriftOutcome{NoDrift,EndpointDiffers,ToolsDiffer{added,removed}}`.
- Exit: 0 no drift; 5 (`CHECK_FAILED`) drift or endpoint differs; could-not-complete errors use the runner
  classification (a missing directory gives 1; a `_meta.json` schema-version mismatch gives 2, distinct from drift, #618). Writes nothing; prints no descriptions.
- Output: names through `sanitize_untrusted_inline`, at most `MAX_PRINTED_NAMES` 20 per list plus "and N more"
  (`MAX_NAME_CHARS` 128); ttl and cache scope printed when present, with a credentials notice for `Private`; the same
  names with a different digest print "which tool changed cannot be determined". Summaries say "server changed" or
  "tool list differs", never "out of date".
- Location (fixed in wave 2, architect ruling; the user is to be informed): `DriftArgs` gained `--name` and
  `--progressive-output` with the names and meaning of `generate`. The location derivation is shared with `generate`
  (`name_override()` and `generated_location()` in `commands/generate.rs`; `resolve_server_dir_name` takes
  `&ServerId`), so a package generated with `--name` or `--progressive-output` is found. The skills directory for
  FR-045.18 is `SkillsDir::resolve(None).skill(<id>)` (`~/.claude/skills/<id>`), id being the `--name` override or the server id.
- FR-045.18 (wave 2): after a `ToolsDiffer` with removed tools, `scripts_using()` lists the saved scripts
  (`scripts::list`) that name a removed tool of this server, through the same bounded and sanitised `NameList` (report
  field `scripts_using_removed_tools`, text heading "saved scripts using a removed tool"). Best effort: a missing
  skill directory or a `list` error gives no names. Only removed tools count: changed tools are not detectable, since
  no per-tool digests are recorded. `read_recorded` is gone (#618): `drift` and `scripts check` both call
  `read_generated_metadata_async`.

### Logging profile for `call`, `run` and `helper`

`Commands::log_profile()` returns `LogProfile::{Standard, Quiet}`. `call`, `run` and the hidden `helper` use `Quiet`:
the default level is `warn` and rmcp is capped at `warn`. `--verbose` or `RUST_LOG` restores the standard behaviour
(rmcp capped at `info`). `runner.rs`: `init_logging(verbose, log_format, profile)`,
`log_filter(verbose, profile, env)`, `cap_rmcp_log_level(filter, LevelFilter)`; `main.rs` passes
`cli.command.log_profile()`. Reason: a script or a model reads the process output, so tracing lines must not mix into
it.

## 17. Helper, Saved Scripts, Typecheck and Restrict (as built)

### Helper (#575, `runtime/{protocol,pool,helper}.rs`, `commands/helper.rs`)

- `HelperServer::bind` (Unix): `UnixStream::pair()`, the child end becomes node's fd 0 through `OwnedFd` and `Stdio`;
  env `MCP_EXECUTION_HELPER=fd:0`, no token env. Windows: a pipe with `first_pipe_instance`, `reject_remote_clients`,
  `max_instances(1)`, name `\\.\pipe\mcp-execution-<uuid>`, one accept raced against stop, never re-created; env
  `MCP_EXECUTION_HELPER=pipe:\\.\pipe\mcp-execution-<uuid>` and the token in `MCP_EXECUTION_HELPER_TOKEN`.
- `serve`: hello read with a 5 s timeout counted from the accepted connect, `MAX_HELLO_BYTES` 4096, version and token
  checked, a mismatch closes the channel without a reply. The reader loop acquires the `MAX_INFLIGHT_CALLS` (16)
  semaphore before spawning each call and holds the permit until the response is written; a `JoinSet` cancels
  in-flight calls at the end; one writer task over a queue of `RESPONSE_QUEUE` 64. `HelperHandle::finish` signals stop, waits for the loop (bounded by `FINISH_GRACE` 2 s) and
  closes the pool.
- Cancel safety (built invariant): the request reader is never cancelled by a completing call. `serve` reaps finished
  calls with `JoinSet::try_join_next` at the top of each loop turn, outside the `select!`, because `read_line` keeps a
  partly read line in a local buffer and cancelling it would drop that prefix (the next request was then rejected as
  malformed with id 0). The only branch racing `read_line` is the stop signal. Regression test:
  `a_call_finishing_while_a_request_arrives_in_pieces_does_not_corrupt_it` (`tests/helper_stdio.rs`).
- Protocol (`protocol.rs`): `read_line` is bounded (never buffers past the limit plus one read); `encode_response`
  turns a line over `MAX_HELPER_LINE_BYTES` into `call_failed` "result exceeds N bytes". A request that cannot be
  parsed or exceeds the bound gets `call_failed` with its `id` echoed when parseable, else id `0`, and the channel
  closes; clients start ids at 1 and treat a close as rejecting every pending call.
- Ownership (#620, #628): `CallHooks::execute` and `SessionPool::call` take the `Request` by value and move its arguments
  into the `ToolCall`; `classify` takes the `CallToolResult` by value and moves the value out; the journal digest and
  argument size are computed from the arguments map by reference, before the move, without cloning it.
- Pool (`pool.rs`): `ServerId` to `Arc<OnceCell<Session>>`; the lock is released before the connect await; a failed
  init evicts only when `Arc::ptr_eq` matches; `MAX_POOLED_SERVERS` 16. Servers resolve through
  `resolve_server_config(ServerSource::Config)`, so the helper reads `mcp.json` (`HelperConfig::servers_dir` also
  feeds the dry-run suggestions, see section 18). Every outcome maps to a `ResponseBody`; runner text goes in `message` and server text only in
  `server_message`; failure texts are sanitized and capped at 500 characters.
- `helper` command (hidden): serves over stdin and stdout until EOF, then closes the pool. Under `run`, the journal and
  the dry-run report come from the typed `journal` and `dry_run` fields of `HelperConfig` (section 18, D24).
- `setup`: `SetupResult.helper_binary` is a `CliBinary` (`Found(path)`, `OverrideInvalid`, `NotFound`; a new JSON
  output field tagged `state` and `path`) and a warning line for the last two. `discover_cli_binary`:
  `MCP_EXECUTION_CLI` wins when set and must be an absolute path to an executable file, otherwise the state is
  `OverrideInvalid`, and there is never a fallback to PATH for a set variable (architect, D17). When the variable is
  unset, the first `mcp-execution-cli` on PATH is used (`which`), else `NotFound`.
- Protocol rule on id 0 (architect, D17): client ids start at 1. A request that cannot be parsed or has id 0 ("request
  id 0 is reserved") gets an error response and the helper closes the channel; the bridge treats a response with id 0
  as fatal and fails the connection ("the helper rejected a request it could not process").

### `run` wiring (`runtime/{exec,preload.js,import_hook}.rs`, `commands/run.rs`)

- `run` binds `HelperServer` (`Carrier::native()`, call timeout `min(run timeout, DEFAULT_CALL_TIMEOUT)`), passes the
  endpoint env and the channel as stdin to node, always calls `finish()`, and prints `journal_summary` and
  `dry_run_report` notices when present. `run_script_process(spec, helper: Option<&mut HelperEndpoint>)`;
  `ScriptProcess::node_args`.
- Preload: always registers the `mcp:<id>` and `mcp:helpers` hooks. Only when `MCP_EXECUTION_HELPER` is set it opens
  `fd:0` or `pipe:<name>`, writes `{"v":1[,"token"]}\n`, `unref()`s the socket, stores it at
  `globalThis[Symbol.for("mcp-execution.helper-channel")]` (core constant `HELPER_CHANNEL_KEY`), replaces
  `process.stdin` with a lazily created empty Readable (`isTTY` false), and deletes `MCP_EXECUTION_HELPER` and
  `MCP_EXECUTION_HELPER_TOKEN` from `process.env`. The bridge only `ref()`s the channel while calls are pending.
- Tokio feature `io-std` added to mcp-cli.

### `--typecheck` (#577, `runtime/typecheck.rs`)

- tsconfig from the codegen `TSCONFIG_JSON` options plus `typeRoots` (existing `<servers>/*/node_modules/@types`, at
  most `MAX_TYPECHECK_SERVER_DIRS` 256 listed), `paths` (`mcp:helpers` to a written `helpers.d.ts`, `mcp:*` to
  `<servers>/*/index.ts`) and `include` (script and helpers); no `extends`; the script is never read. `tsc -p ...
  --noEmit --pretty false`, `TYPECHECK_TIMEOUT` 60 s (skipped on timeout), found with `which`.
- Output capture (#622): `tsc` runs with piped stdout and stderr read through the same `PipeCapture` and `drain(.., DRAIN_GRACE)`
  as the script, each stream capped at `MAX_SPILL_FILE_BYTES` (8 MiB) and always drained so `tsc` never stalls. Exit 0 is
  `Passed` whatever the capture holds (a cut stream is ignored). A failed run is classified from the captured text, and when
  a stream was cut the line `tsc output exceeded 8388608 bytes; remaining diagnostics were not read` follows the bounded
  diagnostics; a `Skipped` outcome is left as is. Each distinct file named by the diagnostics is canonicalized once
  (`distinct_files`, a `HashMap`), not once per diagnostic group. A signal during the type check kills `tsc` (section 16).
  On Unix `tsc` (and, since #646, the `run` script process) runs in its own process group (`ProcessTree`, `runtime/exec.rs`, #633): on timeout or cancel the whole group
  gets SIGKILL through `rustix::process::kill_process_group` while the leader is unreaped, so a recycled pgid is never hit,
  and nothing is signalled after a normal exit (a descendant that outlives tsc survives on Unix; unlike the `run` script, which
  is killed after a normal exit, section 16). On Windows (#649) `tsc` runs in a job object (kill-on-close, no breakaway) that is
  terminated on timeout, cancel, drop and after a normal exit, with the fallback to the direct child described in section 16.
  The capture tasks are aborted when the `PipeCapture` is dropped, on every platform. Residual: the unhandled-signal cases
  listed in section 16 leave tsc running.
- Skipped with a `typecheck skipped: <reason>` notice after the anchor when `tsc` is not on PATH ("tsc was not found on
  PATH"), when it does not finish within 60 s, or when the environment is unresolved (below). A real problem in the
  script fails: at most `MAX_DIAGNOSTIC_LINES` 20 and `MAX_DIAGNOSTIC_BYTES` 4096, escaped, to stderr before node
  starts, exit 2.
- Classification (as built, `classify_diagnostics`): each tsc diagnostic group is one of node types missing (TS2580,
  TS2591, TS2503, TS2688 for `'node'`, TS2307 for `node:*` or a builtin), located in a generated SDK file or global
  (no file at all), an implicit any (TS7006, TS7031), or genuine. Any genuine diagnostic, or an empty diagnostic list
  from a failed tsc, fails the check; implicit-any diagnostics count as genuine only when the environment is resolved.
- Deviation (approved by the architect on condition, D13: only an all-missing-types diagnostic set is skipped): `types: []` is set when no `@types/node` is found under the servers, because a global
  tsc error for the missing `node` library would suppress every other diagnostic. The plan's "no node types: skip"
  pre-check was replaced by the diagnostic-based skip, matching the acceptance tests.
- Top-level `await` in a `.ts` script passes: the check-time overlay tsconfig sets `module` to ESNext,
  `moduleResolution` to Bundler and `moduleDetection` to force. The generated tsconfig is untouched; the overlay changes
  `module` and `moduleResolution` for the check only.
- When there is no genuine diagnostic and node types are missing, the notice is "typecheck skipped: environment
  unresolved (Node type definitions not installed)"; when diagnostics sit in generated SDK files or are global, it is
  "typecheck skipped: environment unresolved (the generated SDK or the compiler reported problems of its own; run npm
  install in the server directory)". TS7006 and TS7031 in the script are ignored only while the environment is
  unresolved. Any other error in the script exits 2.
- Deviation D23 (the check-time overlay and the TS7006/TS7031 handling): approved by the architect, relayed by the
  lead, 2026-11-24, with four conditions:
  1. A diagnostic is attributed to the generated SDK by canonicalized path prefix against the canonical servers
     directory; a diagnostic with no file (a global type) counts as environment.
  2. A script error other than TS7006 or TS7031 always exits 2, including when environment diagnostics are present.
  3. When the environment is unresolved and the script has no other errors, the result is reported as a skip with
     the "environment unresolved (...)" notice above, never as "passed".
  4. The overlay applies only to the check; runtime execution does not change.
  Verified against the code (`runtime/typecheck.rs`, `kind_of` and `classify_diagnostics`; `commands/run.rs`
  notice): conditions 1 to 3 hold as built, with the final wording of the notices given above.

### `--restrict` (#576, `runtime/restrict.rs`)

- `permission_args` (final, architect ruling): `--permission` plus exactly one `--allow-fs-read=<path>` per path
  (script, servers dir, run dir). Windows is refused with exit 2 before node starts (support stays open, #616); a path containing a comma is also
  refused (exit 2), kept as a precaution for older Node lines that split on commas. Node 26 does not split a comma
  list, which is why one flag per path is used. `--preserve-symlinks` was tried in wave 2 and rejected: it is not needed
  once the per-path flags are used (`restrict_still_lets_the_script_call_tools_through_the_helper` passes without it
  under the canonical `/private/var` sandbox), and no path canonicalisation was added. The comma-joined argument and
  its `TODO(review)` are gone.
- The fd-0 channel needs no `--allow-net`. Node 25+ narrows network through the permission model; Node 22 to 24 narrow
  files and processes only.

### `--save` (#579, `commands/run.rs`)

- Validated before node starts, each failure exiting 2: `RecipeName`, `parse_header`, header name equals NAME, one
  server from the header tools, and an existing name without `--overwrite`.
- After a successful run (script exit 0), `scripts::save(~/.claude/skills/<server>, ...)` runs. If saving fails then,
  the run exits 1 with the notice "script succeeded; save failed: ...". If the run did not succeed, nothing is saved and
  the output carries "not saved: script did not succeed". Success adds `saved script <path>`. There is no
  `--skills-dir` flag on `run`. (Architect ruling; replaces the wave-2 behaviour of exit 2 for every save failure.)

### `scripts list` and `scripts check` (#579, `commands/scripts.rs`)

- `list` prints `name [writes]: description (tools)`; `check` prints `script: missing tool server/tool` and
  `file.ts: invalid header` and exits 5 on any problem, 0 when clean; `--format json` is structured. `check` takes
  `--servers-dir` (#619; `CheckArgs` wraps the shared `ScriptsArgs` and the option, `ScriptsAction::Check(CheckArgs)`) and
  reads `<servers-dir>/<server>/_meta.json`, default `~/.claude/servers`, through `read_generated_metadata_async`: a missing
  sidecar is exit 1, a schema-version mismatch exit 2, a symlinked sidecar refused. `--skills-dir` and `--servers-dir` expand
  a leading `~` (section 3).

### Run marker, pipe descriptor and Windows binary resolution (D25)

Architect-approved rulings, relayed by the lead, 2026-11-24; all are built and were checked against the code.

- **Run marker.** `MCP_EXECUTION_RUN=1` (core constant `RUN_MARKER_ENV`) is set on the whole process tree under `run`
  (`runtime/exec.rs`). The preload keeps it; the channel variables stay cleared.
- **Bridge refusal.** When the marker is set and no channel is registered, the bridge refuses to spawn its own helper
  and fails with `call_failed` "refused: tool calls from a child process or worker thread are not supported under
  `run`; call tools from the main script" (`runtime-bridge.ts.hbs`). The residual is stated in the `run` help text.
- **Command refusal.** Under the marker, `call`, the hidden `helper` and `run` exit 2 with "not supported inside
  `mcp-execution-cli run`; call tools from the main script" (`commands/common.rs`, `refuse_inside_run()`); otherwise a
  script could run `call` or a nested `run` to escape `--dry-run` and the journal.
- **Binary resolution in the bridge.** A set `MCP_EXECUTION_CLI` always wins, must be an absolute path and has no PATH
  fallback (D17). When it is unset, the bridge resolves `mcp-execution-cli` (`.exe` on Windows) through absolute `PATH`
  entries only, skipping empty and relative entries and so the current directory (libuv searches the current
  directory first on Windows), and spawns the absolute path; if none is found it fails with "mcp-execution-cli not
  found on PATH; set MCP_EXECUTION_CLI". Windows needs no variable.
- **Pipe descriptor.** On Windows the runner exports `MCP_EXECUTION_HELPER=pipe:\\.\pipe\mcp-execution-<uuid>` and the
  token in `MCP_EXECUTION_HELPER_TOKEN` (Unix: `fd:0`); the preload accepts `fd:0` and `pipe:<name>` only. The accept is
  raced against stop, the 5 s hello timeout is measured from the accept, and `HelperHandle::finish()` is bounded by
  `FINISH_GRACE` 2 s.
- **Pipe access (#617).** The Windows pipe is created with the protected owner-only DACL `D:P(A;;GA;;;OW)` (one allow ACE for
  OWNER RIGHTS, no inherited entries) through `runtime/helper.rs` (`pipe_security`); creation fails closed with an error when the
  descriptor cannot be built. Residual: under an elevated token the owner is BUILTIN\Administrators, so any elevated admin
  process can open the pipe (administrators are outside the threat model).

## 18. Journal, Dry Run, Policy and Harness (as built)

### Runner state directories (`runtime/dirs.rs`, `platform_state_base`)

All runner-owned state (spill files, journals, `journal.key`, per-run scratch) lives under one root,
`<base>/mcp-execution`, where `<base>` is per OS:

| OS | Base |
|----|------|
| Linux | `dirs::state_dir()` |
| macOS | `dirs::data_local_dir()` |
| Windows | an absolute `%LOCALAPPDATA%` first; when it is unset or relative, `dirs::data_local_dir()` |

A missing base is an error, never a silent temp-dir fallback, and so is a relative base (a relative `HOME` or XDG variable): `RuntimeDirs::resolve` and `policy_path` fail with `the <state|config> directory '<value>' is not an absolute path` (#652). On Windows `%LOCALAPPDATA%` is preferred because the
known-folder API behind `dirs` ignores the environment, which would make the state root impossible to redirect in tests.

The dry-run policy file lives under the config directory instead: `<config base>/mcp-execution/policy.json`
(`RuntimeDirs::policy_path`, `platform_config_base`), where `<config base>` is `dirs::config_dir()` on Linux and macOS and,
on Windows, an absolute `%APPDATA%` first; when it is unset or relative, `dirs::config_dir()`. It mirrors the state root
handling for the same reason: the known-folder API ignores the environment, so the policy path could not be redirected in
tests. A missing config base is an error, never a silent temp-dir fallback.

### Journal (#572, `runtime/journal.rs`)

- `Journal::create` prunes journals (7 days, 50 files; a live journal is spared for `LIVE_JOURNAL_GRACE` 600 s),
  loads or creates the per-user key `<state root>/journal.key` (32 random bytes from two v4 UUIDs, written through a
  temporary file and an atomic rename, 0600; a corrupt key is regenerated),
  and opens `journals/journal-<run-id>.jsonl` (0600, no symlink following) through the shared `create_private_file` of
  `runtime/dirs.rs`, which spill also uses. A journal creation error fails the run with exit 1 instead of silently
  disabling the journal (architect, D19).
- Digest: SHA-256 over the domain tag `mcp-execution/journal/args/v1\0`, the key and the canonical arguments;
  `canonical_args` sorts keys recursively, since `serde_json` keeps insertion order workspace-wide. `Journal::digest` takes
  the arguments map by reference and returns the digest with the canonical size, serializing through private sorted
  serializers, so the arguments are not deep-cloned (#620). The summary's `args=` prefix is the first 12 hex characters of
  the digest (#628), and the writer queue takes line text by move.
- Lines: `start {event,seq,server,tool,digest (64 hex),args_bytes}` recorded before the call is sent, `end
  {event,seq,status,duration_ms,result_bytes}`, `truncated {event,dropped}`. `result_bytes` is the length of the whole
  encoded response line, newline included (`{"id":N,"value":...}\n`, #620), for a value body and 0 for any other body; a
  response replaced for exceeding `MAX_HELPER_LINE_BYTES` is journaled with status `failed` and the size of the original
  response (`EncodedResponse::Replaced::original_bytes`), not the replacement's. The file holds server id, tool name, keyed
  digest, argument size, status, duration and result size only, never a value or server text. One writer task flushes
  each line. Caps: 2000 lines and 1 MiB per file, with a reserved `MARKER_RESERVE` 256 bytes for the truncation marker.
- Summary (`finish`): printed only when the script status is not `exit 0` (timeout, signal, non-zero exit): `call
  summary: N call(s); completed calls may have changed state`, then `  #<seq> <server>/<tool> args=<12 hex> <status>
  <ms>ms`, failed and unknown calls first, at most 20 entries plus "and N more"; an in-flight call reads `unknown (in
  flight when the run ended)`. Applies to `run` only.

### Dry run and policy (#573, `runtime/policy.rs`, `runtime/helper.rs`)

- Policy file: `{"version": 1, "reads": ["<server>/<tool>"]}` (as built, architect ruling D19): `version` 1 is
  required and unknown fields are denied; each entry is split at the first `/` and validated as `ServerId` plus
  `ToolName`. `ToolName` cannot contain `/`, so an entry with more than one `/` is invalid, and a wildcard entry is
  invalid (rejected at load, not silently ignored). Matching
  is exact string match. Unknown fields and invalid JSON are rejected with a fixed message and the file path only,
  64 KiB cap. An absent default file is an empty policy (every call is a write); a missing explicit `--policy` path
  is invalid input (exit 2). `--policy` requires `--dry-run`. Annotations are never read: `_meta.json` annotations do
  not classify, so a hostile `readOnly` hint cannot make a write execute.
- `DryRun`: the skip decision plus a bounded report that keeps the first `MAX_REPORTED_WRITES` 50 skipped writes
  (order, `server/tool`, digest prefix, argument size) with a counter for the rest, and an "incomplete" line. The report is a stdout notice, always printed
  when writes were skipped. A skipped write returns `write_skipped` to the script and never reaches the server.
- Helper hooks (architect, D24): there is one `pub fn bind(Carrier, HelperConfig)` (public because integration tests
  use it) and no second constructor. `HelperConfig { servers_dir, call_timeout, journal: Option<Arc<Journal>>,
  dry_run: DryRunMode }`, with `DryRunMode { Off, On(Policy) }` (default `Off`). `CallHooks::execute` runs journal
  start, the dry-run decision, the pool call and journal end; `pool.call` returns `CallReply {body, status}`;
  `HelperHandle::finish` returns the real `RunCallReport`.
- `--dry-run` and `--policy` are not refused; `reject_unwired_flags` is gone.
- `HelperConfig::servers_dir` (architect, D20) serves two readers: the preload resolves `mcp:<id>` imports with it, and
  the dry-run report reads each skipped server's recorded `_meta.json` there to add a suggestion line ("the server marks
  it read-only (a hint, not trusted): list \"server/tool\" under reads to run it") for a tool whose recorded annotations
  say read-only; the suggestion never changes what executes.
- A journal creation error fails the run with exit 1 before node starts (architect, D19).

### Test environment notes

- Integration tests run the CLI in a sandbox (`tests/common/mod.rs`) that redirects `HOME`, `USERPROFILE`, the `XDG_*`
  variables, `APPDATA` and `LOCALAPPDATA`; the last two redirect the Windows policy path and state root (see Runner state directories).
- Unit tests that raise signals, redirect `HOME` or install a scoped log subscriber run only under `cargo nextest`, which gives
  each test its own process (#658; `commands::common::test_isolation`, `process_per_test`; the former `HOME_ENV_LOCK` is gone).
  Under plain `cargo test` they skip with a stderr line; under nextest without `NEXTEST_EXECUTION_MODE=process-per-test` they panic.
- Tests that need `~/.claude/mcp.json` are `#[ignore]`d on Windows: `dirs::home_dir()` ignores `HOME` and `USERPROFILE`
  there, so the file cannot be redirected (#604).

### Context-cost harness (`tests/context_cost_harness.rs`)

- For K in {1, 2, 3, 5, 10, 20}: arm A is K `call` processes, arm B is one `run -` whose script makes K `echo` calls.
  Bytes reaching the context are the command text plus stdout and stderr; `ContextBytes` newtype, tokens
  `div_ceil(4)` labelled "estimate (bytes/4)", never "savings". The crossing is computed from the data, not hard-coded;
  it skips without node locally and hard-fails under `CI`. Results are in [[../NFR-mcp-execution-2026-07-27]]
  (NFR-PERF-003): B is smaller from K = 5. The script text is fixed, so a model's cost of writing the script is not
  counted.

## 19. Script-First Requirements and Contracts

> [!success] Implemented
> The requirements below were implemented in the script-first pull request (#569 accepted, option B). Each **Why**
> paragraph describes the state before the change; the as-built sections above govern where they differ. Decisions
> that are still open are marked.

Command and flag names are final: `stats`, `run`, `call`, `scripts list` and `scripts check`, `--save`, `drift` (sections 16 and 17). The `skill` command also changes through
the script-first skill package (default name, name validation, body shape); that content lives in
[[../skill/spec#14. Script-First Requirements]] and is not repeated here.

### Script runner and bounded output (#571, FR-036)

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

**Constraints.**

- Constitution [[../constitution]] section V applies: path confinement and
  symlink-aware validation, resource bounds as named constants (a bound above an
  existing one derives from it), redacting `Debug` on any type that carries
  script text, arguments or environment values, and the untrusted-text rule for
  error text and for tool and server names that originate from a server (printed
  results are the open decision of "Security posture of the runner" below).
- Reuse the CLI's `ExitCode` and classification; reuse existing confinement
  helpers instead of writing new ones.
- Works against servers generated by the current bridge. If generation must
  change (for example to add a fixed helper file), that is a breaking change
  and follows the fixed-file rule in [[../codegen/spec]].
- Scripts are never interpreted, linted or restricted by the runner.

### Call journal and partial-failure report (#572, FR-037)

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
| FR-037.12 | WHEN a response is replaced for exceeding the helper line bound THE SYSTEM SHALL journal the call as failed with the size of the original response, and for a delivered value SHALL record the length of the whole response line as the result size | must |

No requirement in this section may rely on a server's read-only
hint. Tool and server names in the summary come from an untrusted server and are
sanitised before a model sees them.

**Constraints.**

- The journal is diagnostic, not an audit log: a script runs with the user's
  privileges and can alter or delete its own journal. Tamper resistance belongs
  to FR-041. No document may call the journal tamper-evident.
- Types: `ServerId`, `ToolName`, a digest newtype, a status enum with an
  explicit unknown variant; hand-written redacting `Debug` on any type holding
  argument bytes before digesting.
- All bounds are named constants; per-entry size is bounded by construction.

### Dry run and write policy (#573, FR-038)

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
drift check (FR-045, #580), not to this component. FR-038.4 is satisfied by never classifying from annotations: only exact policy `reads` entries execute (section 18).

**Constraints.**

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

### One-shot tool call through the authoritative client (#574, FR-039)

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

**Constraints.**

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

### Per-script helper and retirement of the generated bridge (#575, FR-040)

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-040.1 | THE SYSTEM SHALL provide a per-script helper holding pooled connections, bounded in count and lifetime, with the connect, validation and result semantics of FR-039 | must |
| FR-040.2 | WHEN the run ends, whether it succeeds, fails or times out, THE SYSTEM SHALL close every pooled connection and leave no server process running | must |
| FR-040.3 | THE SYSTEM SHALL route generated wrappers' tool calls through the helper | must |
| FR-040.4 | THE SYSTEM SHALL remove from the generated bridge its own connection code, its mirrored configuration validation and the `2024-11-05` handshake pin (`runtime-bridge.ts.hbs:966`) | must |
| FR-040.5 | THE SYSTEM SHALL list the generated-output changes and the regeneration requirement as breaking changes in the changelog, with no compatibility shim (pre-1.0) | must |
| FR-040.6 | THE SYSTEM SHALL state the behaviour when the binary cannot be found by a script, and `setup` SHALL verify what that requires | must |

This component retired the generated bridge's connection code, mirrored validation and protocol pin; the generated side is in [[../codegen/spec#14. Helper-Protocol Bridge (as built)]].

**Constraints.**

- Single connection path: the helper uses the shared connect step of FR-039.2.
- Bounds on pool size, lifetime and per-call time are named constants.

### Reduced-privilege script execution (#576, FR-041)

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-041.1 | WHERE the helper carries all tool traffic THE SYSTEM SHALL be able to run a script with its filesystem, process-spawn and network access narrowed to what the user permits | should |
| FR-041.2 | WHEN reduced-privilege execution is not active THE SYSTEM SHALL say so wherever it describes a run, and SHALL NOT describe the runner as sandboxed | must |
| FR-041.3 | THE SYSTEM SHALL state what the narrowing covers and what it does not | must |

Mechanism: the Node.js permission model through `--restrict` (section 17), Unix only; network is narrowed only on Node 25 and later, files and processes on every accepted line.

### Pre-run typecheck (#577, FR-042)

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-042.1 | WHERE a TypeScript compiler is available THE SYSTEM SHOULD typecheck a script against the generated SDK before executing it | should |
| FR-042.2 | WHEN the compiler is absent THE SYSTEM SHALL run the script and say the check was skipped | must |
| FR-042.3 | THE SYSTEM SHALL bound the diagnostics it shows and treat their text as untrusted | must |
| FR-042.4 | THE SYSTEM SHALL cap the compiler's output at 8 MiB per stream while still draining it, treat exit 0 as a pass whatever the capture holds, and say when a failing check's output was cut | must |

A type error blocks the run before node starts (exit 2); an absent compiler or a diagnostic set made only of missing Node types is skipped with a notice; the compiler is found with `which` (section 17). The cost against runner start-up is bounded by `TYPECHECK_TIMEOUT` 60 s.

### Security posture of the runner

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


### context-cost measurement command (#562)

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

**Resolved and open decisions.**

- Resolved: the command is `stats SERVER_ID [--servers-dir]` (section 16). A tool is sized by `manifest_size_bytes`, the compact serialized `ToolInfo` recorded in `_meta.json` v3. The estimator is bytes divided by 4, rounded up, labelled an estimate ([[../decisions/ADR-562-context-cost-estimator]]). `index.ts` and the runtime files are excluded from "all tool files". The mock servers live in `crates/mcp-cli/tests/fixtures/tools_mcp_server.rs`; the harness is the test target `tests/context_cost_harness.rs`, not a subcommand or a benchmark.

Ask first: choosing the estimator; a tokenizer dependency (check current versions); any machine-readable output
format or output file.

**Depends on / blocks.** Landed in the script-first pull request; no blocker remains.

### command-line front doors for the script runner (#571) and the one-shot tool call (#574)

The behaviour of both commands (execution, output budget and spill, notices, the connection path, result
semantics) is specified in [[#Script runner and bounded output (#571, FR-036)]] and
[[#One-shot tool call through the authoritative client (#574, FR-039)]]. This subsection holds
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
| FR-036.23 | WHEN the runner receives SIGINT, SIGTERM, SIGHUP or SIGQUIT (the last two on Unix, and only when it did not inherit that signal as ignored, #655) during `run` (also when the signal reaches the whole process group; on a terminal after the foreground hand-over a terminal-generated signal reaches only the script's group, and the run counts as cancelled only when the script is killed by it, while a script that handles the signal and exits keeps its own exit status) THE SYSTEM SHALL stop the script, the processes that remain in its process group (Unix) and any running type check, take cancellation before the script's own exit status, and exit 130, 143, 129 or 131 through a normal return, including while a stdin script is still being read | must |
| FR-036.24 | WHEN the output capture limit cut a spilled stream THE SYSTEM SHALL say that only the first bytes were saved, not that the full output was | must |
| FR-036.25 | On Unix, WHEN the run times out, is cancelled or the script exits normally THE SYSTEM SHALL stop the processes that remain in the script's process group (and, on timeout or cancel, the script); a descendant that created its own session or process group (`setsid`, detached spawn) is not stopped; after a normal exit of `tsc` nothing is signalled. On Windows THE SYSTEM SHALL assign the script and `tsc` to a kill-on-close job object without breakaway and terminate it on timeout, cancel, drop and after a normal exit, and, when the job cannot be set up, stop only the direct child; a process started before the assignment can escape | must |
| FR-036.26 | On Unix, WHEN the runner is the foreground process group of its controlling terminal THE SYSTEM SHALL make the script's process group the terminal's foreground group right after the spawn and restore the runner's group on completion, timeout, cancel and drop, but only when the foreground group is then the script's own or a dead group (a group counts as dead only on `ESRCH`; `EPERM` counts as alive); otherwise (no terminal, a background runner, a failed hand-over) it SHALL leave the terminal untouched | must |
| FR-036.27 | On Unix, WHEN the script's group is the terminal's foreground group THE SYSTEM SHALL send the group SIGCONT about every 100 ms, so a stop of the group does not suspend the run; a member stopped on purpose is resumed too | must |
| FR-039.20 | WHEN the call command is invoked THE SYSTEM SHALL select the server with the same selectors as `introspect` (`ServerFlags`, `ServerSource`, §2, §3), take the arguments as a single JSON object, and validate the server as a `ServerId` and the tool as a `ToolName` before connecting | must |
| FR-039.21 | WHEN the JSON argument is malformed or is not a single object THE SYSTEM SHALL exit with invalid input and report the failure without repeating any part of the argument | must |
| FR-039.22 | THE SYSTEM SHALL map call failures onto §5 (connection failure and resource limit 3, timeout 4, invalid input 2) and state the class of a tool-reported error | must |
| FR-039.23 | WHEN the call prints a result or an error THE SYSTEM SHALL apply the global `--format`, sanitise and bound server text in error output and mark it as coming from the server, and build any freeform line from untrusted text through `escape_display` (§12) | must |

**Constraints.**

- The call command is a client of the shared connect step that the introspector owns; this crate owns no
  connection code ([[#One-shot tool call through the authoritative client (#574, FR-039)]]).
- `execute_command` keeps resolving to a classified exit code and never propagates a handler failure as `Err`
  (§5; a signal-cancelled `run` returns `Ok(130 | 143 | 129 | 131)`, §16); `main.rs` still calls `std::process::exit` only after classification.
- Help text of the run command states that scripts run with the user's privileges and without a sandbox.

**Resolved and open decisions.**

- Resolved: the commands are `run [FILE|-]` and `call <ServerFlags> -- TOOL [JSON]` (section 16). A tool-reported or JSON-RPC error exits 3, a runner timeout 4, a script that exits non-zero or dies on a signal 6 (`SCRIPT_FAILED`). In text mode the runner prints an anchor line, notices and a trailer carrying the run token; `--format json` prints one envelope.
- Resolved (approved implementation deviation, user, 2026-11-24): the syntax is `call <ServerFlags> -- TOOL [JSON]`
  (clap `last = true`), for example `call --from-config github -- create_issue '{"title":"x"}'` and
  `call --http URL -- echo`. An optional positional server before `TOOL` cannot be parsed by clap. `ServerFlags` is
  unchanged (FR-039.20), so `introspect`, `generate` and `drift` keep their positional server;
  section 17 carries the full command table.
- [NEEDS CLARIFICATION: whether results printed to the caller fall under the constitution's untrusted-text rule or are an accepted exception; a maintainer decision stated here and in the runtime block, with neither sanitisation nor an exception claimed until it is made.]

Ask first: the command names and flags; any change to `ExitCode` or to the classification table; starting the
one-shot call before the runtime decision record is accepted.

**Depends on / blocks.** Landed in the script-first pull request; no blocker remains.

### saved-script commands (#579)

The storage layout, survival across regeneration, header validation and bounds are specified in
[[../skill/spec#14. Script-First Requirements]]; this subsection holds the
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
| FR-044.19 | WHEN the user checks saved scripts THE SYSTEM SHALL accept `--servers-dir` for the generated servers directory, expand a leading `~` in every directory option, and exit 2 on a `_meta.json` schema-version mismatch | must |

**Constraints.**

- [[../constitution#V. Security]]: writes go through the confined no-follow primitives of §8; script bodies and
  parser source excerpts never appear in output, errors or `Debug`; output text follows §12.
- `thiserror` in `mcp-skill`, `anyhow` only here (§13).

**Resolved and open decisions.**

- Resolved: `scripts list` and `scripts check` take `--server ID [--skills-dir]` (`check` also `--servers-dir`, #619), and `run --save NAME [--overwrite]` saves the script that ran (section 17). Problems found exit 5 (`CHECK_FAILED`). Saving happens only after the script exits 0.
- [NEEDS CLARIFICATION: a check of a skill whose server directory is missing or stale: reported as unreadable, distinct from a missing tool.]

Ask first: adding an `ExitCode` constant; the recipe header format (decision record, see the skill block).

**Depends on / blocks.** Landed in the script-first pull request; no blocker remains.

### drift check against recorded provenance (#580)

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

**Resolved and open decisions.**

- Resolved: the command is `drift <ServerFlags> [--name] [--progressive-output]`; it exits 5 (`CHECK_FAILED`) on drift or an endpoint that differs. `ttlMs` is reported, not honoured (section 16).
- [NEEDS CLARIFICATION: record per-tool digests in the `_meta.json` v3 registry (makes "changed" exact) or accept name-level reporting only.]

Ask first: changing `ServerInfo` public fields to carry `ttl_ms` and `cache_scope`; recording per-tool digests;
the exit code values.

**Depends on / blocks.** Landed in the script-first pull request; no blocker remains.

### Runtime hardening batch (#627, #625, #620, #628, #623, #618, #621, #619, #622, #610, #608, #611, #607, #624)

One change closes the issues above; their behaviour is recorded in sections 3, 10, 16, 17 and 18 and in the core, skill,
server and codegen specs. Behaviour changes for users: a symlinked `_meta.json` is refused everywhere; a schema-version
mismatch in `drift` and `scripts check` exits 2; a leading `~` is expanded in every directory option; a `run` stopped by
SIGINT or SIGTERM exits 130 or 143 (and, on Unix, 129 or 131 for SIGHUP or SIGQUIT, #646).

Deferred or excluded:
- #622 item 3 (kill the whole `tsc` process tree on timeout or cancel) was built later for Unix only (#633); the Windows
  half is a follow-up. Items 1, 2 and 4 are built (item 4 needed no change, the code already used `tokio::fs::metadata`).
- #615 and #475 are out of scope and stay open.
- `ServersDir`/`SkillsDir` moved to `mcp-execution-core` and `mcp-server` uses them (#634, #637); a bare `~` is now expanded.

## 20. 1.0 Public API Contract

> [!warning] Target requirement (1.0.0)
> Binds from the 1.0.0 tag; policy in [[../NFR-mcp-execution-2026-07-27#12. 1.0 Contract Requirements]] (`NFR-COM-030` to
> `NFR-COM-046`). The listing below was derived from `crates/mcp-cli/src` at 9473243. Items marked **proposal** are
> not decisions; open decisions are written as Ask First in [[../NFR-mcp-execution-2026-07-27#12.7 Open decisions (Ask First)]].

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-047.7 | THE SYSTEM SHALL expose from `mcp-execution-cli` only the surface listed in this section, mark `#[non_exhaustive]` every enum and growable struct not listed as closed, and keep every foreign type of its public signatures on the list below | must |

### Public surface (proposal, OD-5)

Today `lib.rs` exports `actions`, `cli`, `commands`, `formatters`, `runner` and `runtime` plus `ServerAction`:
about 25 public enums and about 109 public fields (#675). **Proposed** stable surface (option (a) of OD-5): the
parse types `cli::{Cli, Commands}` with `ServerAction`, so a crate or test can drive command parsing in-process
(#188), and `runner::{init_logging, execute_command, report_and_classify}`. **Proposed** `pub(crate)` or
`#[doc(hidden)]` workspace-internal: `commands`, `formatters`, `runtime` (including the helper protocol types such
as `ResultKind`) and the remaining `actions` types; tests that need them move into unit-test modules or use the
hidden items. The `test-fixtures` feature is not a public feature (`NFR-COM-033`).

### Binary contract

The command, flag, exit-code, JSON and environment contract is `NFR-COM-060` to `NFR-COM-065`, specified in
sections 2, 5 and 12 and in sections 16 to 19. `--progressive-output` is subject to OD-1.

### Foreign types (`NFR-COM-044`)

`clap::Parser` on `Cli` and `clap::Subcommand` on `Commands`; the `clap` major version is therefore coupled to this
crate's major version (OD-4).

### `#[non_exhaustive]` and closed sets (`NFR-COM-042`, `NFR-COM-043`)

If the proposal holds, only `Cli`, `Commands` and `ServerAction` are public, and `Commands` and `ServerAction` are
`#[non_exhaustive]` (a new subcommand is a compatible change, `NFR-COM-041`). Enums that stay internal are not
subject to the rule. The three-times-defined result concept is reduced to one (`NFR-COM-034`).

## 21. See Also

- [[../core/spec]] — shared config/redaction/exit-code primitives
- [[../introspector/spec]], [[../codegen/spec]], [[../files/spec]], [[../skill/spec]] — crates this CLI drives directly, without going through [[../server/spec]]
- [[#19. Script-First Requirements and Contracts]] — requirements of the measurement, runner, call, journal, dry run, helper, restrict, typecheck, saved-script and drift commands
- [[../decisions/ADR-569-runtime-strategy-single-mcp-client]] — gates the one-shot call
- [[../NFR-mcp-execution-2026-07-27#12. 1.0 Contract Requirements]] — 1.0 API, wire-format, CLI and release contract
