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
```
Both: `new(impl Into<String>) -> Self`, `as_str(&self) -> &str`,
`into_inner(self) -> String`, `Display`, `From<String>`, `From<&str>`,
`Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize`. No format
validation at this layer (unlike `cli::ServerConnectionString` or
`mcp-skill::validate_server_id`, which are the actual gatekeepers) — these
are pure newtype wrappers for type safety, not validated value objects.

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
    ResourceLimitExceeded { resource: String, actual: usize, limit: usize },
}
pub type Result<T> = std::result::Result<T, Error>;
```
Each variant has an `is_*` predicate (`is_connection_error`,
`is_security_error`, `is_timeout`, `is_validation_error`,
`is_script_generation_error`, `is_resource_limit_exceeded`). No predicate
exists for `InvalidArgument`/`SerializationError` — callers match directly.
`ScriptGenerationError.source` is the vehicle for preserving an inner
`Error`'s own classification through wrapping (see
`mcp-cli`'s `classify_core_error`, which recurses into it — [[../cli/spec]]).

### `ServerConfig` / `ServerConfigBuilder` / `TransportType` (`src/server_config.rs`)

```rust
pub enum TransportType { Stdio, Http, Sse } // #[default] Stdio
pub struct ServerConfig {
    pub transport: TransportType,
    pub command: String,           // stdio only
    pub args: Vec<String>,         // stdio only
    pub env: HashMap<String,String>,   // stdio only
    pub cwd: Option<PathBuf>,      // stdio only
    pub url: Option<String>,       // http/sse only
    pub headers: HashMap<String,String>, // http/sse only
    pub connect_timeout: Duration, // default 30s, all transports
    pub discover_timeout: Duration, // default 30s, all transports
}
```
Builder methods: `command`, `arg`, `args`, `env`, `environment`, `cwd`,
`http_transport(url)`, `sse_transport(url)`, `url`, `header`, `headers`,
`connect_timeout`, `discover_timeout`, `build() -> Result<ServerConfig>`.

