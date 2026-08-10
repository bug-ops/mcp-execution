---
aliases:
  - mcp-execution-skill spec
  - Skill Generation spec
tags:
  - sdd
  - spec
  - skill
  - security
created: 2026-07-27
status: documented
related:
  - "[[../constitution]]"
  - "[[../core/spec]]"
  - "[[../server/spec]]"
  - "[[../cli/spec]]"
---

# Block: Skill Generation (`mcp-execution-skill`)

> [!abstract]
> Path: `crates/mcp-skill`. Generates Claude Code `SKILL.md` files from a
> server's already-generated tool set (reading the `_meta.json` sidecar, not
> the `.ts` source). Used by both `mcp-cli skill` (directly renders SKILL.md)
> and `mcp-server`'s `generate_skill`/`save_skill` tools (returns a prompt for
> Claude to compose SKILL.md, then confines/writes the result). Depends on
> `mcp-execution-core` only.

## 1. Responsibility

1. **Scan** (`parser`): read a server directory's `_meta.json` sidecar,
   cross-check it against the `.ts` files actually on disk, and produce
   `ParsedToolFile`/`ParsedParameter` structures.
2. **Contextualize** (`context`): group tools by category, pick
   representative examples, sanitize every untrusted field, and build an
   LLM-facing generation prompt wrapped in an explicit untrusted-data
   boundary.
3. **Render** (`template`): turn that context into either the LLM prompt or
   a directly-rendered `SKILL.md` (no LLM required), via embedded Handlebars
   templates.
4. **Confine** (`output_path`): validate and confine a caller-supplied
   `save_skill` output path to its base directory, symlink-aware.

## 2. Public API Surface

```rust
// crate root re-exports
pub use context::build_skill_context;
pub use output_path::{OutputPathError, resolve_skill_output_path};
pub use parser::{MAX_FILE_SIZE, MAX_FRONTMATTER_SIZE, MAX_TOOL_FILES,
    ParsedParameter, ParsedToolFile, ScanError, ScanResult, SkillMetadataError,
    extract_skill_metadata, scan_tools_directory};
pub use template::{TemplateError, render_generation_prompt, render_skill_md};
pub use types::{GenerateSkillParams, GenerateSkillResult, MAX_SERVER_ID_LENGTH,
    SaveSkillParams, SaveSkillResult, SkillCategory, SkillMetadata, SkillServerIdError,
    SkillTool, ToolExample, validate_server_id};

pub const MAX_SERVER_ID_LENGTH: usize = 64;
pub fn validate_server_id(server_id: &str) -> Result<(), SkillServerIdError>;
// Rules: non-empty, <= 64 bytes, only [a-z0-9-]

pub const MAX_TOOL_FILES: usize = 500;
pub const MAX_FILE_SIZE: u64 = 1024 * 1024;       // 1 MiB, _meta.json size cap
pub const MAX_FRONTMATTER_SIZE: usize = 8 * 1024; // 8 KiB, extracted YAML block cap
pub async fn scan_tools_directory(dir: &Path) -> Result<ScanResult, ScanError>;
pub struct ScanResult { pub tools: Vec<ParsedToolFile>, pub warnings: Vec<String> }

pub fn build_skill_context(server_id: &str, tools: &[ParsedToolFile], use_case_hints: Option<&[String]>) -> GenerateSkillResult;

pub fn render_generation_prompt(context: &GenerateSkillResult) -> Result<String, TemplateError>;
pub fn render_skill_md(context: &GenerateSkillResult) -> Result<String, TemplateError>;

pub fn extract_skill_metadata(content: &str) -> Result<SkillMetadata, SkillMetadataError>;

pub async fn resolve_skill_output_path(base_dir: &Path, server_id: &str, output_path: Option<&Path>) -> Result<PathBuf, OutputPathError>;
```

