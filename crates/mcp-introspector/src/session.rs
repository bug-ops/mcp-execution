//! A live connection to one MCP server, used to call its tools.
//!
//! [`Introspector::connect`] opens a [`Session`] with the same validation, transports and
//! bounds as discovery; [`Session::call_tool`] then performs one call and classifies the reply
//! into a [`ToolCallOutcome`] with the FR-030 value semantics.

use std::time::Duration;

use mcp_execution_core::helper_protocol::{MAX_RESPONSE_LINE_BYTES, ResultSource};
use mcp_execution_core::untrusted::{MAX_UNTRUSTED_FIELD_LEN, sanitize_untrusted_text};
use mcp_execution_core::{
    Error, ResourceKind, Result, ServerConfig, ServerId, ToolName, Transport,
    validate_server_config,
};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use rmcp::RoleClient;
use rmcp::ServiceExt;
use rmcp::model::{CallToolRequestParams, CallToolResponse, CallToolResult};
use rmcp::service::{RunningService, ServiceError};
use tokio::process::Child;

use crate::{
    Introspector, connect_bounded, connect_http_client, spawn_introspection_child, stdio_halves,
    take_child_stdio,
};

/// Longest wait for the protocol layer to wind down in [`Session::close`].
const CLOSE_GRACE: Duration = Duration::from_secs(2);

/// Per-call timeout, validated to `1..=600` seconds.
///
/// A zero timeout would fail every call and an unbounded one would let a hung server block the
/// caller forever, so neither is representable.
///
/// # Examples
///
/// ```
/// use std::time::Duration;
/// use mcp_execution_introspector::CallTimeout;
///
/// assert!(CallTimeout::new(Duration::from_secs(60)).is_ok());
/// assert!(CallTimeout::new(Duration::ZERO).is_err());
/// assert!(CallTimeout::new(Duration::from_secs(601)).is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallTimeout(Duration);

impl CallTimeout {
    /// Shortest accepted timeout.
    pub const MIN: Duration = Duration::from_secs(1);
    /// Longest accepted timeout.
    pub const MAX: Duration = Duration::from_secs(600);

    /// Validates `timeout` against [`MIN`](Self::MIN) and [`MAX`](Self::MAX).
    ///
    /// # Errors
    ///
    /// Returns [`Error::InvalidArgument`] when `timeout` is outside the accepted range.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use mcp_execution_introspector::CallTimeout;
    ///
    /// let timeout = CallTimeout::new(Duration::from_secs(5)).unwrap();
    /// assert_eq!(timeout.get(), Duration::from_secs(5));
    /// ```
    pub fn new(timeout: Duration) -> Result<Self> {
        if (Self::MIN..=Self::MAX).contains(&timeout) {
            Ok(Self(timeout))
        } else {
            Err(Error::InvalidArgument(format!(
                "call timeout must be between {} and {} seconds",
                Self::MIN.as_secs(),
                Self::MAX.as_secs()
            )))
        }
    }

    /// Returns the validated duration.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::time::Duration;
    /// use mcp_execution_introspector::CallTimeout;
    ///
    /// assert_eq!(CallTimeout::new(Duration::from_secs(2)).unwrap().get().as_secs(), 2);
    /// ```
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

/// One tool invocation.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::ToolName;
/// use mcp_execution_core::helper_protocol::ResultSource;
/// use mcp_execution_introspector::ToolCall;
///
/// let tool = ToolName::new("echo").unwrap();
/// let call = ToolCall {
///     tool: &tool,
///     arguments: serde_json::Map::new(),
///     result_source: ResultSource::Content,
/// };
/// assert_eq!(call.tool.as_str(), "echo");
/// ```
#[derive(Debug, Clone)]
pub struct ToolCall<'a> {
    /// Tool to call.
    pub tool: &'a ToolName,
    /// Arguments; an object by type, never another JSON kind.
    pub arguments: serde_json::Map<String, serde_json::Value>,
    /// Where the result value is read from.
    pub result_source: ResultSource,
}

/// Server-supplied text, sanitized and capped.
///
/// # Examples
///
/// ```
/// use mcp_execution_introspector::ServerText;
///
/// let text = ServerText { text: "boom".to_string(), truncated: false };
/// assert!(!text.truncated);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerText {
    /// Sanitized text.
    pub text: String,
    /// Whether the original was longer than the cap.
    pub truncated: bool,
}

