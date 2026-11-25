---
applyTo: "crates/mcp-codegen/**/*.rs"
---

# Copilot Instructions: mcp-execution-codegen

This crate generates TypeScript from MCP tool schemas using Handlebars templates: one file per tool, a type-only `catalog.ts`, an `index.ts`, the runtime bridge (`_runtime/mcp-bridge.ts`, `_runtime/cli.ts`), `package.json`, `tsconfig.json` and a `_meta.json` sidecar.

## Layout

- `package/` - `PackageGenerator` and the generated-file model
- `common/` - shared types and the JSON Schema to TypeScript converter (`common::ts_types::convert_schema`)
- `template_engine.rs` - `TemplateEngine`, a thin Handlebars wrapper
- `templates/package/*.hbs` - the TypeScript templates

## Error Handling

Use `thiserror`; never `anyhow` in this crate. Template and render failures are typed errors, not panics.

## Security - CRITICAL

Every string that originates from an MCP server (tool names and descriptions, schema descriptions, server name and version, categorization fields) is untrusted. Sanitize it before it is interpolated into generated TypeScript (JSDoc terminators, CR/LF, length caps), and never build TypeScript by string concatenation outside the templates.

## Type Conversion

Parameter types, result types and the catalog go through the single converter `common::ts_types::convert_schema`. Do not add a second schema-to-type path; extend the converter and its tests.

## Templates

- Keep logic in Rust; templates only render prepared data.
- Handlebars HTML escaping is not a TypeScript escaping mechanism; use the crate's sanitizers and literal helpers.
- A template change needs a generation test that asserts on the rendered text.

## Testing

- Unit tests live in the same file under `#[cfg(test)]`; integration tests in `tests/`.
- Assert on generated text for every template change; generated output must stay valid TypeScript.
- Benchmarks: `cargo bench --profile bench-fast -p mcp-execution-codegen`.
