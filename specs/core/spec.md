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
     `validate_path_segment` (used by both for identical `server_id`
     path-segment validation).
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
pub enum ServerIdError { InvalidFormat { id: String } }
pub enum ToolNameError { InvalidFormat { name: String } }
```
Both: `new(impl Into<String>) -> Result<Self, XxxError>`, `as_str(&self) -> &str`,
`into_inner(self) -> String`, `Display`,
`Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize`. `new` enforces a baseline
invariant shared with `path::validate_path_segment`: the input must be a single non-empty
path segment (no `..`, no path separator, no root/prefix component), since both a server id
and a tool name are ultimately used to derive a filesystem path or file name downstream.
There is no `From<String>`/`From<&str>` impl, and `#[serde(try_from = "String")]` (backed by
`impl TryFrom<String>` delegating to `new`) routes `Deserialize` through `new` too — `new` is
the only construction path, including through deserialization, so the invariant cannot be
bypassed by an infallible conversion or a direct-derive deserialize. Without
`try_from = "String"`, a plain `#[derive(Deserialize)]` would still be able to construct
`Self(raw_string)` directly since the derived impl lives in this same module and privacy
doesn't block it — this is exactly the gap that made `mcp_execution_introspector::ServerInfo`/
`ToolInfo` (which derive `Deserialize` and hold a `ServerId`/`ToolName` field) deserializable
with an unvalidated id/name before this was added. `mcp-skill::validate_server_id`
layers a stricter `[a-z0-9-]`-charset + length check on top of this baseline for its own
contract; it does not replace it.

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
Each variant has an `is_*` predicate (`is_connection_error`,
`is_security_error`, `is_timeout`, `is_validation_error`,
`is_script_generation_error`, `is_resource_limit_exceeded`,
`is_duplicate_generated_file_path`). No predicate exists for
`InvalidArgument`/`SerializationError` — callers match directly.
`ScriptGenerationError.source` is the vehicle for preserving an inner
`Error`'s own classification through wrapping (see
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
     names checked against `forbidden_env_names()`/`forbidden_env_prefix()`.
   - `Http`/`Sse` → `validate_network_config`: `url` required,
     `validate_url_scheme` (must be `http://`/`https://`, case-insensitive),
     header name (RFC 7230 `tchar` charset) and value (no control chars)
     checks, duplicate header names rejected case-insensitively.
3. `validate_timeout` on `connect_timeout`/`discover_timeout` — must be
   `> 0` and `<= MAX_TIMEOUT` (10 minutes); **no infinite-timeout option is
   supported by design** (an unbounded wait would let a hung server block
   this non-interactive tool forever).

Forbidden env names (exact match): `LD_PRELOAD`, `LD_LIBRARY_PATH`,
`LD_AUDIT`, `DYLD_INSERT_LIBRARIES`, `DYLD_LIBRARY_PATH`,
`DYLD_FRAMEWORK_PATH`, `PATH`, `NODE_OPTIONS`, `BASH_ENV`, `PYTHONPATH`,
`PYTHONSTARTUP`, `RUBYOPT`, `PERL5OPT`, `JAVA_TOOL_OPTIONS`; plus any name
with prefix `DYLD_`. This list is explicitly documented as an
**accidental-misconfiguration guard, not a sandbox boundary** — it does not
protect against a malicious command/binary itself.

Every rejection error omits the offending value from its message when that
value could be secret-shaped (a misparsed `--api-key sk-...` argument, a
header name/value) — this is enforced by tests, not just convention.

### `cli` module (`src/cli.rs`)

```rust
pub enum OutputFormat { Json, Text, #[default] Pretty } // FromStr, Display
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
pub fn contains_parent_dir(path: &Path) -> bool; // true if any component is `..`
```
`validate_path_segment` is the shared building block for both
`mcp-skill::resolve_skill_output_path` and
`mcp-server::output_dir::resolve_output_dir`'s `server_id` confinement — a
`..` or embedded separator in `server_id` is rejected identically by both;
it also backs `ServerId::new`/`ToolName::new`'s own invariant (see above).
`contains_parent_dir` (issue #289) is the shared `..`-only check used by
`mcp-skill::output_path`, `mcp-server::output_dir`, and
`mcp-cli::commands::skill::has_path_traversal` for the narrower "is any
component `..`" question, previously three byte-for-byte-identical copies.

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
`;` `:`); the resulting token is redacted by handing it to `RedactedUrl`'s
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
pub fn sanitize_untrusted_text(s: &str, max_len: usize) -> String; // flattens all control chars + U+2028/U+2029 to spaces, truncates by char count
pub fn wrap_untrusted_block(context: &str, body: &str) -> String;  // escapes &, <, > in body; wraps in <untrusted-data>...</untrusted-data>
```
Threat model: an introspected MCP server's tool names/descriptions/keywords
are attacker-controlled from this project's perspective. Both functions
exist because `mcp-skill` (SKILL.md + prompt generation) and
`mcp-server` (introspection summaries returned to Claude) both embed this
data into text an LLM later reads as instructions — see
[[../server/spec#Prompt injection defense]] and
[[../skill/spec#Prompt injection defense]].

## 3. Cross-Crate Contracts

| Consumer | What it depends on from `mcp-core` |
|---|---|
| `mcp-introspector` | `ServerConfig`, `ServerId`, `ToolName`, `Transport`, `validate_server_config`, `Error`/`Result` |
| `mcp-codegen` | `Error`/`Result`, `metadata::*` (writes `_meta.json`), `forbidden_chars`/`forbidden_env_names`/`forbidden_env_prefix` (renders them into the generated runtime bridge template) |
| `mcp-files` | `Error`/`Result` indirectly via `mcp-codegen` |
| `mcp-skill` | `sanitize_path_for_error`, `validate_path_segment`, `untrusted::*`, `metadata::*` |
| `mcp-server` | `ServerConfig`, `ServerId`, `sanitize_path_for_error`, `validate_path_segment`, `untrusted::*` |
| `mcp-cli` | `cli::{OutputFormat, ExitCode}`, `ServerConfig`/`ServerConfigBuilder`, `RedactedItems`/`RedactedUrl`, `Error` (for exit-code classification) |

## 4. Defense in Depth

Since #313, `ServerConfig`'s "always validated" guarantee is a **type-level**
property: both fields are private and `Deserialize` is hand-written to run
`validate_server_config` before returning, so there is no longer a construction path
(builder, struct literal, `serde_json::from_str`) that skips it. `mcp-introspector`'s
`Introspector::discover_server` still calls `validate_server_config` again before
spawning/connecting anyway — not because it's needed to close a gap, but so this method
stays self-defending against a future construction path that forgets to validate, rather than
relying solely on the invariant holding elsewhere.

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
| `RedactedUrl` on `https://user:p/w@host/mcp` (unencoded `/` in password) | Whole URL redacted — the authority-terminator ambiguity check fires |

## 6. See Also

- [[../introspector/spec]] — primary consumer of `ServerConfig`/`validate_server_config`
- [[../cli/spec]] — consumer of `cli::{OutputFormat,ExitCode}` and the error-classification contract
- [[../constitution]] — security principles this crate embodies workspace-wide
