---
aliases:
  - mcp-execution-core spec
  - Core Types and Security
tags:
  - sdd
  - spec
  - core
  - security
created: 2026-07-27
status: documented
related:
  - "[[../constitution]]"
  - "[[../README]]"
  - "[[../introspector/spec]]"
---

# Block: Core Types & Security (`mcp-execution-core`)

> [!abstract]
> Foundation crate for the whole workspace. Path: `crates/mcp-core`.
> Zero intra-workspace dependencies — every other crate depends on this one,
> directly or transitively. Provides strong domain types, the shared error
> hierarchy, `ServerConfig` + its security validation, and small
> cross-cutting utility modules (`cli`, `metadata`, `path`, `redact`,
> `untrusted`) that other crates rely on for **consistency**, not just
> convenience — several of these exist specifically so two crates (or Rust
> and generated TypeScript) can't drift apart on the same security rule.

## 1. Responsibility

`mcp-execution-core` owns:

1. Domain newtypes (`ServerId`, `ToolName`) — never pass a raw `String` where
   one of these is expected.
2. The single `Error`/`Result` type used by every crate in the workspace.
3. `ServerConfig` (+ builder) — the one way to describe how to reach an MCP
   server (stdio subprocess, or HTTP/SSE endpoint), and the security
   validation (`validate_server_config`) that must pass before that
   description is used to spawn a process or open a connection.