/// How a tool call ended, when the call itself completed.
///
/// Transport, timeout and limit failures are not outcomes: they are `Err` from
/// [`Session::call_tool`].
///
/// # Examples
///
/// ```
/// use std::assert_matches;
/// use mcp_execution_introspector::ToolCallOutcome;
///
/// let outcome = ToolCallOutcome::Value(serde_json::json!({"ok": true}));
/// assert_matches!(outcome, ToolCallOutcome::Value(_));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolCallOutcome {
    /// The extracted result value.
    Value(serde_json::Value),
    /// The call succeeded and returned no result.
    Empty,
    /// The tool reported an error (`isError`).
    ToolError {
        /// Sanitized server text, if any.
        server_text: Option<ServerText>,
    },
    /// The server answered with a JSON-RPC error.
    RpcError {
        /// JSON-RPC error code, if it carried one.
        code: Option<i32>,
        /// Sanitized server text, if any.
        server_text: Option<ServerText>,
    },
}

/// A live connection to one MCP server.
///
/// Opened by [`Introspector::connect`]. A stdio session owns its child process; [`close`](Self::close)
/// kills it and `Drop` is the backstop.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_core::{ServerConfig, ServerId};
/// use mcp_execution_introspector::Introspector;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let config = ServerConfig::builder().command("server".to_string()).build()?;
/// let session = Introspector::new()
///     .connect(ServerId::new("demo")?, &config)
///     .await?;
/// println!("connected to {}", session.server_id());
/// session.close().await;
/// # Ok(())
/// # }
/// ```
pub struct Session {
    server_id: ServerId,
    client: RunningService<RoleClient, ()>,
    // `kill_on_drop` is set at spawn, so dropping a session without `close` still kills it.
    child: Option<Child>,
    /// Raised when a stdio server sent a response line over the size cap and the session was
    /// closed because of it.
    oversized: Option<Arc<AtomicBool>>,
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Session")
            .field("server_id", &self.server_id)
            .field("owns_child", &self.child.is_some())
            .finish_non_exhaustive()
    }
}

impl Session {
    /// Returns the id this session was opened for.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_introspector::Session;
    /// # fn example(session: &Session) {
    /// println!("{}", session.server_id());
    /// # }
    /// ```
    #[must_use]
    pub const fn server_id(&self) -> &ServerId {
        &self.server_id
    }

    /// Calls one tool and classifies the reply.
    ///
    /// A timeout returns an error and leaves the session usable. Server text in an outcome is
    /// sanitized. Over plain HTTP the JSON body is bounded only by `timeout`. A reply that asks
    /// the client for further input (a multi-round-trip or task reply) cannot be served by this
    /// client and is a [`Error::ConnectionFailed`].
    ///
    /// # Errors
    ///
    /// Returns [`Error::Timeout`] on timeout and [`Error::ConnectionFailed`] on transport
    /// failure or an unsupported reply kind, and [`Error::ResourceLimitExceeded`] when the server
    /// sent a response line over the size cap (see [`is_poisoned`](Self::is_poisoned)).
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_core::ToolName;
    /// use mcp_execution_core::helper_protocol::ResultSource;
    /// use mcp_execution_introspector::{CallTimeout, Session, ToolCall};
    /// use std::time::Duration;
    ///
    /// # async fn example(session: &Session) -> Result<(), Box<dyn std::error::Error>> {
    /// let tool = ToolName::new("echo")?;
    /// let call = ToolCall {
    ///     tool: &tool,
    ///     arguments: serde_json::Map::new(),
    ///     result_source: ResultSource::Content,
    /// };
    /// let outcome = session.call_tool(call, CallTimeout::new(Duration::from_secs(30))?).await?;
    /// println!("{outcome:?}");
    /// # Ok(())
    /// # }
    /// ```
    pub async fn call_tool(
        &self,
        call: ToolCall<'_>,
        timeout: CallTimeout,
    ) -> Result<ToolCallOutcome> {
        let ToolCall {
            tool,
            arguments,
            result_source,
        } = call;
        if self.is_poisoned() {
            return Err(self.oversized_line_error());
        }
        let params = CallToolRequestParams::new(tool.as_str().to_owned()).with_arguments(arguments);

        let reply = tokio::time::timeout(timeout.get(), self.client.call_tool_once(params))
            .await
            .map_err(|_elapsed| Error::Timeout {
                operation: format!("call {}.{tool}", self.server_id),
                duration_secs: timeout.get().as_secs(),
            })?;

        match reply {
            Ok(CallToolResponse::Complete(result)) => Ok(classify(result, result_source)),
            Ok(_) => Err(self.connection_failed(std::io::Error::other(
                "server replied with an input-required or task result, which is not supported",
            ))),
            Err(ServiceError::McpError(error)) => Ok(ToolCallOutcome::RpcError {
                code: Some(error.code.0),
                server_text: Some(bound_server_text(&error.message)),
            }),
            Err(_) if self.is_poisoned() => Err(self.oversized_line_error()),
            Err(other) => Err(self.connection_failed(other)),
        }
    }

