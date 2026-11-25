---
applyTo: "crates/mcp-core/**/*.rs"
---

# Copilot Instructions: mcp-execution-core

This crate is the **foundation** of the workspace: validated types, the shared error type, `ServerConfig` security validation and the pure helpers (sanitization, confinement, redaction) used by every other crate.

## Layout

- `src/types.rs` - `ServerId`, `ToolName` and their error types
- `src/error.rs` - the workspace `Error` and `Result`
- `src/server_config.rs` - `ServerConfig`, `ServerConfigBuilder` and spawn-time validation
- `src/metadata.rs` - `_meta.json` types and the only filesystem reads here, `read_server_metadata` and `read_server_metadata_async`
- `src/log_filter.rs` - shared `tracing` filter helpers, behind the `tracing-filter` feature
- `src/untrusted.rs` - sanitizers for server-controlled text
- `src/cli.rs` - shared CLI value types (`OutputFormat`, `ExitCode`, `LogFormat`, `ServerConnectionString`)
- `src/confinement.rs`, `src/path.rs`, `src/redact.rs`, `src/provenance.rs`, `src/annotations.rs`, `src/command.rs`, `src/helper_protocol.rs` - other shared contracts

Runnable examples are the doctests on these items; copy from them, not from memory.

## Error Handling - CRITICAL

Only `thiserror`. This is a library crate; never `anyhow`. Use `mcp_execution_core::Error` and `Result` from `src/error.rs` instead of adding a parallel error type.

## Strong Types - CRITICAL

Use validated types instead of primitives:

- `ServerId` and `ToolName` wrap a `String`; their constructors validate and return a `Result` (`ServerId::new`, `ToolName::new`), and `Deserialize` goes through the same constructor.
- Never accept `String` or `&str` where `ServerId`, `ToolName` or `ServerConfig` is expected.
- A new identifier-like type gets a validating constructor, a typed error, and doctests showing both the accepted and the rejected input.

## Send + Sync + Debug

All public types implement `Send + Sync + Debug`.

## No Business Logic

This crate holds types, validation, pure helpers, the `_meta.json` reader and the feature-gated log filter. Anything that connects to a server, generates code or does other filesystem work beyond the confinement helpers and the metadata reader belongs in a higher-level crate.

## Documentation

Every public item has a `///` doc comment that explains what and why, and non-trivial items carry a runnable `# Examples` doctest. Run `cargo test --doc -p mcp-execution-core` to verify.

## Testing

- Unit tests live in the same file under `#[cfg(test)]`; integration tests in `tests/`.
- Cover both accepted and rejected input for every validating constructor.
