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
   - `metadata` — the `_meta.json` sidecar schema (version 3) shared by `mcp-codegen`
     (producer) and `mcp-skill`/`mcp-server`/`mcp-cli` (consumers), plus the one reader every
     consumer goes through (`read_server_metadata`, #618).
   - `helper_protocol` — the helper wire constants and `ResultSource`, the one definition of where a
     tool result is read from (#676).
   - `annotations` (#568) — `Hint`, `AnnotationTitle`, `ToolAnnotations`: the typed, sanitized
     shape of the untrusted tool annotations that `mcp-introspector` captures, `mcp-codegen`
     renders and stores, and `provenance` hashes.
   - `provenance` (issue #468) — `GenerationProvenance`/`ConfigFingerprint`/`ToolDigest`, the
     generation-timestamp-and-fingerprint data `mcp-codegen` stamps into every
     `metadata::ServerMetadata` it writes, computed from the same `ServerConfig`/tool list a
     `generate` run is generating from.
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
     `types::ServerSlug` (issues #401, #677) is the separate,
     stricter validated type both crates' `server_id` confinement
     requires *before* calling into `resolve_confined_path`
     (see below) — `resolve_confined_path` itself stays generic and does
     not know about the slug rule.
   - `confinement` — `resolve_confined_path` (issue #395), the shared
     component-by-component resolve-and-confine filesystem walk used by both
     `mcp-skill::resolve_skill_output_path` and
     `mcp-server::output_dir::resolve_output_dir`, previously two independent
     copies of the same algorithm.
   - `redact` — `Debug`-redaction wrapper types (`RedactedItems`,
     `RedactedMapValues`, `RedactedUrl`) used by `ServerConfig` itself and
     by `mcp-cli`'s CLI-argument types. `RedactedUrl`'s URL-splitting logic is factored out into
     a crate-internal `split_url` (issue #468), returning parsed pieces (scheme, userinfo
     presence, authority, path, query/fragment tail) rather than a formatted string — `Debug`
     formats those pieces one way, `provenance::ConfigFingerprint::compute` another, so the two
     can never disagree about where the authority ends, and the fingerprint never depends on a
     `Debug` rendering's unstable format.
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
`code_point` is the `u32` Unicode scalar value of the first rejected character, computed from
the *raw* (unsanitized) input so it is not thrown off by construction-time sanitization (below).
The `{:?}` `Debug` formatting used in both variants' error messages escapes control/format code
points in the rejected string, so an attacker-controlled value can't smuggle an escape sequence
into the rendered error. There is no `From<String>`/`From<&str>` impl, and
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

Since issue #446, the `id`/`name` field stored in both `InvalidFormat` and `DisallowedCharacter`
holds a *sanitized* form of the rejected input, not the raw one: `new` runs the raw value through
`untrusted::sanitize_untrusted_inline` (`sanitize_untrusted_text`, capped at
`MAX_UNTRUSTED_FIELD_LEN` **chars**, then `&`/`<`/`>` entity-escaped) before constructing the
error variant, in every branch that stores the input — the `code_point` computation above still
reads the raw value first. This closes the same class of gap #433 closed for tool-name
collisions: `Identifier_Status=Allowed` rejects a tool name containing `&`/`<`/`>`/control
characters/bidi overrides, but the rejection's own error message previously echoed that same
raw, disallowed substring back into `Display`/`Debug`-formatted text that reaches an LLM (via
`mcp-introspector`'s `Error::ValidationError` mapping and `mcp-server`'s
`caller_or_internal_error` — [[../server/spec]]) or a terminal (`mcp-cli`, `tracing`), letting a
malicious server forge Markdown structure, HTML-like tags (including
`untrusted::wrap_untrusted_block`'s own `</untrusted-data>` delimiter shape), or an unbounded
message (`ServerId::new` has no length bound of its own).

The cap is on the *pre-escaping* char count, not the final message size: escaping runs after
truncation, and each escaped `&` expands to `&amp;` (5x), so a densely-escapable input (e.g. 5000
`&` characters) still yields a bounded but noticeably larger message than
`MAX_UNTRUSTED_FIELD_LEN` alone suggests — up to `MAX_UNTRUSTED_FIELD_LEN * 5` bytes of escaped
content, observed at ~2600 bytes for the full error message in the all-`&` case, not the ~500-700
bytes a naive reading of the cap implies.

Every variant's `#[error(...)]` formats the stored field with `{:?}` (`Debug`), not `{}`
(`Display`) — a required second layer of defense, not incidental. `sanitize_untrusted_inline`
deliberately leaves some characters untouched (e.g. U+200C/U+200D — see
`sanitize_untrusted_text`'s own doc comment), so `Debug`'s own escaping of those characters
(`\u{200d}` rather than the literal) is what actually closes the invisible-payload channel
#425/#430 closed for those residual cases; using `Display` here would silently reopen it.

Emoji is a deliberate exception, not neutralized by this fix — see the edge-case table below.
Human `mcp-cli` users now see the entity-escaped form too (e.g. `invalid server id
"my&amp;server"` rather than `"my&server"`) when a rejected id/name contains `&`/`<`/`>` — an
accepted trade-off of applying one sanitization path uniformly to both the LLM- and
human-facing surfaces, rather than maintaining two different escaping rules for the same field.

### `ServerSlug` / `ServerIdSlugError` (`src/types.rs`, issues #401, #677)

```rust
pub const MAX_SERVER_ID_LENGTH: usize = 64;
#[non_exhaustive]
pub enum ServerIdSlugError { Empty, TooLong { len: usize, limit: usize }, InvalidCharacters, ReservedName }
pub struct ServerSlug(String); // private field; rule `[a-z0-9-]{1,64}`, not a Windows reserved device name
impl ServerSlug {
    pub fn new(id: &str) -> Result<Self, ServerIdSlugError>;
    pub fn as_str(&self) -> &str;
}
impl From<ServerSlug> for ServerId;
// also: Display, AsRef<str>, Borrow<str>, TryFrom<String>, Serialize/Deserialize (try_from = "String"),
// schemars::JsonSchema -> {type: string, maxLength: 64, pattern: "^[a-z0-9-]+$"}
```

The slug rule is decided and frozen (#677): 1 to 64 bytes of lowercase ASCII letters, digits and hyphens, with
**no** restriction on leading, trailing or consecutive hyphens. Consequences: `-`, `--a` and `a-` are valid ids; a
SKILL.md `name` of that shape may be refused by a host that applies the Agent Skills hyphen rule; `--a` given as a
command-line positional must follow `--`.

**Windows reserved device names (#696).** The 24 names `con`, `prn`, `aux`, `nul`, `com0` to `com9` and `lpt0` to
`lpt9` cannot name a directory on Windows and are rejected with `ServerIdSlugError::ReservedName` ("server_id must not
be a Windows reserved device name") on every platform, by `new` and therefore by deserialization. Check order: `Empty`,
`TooLong`, `InvalidCharacters`, `ReservedName`. Names that only resemble one (`con-1`, `nul2`, `com10`, `lpt`) are
valid. The published JSON schema is unchanged and does not express the exclusion.

The rule is stated once, in `is_reserved_device_name(name: &str) -> bool` (`src/path.rs`, `#[doc(hidden)]`,
workspace-internal, #701): true when the whole string equals one of the 24 names in any ASCII letter case; no
extension stripping (`con.ts` and `con-1` are false). Server slugs, saved script names ([[../skill/spec]]) and
generated tool file names ([[../codegen/spec]]) share it.

`validate_server_id_slug` is private to this module and is the single place the rule lives. `ServerSlug` is the
type of every public `server_id` whose producer validates the rule (`NFR-COM-035`); deserialization goes through
`new`, so an invalid id in a request or file is rejected at parse time. A slug is always a valid `ServerId`; the
converse does not hold.

`ServerSlug` is a stricter invariant layered *on top of* `ServerId::new`'s baseline, not a replacement for it.
`ServerId::new` stays permissive (issue #311: a raw `mcp.json` key like `claude_ai_Gmail` is a valid `ServerId`
but not a slug; callers that only need a safe path segment, such as `mcp-cli`'s config-key lookup, keep accepting
non-slug ids). Callers that need the id to become a directory name or generated-code identifier hold a
`ServerSlug`. It also backs both crates' `server_id` output confinement (`mcp-server::output_dir`,
`mcp-skill::output_path`); see [[../server/spec#Output directory resolution]] and [[../skill/spec]].

### `Error` / `Result<T>` (`src/error.rs`)

```rust
pub enum Error {
    ConnectionFailed { server: ServerId, source: Box<dyn Error+Send+Sync> },
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
`is_script_generation_error`, `is_resource_limit_exceeded`). No predicate exists for
`InvalidArgument`/`SerializationError` — callers match directly.
`ConnectionFailed`/`Timeout` have no `is_connection_error`/`is_timeout` predicate either
(removed, issue #427, mirroring #199/#202's identical dead-predicate-removal precedent): their
only real call site, `mcp-cli`'s `classify_core_error`, is an exhaustive `match` over every
`Error` variant with no wildcard arm, so it always matched each variant by name directly rather
than through a predicate — adding a variant there is a compile error at that `match`, a
guarantee an `if`/`else if` predicate chain would silently lose. `ScriptGenerationError.source`
is the vehicle for preserving an inner `Error`'s own classification through wrapping (see
`mcp-cli`'s `classify_core_error`, which recurses into it — [[../cli/spec]]).
`DuplicateGeneratedFilePath` likewise has no `is_duplicate_generated_file_path` predicate
(removed, issue #445): it had no production call site anywhere in the workspace, only test
call sites (`mcp-core`'s own doc-test/unit test, and `mcp-codegen`'s `GeneratedCode::add_file`
tests), which now match the variant directly instead. `is_security_error` and
`is_validation_error` were both flagged as candidates by the same issue but were kept: unlike
the removed predicate, both are used together at a single real production call site,
`mcp-server`'s `caller_or_internal_error` ([[../server/spec]]) —
`err.is_validation_error() || err.is_security_error()` — which classifies a `ValidationError` or
`SecurityViolation` as a caller-facing `invalid_params` response rather than an internal error.
The issue's premise ("zero call sites anywhere, including tests") was correct for
`is_duplicate_generated_file_path` but factually wrong for `is_security_error`; do not re-attempt
removing either predicate in a future cleanup pass without re-checking this call site first.

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
     the forbidden names and the `DYLD_` prefix, ASCII-case-insensitively
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
pub struct ExitCode(i32); // SUCCESS=0, ERROR=1, INVALID_INPUT=2, SERVER_ERROR=3, TIMEOUT=4, CHECK_FAILED=5, SCRIPT_FAILED=6,
                          // HANGUP=129 (SIGHUP), INTERRUPTED=130 (SIGINT), QUIT=131 (SIGQUIT), TERMINATED=143 (SIGTERM); each is 128 + the signal number (#627, #646)
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
pub const METADATA_SCHEMA_VERSION: u32 = 3;
pub const METADATA_FILE_NAME: &str = "_meta.json";
pub const INDEX_FILE_NAME: &str = "index.ts";
pub const CATALOG_FILE_NAME: &str = "catalog.ts";        // #566: the top-level type-only catalog
pub const MAX_METADATA_FILE_BYTES: u64 = 1024 * 1024;    // 1 MiB; shared by producer and consumer
pub const MAX_TOOL_COUNT: usize = 1000;                  // per-server tool bound; introspector re-exports it, skill's MAX_TOOL_FILES equals it (#585)
pub struct ServerMetadata { schema_version: u32, server_id: ServerId, server_name: String, server_version: String, tools: Vec<ToolMetadata>, provenance: GenerationProvenance }
pub struct ToolMetadata {
    name: ToolName, typescript_name: String, category: Option<String>, keywords: Vec<String>,
    description: Option<String>, parameters: Vec<ParameterMetadata>,
    annotations: Option<ToolAnnotations>,       // v3
    result_type: Option<String>,                // v3
    result_type_origin: ResultTypeOrigin,       // v3
    manifest_size_bytes: u64,                   // v3
    digest: Option<ToolEntryDigest>,            // v3, optional (#688)
}
pub enum ResultTypeOrigin { Declared, Degraded, Undeclared } // serde snake_case
pub struct ParameterMetadata { name, typescript_type, required, description: Option<String> }
```

#### Reader (`read_server_metadata`, #618)

```rust
pub fn read_server_metadata(dir: &Path) -> Result<ServerMetadata, MetadataReadError>;
pub async fn read_server_metadata_async(dir: PathBuf) -> Result<ServerMetadata, MetadataReadError>;
pub enum MetadataReadError {
    NotFound, Link, NotRegularFile,
    TooLarge { size: u64, limit: u64 },
    Unreadable(io::Error), Malformed(serde_json::Error),
    SchemaMismatch { found: u32, expected: u32 },
}
```

The one place a `_meta.json` is read: `mcp-cli` (`drift`, `scripts check`, `stats`, `skill`, `generate`, dry-run
policy suggestions) and `mcp-skill`'s scan all call it, so the rules below cannot drift apart.

- **Contract.** `dir` is the server directory and may itself be a symlink; the sidecar `dir/_meta.json` must be a regular
  file that is not a symlink, at most `MAX_METADATA_FILE_BYTES` (1 MiB), and carry `METADATA_SCHEMA_VERSION`. A symlinked
  sidecar is refused even when its target lies inside `dir`.
- **Open.** On Unix the file is opened with `O_NOFOLLOW | O_NONBLOCK` and the regular-file check is an `fstat` on the
  opened handle, so there is no window between check and open and a FIFO never blocks. `ELOOP` maps to `Link`; any other
  type maps to `NotRegularFile`. On Windows (#639) the sidecar is opened with `FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS` and judged only by the
  opened handle: a symlink reparse point is `Link`, any other reparse point or a directory (which opens because of backup
  semantics) is `NotRegularFile`. There is no check-then-open window. Backup semantics bypass the DACL only for a token with
  SeBackupPrivilege enabled (approved deviation, architect, 2026-11-25). The Windows runtime behaviour is covered by a
  `cfg(windows)` test and Windows CI only.
- **Size.** The size is checked from the handle, and the read is bounded by `take(MAX_METADATA_FILE_BYTES + 1)`, so a file
  that grows after the check still yields `TooLarge`.
- **Order.** Valid JSON is parsed to a `serde_json::Value`, `schema_version` is compared first (`SchemaMismatch`, so an
  older sidecar reports a version error rather than a shape error), and only then is the typed `ServerMetadata`
  deserialized (`Malformed` on failure).
- **Errors.** Messages never contain the path (callers add a sanitized path or the server id). `NotFound` maps from
  `io::ErrorKind::NotFound` only. `Malformed` wraps the `serde_json::Error` of the parse.
- **Async.** `read_server_metadata_async` runs the read in one `spawn_blocking`, resumes a panic of the task, and maps a
  cancelled task to `Unreadable`.

#### Registry of v3 fields (single registry for the whole specification package)

No other block defines a metadata field of its own; every other block refers here. Version 3 is
the one bump of the script-first package (FR-034.1, FR-034.2).

| Field (per tool) | Meaning | Needed by | Resolution |
|---|---|---|---|
| `annotations` | `Option<ToolAnnotations>`: `None` when the server sent neither annotations nor a usable title; otherwise a sanitized optional title and four three-valued hints (never `false` for "not stated", never the SDK default) | FR-034, FR-032, FR-038 | in |
| `result_type` | Compact TypeScript text of the result type the tool file declares, **rendered text** (not structured). `None` above `STORED_TYPE_TEXT_MAX_BYTES` (1024) or when the result degraded to the generic form | FR-030 | in |
| `result_type_origin` | `declared`, `degraded` (the server declared an output schema but the generic type is used) or `undeclared`. This is the stored output-schema flag, and it distinguishes "declared but not translated" | FR-046, FR-030 | in |
| `manifest_size_bytes` | Length of the compact `serde_json::to_vec` of the introspected, sanitized `ToolInfo` (after the introspector's bounds, including annotations and output schema) | FR-046 | in |
| `parameters[].name` | The **raw** schema key (never renamed or suffixed, #584); `parameters[].typescript_type` is `"unknown"` above 1024 bytes | FR-031 | in |
| `digest` | `Option<ToolEntryDigest>`: the digest of this tool's entry, so a drift report names a changed tool. Optional (`serde` default, omitted when `None`) ([[#7. Per-tool digests (#688)]]) | FR-045 | in (#688) |
| Annotations inside the digest entries | An annotation-only change reads as drift | FR-045, FR-034 | in (see [[#`provenance` module (`src/provenance.rs`)]]) |
| Catalog reference, converter diagnostics | | FR-032, FR-031 | out (diagnostics are `tracing::warn!` only) |

| ID | Requirement | Status |
|---|---|---|
| FR-034.1 | `METADATA_SCHEMA_VERSION` is 3 and every field marked "in" is persisted | implemented |
| FR-034.2 | No second bump for a registry field; every candidate row is resolved | implemented |
| FR-034.3 | No annotations stored as absent; an unstated hint stored as not stated, never `false`, never the SDK default | implemented |
| FR-034.4 | A consumer reading a sidecar whose `schema_version` is not 3 fails with the existing unsupported-schema error before the typed parse | implemented ([[../skill/spec#ScanError]]) |
| FR-034.5 | The sidecar types live in `mcp-core::metadata`; annotations obey the untrusted-hint rule of [[../introspector/spec#9. Annotation Capture (#568)]] | implemented |
| FR-034.6 | The stored title is sanitized and length-capped (`AnnotationTitle`) | implemented |
| FR-046.1 | `manifest_size_bytes` and `result_type_origin` are the per-tool size and output-schema flag the measurement needs; no measurement-specific field exists | implemented |

**Size cap.** `MAX_METADATA_FILE_BYTES` is the cap `read_server_metadata` applies for every consumer
(`mcp-skill`'s `MAX_FILE_SIZE` is defined from it) and the size at which `mcp-codegen` warns when writing it. A
sidecar above it still generates; the skill scan then returns `ScanError::FileTooLarge`
([[../codegen/spec#9. Resource-Exhaustion Bounds (CWE-400)]]). The producer writes compact JSON.

**What the version bump costs the user.** The shared reader compares `schema_version` before the typed parse
(`MetadataReadError::SchemaMismatch`), which `mcp-skill` maps to `ScanError::UnsupportedSchema` ("unsupported
metadata schema version: found {found}, expected {expected} (re-run 'generate' to regenerate this
server)"). Every server directory generated before the v3 upgrade is therefore unreadable to the
skill scan and to the server tools that read the sidecar until the user reruns
`mcp-execution-cli generate --from-config <SERVER>`. This repeats the 1-to-2 bump of v0.10; the
README carries a warning in the v0.10 callout form and `CHANGELOG.md` a "Breaking" entry.

`server_id`/`name` are `ServerId`/`ToolName` (issue #317, previously bare `String`); both
newtypes' derived `Serialize`/`Deserialize` round-trip through a plain JSON string, so this is
not a wire-format change. `typescript_name` stays `String` — it is a generated TypeScript
identifier, not itself an MCP tool name.

This is the **wire contract** between `mcp-codegen` (producer, writes
`_meta.json`) and `mcp-skill`/`mcp-server` (consumers). Bumping
`METADATA_SCHEMA_VERSION` is the intended way to signal a breaking shape
change; consumers compare it and fail loudly on mismatch (see
[[../skill/spec#ScanError]]).

`INDEX_FILE_NAME` (issue #487) is the filename of the generated re-export entry point
(`index.ts`) `mcp-codegen` always emits alongside per-tool files; `CATALOG_FILE_NAME` (#566) is the
filename of the type-only catalog. Shared so `mcp-server`'s `list_generated_servers` tool-count scan
and `mcp-skill`'s sidecar-verification pass both recognize each as a package file rather than a
per-tool `.ts` file, matched case-insensitively in the server scan — see
[[../server/spec#6. Output Directory Resolution (`output_dir.rs`)]].

`provenance` (issue #468) is required, not `Option`: a `schema_version: 1` sidecar has no
`provenance` key at all, and a consumer is expected to check `schema_version` *before* attempting a typed `ServerMetadata` deserialization (done inside
`read_server_metadata`; see [[../skill/spec#ScanError]]) — so every `ServerMetadata` a consumer actually constructs already
carries real provenance; an `Option` would encode a state the version check has already ruled
out.

### `annotations` module (`src/annotations.rs`, #568)

```rust
pub enum Hint { StatedTrue, StatedFalse, NotStated }   // serde snake_case; impl From<Option<bool>>
pub struct AnnotationTitle(String);                    // private field
impl AnnotationTitle { pub fn new(raw: &str) -> Option<Self>; pub fn as_str(&self) -> &str; }
pub struct ToolAnnotations { pub title: Option<AnnotationTitle>, pub read_only: Hint, pub destructive: Hint, pub idempotent: Hint, pub open_world: Hint }
```

Annotations are **untrusted, advisory** values: the server is the only source and nothing verifies
them. Each hint is three-valued so "the server said `false`" differs from "the server said
nothing"; no consumer may read a missing annotation as safe, and no code branches on a hint to relax
a check or choose a path. `AnnotationTitle::new` runs `sanitize_untrusted_text(raw,
MAX_UNTRUSTED_FIELD_LEN)` and returns `None` when the result is empty; the serde form is a
`String` routed through `try_from`/`into`, so deserialization re-sanitizes and rejects an empty
title (the inner string is private, so an unsanitized title cannot exist). `ToolAnnotations` has
**no `Default`**: every field value is legal, and "no annotations" is `Option::None` on the carrying
type (`ToolInfo.annotations`, `ToolMetadata.annotations`), never an all-`NotStated` value. A caller builds
a value from `ToolAnnotations::not_stated()` (`const`, every hint `NotStated`, no title) and sets the fields by name;
there is no positional constructor, so two hints cannot be swapped silently, and no `Default`, so
`Option::<ToolAnnotations>::unwrap_or_default()` does not exist. Contradictory
hints (read-only with destructive) are stored as advertised.

### `provenance` module (`src/provenance.rs`)

```rust
pub struct GenerationProvenance { generated_at: DateTime<Utc>, config_fingerprint: ConfigFingerprint, tool_digest: ToolDigest }
pub struct ConfigFingerprint(String); // 64 lowercase hex chars (SHA-256)
pub struct ToolDigest(String);        // 64 lowercase hex chars (SHA-256)
pub struct ToolEntryDigest(String);   // same form, one tool entry; derives Eq + Ord (#688)
pub struct ToolDigestEntry<'a> { name: &'a str, description: &'a str, input_schema: &'a Value, output_schema: Option<&'a Value>, annotations: Option<&'a ToolAnnotations> } // #[non_exhaustive], pub fields, `new(name, description, input_schema)`
pub struct DigestFormatError { value: String } // not exactly 64 lowercase hex chars
impl GenerationProvenance { pub fn capture(config: &ServerConfig, tools: &[ToolDigestEntry<'_>]) -> Self; }
impl ConfigFingerprint { pub fn compute(config: &ServerConfig) -> Self; }
impl ToolDigest { pub fn compute(entries: &[ToolDigestEntry<'_>]) -> Self; }
impl ToolEntryDigest { pub fn compute(entry: &ToolDigestEntry<'_>) -> Self; pub fn as_str(&self) -> &str; }
impl TryFrom<String> for ToolEntryDigest; // Error = DigestFormatError
```

`ToolEntryDigest::compute` is the per-entry hash the aggregate is built from (same domain tag `tool-entry:v2`, so
aggregate values are unchanged): it changes with the name, description, input or output schema or any hint, and not
with the annotation title text. Its `Deserialize` is validated like the two types below, so a malformed `digest` makes
the whole sidecar unreadable (`MetadataReadError::Malformed`) for every consumer; this is deliberate, a corrupt value
means regenerate.

`ConfigFingerprint`/`ToolDigest`'s `Deserialize` is routed through `TryFrom<String>` (via
`#[serde(try_from = "String")]`), mirroring `ServerId`/`ToolName`'s validated-newtype pattern: no
separate, unvalidated deserialization path exists, so a hand-edited `_meta.json` cannot produce
one of these types holding a value that violates `as_str`'s documented "64-character
lowercase-hex" contract. Both types' own `compute` is the only internal construction path and
always produces a valid value, so this validation only ever fires against externally-sourced
(deserialized) data.

#### What provenance does and does not answer

Provenance answers **"has the server's exposed surface, or the identity of the endpoint we
generated from, changed since generation?"** It does **not** answer "would re-running `generate`
today produce byte-identical files?" — `mcp-codegen`'s collision-disambiguating TypeScript name
suffixes (index-based, not content-based) and its `categorizations` input both affect generated
output without affecting either digest. Any future comparison mechanism and all user-facing
wording must be phrased as "the server changed", never "your files are out of date
byte-for-byte".

#### Hashing

Both digests are SHA-256 (via the `sha2` crate, `default-features = false`), hex-encoded
lowercase via an explicit `{:02x}` loop (`sha2`'s digest type has no `LowerHex` impl). Preimages
are never built from `Debug` output (no stability guarantee — the same objection that rules out
`std::hash::DefaultHasher`) or from serializing a `serde_json::Value` (this workspace enables
`serde_json/preserve_order` explicitly in the workspace manifest (#514; it was previously only transitive via `handlebars`), and a `HashMap`-backed preimage would
vary by process regardless). Instead, one length-framing primitive (`u64` big-endian length ‖
bytes) builds every preimage from an explicitly sorted, fixed field order.

`ConfigFingerprint::compute`'s preimage carries, in order: a domain tag, the transport
discriminant (`stdio`/`http`/`sse`), then either `command` + `cwd` presence + argument *count* +
sorted environment variable *names* (stdio), or the URL's canonical `scheme://authority/path`
form + deduplicated sorted query-parameter *names* + a userinfo-present marker + sorted
ASCII-lowercased header *names* (http/sse). **No argument value, environment/header value,
query-parameter value, or userinfo is ever fed** — this is a deliberate secrecy guarantee, not
an oversight: rotating a credential must never register as configuration drift. The URL is
parsed via `redact::split_url` (the same parser `RedactedUrl`'s `Debug` impl uses, factored out
so the two can't disagree about where the authority ends), never via `RedactedUrl`'s `Debug`
output itself. A bare query parameter (no `=`) and an unparseable URL each contribute a
type-tagged marker rather than literal `<bare>`/`<unparseable>` text, so a real parameter or URL
segment that happens to contain that literal string cannot collide with the marker.

`ToolDigest::compute` is two-level: each tool entry is hashed on its own (domain tag, framed
`name`/`description`, then `input_schema`/`output_schema` via a recursive `serde_json::Value`
walk that type-tags every variant and sorts object keys at hash time), the resulting 32-byte
digests are sorted, and the sorted sequence is hashed into the aggregate — giving a total order
even for two tools sharing a name. `output_schema`'s `Option` wrapper gets an explicit presence
byte in the same framing `cwd` already uses, so `None` and `Some(Value::Null)` hash differently.

**Annotations in the entry (#568).** After the schemas, each entry feeds its annotations: one byte
for whether an annotations value is present at all, then (when present) the four hints with
three-valued framing (`NotStated` 0, `StatedFalse` 1, `StatedTrue` 2), so a stated `false` hashes
differently from an unstated hint, and `None` differs from an all-`NotStated` value. The **title text
is never hashed**, so a retitle is not drift (the title matters only through the annotations value
existing). The entry domain tag is `mcp-execution:tool-entry:v2` (was `v1`): the preimage changed, so
every digest recorded before this change differs, which the schema-version bump already makes moot.

`GenerationProvenance::capture(config, tools)` stamps `Utc::now()` and computes both digests
from the same `ServerConfig`/tool list a `generate` call is generating from, so the recorded
digest can never drift from the emitted files — see [[../codegen/spec]].

### `home` module (`src/home.rs`, #634, #637, #604)

```rust
pub struct HomeDir(PathBuf);                       // always absolute
pub enum HomeDirError { Unknown, NotAbsolute { path: PathBuf }, UnsupportedUserHome { path: PathBuf } }   // thiserror
pub fn expand_home(path: &Path) -> Result<PathBuf, HomeDirError>;
pub struct ServersDir(PathBuf);                    // default ~/.claude/servers
pub struct SkillsDir(PathBuf);                     // default ~/.claude/skills
impl HomeDir { pub fn from_env() -> Result<Self, HomeDirError>; pub fn as_path(&self) -> &Path }
impl ServersDir { pub fn resolve(Option<PathBuf>) -> Result<Self, HomeDirError>; pub fn server(&self, &ServerId) -> PathBuf; pub fn as_path(&self) -> &Path }
impl SkillsDir  { pub fn resolve(Option<PathBuf>) -> Result<Self, HomeDirError>; pub fn skill(&self, &ServerId) -> PathBuf; pub fn as_path(&self) -> &Path }
```

The one place the home directory is resolved; `mcp-cli`, `mcp-server`, `mcp-files` and `sanitize_path_for_error` all go
through it.

- **Source.** `HomeDir::from_env` calls `std::env::home_dir`, which honours a non-empty `HOME` (falling back to the account
  database) on Unix and a non-empty `USERPROFILE` (falling back to the profile directory) on Windows. The filesystem is not
  touched. `Unknown` when no source names a home; `NotAbsolute` (whose text names the offending path) when it is relative,
  because a relative home would make every derived path depend on the working directory.
- **Ban.** The workspace `clippy.toml` lists `dirs::home_dir` and `std::env::home_dir` under `disallowed-methods`; the only
  `#[expect]` is in `HomeDir::from_env`. `dirs` is no longer a dependency of `mcp-core`, `mcp-files` or `mcp-server`;
  `mcp-cli` keeps it for its runtime directories (`runtime/dirs.rs`), which are not covered by the ban.
- **`expand_home`.** Only a first component that is exactly `~` is replaced (`~`, `~/x`, and `~\x` on Windows). A first component that starts with `~` and is not exactly `~` (`~user/x`, `~~`)
  is rejected with `UnsupportedUserHome { path }` (#647): the user database is never consulted, and its text ("a '~name' path (another user's home directory) is not supported; use '~/...' or an absolute path, or './~name' for a directory with that literal name") never echoes the path. `./~name` is returned unchanged. The CLI runner maps it to `INVALID_INPUT` for the options the CLI expands itself;
  `mcp-files` wraps it in the typed `FilesError::HomeDirectory { source }` (#657), which the CLI classifier maps to `INVALID_INPUT` for `UnsupportedUserHome` as well. `x/~` and `.~` are returned unchanged. A path without a leading `~` never reads the home, so an absolute path works
  when the home is unusable.
- **`ServersDir` / `SkillsDir`.** `resolve(None)` is the default location under the home; `resolve(Some(p))` is
  `expand_home(p)`. `server()` / `skill()` join a `ServerId`, which rejects separators and `..`, so the result cannot leave
  the base.

### `path` module (`src/path.rs`)

```rust
pub fn sanitize_path_for_error(path: &Path) -> String; // redacts home dir (from `HomeDir::from_env`) to "~", falls back to scrubbing bare username
pub fn validate_path_segment(segment: &str) -> Option<Component<'_>>; // single plain component, no "..", no separator
pub fn first_disallowed_identifier_char(s: &str) -> Option<char>; // first char failing UTS #39 Identifier_Status=Allowed
pub fn contains_parent_dir(path: &Path) -> bool; // true if any component is `..`
#[doc(hidden)]
pub fn is_reserved_device_name(name: &str) -> bool; // workspace-internal, see `ServerSlug` (#701)
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
`ServerSlug` already cover; Unicode-identifier safety is a display/LLM-spoofing
concern specific to `ServerId::new`/`ToolName::new`'s construction-time gate, not a filesystem
concern. Backed by the `unicode-security` crate's `GeneralSecurityProfile::identifier_allowed`
(Unicode 16.0 tables); does not detect homoglyphs (see `ServerId`/`ToolName`'s own doc comment
above for the full rationale).

`validate_path_segment` backs `ServerId::new`/`ToolName::new`'s own baseline invariant (see
above) and, since #395, is used internally by `confinement::resolve_confined_path` as its own
generic, looser structural check on the `segment` (`server_id`) it's given —
`resolve_confined_path` stays a generic reusable primitive, not coupled to the `server_id`
domain concept specifically. It is *not* the primary rule of the two `server_id` confinement paths:
`mcp-skill::resolve_skill_output_path` takes a `&ServerSlug`, so the slug rule holds by type, and the
workspace-internal `mcp-server::output_dir::resolve_output_dir` (which takes `&str`) builds a `ServerSlug`
before calling into `resolve_confined_path`. `resolve_confined_path`'s internal re-check (and, on its
`ConfinementError::InvalidSegment` path, each crate's `From<ConfinementError>` impl re-deriving a
`ServerIdSlugError` through `ServerSlug::new`) exist as defense-in-depth.
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
pub async fn resolve_confined_existing(
    base_dir: &Path,
    relative: &Path,
) -> Result<PathBuf, ConfinementError>;
pub async fn write_confined_file(path: &Path, content: &[u8]) -> Result<(), ConfinementError>;
pub fn open_confined_write(path: &Path) -> std::io::Result<std::fs::File>;
```

`resolve_confined_existing` (issue #486) is the read-only counterpart for callers that list a
directory inside the base and have no `segment`: it creates nothing, returns a non-existent
joined path as-is (the caller's own existence check yields an empty result), rejects a rooted,
prefixed or `..`-containing `relative` as `Escape`, and, when the path exists, canonicalizes and
re-checks it against the canonicalized `base_dir` (an in-base symlink is resolved, not rejected,
unlike `resolve_confined_path`'s segment directory). "Missing" is `tokio::fs::metadata` failing for any reason, as with `Path::exists`: a dangling symlink,
a non-directory parent or an unreadable parent (`EACCES`) are returned joined, not as errors (the
link is never followed, so nothing escapes). An empty `relative` returns `base_dir`.

Since issue #461, `ConfinementError::InvalidSegment`'s `segment` field holds a *sanitized* form
of the rejected segment, not the raw one — `resolve_confined_path` runs it through
`untrusted::sanitize_untrusted_inline` before constructing the error, mirroring `ServerId`/
`ToolName`'s own error-construction sanitization (see [[#ServerId / ToolName (src/types.rs)]])
since a rejected `server_id` segment is attacker-controlled and reaches the same LLM-facing/
terminal-rendered error text. The `{segment:?}` (`Debug`) formatting on `InvalidSegment`'s
`#[error(...)]` is a required second layer of defense on top of that sanitization, not
incidental — it still escapes the residual characters `sanitize_untrusted_inline` deliberately
leaves untouched (e.g. U+200D). Every caller that maps `ConfinementError::InvalidSegment` into
its own `InvalidServerId` variant (`mcp-server::OutputDirError`) moves this field verbatim without
re-sanitizing, so this is the sole sanitization point for that crate's `server_id` confinement errors.
`mcp-skill::OutputPathError` has no such variant since #691.

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

**Directory creation is idempotent under concurrency (issue #491).** `resolve_confined_path`'s
segment-directory and lenient-intermediate-directory creation both tolerate
`ErrorKind::AlreadyExists` from `create_dir`: when two callers race to resolve the same
not-yet-existing directory, the loser's `create_dir` failing with `AlreadyExists` no longer
surfaces as `ConfinementError::CreateDir`. Instead the loser re-runs the exact same
existing-component validation the winner's directory would have gone through had it already been
there when the walk started (segment directories: rejected if the winner's entry is a symlink or
non-directory; lenient intermediate directories: resolved and re-confined if the winner's entry is
a symlink, consistent with that walk's existing lenient policy) — tolerating `AlreadyExists` never
skips or weakens confinement validation, it only changes which of the two racing callers performed
the `create_dir` syscall that won. This is the only race `resolve_confined_path` defends against;
it still does not defend the terminal `target` component against a symlink planted between this
function's checks and a caller's subsequent write — that gap is what `write_confined_file` closes,
below.

**`write_confined_file` closes the write-time symlink race for the terminal path (issue #496).** A
caller that resolved a [`ConfinementTarget::File`] path and is about to create it faces a gap
`resolve_confined_path` itself cannot close: the terminal component is deliberately checked but
never created, so a symlink planted at that exact path between the check and a plain
`tokio::fs::write` would be followed, redirecting the write outside the confined directory.

The entire check-then-write sequence runs inside one `tokio::task::spawn_blocking` call, using
`std::fs` rather than `tokio::fs`. This is load-bearing, not incidental: an earlier async-native
version built from `tokio::fs::OpenOptions` plus `AsyncWriteExt::write_all` had a real regression —
each `.await` point (`open`, `write_all`, `flush`) was a place a dropped future (e.g. `rmcp`
dropping the handler future on client disconnect) could land between `O_TRUNC` truncating the file
and the content being written, leaving a 0-byte file where `tokio::fs::write` would have written
the full content. A single `spawn_blocking` call restores `tokio::fs::write`'s original property:
once the blocking task is queued, dropping the returned future does not stop or partially undo a
write that already started running on the blocking pool.

On Unix, `write_confined_file` opens with `O_CREAT | O_TRUNC | O_NOFOLLOW` in a single syscall —
there is no separate check-then-write step for a racing process to land in between *for that exact
path*, and a pre-existing *regular* file is still opened and truncated normally (`O_NOFOLLOW` only
rejects a symlink at the final component), so overwrite semantics for a caller's own
`exists()`/`overwrite` decision are unaffected.

Windows has no usable equivalent for this specific open. `custom_flags` is exposed there too
(`std::fs::OpenOptions::custom_flags` via `std::os::windows::fs::OpenOptionsExt`), but the
candidate flag, `FILE_FLAG_OPEN_REPARSE_POINT`, is documented by `CreateFileW` as unusable
together with `CREATE_ALWAYS` — which is exactly what `create(true).truncate(true)` maps to — and
even where it *can* be used, it does not reject on open the way `O_NOFOLLOW` does: it hands back a
handle to the symbolic link itself, which a subsequent write would still land on. So Windows relies
solely on an immediately-preceding `std::fs::symlink_metadata` check, still inside the same
blocking closure and so still with no yield point in between it and the `open` call. That narrows
the window against a symlink already present when the check runs, but — unlike the Unix path —
remains genuinely open, not just narrowed, against a symlink a racing *process* plants between the
check and the open, since there is no single-syscall check-and-open available on this platform.
This crate's test suite has no Windows symlink coverage (creating one needs a privilege most CI
runners don't grant).

**Residual gaps — deliberately not claimed as closed.** `O_NOFOLLOW` only guards the *terminal*
path component, and only on Unix. Something with write access further up the confined path (e.g.
`{server_id}/` itself) could rename that directory aside and drop a symlink in its place after the
caller's confinement check but before the write — the open then traverses the symlinked *parent*
and still escapes, one directory level up from what a flag on the terminal `open` call can see. A
hardlink planted at the target path (same filesystem) is also not a symlink, so `O_NOFOLLOW` does
not reject it, and the write clobbers whatever the hardlink points at. Closing either would need a
directory file descriptor captured during the confinement walk itself (`openat`/`openat2` with
`RESOLVE_NO_SYMLINKS` on Linux) rather than a flag on the terminal open call alone — out of scope
for the issue #496 fix, and not silently claimed as covered. On Windows, the process-race gap
described above is a third, platform-specific residual on top of these two.

**`open_confined_write` exposes the same guard as its own synchronous primitive (issue #504).**
`write_confined_file` writes caller-supplied content directly to its *final* path in one call; a
caller that instead needs to stage into a separate `.tmp` path and `rename` it into place —
`mcp-execution-files`' `write_file_atomic` — cannot reuse `write_confined_file` itself, since the
symlink race it closes is specific to the exact path passed in (the `.tmp` staging path, not the
final one). `open_confined_write(path: &Path) -> std::io::Result<std::fs::File>` is the check-and-open
body extracted out of `write_confined_file`'s blocking closure — same `O_NOFOLLOW`/`symlink_metadata`
guard, same residual gaps noted above — so both crates share one implementation instead of each
hand-rolling its own. It is synchronous and not wrapped in `spawn_blocking`, since
`mcp-execution-files`' export path has no `tokio` runtime dependency to preserve; callers that need
the same future-drop atomicity `write_confined_file` provides must still run it inside their own
`spawn_blocking` (or equivalent) rather than calling it directly from async code. Its doc comment
carries the same "Residual gaps, even on Unix" caveats (parent-directory symlink swap, planted
hardlink not rejected) as `write_confined_file`, so callers integrating the primitive see them at
the call site (issue #507).

`ConfinementError` is a closed set every caller maps with a **total, 1:1** `From` impl into its
own pre-existing, byte-identical error enum — `mcp-server::OutputDirError` and
`mcp-skill::OutputPathError` — rather than matching on `ConfinementError` directly. The single
`ConfinementError::WrongTargetKind` variant carries two different truthful call-site meanings:
`OutputDirError::NotADirectory` (the terminal `resolve_output_dir` walks is always a directory)
and `OutputPathError::NotAFile` (the terminal `resolve_skill_output_path` walks is always a
file) — one core variant, zero unreachable arms in either `From` impl. `ConfinementError::InvalidSegment`/`SegmentIsSymlink`/`Escape`/`NotADirectory`/`CreateDir`/`Io` map
1:1 by name to each crate's own variant of the same name (`SegmentIsSymlink` →
`ServerDirIsSymlink`/`ServerIdIsSymlink`). `InvalidSegment` maps to `OutputDirError::InvalidServerId`; in
`mcp-skill`, whose caller already holds a `ServerSlug`, it has no variant and is reported as `Io` (#691). The absolute-path, `..`-component,
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
pub fn sanitize_untrusted_inline(s: &str) -> String;  // sanitize_untrusted_text(s, MAX_UNTRUSTED_FIELD_LEN), then escapes &, <, > in place — same escaping as wrap_untrusted_block's body but with no surrounding boundary markers
pub fn wrap_untrusted_block(context: &str, body: &str) -> String;  // escapes &, <, > in body; wraps in <untrusted-data>...</untrusted-data>
pub const BARE_IDENTIFIER_PATTERN: &str = "^[A-Za-z_$][A-Za-z0-9_$]*$"; // JS RegExp source rendered into the runtime bridge
pub fn is_bare_identifier(s: &str) -> bool; // the same rule in Rust; the one definition used by codegen (quoting, observed shapes) and skill (parameter display) (#600)
```
Threat model: an introspected MCP server's tool names/descriptions/keywords
are attacker-controlled from this project's perspective. Both `sanitize_untrusted_text`/
`wrap_untrusted_block` exist because `mcp-skill` (SKILL.md + prompt generation) and
`mcp-server` (introspection summaries returned to Claude) both embed this
data into text an LLM later reads as instructions — see
[[../server/spec#Prompt injection defense]] and
[[../skill/spec#Prompt injection defense]].

Since issue #446, `sanitize_untrusted_inline` (added because `wrap_untrusted_block`'s boundary
markers don't fit a single-line message like an error's `Display` output) is used by
`ServerId`/`ToolName`'s error-construction sites ([[#ServerId / ToolName (src/types.rs)]]) to
sanitize and escape a rejected identifier before it is stored in the error, since that value is
attacker-controlled and reaches LLM-facing text with no `wrap_untrusted_block` boundary around
it.

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

1. **Whole-value total** (`MAX_TOTAL_VARIATION_SELECTORS`, `pub` since #570, currently 16): if the value's
   total variation-selector count, summed across every run regardless of how it's distributed
   across base characters, exceeds this bound, every variation selector in the value is dropped.
2. **Per-run threshold** (`MAX_VARIATION_SELECTOR_RUN`, `pub` since #570, currently 2), applied only when
   the total stays under the bound above: a run of at most 2 consecutive variation selectors is
   left untouched (covers the normal single-selector case plus the occasional legitimate second
   selector); a longer run is dropped in full.

Both constants and the "drop the whole run/value, not just the excess" choice are documented in
code in `src/untrusted.rs`. The constants are `pub` so `mcp-codegen` can render them into the
generated bridge's `boundServerText` instead of copying them; shared test vectors pin the Rust and
TypeScript implementations together.

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

### `rmcp` log cap (`cli::RmcpLogCap`, workspace-internal, #608, #678)

```rust
#[doc(hidden)]
#[non_exhaustive]
pub enum RmcpLogCap { Info, Warn }
impl RmcpLogCap {
    pub const fn directive(self) -> &'static str; // "rmcp=info" | "rmcp=warn"
    pub fn cap_filter_directives(self, base_filter: &str) -> String;
}
```

The one statement of the `rmcp` log cap policy, free of `tracing` types, so `mcp-core` does not depend on
`tracing-subscriber`. It is workspace-internal (`NFR-COM-032`): not part of the documented 1.0 API. `mcp-cli` uses
`Info`, or `Warn` for the quiet profile of `call`, `run` and `helper`; `mcp-server` uses `Info`. The former
`log_filter` module (`cap_rmcp_log_level`) and the `tracing-filter` feature were removed (#678).

The cap is a second filter combined with the base filter by logical AND, so it only lowers verbosity.
`cap_filter_directives(base_filter)` returns `trace`, then `directive()`, then every comma-separated directive of
`base_filter` that starts with `rmcp::` and has no span or field part, comma-joined. Those repeated directives keep
their targets exempt from the cap; an `rmcp::` directive with a span or field part is not repeated, so the cap applies
to it. Each binary parses the result into a `Targets` filter and replaces the `trace` default with the base filter's
max level hint, so the cap does not raise the subscriber's max level for targets outside `rmcp`.

### Untrusted-text vectors (#624)

`sanitize_untrusted_text` is pinned to hand-written vectors in `crates/mcp-core/tests/fixtures/untrusted_text_vectors.json`,
run by `crates/mcp-core/tests/untrusted_text_vectors.rs`. The expected outputs come from the documented rules, never from
a capture of the function. The `untrusted_text` section of `mcp-codegen`'s `shared_vectors.json` no longer exists (no TypeScript consumer was left;
[[../codegen/spec]]).

## 3. Cross-Crate Contracts

| Consumer | What it depends on from `mcp-core` |
|---|---|
| `mcp-introspector` | `ServerConfig`, `ServerId`, `ToolName`, `Transport`, `validate_server_config`, `Error`/`Result`, `annotations::{AnnotationTitle, Hint, ToolAnnotations}` |
| `mcp-codegen` | `Error`/`Result`, `is_reserved_device_name` (workspace-internal), `metadata::*` (writes `_meta.json`, v3), `annotations::*`, `provenance::ToolDigestEntry` (with `annotations`), `untrusted::{MAX_UNTRUSTED_FIELD_LEN, MAX_VARIATION_SELECTOR_RUN, MAX_TOTAL_VARIATION_SELECTORS}` and `untrusted::BARE_IDENTIFIER_PATTERN` (rendered into the bridge), `helper_protocol::*` (rendered into the bridge via `BridgeContext`), `provenance::ToolEntryDigest` |
| `mcp-files` | `Error`/`Result` indirectly via `mcp-codegen`; `confinement::open_confined_write` directly (issue #504) |
| `mcp-skill` | `sanitize_path_for_error`, `contains_parent_dir`, `is_reserved_device_name` (workspace-internal), `ServerSlug`, `ServerId`, `ServerIdSlugError`, `MAX_SERVER_ID_LENGTH`, `untrusted::*`, `metadata::*` (including `read_server_metadata_async`), `confinement::{ConfinementError, ConfinementTarget, resolve_confined_path}` |
| `mcp-server` | `ServerConfig`, `ServerId`, `ServerSlug`, `ServerIdSlugError`, `sanitize_path_for_error`, `contains_parent_dir`, `untrusted::*`, `metadata::{CATALOG_FILE_NAME, INDEX_FILE_NAME, METADATA_FILE_NAME, ServerMetadata}`, `cli::{LogFormat, LOG_FORMAT_ENV_VAR, RmcpLogCap}`, `confinement::{ConfinementError, ConfinementTarget, resolve_confined_path, write_confined_file}` |
| `mcp-cli` | `cli::{OutputFormat, ExitCode, LogFormat, LOG_FORMAT_ENV_VAR, RmcpLogCap}`, `helper_protocol::*` (including `ResultSource`), `metadata::{read_server_metadata, read_server_metadata_async, MetadataReadError}`, `ServerConfig`/`ServerConfigBuilder`, `ServerSlug`, `ServerIdSlugError`, `provenance::{ConfigFingerprint, ToolDigest, ToolDigestEntry, ToolEntryDigest}`, `RedactedItems`/`RedactedUrl`, `Error` (for exit-code classification) |

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
`mcp-server::service::introspect_server`'s tighter `ServerSlug` charset check) to have
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
| Two concurrent callers resolve the same not-yet-existing segment directory or lenient intermediate directory (issue #491) | Both succeed — the loser's `create_dir` `AlreadyExists` is tolerated, and the winner's directory is re-validated under the same rule (strict/symlink-rejecting for the segment directory, lenient/symlink-resolving for an intermediate directory) rather than trusted unconditionally |
| `create_dir` fails with an error other than `AlreadyExists` (e.g. permission denied) while resolving the segment directory or a lenient intermediate directory | Surfaces as `ConfinementError::CreateDir`, unchanged from before #491 |
| `write_confined_file` on a path whose terminal component is a symlink (dangling or not), planted after a caller's own confinement check on Unix (issue #496) | Rejected on open — the `O_NOFOLLOW` open fails (`ELOOP`, or a platform-specific equivalent errno on some BSDs) |
| Same, on Windows | Rejected only if the pre-existing `symlink_metadata` check (run just before `open`, same blocking closure) observes the symlink — genuinely TOCTOU against a symlink a racing *process* plants between that check and `open`, since Windows has no single-syscall `O_NOFOLLOW` equivalent usable with a create+truncate open |
| `write_confined_file` on a path that already exists as a regular file | Opened and truncated normally, same as a plain `tokio::fs::write` — `O_NOFOLLOW` does not affect non-symlink targets |
| `write_confined_file`'s caller drops the returned future (e.g. client disconnect) after the write has started | No partial/0-byte file — the whole check-then-write sequence runs inside one `spawn_blocking` call, which cannot be interrupted once queued, matching `tokio::fs::write`'s original atomicity |
| `write_confined_file` on a path whose *parent* directory is swapped for a symlink after the confinement check but before the call, or on a path that is a pre-existing hardlink to a file outside the confined directory | **Not** defended against on any platform — `O_NOFOLLOW` guards only the terminal component (and only on Unix); documented as a residual gap, not silently claimed as closed |
| `ServerId::new`/`ToolName::new` rejects an input containing `&`, `<`, `>`, a control character, or a bidi override (issue #446) | The rejected value is sanitized (`untrusted::sanitize_untrusted_inline`) and `&`/`<`/`>` entity-escaped before being stored in `ServerIdError`/`ToolNameError`, so the raw disallowed characters never reach the `Display`/`Debug`-formatted error text an LLM or terminal renders |
| `ServerId::new`/`ToolName::new` rejects an input containing emoji | **Not** sanitized or stripped — a deliberate accepted-risk exception, not an oversight: emoji is printable, cannot forge Markdown/HTML-like structure the way `&`/`<`/`>` can, and stripping arbitrary non-identifier printables would need a new allowlist pass that risks corrupting legitimate non-ASCII text elsewhere in the sanitizer |

## 6. Helper Protocol and Exit Codes (as built)

- **`helper_protocol` module** (`src/helper_protocol.rs`): `HELPER_PROTOCOL_VERSION` 1; `MAX_RESPONSE_LINE_BYTES`
  4 MiB (moved from the introspector); `HELPER_ENVELOPE_SLACK` 64 KiB; `MAX_HELPER_LINE_BYTES` = response line plus
  slack; `HELPER_HELLO_TIMEOUT` 5 s; the environment-name constants (`HELPER_ENV`, `HELPER_TOKEN_ENV`,
  `CLI_PATH_ENV`); and `HELPER_CHANNEL_KEY`, the `Symbol.for` key under which the preload registers the channel, so
  the preload and the generated bridge render the same key (added in wave 2, additive, approved by the architect);
  and `RUN_MARKER_ENV` (`MCP_EXECUTION_RUN`), the marker set to `1` on the whole process tree under `run`.
  The generated bridge renders the version, the line bound and the names from here ([[../codegen/spec]]).
- **`ExitCode`** gains `CHECK_FAILED` (5: drift found or `scripts check` problems) and `SCRIPT_FAILED` (6: a script
  exited non-zero or was killed by a signal); success 0, error 1, invalid input 2, server error 3, timeout 4. #627 adds
  `INTERRUPTED` (130, the runner stopped by SIGINT) and `TERMINATED` (143, SIGTERM), `128 + signal number` as shells
  report them; #646 adds `HANGUP` (129, SIGHUP) and `QUIT` (131, SIGQUIT) the same way; `run` is the only command that returns them ([[../cli/spec#`run` (#571, `commands/run.rs`, `runtime/{exec,notice,import_hook}.rs`)]]).

## 7. Per-tool digests (#688)

> [!success] Implemented
> The granularity left out of #580 (#683) landed before 1.0.0 as the optional `tools[].digest` field of
> `_meta.json` version 3 (`NFR-COM-053`, `NFR-COM-056`; amendment of D4 in
> [[../decisions/ADR-670-1-0-api-wire-release-contract]]). No schema version bump and no change of the aggregate
> preimage. A version 3 reader that does not know the field ignores it.

**As built.**

- `generate` records `ToolEntryDigest::compute(&tool.digest_entry())` for every tool, so the value comes from the
  same entry mapping as the aggregate and as the live side of `drift`.
- **All or nothing under the size cap.** When the serialized file with digests would exceed
  `MAX_METADATA_FILE_BYTES`, every digest is omitted, the file is serialized again and one `tracing::warn!` says
  that drift cannot name changed tools for this server. The cap is unchanged; the over-cap warning still applies if
  the file stays above it ([[../codegen/spec#9. Resource-Exhaustion Bounds (CWE-400)]]).
- A sidecar without digests reads normally (`digest = None`); `ToolMetadata::new` sets `None`.
- Duplicate raw names need no keying in the file: each entry carries its own digest, and `drift` compares the set of
  digests per name ([[../cli/spec]]).
- The aggregate `tool_digest` stays and remains the first comparison.

| ID | Requirement | Status |
|---|---|---|
| FR-034.7 | WHERE annotations are included in the digest entries THE SYSTEM SHALL feed each hint as stated-true, stated-false or not stated, with distinct framing, so an absent hint and `false` hash differently | implemented (#568) |
| FR-045.1 | WHEN the drift check needs to name a changed tool THE SYSTEM SHALL have per-tool digests recorded in the sidecar; otherwise it SHALL report only that the list differs | implemented (#688) |
| FR-045.2 | THE SYSTEM SHALL compute the live digest with the same entry mapping and function that `generate` uses, and that mapping SHALL exist once | implemented |
| FR-045.3 | THE SYSTEM SHALL keep the existing secrecy guarantee: no credential value, argument value or header value enters any preimage | implemented |
| FR-045.4 | THE SYSTEM SHALL word results as "the server changed", never "your files are out of date byte for byte" (see the provenance wording rule above) | implemented |

**Constraints.** The digest preimage is a persisted contract: it is built from explicitly sorted, length-framed
fields and never from `Debug` or serialised JSON (hashing section above). Any change to it changes every recorded
digest, so it is Ask First and lands only with a schema bump.

## 8. 1.0 Public API Contract

> [!info] As built; the semver promise binds from the 1.0.0 tag
> Policy in [[../NFR-mcp-execution-2026-07-27#12. 1.0 Contract Requirements]] (`NFR-COM-030` to `NFR-COM-046`);
> decisions in [[../decisions/ADR-670-1-0-api-wire-release-contract]]. The listing describes the code after the
> API freeze (#673, #674 to #680) and the surface cut of #687 (`NFR-COM-031`: whatever is public at the tag is
> frozen).

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-047.1 | THE SYSTEM SHALL expose from `mcp-execution-core` only the surface listed in this section, mark `#[non_exhaustive]` every enum and growable struct not listed as closed, and keep every foreign type of its public signatures on the list below | must |

### Public surface

| Item | Kind | Notes |
|------|------|-------|
| `Error`, `ResourceKind`, `Result` | error types | Crate-wide error hierarchy; `Error::ConnectionFailed.server` is a `ServerId` |
| `ServerId`, `ToolName`, `ServerSlug`, `ServerIdError`, `ServerIdSlugError`, `ToolNameError`, `MAX_SERVER_ID_LENGTH` | typed identifiers | The validated server id of `NFR-COM-035` is built here; the slug rule excludes the Windows reserved device names (#696) |
| `ServerConfig`, `ServerConfigBuilder`, `Transport` | configuration | Validation runs at construction |
| `validate_server_config`, `validate_url_scheme`, the `MAX_ARG_*`, `MAX_ENV_*`, `MAX_HEADER_*`, `MAX_URL_LEN` bounds | validation | The bound values are documented limits (`NFR-COM-040`) |
| `HomeDir`, `HomeDirError`, `ServersDir`, `SkillsDir`, `expand_home` | directories | One resolution of home, servers and skills directories |
| `contains_parent_dir`, `first_disallowed_identifier_char`, `sanitize_path_for_error`, `validate_path_segment`, `ConfinementError`, `ConfinementTarget`, `open_confined_write`, `resolve_confined_existing`, `resolve_confined_path`, `write_confined_file` | path confinement | Shared by `mcp-execution-skill` and `mcp-execution-server` |
| `REDACTED_PLACEHOLDER`, `RedactedItems`, `RedactedMapValues`, `RedactedUrl`, `redact_urls_in_text` | redaction | Hand-written `Debug` discipline of the workspace |
| module `annotations` | tool annotations | `Hint`, `AnnotationTitle`, `ToolAnnotations` (`not_stated()`, no `Default`) |
| module `cli` | CLI contract types | `OutputFormat`, `LogFormat`, `ExitCode` (values frozen by `NFR-COM-061`), `ServerConnectionString`, `LOG_FORMAT_ENV_VAR` |
| module `helper_protocol` | wire constants and types | `HELPER_PROTOCOL_VERSION`, the line bounds, environment names, channel key and `ResultSource` (`NFR-COM-050`, `NFR-COM-034`) |
| module `metadata` | wire format | `METADATA_SCHEMA_VERSION` (3), `ServerMetadata`, `ToolMetadata`, `ParameterMetadata`, `ResultTypeOrigin`, `MetadataReadError`, `read_server_metadata`, `read_server_metadata_async`, file-name constants, `MAX_TOOL_COUNT`, `MAX_METADATA_FILE_BYTES` |
| module `provenance` | persisted record | `GenerationProvenance`, `ToolDigestEntry`, `ToolDigest`, `ConfigFingerprint`, `ToolEntryDigest` (`compute`, `as_str`, `Ord`), `DigestFormatError` (#688) |
| module `untrusted` | injection defense | `sanitize_untrusted_text`, `sanitize_untrusted_inline`, `wrap_untrusted_block`, `is_bare_identifier`, the bound constants |

### Workspace-internal (`NFR-COM-032`)

`cli::RmcpLogCap` (with `directive` and `cap_filter_directives`) and `is_reserved_device_name` (both
`#[doc(hidden)]`), excluded from the semver promise.

### Removed before 1.0.0 (#687)

`forbidden_chars`, `forbidden_env_names`, `forbidden_env_prefix`, `env_name_charset_pattern` and
`env_name_charset_desc` are removed: nothing outside this crate used them since the generated bridge stopped
mirroring the configuration validation (#575). The validation rules themselves are unchanged. The path-confinement
primitives, including the write primitives, stay public.

### Foreign types (`NFR-COM-044`, OD-4 (c))

`chrono::DateTime<Utc>` (`GenerationProvenance::generated_at`), `serde_json::Value` (`ToolDigestEntry`),
`serde_json::Error` (`MetadataReadError::Malformed`), `schemars::JsonSchema` (implemented by hand for `ServerSlug`;
`schemars` is a dependency of this crate since #677) and the `serde` implementations. No `tracing-subscriber` type
or dependency.

### `#[non_exhaustive]` and closed sets (`NFR-COM-042`, `NFR-COM-043`)

`#[non_exhaustive]`: `Error`, `ResourceKind`, `ServerIdError`, `ServerIdSlugError`, `ToolNameError`, `HomeDirError`,
`ConfinementError`, `MetadataReadError`, `Transport`, `OutputFormat`, `LogFormat`, `RmcpLogCap`, `ResultTypeOrigin`,
`ServerMetadata`, `ToolMetadata`, `ParameterMetadata`, `GenerationProvenance`, `ToolDigestEntry`, `ToolAnnotations`.
Structs built outside the crate have a constructor (`new` with the required fields; `ToolAnnotations::not_stated`).

Closed sets (adding a variant is a major change, stated in the rustdoc): `Hint`, `ConfinementTarget`,
`helper_protocol::ResultSource`.

### Owned wire formats

`_meta.json` version 3 (including the optional `tools[].digest`, #688) and helper protocol version 1 ([[#2. Public API Surface]] and
[[#6. Helper Protocol and Exit Codes (as built)]]); rules in `NFR-COM-050` to `NFR-COM-056`.

## 9. See Also

- [[../introspector/spec]] — primary consumer of `ServerConfig`/`validate_server_config`
- [[#7. Per-tool digests (#688)|Per-tool digests]] — the optional digest field; the v3 field registry is in §2 `metadata`
- [[../cli/spec]] — consumer of `cli::{OutputFormat,ExitCode}` and the error-classification contract
- [[../server/spec#6. Output Directory Resolution (`output_dir.rs`)]] / [[../skill/spec#8. `resolve_skill_output_path` — Path Confinement]] — the two `confinement::resolve_confined_path` consumers
- [[../constitution]] — security principles this crate embodies workspace-wide
- [[../NFR-mcp-execution-2026-07-27#12. 1.0 Contract Requirements]] — 1.0 API, wire-format, CLI and release contract