    /// Closes the session, killing any child process it owns.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_introspector::Session;
    /// # async fn example(session: Session) {
    /// session.close().await;
    /// # }
    /// ```
    pub async fn close(mut self) {
        if let Err(error) = self.client.close_with_timeout(CLOSE_GRACE).await {
            tracing::warn!(server = %self.server_id, %error, "MCP client task ended abnormally");
        }
        if let Some(mut child) = self.child.take()
            && let Err(error) = child.kill().await
        {
            tracing::warn!(server = %self.server_id, %error, "failed to terminate MCP server process");
        }
    }

    /// Whether a stdio server sent a response line over the size cap.
    ///
    /// The line was dropped unparsed, so no pending call can be matched to it: the transport is
    /// closed, every call in flight fails, and every later call on this session fails at once
    /// with [`Error::ResourceLimitExceeded`]. Open a new session to continue.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_introspector::Session;
    /// # fn example(session: &Session) {
    /// if session.is_poisoned() {
    ///     // reconnect
    /// }
    /// # }
    /// ```
    #[must_use]
    pub fn is_poisoned(&self) -> bool {
        self.oversized
            .as_ref()
            .is_some_and(|flag| flag.load(Ordering::Acquire))
    }

    fn oversized_line_error(&self) -> Error {
        Error::ResourceLimitExceeded {
            resource: ResourceKind::ResponseLine {
                server_id: self.server_id.clone(),
            },
            actual: MAX_RESPONSE_LINE_BYTES + 1,
            limit: MAX_RESPONSE_LINE_BYTES,
        }
    }

    fn connection_failed(&self, source: impl std::error::Error + Send + Sync + 'static) -> Error {
        Error::ConnectionFailed {
            server: self.server_id.clone(),
            source: Box::new(source),
        }
    }
}

impl Introspector {
    /// Opens a [`Session`] to the server, re-validating `config` before any spawn or connect.
    ///
    /// Transports, bounds and errors are identical to discovery.
    ///
    /// # Errors
    ///
    /// Returns a validation error for a bad config, [`Error::Timeout`] when the handshake
    /// exceeds the connect timeout, or [`Error::ConnectionFailed`].
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_core::{ServerConfig, ServerId};
    /// use mcp_execution_introspector::Introspector;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let config = ServerConfig::builder().command("server".to_string()).build()?;
    /// let session = Introspector::new().connect(ServerId::new("demo")?, &config).await?;
    /// session.close().await;
    /// # Ok(())
    /// # }
    /// ```
    #[tracing::instrument(skip_all, fields(server_id = %server_id))]
    pub async fn connect(&self, server_id: ServerId, config: &ServerConfig) -> Result<Session> {
        validate_server_config(config)?;

        let (client, child, oversized) = match config.transport() {
            Transport::Stdio { command, .. } => {
                let mut child = spawn_introspection_child(&server_id, command, config)?;
                let stdio = take_child_stdio(&server_id, &mut child)?;
                let oversized = Arc::new(AtomicBool::new(false));
                let halves = stdio_halves(stdio, Some(Arc::clone(&oversized)));
                match connect_bounded(&server_id, config, ().serve(halves)).await {
                    Ok(client) => (client, Some(child), Some(oversized)),
                    Err(error) => {
                        if let Err(kill_error) = child.kill().await {
                            tracing::warn!(%kill_error, "failed to terminate MCP server process");
                        }
                        return Err(error);
                    }
                }
            }
            Transport::Http { .. } | Transport::Sse { .. } => {
                (connect_http_client(&server_id, config).await?, None, None)
            }
        };

        Ok(Session {
            server_id,
            client,
            child,
            oversized,
        })
    }
}

/// Builds sanitized, capped server text, flagging whether the original was cut.
fn bound_server_text(raw: &str) -> ServerText {
    let sanitized = sanitize_untrusted_text(raw, MAX_UNTRUSTED_FIELD_LEN + 1);
    let truncated = sanitized.chars().count() > MAX_UNTRUSTED_FIELD_LEN;
    let text = if truncated {
        sanitized.chars().take(MAX_UNTRUSTED_FIELD_LEN).collect()
    } else {
        sanitized
    };
    ServerText { text, truncated }
}

/// Takes a `structuredContent` that is present and not a literal JSON `null`.
fn take_structured(slot: &mut Option<serde_json::Value>) -> Option<serde_json::Value> {
    slot.take().filter(|v| !v.is_null())
}

