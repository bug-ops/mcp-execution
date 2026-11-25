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
---

# Block: Code Generation & Templating (`mcp-execution-codegen`)

> [!abstract]
> Path: `crates/mcp-codegen`. Renders a `ServerInfo` into a complete,
> self-contained TypeScript package via Handlebars templates: one `.ts` file
> per tool, an `index.ts` re-export, a type-only `catalog.ts`, a runtime
> bridge, a shared single-call module (`_runtime/cli.ts`), `package.json`,
> `tsconfig.json`, and a `_meta.json` metadata sidecar. Depends on
> `mcp-execution-core` and `mcp-execution-introspector`.
>
> The script-first changes #482, #565, #566, #567, #568, #570, #583, #584 and
> #587 (parent #556) are implemented and described as built in
> [[#13. Script-First Capabilities (implemented, #556)]]. Only the retirement
> of the generated bridge (#575) is still planned
> ([[#15. Bridge Retirement Requirements (#575)]]).

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
pub struct ProgressiveGenerator<'a> { /* engine: TemplateEngine<'a>, observed: ObservedShapes */ }
impl<'a> ProgressiveGenerator<'a> {
    pub fn new() -> Result<Self>;
    // Parses a JSON-lines recording of result shapes (#587). Lines that fail the shape rules are
    // skipped and counted in one tracing::warn!; the last valid line wins per (server id, raw
    // tool name). Parsing is internal, so ObservedShapes stays out of the public API. Without
    // this call the output is unchanged. See [[#13.8 Observed result shapes (#587)]].
    pub fn with_observed_shapes(self, jsonl: &str) -> Self;
    pub fn generate(&self, server_info: &ServerInfo, server_config: &ServerConfig) -> Result<GeneratedCode>;
    pub fn generate_with_categories(&self, server_info: &ServerInfo, server_config: &ServerConfig, categorizations: &HashMap<String, ToolCategorization>) -> Result<GeneratedCode>;
}
pub struct ToolCategorization { pub category: String, pub keywords: Vec<String>, pub short_description: String }

// Five of BridgeContext's fields (forbidden_chars, forbidden_env_names, forbidden_env_prefix,
// env_name_charset_pattern, identifier_pattern) are private with read-only accessor methods of the same name —
// each could fail open (e.g. an empty forbidden_chars, or an env_name_charset_pattern of ""
// which `new RegExp('')` matches every string against) if a caller bypassed Default via a
// struct literal, so `pub` fields aren't used for these. The remaining fields — the DoS
// size/count ceilings (max_arg_count and siblings, see [[#9. Resource-Exhaustion Bounds
// (CWE-400)]]), the server-message and recorder bounds, and env_name_charset_desc (message text
// only, not enforcement) — are plain
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
    pub max_server_message_len: usize,          // MAX_UNTRUSTED_FIELD_LEN (code points)
    pub max_variation_selector_run: usize,      // core untrusted::MAX_VARIATION_SELECTOR_RUN
    pub max_total_variation_selectors: usize,   // core untrusted::MAX_TOTAL_VARIATION_SELECTORS
    pub observed_shape_max_depth: usize,        // OBSERVED_SHAPE_MAX_DEPTH = 4
    pub observed_shape_max_keys: usize,         // OBSERVED_SHAPE_MAX_KEYS = 16
    pub observed_shape_max_key_len: usize,      // OBSERVED_SHAPE_MAX_KEY_LEN = 32
    pub observed_shape_max_line_bytes: usize,   // OBSERVED_SHAPE_MAX_LINE_BYTES = 8192
}
impl BridgeContext {
    pub fn forbidden_chars(&self) -> &[String];
    pub fn forbidden_env_names(&self) -> &[String];
    pub fn forbidden_env_prefix(&self) -> &str;
    pub fn env_name_charset_pattern(&self) -> &str;
    pub fn identifier_pattern(&self) -> &str; // core::untrusted::BARE_IDENTIFIER_PATTERN, escaped via sanitize_ts_string_literal
}
impl Default for BridgeContext { /* populates every field from mcp_execution_core: forbidden_chars()/forbidden_env_names()/forbidden_env_prefix()/env_name_charset_pattern()/env_name_charset_desc() and the MAX_* constants — hand-written, so it can never render a fail-open (empty) bridge. forbidden_chars, env_name_charset_pattern, and env_name_charset_desc are each passed through sanitize_ts_string_literal before storing, so a future Rust-side value containing `'`/`\` renders as a valid (and correct) TS string literal rather than breaking or silently changing what it matches. */ }

pub const MAX_GENERATED_FILES: usize; // = introspector::MAX_TOOL_COUNT + FIXED_FILE_COUNT (7)
pub const MAX_GENERATED_BYTES: usize; // = MAX_TOOL_COUNT * PER_TOOL_BYTES_MAX + FIXED_FILES_BYTES_MAX, see [[#9. Resource-Exhaustion Bounds (CWE-400)]]

// mcp_execution_codegen::progressive
pub enum ResultSource { Content, Structured } // serde snake_case; how a tool function asks the bridge for its result

// mcp_execution_codegen::common::typescript
pub const MAX_SCHEMA_RECURSION_DEPTH: usize = 128; // see [[#Recursion Depth Bound]]
pub fn to_camel_case(s: &str) -> String;
pub fn to_pascal_case(s: &str) -> String;
pub fn sanitize_ts_identifier(s: &str) -> String; // collapses invalid-char runs to one '_'; prefixes '_' if leading digit/empty

// mcp_execution_codegen::common::ts_types — the one schema-to-TypeScript converter (#565, #584)
pub fn convert_schema(schema: &serde_json::Value) -> ConvertedType; // never panics/errors/fetches; deterministic
pub struct ConvertedType { /* opaque */ }
impl ConvertedType { pub fn render(&self, style: RenderStyle) -> String; }
pub enum RenderStyle { Declaration, Compact } // multi-line with member JSDoc (tool files) | one line, no JSDoc (catalog, stored texts)
// pub(crate) only: diagnostics(), dropped_diagnostics(), root_degraded(), is_object_like(), members(),
// SchemaDiagnostic, SchemaPath, DiagnosticKind, MemberSummary and the bound constants.

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

`ToolInfo.annotations` (captured by [[../introspector/spec]]) is read by generation only to
render hint tags/markers, to fill `_meta.json` and to feed the provenance digest entries; no code path branches on a hint
([[#13.5 Annotations in generated output (#568)]]).

`ToolContext::short_description` (populated for each tool's JSDoc header) is
a plain `String`, not `Option<String>`: `create_tool_context` always falls
back to the tool's own (sanitized) `description` when no categorization
short description is supplied, so the type reflects that a tool file's
header always has a short description to render.

## 4. Output: Generated File Set

For a server with N tools, exactly `N + 7` files (`FIXED_FILE_COUNT` = 7;
`MAX_GENERATED_FILES` = `MAX_TOOL_COUNT` + 7, and `mcp-files`' export bound equals it, see
[[../files/spec]]):

| File | Content |
|---|---|
| `{typescriptName}.ts` × N | One per tool: a single JSDoc header (`@tool`, `@server`, optional `@category`/`@keywords`/`@description`, annotation tags), the exported async function, `{Name}Params`/`{Name}Result` types, and one trailing line `runIfMain(import.meta.url, {name});`. No inline command-line block, no fixed explanatory comments (#567) |
| `index.ts` | Re-exports every tool (grouped by category if provided) + `callMCPTool` from the runtime bridge. Unchanged by the script-first work |
| `catalog.ts` | Type-only signatures, one per tool, sorted by TypeScript name (see [[#13.4 Typed tool catalog (#566)]]) |
| `_runtime/mcp-bridge.ts` | Connection management + JSON-RPC client (see [[#8. Runtime Bridge (`_runtime/mcp-bridge.ts`)]]) |
| `_runtime/cli.ts` | Shared single-call module exporting `runIfMain` (see [[#13.6 Single-call mode (#567, #570, #583)]]); static text, identical for every server, no server-supplied data |
| `package.json` | `{"type":"module","devDependencies":{"@types/node":"^22"}}` |
| `tsconfig.json` | `target: ES2022`, `module`/`moduleResolution: NodeNext`, `strict: true`, `noEmit: true`, `allowImportingTsExtensions: true`, `skipLibCheck: true`, `types: ["node"]` |
| `_meta.json` | `mcp_execution_core::metadata::ServerMetadata`, schema version 3, written as **compact** JSON (`serde_json::to_string`): per-tool metadata incl. **raw, unsanitized** parameter names and descriptions, annotations, stored result type, result-type origin and manifest size; and `provenance` (a `generated_at` timestamp plus a `ConfigFingerprint`/`ToolDigest` pair computed from the same `server_config`/`server_info.tools` this call is generating from — see [[../core/spec#provenance module (src/provenance.rs)]], [[../core/spec#`metadata` module (`src/metadata.rs`)]]) |

Each tool's `{Name}Params` is emitted as a `type` alias, not an `interface` —
only a `type` alias gets the implicit `Record<string, unknown>`-compatible
index signature `callMCPTool`'s parameter requires; an `interface` is not
structurally assignable there. Params and Result texts come from the one converter
([[#13.1 Schema-to-TypeScript conversion (#565, #584)]]); the Result rules are in
[[#13.2 Typed results (#482)]]. Every `{Name}Result` ends in `| undefined`, because an empty
success is a normal outcome (FR-030.6). The types a tool file declares are the types
`callMCPTool` resolves to only up to what the server sends: nothing validates a result against
its schema.

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
- `RESERVED_OUTPUT_NAMES` is `["index", "catalog", "runIfMain", "callMCPTool"]`: a tool whose
  TypeScript name equals one of them (case-insensitively, as for `index`) is disambiguated, never
  the generated file or export (FR-032.9, #583: a tool function named `runIfMain` or
  `callMCPTool` would shadow the module's own import).
- Property keys are **never renamed or suffixed**: the Params type uses the raw schema key, bare
  when it is an ASCII identifier (`^[A-Za-z_$][A-Za-z0-9_$]*$`), otherwise as a quoted literal
  produced by `ts_string_literal_exact`, and the object is sent to the server with exactly those
  keys (#584). A key that has no exact literal (over 500 chars, or text the sanitizer would
  change) is omitted from the type, the object gains `[key: string]: unknown`, and a
  `KeyNotRepresentable` diagnostic is recorded. The old `a-b`/`a.b` → `a_b`/`a_b_2` scheme no
  longer exists; `_meta.json` `parameters[].name` holds the raw key.

## 6. Recursion Depth Bound

`common::typescript::MAX_SCHEMA_RECURSION_DEPTH` (`= 128`) bounds how far
the converter (`convert_schema`; every descent counts: a property, an item type, a composition
member, an `additionalProperties` schema and a `$ref` hop) and the JSDoc description sanitizer
(`sanitize_schema_jsdoc_descriptions`/`sanitize_schema_jsdoc_value` in
`progressive::generator`) will descend into a nested schema
before treating the remaining branch as opaque (`unknown`, with a `DepthExceeded` diagnostic in
the converter, or leaving a `description` unsanitized) rather than recursing
further. The sanitizer logs a single `tracing::warn!` per call the first
time any branch trips the cap (not once per clipped branch); the converter records one
diagnostic per clipped node, bounded by `MAX_DIAGNOSTICS`.

This is **defense-in-depth for direct callers of these `pub` functions
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
| `ts_string_literal_exact(s)` (`pub(crate)`) | returns the body of a single-quoted literal that evaluates back to exactly `s`, or `None`: backslash, quote, `\n`, `\r`, `\t`, other control characters and U+2028/U+2029 are escaped; `None` above `MAX_UNTRUSTED_FIELD_LEN` (500) chars, or when `sanitize_untrusted_text` would change the escaped text (bidi controls, Tags block, zero-width operators, variation-selector runs). A literal that would be truncated or altered is a different value, so it is never rendered as if it were the declared one | enum/const string members and quoted property keys rendered by the converter |
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

> [!warning] Superseded by section 14
> Wave 2 (#575, 2026-11-24) retired the connection code described below: the mcp.json read, the mirrored validation,
> the `2024-11-05` handshake pin and the stdio spawn no longer exist in the generated bridge, which is now a thin
> helper-protocol client ([[#14. Helper-Protocol Bridge (as built)]]). The text below is
> kept for the parts that still hold (result semantics, error kinds) and is rewritten at SDD-4.

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
- `callMCPTool(serverId, toolName, params, resultSource = 'content')` takes an optional fourth
  argument, `'structured' | 'content'` (`ResultSource`). Result extraction:
  - `isError` result: `McpToolError('tool_error')`; JSON-RPC `error`: `McpToolError('rpc_error')`;
    transport failure, timeout or a malformed response: `McpToolError('call_failed')`. It throws
    nothing else (any other thrown value is wrapped as `call_failed`).
  - `resultSource === 'structured'` and a non-null `structuredContent`: return it, even when
    `content` is populated (FR-030.4). The generator passes `'structured'` for every tool that
    declares an output schema, at every degrade stage, so extraction never depends on byte budgets.
  - Otherwise JSON-shaped text content is parsed, plain text is returned, a non-text content item
    is returned as the item itself, and `structuredContent` is the fallback when `content` is
    empty or its first item is null.
  - Empty `content` with no `structuredContent` resolves to `undefined` and does not throw
    (FR-030.6); it is not recorded as an observed shape.
- `McpToolError extends Error { kind: 'tool_error' | 'rpc_error' | 'call_failed';
  serverMessage?: string; serverMessageTruncated: boolean }` (#570, A5):
  - `.message` is fixed bridge text. Server text (an `isError` result's text, falling back to its
    `structuredContent` as JSON; a JSON-RPC `error.message`, including the one of a failed
    `initialize`) goes only into `serverMessage`, after `boundServerText`. A server text that is
    not a string is dropped, not coerced.
  - `boundServerText` mirrors `mcp_execution_core::untrusted::sanitize_untrusted_text` exactly
    (control, bidi, zero-width, Tags-block handling, and both variation-selector rules) and
    truncates by **code points** to `MAX_SERVER_MESSAGE_LEN` (= `MAX_UNTRUSTED_FIELD_LEN`, 500),
    setting `serverMessageTruncated`. The three constants are rendered from `BridgeContext`; the
    sanitizer's vectors live in `crates/mcp-core/tests/fixtures/untrusted_text_vectors.json` and are run against the Rust
    `sanitize_untrusted_text` by `crates/mcp-core/tests/untrusted_text_vectors.rs` (#624); `shared_vectors.json` keeps only
    `observed_key_rule`.
  - Breaking: scripts that matched server wording in `error.message` must read `serverMessage`.
- The opt-in observed-shape recorder is in [[#13.8 Observed result shapes (#587)]].

## 9. Resource-Exhaustion Bounds (CWE-400)

- `enforce_tool_count_bound` rejects before any per-tool rendering if
  `tools.len() + FIXED_FILE_COUNT > MAX_GENERATED_FILES`.
- `add_tracked` checks both the running byte total (`MAX_GENERATED_BYTES`)
  and file count (`MAX_GENERATED_FILES`) **incrementally, as each file is
  produced** — not only after the whole `GeneratedCode` is assembled — so
  an oversized `ServerInfo` is rejected as soon as the offending file is generated, never
  holding the full amplified output in memory first. This incremental check remains the backstop
  for direct library callers; for an introspection-valid `ServerInfo` it never fires (below).
- Both bounds are **derived from**, not independently chosen relative to,
  `mcp-introspector`'s own `MAX_TOOL_COUNT`/`MAX_TOOL_NAME_LEN`/etc. — a
  `ServerInfo` that already cleared introspection's bounds can never be
  deterministically rejected here for simply being "as large as
  introspection already allows."

### Provable bytes bound (FR-030.8)

| Constant (`progressive/generator.rs`) | Value / definition |
|---|---|
| `PER_TOOL_BYTES_MAX` | `2 * (MAX_TOOL_NAME_LEN + MAX_TOOL_DESCRIPTION_LEN + MAX_SCHEMA_SIZE_BYTES)` = 147,968 |
| `MAX_GENERATED_BYTES` | `MAX_TOOL_COUNT * PER_TOOL_BYTES_MAX + FIXED_FILES_BYTES_MAX` = 148,230,144 (about 148.2 MB) |
| `FIXED_FILES_BYTES_MAX` | 256 KiB: the bridge, `_runtime/cli.ts`, `package.json`, `tsconfig.json` and the headers of `index.ts`, `catalog.ts`, `_meta.json` (a test keeps the real output under half of it) |
| `INDEX_ENTRY_MAX_BYTES`, `CATALOG_ENTRY_MAX_BYTES` | 4096 each |
| `PER_TOOL_RENDER_BUDGET` | `PER_TOOL_BYTES_MAX - CATALOG_ENTRY_MAX_BYTES - INDEX_ENTRY_MAX_BYTES` |
| `STORED_TYPE_TEXT_MAX_BYTES` | 1024 |
| `TOOL_FILE_DEGRADED_MAX`, `META_ENTRY_DEGRADED_MAX` | const expressions over capped inputs only (see below) |

`MAX_GENERATED_BYTES` keeps the per-tool basis of the pre-#556 value (one `2 *` factor over name +
description + one schema, 147,968,000 bytes at 1,000 tools) and adds `FIXED_FILES_BYTES_MAX`
for the files that do not scale with the tool count. The per-tool term is a bound on everything
one tool contributes to its tool file, its `_meta.json` entry, its `catalog.ts` entry and its
`index.ts` line, not a count of one schema.

**Two-stage per-tool degrade.** For each tool the generator measures `T` (the tool file's bytes)
and `M` (the bytes of its compact `_meta.json` entry, plus 1 for the comma). The stage logic lives in
`ProgressiveGenerator::render_within_budget`; the per-tool conversions and the `DegradeStage`
(`Full`, `ResultGeneric`, `AllGeneric`) live in the internal `progressive/tool_types.rs`
(`ToolConversions`, `ToolTypes`; `pub(crate)`, no public API).

- **Stage A** applies whenever the tool's total `T + M` exceeds `PER_TOOL_RENDER_BUDGET`,
  regardless of whether the input or the output side caused it; a declared result type can
  therefore be dropped because of a large input schema. Stage A makes the result generic: the
  observed-shape hint is dropped, `{Name}Result` becomes the generic union (see
  [[#13.2 Typed results (#482)]]), the stored `result_type` is `None`, and the origin is
  `Degraded` when a schema was declared.
- **Stage B** applies when the total still exceeds the budget after Stage A. It then also replaces
  Params with `Record<string, unknown>` and stores `parameters: []`.
- The order is fixed so the output is deterministic; the bound proof relies only on Stage B. Stage A
  or B can be reached well below `MAX_SCHEMA_SIZE_BYTES`, for example by an input schema of about
  1,500 small properties, because each `_meta` parameter entry costs about 80 B. Each stage step
  emits one `tracing::warn!` (tool, size, budget). The skill then shows no parameters for a tool
  degraded at Stage B.
- **Why Stage B fits.** It contains only capped inputs: name ≤ 256 B (three places in the file,
  two in the entry, each ×6 for `\u00XX` escaping), the sanitized description and short
  description, the categorization (assumed ≤ 1 KiB, ×6), the title (≤ 500 chars × 4 B, in two
  places), four hint tags, and fixed template text. `TOOL_FILE_DEGRADED_MAX` and
  `META_ENTRY_DEGRADED_MAX` are const expressions over those caps, and a compile-time `assert!`
  checks `TOOL_FILE_DEGRADED_MAX + META_ENTRY_DEGRADED_MAX + CATALOG_ENTRY_MAX_BYTES +
  INDEX_ENTRY_MAX_BYTES <= PER_TOOL_BYTES_MAX`. The assert checks the arithmetic over the modelled
  constants only; the adversarial-fixture tests are what prove that rendered output stays within
  them.
- **Conclusion.** For an introspection-valid `ServerInfo`, total output ≤ `MAX_TOOL_COUNT` ×
  `PER_TOOL_BYTES_MAX` + `FIXED_FILES_BYTES_MAX`, so the incremental check never fires.
- **Stated assumptions.** (1) Categorization stays within `mcp-server`'s own caps (category 100,
  keywords 500, short description 320). (2) FR-030.8 assumes only the server name/version copy in
  `_meta.json` is unbounded at introspection: `index.ts` already caps both (256 and 64
  characters), but `_meta.json` stores `server_name`/`server_version` raw. A larger input from a
  direct library caller is still rejected cleanly by the incremental check. Known limitation:
  #599.
- `mcp-files`' `MAX_EXPORT_FILES`/`MAX_EXPORT_BYTES` equal these values through the existing
  derivation ([[../files/spec]]).

### `_meta.json` against the skill scan cap

`_meta.json` is compact JSON, which removes indentation from every entry and makes the per-entry
measurement above exact. Stored result and parameter type texts are capped at
`STORED_TYPE_TEXT_MAX_BYTES` (1024): a longer `result_type` is stored as `None`, a longer
`parameters[].typescript_type` as `"unknown"`, so per-tool growth over v2 is bounded. The cap the
skill scan accepts is `mcp_execution_core::metadata::MAX_METADATA_FILE_BYTES` (1 MiB), shared by
producer and consumer (`mcp-skill`'s `MAX_FILE_SIZE` is defined from it). **Behaviour above the
cap:** generation still succeeds, because the tool files are usable directly, and emits one
`tracing::warn!` naming the size, the cap and the consequence; the skill scan (and the server tools
reusing it) then returns `ScanError::FileTooLarge`. Generation is not rejected because v2 could
already exceed the cap for large servers.

## 10. Error Conditions

| Condition | `Error` variant |
|---|---|
| Tool count would exceed `MAX_GENERATED_FILES` | `ResourceLimitExceeded { resource: ResourceKind::ToolCount { server_id }, .. }` |
| Running byte total exceeds `MAX_GENERATED_BYTES` | `ResourceLimitExceeded { resource: ResourceKind::GeneratedOutputSize, .. }` |
| Malformed or unsupported schema node | **No error**: the converter never fails; the node renders `unknown` and a diagnostic is recorded ([[#13.1 Schema-to-TypeScript conversion (#565, #584)]]) |
| Rendered catalog exceeds `MAX_CATALOG_BYTES` (only for more than `MAX_TOOL_COUNT` entries or an oversized server name) | `ResourceLimitExceeded { resource: ResourceKind::GeneratedOutputSize, .. }` |
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
  (schema owned by `mcp-core::metadata`, version 3). Both consumers exempt
  `mcp_execution_core::metadata::CATALOG_FILE_NAME` from their tool-file scans, and `mcp-skill`'s
  `MAX_FILE_SIZE` is `MAX_METADATA_FILE_BYTES`, the cap this crate warns against.
- **Consumes** `mcp-core::annotations::{Hint, ToolAnnotations}` (carried on `ToolInfo.annotations`),
  `mcp-core::metadata::{ResultTypeOrigin, CATALOG_FILE_NAME, MAX_METADATA_FILE_BYTES}` and
  `mcp-core::untrusted::{MAX_UNTRUSTED_FIELD_LEN, MAX_VARIATION_SELECTOR_RUN,
  MAX_TOTAL_VARIATION_SELECTORS}` (rendered into the bridge).
- **Consumed by** `mcp-cli`'s `generate --observed-shapes` through
  `ProgressiveGenerator::with_observed_shapes`.

## 12. Edge Cases & Notable Behaviors

- A tool name containing only invalid identifier characters (e.g. all
  non-ASCII) sanitizes to a bare `_`, then gets disambiguated if colliding.
- Property keys are rendered raw (bare or quoted), so a key that is a JS/TS reserved word is a
  legal property name and needs no special case; only the top-level tool function name is
  reserved-word-aware.
- `sanitize_ts_identifier` collapses an entire run of consecutive
  invalid/underscore-producing characters into a single `_`, not one `_`
  per character — a name like `café_menu_日本語` sanitizes to `caf_menu_`,
  and regenerating a server whose tools have such names can produce
  different identifiers than an older generated package did.
- `ProgressiveGenerator::render_within_budget` converts each tool's schemas once
  (`ToolConversions`); the tool file, the catalog entry and the `_meta.json` entry are all rendered
  from that one conversion at the same degrade stage (FR-032.11, issue #295's single-walk rule).
- An object schema with no properties renders `{ [key: string]: unknown }` when
  `additionalProperties` is absent or `true`, and `Record<string, never>` when it is `false`.
- Two `generate`/`generate_with_categories` calls against identical `ServerInfo`/`ServerConfig`
  input agree on `_meta.json`'s `provenance.config_fingerprint` and `provenance.tool_digest` —
  only `provenance.generated_at` differs between runs (issue #468). `_meta.json`'s digest is
  computed from `server_info.tools`, not from the generated `.ts` files' content, so it does
  **not** detect drift introduced purely by `resolve_typescript_names`' index-based collision
  suffixes or by a changed `categorizations` map — see
  [[../core/spec#What provenance does and does not answer]].

## 13. Script-First Capabilities (implemented, #556)

> [!info] Provenance of this section
> Written from the architect's plan (`.local/handoff/2026-11-24T13-07-44-architect.md`, revision 3)
> and the code as merged, for issues #583, #565, #482, #587, #584, #570, #567, #566 and #568
> (parent #556). Requirement IDs are those of the project requirements document plus a suffix
> (`FR-032.4` is the fourth requirement of FR-032). Where the plan and the code differ, the code
> is described. Several packages changed generated bytes for every server (lossless types, slimmer
> files, quiet output, a catalog, `| undefined` on every result); each is listed under "Breaking"
> in `CHANGELOG.md`, and there is no compatibility shim before 1.0.

### 13.1 Schema-to-TypeScript conversion (#565, #584)

One converter, `common::ts_types::convert_schema(&Value) -> ConvertedType`, serves input schemas,
output schemas and catalog signatures (FR-031.11). It walks a schema once into a private IR with
bounds and diagnostics, and renders it as `Declaration` (tool files: multi-line, member JSDoc) or
`Compact` (catalog and stored texts: one line, no JSDoc, never longer than `Declaration`). The
size budget counts the `Declaration` form, so the `Compact` form always fits. The old
`json_schema_to_typescript`, `json_type_to_typescript`, `extract_properties` and the
`PropertyInfo`/`extract_property_data` pair are removed (breaking, FR-030.7: no output-only path).
`$ref` resolves against the root document.

| ID | Requirement | Status |
|----|-------------|--------|
| FR-031.1 | `enum`/`const`: a union of the literal members (or the single literal) in declared order, deduplicated. An enum over `MAX_UNION_MEMBERS` renders its base primitive when all members share one JSON type, otherwise `unknown`, plus `UnionTooLarge` | implemented |
| FR-031.2 | `oneOf`/`anyOf`: a union; member unions are flattened and deduplicated (#601), and a flattened union over `MAX_UNION_MEMBERS` is `unknown` plus `UnionTooLarge`. `allOf`: an intersection up to `MAX_INTERSECTION_MEMBERS` (8), `unknown` plus `IntersectionTooLarge` beyond. A node with several parts renders their intersection | implemented |
| FR-031.3 | `type` as an array: the union of the listed types; `null` renders `null`; optional (member may be absent) stays distinct from nullable | implemented |
| FR-031.4 | A local `$ref` (JSON Pointer into the same document) is resolved with cycle detection and an expansion bound | implemented |
| FR-031.5 | A non-local ref renders `unknown` plus `NonLocalRef`, an unresolvable one `UnresolvedRef`; nothing is fetched. A cyclic ref is cut at its first re-entry (the ref is already on the expansion stack) with `unknown` plus `CyclicRef`; non-cyclic refs count against `MAX_REF_EXPANSIONS` (64 per root), beyond it `RefExpansionLimit` | implemented |
| FR-031.6 | An unsupported construct renders `unknown` for the smallest enclosing node plus a diagnostic. **Deliberate deviation, see below** | implemented with deviation |
| FR-031.7 | The converter returns the type with a structured diagnostic list (JSON-pointer `SchemaPath`, sanitized and capped at 256 chars, plus a `DiagnosticKind`), at most `MAX_DIAGNOSTICS` (32) plus a dropped count | implemented |
| FR-031.8 | `format`, `default`, `minimum`, `maximum`, `minLength`, `maxLength`, `minItems`, `maxItems` render as JSDoc tags (`@format`, ...) on the member, `sanitize_jsdoc`-ed and capped at 256 chars, in the `Declaration` form only | implemented |
| FR-031.9 | `additionalProperties`: absent or `true` gives `[key: string]: unknown`; `false` gives none; a schema gives a typed index signature (see S1 below) | implemented, amended |
| FR-031.10 | Output is byte-identical across runs (declared property and union order) | implemented |
| FR-031.11 | One converter for Params, Result and catalog signatures | implemented |

**Deviation from FR-031.6 (A8, approved by the user).** Narrowing-only keywords (`not`, `if`/`then`/
`else`, `patternProperties`, `propertyNames`, `dependentSchemas`/`dependentRequired`/`dependencies`,
`unevaluatedProperties`/`unevaluatedItems`, `contains`, `uniqueItems`, `multipleOf`) are deliberately
**not** turned into `unknown`: the node keeps its superset rendering and a `NarrowingKeywordIgnored`
diagnostic is recorded. Tuples (`items` as an array, `prefixItems`) render `unknown[]` plus a
`TupleItems` diagnostic. A `$ref` with non-annotation sibling keywords resolves the ref and ignores
the siblings, recording `RefSiblingsIgnored`; annotation siblings (`description`, `title`,
`default`, ...) record nothing. `nullable` and `$schema` are ignored (not 2020-12 vocabulary).
**Rationale (#565):** the type should be as close as possible to what the server accepts, and a
sound superset loses less than `unknown`. This supersedes the original wording of FR-031.6 for
these constructs.

**Bounds** (all `pub(crate)` in `ts_types.rs`): `MAX_SCHEMA_RECURSION_DEPTH` 128 (every descent
counts); `MAX_UNION_MEMBERS` = **128** (raised from the planned 64 to match the protected
acceptance test); `MAX_INTERSECTION_MEMBERS` 8; `MAX_REF_EXPANSIONS` 64; `MAX_DIAGNOSTICS` 32;
`MAX_RENDERED_TYPE_BYTES` = `MAX_SCHEMA_SIZE_BYTES` (64 KiB), charged per node, so a small schema
whose references multiply cannot amplify past the budget. Hitting a bound yields `unknown` at the
smallest node plus one diagnostic, never an error or a hang.

**Typed index signature (S1).** WHEN an object schema has representable properties and an
`additionalProperties` schema THE SYSTEM SHALL render a single object type whose index signature
value is the flattened, deduplicated union (member unions are flattened first, so no member repeats) of the converted `additionalProperties` type, followed by each
property type in declared order, followed by `undefined` when any property is optional (e.g. `{ a?:
string; [key: string]: number | string | undefined }`). It SHALL NOT render an intersection for
this case (`{a?: string} & {[k: string]: number}` cannot be satisfied). The index value SHALL be
`unknown` when the union would exceed `MAX_UNION_MEMBERS` (128), contains `unknown`, or would exceed
the rendered-size budget (`MAX_RENDERED_TYPE_BYTES`). No diagnostic is recorded. WHEN there are no
representable properties the index value is the converted `additionalProperties` type alone. This
applies to input and output schemas alike, because there is one converter. An absent
`additionalProperties` adds an index signature, so the compiler no longer flags typos in excess
properties (FR-031.9 as written).

**Property keys and literals (#584).** A key that is an ASCII identifier renders bare; any other
key renders quoted through `ts_string_literal_exact` ([[#7. Injection Defense (Sanitization Pipeline)]]).
Keys are never renamed or suffixed, and `_meta.json` `parameters[].name` holds the raw key. A key
with no exact literal is omitted, and the object gains `[key: string]: unknown` plus
`KeyNotRepresentable`. Enum and const strings follow the same rule: if any member cannot be
represented exactly (for example a 600-character string), the node renders its base primitive plus
`LiteralNotRepresentable`.

**Public result and root state.** `ConvertedType` is opaque; `root_degraded()` is true **only** when
the root itself became `unknown` and a diagnostic exists at the root path. The A8 diagnostics
`NarrowingKeywordIgnored` and `RefSiblingsIgnored` keep the declared superset type in use (they do
not degrade the root). A root `{}` with no diagnostic renders `unknown` and is not degraded.
`is_object_like()` (the root is an object, or a union/intersection of object-like members) gates
Params: a non-object-like input schema renders `Record<string, unknown>` plus a warning.
`members()` lists the root's representable members (raw name, `Compact` type text, required) and
feeds `_meta.json` parameters.

**Diagnostics surface.** One `tracing::warn!` per tool and role (input, output, observed, budget)
giving the count and up to 4 kinds. They are not stored in `_meta.json` (FR-030.9 is met by the
warning). Server strings reach the output only via `sanitize_jsdoc` (comments) or
`ts_string_literal_exact` (literals and keys). Ask first: changing the public converter functions
(`convert_schema`, `ConvertedType::render`, `RenderStyle`).

### 13.2 Typed results (#482)

| ID | Requirement | Status |
|----|-------------|--------|
| FR-030.1 | WHEN a tool declares an output schema the converter can represent, `{Name}Result` is derived from it whatever the root (object, array, primitive, type array, composition), as `<Declaration text> \| undefined` | implemented |
| FR-030.2 | WHEN a tool declares no output schema, `{Name}Result` is `Record<string, unknown> \| unknown[] \| string \| undefined` | implemented |
| FR-030.3 | **Amended (Q1).** WHEN a tool declares an output schema and its declared result type is not used (the converter degraded the root, or the per-tool budget degraded the result at Stage A or Stage B) THE SYSTEM SHALL keep the structured result source and SHALL emit `{Name}Result` as `Record<string, unknown> \| unknown[] \| string \| number \| boolean \| null \| undefined`, because `structuredContent` may be any JSON value. Tools without an output schema keep `Record<string, unknown> \| unknown[] \| string \| undefined`. Generation does not fail | implemented |
| FR-030.4 | A tool that declares an output schema and returns non-null `structuredContent` gets it from the runtime, whether or not `content` is populated | implemented (see [[#8. Runtime Bridge (`_runtime/mcp-bridge.ts`)]]) |
| FR-030.5 | Without `structuredContent` the value is returned as before; it is never validated against the schema | implemented |
| FR-030.6 | A success with empty `content` and no `structuredContent` resolves to `undefined` and does not throw. The printed form of an empty success in single-call mode is nothing, exit 0 | implemented |
| FR-030.7 | One converter for output schemas, no output-only path | implemented |
| FR-030.8 | The generated-bytes bound is derived so an introspection-valid `ServerInfo` is never rejected at generation | implemented ([[#9. Resource-Exhaustion Bounds (CWE-400)]]) |
| FR-030.9 | A degraded output schema surfaces its diagnostic in a `tracing::warn!` | implemented |

Every Result type ends in `| undefined` (the `void` member of the old open decision is `undefined`
on all tools). The generated tool function passes `result_source` (`'structured'` when the tool
declares an output schema, else `'content'`) as the fourth argument of `callMCPTool`, independent
of the degrade stage: extraction semantics never depend on byte budgets. `ResultSource` is a
public enum (`Content`, `Structured`). `_meta.json` records `result_type` (the `Compact` text,
`None` above 1024 bytes or when degraded) and `result_type_origin` (`declared`, `degraded`,
`undeclared`).

### 13.3 Per-tool budget and degrade

Specified in [[#9. Resource-Exhaustion Bounds (CWE-400)]] (two stages, constants, proof,
assumptions).

### 13.4 Typed tool catalog (#566)

`catalog.ts` is a top-level, type-only file with one signature per tool.

**Why a `.ts`, at the top level.** A `.ts` file is type-checked under the existing tsconfig, whereas
a `.d.ts` would be skipped because of `skipLibCheck`. It sits at the top level, not under `_runtime/`,
so a directory listing shows it beside `index.ts` as the second detail level (FR-032.6).

| ID | Requirement | Status |
|----|-------------|--------|
| FR-032.1 | Exactly one catalog per server, one entry per tool | implemented |
| FR-032.2 | An entry holds the TypeScript name, the Params type, the Result type (both `Compact`), a one-line description (sanitized, at most `CATALOG_DESCRIPTION_CHARS` = 120) and, when hints are stated, `@hints readOnly=true destructive=false` (only stated hints; fixed tokens) | implemented |
| FR-032.3 | Valid TypeScript that type-checks and imports nothing: `export interface Catalog { name(params: P): Promise<R>; ... }` | implemented |
| FR-032.4 | Entries sorted by TypeScript name; byte-identical output for identical input | implemented |
| FR-032.5 | A description is reduced to its first line, sanitized and capped | implemented |
| FR-032.6 | The header documents the three detail levels: `index.ts` (names), `catalog.ts` (signatures), `<tool>.ts` (full) | implemented |
| FR-032.7 | An entry over `CATALOG_ENTRY_MAX_BYTES` (4096) degrades to `name(params: Record<string, unknown>): Promise<unknown>` with `@see ./name.ts`; no tool is ever dropped. The file is bounded by `MAX_CATALOG_BYTES` = `CATALOG_HEADER_MAX_BYTES` (4096) + `MAX_TOOL_COUNT` × `CATALOG_ENTRY_MAX_BYTES`; exceeding it is `ResourceLimitExceeded` | implemented |
| FR-032.8 | The catalog counts in the file and byte bounds (`FIXED_FILE_COUNT` is 7; the per-tool budget reserves `CATALOG_ENTRY_MAX_BYTES`) and passes the incremental check | implemented |
| FR-032.9 | `catalog` is in `RESERVED_OUTPUT_NAMES`: a colliding tool is disambiguated, not the catalog | implemented |
| FR-032.10 | `mcp-skill`'s `verify_tool_files_on_disk` and `mcp-server`'s directory-listing tool count exempt `CATALOG_FILE_NAME` (`mcp_execution_core::metadata`), so it is neither counted as a tool nor reported as unreferenced | implemented |
| FR-032.11 | The catalog and each tool file are rendered from the same converted types in one pass | implemented |

Untrusted text: names, descriptions and enum literals are sanitized as in [[#7. Injection Defense (Sanitization Pipeline)]];
hint markers are fixed tokens, never server text; the optional annotation title is not in the catalog.

### 13.5 Annotations in generated output (#568)

The fields of `_meta.json` v3 are defined once, in [[../core/spec#`metadata` module (`src/metadata.rs`)]];
capture is in [[../introspector/spec]].

| ID | Requirement | Status |
|----|-------------|--------|
| FR-034.20 | Each stated hint renders as a JSDoc tag with a fixed name and value in the tool file: `@readOnlyHint true\|false`, `@destructiveHint`, `@idempotentHint`, `@openWorldHint` | implemented |
| FR-034.21 | A hint that is not stated, or a tool with no annotations, renders no tag and no default | implemented |
| FR-034.22 | The title renders only as `@title <text>` through `sanitize_jsdoc` capped at 256 chars | implemented |
| FR-034.23 | Hints are rendered as server-declared values, never verified properties | implemented |
| FR-034.24 | `_meta.json` stores annotations, `result_type`, `result_type_origin` and `manifest_size_bytes` (the length of the compact `serde_json::to_vec` of the `ToolInfo`); one schema bump, to 3 | implemented |
| FR-034.25 | No generation code branches on an annotation | implemented |
| FR-034.26 | Contradictory hints (read-only with destructive) are rendered as advertised | implemented |

Annotations enter the provenance digest entries (`ToolDigestEntry.annotations`): the presence of the
annotations value and the four hints, three-valued; the title text is excluded (domain tag
`tool-entry:v2`, see [[../core/spec#`provenance` module (`src/provenance.rs`)]]). Per-tool digests,
a catalog reference and stored converter diagnostics were resolved to "out".

### 13.6 Single-call mode (#567, #570, #583)

Tool files are slim and delegate to `_runtime/cli.ts`, a static module (no server data, so the
top-level ask-first does not apply) that exports `runIfMain(moduleUrl, tool)`. Each tool file ends
with `runIfMain(import.meta.url, {name});`. The shebang line stays (`setup` chmods `.ts` files to
0755). The direct-run defect #583 (the old `import.meta.url` vs `` `file://${argv[1]}` `` comparison
failed for a path with a space, non-ASCII characters or a symlink, so the file exited 0 without
running) is fixed in the shared module; it is no longer a separate item from #567.

| ID | Requirement | Status |
|----|-------------|--------|
| FR-033.1 | One shared module per server holds the single-call behaviour; tool files delegate | implemented (`_runtime/cli.ts`) |
| FR-033.2 | Header information is emitted once per tool file | implemented |
| FR-033.3 | No fixed explanatory comment before `{Name}Result` (an `@observed` hint, [[#13.8 Observed result shapes (#587)]], is the only comment there) | implemented |
| FR-033.4 | A tool file run directly with a JSON argument behaves as FR-035 states (this changes the old block's stdout/stderr shape) | implemented |
| FR-033.5 | Importing a tool file executes no command-line code | implemented |
| FR-033.6 | `@tool` and `@keywords` appear exactly once and stay greppable | implemented |
| FR-033.7 | Exported names `{name}`, `{Name}Params`, `{Name}Result` and the `index.ts` re-exports are unchanged | implemented |
| FR-033.8 | The shared module is counted in the bounds (`FIXED_FILE_COUNT` 7); it lives under `_runtime/`, which the directory listing ignores | implemented |
| FR-033.9 | No server-supplied text in the shared module; tool files embed names and server ids only through the string-literal sanitizer | implemented |
| FR-033.10 | The average tool file is at most 50% of the recorded master baseline on the integration fixture | implemented (tested) |
| FR-035.1 | Success: one compact JSON line on stdout, then exit 0 inside the write callback; an empty success writes nothing and exits 0 | implemented |
| FR-035.2 | Failure: exactly one JSON line on stderr, `{"error": <fixed message>, "kind": "invalid_arguments"\|"tool_error"\|"rpc_error"\|"call_failed"}` plus `server_message` and `server_message_truncated` when the server supplied text; exit 1 inside the write callback | implemented |
| FR-035.3 | With `MCPBRIDGE_DEBUG` not `1`, no stack or internal path; with it, a `stack` field is added and the bridge's own debug lines may precede the failure line | implemented |
| FR-035.4 | The argument is `argv[2]` as one JSON object; a missing or empty argument means `{}`; text that is not valid JSON or not an object gives `invalid_arguments` and the argument text is never echoed | implemented |
| FR-035.5 | Server text is carried only in `server_message` (sanitized, truncated to 500 code points, `server_message_truncated` set), never in `error`, so it cannot pass for the tool's own wording | implemented |
| FR-035.6 | A throw that is not an `McpToolError` yields `kind: "call_failed"` and a fixed message | implemented |
| FR-035.7 | Exit 0 for success only, 1 for every failure; there are no distinct non-zero codes | implemented |
| FR-035.8 | The relocation by #567 preserves FR-035.1 to FR-035.7 | implemented |

**Entry detection.** `realpathSync(fileURLToPath(moduleUrl))` is compared with
`realpathSync(argv[1])`; if `realpath` throws, the side falls back to `path.resolve`; an undefined
`argv[1]` is "not the entry point". So a symlinked directory, a path with spaces or non-ASCII
characters and a relative path all match. **Exit after drain:** `process.exit` is called inside the
`write` callback because an exit straight after a write to a pipe can cut buffered output, and live
bridge child handles keep the process alive, so `exitCode` alone would hang (a result over 128 KiB
through a pipe arrives complete). The module is static text with a golden test; it adds no
dependency and uses no `eval` or dynamic import.

Successful results are printed as the server returned them (compact JSON). This specification
claims neither sanitisation nor an exception for them; whether the untrusted-text boundary rule
applies to successful results printed by single-call mode, the runner and the one-shot call is a
maintainer decision that remains open ([[../cli/spec#Security posture of the runner]]).

### 13.7 Reserved names

`RESERVED_OUTPUT_NAMES` = `index`, `catalog`, `runIfMain`, `callMCPTool` (see
[[#5. TypeScript Identifier Resolution]]).

### 13.8 Observed result shapes (#587)

An opt-in recorder lets a generated bridge learn the JSON shape of results from tools that declare
no output schema, and a later `generate` shows it as a hint.

**Recording.** WHEN the environment variable `MCPBRIDGE_OBSERVE_SHAPES=<file>` is set THE SYSTEM
SHALL append, after each successful call that returned a value (not for an empty success), one JSON
line `{"server","tool","shape"}`, where `shape` is a JSON-Schema fragment of JSON types only, never
of values: `null`, `boolean`, `integer`/`number`, `string`, `array` (the first element's shape as
`items`), `object`. When the variable is unset nothing is written. Failures are swallowed and
debug-logged; recording never affects the call.

**Key rule (S7) and bounds.** An object keeps its keys only when it has at most 16 keys and every
key matches `^[A-Za-z_$][A-Za-z0-9_$]*$` and is at most 32 characters; otherwise it is recorded as
`{"type":"object","additionalProperties":<shape of its first value>}`, so IDs, emails, paths and
UUIDs used as map keys never reach the record. Nesting stops at depth 4. A line over 8192 bytes
**including the trailing newline** is not recorded. Every bound is a Rust constant in
`progressive/observed.rs` (`OBSERVED_SHAPE_MAX_DEPTH` = 4, `OBSERVED_SHAPE_MAX_KEYS` = 16,
`OBSERVED_SHAPE_MAX_KEY_LEN` = 32, `OBSERVED_SHAPE_MAX_LINE_BYTES` = 8192) and the identifier
pattern is `core::untrusted::BARE_IDENTIFIER_PATTERN`; all are rendered into the bridge from Rust through
`BridgeContext` (the pattern through `sanitize_ts_string_literal`, private with an accessor), so
the TypeScript recorder hand-copies no bound. The shared vectors file (`observed_key_rule`) runs the
recorder and the Rust reader against the same cases.

**File safety.** The file is opened append-only (`O_APPEND`, created `0600`), with no-follow
(`O_NOFOLLOW`) and non-blocking (`O_NONBLOCK`, so a FIFO cannot hang the open). After opening, the
descriptor must be a regular file owned by the current user with no group or other permission bits
(where `process.getuid` exists); otherwise recording is silently skipped (debug-logged).

**Reader and rendering.** `ProgressiveGenerator::with_observed_shapes(&str)` parses the JSON lines
and treats them as untrusted: a line is skipped (counted in one warning) if it is not valid JSON, is
longer than 8192 bytes, is deeper than 4, breaks the key rule, or uses a keyword outside `type`,
`properties`, `required`, `items`, `additionalProperties`. The last valid line wins per (server id,
raw tool name); recorded objects are closed (`additionalProperties: false` unless the recorder wrote
one). A shape is used **only for tools without an output schema**: it is converted, rendered in
`Compact` form (at most `STORED_TYPE_TEXT_MAX_BYTES`), and shown as a JSDoc line above
`{Name}Result`: "Result shape observed in a past call, not declared by the server: ...". The type
itself stays generic and nothing is stored in `_meta.json`. Stage A of the budget degrade drops the
hint. The CLI front door is `generate --observed-shapes <file>` ([[../cli/spec]]), which reads only
the last 4 MiB.

> [!warning] Accepted residual risk (user-accepted)
> Identifier-like data keys still pass the key rule and are recorded: for example usernames, or
> hex IDs that start with a letter such as `deadbeef12`, when used as keys of a small object. The
> recorder keeps key names of objects with at most 16 identifier-shaped keys; it cannot tell a
> schema-like key from a data key. The file is opt-in and private (`0600`, owner-only), and the
> hint is rendered through `sanitize_jsdoc`.

### 13.9 `_meta.json` v3

Version 3 is the single schema bump of the package (`METADATA_SCHEMA_VERSION` = 3). The fields are
registered in [[../core/spec#`metadata` module (`src/metadata.rs`)]]. The generated file is compact
JSON, stored type texts are capped at 1 KiB, and the shared `MAX_METADATA_FILE_BYTES` (1 MiB) cap
and the oversize behaviour are in [[#9. Resource-Exhaustion Bounds (CWE-400)]]. A v2 sidecar is
rejected by consumers with the existing unsupported-schema error ([[../skill/spec#ScanError]]), so
every server directory generated before the upgrade must be regenerated.

### 13.10 Known limitations (tracked, not requirements)

- #598: the server listing misses tool files whose name starts with an underscore (the listing
  skips names beginning with `_`).
- #599: `server_name` and `server_version` are unbounded in `_meta.json` (see the FR-030.8
  assumption in §9).
- #600, #601: cosmetic refactors.

## 14. Helper-Protocol Bridge (as built)

- **Shape.** `_runtime/mcp-bridge.ts` keeps `callMCPTool`, `McpToolError` (kinds plus `write_skipped`),
  `ResultSource` and the observed-shape recorder (#587). Removed: all connection code, the mirrored validation, the
  mcp.json read, the `2024-11-05` pin, `boundServerText`, `closeAllConnections`, the SIGINT and SIGTERM handlers and
  the `MCPBRIDGE_REQUEST_TIMEOUT_MS` variable (breaking; call timeouts are now the helper's `DEFAULT_CALL_TIMEOUT`
  capped by the run time). The template went from about 1503 to about 600 lines. `FIXED_FILE_COUNT` stays 7. `cli.ts`
  derives `ErrorKind` from `McpToolError['kind']`.
- **`BridgeContext`** carries `helper_protocol_version`, `max_helper_line_bytes`, `helper_env`, `helper_token_env`,
  `cli_path_env` (rendered from core, [[../core/spec]]) plus the observed-shape fields and a private
  `identifier_pattern`; the validation fields and accessors are gone.
- **Channel selection.** The channel registered at `globalThis[Symbol.for('mcp-execution.helper-channel')]` wins. Else,
  if `MCP_EXECUTION_HELPER` is set, the call fails with `call_failed` (never a second helper). Else, if the run marker
  `MCP_EXECUTION_RUN` is set, the call fails with `call_failed` "refused: tool calls from a child process or worker
  thread are not supported under `run`; call tools from the main script". Else the bridge spawns the helper binary:
  `$MCP_EXECUTION_CLI` when set (absolute, no PATH fallback), otherwise `mcp-execution-cli` found through absolute
  `PATH` entries only, else "mcp-execution-cli not found on PATH; set MCP_EXECUTION_CLI"
  ([[../cli/spec#Run marker, pipe descriptor and Windows binary resolution (D25)]]), with `helper` as its argument, stdio `['pipe','pipe','inherit']`, no shell,
  `windowsHide`, `MCP_EXECUTION_HELPER*` removed from its environment, the child killed on node `exit`. The bridge never
  opens fd 0; the preload owns claiming, the hello and the token. In spawn mode the bridge sends the hello; in both
  modes the first helper line is the hello reply, version-checked, and a mismatch says to regenerate the servers.
  Requests wait for the reply.
- **ref/unref.** The channel is `unref()`d on adoption, `ref()`d while at least one call is pending and `unref()`d at
  zero; in spawn mode the child, its stdout and its stdin follow the same rule.
- **Bounds and failure.** The read buffer is capped at `MAX_HELPER_LINE_BYTES`; overflow, EOF, a stream error, a
  non-JSON line or child exit fails the connection: pending calls reject with `helper channel closed: <reason>`, later
  calls too, never reconnected. A request over the bound is rejected locally (`call_failed`, not sent). An unknown
  `kind` or a response with no body becomes `call_failed`; `server_message` and `server_message_truncated` pass
  through unchanged (the helper sanitizes).
- **Tests.** `tests/runtime_bridge.rs` was rewritten (14 tests of the bridge side of the wire under node, on a fake
  helper channel in `tests/acceptance_support/fake_helper.cjs`); `mcp-cli/tests/single_call_e2e.rs` runs against the
  real helper (only the typed-error test runs on Windows, the rest are ignored there, #604).
- **Breaking changes** (changelog): the list above, `BridgeContext` fields, regeneration of every existing server.
- The user docs (`crates/mcp-codegen/README.md`, `crates/mcp-cli/README.md`) describe the helper-protocol bridge and
  the regeneration requirement.
- Resolved (#624): the dead `untrusted_text` section is removed from `fixtures/shared_vectors.json`; the Rust sanitizer is
  pinned to hand-derived vectors in `mcp-core` ([[../core/spec#Untrusted-text vectors (#624)]]).

## 15. Bridge Retirement Requirements (#575)

> [!success] Implemented
> The requirements below were implemented in the script-first pull request (#569 accepted, option B). Each **Why**
> paragraph describes the state before the change; the as-built sections above govern where they differ. Decisions
> that are still open are marked.

> [!info] Fixed-file rule
> Every generation emits `FIXED_FILE_COUNT` = 7 fixed files: `index.ts`, `catalog.ts`, the runtime
> bridge, `_runtime/cli.ts`, `package.json`, `tsconfig.json` and `_meta.json`
> (`crates/mcp-codegen/src/progressive/generator.rs`). `MAX_GENERATED_FILES` is the introspection
> tool-count bound plus that constant, and `mcp-files` sets its export bound equal to it
> ([[../files/spec]]). A change that adds or removes a fixed generated file updates, in the same
> pull request, `FIXED_FILE_COUNT`, the equal bounds in `mcp-files`, and the "tool count + 7" and
> `N + 7` wording in [[#2. Public API Surface]], [[#4. Output: Generated File Set]] and
> [[#9. Resource-Exhaustion Bounds (CWE-400)]].

### retirement of the generated bridge (#575)

Code-generation side only. The per-script helper itself is specified in [[../cli/spec#Per-script helper and retirement of the generated bridge (#575, FR-040)]]. Gated
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
  specified in [[../cli/spec]] and adds no server-supplied text to generated source.
- Generation bounds and the sanitizers of [[#7. Injection Defense (Sanitization Pipeline)]] still
  apply to every remaining generated file.

**Resolved and open decisions.**

- Resolved: `callMCPTool` and `McpToolError` keep their names and signatures and are a thin client of the helper channel; `BridgeContext` is reduced to the helper-protocol values (section 14).

Ask first: removing `callMCPTool` from the package surface; changing the import specifier of any
generated file.

**Depends on / blocks.** Landed in the script-first pull request; no blocker remains.

## 16. See Also

- [[../introspector/spec]] — source of `ServerInfo`/`ToolInfo`
- [[../files/spec]] — consumer of `GeneratedCode`
- [[../server/spec#save_categorized_tools]] — MCP-tool caller of `generate_with_categories`
- [[../cli/spec#generate]] — CLI caller of `generate`
- [[../core/spec]] — registry of `_meta.json` v3 fields, annotation types, provenance digest
- [[../cli/spec]] — script runner, one-shot call and per-script helper
- [[#13. Script-First Capabilities (implemented, #556)]] — as-built text of #482, #565, #566, #567, #568, #570, #583, #584, #587
- [[#15. Bridge Retirement Requirements (#575)]] — the one change still planned in this block
