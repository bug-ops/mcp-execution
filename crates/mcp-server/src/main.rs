//! MCP server entry point for progressive loading generation.
//!
//! This binary provides an MCP server that helps generate progressive loading
//! TypeScript files for other MCP servers. Claude provides categorization
//! intelligence through natural language understanding.
//!
//! # Usage
//!
//! Run the server via stdio transport:
//!
//! ```bash
//! mcp-execution-server
//! ```
//!
//! Or configure in `~/.config/claude/mcp.json`:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "mcp-execution": {
//!       "command": "mcp-execution-server"
//!     }
//!   }
//! }
//! ```

use anyhow::Result;
use futures_util::StreamExt;
use futures_util::stream::{self, Stream};
use mcp_execution_server::service::GeneratorService;
use rmcp::RoleServer;
use rmcp::ServiceExt;
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::async_rw::{JsonRpcMessageCodec, JsonRpcMessageCodecError};
use rmcp::transport::stdio;
use std::task::Poll;
use tokio::io::AsyncRead;
use tokio_util::codec::{FramedRead, FramedWrite};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

/// Maximum size, in bytes, of a single newline-delimited JSON-RPC request read from
/// stdin. Requests exceeding this are discarded rather than buffered without bound.
///
/// `rmcp`'s [`stdio`] transport reads lines via an unbounded `BufReader::read_until`,
/// so the cap is enforced here by wiring the stdin side through
/// [`JsonRpcMessageCodec::new_with_max_length`] instead of using `stdio()` directly.
/// 4 MiB leaves headroom over the largest legitimate payload (`save_categorized_tools`
/// with `MAX_TOOL_FILES` entries, or a `MAX_SKILL_CONTENT_SIZE` skill body), both under
/// 1 MiB — but this constant is *not* the peak resident buffer size: `tokio_util`'s
/// internal read buffer grows by doubling and is only checked against this bound after
/// each read fills whatever capacity it already reserved, so an attacker's oversized
/// line can push peak buffer capacity to roughly 4x this value (measured ~16 MiB for a
/// 4 MiB cap) before it is rejected. Still strictly bounded, just not 1:1 with the cap.
const MAX_REQUEST_LINE_SIZE: usize = 4 * 1024 * 1024;