4. Small shared modules used by more than one downstream crate to avoid two
   independent (and driftable) implementations of the same rule:
   - `cli` — `OutputFormat`, `ExitCode`, `ServerConnectionString` (used by
     `mcp-cli`).
   - `metadata` — the `_meta.json` sidecar schema shared by `mcp-codegen`
     (producer) and `mcp-skill`/`mcp-server` (consumers).
   - `path` — `sanitize_path_for_error` (used by `mcp-skill` and
     `mcp-server` for identical error-message redaction) and
     `contains_parent_dir` (used by both, plus `mcp-cli`, for identical
     `..`-traversal checks); `validate_path_segment` backs `ServerId`/
     `ToolName`'s own baseline path-segment invariant and, since #395, is
     also used internally by `confinement::resolve_confined_path` as its
     generic (looser) segment check. `first_disallowed_identifier_char`
     (issue #433) is the second, independent layer `ServerId`/`ToolName`
     also gate on — a UTS #39 `Identifier_Status=Allowed` Unicode-safety
     check, unrelated to path-segment structure (see below).
     `types::validate_server_id_slug` (issue #401) is the separate,
     stricter charset check both crates' `server_id` confinement now
     additionally enforces *before* calling into `resolve_confined_path`
     (see below) — `resolve_confined_path` itself stays generic and does
     not know about the slug rule.
   - `confinement` — `resolve_confined_path` (issue #395), the shared
     component-by-component resolve-and-confine filesystem walk used by both
     `mcp-skill::resolve_skill_output_path` and
     `mcp-server::output_dir::resolve_output_dir`, previously two independent
     copies of the same algorithm.
   - `redact` — `Debug`-redaction wrapper types (`RedactedItems`,
     `RedactedMapValues`, `RedactedUrl`) used by `ServerConfig` itself and
     by `mcp-cli`'s CLI-argument types.
   - `untrusted` — sanitization/boundary-wrapping helpers for
     attacker-controlled MCP-server metadata embedded into Markdown/LLM
     prompts, used by `mcp-skill` and `mcp-server`.

## 2. Public API Surface

### `ServerId` / `ToolName` (`src/types.rs`)

```rust
pub struct ServerId(String);
pub struct ToolName(String);
pub enum ServerIdError { InvalidFormat { id: String }, DisallowedCharacter { id: String, code_point: u32 } }
pub enum ToolNameError { InvalidFormat { name: String }, DisallowedCharacter { name: String, code_point: u32 } }
```
Both: `new(impl Into<String>) -> Result<Self, XxxError>`, `as_str(&self) -> &str`,
`into_inner(self) -> String`, `Display`,
`Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize`. `new` enforces two layered
invariants, in order: first `path::validate_path_segment` (the input must be a single
non-empty path segment — no `..`, no path separator, no root/prefix component — since both a
server id and a tool name are ultimately used to derive a filesystem path or file name
downstream), then `path::first_disallowed_identifier_char` (issue #433: every character must
be UTS #39 `Identifier_Status=Allowed`, checked via the `unicode-security` crate's
`GeneralSecurityProfile::identifier_allowed`). The second check exists because tool
names/server ids are attacker-controlled (a remote MCP server) and are rendered into
LLM-facing text (`introspect_server` summaries, generated `SKILL.md`); before #433 a hostile
server could publish near-identical tool names differing only by an invisible or bidi-control
character (e.g. `get_issue` vs. `get_issue\u{00AD}`) that `untrusted::sanitize_untrusted_text`
deliberately passes through by design (see its own doc comment). The Allowed set does **not**
detect homoglyphs (e.g. Cyrillic `а` U+0430 is Allowed and renders identically to Latin `a`) —
that is an explicitly out-of-scope, separate protection. A path-traversal attempt is still
reported as `InvalidFormat` (checked first), not `DisallowedCharacter`. `DisallowedCharacter`'s
`code_point` is the `u32` Unicode scalar value of the first rejected character; the `{:?}`
`Debug` formatting used in both variants' error messages escapes control/format code points in
the rejected string, so an attacker-controlled value can't smuggle an escape sequence into the
rendered error. There is no `From<String>`/`From<&str>` impl, and
`#[serde(try_from = "String")]` (backed by `impl TryFrom<String>` delegating to `new`) routes
`Deserialize` through `new` too — `new` is the only construction path, including through
deserialization, so neither invariant can be bypassed by an infallible conversion or a
direct-derive deserialize. Without `try_from = "String"`, a plain `#[derive(Deserialize)]`
would still be able to construct `Self(raw_string)` directly since the derived impl lives in
this same module and privacy doesn't block it — this is exactly the gap that made
`mcp_execution_introspector::ServerInfo`/`ToolInfo` (which derive `Deserialize` and hold a
`ServerId`/`ToolName` field) deserializable with an unvalidated id/name before this was added.

`ToolName::new` deliberately carries no length bound of its own — only the two character-level
invariants above. The root tool-name length limit lives one layer up, at
`mcp-introspector::MAX_TOOL_NAME_LEN`, per the "resource bounds cascade downward by value" cross-
block contract in [[../README#Cross-Block Contracts (the load-bearing ones)]]: adding a length
cap here would duplicate that root bound rather than deriving from it, and would invert the
contract by making a lower layer (`mcp-core`) reject data a higher layer (`mcp-introspector`) is
still willing to accept (issue #447).

Compatibility note (#433): a remote MCP server exposing a tool named with a space or
`@`/`+`/`(` now fails `ToolName::new` outright, which aborts `Introspector::discover_server`
for the whole server (`mcp-introspector` maps the failure to a graceful
`Error::ValidationError`, not a panic, but discovery still cannot proceed). This is a
deliberate fail-closed trade-off: Claude's own tool-name contract is `^[a-zA-Z0-9_-]{1,128}$`,
a strict subset of what remains accepted. Similarly, an `mcp.json` key containing a space is no
longer usable as a `ServerId` at all — `mcp-cli`'s config-key lookup
(`commands::common::get_mcp_server`) previously accepted such keys, deliberately not enforcing
the stricter slug rule ([[../cli/spec]]) since `ServerId::new`'s own baseline was already the
only gate; that baseline is now the Unicode-identifier-safe one described above.

Issues #432 and #431 originally closed this same gap with a second, denylist-based check in
`ToolName::new` (a `contains_invisible_payload_char` predicate covering the Tags block, bidi
embedding/override/isolate controls, the weaker bidi directional marks, and
zero-width/invisible-operator characters, plus a `contains_variation_selector` predicate for any
variation selector, stricter than `sanitize_untrusted_text`'s display-text run/total thresholds
since an identifier has no rendering to protect). Issue #444's UTS #39 allowlist (above)
independently closes the identical gap — none of the characters either predicate flagged carry
`Identifier_Status=Allowed`, so `first_disallowed_identifier_char` rejects all of them as a side
effect of accepting only the allowlisted set — so the denylist check was removed from
`ToolName::new` rather than kept alongside the allowlist gate (avoiding an unreachable second
`ToolNameError` variant that could never fire once the allowlist check, which runs first, had
already rejected the same input), and both predicates were deleted outright rather than kept as
unused public API: with the denylist call site gone, they had no in-tree caller left, the exact
dead-capability pattern PR #444 — the same PR that added this allowlist gate — removed
`Error::is_connection_error`/`is_timeout` for (issue #427).

`ServerId::new` gets the same single UTS #39 allowlist gate as `ToolName::new` (issue #444) and
never had the denylist check `ToolName::new` briefly carried. `sanitize_ts_string_literal` (the
one place a `ServerId`'s raw `&str` form reaches generated code) performs its own
length-bound/defense-in-depth pass regardless of either type's construction-time gate, since
generated code is a sink both hand-built and introspected values can reach
([[../codegen/spec#7. Injection Defense (Sanitization Pipeline)]]).

### `validate_server_id_slug` / `ServerIdSlugError` (`src/types.rs`, issue #401)

```rust
pub const MAX_SERVER_ID_LENGTH: usize = 64;
pub enum ServerIdSlugError { Empty, TooLong { len: usize, limit: usize }, InvalidCharacters }
pub fn validate_server_id_slug(id: &str) -> Result<(), ServerIdSlugError>;
// Rules: 1-64 bytes, only `[a-z0-9-]` (ASCII lowercase letters, digits, hyphen)
```
A stricter, opt-in invariant layered *on top of* `ServerId::new`'s baseline — not a replacement
for it, and not enforced by `ServerId::new`/`Deserialize` itself. `ServerId::new` deliberately
stays permissive (issue #311: a raw `mcp.json` key like `claude_ai_Gmail` is a valid `ServerId`
but not a valid slug — callers that only need a safe path segment, e.g. `mcp-cli`'s config-key
lookup, must keep accepting non-slug-shaped ids). Callers that need the id to become a directory
name or generated-code identifier — where entry validation and filesystem confinement must
agree on the exact same rule — call this function in addition to `ServerId::new`.

This is the single, authoritative home for the rule previously hand-rolled independently in
`mcp-execution-skill::validate_server_id`/`SkillServerIdError` (which now delegates to it —
`SkillServerIdError` is a re-export of `ServerIdSlugError`, not a separate mirror type, so the
two crates' error wording cannot drift apart) and imported piecemeal by
`mcp-execution-server`'s tool handlers. It also now backs both crates' `server_id`
output-confinement checks (`mcp-server::output_dir`, `mcp-skill::output_path`), which previously
confined using the looser `validate_path_segment` even though entry validation already gated
with the stricter rule — see [[../server/spec#Output directory resolution]] and
[[../skill/spec]].

### `Error` / `Result<T>` (`src/error.rs`)

```rust
pub enum Error {
    ConnectionFailed { server: String, source: Box<dyn Error+Send+Sync> },
    SecurityViolation { reason: String },
    Timeout { operation: String, duration_secs: u64 },
    SerializationError { message: String, source: Option<serde_json::Error> },
    InvalidArgument(String),
    ValidationError { field: String, reason: String },
    ScriptGenerationError { tool: String, message: String, source: Option<Box<dyn Error+Send+Sync>> },
    ResourceLimitExceeded { resource: ResourceKind, actual: usize, limit: usize },
    DuplicateGeneratedFilePath { path: String },
}
pub type Result<T> = std::result::Result<T, Error>;

pub enum ResourceKind {
    ToolCount { server_id: ServerId },
    ToolNameLength,
    DescriptionLength { tool_name: String },
    InputSchemaSize { tool_name: String },
    OutputSchemaSize { tool_name: String },
    GeneratedOutputSize,
    GeneratedFileCount,
}
```
`ResourceKind` (`error::ResourceKind`, re-exported at crate root) is a closed set replacing a
free-form `resource: String` (issue #317): each variant's `Display` reproduces the same
human-readable wording call sites used to build by hand (e.g. `ToolCount` renders `"tool count
for server '{id}'"`), so `Error::ResourceLimitExceeded`'s own message is unchanged in substance.
Covers `mcp-core` only. `mcp-files::FilesError::ResourceLimitExceeded` closed its own free-form
`resource: String` separately (issue #343) with a local `FilesResourceKind` enum rather than
adding variants here: `mcp-files` has no direct dependency on `mcp-core` (only a transitive one
via `mcp-execution-codegen`), so sharing this enum would mean adding a new direct dependency on
`mcp-core` for a single error variant — see [[../files/spec#7. Error Conditions]].
Most variants have an `is_*` predicate (`is_security_error`, `is_validation_error`,
`is_script_generation_error`, `is_resource_limit_exceeded`,
`is_duplicate_generated_file_path`). No predicate exists for
`InvalidArgument`/`SerializationError` — callers match directly.
`ConnectionFailed`/`Timeout` have no `is_connection_error`/`is_timeout` predicate either
(removed, issue #427, mirroring #199/#202's identical dead-predicate-removal precedent): their
only real call site, `mcp-cli`'s `classify_core_error`, is an exhaustive `match` over every
`Error` variant with no wildcard arm, so it always matched each variant by name directly rather
than through a predicate — adding a variant there is a compile error at that `match`, a
guarantee an `if`/`else if` predicate chain would silently lose. `ScriptGenerationError.source`
is the vehicle for preserving an inner `Error`'s own classification through wrapping (see
`mcp-cli`'s `classify_core_error`, which recurses into it — [[../cli/spec]]).

`Error::DuplicateGeneratedFilePath { path: String }` (issue #312) is raised by
`mcp-execution-codegen`'s `GeneratedCode::add_file` when a second file is added at a path
already present in the same generated-code collection (e.g. a sanitized tool name colliding
with a generator's own reserved output filename like `index`) — a silent overwrite that used
to lose a generated file with no signal to the caller now fails loudly instead. `add_file`'s
signature changed from `fn add_file(..)` to `fn add_file(..) -> Result<()>` accordingly; every
in-tree call site (`mcp-codegen`'s own generator, `mcp-files`'s `FilesBuilder`) was updated to
handle the new `Result`.

### `ServerConfig` / `ServerConfigBuilder` / `Transport` (`src/server_config.rs`)

```rust
pub enum Transport {
    Stdio { command: String, args: Vec<String>, env: HashMap<String,String>, cwd: Option<PathBuf> },
    Http { url: String, headers: HashMap<String,String> },
    Sse { url: String, headers: HashMap<String,String> },
}
pub struct ServerConfig {
    transport: Transport,           // private
    connect_timeout: Duration,      // private, default 30s, all transports
    discover_timeout: Duration,     // private, default 30s, all transports
}
```
`Transport` (issue #313) carries each transport's fields as enum payload rather than as flat,
always-present fields on `ServerConfig`: a `Stdio` config has no `url`/`headers` fields to
populate, and an `Http`/`Sse` config has no `command`/`args`/`env`/`cwd` fields — the
illegal cross-transport combination (e.g. `args` set on an `Http` config) is unrepresentable,
not merely unvalidated. `ServerConfig`'s two fields are private; read access goes through
`transport()`, `command()`/`args()`/`env()`/`cwd()`/`url()`/`headers()` (each returning an
empty/`None` default for the transport that doesn't carry that field), and
`connect_timeout()`/`discover_timeout()`. `command()` returns `Option<&str>` (`None` for
`Http`/`Sse`, issue #317) rather than `&str` with an empty string standing in for "not
applicable" — mirroring `url()`, which was already `Option<&str>` (`None` for `Stdio`).

Builder methods: `command`, `arg`, `args`, `env`, `environment`, `cwd`,
`http_transport(url)`, `sse_transport(url)`, `url`, `header`, `headers`,
`connect_timeout`, `discover_timeout`, `build() -> Result<ServerConfig>`. The builder itself
still accumulates all six transport-specific fields as flat, independently-settable state
(via a private `TransportKind` discriminant) — only the *assembled* `ServerConfig` enforces
the enum shape; `build()` picks the right `Transport` variant's fields from what was set.

`build()` = `build_structural()` (presence checks: `command` required
non-empty for stdio, `url` required for http/sse) **then**
`validate_server_config(&config)` — full security validation runs
unconditionally inside `build()`, so a `ServerConfig` obtained through the
builder cannot exist without having passed it. Since #313 this is also a
**type-level** guarantee, not just a builder-level one: both fields are private, and
`ServerConfig`'s `Deserialize` impl is hand-written to deserialize into a private shadow
shape and then run `validate_server_config` before returning an `Err` on failure — so a
struct literal (impossible outside this module — fields are private) or
`serde_json::from_str` can no longer bypass validation. See [[#Defense in depth]].

`ServerConfig` and `ServerConfigBuilder` both hand-write `Debug` to redact
`args` (wholesale, via `RedactedItems`), `env`/`headers` (values only, keys
kept, via `RedactedMapValues`), and `url` (userinfo + query stripped, via
`RedactedUrl`); `command`/`cwd` go through `sanitize_path_for_error`.
**`Serialize` is not redacted** — a serialized `ServerConfig` carries real
secrets and must never be logged.

### `validate_server_config` and friends (`src/command.rs`)

```rust
pub fn validate_server_config(config: &ServerConfig) -> Result<()>;
pub fn validate_url_scheme(url: &str) -> Result<()>;
pub const fn forbidden_chars() -> &'static [char];
pub const fn forbidden_env_names() -> &'static [&'static str];
pub const fn forbidden_env_prefix() -> &'static str; // "DYLD_"
```
Constants (all `pub`): `MAX_ARG_COUNT` (256), `MAX_ARG_LEN` (4096),
`MAX_ENV_COUNT` (256), `MAX_ENV_VALUE_LEN` (32 KiB), `MAX_HEADER_COUNT`
(128), `MAX_HEADER_VALUE_LEN` (8 KiB), `MAX_URL_LEN` (8 KiB).

Validation order inside `validate_server_config` (all run unconditionally,
regardless of transport, before transport-specific checks):

1. `validate_stdio_size_bounds`/`validate_network_size_bounds` — command/url/arg/env/header
   count and length caps (CWE-400 backstop), dispatched per `Transport` variant. Since #313
   each only needs to check the fields that variant actually has — there is no cross-transport
   bypass to guard against, because e.g. a `Stdio` config has no `headers` field to populate.
2. Transport dispatch:
   - `Stdio` → `validate_command_string` (forbidden shell metachars:
     `; | & > < \` $ ( ) \n \r`) on `command` and each `arg`; absolute-path
     commands additionally require existence + executable bit (Unix); env
     names are first required to match the POSIX/Windows identifier charset
     `^[A-Za-z_][A-Za-z0-9_]*$` (rejects empty names, names starting with a
     digit, and any non-ASCII character), then checked against
     `forbidden_env_names()`/`forbidden_env_prefix()`, ASCII-case-insensitively
     (so `Path`/`path`/`PATH` are all rejected) — Windows treats environment
     variable names as case-insensitive at the OS/`CreateProcess` level, so a
     case-varied spelling would otherwise bypass this list while still
     functioning as a real override at spawn time. The charset check runs
     first because Windows' own case folding uses the OS's Unicode uppercase
     table, which is broader than ASCII-only folding (e.g. `ı` U+0131 folds to
     `I`, `ſ` U+017F folds to `S`); a forbidden name spelled with such a
     confusable in place of an ASCII letter would otherwise pass the
     ASCII-only comparison here yet still resolve as the forbidden name on a
     real Windows host, so it is rejected outright as not being a valid
     identifier to begin with, rather than chasing individual Unicode
     confusables.
   - `Http`/`Sse` → `validate_network_config`: `url` required,
     `validate_url_scheme` (must be `http://`/`https://`, case-insensitive),
     header name (RFC 7230 `tchar` charset) and value (no control chars)
     checks, duplicate header names rejected case-insensitively.
3. `validate_timeout` on `connect_timeout`/`discover_timeout` — must be
   `> 0` and `<= MAX_TIMEOUT` (10 minutes); **no infinite-timeout option is
   supported by design** (an unbounded wait would let a hung server block
   this non-interactive tool forever).

Env name charset (checked before the forbidden-name list below): must match
`^[A-Za-z_][A-Za-z0-9_]*$`. A name outside this charset — including one built
from a non-ASCII Unicode case-confusable of a forbidden name, e.g. `NODE_OPTıONS`
using `ı` (U+0131) in place of `I` — is rejected as `Error::SecurityViolation`
even though it is not an exact (case-insensitive ASCII) match against the list
itself.

Forbidden env names (exact match, case-insensitive): `LD_PRELOAD`,
`LD_LIBRARY_PATH`, `LD_AUDIT`, `DYLD_INSERT_LIBRARIES`, `DYLD_LIBRARY_PATH`,
`DYLD_FRAMEWORK_PATH`, `PATH`, `NODE_OPTIONS`, `BASH_ENV`, `PYTHONPATH`,
`PYTHONSTARTUP`, `RUBYOPT`, `PERL5OPT`, `JAVA_TOOL_OPTIONS`; plus any name
with prefix `DYLD_` (also case-insensitive). This list is explicitly
documented as an **accidental-misconfiguration guard, not a sandbox
boundary** — it does not protect against a malicious command/binary itself.

Every rejection error omits the offending value from its message when that
value could be secret-shaped (a misparsed `--api-key sk-...` argument, a
header name/value) — this is enforced by tests, not just convention.

### `cli` module (`src/cli.rs`)

```rust
pub enum OutputFormat { Json, Text, #[default] Pretty } // FromStr, Display
pub enum LogFormat { #[default] Text, Json } // FromStr, Display; mirrors OutputFormat's shape
pub const LOG_FORMAT_ENV_VAR: &str = "MCP_EXECUTION_LOG_FORMAT";
// LogFormat::resolve(flag: Option<LogFormat>, env_value: Option<&str>) -> LogFormat: flag wins
// unconditionally (env not even inspected on Some); empty/whitespace env is treated as unset;
// an unrecognized env value falls back to Text. Returns only the resolved format -- no rejected
// raw value threaded through, since no production caller logs it (see below). Pure and
// process-env-free by design: callers pass `std::env::var(LOG_FORMAT_ENV_VAR).ok()` in
// themselves, which keeps this testable without mutating process env.
// LogFormat::parse_env(raw: &str) -> Option<LogFormat>: `resolve`'s env-parsing step, exposed
// separately -- None for both an empty/whitespace value and an unrecognized one.
// LogFormat::is_invalid_env_value(raw: &str) -> bool: true only for a non-empty value
// `parse_env` rejects, i.e. the "should a caller warn about this" question `resolve` itself
// doesn't answer. Lets a caller (both `mcp-cli`'s `init_logging` and `mcp-server`'s
// `resolve_log_format`/`log_format_env_is_invalid`) decide whether to warn without `resolve`
// carrying a second, production-dead return value -- an earlier `(LogFormat, Option<String>)`
// shape was reworked away for exactly that reason.
pub struct ExitCode(i32); // SUCCESS=0, ERROR=1, INVALID_INPUT=2, SERVER_ERROR=3, TIMEOUT=4
// ExitCode::from_i32(code: i32) -> Option<ExitCode>: None outside 0..=255 (std::process::exit's
// actual valid range), instead of accepting any i32 unchecked.
pub struct ServerConnectionString(String); // charset [A-Za-z0-9-_./:], <=256 chars, no control chars
```
`ServerConnectionString::new` is a defense-in-depth CLI-input validator
(command-injection / CRLF-injection prevention); note it is **not** what
`mcp-cli` actually uses for `--from-config`/`server` values today (those
flow through `ServerId`/`ServerConfig` instead) — it exists as reusable,
tested infrastructure in this crate's public surface.

`LogFormat` (issue #399) lives here rather than in `mcp-cli` specifically
because `mcp-server` needs it too and does not depend on `mcp-cli`: putting
it here means `mcp-core` gains a second CLI-adjacent enum but no new
dependency (it's a plain enum), and the two binaries' `--log-format`
flag/`MCP_EXECUTION_LOG_FORMAT` fallback logic can never drift apart the
way two independently hand-rolled copies could. Both
`mcp-cli::runner::init_logging` and `mcp-server`'s `resolve_log_format`
call `LogFormat::resolve` and then build the same `.boxed()`-branched
`fmt::layer()`/`fmt::layer().json()` pair — see
[[../cli/spec#5. runner.rs|cli spec §5]] and
[[../server/spec#Logging & CLI Surface|server spec §9]].

### `metadata` module (`src/metadata.rs`)

```rust
pub const METADATA_SCHEMA_VERSION: u32 = 1;
pub const METADATA_FILE_NAME: &str = "_meta.json";
pub struct ServerMetadata { schema_version: u32, server_id: ServerId, server_name: String, server_version: String, tools: Vec<ToolMetadata> }
pub struct ToolMetadata { name: ToolName, typescript_name: String, category: Option<String>, keywords: Vec<String>, description: Option<String>, parameters: Vec<ParameterMetadata> }
pub struct ParameterMetadata { name, typescript_type, required, description: Option<String> }
```
`server_id`/`name` are `ServerId`/`ToolName` (issue #317, previously bare `String`); both
newtypes' derived `Serialize`/`Deserialize` round-trip through a plain JSON string, so this is
not a wire-format change. `typescript_name` stays `String` — it is a generated TypeScript
identifier, not itself an MCP tool name.

This is the **wire contract** between `mcp-codegen` (producer, writes
`_meta.json`) and `mcp-skill`/`mcp-server` (consumers). Bumping
`METADATA_SCHEMA_VERSION` is the intended way to signal a breaking shape
change; consumers compare it and fail loudly on mismatch (see
[[../skill/spec#ScanError]]).

### `path` module (`src/path.rs`)

```rust
pub fn sanitize_path_for_error(path: &Path) -> String; // redacts home dir to "~", falls back to scrubbing bare username
pub fn validate_path_segment(segment: &str) -> Option<Component<'_>>; // single plain component, no "..", no separator
pub fn first_disallowed_identifier_char(s: &str) -> Option<char>; // first char failing UTS #39 Identifier_Status=Allowed
pub fn contains_parent_dir(path: &Path) -> bool; // true if any component is `..`
```
On Windows/macOS, the home-directory component comparison inside `sanitize_path_for_error`
(`components_match`/`replace_case_aware`) is both Unicode-case-insensitive (`str::to_lowercase`,
issue #417) and Unicode-normalization-insensitive (NFC via the `unicode-normalization` crate,
issue #416): a username that arrives pre-composed (NFC) from one source and decomposed (NFD)
from another — the same rendered text, different byte sequence — still matches, on both the
primary `strip_home_prefix`/`components_match` path and the `scrub_username`/`replace_case_aware`
fallback. `replace_case_aware` NFC-normalizes `haystack` and `needle` as whole strings *before*
windowing, not per-candidate-window: an earlier version normalized only inside each window, which
left `needle`'s raw (pre-normalization) char count mismatched against a differently-sized
normalized span in `haystack` (e.g. an NFC needle against an NFD-spelled haystack span), silently
missing the match. Every slice point used to build the redacted output is still that
already-normalized `haystack` string's own char boundary, so this cannot panic or mis-slice even
though normalization (like the case fold) can change a character's encoded byte length. One
observable side effect of normalizing `haystack` as a whole: the `scrub_username` fallback's
*entire* returned string is NFC-normalized, not just the redacted span — an NFD-spelled segment
elsewhere in the path comes back precomposed. This is harmless for display, but on Windows, where
NTFS treats an NFC- and an NFD-spelled filename as different files, the rendered path is not
guaranteed to name a file that exists on disk under that exact spelling. The primary
`strip_home_prefix` path has no such effect — it emits components verbatim.
`normalize_and_fold` (used by both functions) also re-normalizes *after* folding, not just before:
`str::to_lowercase` can turn an already-NFC string into a non-NFC one for a character whose
lowercase has a precomposed form but whose uppercase does not (e.g. `"J\u{30C}"`, no precomposed
uppercase, folds to `"j\u{30C}"`, which is not NFC since precomposed `"\u{1F0}"` "ǰ" exists) — the
trailing re-normalization catches this case too. The one residual limitation left in
`replace_case_aware`: its window is sized to `needle`'s *normalized* char count, but the
comparison runs on the further-*folded* form, whose char count can differ from the normalized one
— so a needle/haystack pair whose folded forms only line up at a different char count than their
normalized forms is missed. This covers both German "ß" needle against a haystack spelled "ss"
and a normalization-adjacent case introduced by the post-fold re-normalization itself (e.g. needle
`"J\u{30C}an"` against haystack `"\u{1F0}an"`) — an accepted, pre-existing limitation (see the
module's tests), not a regression from either the #417 or #416 fix.

`first_disallowed_identifier_char` (issue #433) is a sibling check, not a replacement for
`validate_path_segment` and deliberately not folded into it: it says nothing about path
separators/`..`/root components, and `validate_path_segment` says nothing about Unicode
identifier safety. `ServerId::new`/`ToolName::new` apply both, in that order (structural check
first, so a traversal attempt keeps reporting `InvalidFormat`). It is *not* used by
`confinement::resolve_confined_path` or either crate's `server_id` output-confinement path —
those stay scoped to filesystem-path safety, which `validate_path_segment`/
`validate_server_id_slug` already cover; Unicode-identifier safety is a display/LLM-spoofing
concern specific to `ServerId::new`/`ToolName::new`'s construction-time gate, not a filesystem
concern. Backed by the `unicode-security` crate's `GeneralSecurityProfile::identifier_allowed`
(Unicode 16.0 tables); does not detect homoglyphs (see `ServerId`/`ToolName`'s own doc comment
above for the full rationale).

`validate_path_segment` backs `ServerId::new`/`ToolName::new`'s own baseline invariant (see
above) and, since #395, is used internally by `confinement::resolve_confined_path` as its own
generic, looser structural check on the `segment` (`server_id`) it's given —
`resolve_confined_path` stays a generic reusable primitive, not coupled to the `server_id`
domain concept specifically. It is *no longer* the primary rule
`mcp-skill::resolve_skill_output_path` and `mcp-server::output_dir::resolve_output_dir` gate
`server_id` confinement with directly — since issue #401, both call `types::validate_server_id_slug`
up front, before calling into `resolve_confined_path` at all (the same rule their respective
entry-point handlers already validate with), so the domain-specific slug rule and the generic
structural rule both apply, in that order. Neither confinement helper calls
`validate_path_segment` directly anymore; `resolve_confined_path`'s internal re-check (and, on
its `ConfinementError::InvalidSegment` path, each crate's `From<ConfinementError>` impl
re-deriving a `ServerIdSlugError` by calling `validate_server_id_slug` again) exist purely as
defense-in-depth against a `server_id` that reaches the walk some other way, or after a future
change loosens the caller-side check — not because either helper still leans on
`validate_path_segment` as its primary gate.
`contains_parent_dir` (issue #289) is the shared `..`-only check used by
`mcp-skill::output_path`, `mcp-server::output_dir`, and
`mcp-cli::commands::skill::has_path_traversal` for the narrower "is any
component `..`" question, previously three byte-for-byte-identical copies.

### `confinement` module (`src/confinement.rs`)

```rust
pub enum ConfinementTarget<'a> { Directory(&'a OsStr), File(&'a OsStr) }
pub enum ConfinementError {
    InvalidSegment { segment: String },
    SegmentIsSymlink { path: String },
    Escape { path: String },
    NotADirectory { path: String },
    WrongTargetKind { path: String },
    CreateDir { path: String, source: std::io::Error },
    Io(#[from] std::io::Error),
}
pub async fn resolve_confined_path(
    base_dir: &Path,
    segment: &str,
    relative_dirs: &Path,
    target: Option<ConfinementTarget<'_>>,
) -> Result<PathBuf, ConfinementError>;
```

Issue #395: `mcp-skill::resolve_skill_output_path` and
`mcp-server::output_dir::resolve_output_dir` independently implemented the same
component-by-component resolve-and-confine walk (validate the `server_id` segment, canonicalize
`base_dir` once, confine-and-create the segment directory rejecting any pre-existing symlink at
it outright, then confine-and-create each further directory component leniently — following an
existing symlink only if it still resolves inside the segment directory — before
confinement-checking, but deliberately not creating, the terminal component). `resolve_confined_path`
is that walk extracted once; the two crates differ only in what an *absent* input path means and
in whether the terminal component is a directory or a file, both of which stay at the call site
(see [[../server/spec#6. Output Directory Resolution (`output_dir.rs`)]] and
[[../skill/spec#8. `resolve_skill_output_path` — Path Confinement]]).

`ConfinementTarget` names the walk's terminal component without creating it: `Directory` is
resolved and canonicalized (the typical caller publishes it itself via an atomic staged rename),
`File` is confinement-checked but left uncanonicalized (the caller is about to create it) —
this asymmetry is deliberate and preserved from both crates' pre-consolidation behavior, not
unified away. `target: None` returns the walked `relative_dirs` chain itself, with nothing beyond
the segment directory created.

`ConfinementError` is a closed set every caller maps with a **total, 1:1** `From` impl into its
own pre-existing, byte-identical error enum — `mcp-server::OutputDirError` and
`mcp-skill::OutputPathError` — rather than matching on `ConfinementError` directly. The single
`ConfinementError::WrongTargetKind` variant carries two different truthful call-site meanings:
`OutputDirError::NotADirectory` (the terminal `resolve_output_dir` walks is always a directory)
and `OutputPathError::NotAFile` (the terminal `resolve_skill_output_path` walks is always a
file) — one core variant, zero unreachable arms in either `From` impl. `ConfinementError::InvalidSegment`/`SegmentIsSymlink`/`Escape`/`NotADirectory`/`CreateDir`/`Io` map
1:1 by name to each crate's own variant of the same name (`InvalidSegment` → `InvalidServerId`,
`SegmentIsSymlink` → `ServerDirIsSymlink`/`ServerIdIsSymlink`). The absolute-path, `..`-component,
and file-name pre-checks (`relative_subpath`/`relative_target`) are **not** part of this module —
they stay in each crate, since an absent path means "use the segment directory as-is" for
`mcp-server` but "use the default `SKILL.md`" for `mcp-skill`, and only a crate-specific helper
can express that.

### `redact` module (`src/redact.rs`)

```rust
pub const REDACTED_PLACEHOLDER: &str = "<redacted>";
pub struct RedactedMapValues<'a>(pub &'a HashMap<String,String>); // Debug: keys visible, values redacted
pub struct RedactedItems<'a>(pub &'a [String]);                  // Debug: every entry redacted wholesale
pub struct RedactedUrl<'a>(pub &'a str);                         // Debug: userinfo + query redacted, host/path kept
pub fn redact_urls_in_text(text: &str) -> String;                // scans free text, redacts every URL-shaped token found
```
`RedactedUrl` is deliberately parse-free (no `url` crate dependency) and
redacts the **whole** input if it cannot unambiguously identify the
authority boundary (e.g. an unencoded `/` or `?` inside userinfo) or if the
scheme contains an invalid character — fails closed toward over-redaction
rather than partial leakage.

`redact_urls_in_text` (issue #353) handles the case `RedactedUrl` doesn't: a
URL buried inside already-assembled prose rather than isolated behind its
own field — the shape a `reqwest`/`rmcp` transport error's `Display` takes,
embedding the full request URL (query string included) inline in a
sentence. It locates each `scheme://…` token by walking left over
scheme-legal characters from `://`, walking right to the first whitespace,
control character, or wrapping-punctuation character (quote, backtick,
paren — or text's end), and trimming trailing sentence punctuation (`,` `.`
`;` `:` `\`) — the backslash covers a JSON string serializer's escaping
backslash before a closing `"` around a quoted URL, which would otherwise be
absorbed into the token and deleted, leaving unescaped, invalid JSON; the
resulting token is redacted by handing it to `RedactedUrl`'s
own `Debug` impl, so the *masking* decision — what counts as
authority/query, what gets hidden — can never drift between the two.

The token-*boundary* rules are deliberately looser than `RedactedUrl`'s own
"fails closed" stance, not the same: `RedactedUrl` redacts a whole field in
full whenever it's ambiguous, but this function only ever sees a substring
of prose it must first isolate, so it fails toward capturing more text into
a token rather than less — RFC 3986 IP-literal delimiters (`[`/`]`, needed
verbatim for an IPv6 authority) and every other RFC 3986 "unsafe" character
are deliberately *not* terminators, since ending the token on one of them
would cut it short before a query string embedded raw (unescaped) right
after — the exact shape a dependency's `Display` impl produces. The
remaining terminator set (whitespace, control characters, quote/backtick/
paren wrappers) is a heuristic, not a parser: a raw instance of *any* of
those characters — not just the quote/backtick/paren wrappers, though
whitespace/control chars surviving raw inside a secret is the less likely
case, since a real `reqwest`-sent URL would already have percent-encoded
them — appearing *inside* the secret itself (rather than used by the
surrounding log line to wrap the URL) still ends the token early — the same
class of unencoded-delimiter ambiguity `RedactedUrl` documents for an
unencoded `/` or `?` inside userinfo, inherited here rather than newly
introduced, since resolving it needs a real URL parser this module
deliberately does not depend on.

One widening step keeps parity with `RedactedUrl`'s malformed-scheme rule:
if a non-scheme, non-terminator character glues more text onto the scheme
run's left (e.g. `ghp_leakedtoken://host.com/`, where `_` breaks the
scheme-char walk without being a terminator either), the token is widened
left to the nearest terminator — bounded to text not yet emitted by a prior
token, so it can never rewind past output already committed — before
redaction, so the widened "scheme" fails `RedactedUrl`'s validity check and
the whole run is redacted, not just its scheme-shaped tail. Known accepted
limitation, inherited unchanged from `RedactedUrl`: a secret glued to a URL
by scheme-*legal* characters (e.g. `Bearer sk-abc.https://h/p?t=1`) is
absorbed into the token's "scheme" and survives, exact parity with what
`RedactedUrl` itself does on that same string.

### `untrusted` module (`src/untrusted.rs`)

```rust
pub const MAX_UNTRUSTED_FIELD_LEN: usize = 500;
pub fn sanitize_untrusted_text(s: &str, max_len: usize) -> String; // flattens control chars, U+2028/U+2029, bidi override/isolate controls, and U+200B to spaces; removes bidi marks, the Unicode Tags block (U+E0000-U+E007F), U+FEFF, and U+2060-U+2064 entirely; leaves U+200C/U+200D untouched; THEN, on the filtered result, drops all variation selectors if their whole-value total exceeds 16, else drops any run of more than 2 consecutive ones; truncates by char count
pub fn wrap_untrusted_block(context: &str, body: &str) -> String;  // escapes &, <, > in body; wraps in <untrusted-data>...</untrusted-data>
```
Threat model: an introspected MCP server's tool names/descriptions/keywords
are attacker-controlled from this project's perspective. Both functions
exist because `mcp-skill` (SKILL.md + prompt generation) and
`mcp-server` (introspection summaries returned to Claude) both embed this
data into text an LLM later reads as instructions — see
[[../server/spec#Prompt injection defense]] and
[[../skill/spec#Prompt injection defense]].

Since issue #422, `sanitize_untrusted_text` also neutralizes the Unicode bidirectional-formatting
characters a "Trojan Source"-style attack relies on to visually reorder or relabel text for a
human reader without changing its logical byte order, with two different treatments depending on
whether the character alone can reorder/join text: the explicit embedding/override controls
U+202A-U+202E (LRE, RLE, PDF, LRO, RLO) and the isolate controls U+2066-U+2069 (LRI, RLI, FSI,
PDI) are *replaced with a space* (removing them outright could join two tokens that were only
separated by the removed character); the weaker directional marks U+200E/U+200F (LRM/RLM) and
U+061C (ALM), which only set direction for adjacent neutral characters and cannot reorder or join
anything on their own, are *removed entirely* so a legitimate mark inside otherwise-correct RTL
text doesn't inject a spurious word break. None of these are covered by `char::is_control` (they
are Unicode `Cf` format characters, not control characters), so they previously passed through
this function unmodified.

Since issue #425, `sanitize_untrusted_text` also neutralizes a specific, enumerated set of
invisible-character smuggling channels neither #422 nor `char::is_control` covers — not
"invisible/zero-width Unicode characters" as a general class (variation selectors
U+FE00-U+FE0F/U+E0100-U+E01EF and several other format characters remain unhandled; see the
limitation note below):

- The Unicode Tags block U+E0000-U+E007F (U+E0001 LANGUAGE TAG plus the U+E0020-U+E007F TAG
  characters, which mirror ASCII 0x20-0x7F and can encode an entire ASCII payload invisible to a
  human reviewer but legible to an LLM tokenizer — a known prompt-injection smuggling technique),
  U+FEFF (ZERO WIDTH NO-BREAK SPACE / BOM), and the contiguous invisible-operator run
  U+2060-U+2064 (WORD JOINER, FUNCTION APPLICATION, INVISIBLE TIMES, INVISIBLE SEPARATOR,
  INVISIBLE PLUS) are *removed entirely*, the same treatment as #422's bidi marks: none of them
  has a glyph in any mainstream font (no visible gap for a space to preserve) and none denotes a
  break opportunity, so removing one cannot join two tokens a renderer would otherwise show apart.
- U+200B (ZERO WIDTH SPACE) is instead *replaced with a space*, the same treatment as #422's bidi
  embedding/override controls, not removed like the characters above: it is itself a Unicode
  line-break opportunity and the conventional word separator in Thai/Lao/Khmer/Japanese text, so
  removing it outright would reproduce the exact join hazard (`a\u{200B}b` -> `"ab"`) those
  controls are spaced rather than removed to avoid.
- U+200C (ZERO WIDTH NON-JOINER) and U+200D (ZERO WIDTH JOINER) are deliberately left untouched:
  unlike every character above, they are orthographically load-bearing (Persian/Indic script
  joining behavior, emoji ZWJ sequences), so stripping or spacing them would corrupt legitimate
  text rather than only closing an attacker's invisible channel. This is a documented divergence
  from `ServerConnectionString::new` (`crates/mcp-core/src/cli.rs`), whose stricter ASCII-only
  allowlist rejects them outright at a validation boundary with no legitimate-content concern to
  weigh against.

Since issue #431, `sanitize_untrusted_text` also mitigates the variation-selector channel
(U+FE00-U+FE0F "VS1-VS16" and the Variation Selectors Supplement U+E0100-U+E01EF), adjacent to the
Tags block above and left out of #425's scope at the time. Unlike the Tags block and zero-width
characters, variation selectors carry genuine rendering semantics — emoji-presentation selection
and Ideographic Variation Sequences (IVS) for CJK text — so unconditional stripping was rejected
as a fix: it would visibly alter legitimate content. Instead, two length-based checks run *after*
the per-character filter above, not before (ordering matters — see below), on the filtered
string:

1. **Whole-value total** (`MAX_TOTAL_VARIATION_SELECTORS`, private, currently 16): if the value's
   total variation-selector count, summed across every run regardless of how it's distributed
   across base characters, exceeds this bound, every variation selector in the value is dropped.
2. **Per-run threshold** (`MAX_VARIATION_SELECTOR_RUN`, private, currently 2), applied only when
   the total stays under the bound above: a run of at most 2 consecutive variation selectors is
   left untouched (covers the normal single-selector case plus the occasional legitimate second
   selector); a longer run is dropped in full.

Both constants and the "drop the whole run/value, not just the excess" choice are documented in
code in `src/untrusted.rs`.

A per-run-only check (the original #431 implementation) is not sufficient on its own: an attacker
who distributes the payload as many short runs — each at or below the per-run threshold, each
after a different base character — defeats a per-run check entirely, since every individual run
is indistinguishable from ordinary emoji-presentation/IVS use in isolation. Measured during
review: 2 selectors per base character over 59 characters of ordinary prose smuggled 96 payload
characters this way, denser than the Tags-block channel this mitigation complements — this is not
an edge case, it is the straightforward way to use the channel once the per-run threshold is
known. The whole-value total closes this: it is computed independently of how the selectors are
grouped into runs, so no distribution strategy evades it.

> [!warning]
> The two checks above must run **after** the per-character filter (bidi marks, Tags block,
> U+FEFF, U+2060-U+2064), not before. Every character that filter removes entirely is itself
> invisible, so an attacker can interleave one of those characters between variation selectors to
> split what is really one long run into several separator-divided, sub-threshold pieces with
> zero visual cost. Detecting runs *before* the filter sees each piece as independently
> under-threshold and passes it; the filter then deletes the separators afterward and the pieces
> silently re-join into the original, full-length run in the output. Running detection on the
> already-filtered string means it sees the same adjacency the filter's removals actually
> produce, so a run can't be disguised by characters that won't be there in the final output.

**Known limitation**: the whole-value total is a global count, not a semantic check — it cannot
distinguish "16 variation selectors forming 16 legitimate independent emoji" from "16 variation
selectors carrying 16 units of an encoded payload distributed one-per-base-character," and treats
both the same way once the bound is crossed. Raising the bound to reduce false positives on
heavily emoji-decorated legitimate text directly raises the smuggling capacity available below
it; the current value (16, raised from an initial 8 — see below) is chosen to keep that capacity
small (each variation selector can only encode a value from a small, fixed code-point set, so 16
of them carry at most a handful of bytes, not a meaningful instruction) while tolerating a
realistic amount of independent legitimate emoji in one field. Closing this fully would require a
semantic check (is this base character + selector combination a real, assigned emoji sequence or
IVS?) this module deliberately does not implement.

The bound was raised from 8 to 16 after critic review (issue #431, finding M6) found the tighter
value false-positived on ordinary content: a description with 9 presentation-selected emoji (a
realistic count for a tool description listing several capabilities, each with its own leading
icon) lost every selector under the 8 bound. The security delta between 8 and 16 is negligible —
neither carries a meaningful payload — while the false-positive rate on legitimate multi-emoji
text differs substantially, so the wider bound is a strictly better trade-off, not a weaker one.

**Known limitation, second channel (issue #431, critic finding M5)**: the bound above is
per-field — each call to `sanitize_untrusted_text` gets its own independent allowance of up to
16 surviving variation selectors. A server with many tools, each contributing a sanitized name,
description, keyword list, and per-parameter description, therefore has many independent fields
each capable of carrying up to that many payload-bearing selectors: 100 sanitized fields could
carry up to 1,600 surviving selectors in aggregate across the whole introspection response, 2,000
fields up to 32,000. This is still far weaker than the pre-#431 channel (unlimited per field) and
is arguably inherent to any purely per-field sanitizer that has no cross-field state to consult —
but it is the one variation-selector-smuggling shape that still survives sanitization at all, so
it is called out here explicitly rather than left implicit in the per-field framing above. Closing
it would require either a request-wide (not per-field) budget threaded through every
`sanitize_untrusted_text` call site, or the semantic check already noted as out of scope.

## 3. Cross-Crate Contracts

| Consumer | What it depends on from `mcp-core` |
|---|---|
| `mcp-introspector` | `ServerConfig`, `ServerId`, `ToolName`, `Transport`, `validate_server_config`, `Error`/`Result` |
| `mcp-codegen` | `Error`/`Result`, `metadata::*` (writes `_meta.json`), `forbidden_chars`/`forbidden_env_names`/`forbidden_env_prefix` (renders them into the generated runtime bridge template) |
| `mcp-files` | `Error`/`Result` indirectly via `mcp-codegen` |
| `mcp-skill` | `sanitize_path_for_error`, `contains_parent_dir`, `validate_server_id_slug`, `ServerIdSlugError`, `MAX_SERVER_ID_LENGTH`, `untrusted::*`, `metadata::*`, `confinement::{ConfinementError, ConfinementTarget, resolve_confined_path}` |
| `mcp-server` | `ServerConfig`, `ServerId`, `sanitize_path_for_error`, `contains_parent_dir`, `validate_server_id_slug`, `ServerIdSlugError`, `untrusted::*`, `confinement::{ConfinementError, ConfinementTarget, resolve_confined_path}`, `cli::{LogFormat, LOG_FORMAT_ENV_VAR}` |
| `mcp-cli` | `cli::{OutputFormat, ExitCode, LogFormat, LOG_FORMAT_ENV_VAR}`, `ServerConfig`/`ServerConfigBuilder`, `RedactedItems`/`RedactedUrl`, `Error` (for exit-code classification) |

## 4. Defense in Depth

Since #313, `ServerConfig`'s "always validated" guarantee is a **type-level**
property: both fields are private and `Deserialize` is hand-written to run
`validate_server_config` before returning, so there is no longer a construction path
(builder, struct literal, `serde_json::from_str`) that skips it. `mcp-introspector`'s
`Introspector::discover_server` still calls `validate_server_config` again before
spawning/connecting anyway — not because it's needed to close a gap, but so this method
stays self-defending against a future construction path that forgets to validate, rather than
relying solely on the invariant holding elsewhere.

`resolve_confined_path` (issue #395) re-validates its `segment` argument via
`validate_path_segment` on every call rather than trusting a caller's own upstream check (e.g.
`mcp-server::service::introspect_server`'s tighter `validate_server_id` charset check) to have
already run — the same self-defending posture: a future call site that reaches this function
with an unvalidated `segment` is still caught here, not silently trusted. Both `mcp-skill` and
`mcp-server` keep their own crate-specific `OutputPathError`/`OutputDirError` enums as the public
error surface rather than exposing `ConfinementError` directly, so a caller matching on either
enum sees no observable change from before #395 — the `From<ConfinementError>` impls are total
and byte-preserving of each variant's existing `Display` message.

## 5. Edge Cases & Notable Behaviors (from tests)

| Scenario | Behavior |
|---|---|
| Header names differing only by case (`Authorization` vs `authorization`) | Rejected as duplicate (case-insensitive comparison), since `http::HeaderName` would otherwise silently collapse them |
| Header name containing space/`:`/`@` | Rejected — not a control character, but outside RFC 7230 `tchar` |
| `mcp.json` with `"transport": "http"` and no `url` | Fails to deserialize (`url` is a required field of `Transport::Http`, not `#[serde(default)]`) — rejected before a `ServerConfig` value exists at all |
| Timeout of exactly `0` | Always rejected — no "infinite timeout" sentinel exists |
| Timeout of `601s` (`MAX_TIMEOUT` = 600s) | Rejected; `600s` itself is accepted |
| Absolute-path command that exists but lacks the execute bit | Rejected (Unix only) |
| `sanitize_path_for_error` on a mounted/bind-mounted home directory | Falls back to scrubbing the bare username substring when the leading-prefix match fails |
| `sanitize_path_for_error` on a home directory whose username differs from the path's only by NFC-vs-NFD composition form (Windows/macOS) | Still matched and redacted on both the primary `components_match` path and the `scrub_username`/`replace_case_aware` fallback — `haystack`/`needle` are NFC-normalized as whole strings, not per-window, before comparison |
| `sanitize_untrusted_text` on a value containing U+202E (RIGHT-TO-LEFT OVERRIDE) or an isolate control (U+2066-U+2069) | Flattened to a space, same as a control character |
| `sanitize_untrusted_text` on a value containing a Unicode Tags block character (U+E0000-U+E007F), U+FEFF, or a character in the U+2060-U+2064 invisible-operator run | Removed entirely, no space substituted |
| `sanitize_untrusted_text` on a value containing U+200B (ZERO WIDTH SPACE) | Flattened to a space, same as a control character — it is a genuine break opportunity, unlike the removed-entirely characters above |
| `sanitize_untrusted_text` on a value containing U+200C (ZERO WIDTH NON-JOINER) or U+200D (ZERO WIDTH JOINER) | Left untouched — orthographically load-bearing, deliberately out of scope |
| `sanitize_untrusted_text` on a value whose total variation-selector count (U+FE00-U+FE0F, U+E0100-U+E01EF) is 1-2 and every run is 1-2 consecutive | Left untouched — the legitimate emoji-presentation/IVS case |
| `sanitize_untrusted_text` on a value containing a single run of 3+ consecutive variation selectors, total under 16 | That run dropped, no space substituted — treated as smuggled payload |
| `sanitize_untrusted_text` on a value whose variation selectors, however distributed across however many runs/base characters, total more than 16 | Every variation selector in the value dropped, regardless of individual run length |
| `sanitize_untrusted_text` on a value with variation selectors interleaved with invisible, removed-entirely characters (Tags block, bidi marks, etc.) meant to split a long run into sub-threshold pieces | The interleaving separators are removed *before* run/total detection runs, so the pieces are seen as one contiguous run/total, not several independent short ones — the split does not help |
| `ToolName::new`/`ServerId::new` on a name containing a Unicode Tags block character, a bidi mark/control, or a zero-width/invisible-operator character | Rejected with `ToolNameError`/`ServerIdError::DisallowedCharacter` (none of these carry UTS #39 `Identifier_Status=Allowed`), not silently constructed |
| `ToolName::new`/`ServerId::new` on a name containing even a single variation selector | Rejected with `ToolNameError`/`ServerIdError::DisallowedCharacter` — the UTS #39 allowlist has no allowed variation selector, so this is stricter than `sanitize_untrusted_text`'s display-text thresholds without a dedicated variation-selector check |
| `RedactedUrl` on `https://user:p/w@host/mcp` (unencoded `/` in password) | Whole URL redacted — the authority-terminator ambiguity check fires |
| `resolve_confined_path` terminal component is a dangling symlink, under either `ConfinementTarget` | Rejected as `Escape` — checked via `symlink_metadata`, not `metadata`/`canonicalize`, so a target that can't be resolved is never mistaken for "doesn't exist yet" |
| `resolve_confined_path` terminal component exists as the other `ConfinementTarget` kind (file where `Directory` expected, or vice versa) | `WrongTargetKind`, mapped by each caller to its own truthful variant name |
| `resolve_confined_path` with `target: None` | Returns the walked `relative_dirs` chain itself; nothing beyond the segment directory is created |
| `resolve_confined_path`'s lenient intermediate walk hits a symlink loop (`ELOOP`) | Surfaces as `Io`, not `Escape` — `canonicalize`'s error propagates via `?` before any confinement comparison runs |

## 6. See Also

- [[../introspector/spec]] — primary consumer of `ServerConfig`/`validate_server_config`
- [[../cli/spec]] — consumer of `cli::{OutputFormat,ExitCode}` and the error-classification contract
- [[../server/spec#6. Output Directory Resolution (`output_dir.rs`)]] / [[../skill/spec#8. `resolve_skill_output_path` — Path Confinement]] — the two `confinement::resolve_confined_path` consumers
- [[../constitution]] — security principles this crate embodies workspace-wide
