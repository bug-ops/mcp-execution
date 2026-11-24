//! Constants shared by the helper protocol's two ends: the Rust runner and the generated
//! TypeScript bridge.
//!
//! The protocol is newline-delimited JSON over a per-run channel. Both ends read these values
//! from here (the bridge template renders them), so a limit cannot drift between languages.

use std::time::Duration;

/// Wire protocol version sent in the hello line and checked by both ends.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::HELPER_PROTOCOL_VERSION;
///
/// assert_eq!(HELPER_PROTOCOL_VERSION, 1);
/// ```
pub const HELPER_PROTOCOL_VERSION: u32 = 1;

/// Maximum size, in bytes, of one JSON-RPC response line accepted from an MCP server over stdio.
///
/// Also the largest tool result the helper forwards to a script, before the envelope slack.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::MAX_RESPONSE_LINE_BYTES;
///
/// assert_eq!(MAX_RESPONSE_LINE_BYTES, 4 * 1024 * 1024);
/// ```
pub const MAX_RESPONSE_LINE_BYTES: usize = 4 * 1024 * 1024;

/// Bytes reserved for the helper's own response envelope (id, kind, messages) on top of a
/// maximal tool result.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::HELPER_ENVELOPE_SLACK;
///
/// assert_eq!(HELPER_ENVELOPE_SLACK, 64 * 1024);
/// ```
pub const HELPER_ENVELOPE_SLACK: usize = 64 * 1024;

/// Maximum size, in bytes, of one helper protocol line in either direction.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::{
///     HELPER_ENVELOPE_SLACK, MAX_HELPER_LINE_BYTES, MAX_RESPONSE_LINE_BYTES,
/// };
///
/// assert_eq!(MAX_HELPER_LINE_BYTES, MAX_RESPONSE_LINE_BYTES + HELPER_ENVELOPE_SLACK);
/// ```
pub const MAX_HELPER_LINE_BYTES: usize = MAX_RESPONSE_LINE_BYTES + HELPER_ENVELOPE_SLACK;

/// How long the helper waits for the client's hello line, measured from the accepted connect.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::HELPER_HELLO_TIMEOUT;
///
/// assert_eq!(HELPER_HELLO_TIMEOUT.as_secs(), 5);
/// ```
pub const HELPER_HELLO_TIMEOUT: Duration = Duration::from_secs(5);

/// Environment variable naming the helper channel for the script process (`fd:0` on Unix,
/// `pipe:<name>` on Windows). Internal to the runner; cleared from the script's own environment.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::HELPER_ENV;
///
/// assert!(HELPER_ENV.starts_with("MCP_EXECUTION_"));
/// ```
pub const HELPER_ENV: &str = "MCP_EXECUTION_HELPER";

/// Environment variable carrying the per-run token the Windows named-pipe client must present.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::{HELPER_ENV, HELPER_TOKEN_ENV};
///
/// assert!(HELPER_TOKEN_ENV.starts_with(HELPER_ENV));
/// ```
pub const HELPER_TOKEN_ENV: &str = "MCP_EXECUTION_HELPER_TOKEN";

/// Public environment variable holding the absolute path of the `mcp-execution-cli` binary the
/// bridge spawns as its helper when no runner channel exists.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::CLI_PATH_ENV;
///
/// assert_eq!(CLI_PATH_ENV, "MCP_EXECUTION_CLI");
/// ```
pub const CLI_PATH_ENV: &str = "MCP_EXECUTION_CLI";

/// Key, for `Symbol.for`, under which the `run` preload registers the claimed helper channel on
/// `globalThis`; the generated bridge takes the channel from there.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::HELPER_CHANNEL_KEY;
///
/// assert_eq!(HELPER_CHANNEL_KEY, "mcp-execution.helper-channel");
/// ```
pub const HELPER_CHANNEL_KEY: &str = "mcp-execution.helper-channel";

/// Marker the runner sets, with value `1`, on the whole process tree of a script under `run`.
///
/// Non-secret. A bridge that sees it but has no registered helper channel (a worker thread, or a
/// child process of the script) must refuse to spawn a helper of its own: that helper would have
/// no journal and no dry-run policy. It never stands in for the channel.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::helper_protocol::{HELPER_ENV, RUN_MARKER_ENV};
///
/// assert_ne!(RUN_MARKER_ENV, HELPER_ENV);
/// ```
pub const RUN_MARKER_ENV: &str = "MCP_EXECUTION_RUN";
