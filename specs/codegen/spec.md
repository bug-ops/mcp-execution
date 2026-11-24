---
aliases:
  - mcp-execution-codegen spec
  - Codegen/Templating spec
tags:
  - sdd
  - spec
  - codegen
  - security
created: 2026-07-27
status: documented
related:
  - "[[../constitution]]"
  - "[[../introspector/spec]]"
  - "[[../files/spec]]"
  - "[[../core/spec]]"
  - "[[../runtime/spec]]"
---

# Block: Code Generation & Templating (`mcp-execution-codegen`)

> [!abstract]
> Path: `crates/mcp-codegen`. Renders a `ServerInfo` into a complete,
> self-contained TypeScript package via Handlebars templates: one `.ts` file
> per tool, an `index.ts` re-export, a runtime bridge, `package.json`,
> `tsconfig.json`, and a `_meta.json` metadata sidecar. Depends on
> `mcp-execution-core` and `mcp-execution-introspector`.

## 1. Responsibility

Convert `ServerInfo`/`ToolInfo` (JSON Schema-based tool definitions) into:
1. TypeScript source implementing the "progressive loading" pattern —
   Claude Code discovers tools via `ls`/`cat` on individual files instead of
   loading one large manifest (~500-1,500 tokens/tool vs. ~30,000 tokens for
   the whole set, per the crate's own doc comments).
2. A structured `_meta.json` sidecar (via `mcp-core::metadata`) that lets
   `mcp-skill`/`mcp-server` recover tool metadata **without re-parsing
   generated TypeScript** (replacing a fragile regex-based scanner that
   existed historically — issue #141's fix).

## 2. Public API Surface

```rust
// mcp_execution_codegen (crate root)
pub use common::types::{GeneratedCode, GeneratedFile, TemplateContext, ToolDefinition};
pub use progressive::ProgressiveGenerator;
pub use template_engine::TemplateEngine;

impl GeneratedCode {
    // Returns Error::DuplicateGeneratedFilePath if `file.path` is already present, instead
    // of silently overwriting the existing entry (issue #312).
    pub fn add_file(&mut self, file: GeneratedFile) -> Result<()>;
}

// mcp_execution_codegen::progressive
pub struct ProgressiveGenerator<'a> { /* engine: TemplateEngine<'a> */ }
impl<'a> ProgressiveGenerator<'a> {
    pub fn new() -> Result<Self>;
    pub fn generate(&self, server_info: &ServerInfo, server_config: &ServerConfig) -> Result<GeneratedCode>;
    pub fn generate_with_categories(&self, server_info: &ServerInfo, server_config: &ServerConfig, categorizations: &HashMap<String, ToolCategorization>) -> Result<GeneratedCode>;
}
pub struct ToolCategorization { pub category: String, pub keywords: Vec<String>, pub short_description: String }

// Four of BridgeContext's fields (forbidden_chars, forbidden_env_names, forbidden_env_prefix,
// env_name_charset_pattern) are private with read-only accessor methods of the same name —
// each could fail open (e.g. an empty forbidden_chars, or an env_name_charset_pattern of ""
// which `new RegExp('')` matches every string against) if a caller bypassed Default via a
// struct literal, so `pub` fields aren't used for these. The remaining fields — the DoS
// size/count ceilings (max_arg_count and siblings, see [[#9. Resource-Exhaustion Bounds
// (CWE-400)]]) and env_name_charset_desc (message text only, not enforcement) — are plain
// `pub`: a wrong value there cannot fail open, only reject more (or less clearly) than
// intended, so the accessor ceremony buys nothing. `BridgeContext::default()` is the only
// construction path (no `derive(Default)`, no `Deserialize`) — see [[#Runtime bridge]] and
// issues #315, #471, #467.
pub struct BridgeContext {
    /* forbidden_chars, forbidden_env_names, forbidden_env_prefix, env_name_charset_pattern: private */
    pub max_arg_count: usize,
    pub max_arg_len: usize,
    pub max_env_count: usize,
    pub max_env_value_len: usize,
    pub max_url_len: usize,
    pub max_header_count: usize,
    pub max_header_value_len: usize,
    pub env_name_charset_desc: String,
}
impl BridgeContext {
    pub fn forbidden_chars(&self) -> &[String];
    pub fn forbidden_env_names(&self) -> &[String];
    pub fn forbidden_env_prefix(&self) -> &str;
    pub fn env_name_charset_pattern(&self) -> &str;
}
impl Default for BridgeContext { /* populates every field from mcp_execution_core: forbidden_chars()/forbidden_env_names()/forbidden_env_prefix()/env_name_charset_pattern()/env_name_charset_desc() and the MAX_* constants — hand-written, so it can never render a fail-open (empty) bridge. forbidden_chars, env_name_charset_pattern, and env_name_charset_desc are each passed through sanitize_ts_string_literal before storing, so a future Rust-side value containing `'`/`\` renders as a valid (and correct) TS string literal rather than breaking or silently changing what it matches. */ }

pub const MAX_GENERATED_FILES: usize; // = introspector::MAX_TOOL_COUNT + 5 (fixed files)
pub const MAX_GENERATED_BYTES: usize; // = 2 * MAX_TOOL_COUNT * (MAX_TOOL_NAME_LEN + MAX_TOOL_DESCRIPTION_LEN + MAX_SCHEMA_SIZE_BYTES)

// mcp_execution_codegen::common::typescript
pub const MAX_SCHEMA_RECURSION_DEPTH: usize = 128; // see [[#Recursion Depth Bound]]
pub fn to_camel_case(s: &str) -> String;
pub fn to_pascal_case(s: &str) -> String;
pub fn sanitize_ts_identifier(s: &str) -> String; // collapses invalid-char runs to one '_'; prefixes '_' if leading digit/empty
pub fn json_type_to_typescript(json_type: &str) -> &'static str;
pub fn json_schema_to_typescript(schema: &serde_json::Value) -> String; // depth-capped at MAX_SCHEMA_RECURSION_DEPTH
pub fn extract_properties(schema: &serde_json::Value) -> Vec<serde_json::Value>;

// mcp_execution_codegen::template_engine
pub struct TemplateEngine<'a> { /* handlebars: Handlebars<'a> */ }
impl<'a> TemplateEngine<'a> {
    pub fn new() -> Result<Self>; // strict_mode(true), no_escape (see below)
    pub fn render<T: Serialize>(&self, template_name: &str, context: &T) -> Result<String>;
    pub fn register_template_string(&mut self, name: &str, template: &str) -> Result<()>;
}
```

`TemplateEngine::new` **disables Handlebars' default HTML-escaping**
(`register_escape_fn(handlebars::no_escape)`), since output is
TypeScript/JSDoc, not HTML. Injection safety is instead the responsibility
of the sanitizers documented below, run **before** rendering.

## 3. Input Contract

`generate`/`generate_with_categories` accept `&ServerInfo` (from
[[../introspector/spec]]), `&ServerConfig` (issue #468 — the config used to connect to and
introspect the server, consumed only to stamp `_meta.json`'s `provenance` field; see
[[../core/spec#provenance module (src/provenance.rs)]]), and an optional `HashMap<String, ToolCategorization>`
keyed by the tool's **raw** name (`ToolInfo.name`), not its
display-sanitized `typescript_name` — every lookup in
`generate_with_categories` indexes the map by `tool.name.as_str()` before any
sanitization, so a `categorizations` map keyed by a display name would
silently miss. `ToolCategorization.keywords` is a `Vec<String>` (not a
comma-joined `String`); JSDoc rendering joins it via the internal
`render_keywords_for_jsdoc` helper (`keywords.join(", ")`, then
`sanitize_jsdoc`) whenever a display string is needed — supplied by Claude
via `mcp-server`'s `save_categorized_tools`, or absent for the CLI's plain
`generate` command. `generate` delegates to `generate_with_categories` with
an **empty** map — this must produce byte-identical `index.ts` category
behavior to "no categorization at all" (regression-tested: an empty-but-
`Some` map must not synthesize a spurious "uncategorized" `CategoryInfo`
group).

`ToolContext::short_description` (populated for each tool's JSDoc header) is
a plain `String`, not `Option<String>`: `create_tool_context` always falls
back to the tool's own (sanitized) `description` when no categorization
short description is supplied, so the type reflects that a tool file's
header always has a short description to render.

## 4. Output: Generated File Set

For a server with N tools, exactly `N + 5` files:

| File | Content |
|---|---|
| `{typescriptName}.ts` × N | One per tool: JSDoc header, exported async function, `{Name}Params`/`{Name}Result` types, CLI-mode self-execution block (`if (import.meta.url === ...)`) |
| `index.ts` | Re-exports every tool (grouped by category if provided) + `callMCPTool` from the runtime bridge |
| `_runtime/mcp-bridge.ts` | Connection management + JSON-RPC client (see [[#Runtime bridge]]) |
| `package.json` | `{"type":"module","devDependencies":{"@types/node":"^22"}}` |
| `tsconfig.json` | `target: ES2022`, `module`/`moduleResolution: NodeNext`, `strict: true`, `noEmit: true`, `allowImportingTsExtensions: true`, `skipLibCheck: true`, `types: ["node"]` |
| `_meta.json` | `mcp_execution_core::metadata::ServerMetadata` (schema_version, server_id/name/version, per-tool metadata incl. **raw, unsanitized** parameter descriptions, and `provenance`: a `generated_at` timestamp plus a `ConfigFingerprint`/`ToolDigest` pair computed from the same `server_config`/`server_info.tools` this call is generating from — see [[../core/spec#provenance module (src/provenance.rs)]]) |

Each tool's `{Name}Params` is emitted as a `type` alias, not an `interface` —
only a `type` alias gets the implicit `Record<string, unknown>`-compatible
index signature `callMCPTool`'s parameter requires; an `interface` is not
structurally assignable there. Each tool's `{Name}Result` is
`Record<string, unknown> | unknown[] | string` — a union, not a bare
`interface { [key: string]: unknown }` — reflecting that `callMCPTool`
resolves to a parsed JSON object/array, plain text, or (for a non-text
content item) the content object itself; callers must narrow the type
before accessing a field on the result.

`package.json`/`tsconfig.json` are regenerated on every `generate` call —
documented as **read-only, not meant to be extended** (e.g. via
`tsconfig.json`'s `"extends"`, which would silently inherit `noEmit: true`
into a consumer's own build).

## 5. TypeScript Identifier Resolution

- `resolve_typescript_names(tools)` computes a collision-free
  `typescript_name` per tool via `disambiguate_output_filename`, which
  combines two collision checks with different case sensitivity (numeric
  suffix `_2`, `_3`, ... on collision, mirroring `disambiguate_identifier`'s
  scheme):
  - JS/TS reserved words (`delete`, `class`, `new`, ...) are checked
    **case-sensitively** — an exact match against the lowercase reserved
    word — since reserved words are only reserved in their exact lowercase
    form (`Delete`, `New`, `Import` are all legal identifiers). A tool
    literally named `delete` becomes `delete_2`; a tool named `Delete` is
    left as-is.
  - The fixed output filenames (`index`) and every previously-resolved tool
    name are checked **case-insensitively**, since `typescript_name` doubles
    as an output filename and filenames collide regardless of case on a
    case-insensitive filesystem (macOS APFS, Windows NTFS by default) — a
    tool named `Index` collides with the fixed `index.ts` output, and two
    tools named `getUser`/`GetUser` collide with each other.
  Indexed by **position**, not by raw name, since two tools can share
  an identical raw name.
- `json_schema_to_typescript`/`extract_properties` similarly disambiguate
  sibling property names that sanitize to the same identifier (e.g. `a-b`
  and `a.b` both → `a_b`) — the second becomes `a_b_2`.

## 6. Recursion Depth Bound

`common::typescript::MAX_SCHEMA_RECURSION_DEPTH` (`= 128`) bounds how far
`json_schema_to_typescript` and the JSDoc description sanitizer
(`sanitize_schema_jsdoc_descriptions`/`sanitize_schema_jsdoc_value` in
`progressive::generator`) will descend into a nested `object`/`array`
schema before treating the remaining branch as opaque (`unknown`/
`unknown[]`, or leaving a `description` unsanitized) rather than recursing
further. Both functions log a single `tracing::warn!` per call the first
time any branch trips the cap (not once per clipped branch).

This is **defense-in-depth for direct callers of these two `pub` functions
with a hand-built `serde_json::Value`**, not a fix for a reachable
wire-path denial of service: a schema arriving over the wire from an MCP
server's `tools/list` response is deserialized by `serde_json`, which
enforces its own default recursion limit (128) before
`mcp-execution-introspector` ever constructs a `ToolInfo` — that ceiling
caps *reachable* nesting at 122 levels for an array-shaped schema and ~61
levels for `properties`-nested objects, well under this constant's value,
so no schema that actually clears introspection is ever clipped here. `128`
is chosen to sit at that wire-path ceiling and is coupled to `serde_json`'s
own default staying at 128; a future change to either needs re-verifying
this property.

This cap does **not** cover the rest of the generation pipeline's other
unconditionally-recursive touches on the same schema — e.g.
`create_tool_context`'s `tool.input_schema.clone()`, the schema's later
re-serialization and Handlebars rendering, or `serde_json::Value`'s own
recursive `Drop` impl.

## 7. Injection Defense (Sanitization Pipeline)

All of the following run **before** any Handlebars render, and are the
actual injection-safety mechanism given `no_escape` is set:

| Function | Neutralizes | Applied to |
|---|---|---|
| `sanitize_jsdoc(s, max_len)` | Every control character (C0, DEL, C1 — everything `char::is_control` reports — plus U+2028/U+2029, which ECMAScript treats as line terminators) is *replaced with a space*; the Unicode bidi embedding/override controls (U+202A-U+202E), isolate controls (U+2066-U+2069), and U+200B (ZERO WIDTH SPACE, issue #425 — a genuine break opportunity, so spaced rather than removed) are likewise replaced with a space; the weaker bidi directional marks (U+200E/U+200F/U+061C, issue #422), the Unicode Tags block (U+E0000-U+E007F), U+FEFF, and the U+2060-U+2064 invisible-operator run (issue #425) are removed entirely; U+200C/U+200D are deliberately left untouched (orthographically load-bearing) — all by delegating to `mcp_execution_core::untrusted::sanitize_untrusted_text`; **then** `*/` (JSDoc comment terminator) is escaped to `*\/`; truncation to `max_len` chars runs last | tool/server descriptions, categories, keywords rendered into `.ts` JSDoc comments |
| `sanitize_ts_string_literal(s)` | the **raw** input is truncated to `MAX_UNTRUSTED_FIELD_LEN` (500 chars) *first*; **then** backslash, single-quote, `\r`/`\n`, U+2028/U+2029 are escaped, in that order; **then** the escaped result is passed through `mcp_execution_core::untrusted::sanitize_untrusted_text` with no further length cap (the input is already bounded) to neutralize any remaining invisible-payload character — the Unicode Tags block, bidi embedding/override/isolate controls, bidi directional marks, and zero-width/invisible operators, plus variation-selector runs/totals over `sanitize_untrusted_text`'s own thresholds (issue #432). This also flattens any other control character (tabs, C0/C1) the escape step above doesn't specifically handle to a space, the same way `sanitize_jsdoc` does | tool name / server id embedded as single-quoted TS string literals (`callMCPTool('{{{server_id_literal}}}', ...)`) |
| `sanitize_schema_jsdoc_descriptions(value)` | recursively applies `sanitize_jsdoc` to every `"description"` key in the input JSON Schema before it's embedded in a tool's JSDoc, up to [[#Recursion Depth Bound]] | `input_schema` field of `ToolContext` |

> [!warning]
> `sanitize_jsdoc`'s ordering is load-bearing: control-character
> neutralization must run **before** the `*/` escape step. A control
> character sitting between `*` and `/` would otherwise prevent the `*/`
> match during escaping, and neutralizing/removing it afterward could
> collapse the two characters back together into a live, comment-closing
> `*/` — reopening the JSDoc block comment (issue #300). Replacing with a
> space (not deleting) also avoids gluing adjacent words together.

> [!warning]
> `sanitize_ts_string_literal`'s ordering is also load-bearing, the mirror
> image of `sanitize_jsdoc`'s: TS escaping (`\r`/`\n` -> `\\r`/`\\n`, etc.)
> must run **before** the `sanitize_untrusted_text` neutralization pass,
> not after. `sanitize_untrusted_text` maps raw `\r`/`\n` to a space, which
> would defeat this function's own newline escaping if it ran first on the
> unescaped input; running it second, on the already-escaped result (which
> no longer contains a raw `\r`/`\n`), only touches characters this
> function doesn't already handle (issue #432).
>
> Defense-in-depth: `ToolName::new`/`ServerId::new` (`specs/core/spec.md`,
> issue #444) independently reject the same invisible-payload character
> classes, and any variation selector, at construction, via the UTS #39
> `Identifier_Status=Allowed` allowlist gate — so a hostile
> tool name or server id from introspection never reaches this function in
> the first place — but `sanitize_ts_string_literal` also runs this
> neutralization itself as a length-bound/defense-in-depth layer, since it
> is character-class-agnostic and applies uniformly regardless of whether
> the value went through either constructor's gate. Its
> `MAX_UNTRUSTED_FIELD_LEN` cap is likewise this call site's own defense,
> not a delegation elsewhere: neither `ToolName` nor `ServerId` enforces a
> length bound (`validate_path_segment` accepts any non-empty single path
> component, however long — `ToolName::new` on a 100,000-character string
> succeeds), and `mcp-introspector`'s `MAX_TOOL_NAME_LEN` only gates names
> that actually arrive via `tools/list`, not a hand-built `ToolName`
> (critic finding S3).
>
> The `MAX_UNTRUSTED_FIELD_LEN` cap is applied to the **raw** input, before
> escaping runs — not to the escaped output. An earlier version of this fix
> capped the escaped output instead (passing `MAX_UNTRUSTED_FIELD_LEN` as
> `sanitize_untrusted_text`'s own `max_len`), which was itself a regression
> (critic finding C3): escaping can expand one input `char` into a
> multi-character output sequence (a lone `\` becomes `\\`), so truncating
> post-escape could cut a sequence in half and leave a dangling, odd-length
> run of trailing backslashes — which then escapes the generated template's
> own closing quote and leaves the `callMCPTool('...')` string literal
> unterminated. Truncating the raw input first guarantees every
> multi-character escape sequence in the final output is either wholly
> present or wholly absent, never split.

> [!warning]
> The `_meta.json` sidecar deliberately uses the **raw, unsanitized** MCP
> values (name/description/parameter descriptions) — it's a data contract
> consumed by Rust code, not a JS comment, so JSDoc-safety sanitization
> would only lose fidelity (regression-tested against issue #141, where the
> old regex-based parser could not recover parameter descriptions at all).

## 8. Runtime Bridge (`_runtime/mcp-bridge.ts`)

Generated once per server (identical content regardless of tool count,
except for the forbidden-char/env-name/DoS-bound/charset-pattern fields
rendered from `BridgeContext`).
Responsibilities:
- Loads `~/.claude/mcp.json` at **call time** (not generation time) and
  **re-validates** the resolved server config on every connection —
  mirroring `mcp_execution_core::validate_server_config` (element
  counts/lengths, command metacharacters, env-name charset, forbidden env
  names, URL scheme, header safety) to the same depth and ordering (DoS
  size/count bounds before injection-specific checks), since the config
  file can be hand-edited after generation to add e.g. `LD_PRELOAD` or an
  oversized argument list. Two Rust-side checks are intentionally *not*
  mirrored: absolute-path executability (would require a blocking
  filesystem call; a bad path surfaces as a `spawn` failure instead) and
  `connect_timeout`/`discover_timeout` bounds (not part of the JSON-RPC
  protocol this bridge speaks).
  - `validateEnvName` first checks the name against
    `ENV_NAME_CHARSET_REGEX` (`[A-Za-z_][A-Za-z0-9_]*`, #467) before the
    forbidden-name/prefix comparison — mirroring `validate_env_name`'s own
    check order (#438) and closing the Unicode-case-confusable gap
    directly, rather than relying only on the incidental case-insensitive
    match below. The forbidden-env-name/prefix check itself is
    case-insensitive (`validateEnvName` upper-cases `name` before
    comparing against the already-upper-cased
    `FORBIDDEN_ENV_NAMES`/`FORBIDDEN_ENV_PREFIX`), mirroring
    `validate_env_name`'s `eq_ignore_ascii_case` comparison so a
    case-varied spelling like `Path`/`path` cannot bypass either layer.
  - `validateStdioSizeBounds`/`validateNetworkSizeBounds` bound element
    counts/lengths (`MAX_ARG_COUNT`, `MAX_ARG_LEN`, `MAX_ENV_COUNT`,
    `MAX_ENV_VALUE_LEN`, `MAX_URL_LEN`, `MAX_HEADER_COUNT`,
    `MAX_HEADER_VALUE_LEN`, #471) before the injection-specific checks run,
    using `Buffer.byteLength(v, 'utf8')` (not `.length`, which
    under-counts non-ASCII bytes) to match Rust's `str::len()` semantics.
    A non-string `env`/header value is rejected outright rather than
    exempted from the length check, since `spawn`'s `env` option would
    otherwise silently coerce it via `String()`, bypassing the ceiling
    entirely. A non-object `env`/`headers` itself (a string, array, or
    other primitive) is rejected the same way, not treated as zero
    entries (`recordEntries`, #471/#467 critique S5): `Object.entries`
    on a string/array would otherwise let each character/element bypass
    `MAX_ENV_COUNT`/`MAX_ENV_VALUE_LEN`/`validateEnvName` while still
    being spread into `spawn`'s `env` option per character/element.
- `FORBIDDEN_CHARS`/`FORBIDDEN_ENV_NAMES`/`FORBIDDEN_ENV_PREFIX`/
  `ENV_NAME_CHARSET_REGEX`/`ENV_NAME_CHARSET_DESC` and the seven `MAX_*`
  constants are rendered **directly from the Rust constants/accessors** at
  generation time (via `BridgeContext`), not hand-copied, so the TS copy
  cannot silently drift from `mcp-core`'s source of truth. The two
  string-valued renders that participate in matching (`FORBIDDEN_CHARS`'
  entries, `ENV_NAME_CHARSET_REGEX`'s pattern) — plus `ENV_NAME_CHARSET_DESC`
  for consistency, though it's message text only — go through
  `sanitize_ts_string_literal` first, so a future Rust-side value
  containing `'`/`\` cannot break or silently change what the generated
  `new RegExp('...')` matches.
- Connection caching per `serverId` (`serverConnections: Map<string,
  Promise<ServerConnection>>`), caching the **in-flight promise** so
  concurrent cold-start callers share one spawn rather than racing.
- JSON-RPC request/response demultiplexing by numeric id over a single
  stdio child's stdout, with a configurable per-request timeout
  (`MCPBRIDGE_REQUEST_TIMEOUT_MS`, default 30s).
- Only **stdio** transport is actually executable by this bridge today —
  an http/sse `mcp.json` entry is validated to the same depth as stdio
  (so a bad URL/header is reported precisely) but then rejected as
  "unsupported transport" (forward-compatibility groundwork noted in the
  source for a future http/sse bridge).
- Result extraction handles: JSON-shaped text content (parsed), plain text,
  `structuredContent`-only responses (MCP spec 2025-06-18+), and a
  documented gap — a genuinely empty (`content: []`, no
  `structuredContent`) success response is currently indistinguishable from
  a misbehaving server and is surfaced as a thrown error.

## 9. Resource-Exhaustion Bounds (CWE-400)

- `enforce_tool_count_bound` rejects before any per-tool rendering if
  `tools.len() + 5 > MAX_GENERATED_FILES`.
- `add_tracked` checks both the running byte total (`MAX_GENERATED_BYTES`)
  and file count (`MAX_GENERATED_FILES`) **incrementally, as each file is
  produced** — not only after the whole `GeneratedCode` is assembled — so
  an oversized `ServerInfo` (or a `_meta.json` sidecar re-embedding every
  tool's schema, pushing the total over the edge) is rejected as soon as
  the offending file is generated, never holding the full amplified output
  in memory first.
- Both bounds are **derived from**, not independently chosen relative to,
  `mcp-introspector`'s own `MAX_TOOL_COUNT`/`MAX_TOOL_NAME_LEN`/etc. — a
  `ServerInfo` that already cleared introspection's bounds can never be
  deterministically rejected here for simply being "as large as
  introspection already allows."

## 10. Error Conditions

| Condition | `Error` variant |
|---|---|
| Tool count would exceed `MAX_GENERATED_FILES` | `ResourceLimitExceeded { resource: ResourceKind::ToolCount { server_id }, .. }` |
| Running byte total exceeds `MAX_GENERATED_BYTES` | `ResourceLimitExceeded { resource: ResourceKind::GeneratedOutputSize, .. }` |
| Malformed property schema (`name`/`type` not a string) | `ValidationError` |
| Handlebars render failure | `SerializationError` (message embeds Handlebars' own error text) |
| `_meta.json` serialization failure | `SerializationError` |
| `GeneratedCode::add_file` called with a `path` already present in the collection | `DuplicateGeneratedFilePath { path }` — the original entry is left untouched, not overwritten; defense-in-depth once `resolve_typescript_names` already seeds its own collision set with this generator's reserved output filenames, not a path this generator expects to hit in practice (issue #312) |
| Any per-tool failure above | Re-wrapped as `ScriptGenerationError { tool, message, source: Some(...) }` via `wrap_tool_generation_error`, preserving the original error's own classification (e.g. a wrapped `ResourceLimitExceeded` still reports as such downstream — see [[../cli/spec#classify_core_error]]) |

## 11. Cross-Crate Contracts

- **Consumes**: `mcp-core::metadata`/error types/forbidden-char constants;
  `mcp-introspector::{ServerInfo, ToolInfo}` and its `MAX_TOOL_COUNT`/
  `MAX_TOOL_NAME_LEN`/`MAX_TOOL_DESCRIPTION_LEN`/`MAX_SCHEMA_SIZE_BYTES`.
- **Produced for** `mcp-files`: `GeneratedCode`/`GeneratedFile` are the
  direct input to `FilesBuilder::from_generated_code` — see
  [[../files/spec#Input contract]]. `mcp-files::MAX_EXPORT_FILES`/
  `MAX_EXPORT_BYTES` are set **equal to** this crate's
  `MAX_GENERATED_FILES`/`MAX_GENERATED_BYTES`, not independently chosen.
- **Produced for** `mcp-skill`/`mcp-server`: the `_meta.json` sidecar
  (schema owned by `mcp-core::metadata`).

## 12. Edge Cases & Notable Behaviors

- A tool name containing only invalid identifier characters (e.g. all
  non-ASCII) sanitizes to a bare `_`, then gets disambiguated if colliding.
- A schema property whose sanitized name collides with a JS/TS reserved
  word is *not* itself special-cased (only the top-level tool function name
  is reserved-word-aware); collision handling there is purely
  sibling-vs-sibling.
- `disambiguate_identifier`'s `used` set is scoped per schema object level —
  two independent nested objects can each reuse the same disambiguated name
  without conflict (verified by
  `test_disambiguate_identifier_reuses_base_across_independent_scopes`).
- `sanitize_ts_identifier` collapses an entire run of consecutive
  invalid/underscore-producing characters into a single `_`, not one `_`
  per character — a name like `café_menu_日本語` sanitizes to `caf_menu_`,
  and regenerating a server whose tools have such names can produce
  different identifiers than an older generated package did.
- `ProgressiveGenerator::create_tool_context`/`create_tool_metadata` both
  consume a single `extract_property_data` call per tool rather than
  extracting twice — the JSDoc-sanitized `PropertyInfo` half feeds the
  `.ts` template, the raw-description half feeds `_meta.json`, sharing one
  schema walk (issue #295).
- Two `generate`/`generate_with_categories` calls against identical `ServerInfo`/`ServerConfig`
  input agree on `_meta.json`'s `provenance.config_fingerprint` and `provenance.tool_digest` —
  only `provenance.generated_at` differs between runs (issue #468). `_meta.json`'s digest is
  computed from `server_info.tools`, not from the generated `.ts` files' content, so it does
  **not** detect drift introduced purely by `resolve_typescript_names`' index-based collision
  suffixes or by a changed `categorizations` map — see
  [[../core/spec#What provenance does and does not answer]].

## 13. Planned Changes (script-first direction, adopted 2026-11-24)

> [!warning] Not implemented
> Nothing in this section exists in the code yet. Each subsection names its tracking issue.
> When a change lands, its content moves into the as-built sections above and the subsection is
> removed in the same pull request.

Requirement IDs are the IDs of the project requirements document plus a suffix (`FR-032.4` is the
fourth requirement of FR-032). `run`, `call` and "catalog" are working names. Several packages
below change generated bytes for every server (lossless types, slimmer files, quiet output, a
catalog), so byte-identical output for tools without an output schema is not a requirement of any
of them. Each lists its change under "Breaking" in `CHANGELOG.md`; there is no compatibility shim
before 1.0.

> [!info] Fixed-file rule (applies to #566, #567, #571 and #575)
> Today every generation emits `FIXED_FILE_COUNT` = 5 fixed files: `index.ts`, the runtime bridge,
> `package.json`, `tsconfig.json` and `_meta.json`
> (`crates/mcp-codegen/src/progressive/generator.rs:56`). `MAX_GENERATED_FILES` is the introspection
> tool-count bound plus that constant (`generator.rs:77-78`), and `mcp-files` sets its export bound
> equal to it ([[../files/spec]], planned section on the export bounds). A change that adds or
> removes a fixed generated file updates, in the same pull request: `FIXED_FILE_COUNT`; the equal
> bounds in `mcp-files`; and the "tool count + 5" and `N + 5` wording in
> [[#2. Public API Surface]], [[#4. Output: Generated File Set]] and
> [[#9. Resource-Exhaustion Bounds (CWE-400)]]. The final value is the sum of what has merged, not
> a number chosen in advance. The subsections below refer to this rule and do not restate it.

### Planned: typed results (#482)

**Why.** Every tool's result type is the fixed line `export type {{typescript_name}}Result =
Record<string, unknown> | unknown[] | string;` (`templates/progressive/tool.ts.hbs:79`). The
introspector admits an output schema per tool, bounded at `MAX_SCHEMA_SIZE_BYTES`
(`crates/mcp-introspector/src/lib.rs:1266-1287`), and stores it as
`ToolInfo.output_schema: Option<serde_json::Value>` (`lib.rs:245`), but the generator reads it only
for the provenance digest (`generator.rs:873`). The runtime returns `structuredContent` only when
`content` is empty (`templates/progressive/runtime-bridge.ts.hbs:1142-1144`) and otherwise parses
`content[0].text` (`:1172-1183`), so a declared type can describe a value a script never
receives. An empty success throws (`:1152-1154`).

**Requirements**

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-030.1 | WHEN a tool declares an output schema that the shared converter (the lossless-types subsection, #565) can represent THE SYSTEM SHALL emit `{Name}Result` derived from it, whatever the schema root (object, array, primitive, type array, composition) | must |
| FR-030.2 | WHEN a tool declares no output schema THE SYSTEM SHALL keep the result declaration as the generic union (`Record<string, unknown> \| unknown[] \| string`, plus a `void` member if the open decision below goes that way) | must |
| FR-030.3 | WHEN the converter cannot represent the root of an output schema THE SYSTEM SHALL emit the generic union for that tool, record a diagnostic, and NOT fail generation | must |
| FR-030.4 | WHEN a tool that declares an output schema returns non-null `structuredContent` THE SYSTEM SHALL make the generated runtime return it, whether or not `content` is also populated | must |
| FR-030.5 | WHEN a tool declares an output schema and the server returns no `structuredContent` THE SYSTEM SHALL return the value as today and SHALL NOT validate it against the schema or claim a validated shape | must |
| FR-030.6 | WHEN a call succeeds with empty `content` and no `structuredContent` THE SYSTEM SHALL treat it as a successful empty result and SHALL NOT throw | must |
| FR-030.7 | THE SYSTEM SHALL use the one converter of FR-031 for output schemas and SHALL NOT keep an output-only conversion path | must |
| FR-030.8 | THE SYSTEM SHALL derive the generated-bytes bound so that a `ServerInfo` that passed introspection, with an output schema per tool, is never rejected at generation (see Constraints) | must |
| FR-030.9 | WHEN generation degrades an output schema to the generic union THE SYSTEM SHOULD surface the diagnostic and NOT discard it silently | should |

**Constraints**
- Server-supplied strings in an output schema (property names, enum and const literals,
  descriptions) reach generated text only through the sanitizers of
  [[#7. Injection Defense (Sanitization Pipeline)]]
  ([[../constitution#V. Security (the dominant concern of this codebase)]]).
- Recursion and size bounds are those of FR-031; this package adds none of its own.
- Generated-bytes bound, re-derived. `MAX_GENERATED_BYTES` is `2 * MAX_TOOL_COUNT * (name +
  description + one schema)` (`generator.rs:98-102`) and so counts one schema per tool, while
  introspection admits an input schema and an output schema per tool. The bound is re-derived from
  introspection's per-tool bounds, counting both schemas and every rendering of them (tool file,
  `_meta.json`, and the catalog if it is a separate file), either proving the worst case fits or
  raising the factor. `mcp-files::MAX_EXPORT_BYTES` stays equal to the result, and the check stays
  incremental ([[#9. Resource-Exhaustion Bounds (CWE-400)]]). The value is open.
- Type safety: the converter returns a typed outcome (type text plus diagnostics), not a bare string
  that callers must interpret.
- No runtime validation of results against schemas, and no fetching of references.

**Open decisions**
- [NEEDS CLARIFICATION: Does the `structuredContent` preference apply to every tool or only to tools that declare an output schema? The bridge is generated once per server, so per-tool behaviour needs schema presence passed to the call.]
- [NEEDS CLARIFICATION: Which tools get the `void` member: all tools or only tools without an output schema? FR-030.2 holds either way.]
- [NEEDS CLARIFICATION: Printed form of an empty success (nothing, an explicit marker line, or a structured line). This subsection owns the decision; the quiet single-call subsection (#570) and the one-shot call in [[../runtime/spec]] refer to it.]
- [NEEDS CLARIFICATION: Inline type literal or separately emitted named type for `{Name}Result`; treatment of an empty-object output schema; where the diagnostic of FR-030.9 surfaces (debug log, `generate` summary, measurement report).]
- [NEEDS CLARIFICATION: Stored form of the result type in `_meta.json` v3 (rendered text or structured). Owned by the registry in [[../core/spec]].]
- [NEEDS CLARIFICATION: The re-derived value of the generated-bytes bound.]

Ask first: applying the `structuredContent` preference to tools without an output schema (it
changes what existing scripts receive); adding a dependency; raising the derived factor of the
bytes bound.

**Depends on / blocks.** Depends on #565 (converter) and #568 (stored form, registry). Blocks #566
(the catalog prints result types). Shares the generated-bytes change with #566 and #567. The
value semantics are carried over to the per-script helper by #575.

### Planned: lossless schema-to-TypeScript types (#565)

**Why.** The converter understands only `type` values of object, array and the primitives.
`json_type_to_typescript` maps a type name to a fixed type
(`crates/mcp-codegen/src/common/typescript.rs:209-219`) and `json_schema_to_typescript_at_depth`
reads `type`, `properties`, `required` and `items` and nothing else (`typescript.rs:268-330`). An
`enum` renders as its base type, a `type` array falls to `unknown`, and `oneOf`, `anyOf`, `allOf`,
`$ref`, `additionalProperties`, `format` and `default` are ignored. The per-tool parameters type is
built through `extract_properties` (`typescript.rs:364`) and `extract_property_data`
(`generator.rs:747`). A script author, usually a model, reads these signatures to decide which
arguments to pass.

**Requirements.** One converter serves input schemas, output schemas and catalog signatures. This
subsection owns the converter; the typed-results subsection (#482) owns where it is applied to
output schemas.

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-031.1 | WHEN a schema has `enum` or `const` THE SYSTEM SHALL render a union of the literal members, or the single literal, in declared order without duplicates | must |
| FR-031.2 | WHEN a schema has `oneOf` or `anyOf` THE SYSTEM SHALL render the union of its members; WHEN it has `allOf` THE SYSTEM SHALL render an intersection up to a bounded member count and `unknown` with a diagnostic beyond it | must |
| FR-031.3 | WHEN `type` is an array THE SYSTEM SHALL render the union of the listed types, with `null` rendered as `null`, and SHALL keep "may be absent" (optional member) distinct from "may be null" | must |
| FR-031.4 | WHEN a schema has a `$ref` that is a local JSON Pointer into the same document THE SYSTEM SHALL resolve it with cycle detection and an expansion bound | must |
| FR-031.5 | WHEN a `$ref` is not local, cannot be resolved, or is cyclic beyond the bound THE SYSTEM SHALL render `unknown` for that node, record a diagnostic, and fetch nothing | must |
| FR-031.6 | WHEN a construct is outside the supported subset THE SYSTEM SHALL render `unknown` for the smallest enclosing node and record a diagnostic instead of failing generation | must |
| FR-031.7 | WHEN the converter finishes THE SYSTEM SHALL return the rendered type together with a structured list of diagnostics (a bounded, sanitized node path and a reason kind), not only a log line | must |
| FR-031.8 | WHEN a schema carries `format`, `default`, or numeric, length or item bounds THE SYSTEM SHOULD emit them as JSDoc tags on the member, sanitized and length-capped | should |
| FR-031.9 | WHEN an object schema has `additionalProperties` THE SYSTEM SHOULD render an index signature of `unknown` for absent or `true`, none for `false`, and the converted type for a schema | should |
| FR-031.10 | WHEN the same schema is converted twice THE SYSTEM SHALL produce byte-identical output (declared property and union order, no hash-order dependence) | must |
| FR-031.11 | THE SYSTEM SHALL use this one converter for per-tool parameter types, per-tool result types and catalog signatures | must |

**Constraints**
- Server-supplied enum and const string values are untrusted string literals: each goes through
  `sanitize_ts_string_literal` ([[#7. Injection Defense (Sanitization Pipeline)]]). That function
  truncates its raw input at `MAX_UNTRUSTED_FIELD_LEN` (500,
  `crates/mcp-core/src/untrusted.rs:35`), and a truncated literal is a different value, so a
  member that would be truncated MUST NOT be rendered as if it were the declared value (mechanism
  open). Descriptions, `format`, `default` and `pattern` text go through `sanitize_jsdoc` with a
  length cap; property names keep the identifier rules of
  [[#5. TypeScript Identifier Resolution]].
- Resource bounds (CWE-400): recursion stays under `MAX_SCHEMA_RECURSION_DEPTH`
  (`typescript.rs:75`, see [[#6. Recursion Depth Bound]]); union members, intersection members and
  reference expansions are bounded separately; the total rendered size per schema is bounded by a
  limit derived from `MAX_SCHEMA_SIZE_BYTES`, so a small schema whose references multiply cannot
  amplify past the generated-bytes budget. Hitting a bound yields `unknown` plus a diagnostic,
  never an error or a hang.
- The converter lives in one module with a table-driven test matrix, and every rendered type is
  type-checked in tests. It is not inlined in a template.
- Type safety: the converter's public result is a typed struct, not a `String`. Changing its public
  function is ask-first because the typed-results and catalog packages share it.

**Open decisions**
- [NEEDS CLARIFICATION: Concrete bounds: union members, intersection members, reference expansions, rendered size, and whether the depth cap stays at 128 for the richer node kinds.]
- [NEEDS CLARIFICATION: An `enum` above the member bound renders the base primitive type or `unknown`.]
- [NEEDS CLARIFICATION: Recursive `$defs`: expand to the bound and cut with `unknown`, or emit named aliases (which changes file layout and the name rules of [[#5. TypeScript Identifier Resolution]]).]
- [NEEDS CLARIFICATION: `$ref` with sibling keywords (merge or ignore with a diagnostic), `nullable: true` of the OpenAPI dialect, and other `$schema` dialects.]
- [NEEDS CLARIFICATION: Where diagnostics surface for a user (debug log, `generate` summary, measurement report), and whether the JSDoc tags of FR-031.8 also appear in the catalog, where they cost bytes.]

Ask first: adding a dependency (a schema-to-TypeScript crate or a reference resolver); changing the
converter's public function; changing how property names are rendered.

Related defect, not a requirement here: parameter keys that are not valid identifiers are renamed in
the type while the object is sent to the server unchanged (`generator.rs:788`,
`sanitize_ts_identifier`); tracked as #584.

**Depends on / blocks.** No dependencies. Blocks #482 and #566. `_meta.json` records the generated
types in the form the registry in [[../core/spec]] decides (#568).

### Planned: typed tool catalog (#566)

**Why.** To learn what a generated server offers, an agent opens tool files. The generated skill
teaches `ls`, `cat <tool>.ts` and `grep -l` (`crates/mcp-skill/src/templates/skill-md.hbs:18-35`).
The closest overview today is `index.ts` (`templates/progressive/index.ts.hbs`): a JSDoc list of
tool names, short descriptions and keywords followed by re-exports, with no parameter or result
types, so the model still opens every file it wants to call.

**Requirements**

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-032.1 | WHEN `generate` runs THE SYSTEM SHALL emit exactly one catalog per server with one entry per tool | must |
| FR-032.2 | WHEN an entry is rendered THE SYSTEM SHALL include the tool's TypeScript name, its parameters type (FR-031), its result type (FR-030), a one-line description, and annotation markers when annotations are known (FR-034) | must |
| FR-032.3 | THE SYSTEM SHALL write the catalog as valid TypeScript declarations that type-check without importing runtime code | must |
| FR-032.4 | THE SYSTEM SHALL order entries by TypeScript name and produce byte-identical output for identical input | must |
| FR-032.5 | WHEN a description is longer than one line or contains control or invisible characters THE SYSTEM SHALL reduce it to one sanitized, length-capped line with the existing sanitizers | must |
| FR-032.6 | THE SYSTEM SHOULD offer three detail levels (names only, the catalog, the full tool file) and SHOULD make the names-only level derivable without reading any tool file | should |
| FR-032.7 | WHEN the entries exceed the catalog byte bound THE SYSTEM SHALL still emit a valid catalog, degrading entry detail, and SHALL NOT drop tools silently | must |
| FR-032.8 | THE SYSTEM SHALL count the catalog in the generated-file and generated-byte bounds and reject an over-budget run through the incremental check, not after assembly | must |
| FR-032.9 | WHEN a tool's name would collide with the catalog's file or export name THE SYSTEM SHALL disambiguate the tool, not the catalog | must |
| FR-032.10 | THE SYSTEM SHALL ensure the skill scan and the server's directory listing neither count the catalog as a tool nor warn that it is unreferenced | must |
| FR-032.11 | THE SYSTEM SHALL render the catalog and each tool file from the same converted types in one pass | must |

**Constraints**
- Catalog byte bound. A named constant (working name: the catalog byte bound), derived from the
  generated-bytes budget (`generator.rs:98-102`) so that the catalog's worst case, across the
  largest tool count introspection admits, fits inside the re-derived budget of the typed-results
  subsection (#482). Its value is open. The skill's optional load-time catalog injection is bounded
  by this same constant ([[../skill/spec]]).
- Fixed-file rule (above): if the catalog is a new file, `FIXED_FILE_COUNT` grows by one in this
  pull request, `RESERVED_OUTPUT_NAMES` (`generator.rs:1123`, today `["index"]`) gains its name so
  the collision of issue #312 does not return, and the scanners are exempted. If the catalog
  extends `index.ts`, no file bound changes.
- Scanners. `verify_tool_files_on_disk` (`crates/mcp-skill/src/parser.rs:393`) and the tool count
  of the server's directory listing (`crates/mcp-server/src/service.rs:845-856`, which excludes
  only `index.ts` and names starting with `_`) would report a new top-level `.ts` file.
- Untrusted text: names, descriptions, enum literals (through the converter) and annotation titles
  are sanitized as in [[#7. Injection Defense (Sanitization Pipeline)]]; descriptions are inert text
  and annotation markers are fixed tokens, never server text.
- Targets (provisional, measured with the harness of #562 on in-repo fixtures; open): a median
  entry of about 250 bytes without parameter docs, and a whole catalog of about a tenth of the
  tool files' bytes for the reference server.

**Open decisions**
- [NEEDS CLARIFICATION: New file or extension of `index.ts`. A new file raises `FIXED_FILE_COUNT` and needs scanner exemptions; extending `index.ts` keeps both unchanged but mixes declarations with the re-exports scripts import.]
- [NEEDS CLARIFICATION: File name, location (top level or under `_runtime/`, whose names the listing already ignores) and extension (`.ts` or `.d.ts`), if a new file.]
- [NEEDS CLARIFICATION: Entry format (`declare function` signatures, an interface with a member per tool, other), the written form of result type and markers, and whether parameter docs are included.]
- [NEEDS CLARIFICATION: Behaviour above the byte bound (truncate descriptions, drop parameter detail, or split); the reference fixtures and their byte targets; whether the catalog is recorded in or checked against `_meta.json`.]

Ask first: choosing new file versus extension; raising the derived factor of the bytes bound;
exempting a top-level file from the scanners; adding a dependency.

**Depends on / blocks.** Depends on #565 and #482; soft dependency on #568 (markers). Blocks #578
(skill injection) and the catalog byte targets of #562. Shares the fixed-file rule with #567 and
#571.

### Planned: slim tool files with a shared command-line module (#567)

**Why.** More than half of a tool file is text identical across tools
(`templates/progressive/tool.ts.hbs`, 105 lines): the header JSDoc (lines 2-14) is repeated almost
verbatim in the function JSDoc (lines 17-38); `{Name}Result` carries a fixed comment block (lines
68-78); and a command-line block is copied into every file (lines 81-105). An agent that opens a
tool file for its signature pays for all of it, and the bytes count against the generation bounds.

**Requirements.** This package is a refactor: the behaviour of single-call mode equals the
command-line block on the base branch at merge time, whichever of this package and the quiet
single-call package (#570) merges first. The stack-trace field of the failure object is not pinned
here.

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-033.1 | THE SYSTEM SHALL generate once per server a shared module holding the single-call behaviour, and tool files SHALL delegate to it instead of embedding the block | must |
| FR-033.2 | THE SYSTEM SHALL emit header information once per tool file, not in both the header and the function JSDoc | must |
| FR-033.3 | THE SYSTEM SHALL NOT emit the fixed explanatory comment before `{Name}Result` | must |
| FR-033.4 | WHEN a tool file is run directly with a JSON argument THE SYSTEM SHALL behave as the command-line block on the base branch does (argument default, stdout payload, stderr object, exit codes) | must |
| FR-033.5 | WHEN a tool file is imported by another module THE SYSTEM SHALL execute no command-line code and no side effect | must |
| FR-033.6 | THE SYSTEM SHALL keep the tool name and keywords greppable in the tool file, because the skill teaches `grep -l "<keyword>"` over tool files | must |
| FR-033.7 | THE SYSTEM SHALL keep the exported names (`{name}`, `{Name}Params`, `{Name}Result`) and the `index.ts` re-exports unchanged by this package | must |
| FR-033.8 | THE SYSTEM SHALL count the shared module in the bounds per the fixed-file rule and keep the skill scan and the server's directory listing in agreement with it present | must |
| FR-033.9 | THE SYSTEM SHALL carry no server-supplied text in the shared module; tool files embed names and server ids only through the existing string-literal sanitizer | must |
| FR-033.10 | THE SYSTEM SHOULD reduce the average tool file by at least half on the reference fixtures | should |

**Constraints**
- Fixed-file rule (above): the shared module is a fixed file.
- The module is static text rendered like the runtime bridge (identical for every server except
  bound-derived fields), with a golden test. Rendered tool files and the module type-check, and a
  test runs a generated file against an in-repo mock server before and after the change.
- Argument parsing introduces no `eval` or dynamic import.

**Open decisions**
- [NEEDS CLARIFICATION: Module name, location (names starting with `_` are ignored by the directory listing, `service.rs:851`; a top-level file needs scanner exemptions) and how a tool file tells the module which file is the entry point.]
- [NEEDS CLARIFICATION: Keep or drop the `#!/usr/bin/env node` first line; which header copy survives; where the fixed `@param`/`@returns`/`@throws` text goes.]
- [NEEDS CLARIFICATION: Reference fixtures, and whether the 50% target is measured after #565 and #482 enlarge the types; whether `_meta.json` or the sidecar check must know the module (a metadata field would need a row in the registry in [[../core/spec]] before the v3 bump lands).]

Ask first: placing the module at the top level; changing anything in the header that grep-based
discovery uses.

The direct-run defect (the check at `tool.ts.hbs:83` compares `import.meta.url` with
`` `file://${process.argv[1]}` ``, which does not match for a path with a space or a symlink, so the
file exits 0 without running) is #583 and is not part of this package.

**Depends on / blocks.** No dependency; either merge order with #570. Shares the fixed-file rule
with #566 and #571. Reshaped or retired by #575 if the generated bridge is retired.

### Planned: annotations in generated output (#568)

Code-generation side only. The fields of `_meta.json` v3 are defined once, in the registry in
[[../core/spec]]; capture is specified in [[../introspector/spec]]. This block defines no metadata
field of its own.

**Why.** `ToolInfo` has four fields and the introspector copies only name, description and the two
schemas (`crates/mcp-introspector/src/lib.rs:237-246`, `:1295-1300`), so a server's annotations
(title and the read-only, destructive, idempotent and open-world hints) never reach generated
output. The tool-file JSDoc carries no hint, and the digest entry is built from name, description
and the two schemas (`generator.rs:866-875`).

**Requirements**

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-034.20 | WHEN a tool's `ToolInfo` carries annotations THE SYSTEM SHOULD render each known hint as a JSDoc tag with a fixed tag name and a fixed value in that tool's file | should |
| FR-034.21 | WHEN a hint is not stated, or the tool has no annotations, THE SYSTEM SHALL render no tag for it and SHALL NOT render a default such as "not read-only" | must |
| FR-034.22 | WHEN annotations carry a title THE SYSTEM SHALL render it only through `sanitize_jsdoc` with a length cap, as the one server-supplied text in the annotation tags | must |
| FR-034.23 | THE SYSTEM SHALL word every rendering as a server-declared hint, never as a verified property | must |
| FR-034.24 | WHEN generation writes `_meta.json` THE SYSTEM SHALL write the annotation, result-type and size fields of the registry from `ToolInfo` and SHALL NOT add a field outside it or a second schema bump | must |
| FR-034.25 | THE SYSTEM SHALL NOT branch on any annotation in generation to relax a check or choose a code path | must |
| FR-034.26 | WHEN stated hints contradict each other (for example read-only with destructive) THE SYSTEM SHOULD render the values as advertised without reinterpreting them | should |

**Constraints**
- Untrusted hints ([[../constitution#V. Security (the dominant concern of this codebase)]]): hint
  values are booleans and carry no text; tag names and values are fixed tokens. A missing
  annotation is never read as safe.
- The annotation payload counts toward the per-tool introspection bounds and the incremental
  generated-bytes check ([[#9. Resource-Exhaustion Bounds (CWE-400)]]).
- The JSDoc tags and catalog markers are a display of the registry, not a second source of truth.

**Open decisions**
- [NEEDS CLARIFICATION: Tag names and layout of the JSDoc tags, and how the catalog abbreviates them (a fixed token set).]
- [NEEDS CLARIFICATION: Whether annotations enter the provenance digest entries built at `generator.rs:866-875`. Decided in the registry in [[../core/spec]] before the v3 bump lands; including them is the recommendation recorded there.]
- [NEEDS CLARIFICATION: Precedence of the SDK's tool-level title and the annotation title. Owned by the introspector and the registry.]

Ask first: adding annotations to the digest; making any generator or consumer read a hint for a
decision.

**Depends on / blocks.** #568 owns the one `_meta.json` bump. This subsection blocks #566 (markers)
and the static report of #562; the dry-run package (#573) consumes the stored hints.

### Planned: quiet single-call output (#570)

**Why.** The only supported single call, `node <tool>.ts '<json>'`, writes straight into a model's
context. The block at `templates/progressive/tool.ts.hbs:81-105` prints the result with
`JSON.stringify(result, null, 2)` (line 93), prints failures as an indented `{ error, stack }` to
stderr (lines 97-101), and parses the argument inside the same handler (line 87), so a malformed
argument fails with a `SyntaxError` whose message embeds an excerpt of the input. Failure text is
also server-supplied: the bridge throws `Tool returned error: ${errorMessage}` with the server's
text, unbounded and unsanitised (`runtime-bridge.ts.hbs:1126-1133`).

**Requirements.** Works on today's generated bridge; not gated by the runtime decision (#569).

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-035.1 | WHEN a single call succeeds THE SYSTEM SHALL print the result as compact JSON on one line to stdout and exit 0 | must |
| FR-035.2 | WHEN a single call fails THE SYSTEM SHALL print exactly one line of structured JSON to stderr with a message and a failure kind, and exit non-zero | must |
| FR-035.3 | WHILE debug mode is off THE SYSTEM SHALL NOT include a stack trace or internal file path in failure output; WHILE it is on THE SYSTEM SHOULD include the stack | must |
| FR-035.4 | THE SYSTEM SHALL take the argument as a single JSON object; WHEN it is not valid JSON or not an object THE SYSTEM SHALL report an invalid-arguments failure that reproduces no part of the argument text | must |
| FR-035.5 | WHEN a failure message originates from an MCP server THE SYSTEM SHALL strip control characters, truncate it to a fixed bound, mark the truncation, and mark the text as coming from the server so it cannot pass as the tool's own wording | must |
| FR-035.6 | WHEN a failure has no usable message THE SYSTEM SHOULD still emit a valid one-line object with a generic message and a failure kind | should |
| FR-035.7 | THE SYSTEM SHALL keep exit 0 for success only and a non-zero exit for every failure | must |
| FR-035.8 | WHEN the command-line block is relocated by #567 THE SYSTEM SHALL preserve FR-035.1 to FR-035.7 | must |

**Constraints**
- Untrusted-text rule ([[../constitution#V. Security (the dominant concern of this codebase)]]).
  Whether successful results printed by single-call mode, the runner and the one-shot call (the
  latter two in [[../runtime/spec]]) fall under the boundary rule or are an accepted exception is
  an open maintainer decision. This subsection claims neither sanitisation nor an exception for
  successful results. Error text is sanitised, bounded and marked as coming from the server
  (FR-035.5).
- The truncation bound is rendered from a Rust constant into the template (the `BridgeContext`
  pattern of [[#2. Public API Surface]]), not hand-copied, and derives from an existing bound where
  one applies (for example `MAX_UNTRUSTED_FIELD_LEN`, `crates/mcp-core/src/untrusted.rs:35`).
- The printed form of an empty success is owned by the typed-results subsection (#482). The output
  budget and spill-to-file belong to the runner and the one-shot call, not to single-call mode.
- No dependency is added to the generated package (Node built-ins only). Output shape is tested by
  running a generated file against an in-repo mock server (success, tool error, invalid arguments,
  oversized error, debug on and off). This is a breaking change to the stdout and stderr shape; no
  flag restores the old form.

**Open decisions**
- [NEEDS CLARIFICATION: The successful-result decision above: apply the boundary rule or record an exception. Made by the maintainer.]
- [NEEDS CLARIFICATION: Debug switch: reuse `MCPBRIDGE_DEBUG` (`runtime-bridge.ts.hbs:143`), which also logs call parameters (`:1090`), or add a narrower stack-only switch.]
- [NEEDS CLARIFICATION: Field names and failure-kind vocabulary (and alignment with the CLI exit categories), how the server-text marking is expressed inside a JSON string, and the numeric truncation bound.]

Ask first: distinct non-zero exit codes in generated files; the exact marker for server text;
adding a dependency.

**Depends on / blocks.** Not gated by #569. Either merge order with #567. Refers to #482 (empty
success). #571 and #574 reuse the failure shape.

### Planned: retirement of the generated bridge (#575)

Code-generation side only. The per-script helper itself is specified in [[../runtime/spec]]. Gated
by the decision record #569: nothing here lands unless it chooses the Rust binary as the single
client; otherwise the bridge stays the only client and this subsection is removed.

**Why.** The generated bridge is the only code that calls a tool. It is stdio-only (a validated
http or sse entry is rejected, `templates/progressive/runtime-bridge.ts.hbs:634-650`), pins the
handshake to protocol `2024-11-05` (`:966`), and mirrors the Rust-side config validation in
TypeScript (section 8, the runtime bridge). Keeping it beside a Rust call path
would make two ways to call a tool.

**Requirements**

| ID | Requirement | Priority |
|----|-------------|----------|
| FR-040.20 | WHERE #569 chooses the Rust single client THE SYSTEM SHALL generate tool wrappers that call tools through the per-script helper and SHALL embed no connection code | must |
| FR-040.21 | WHERE #569 chooses it THE SYSTEM SHALL NOT generate the runtime bridge: its connection code, its mirrored config validation and the `2024-11-05` handshake pin are removed | must |
| FR-040.22 | THE SYSTEM SHALL keep the result semantics of FR-030 and the error rules of FR-035 for wrappers calling through the helper | must |
| FR-040.23 | THE SYSTEM SHALL keep the exported names `{name}`, `{Name}Params` and `{Name}Result` unchanged | must |
| FR-040.24 | WHEN the bridge file is removed THE SYSTEM SHALL lower the fixed-file count in the same pull request per the fixed-file rule | must |
| FR-040.25 | THE SYSTEM SHOULD remove from generation any input that exists only to render the bridge's mirrored validation, or keep it only for values a remaining generated module needs | should |

**Constraints**
- Wrappers must type-check as a package without the bridge file. How they reach the helper is
  specified in [[../runtime/spec]] and adds no server-supplied text to generated source.
- Generation bounds and the sanitizers of [[#7. Injection Defense (Sanitization Pipeline)]] still
  apply to every remaining generated file.

**Open decisions**
- [NEEDS CLARIFICATION: What `callMCPTool`, re-exported from `index.ts` today (last line of `templates/progressive/index.ts.hbs`), becomes: a thin export over the helper, or removed (a breaking change for scripts that import it).]
- [NEEDS CLARIFICATION: Whether `BridgeContext` and its accessors ([[#2. Public API Surface]]) are removed or reduced to the values the shared module of #567 still needs.]

Ask first: removing `callMCPTool` from the package surface; changing the import specifier of any
generated file.

**Depends on / blocks.** Gated by #569; depends on #574 (one-shot call path) and the helper in
[[../runtime/spec]]. Relates to #567 (shared module) and #571.

## 14. See Also

- [[../introspector/spec]] — source of `ServerInfo`/`ToolInfo`
- [[../files/spec]] — consumer of `GeneratedCode`
- [[../server/spec#save_categorized_tools]] — MCP-tool caller of `generate_with_categories`
- [[../cli/spec#generate]] — CLI caller of `generate`
- [[../core/spec]] — registry of `_meta.json` v3 fields (planned section)
- [[../runtime/spec]] — script runner, one-shot call and per-script helper (planned block)
- [[#13. Planned Changes (script-first direction, adopted 2026-11-24)]] — script-first changes, not yet implemented