`build()` = `build_structural()` (presence checks: `command` required
non-empty for stdio, `url` required for http/sse) **then**
`validate_server_config(&config)` — full security validation runs
unconditionally inside `build()`, so a `ServerConfig` obtained through the
builder cannot exist without having passed it. This is a **builder-level**
guarantee, not type-level: every field is `pub` and the type derives
`Deserialize`, so a struct literal or `serde_json::from_str` bypasses it —
see [[#Defense in depth]].

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

1. `validate_size_bounds` — command/url/arg/env/header count and length
   caps (CWE-400 backstop; runs even for a hand-crafted `ServerConfig` whose
   fields don't match its declared transport, since every field is
   `#[serde(default)]` and populated independent of transport at the type
   level).
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
pub struct ServerMetadata { schema_version, server_id, server_name, server_version, tools: Vec<ToolMetadata> }
pub struct ToolMetadata { name, typescript_name, category: Option<String>, keywords: Vec<String>, description: Option<String>, parameters: Vec<ParameterMetadata> }
pub struct ParameterMetadata { name, typescript_type, required, description: Option<String> }
```
This is the **wire contract** between `mcp-codegen` (producer, writes
`_meta.json`) and `mcp-skill`/`mcp-server` (consumers). Bumping
`METADATA_SCHEMA_VERSION` is the intended way to signal a breaking shape
change; consumers compare it and fail loudly on mismatch (see
[[../skill/spec#ScanError]]).

### `path` module (`src/path.rs`)

```rust
pub fn sanitize_path_for_error(path: &Path) -> String; // redacts home dir to "~", falls back to scrubbing bare username
pub fn validate_path_segment(segment: &str) -> Option<Component<'_>>; // single plain component, no "..", no separator
```
`validate_path_segment` is the shared building block for both
`mcp-skill::resolve_skill_output_path` and
`mcp-server::output_dir::resolve_output_dir`'s `server_id` confinement — a
`..` or embedded separator in `server_id` is rejected identically by both.

### `redact` module (`src/redact.rs`)

```rust
pub const REDACTED_PLACEHOLDER: &str = "<redacted>";
pub struct RedactedMapValues<'a>(pub &'a HashMap<String,String>); // Debug: keys visible, values redacted
pub struct RedactedItems<'a>(pub &'a [String]);                  // Debug: every entry redacted wholesale
pub struct RedactedUrl<'a>(pub &'a str);                         // Debug: userinfo + query redacted, host/path kept
```
`RedactedUrl` is deliberately parse-free (no `url` crate dependency) and
redacts the **whole** input if it cannot unambiguously identify the
authority boundary (e.g. an unencoded `/` or `?` inside userinfo) or if the
scheme contains an invalid character — fails closed toward over-redaction
rather than partial leakage.

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
| `mcp-introspector` | `ServerConfig`, `ServerId`, `ToolName`, `TransportType`, `validate_server_config`, `Error`/`Result` |
| `mcp-codegen` | `Error`/`Result`, `metadata::*` (writes `_meta.json`), `forbidden_chars`/`forbidden_env_names`/`forbidden_env_prefix` (renders them into the generated runtime bridge template) |
| `mcp-files` | `Error`/`Result` indirectly via `mcp-codegen` |
| `mcp-skill` | `sanitize_path_for_error`, `validate_path_segment`, `untrusted::*`, `metadata::*` |
| `mcp-server` | `ServerConfig`, `ServerId`, `sanitize_path_for_error`, `validate_path_segment`, `untrusted::*` |
| `mcp-cli` | `cli::{OutputFormat, ExitCode}`, `ServerConfig`/`ServerConfigBuilder`, `RedactedItems`/`RedactedUrl`, `Error` (for exit-code classification) |

## 4. Defense in Depth

`ServerConfig`'s "always validated" guarantee is a **builder-level**
property, not a type-level one. Every downstream consumer that might
receive a `ServerConfig` from somewhere other than the builder
re-validates:

- `mcp-introspector::Introspector::discover_server` calls
  `validate_server_config` again before spawning/connecting.
- Test code (`command.rs`'s own tests) deliberately constructs an
  unvalidated `ServerConfig` via `serde_json::from_str` to exercise this
  gap directly (e.g. an HTTP-transport config missing `url`, which
  deserializes fine because every field is `#[serde(default)]`).

## 5. Edge Cases & Notable Behaviors (from tests)

| Scenario | Behavior |
|---|---|
| Header names differing only by case (`Authorization` vs `authorization`) | Rejected as duplicate (case-insensitive comparison), since `http::HeaderName` would otherwise silently collapse them |
| Header name containing space/`:`/`@` | Rejected — not a control character, but outside RFC 7230 `tchar` |
| `mcp.json` with `"transport": "http"` and no `url` | Deserializes successfully (all fields `#[serde(default)]`); caught by `validate_network_config`, not by deserialization |
| Timeout of exactly `0` | Always rejected — no "infinite timeout" sentinel exists |
| Timeout of `601s` (`MAX_TIMEOUT` = 600s) | Rejected; `600s` itself is accepted |
| Absolute-path command that exists but lacks the execute bit | Rejected (Unix only) |
| `sanitize_path_for_error` on a mounted/bind-mounted home directory | Falls back to scrubbing the bare username substring when the leading-prefix match fails |
| `RedactedUrl` on `https://user:p/w@host/mcp` (unencoded `/` in password) | Whole URL redacted — the authority-terminator ambiguity check fires |

## 6. See Also

- [[../introspector/spec]] — primary consumer of `ServerConfig`/`validate_server_config`
- [[../cli/spec]] — consumer of `cli::{OutputFormat,ExitCode}` and the error-classification contract
- [[../constitution]] — security principles this crate embodies workspace-wide