/// Classifies a completed `tools/call` reply (FR-030 semantics), moving the result value out
/// of `result` instead of copying it.
fn classify(mut result: CallToolResult, source: ResultSource) -> ToolCallOutcome {
    if result.is_error == Some(true) {
        let text = result
            .content
            .first()
            .and_then(|block| block.as_text())
            .map(|t| t.text.clone())
            .filter(|t| !t.is_empty())
            .or_else(|| take_structured(&mut result.structured_content).map(|v| v.to_string()));
        return ToolCallOutcome::ToolError {
            server_text: text.as_deref().map(bound_server_text),
        };
    }

    if source == ResultSource::Structured
        && let Some(value) = take_structured(&mut result.structured_content)
    {
        return ToolCallOutcome::Value(value);
    }

    let structured_or_empty = |slot: &mut Option<serde_json::Value>| {
        take_structured(slot).map_or(ToolCallOutcome::Empty, ToolCallOutcome::Value)
    };
    let Some(first) = result.content.first() else {
        return structured_or_empty(&mut result.structured_content);
    };

    match first.as_text() {
        Some(text) if !text.text.is_empty() => ToolCallOutcome::Value(parse_text(&text.text)),
        _ => serde_json::to_value(first).map_or_else(
            |_| structured_or_empty(&mut result.structured_content),
            ToolCallOutcome::Value,
        ),
    }
}

/// Parses text that looks like a JSON object or array, keeping any other text as a string.
fn parse_text(text: &str) -> serde_json::Value {
    let trimmed = text.trim();
    if trimmed.starts_with(['{', '[']) {
        serde_json::from_str(trimmed).unwrap_or_else(|_| serde_json::Value::String(text.to_owned()))
    } else {
        serde_json::Value::String(text.to_owned())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn call_timeout_bounds_are_inclusive() {
        assert!(CallTimeout::new(CallTimeout::MIN).is_ok());
        assert!(CallTimeout::new(CallTimeout::MAX).is_ok());
        assert!(CallTimeout::new(Duration::from_millis(999)).is_err());
        assert!(CallTimeout::new(Duration::from_millis(600_001)).is_err());
    }

    use rmcp::model::ContentBlock;
    use serde_json::json;

    fn value(outcome: ToolCallOutcome) -> serde_json::Value {
        let ToolCallOutcome::Value(value) = outcome else {
            panic!("expected a value, got {outcome:?}");
        };
        value
    }

    #[test]
    fn classify_structured_null_counts_as_absent() {
        let mut result = CallToolResult::success(vec![ContentBlock::text("plain")]);
        result.structured_content = Some(serde_json::Value::Null);

        let outcome = classify(result, ResultSource::Structured);

        assert_eq!(value(outcome), json!("plain"));
    }

    #[test]
    fn classify_falls_back_to_structured_when_content_is_empty() {
        let mut result = CallToolResult::success(Vec::new());
        result.structured_content = Some(json!({"k": 1}));

        assert_eq!(
            value(classify(result, ResultSource::Content)),
            json!({"k": 1})
        );
    }

    #[test]
    fn classify_tool_error_without_text_uses_structured_content() {
        let mut result = CallToolResult::error(Vec::new());
        result.structured_content = Some(json!({"why": "x"}));

        let ToolCallOutcome::ToolError {
            server_text: Some(text),
        } = classify(result, ResultSource::Content)
        else {
            panic!("expected a tool error with text");
        };

        assert_eq!(text.text, r#"{"why":"x"}"#);
    }

    #[test]
    fn classify_non_text_first_block_is_returned_as_its_json_form() {
        let result = CallToolResult::success(vec![ContentBlock::image("AAAA", "image/png")]);

        let found = value(classify(result, ResultSource::Content));

        assert_eq!(found["type"], "image");
    }

    #[test]
    fn bound_server_text_neutralizes_controls_and_caps_with_a_flag() {
        let hostile = format!("a\u{1b}[2Jb{}", "z".repeat(600));

        let bounded = bound_server_text(&hostile);

        assert!(bounded.truncated);
        assert_eq!(bounded.text.chars().count(), MAX_UNTRUSTED_FIELD_LEN);
        assert!(!bounded.text.contains('\u{1b}'));
        assert!(!bound_server_text("short").truncated);
    }

    #[test]
    fn parse_text_keeps_non_json_and_broken_json_as_strings() {
        assert_eq!(parse_text(" [1, 2] "), json!([1, 2]));
        assert_eq!(parse_text("{oops"), json!("{oops"));
        assert_eq!(parse_text("42"), json!("42"));
    }
}
