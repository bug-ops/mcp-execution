//! Wire types of the helper protocol, version [`HELPER_PROTOCOL_VERSION`].
//!
//! Newline-delimited JSON, UTF-8, one object per line, at most
//! [`MAX_HELPER_LINE_BYTES`] bytes.
//! The client sends a [`Hello`] and the helper answers a [`HelloReply`]; after that the client
//! sends [`Request`] lines and the helper answers [`Response`] lines, possibly out of order and
//! matched by `id`. The line codec lives here; the server loop is in
//! [`crate::runtime::helper`].

use mcp_execution_core::helper_protocol::{HELPER_PROTOCOL_VERSION, MAX_HELPER_LINE_BYTES};
use mcp_execution_core::{ServerId, ToolName};
use mcp_execution_introspector::ResultSource;
use serde::{Deserialize, Serialize};
use tokio::io::{AsyncBufRead, AsyncBufReadExt as _};

/// First line from the client; `token` is present only on the Windows named-pipe carrier.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::Hello;
///
/// let hello: Hello = serde_json::from_str(r#"{"v":1}"#).unwrap();
/// assert!(hello.token.is_none());
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    /// Protocol version the client speaks.
    pub v: u32,
    /// Per-run token, required on Windows.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
}

/// The helper's answer to a matching [`Hello`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::HelloReply;
///
/// assert_eq!(serde_json::to_string(&HelloReply::current()).unwrap(), r#"{"v":1}"#);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct HelloReply {
    /// Protocol version the helper speaks.
    pub v: u32,
}

impl HelloReply {
    /// The reply for the protocol version this build speaks.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::protocol::HelloReply;
    ///
    /// assert_eq!(HelloReply::current().v, 1);
    /// ```
    #[must_use]
    pub const fn current() -> Self {
        Self {
            v: HELPER_PROTOCOL_VERSION,
        }
    }
}

/// Where a call's result value is read from; the wire form of
/// [`ResultSource`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::ResultKind;
///
/// let kind: ResultKind = serde_json::from_str(r#""structured""#).unwrap();
/// assert_eq!(kind, ResultKind::Structured);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultKind {
    /// Prefer `structuredContent`.
    Structured,
    /// Take `content[0]`.
    Content,
}

impl From<ResultKind> for ResultSource {
    fn from(kind: ResultKind) -> Self {
        match kind {
            ResultKind::Structured => Self::Structured,
            ResultKind::Content => Self::Content,
        }
    }
}

/// One tool call from the script.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::Request;
///
/// let line = r#"{"id":1,"server":"demo","tool":"echo","arguments":{},"result":"content"}"#;
/// let request: Request = serde_json::from_str(line).unwrap();
/// assert_eq!(request.id, 1);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Request {
    /// Client-chosen id echoed in the response.
    pub id: u64,
    /// Server to call.
    pub server: ServerId,
    /// Tool to call.
    pub tool: ToolName,
    /// Arguments; an object by type.
    pub arguments: serde_json::Map<String, serde_json::Value>,
    /// Where the result value is read from.
    pub result: ResultKind,
}

/// Why a call failed.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::ErrorKind;
///
/// assert_eq!(serde_json::to_string(&ErrorKind::WriteSkipped).unwrap(), r#""write_skipped""#);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorKind {
    /// The tool reported an error.
    ToolError,
    /// The server answered with a JSON-RPC error.
    RpcError,
    /// The call could not be made or completed (transport, limit, timeout, channel).
    CallFailed,
    /// Dry run: a write call was not executed.
    WriteSkipped,
}

/// The error part of a failed [`Response`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::{ErrorBody, ErrorKind};
///
/// let body = ErrorBody {
///     kind: ErrorKind::CallFailed,
///     message: "helper channel closed".to_string(),
///     server_message: None,
///     server_message_truncated: None,
/// };
/// assert!(!serde_json::to_string(&body).unwrap().contains("server_message"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ErrorBody {
    /// Failure class.
    pub kind: ErrorKind,
    /// Runner-written text, never server text.
    pub message: String,
    /// Sanitized server text, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_message: Option<String>,
    /// Whether `server_message` was cut.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub server_message_truncated: Option<bool>,
}

/// What a [`Response`] carries.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::ResponseBody;
///
/// let body: ResponseBody = serde_json::from_str(r#"{"empty":true}"#).unwrap();
/// assert!(matches!(body, ResponseBody::Empty { .. }));
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ResponseBody {
    /// The extracted result value.
    Value {
        /// The value.
        value: serde_json::Value,
    },
    /// The call succeeded with no result.
    Empty {
        /// Always `true` on the wire.
        empty: bool,
    },
    /// The call failed.
    Error {
        /// Why.
        error: ErrorBody,
    },
}

/// The helper's answer to one [`Request`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::{Response, ResponseBody};
///
/// let response = Response {
///     id: 7,
///     body: ResponseBody::Empty { empty: true },
/// };
/// assert_eq!(serde_json::to_string(&response).unwrap(), r#"{"id":7,"empty":true}"#);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Response {
    /// Id of the request this answers.
    pub id: u64,
    /// The outcome.
    #[serde(flatten)]
    pub body: ResponseBody,
}

