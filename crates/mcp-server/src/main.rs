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
use rmcp::model::{GetExtensions, JsonRpcMessage};
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::async_rw::{JsonRpcMessageCodec, JsonRpcMessageCodecError};
use rmcp::transport::stdio;
use std::collections::VecDeque;
use std::sync::Arc;
use std::task::Poll;
use tokio::io::AsyncRead;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::codec::{FramedRead, FramedWrite};
use tokio_util::sync::PollSemaphore;
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

/// Maximum number of decoded requests allowed to be in flight (admitted to a handler
/// but not yet resolved) at once. Notifications and responses are never gated, so
/// cancellation and completion signals always flow regardless of this cap.
///
/// `rmcp` 2.2.0 spawns a bare `tokio::spawn` per inbound request (`spawn_service_task`
/// in its `service.rs`) with no concurrency bound of its own and no config knob to add
/// one, so admission is gated here instead, at the transport boundary this project
/// owns. Per-request worst case costs bounding this choice: `introspect_server` holds a
/// subprocess for up to 20 minutes (two caller-supplied timeouts, each capped at
/// `MAX_TIMEOUT` 600s); `generate_skill` can peak near 500 MB (500 files x 1 MiB, see
/// `mcp-skill`'s parser); `save_categorized_tools` holds a multi-MB VFS plus a
/// blocking-pool thread. A single stdio client needs only low single-digit
/// concurrency, so 8 is generous headroom while still capping the worst case.
///
/// This bounds task count, not memory: 8 concurrent `generate_skill` calls peaks near
/// 4 GB resident. [`bounded_request_stream`] additionally decodes up to
/// `MAX_CONCURRENT_REQUESTS` further requests ahead of admission (see its doc comment),
/// but a queued request has not executed yet, so it holds only its decoded message —
/// bounded by `MAX_REQUEST_LINE_SIZE` (~4 MiB), not by a running handler's own working
/// set. The combined worst case is therefore ~4 GB (8 running x ~500 MB) plus ~32 MiB
/// (8 queued x ~4 MiB), not the ~16 requests' worth a naive doubling would suggest. This
/// doubled task-count bound is an accepted trade-off, not an oversight: the threat model
/// here is a local,
/// already-trusted MCP client (e.g. Claude Code itself), not a remote or untrusted
/// attacker, consistent with the security audit that motivated this cap rating the
/// exposure P2 (arguably P3) with no remote reachability. If that threat model ever
/// changes, lower this constant and/or the decode-ahead queue size, or replace the pure
/// request-count admission with a byte-budget-based one.
const MAX_CONCURRENT_REQUESTS: usize = 8;

/// Attaches an acquired concurrency permit to a decoded request so it is released once
/// the handler's [`rmcp::service::RequestContext`] (which owns the `Extensions` this
/// permit lives in) is dropped — on completion (including an early return after the
/// handler itself observes cancellation) or panic, with no coupling to when the
/// response is sent. Cancellation alone does not release it: `rmcp`'s cancel path only
/// marks the request's `CancellationToken` cancelled and never aborts the handler task,
/// and several of `rmcp`'s own built-in request handlers never observe that token at
/// all, so for those only completion or a panic frees the permit.
///
/// Exception: `initialize` releases its permit when `serve_server_with_ct`'s setup
/// routine returns — shortly after the initialize response is sent — rather than on
/// a `RequestContext` drop. That setup code clones the request/extensions several
/// times along the way (once into the `RequestContext` it builds for the handler, once
/// more for the handler call itself), but each clone is dropped before the setup
/// function returns; the permit's last live handle is the setup routine's own
/// `request` local, so this exception is still prompt, just released one call frame
/// later than the general per-request path.
///
/// Only [`JsonRpcMessage::Request`] carries a permit; notifications and responses are
/// returned unchanged since they are never gated by [`bounded_request_stream`].
fn attach_permit(
    mut message: RxJsonRpcMessage<RoleServer>,
    permit: OwnedSemaphorePermit,
) -> RxJsonRpcMessage<RoleServer> {
    if let JsonRpcMessage::Request(ref mut request) = message {
        request.request.extensions_mut().insert(Arc::new(permit));
    }
    message
}