/// Wraps a size-bounded [`FramedRead`] so one oversized or malformed line drops that
/// request without ending the session, while a genuine I/O error still ends it.
///
/// `tokio_util`'s `FramedImpl` treats any `Decoder::decode` error as terminal: the poll
/// immediately after an `Err` unconditionally returns `None`, which `Stream` consumers
/// (including `rmcp`'s transport) read as end-of-stream. `JsonRpcMessageCodecError`
/// carries both cases through that same `Err` channel, so they must be told apart:
/// [`JsonRpcMessageCodecError::MaxLineLengthExceeded`] and `::Serde` mean "one bad line",
/// and we swallow the mandatory sentinel `None` that follows to keep the connection
/// alive; `::Io` means the underlying reader itself is broken (e.g. a closed or
/// orphaned fd erroring on every poll), and re-swallowing it would spin the task at
/// 100% CPU forever, so it is left fatal, matching the prior `stdio()` transport's
/// behavior on a read error. The enum is `#[non_exhaustive]`; unknown future variants
/// are treated as recoverable, consistent with the two known non-I/O cases.
fn bounded_request_stream<R>(
    reader: R,
    max_length: usize,
) -> impl Stream<Item = RxJsonRpcMessage<RoleServer>> + Send + Unpin + 'static
where
    R: AsyncRead + Send + Unpin + 'static,
{
    let mut framed = FramedRead::new(
        reader,
        JsonRpcMessageCodec::<RxJsonRpcMessage<RoleServer>>::new_with_max_length(max_length),
    );
    let mut recovering_from_error = false;
    stream::poll_fn(move |cx| {
        loop {
            return match framed.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(message))) => {
                    recovering_from_error = false;
                    Poll::Ready(Some(message))
                }
                Poll::Ready(Some(Err(JsonRpcMessageCodecError::Io(error)))) => {
                    tracing::error!(%error, "stdin read failed; ending session");
                    Poll::Ready(None)
                }
                Poll::Ready(Some(Err(error))) => {
                    tracing::warn!(%error, "dropping oversized or malformed request line");
                    recovering_from_error = true;
                    continue;
                }
                Poll::Ready(None) if recovering_from_error => {
                    recovering_from_error = false;
                    continue;
                }
                Poll::Ready(None) => Poll::Ready(None),
                Poll::Pending => Poll::Pending,
            };
        }
    })
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging to stderr (stdout is for MCP protocol)
    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,mcp_execution_server=debug")),
        )
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(std::io::stderr)
                .with_target(true),
        )
        .init();

    tracing::info!(
        "Starting mcp-execution-server v{}",
        env!("CARGO_PKG_VERSION")
    );

    // Wire stdio through a size-bounded codec instead of `stdio()` directly: the
    // default transport's read path bypasses the codec's max-length check entirely.
    let (stdin, stdout) = stdio();
    let sink = FramedWrite::new(
        stdout,
        JsonRpcMessageCodec::<TxJsonRpcMessage<RoleServer>>::new(),
    );
    let stream = bounded_request_stream(stdin, MAX_REQUEST_LINE_SIZE);

    let service = GeneratorService::new().serve((sink, stream)).await?;
    service.waiting().await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{AsyncRead, StreamExt, bounded_request_stream};
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll};
    use tokio::io::ReadBuf;

    const TEST_MAX: usize = 64;

    fn oversized_line() -> Vec<u8> {
        let mut line = vec![b'x'; TEST_MAX * 3];
        line.push(b'\n');
        line
    }

    fn valid_notification_line() -> Vec<u8> {
        br#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#
            .iter()
            .copied()
            .chain(std::iter::once(b'\n'))
            .collect()
    }

    /// `AsyncRead` that yields a fixed script of chunks in order, then a clean EOF.
    struct Script {
        chunks: Vec<Vec<u8>>,
        idx: usize,
    }

    impl AsyncRead for Script {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.idx < self.chunks.len() {
                let chunk = self.chunks[self.idx].clone();
                self.idx += 1;
                debug_assert!(
                    chunk.len() <= buf.remaining(),
                    "test fixture chunk exceeds the reader's spare buffer capacity"
                );
                buf.put_slice(&chunk);
                return Poll::Ready(Ok(()));
            }
            Poll::Ready(Ok(())) // 0 bytes read signals EOF
        }
    }

    /// Number of times [`ErrAfter`] returns a real error before giving up and
    /// signaling EOF. Bounds the fixture itself so that if a regression ever makes
    /// `bounded_request_stream` treat an I/O error as recoverable again, the resulting
    /// test fails its assertion instead of spinning forever with no `Poll::Pending`.
    const MAX_ERR_AFTER_POLLS: usize = 8;

    /// `AsyncRead` that yields a fixed script, then a persistent I/O error up to
    /// [`MAX_ERR_AFTER_POLLS`] times, then a clean EOF; counts how many errors it returned.
    struct ErrAfter {
        chunks: Vec<Vec<u8>>,
        idx: usize,
        error_polls: Arc<AtomicUsize>,
    }

    impl AsyncRead for ErrAfter {
        fn poll_read(
            mut self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<io::Result<()>> {
            if self.idx < self.chunks.len() {
                let chunk = self.chunks[self.idx].clone();
                self.idx += 1;
                debug_assert!(
                    chunk.len() <= buf.remaining(),
                    "test fixture chunk exceeds the reader's spare buffer capacity"
                );
                buf.put_slice(&chunk);
                return Poll::Ready(Ok(()));
            }
            if self.error_polls.fetch_add(1, Ordering::SeqCst) >= MAX_ERR_AFTER_POLLS {
                return Poll::Ready(Ok(())); // give up: signal EOF so a regression fails loudly
            }
            Poll::Ready(Err(io::Error::other("persistent read failure")))
        }
    }

    #[tokio::test]
    async fn recovers_from_oversized_lines_and_keeps_serving() {
        let script = Script {
            chunks: vec![
                oversized_line(),
                oversized_line(),
                valid_notification_line(),
            ],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX);

        assert!(
            stream.next().await.is_some(),
            "the trailing valid line must still decode after two oversized lines"
        );
        assert!(
            stream.next().await.is_none(),
            "stream ends cleanly at EOF after the valid message"
        );
    }

    #[tokio::test]
    async fn ends_cleanly_when_oversized_line_is_last_before_eof() {
        let script = Script {
            chunks: vec![oversized_line()],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ends_cleanly_on_unterminated_oversized_line_then_eof() {
        let script = Script {
            chunks: vec![vec![b'y'; TEST_MAX * 3]], // no trailing newline
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX);
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ends_session_on_persistent_io_error_without_spinning() {
        let error_polls = Arc::new(AtomicUsize::new(0));
        let reader = ErrAfter {
            chunks: vec![valid_notification_line()],
            idx: 0,
            error_polls: error_polls.clone(),
        };
        let mut stream = bounded_request_stream::<ErrAfter>(reader, TEST_MAX);

        assert!(
            stream.next().await.is_some(),
            "the valid line decodes before the reader starts failing"
        );
        assert!(
            stream.next().await.is_none(),
            "a persistent I/O error must end the session rather than recover"
        );
        assert_eq!(
            error_polls.load(Ordering::SeqCst),
            1,
            "the I/O error must be surfaced on the first failing poll, not retried in a hot loop"
        );
    }

    #[tokio::test]
    async fn accepts_line_at_exact_cap_and_rejects_one_byte_over() {
        // The codec's length check runs on raw byte count before any JSON parsing, so
        // the rejected case doesn't need to be valid JSON — only the accepted case does.
        let mut content = valid_notification_line();
        assert_eq!(content.pop(), Some(b'\n'), "fixture must end in a newline");
        let boundary_max = content.len();

        let mut at_cap = content.clone();
        at_cap.push(b'\n');
        let mut stream = bounded_request_stream(
            Script {
                chunks: vec![at_cap],
                idx: 0,
            },
            boundary_max,
        );
        assert!(
            stream.next().await.is_some(),
            "a line whose content is exactly max_length bytes must be accepted"
        );
        assert!(stream.next().await.is_none());

        let mut one_over = content;
        one_over.push(b' ');
        one_over.push(b'\n');
        let mut stream = bounded_request_stream(
            Script {
                chunks: vec![one_over],
                idx: 0,
            },
            boundary_max,
        );
        assert!(
            stream.next().await.is_none(),
            "a line one byte over max_length must be dropped, not accepted"
        );
    }
}