Key types (`types.rs`): `GenerateSkillParams`/`GenerateSkillResult` (the
MCP-tool-shaped request/response — `GenerateSkillResult` doubles as the
Handlebars render context for both templates), `SkillCategory`/`SkillTool`/
`ToolExample`, `SaveSkillParams`/`SaveSkillResult`/`SkillMetadata`.

## 3. `scan_tools_directory` — Sidecar-Backed Scanning

Replaces a historical regex-based `.ts`-file scanner (issue #141: it could
never recover parameter descriptions). Now:

1. Canonicalizes `dir`: an `io::ErrorKind::NotFound` maps to
   `ScanError::DirectoryNotFound`; any other I/O error (e.g. a symlink loop,
   a permission failure) now propagates as `ScanError::Io` with the real
   underlying cause, instead of being collapsed into `DirectoryNotFound` as
   before (issue #302's fix). Then canonicalizes `dir/_meta.json` under the
   same split — `NotFound` → `ScanError::MissingMetadata`, any other kind →
   `ScanError::Io` — and requires the resolved sidecar path to
   `starts_with(canonical_base)` — defends against a symlinked
   `_meta.json` escaping the directory (path-traversal via symlink).
2. Size-checks the sidecar (`MAX_FILE_SIZE`), parses as
   `mcp_core::metadata::ServerMetadata`, checks
   `schema_version == METADATA_SCHEMA_VERSION`, checks tool count ≤
   `MAX_TOOL_FILES`.
3. `verify_tool_files_on_disk`: every sidecar entry's `{typescript_name}.ts`
   must exist on disk, or the whole scan fails with
   `ScanError::StaleMetadata` (naming the tool, the missing file, and
   telling the caller to re-run `generate`) — first missing entry in
   sidecar order, not an exhaustive scan. A `.ts` file present on disk but
   **not** referenced by the sidecar is non-fatal: logged via
   `tracing::warn!`, excluded from the result, and surfaced as a
   human-readable string in `ScanResult::warnings` (issue #161's fix — the
   drift must be visible to a structured caller, not just server-side
   tracing). `index.ts` (the aggregator) is never treated as an "extra"
   file.
4. Each surviving sidecar entry is converted to a `ParsedToolFile` via the
   private `parsed_tool_file_from_metadata(meta, server_id)` function, which
   takes the scanned `server_id` directly. This replaced a public
   `impl From<ToolMetadata> for ParsedToolFile` that could only ever set
   `server_id: String::new()` as a placeholder, relying on this function to
   patch it in afterward — a representable-but-wrong intermediate state.
   Removing the `From` impl is a breaking change for any external caller
   that used it directly; `ParsedToolFile`'s own fields stay public, so it
   remains constructible directly (e.g. by test fixtures) (issue #342).
5. Tools returned sorted by name.

## 4. `build_skill_context` — Sanitization & Grouping

`group_by_category` treats every field on `ParsedToolFile` as **untrusted**
(it originates from the introspected MCP server, stored raw in `_meta.json`
by `mcp_execution_codegen::create_tool_metadata`) and sanitizes each via
`mcp_core::untrusted::sanitize_untrusted_text(_, MAX_UNTRUSTED_FIELD_LEN)`
**before** it can reach either the SKILL.md body (rendered with
triple-stash `{{{...}}}`, so HTML-escaping doesn't help) or the LLM-facing
prompt: `name`, `description`, `category` (sanitized *before*
`humanize_category` derives `display_name`, so the heading can't
reintroduce a control character), `keywords`, parameter names. Tools
without a category are grouped under `"uncategorized"`, sorted last.

`select_example_tools` prioritizes tools whose name starts with `create`,
`list`, `get`, `search`, `update` (in that order), one per not-yet-seen
category, then fills remaining slots.

## 5. Prompt Injection Defense

`build_generation_prompt` accumulates the categorized-tool-metadata section
separately and wraps it via `mcp_core::untrusted::wrap_untrusted_block`
(escaping `&`/`<`/`>` in the body, so a hostile description cannot forge the
boundary's own `<untrusted-data>`/`</untrusted-data>` delimiters) —
regression-tested end to end with a description attempting exactly that
forgery. Sanitization alone (flattening control characters) stops
structural Markdown breakout; the explicit boundary additionally tells the
LLM reader the enclosed text is inert data, not an instruction to follow —
these are two distinct, both-necessary defenses (issue #288's fix).

## 6. `render_skill_md` — Direct Rendering (No LLM)

Renders `GenerateSkillResult` through the embedded `skill-md.hbs` template
with triple-stash interpolation (no HTML-escaping needed/wanted). Before
rendering, `server_description` is **YAML-quoted** (`yaml_quote`: escapes
`\`, `"`, strips `\r`) so a `:`, leading `-`, or embedded newline in
server-supplied metadata cannot corrupt the frontmatter or inject a sibling
YAML key (issue's S3 fix). Output is CRLF-normalized to LF for
cross-platform (Windows CI) consistency.

The shared `HANDLEBARS` instance (both this and `render_generation_prompt`
render through it) enables `strict_mode(true)`, matching
`mcp_execution_codegen::TemplateEngine::new` (`specs/codegen/spec.md`, §2).
A template referencing a field absent from the render context now hard-fails
with `TemplateError::RenderFailed` instead of silently rendering an empty
string — the failure mode that regression-tests pin in `template.rs`.

## 7. `extract_skill_metadata` — Frontmatter Parsing

Extracts the `---`-delimited YAML block via a pre-compiled regex, then
parses it with **`serde_norway`** (a real YAML parser — block/folded
scalars, quoted scalars all handled correctly, unlike a naive single-line
regex capture that an earlier version used). The extracted block is
size-capped at `MAX_FRONTMATTER_SIZE` (8 KiB) **before** parsing, since
`serde_norway` (like other libyaml-based parsers) is not linear-time on
pathologically nested input — bounding only the overall `SKILL.md` size
would not bound parse latency. This pre-parse cap is the project-wide
contract for YAML input, not a local detail of this function: any future
YAML parse entry point applies its own cap to the slice it passes to
`serde_norway`, rather than inheriting a bound from an enclosing
document-size limit (see [[../constitution#V. Security]]).

`name`/`description` are required and treated as invalid if absent,
`null`/`~`, or blank-after-trim. A `serde_norway` deserialization error's
line number is corrected to be file-relative (the block starts one line
after the file's opening `---`).

> [!note]
> `serde_norway` remains this project's mandated YAML parser. A pure-Rust
> replacement (`serde-saphyr`) was evaluated and **not adopted** — see
> [[../decisions/ADR-341-serde-saphyr-vs-serde-norway]] for the full
> evidence ledger, the reasons the swap is deferred rather than rejected,
> and the measurable gate/review date (2026-10-27) for revisiting it.

Regression tests pin an incidental parser characteristic worth preserving:
`RawFrontmatter`'s `name`/`description` fields are declared as plain
`Option<String>`, not a buffering type. Because of that, a YAML "alias
bomb" (a handful of anchors each referencing the previous several times,
expanding to millions of nodes if fully materialized) placed under an
undeclared key — or under `description` itself, since deserializing a
sequence into `Option<String>` short-circuits on an immediate type
mismatch — is discarded by serde's derived visitor today without expanding
nested aliases, so parsing stays cheap. Retyping either field to a
buffering shape (`serde_norway::Value`, an untagged enum, or a buffering
`#[serde(deserialize_with)]`) would force alias expansion before per-field
routing and trip `serde_norway`'s own repetition-limit guard instead —
still bounded, but no longer free. This is a currently-true property of
`RawFrontmatter`'s field shape, not a designed defense in its own right;
see [[../decisions/ADR-341-serde-saphyr-vs-serde-norway]] for the
evaluation this characteristic factored into.

## 8. `resolve_skill_output_path` — Path Confinement

Confines `save_skill`'s optional `output_path` to `base_dir/server_id`
(issue #184's fix — without this, an absolute path, `..`, or a
symlink-planted-inside-`base_dir` path could write anywhere the process can
reach):

- `server_id` and `output_path` walk the **same** checked, one-component-
  at-a-time loop, rooted at a once-canonicalized `base_dir`.
- `server_id`'s own directory is checked **first** and rejected outright if
  it already exists as a symlink, regardless of where it points — including
  at a *sibling* server's own directory, which would otherwise pass a
  plain resolve-and-confine check since it still resolves under
  `base_dir` (issue #217's fix).
- Every subsequent directory component (up to but not including the final
  file) is confinement-checked and created eagerly; a pre-existing symlink
  there is followed only if it still resolves inside `server_id`'s
  directory.
- The final path component is rejected outright if it exists as a symlink,
  **dangling or not** — a dangling symlink makes `canonicalize` fail, which
  must not be treated as "safe, doesn't exist yet" (a subsequent write
  would still follow it).
- This is a check against **pre-existing** state at call time — not a
  concurrency guarantee against a symlink planted by a racing process
  between this check and the caller's write.

## 9. Error Conditions

`ScanError`: `Io`, `DirectoryNotFound`, `MissingMetadata`,
`MetadataParse`, `UnsupportedSchema`, `TooManyFiles`, `FileTooLarge`,
`StaleMetadata`.

`SkillMetadataError`: `MissingFrontmatter`, `FrontmatterTooLarge`,
`InvalidYaml`, `MissingField`.

`OutputPathError`: `InvalidServerId`, `AbsolutePath`, `ParentTraversal`,
`InvalidPath`, `ServerIdIsSymlink`, `Escape`, `NotADirectory`, `NotAFile`,
`CreateDir`, `Io`.

`SkillServerIdError`: `Empty`, `TooLong { len, limit }`, `InvalidCharacters`.

## 10. Cross-Crate Contracts

- **Consumes** `mcp-core`: `metadata::{METADATA_FILE_NAME,
  METADATA_SCHEMA_VERSION, ServerMetadata}`, `sanitize_path_for_error`,
  `validate_path_segment`, `untrusted::*`.
- **Used by** `mcp-cli skill` (directly calls `scan_tools_directory` +
  `build_skill_context` + `render_skill_md`, no LLM/prompt step — see
  [[../cli/spec#skill]]).
- **Used by** `mcp-server`'s `generate_skill`/`save_skill` tools (returns
  the LLM-facing prompt via `generate_skill`, writes Claude's composed
  content via `save_skill` — see [[../server/spec#generate_skill]] and
  [[../server/spec#save_skill]]).
- **Schema shape checks**: `types.rs`'s own tests assert the `schemars`-
  derived JSON Schema for `GenerateSkillParams`/`SaveSkillParams` declares
  bounds matching the real runtime constants (`MAX_SERVER_ID_LENGTH`) — a
  drift-guard, since `schemars` attributes cannot reference a Rust `const`
  directly and must mirror it as a literal.

## 11. Edge Cases & Notable Behaviors

- `SaveSkillParams::content`'s declared JSON-Schema `maxLength` (102,400)
  and the runtime byte cap it mirrors (`MAX_SKILL_CONTENT_SIZE`, owned by
  `mcp-server::service`) only coincide exactly for ASCII input — JSON
  Schema's `maxLength` counts Unicode code points, not bytes, so the
  runtime byte check can reject legitimate multi-byte UTF-8 content the
  declared schema would still accept (never the reverse).
- A category string itself can carry an injected heading
  (`"issues\n### Injected Heading"`) — sanitized the same way as
  description text, verified by a dedicated regression test distinct from
  the tool-description case.

## 12. See Also

- [[../core/spec]] — shared path-confinement and untrusted-text primitives
- [[../server/spec]] — MCP-tool-level `generate_skill`/`save_skill` wrappers
- [[../cli/spec#skill]] — CLI-level direct SKILL.md rendering
