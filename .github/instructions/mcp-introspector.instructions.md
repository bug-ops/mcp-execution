---
applyTo: "crates/mcp-introspector/**/*.rs"
---
# Copilot Instructions: mcp-execution-introspector

This crate connects to MCP servers through the official `rmcp` SDK and discovers their tools. The `rmcp` version is pinned in the workspace `Cargo.toml` and `Cargo.lock`; check the pinned version on docs.rs before calling any `rmcp` API.

## Layout

- `src/lib.rs` - `Introspector`, `ServerInfo`, `ToolInfo`, `Discovery`, `ListingHints`, `CacheScope` and the bounded connect and listing logic
- `src/session.rs` - `Session` for repeated tool calls on one connection, with `CallTimeout`, `ToolCall` and `ToolCallOutcome`

The runnable usage examples are the doctests on `Introspector::discover_server` and `Introspector::discover`; copy from those, not from memory.

## Error Handling

Use `thiserror`; never `anyhow` in this crate. Connection, timeout and protocol failures are typed errors from `mcp_execution_core::Error`, not panics.

## rmcp Usage

- Use `rmcp` for all MCP communication; never implement JSON-RPC for MCP by hand.
- Connect through the existing bounded helpers in `src/lib.rs` (`connect_bounded` and `stdio_halves`) instead of adding a second connection path.
- A `ServerConfig` is validated at construction; spawn processes only from a `ServerConfig`, never from a raw command string.

## Security - CRITICAL

Every string a server returns (tool names, descriptions, schemas, server name and version) is untrusted. Sanitize it with the helpers in `mcp_execution_core::untrusted` before it is displayed or interpolated, and never log it raw.

## Timeouts and Bounds

Every network operation has a timeout, and tool listings are capped by `MAX_TOOL_COUNT`. Do not add an unbounded wait or an unbounded pagination loop.

## Testing

- Unit tests live in the same file under `#[cfg(test)]`; integration tests in `tests/`.
- Test against in-process rmcp mock servers, never against external servers.
- Async tests use `#[tokio::test]`.
