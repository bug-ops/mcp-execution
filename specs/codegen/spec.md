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
    pub fn generate(&self, server_info: &ServerInfo) -> Result<GeneratedCode>;
    pub fn generate_with_categories(&self, server_info: &ServerInfo, categorizations: &HashMap<String, ToolCategorization>) -> Result<GeneratedCode>;
}
pub struct ToolCategorization { pub category: String, pub keywords: Vec<String>, pub short_description: String }

// BridgeContext's three fields are private with read-only accessor methods of the same
// name; `BridgeContext::default()` is the only construction path (no `pub` fields, no
// `derive(Default)`, no `Deserialize`) — see [[#Runtime bridge]] and issue #315.
pub struct BridgeContext { /* forbidden_chars, forbidden_env_names, forbidden_env_prefix: private */ }
impl BridgeContext {
    pub fn forbidden_chars(&self) -> &[String];
    pub fn forbidden_env_names(&self) -> &[String];
    pub fn forbidden_env_prefix(&self) -> &str;
}
impl Default for BridgeContext { /* populates from mcp_execution_core::forbidden_chars()/forbidden_env_names()/forbidden_env_prefix() — hand-written, so it can never render a fail-open (empty) bridge */ }

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
[[../introspector/spec]]) and an optional `HashMap<String, ToolCategorization>`
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
| `_meta.json` | `mcp_execution_core::metadata::ServerMetadata` (schema_version, server_id/name/version, per-tool metadata incl. **raw, unsanitized** parameter descriptions) |

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
| `sanitize_ts_string_literal(s)` | backslash, single-quote, `\r`/`\n`, U+2028/U+2029 | tool name / server id embedded as single-quoted TS string literals (`callMCPTool('{{{server_id_literal}}}', ...)`) |
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
> The `_meta.json` sidecar deliberately uses the **raw, unsanitized** MCP
> values (name/description/parameter descriptions) — it's a data contract
> consumed by Rust code, not a JS comment, so JSDoc-safety sanitization
> would only lose fidelity (regression-tested against issue #141, where the
> old regex-based parser could not recover parameter descriptions at all).

## 8. Runtime Bridge (`_runtime/mcp-bridge.ts`)

Generated once per server (identical content regardless of tool count,
except for the forbidden-char/env-name lists rendered from `BridgeContext`).
Responsibilities:
- Loads `~/.claude/mcp.json` at **call time** (not generation time) and
  **re-validates** the resolved server config on every connection —
  mirroring `mcp_execution_core::validate_server_config` (command
  metacharacters, forbidden env names, URL scheme, header safety) to the
  same depth, since the config file can be hand-edited after generation to
  add e.g. `LD_PRELOAD`. The forbidden-env-name/prefix check is
  case-insensitive (`validateEnvName` upper-cases `name` before comparing
  against the already-upper-cased `FORBIDDEN_ENV_NAMES`/`FORBIDDEN_ENV_PREFIX`),
  mirroring `validate_env_name`'s `eq_ignore_ascii_case` comparison so a
  case-varied spelling like `Path`/`path` cannot bypass either layer; note
  that JS `String.prototype.toUpperCase` folds full Unicode case mappings
  (a strict superset of Rust's ASCII-only fold), so the TS side never
  matches *fewer* names than the Rust side — the "mirrors" claim holds in
  the fail-closed direction, even where the two foldings could in principle
  diverge on non-ASCII input.
- `FORBIDDEN_CHARS`/`FORBIDDEN_ENV_NAMES`/`FORBIDDEN_ENV_PREFIX` are
  rendered **directly from the Rust constants** at generation time (via
  `BridgeContext`), not hand-copied, so the TS copy cannot silently drift
  from `mcp-core`'s source of truth.
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

## 13. See Also

- [[../introspector/spec]] — source of `ServerInfo`/`ToolInfo`
- [[../files/spec]] — consumer of `GeneratedCode`
- [[../server/spec#save_categorized_tools]] — MCP-tool caller of `generate_with_categories`
- [[../cli/spec#generate]] — CLI caller of `generate`