/// Wraps a size-bounded [`FramedRead`] so one oversized or malformed line drops that
/// request without ending the session, while a genuine I/O error still ends it; also
/// gates admission of decoded requests behind `concurrency_limit` so `rmcp`'s
/// unbounded per-request `tokio::spawn` cannot be driven arbitrarily high by a
/// pipelining client. Callers own the [`Semaphore`], so tests can inspect
/// `available_permits` directly instead of relying on the stream's internal state.
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
///
/// The concurrency gate runs strictly after this error recovery, so a rejected
/// oversized or malformed line never consumes a permit. Admission uses
/// [`PollSemaphore::poll_acquire`] rather than a bare [`tokio::sync::Semaphore::acquire`]
/// future: `poll_fn` is driven from inside `rmcp`'s `tokio::select!` alongside a
/// response-drain branch, and only `PollSemaphore` keeps its waiter alive across polls
/// that `select!` may otherwise drop, which is required for cancel-safety here.
///
/// Decoded requests are admitted through a bounded decode-ahead queue
/// (`pending_admission`), not a single blocking slot: up to `MAX_CONCURRENT_REQUESTS`
/// decoded-but-not-yet-admitted requests may sit in that queue while the underlying
/// stream keeps being polled and decoded, so a notification or response arriving behind
/// them — including a `notifications/cancelled` for one of the requests already
/// running — is still decoded and yielded immediately rather than stuck behind an
/// unadmitted request. Only the head of the queue is ever offered a permit, preserving
/// FIFO admission order.
///
/// This still bounds memory rather than buffering without limit: once
/// `pending_admission` itself reaches `MAX_CONCURRENT_REQUESTS`, the stream stops
/// decoding further input entirely until a running request's permit frees up the head
/// of the queue (see [`MAX_CONCURRENT_REQUESTS`]'s doc comment for the resulting
/// combined worst-case memory bound). In that specific saturation state a
/// `notifications/cancelled` arriving after the `2 * MAX_CONCURRENT_REQUESTS`th
/// pipelined, unresolved request would itself stall behind it — the same head-of-line
/// blocking the single-slot design this replaces had, just requiring a client to
/// pipeline `2 * MAX_CONCURRENT_REQUESTS` unresolved requests instead of 1 before it can
/// occur. This is an accepted residual risk under the trusted-local-client threat model
/// documented on `MAX_CONCURRENT_REQUESTS`, not a claim that stalls are impossible.
///
/// A `notifications/cancelled` for a request still sitting in `pending_admission` (decoded
/// but not yet yielded to `rmcp`) is silently dropped by `rmcp` itself — it has never seen
/// that request, so no `local_ct_pool` entry exists for the cancel to mark — and the
/// request runs to completion once eventually admitted, unaffected by the cancel it
/// received. This applies any time a request is merely queued, not only in the saturation
/// case above, but is not a regression: before this change every request ran to completion
/// immediately, so a cancel arriving before admission was equally unable to stop it.
///
/// Separately, `rmcp`'s own notification-handling task spawn is intentionally left
/// ungated here: notifications are fire-and-forget with no response for a caller to
/// wait on, so nothing can accumulate unresolved state behind an unbounded number of
/// them the way a stalled *request* could — this asymmetry is why only requests are
/// gated at all.
fn bounded_request_stream<R>(
    reader: R,
    max_length: usize,
    concurrency_limit: Arc<Semaphore>,
) -> impl Stream<Item = RxJsonRpcMessage<RoleServer>> + Send + Unpin + 'static
where
    R: AsyncRead + Send + Unpin + 'static,
{
    let mut framed = FramedRead::new(
        reader,
        JsonRpcMessageCodec::<RxJsonRpcMessage<RoleServer>>::new_with_max_length(max_length),
    );
    let mut recovering_from_error = false;
    let mut semaphore = PollSemaphore::new(concurrency_limit);
    let mut pending_admission: VecDeque<RxJsonRpcMessage<RoleServer>> = VecDeque::new();
    stream::poll_fn(move |cx| {
        loop {
            if !pending_admission.is_empty() {
                match semaphore.poll_acquire(cx) {
                    Poll::Ready(Some(permit)) => {
                        let message = pending_admission
                            .pop_front()
                            .expect("just checked pending_admission is non-empty");
                        return Poll::Ready(Some(attach_permit(message, permit)));
                    }
                    Poll::Ready(None) => {
                        // `PollSemaphore::close` is never called on this semaphore, so
                        // this arm is unreachable in practice; treat it as end-of-stream
                        // rather than silently discarding admitted requests.
                        tracing::error!("request concurrency semaphore closed unexpectedly");
                        return Poll::Ready(None);
                    }
                    Poll::Pending => {
                        if pending_admission.len() >= MAX_CONCURRENT_REQUESTS {
                            // The decode-ahead queue is at capacity and its head still
                            // has no permit: stop decoding further input so memory stays
                            // bounded. `poll_acquire` above already registered a waker,
                            // so this wakes as soon as a running request's permit frees.
                            return Poll::Pending;
                        }
                    }
                }
            }

            return match framed.poll_next_unpin(cx) {
                Poll::Ready(Some(Ok(message @ JsonRpcMessage::Request(_)))) => {
                    recovering_from_error = false;
                    pending_admission.push_back(message);
                    continue;
                }
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
                Poll::Ready(None) if !pending_admission.is_empty() => {
                    // Genuine EOF, but earlier-decoded requests are still queued
                    // awaiting a permit; the `poll_acquire` above already registered a
                    // waker for the queue's head, so keep waiting instead of dropping
                    // them by ending the stream early.
                    Poll::Pending
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
    let stream = bounded_request_stream(
        stdin,
        MAX_REQUEST_LINE_SIZE,
        Arc::new(Semaphore::new(MAX_CONCURRENT_REQUESTS)),
    );

    let service = GeneratorService::new().serve((sink, stream)).await?;
    service.waiting().await?;

    tracing::info!("Server shutdown complete");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        AsyncRead, GetExtensions, JsonRpcMessage, MAX_CONCURRENT_REQUESTS, MAX_REQUEST_LINE_SIZE,
        OwnedSemaphorePermit, RoleServer, RxJsonRpcMessage, Semaphore, Stream, StreamExt,
        bounded_request_stream,
    };
    use mcp_execution_server::service::GeneratorService;
    use rmcp::ServiceExt;
    use rmcp::model::NumberOrString;
    use rmcp::service::TxJsonRpcMessage;
    use rmcp::transport::async_rw::JsonRpcMessageCodec;
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, ReadBuf};
    use tokio_util::codec::FramedWrite;

    const TEST_MAX: usize = 64;
    const TEST_CONCURRENCY: usize = 8;

    fn semaphore(permits: usize) -> Arc<Semaphore> {
        Arc::new(Semaphore::new(permits))
    }

    /// Polls `stream.next()` with a bounded timeout so a regression that makes the
    /// stream stall forever fails the test with a clear panic instead of hanging.
    async fn next_or_timeout<S>(stream: &mut S) -> Option<S::Item>
    where
        S: Stream + Unpin,
    {
        tokio::time::timeout(Duration::from_secs(2), stream.next())
            .await
            .expect("stream.next() must resolve within 2s instead of hanging")
    }

    /// Extracts the numeric id of a decoded request, to distinguish reordering or
    /// duplication from genuine admission in tests with more than one in-flight request.
    fn request_id(message: &RxJsonRpcMessage<RoleServer>) -> i64 {
        match message {
            JsonRpcMessage::Request(request) => match &request.id {
                NumberOrString::Number(id) => *id,
                NumberOrString::String(id) => {
                    panic!("test fixtures only use numeric request ids, got {id:?}")
                }
            },
            _ => panic!("expected a JsonRpcMessage::Request"),
        }
    }

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

    fn valid_request_line(id: u64) -> Vec<u8> {
        format!(r#"{{"jsonrpc":"2.0","id":{id},"method":"ping"}}"#)
            .into_bytes()
            .into_iter()
            .chain(std::iter::once(b'\n'))
            .collect()
    }

    /// Extracts the permit a gated request must carry, so tests can control exactly
    /// when it is released by dropping the returned value.
    fn take_permit(message: &mut RxJsonRpcMessage<RoleServer>) -> Arc<OwnedSemaphorePermit> {
        match message {
            JsonRpcMessage::Request(request) => request
                .request
                .extensions_mut()
                .get::<Arc<OwnedSemaphorePermit>>()
                .cloned()
                .expect("a decoded request must carry a permit inserted by attach_permit"),
            _ => panic!("expected a JsonRpcMessage::Request"),
        }
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
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

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
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn ends_cleanly_on_unterminated_oversized_line_then_eof() {
        let script = Script {
            chunks: vec![vec![b'y'; TEST_MAX * 3]], // no trailing newline
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));
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
        let mut stream =
            bounded_request_stream::<ErrAfter>(reader, TEST_MAX, semaphore(TEST_CONCURRENCY));

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
            semaphore(TEST_CONCURRENCY),
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
            semaphore(TEST_CONCURRENCY),
        );
        assert!(
            stream.next().await.is_none(),
            "a line one byte over max_length must be dropped, not accepted"
        );
    }

    #[tokio::test]
    async fn permit_is_released_after_request_is_dropped() {
        let limit = semaphore(1);
        let script = Script {
            chunks: vec![valid_request_line(1)],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, limit.clone());

        let mut message = stream
            .next()
            .await
            .expect("a valid request line must decode");
        assert_eq!(
            limit.available_permits(),
            0,
            "the single permit must be held while the request is in flight"
        );

        let permit = take_permit(&mut message);
        drop(message);
        assert_eq!(
            limit.available_permits(),
            0,
            "the Arc-wrapped permit clone held by the test must still keep it reserved"
        );

        drop(permit);
        assert_eq!(
            limit.available_permits(),
            1,
            "dropping the last Arc<OwnedSemaphorePermit> must release the permit"
        );
    }

    #[tokio::test]
    async fn yields_pending_at_capacity_instead_of_dropping_or_erroring() {
        let limit = semaphore(1);
        let script = Script {
            chunks: vec![valid_request_line(1), valid_request_line(2)],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, limit.clone());

        let mut first = stream
            .next()
            .await
            .expect("the first request line must decode and be admitted");
        assert_eq!(limit.available_permits(), 0);

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        match Pin::new(&mut stream).poll_next(&mut cx) {
            Poll::Pending => {}
            Poll::Ready(item) => {
                panic!(
                    "expected Poll::Pending while at capacity, got Some={}",
                    item.is_some()
                )
            }
        }

        // Releasing here hands the freed permit straight to the stream's already-
        // registered `PollSemaphore` waiter (tokio's fair queueing skips the general
        // pool when a waiter exists), so `available_permits` would read 0 even though
        // the second request is now unblocked — check admission instead of the count.
        let permit = take_permit(&mut first);
        drop(first);
        drop(permit);

        let second = next_or_timeout(&mut stream)
            .await
            .expect("the second request must be admitted once a permit is available");
        assert_eq!(
            request_id(&second),
            2,
            "the admitted request must be the second one (id 2), not a reorder or duplicate of the first"
        );
    }

    #[tokio::test]
    async fn oversized_line_consumes_no_permit() {
        let limit = semaphore(1);
        let script = Script {
            chunks: vec![oversized_line(), valid_request_line(1)],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, limit.clone());

        let mut message = next_or_timeout(&mut stream)
            .await
            .expect("the valid request line after the oversized one must still decode");
        assert_eq!(
            limit.available_permits(),
            0,
            "only the valid request should have consumed the single permit"
        );

        let permit = take_permit(&mut message);
        drop(message);
        drop(permit);
        assert_eq!(limit.available_permits(), 1);
    }

    #[tokio::test]
    async fn notification_bypasses_a_request_still_awaiting_a_permit() {
        // No permits at all: the request can never be admitted in this test, so any
        // notification decoded after it can only reach the caller if the decode-ahead
        // queue lets the stream keep reading past an unadmitted request.
        let limit = semaphore(0);
        let script = Script {
            chunks: vec![valid_request_line(1), valid_notification_line()],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, limit);

        let notification = next_or_timeout(&mut stream)
            .await
            .expect("the notification behind the unadmitted request must still be decoded");
        assert!(
            matches!(notification, JsonRpcMessage::Notification(_)),
            "expected a notification to bypass the request stuck waiting for a permit"
        );
    }

    #[tokio::test]
    async fn decode_ahead_queue_stalls_once_it_reaches_capacity() {
        // No permits available, so every decoded request piles up in the bounded
        // decode-ahead queue. Once it holds `MAX_CONCURRENT_REQUESTS` requests, the
        // stream must stop decoding further input entirely (bounding memory) rather
        // than admit a `MAX_CONCURRENT_REQUESTS + 1`th request or bypass it with the
        // trailing notification.
        let limit = semaphore(0);
        let mut chunks: Vec<Vec<u8>> = (1..=MAX_CONCURRENT_REQUESTS as u64)
            .map(valid_request_line)
            .collect();
        chunks.push(valid_notification_line());
        let script = Script { chunks, idx: 0 };
        let mut stream = bounded_request_stream(script, TEST_MAX, limit);

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        match Pin::new(&mut stream).poll_next(&mut cx) {
            Poll::Pending => {}
            Poll::Ready(item) => panic!(
                "expected Poll::Pending once the decode-ahead queue is full, got Some={}",
                item.is_some()
            ),
        }
    }

    #[tokio::test]
    async fn permit_returns_to_capacity_after_real_rmcp_round_trip() {
        const CAPACITY: usize = 2;
        let limit = semaphore(CAPACITY);

        let (client, server) = tokio::io::duplex(64 * 1024);
        let (server_read, server_write) = tokio::io::split(server);
        let (client_read, mut client_write) = tokio::io::split(client);

        let stream = bounded_request_stream(server_read, MAX_REQUEST_LINE_SIZE, limit.clone());
        let sink = FramedWrite::new(
            server_write,
            JsonRpcMessageCodec::<TxJsonRpcMessage<RoleServer>>::new(),
        );

        let service_task = tokio::spawn(async move {
            let service = GeneratorService::new()
                .serve((sink, stream))
                .await
                .expect("initialize handshake must succeed over the bounded stream");
            service.waiting().await
        });

        let mut client_reader = BufReader::new(client_read);

        client_write
            .write_all(
                br#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"test-client","version":"0.0.0"}}}
"#,
            )
            .await
            .expect("write initialize request");

        let mut init_response = String::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            client_reader.read_line(&mut init_response),
        )
        .await
        .expect("initialize response must arrive within 5s")
        .expect("read initialize response");
        assert!(
            init_response.contains(r#""id":1"#),
            "expected an initialize response, got: {init_response}"
        );

        client_write
            .write_all(
                br#"{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"list_generated_servers","arguments":{"base_dir":"nonexistent-mcp-permit-test-dir"}}}
"#,
            )
            .await
            .expect("write tools/call request");

        let mut tool_response = String::new();
        tokio::time::timeout(
            Duration::from_secs(5),
            client_reader.read_line(&mut tool_response),
        )
        .await
        .expect("tools/call response must arrive within 5s")
        .expect("read tools/call response");
        assert!(
            tool_response.contains(r#""id":2"#),
            "expected a tools/call response, got: {tool_response}"
        );
        // The relative `base_dir` above must resolve and scan successfully - it just isn't
        // expected to find anything - so this exercises the same `spawn_blocking` scan path
        // as before `base_dir` confinement (#236) started rejecting absolute paths. A rejected
        // `base_dir` surfaces as a JSON-RPC `error` object rather than a `result`, so checking
        // for `result` distinguishes an actual scan from an early confinement bail-out.
        assert!(
            tool_response.contains(r#""result":"#),
            "expected list_generated_servers to succeed, got: {tool_response}"
        );

        // Permit release (on `RequestContext` drop) is not coupled to when the response
        // is sent, so it can lag the response by a poll or two; poll for it instead of
        // asserting immediately.
        tokio::time::timeout(Duration::from_secs(2), async {
            loop {
                if limit.available_permits() == CAPACITY {
                    return;
                }
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
        })
        .await
        .unwrap_or_else(|_| {
            panic!(
                "semaphore did not return to full capacity ({CAPACITY}) within 2s; available: {}",
                limit.available_permits()
            )
        });

        drop(client_write);
        drop(client_reader);
        let _ = tokio::time::timeout(Duration::from_secs(5), service_task).await;
    }
}