/// One line read from a channel.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::Line;
///
/// assert_ne!(Line::Eof, Line::Oversized);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Line {
    /// A complete line, without its newline.
    Text(Vec<u8>),
    /// A line longer than the limit; the rest of it was not read.
    Oversized,
    /// The channel ended (a final unterminated fragment is dropped).
    Eof,
}

/// Reads one newline-terminated line of at most `limit` bytes (newline excluded), never
/// buffering more than `limit` plus one read buffer.
///
/// # Errors
///
/// Returns the I/O error of the underlying reader.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::{Line, read_line};
///
/// # tokio_test_block(async {
/// let mut reader = tokio::io::BufReader::new(&b"{\"v\":1}\nrest"[..]);
/// assert_eq!(read_line(&mut reader, 64).await.unwrap(), Line::Text(b"{\"v\":1}".to_vec()));
/// assert_eq!(read_line(&mut reader, 64).await.unwrap(), Line::Eof);
/// # });
/// # fn tokio_test_block(f: impl std::future::Future<Output = ()>) {
/// #     tokio::runtime::Builder::new_current_thread().build().unwrap().block_on(f);
/// # }
/// ```
pub async fn read_line<R: AsyncBufRead + Unpin>(
    reader: &mut R,
    limit: usize,
) -> std::io::Result<Line> {
    let mut line = Vec::new();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(Line::Eof);
        }
        let newline = available.iter().position(|byte| *byte == b'\n');
        let take = newline.unwrap_or(available.len());
        if line.len() + take > limit {
            return Ok(Line::Oversized);
        }
        line.extend_from_slice(&available[..take]);
        reader.consume(take + usize::from(newline.is_some()));
        if newline.is_some() {
            return Ok(Line::Text(line));
        }
    }
}

/// Serializes a response as one line (with its newline). A line over
/// [`MAX_HELPER_LINE_BYTES`] is replaced by a `call_failed` response for the same id.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::protocol::{Response, ResponseBody, encode_response};
///
/// let line = encode_response(&Response { id: 3, body: ResponseBody::Empty { empty: true } });
/// assert_eq!(line, b"{\"id\":3,\"empty\":true}\n");
/// ```
#[must_use]
pub fn encode_response(response: &Response) -> Vec<u8> {
    let mut line = serde_json::to_vec(response).unwrap_or_default();
    if line.is_empty() || line.len() > MAX_HELPER_LINE_BYTES {
        let failed = Response {
            id: response.id,
            body: ResponseBody::Error {
                error: ErrorBody {
                    kind: ErrorKind::CallFailed,
                    message: format!("result exceeds {MAX_HELPER_LINE_BYTES} bytes"),
                    server_message: None,
                    server_message_truncated: None,
                },
            },
        };
        line = serde_json::to_vec(&failed).unwrap_or_default();
    }
    line.push(b'\n');
    line
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn response_variants_round_trip_through_the_untagged_body() {
        let lines = [
            r#"{"id":1,"value":{"a":1}}"#,
            r#"{"id":2,"empty":true}"#,
            r#"{"id":3,"error":{"kind":"tool_error","message":"m","server_message":"s","server_message_truncated":false}}"#,
        ];
        for line in lines {
            let response: Response = serde_json::from_str(line).unwrap();
            assert_eq!(serde_json::to_string(&response).unwrap(), line);
        }
    }

    #[test]
    fn request_rejects_an_invalid_server_id() {
        let line = r#"{"id":1,"server":"../x","tool":"t","arguments":{},"result":"content"}"#;
        assert!(serde_json::from_str::<Request>(line).is_err());
    }

    #[test]
    fn request_rejects_non_object_arguments() {
        let line = r#"{"id":1,"server":"demo","tool":"t","arguments":[],"result":"content"}"#;
        assert!(serde_json::from_str::<Request>(line).is_err());
    }

    #[tokio::test]
    async fn read_line_splits_lines_and_flags_oversize() {
        let mut reader = tokio::io::BufReader::new(&b"ab\ncdefgh\nz"[..]);
        assert_eq!(
            read_line(&mut reader, 4).await.unwrap(),
            Line::Text(b"ab".to_vec())
        );
        assert_eq!(read_line(&mut reader, 4).await.unwrap(), Line::Oversized);
    }

    #[tokio::test]
    async fn read_line_accepts_exactly_the_limit_across_small_reads() {
        let (mut tx, rx) = tokio::io::duplex(2);
        tokio::spawn(async move {
            use tokio::io::AsyncWriteExt as _;
            tx.write_all(b"abcd\n").await.unwrap();
        });
        let mut reader = tokio::io::BufReader::with_capacity(2, rx);
        assert_eq!(
            read_line(&mut reader, 4).await.unwrap(),
            Line::Text(b"abcd".to_vec())
        );
    }

    #[test]
    fn encode_response_replaces_an_oversized_line_with_call_failed() {
        let response = Response {
            id: 9,
            body: ResponseBody::Value {
                value: serde_json::Value::String("x".repeat(MAX_HELPER_LINE_BYTES)),
            },
        };
        let line = encode_response(&response);
        let text = String::from_utf8(line).unwrap();
        assert!(
            text.contains(r#""id":9"#) && text.contains("call_failed") && text.contains("exceeds")
        );
    }
}
