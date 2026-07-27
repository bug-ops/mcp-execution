//! Test fixture: an executable that spawns successfully but exits immediately
//! without ever behaving as an MCP server, closing its stdout pipe before
//! answering the `initialize` handshake.
//!
//! Used by `tests/stdio_connect_failure_test.rs` (issue #332) to exercise
//! `connect_and_list_tools`'s non-timeout `Error::ConnectionFailed` mapping via
//! the stdio path: `test_discover_server_nonexistent_command` in
//! `tests/integration_test.rs` fails during process *spawn* itself (the
//! command does not resolve on `PATH`), so it never reaches
//! `connect_and_list_tools`. This fixture is a real, spawnable executable, so
//! the failure instead surfaces once the client tries to read the handshake
//! response from the (immediately closed) stdout pipe.

const fn main() {}
