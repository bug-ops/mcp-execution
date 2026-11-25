---
aliases:
  - Project Constitution
  - mcp-execution Constitution
tags:
  - sdd
  - constitution
created: 2026-07-27
status: permanent
related:
  - "[[README]]"
---

# Project Constitution (Reverse-Engineered)

> [!important]
> This constitution was **derived from reading the actual codebase**, not authored
> up front. It documents the principles the existing code already enforces —
> consistently, across every crate — so future specs and changes stay
> compatible with the patterns already in place. Treat it as a description of
> observed reality, not an aspirational document.

## I. Architecture

- **Layered workspace, strict dependency direction (low → high)**:
  `mcp-core` → `mcp-introspector` → `mcp-codegen` → `mcp-files` → `mcp-skill` →
  `mcp-server`/`mcp-cli`. Higher crates depend on lower ones; lower crates never
  depend on higher ones (`mcp-core` has zero intra-workspace dependencies).
- **Strong types over primitives** (`ServerId`, `ToolName`, `FilePath`,
  `ServerConnectionString`, `ExitCode`) — never a raw `String`/`i32` for a
  domain concept. See [[core/spec#ServerId and ToolName|core spec]].
- **Builder + validate-at-construction**: types that require invariants
  (`ServerConfig`) are only constructible through a builder whose `build()`
  runs full validation, so an invalid instance cannot exist via the sanctioned
  path (though `pub` fields + `Deserialize` mean a struct literal or
  `serde_json::from_str` can still bypass it — every consumer that might
  receive such a value re-validates defensively; see
  [[core/spec#Defense in depth|core spec]]).
- **In-memory VFS before real I/O**: codegen output is staged as
  `GeneratedCode` → `FileSystem` (in-memory) → exported to disk only at the
  end, via a staged-directory-then-atomic-rename pattern. See
  [[files/spec]].

## II. Technology Stack

- Language: Rust, edition 2024, MSRV 1.98 (`Cargo.toml` `workspace.package`; rolling policy: stable minus two, raised in a minor release).
- Async runtime: `tokio` throughout.
- MCP protocol: `rmcp` (official Rust SDK), both as client (`mcp-introspector`)
  and server (`mcp-server`).
- Templating: `handlebars`, HTML-escaping disabled (`no_escape`) because output
  is TypeScript/JSDoc, not HTML — injection safety is handled upstream by
  hand-written sanitizers instead.
- Schema/validation: `schemars` for JSON-Schema-derived MCP tool parameter
  schemas; `serde`/`serde_json` for wire types; `serde-saphyr` for YAML
  (SKILL.md frontmatter) — never `serde_yaml`/`serde_yml`/`serde_norway`. See
  §V's YAML parse-time bound for the pre-parse cap and explicit parse
  `Budget` this parser requires (see
  [[decisions/ADR-405-adopt-serde-saphyr]]).
- Error handling: `thiserror` in every library crate; `anyhow` only in
  `mcp-execution-cli`.
- CLI: `clap` (derive API) + `clap_complete`.

## III. Testing (NON-NEGOTIABLE)

- Every crate carries an extensive `#[cfg(test)] mod tests` in the same file
  as the code under test, plus `tests/` integration suites for
  cross-component behavior (process spawning, filesystem export, HTTP
  transport).
- `cargo nextest run` is the canonical runner (CI-enforced), not `cargo test`.
- Doc comments on every `pub` item include a runnable `# Examples` section
  (`cargo test --doc`), which doubles as executable API documentation.
- Regressions are tracked by issue number in comments and test names (e.g.
  `test_build_and_export_rejects_empty_top_level_component` cites the exact
  bug it guards against) — the test suite is also a changelog of past
  security/correctness incidents.
- Fakeable time: `mcp-server` injects a `Clock` trait (`SystemClock` in
  production, `TestClock` in tests) rather than calling `Utc::now()`
  directly, so expiry logic is deterministically testable.

## IV. Code Style

- Clippy `all`, `cargo`, `nursery`, `pedantic` are `deny` at workspace level.
  The only surviving `#[allow(...)]` **attribute** suppressions are the two
  crate-level ones in `mcp-cli` (`clippy::unused_async`, for handlers
  uniformly dispatched through a single async entry point;
  `clippy::needless_collect`, for a test-readability `collect()` that isn't
  reused as an iterator afterward) — deliberately out of scope for the
  `#[expect]` migration below, which covers item-level suppressions only.
  Separately, the root `Cargo.toml`'s `[workspace.lints.clippy]` table pins
  three lint IDs to `allow` at the TOML level rather than via an attribute —
  `cargo_common_metadata`, `multiple_crate_versions`,
  `needless_borrows_for_generic_args` — each with an inline rationale
  comment (issue #442). This is a workspace-wide policy decision, not a
  per-site suppression, so it is exempt from the `#[expect]`-over-`#[allow]`
  convention below, which applies to attribute-level suppressions in source
  files.
- Convention: every item-level lint suppression uses
  `#[expect(lint, reason = "...")]` rather than `#[allow(lint)]` — `#[expect]`
  emits its own warning if the lint stops firing, which surfaces a
  suppression that has outlived its justification instead of letting it go
  stale silently. `too_many_lines`, suppressed item-level on two
  concurrency-test functions whose length is inherent to the scenario they
  cover, was the first application of this convention (issue #459); every
  other item-level `#[allow(...)]` site in the workspace has since been
  converted the same way (issue #465), except one
  (`mcp-cli/src/commands/skill.rs` `too_many_arguments`) that was removed
  outright rather than converted — the function has exactly 7 parameters, at
  clippy's default `too-many-arguments-threshold` (the lint fires only above
  it), so the suppression no longer suppressed anything. Under CI's
  `-D warnings`, an unfulfilled `#[expect]` (the lint no longer fires) is a
  hard build failure, not a warning — this is the intended mechanism, but it
  means a routine simplification that brings a function back under a
  threshold like `too_many_lines` breaks the build until the now-unfulfilled
  `#[expect]` is deleted; do not pad a function back over the threshold just
  to keep the attribute satisfied.
- `unsafe_code = "deny"` workspace-wide (`[workspace.lints.rust]`); the only
  exceptions are `#[expect(unsafe_code)]` on test modules that wrap edition-2024
  `env::set_var`. `clippy::unwrap_used`/`expect_used` are denied outside tests
  (`clippy.toml` exempts `#[cfg(test)]` code; integration tests, benches and
  examples carry a file-level `#![expect]`), and `.cargo/config.toml` sets
  `build.warnings = "deny"` (escape hatch: `--config build.warnings=warn`).
- `#![warn(missing_docs, missing_debug_implementations)]` everywhere: every
  `pub` item is documented, every `pub` type derives or hand-implements
  `Debug`.
- Every `pub` type and function must be `Send + Sync` where it can reasonably
  be (explicitly tested via `assert_send`/`assert_sync` helpers in several
  crates).

## V. Security (the dominant concern of this codebase)

This is not a generic principle here — it is the single most heavily invested
area of the entire project, evidenced by the volume of doc comments, tests,
and issue-number references dedicated to it.

- **Command-injection defense in depth**: `mcp-core::command` validates
  shell metacharacters, forbidden environment variables (dynamic-linker and
  interpreter hijack vectors), URL schemes, and HTTP header safety —
  enforced once, in Rust (`validate_server_config`), on the one connection
  path (`mcp-introspector::Introspector::connect`). The generated TypeScript
  runtime bridge no longer holds a mirrored copy: it is a thin client of the
  Rust helper and renders only the helper-protocol constants of
  `mcp-core::helper_protocol` (ADR-569, option B).
- **Resource-exhaustion (CWE-400) bounds at every layer**: tool count, tool
  name/description length, schema size, generated file count/bytes, VFS
  export file count/bytes, pending-session count/aggregate bytes, request
  line size, request concurrency — each layer derives its bound from the
  layer below it rather than choosing an independent number, specifically so
  that data which already cleared a lower layer's bound can never be
  rejected by a higher layer for merely being "as large as already allowed."
- **YAML parse-time bound (deliberate exceptions to the rule above)**:
  `MAX_FRONTMATTER_SIZE` (`crates/mcp-skill/src/parser.rs`) caps the
  extracted `SKILL.md` frontmatter block at 8 KiB *before* handing it to
  `serde-saphyr`. Unlike the resource-exhaustion bounds above, this cap is
  intentionally independent of the enclosing `SKILL.md` size limit rather
  than derived from it: YAML parsers are not inherently linear-time on
  pathologically nested or aliased input, so a much larger document-size
  bound would not itself bound parse latency. Any future YAML parse entry
  point must apply this same kind of independent, pre-parse cap to the exact
  slice it hands to its parser. This size cap is now paired with an explicit
  parse `Budget` (`frontmatter_options` in `crates/mcp-skill/src/parser.rs`;
  see [[decisions/ADR-405-adopt-serde-saphyr]]) as a second, parser-level
  bound. A second deliberate exception to the derived-bound rule lives
  inside that budget: `max_depth: 64` is not derived from the 8 KiB cap the
  way the budget's other fields are — 8 KiB of deeply nested flow sequences
  can nest thousands of levels deep, so no size-derived depth value would be
  meaningful.
- **Prompt-injection / Markdown-injection defense**: any text that
  originates from an introspected (untrusted) MCP server and is later shown
  to an LLM or embedded in a document (tool names, descriptions, keywords,
  categories) is sanitized (`mcp_execution_core::untrusted`) and, where it
  reaches an LLM-facing prompt, wrapped in an explicit
  `<untrusted-data>...</untrusted-data>` boundary that cannot be forged from
  within.
- **No untrusted source echo in error text**: error messages derived from
  parsing or processing untrusted input (YAML, JSON, protocol-specific
  formats) that reach an LLM or client must not verbatim reproduce source
  excerpts — only the location of the error and the kind of failure are safe
  to surface. This applies to every parsing dependency and entry point,
  independent of which specific parser or format is in use — any future
  parser swap or new parsing path must validate its error rendering against
  this principle up front. See ADR-341 (section 3.5) for the concrete case
  that motivated this rule.
- **Debug-redaction of secrets**: any type carrying environment variables,
  HTTP headers, CLI args, or URLs implements `Debug` by hand (never
  `#[derive(Debug)]`) to redact values while keeping keys — because these
  routinely carry bearer tokens and API keys, and get printed by `tracing`,
  `anyhow`, or `eprintln!`. `Serialize` deliberately does **not** redact
  (round-tripping a config store requires the real values); this asymmetry
  is documented, not accidental.
- **Path confinement**: every filesystem write derived from caller/attacker
  input (`save_skill`'s `output_path`, `introspect_server`'s `output_dir`,
  `list_generated_servers`'s `base_dir`) is confined to a base directory with
  symlink-aware, component-by-component validation — not a single
  `canonicalize`-then-`starts_with` check, because that alone misses a
  symlink planted at the exact confinement boundary.
- **Atomic, non-destructive filesystem export**: every write to
  `~/.claude/servers/...` / `~/.claude/skills/...` stages content in a
  sibling temp directory and publishes via rename, so a killed process never
  leaves a half-written tree, and a failed export never touches a
  previously-published one.

## VI. Performance

- Progressive loading's entire raison d'être is a **token-budget
  performance goal**: one file per tool so Claude Code loads only what it
  needs (~500-1,500 tokens/tool vs. ~30,000 tokens for the whole tool set).
- `FileSystem::export_to_filesystem` targets <50ms for a 30-file export
  (documented target in `crates/mcp-files/src/filesystem.rs`), achieved via
  single-pass directory pre-creation and an optional `rayon`-parallel write
  path (`parallel` feature).
- `criterion` benchmarks exist for codegen (`mcp-codegen/benches`) and VFS
  export (`mcp-files/benches`), plus a `dhat`-based memory-profiling example
  in `mcp-files/examples/profile_memory.rs`.

## VII. Simplicity

- No dependency on an LLM API for tool categorization: `mcp-server`'s
  `introspect_server`/`save_categorized_tools` split exists specifically so
  the *calling* Claude Code session does the categorization via natural
  language, not a second, separately-billed LLM call.
- Generated output is a self-contained package (own `package.json`,
  `tsconfig.json`) meant to be run via `tsx`/`deno`/Node's native TS
  stripping — not merged into a consumer's own TypeScript build.

## VIII. Git Workflow

- Conventional Commits (see `.claude/rules/commits-and-issues.md` — not part
  of this reverse-engineered constitution, but the enclosing repo's own
  convention).
- CHANGELOG.md maintained under `[Unreleased]` until a version is cut.

---

## Adopted Direction (2026-11-24), observed in code

> [!success] Observed
> The principles below were adopted by the maintainer on 2026-11-24 and are now
> observed in the code of the script-first work (#569). They are kept in this
> section so their origin stays visible.

- **The context budget is a first-class resource.** What a model must read
  (tool definitions, results, skill text) is designed, bounded and measured
  like memory or disk, not left to whatever a tool prints. Observed: the
  runner's output budget and spill, the bounded skill listing, and the `stats`
  command with its harness (NFR-PERF-003).
- **Documentation figures are reproducible or labelled as illustrations.**
  A number in user-facing documentation comes from a command a reader can
  rerun, or says it is an illustration and names its source.
- **One MCP client.** Every process that calls a tool does so through
  `mcp-introspector::Introspector::connect` and `Session::call_tool`, which
  re-validate the configuration and bound responses; no second client or copy
  of the validation is added (ADR-569, accepted 2026-11-24).
- **Diagnostics are not boundaries.** The call journal and the dry run help an
  honest script and a model that reads the output; they do not resist a script
  written to evade them, and no document may call them tamper-evident or the
  runner sandboxed. Runner state (spill, journals, the journal key, run
  directories) lives in a per-user, system-derived location with owner-only
  modes and is pruned under the same confinement as it is created.

## See Also

- [[README]] — cross-block index and end-to-end data flow
- [[core/spec]], [[introspector/spec]], [[codegen/spec]], [[files/spec]],
  [[skill/spec]], [[server/spec]], [[cli/spec]]
