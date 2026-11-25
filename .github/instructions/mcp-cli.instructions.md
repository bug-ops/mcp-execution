---
applyTo: "crates/mcp-cli/**/*.rs"
---

# Copilot Instructions: mcp-cli

This crate is the **command-line interface** for MCP Execution. It is the **only crate** in the workspace that uses `anyhow` for error handling.

## Layout

- `src/main.rs` - parses `Cli`, initializes logging, runs `runner::execute_command`, exits with its `ExitCode`
- `src/cli.rs` - the `clap` derive definitions (`Cli`, `Commands`) and their help text
- `src/runner.rs` - logging setup and command dispatch
- `src/commands/` - one module per subcommand
- `src/formatters.rs` - the shared output formatting (`format_output`, `emit`)

The subcommand list changes; read `src/cli.rs` for the current set instead of relying on a copy here.

## Error Handling - CRITICAL

Use `anyhow::Result` with `.context(...)` on fallible calls and `bail!` for early failures. Do not use `thiserror` in this crate. Make errors descriptive and actionable for end users.

## CLI Patterns

- Define arguments with `clap` derive macros in `src/cli.rs`; add a subcommand there and a module in `src/commands/`.
- Route output through `src/formatters.rs` rather than printing ad hoc, so server-controlled text is escaped in one place.
- `--format` is typed as `OutputFormat` from `mcp-execution-core`; do not re-parse it from strings.
- Use the shared `ExitCode` from `mcp-execution-core` for process exit status.

## Library Dependencies

Library crates return their own typed errors; convert them to `anyhow` with `.context(...)` at the boundary. Build servers from a validated `ServerConfig`, never from a raw command string.

## Testing

- Unit tests live in the same file under `#[cfg(test)]`; integration tests in `crates/mcp-cli/tests/`.
- Async tests use `#[tokio::test]`.
- Tests may return `anyhow::Result`.

## Key Differences from Library Crates

| Aspect | mcp-cli (Application) | Other Crates (Libraries) |
|--------|----------------------|--------------------------|
| Error type | `anyhow::Error` | `thiserror::Error` |
| Result alias | `anyhow::Result<T>` | `crate::Result<T>` |
| Error creation | `bail!()`, `.context()` | Custom enum variants |
| Panics | Acceptable in main.rs | Never in library code |
