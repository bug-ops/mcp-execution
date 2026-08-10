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
//! mcp-execution
//! ```
//!
//! Or configure in `~/.config/claude/mcp.json`:
//!
//! ```json
//! {
//!   "mcpServers": {
//!     "mcp-execution": {
//!       "command": "mcp-execution"
//!     }
//!   }
//! }
//! ```

use anyhow::Result;
use clap::Parser;
use clap::builder::{PossibleValuesParser, TypedValueParser as _};
use futures_util::StreamExt;
use futures_util::stream::{self, Stream};
use mcp_execution_core::cli::{LOG_FORMAT_ENV_VAR, LogFormat};
use mcp_execution_core::untrusted::{MAX_UNTRUSTED_FIELD_LEN, sanitize_untrusted_text};
use mcp_execution_server::service::GeneratorService;
use rmcp::RoleServer;
use rmcp::ServiceExt;
use rmcp::model::{GetExtensions, JsonRpcMessage};
use rmcp::service::{RxJsonRpcMessage, TxJsonRpcMessage};
use rmcp::transport::async_rw::{JsonRpcMessageCodec, JsonRpcMessageCodecError};
use rmcp::transport::stdio;
use std::collections::VecDeque;
use std::fmt;
use std::str::FromStr;
use std::sync::Arc;
use std::task::Poll;
use tokio::io::AsyncRead;
use tokio::sync::{OwnedSemaphorePermit, Semaphore};
use tokio_util::bytes::BytesMut;
use tokio_util::codec::{Decoder, FramedRead, FramedWrite};
use tokio_util::sync::PollSemaphore;
use tracing_subscriber::{EnvFilter, Layer as _, layer::SubscriberExt, util::SubscriberInitExt};

/// Command-line arguments for the `mcp-execution` server binary.
///
/// Minimal by design: today, no in-repo `mcp.json` entry passes arguments to this server (see
/// `examples/README.md`, `crates/mcp-server/README.md`), so adding `clap` here to gain
/// `--log-format` is a deliberate, low-risk behavior change -- the binary now parses argv
/// (gaining `--help`/`--version`, both written to stdout, harmless since the process exits
/// immediately after) and rejects unknown arguments with exit code 2, rather than silently
/// ignoring them as before.
#[derive(Parser)]
#[command(
    name = "mcp-execution",
    version,
    about = "MCP server for progressive loading TypeScript code generation"
)]
struct ServerArgs {
    /// Diagnostic log format: `text` (default) or `json`.
    ///
    /// Independent of the MCP protocol's own JSON-RPC framing on stdout; this only affects the
    /// diagnostic logs this process writes to stderr. When unset, falls back to the
    /// `MCP_EXECUTION_LOG_FORMAT` environment variable; when that is also unset or invalid,
    /// defaults to `text`.
    #[arg(
        long = "log-format",
        ignore_case = true,
        value_parser = PossibleValuesParser::new(["text", "json"])
            .map(|s| LogFormat::from_str(&s).expect("possible values are LogFormat variants"))
    )]
    log_format: Option<LogFormat>,
}

/// Resolves the effective log format from the parsed `--log-format` flag and the
/// `MCP_EXECUTION_LOG_FORMAT` environment variable, mirroring `mcp-execution-cli`'s
/// `runner::init_logging`. Kept as its own function (rather than inlined in `main`) so tests can
/// assert the environment variable is actually consulted -- see
/// `resolve_log_format_reads_env_var_when_flag_unset` -- rather than only exercising the pure
/// `LogFormat::resolve` this delegates to.
fn resolve_log_format(args: &ServerArgs) -> LogFormat {
    let env_value = std::env::var(LOG_FORMAT_ENV_VAR).ok();
    LogFormat::resolve(args.log_format, env_value.as_deref())
}

/// Whether `main` should warn about a rejected `MCP_EXECUTION_LOG_FORMAT` value: the flag was
/// not passed (matching `LogFormat::resolve`'s own precedence -- a bad env value is not even
/// inspected once the flag has decided, so it must not warn either) and the environment variable
/// is set to a non-empty value [`LogFormat::is_invalid_env_value`] rejects. A separate function
/// from [`resolve_log_format`] (rather than a second return value there) since production code
/// needs only a yes/no answer, not the rejected value itself -- see [`LogFormat::resolve`]'s own
/// doc comment on why that value isn't threaded through.
fn log_format_env_is_invalid(args: &ServerArgs) -> bool {
    args.log_format.is_none()
        && std::env::var(LOG_FORMAT_ENV_VAR)
            .ok()
            .is_some_and(|raw| LogFormat::is_invalid_env_value(&raw))
}

/// Emits the fixed-message `WARN` log line for a rejected `MCP_EXECUTION_LOG_FORMAT` value.
/// Deliberately takes no reference to the rejected value itself: interpolating external
/// environment input into this line -- even truncated -- would open a log-injection vector,
/// since this process has no `RedactingWriter` guarding its logs (see `main`'s comment on the
/// `tracing_subscriber::registry()` setup). Extracted so a test can assert this actually fires
/// on a rejected value, using the same `WarnCounter` subscriber pattern already established
/// below for [`bounded_request_stream`]'s own warnings.
fn warn_on_rejected_log_format(rejected: bool) {
    if rejected {
        tracing::warn!(
            "invalid value for {LOG_FORMAT_ENV_VAR} (expected 'text' or 'json'), falling back to text"
        );
    }
}

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

/// Outcome of decoding one line through [`RecoveringCodec`]: either a successfully
/// parsed message, a line that was rejected (oversized or malformed), or input the
/// inner codec consumed without producing a message — e.g. a non-standard line its
/// compatibility handling chooses to ignore, or another buffered chunk of an
/// in-progress oversized-line discard. All three keep the stream going; only
/// `Message` reaches admission or a caller.
///
/// `Message` is boxed because `RxJsonRpcMessage` is far larger than the other variants;
/// an enum is sized by its largest variant, so without the `Box` every `DecodedFrame`
/// value — even a `Skipped` one — would pay `RxJsonRpcMessage`'s size. `Malformed`
/// carries the original [`JsonRpcMessageCodecError`] rather than a pre-rendered
/// `String` so logging it stays as lazy as the `Err` path it replaced (formatted only
/// if the log level is enabled) and so it can't grow past this enum's own size — a
/// `Serde` error's `Display` is not bounded by anything this project controls, and
/// could in principle embed attacker-controlled text approaching
/// [`MAX_REQUEST_LINE_SIZE`]; see [`SanitizedCodecError`]'s doc comment for why that is
/// not currently reachable with the pinned `rmcp` version, and why this code does not
/// rely on it staying unreachable.
enum DecodedFrame {
    Message(Box<RxJsonRpcMessage<RoleServer>>),
    Malformed(JsonRpcMessageCodecError),
    Skipped,
}

/// Formats a [`JsonRpcMessageCodecError`] for the `tracing::warn!` that reports a
/// dropped line, with control characters (including any embedded newline) replaced by
/// a space and the result capped at [`MAX_UNTRUSTED_FIELD_LEN`].
///
/// Defense-in-depth, not a fix for a path reachable today: `serde_json`'s `unknown
/// variant` error message interpolates the offending JSON string value verbatim via
/// `Display`, not `Debug` -- unlike most of its other error variants, this one does
/// *not* escape embedded control characters. If such an error ever reached a
/// [`JsonRpcMessageCodecError::Serde`] here, a hostile stdin line could smuggle raw
/// newlines (and other control characters) into its `Display` output and forge
/// additional log lines in the plain-text log format. With the pinned `rmcp` 3.1.2,
/// this is not reachable in practice: `RxJsonRpcMessage<RoleServer>`'s request and
/// notification payload types (`ClientRequest`/`ClientNotification`) are
/// `#[serde(untagged)]`, so a mismatched inner variant's real error -- including any
/// attacker-controlled text it carries -- is discarded by `serde` and replaced with a
/// fixed "data did not match any variant" message before it can reach
/// `JsonRpcMessageCodecError`. This wrapper exists anyway because that
/// error-swallowing is an `rmcp` implementation detail this project does not control,
/// and `JsonRpcMessageCodecError` is `#[non_exhaustive]`: a future `rmcp` release could
/// start propagating the inner error, or add a variant that does, without this crate's
/// `Cargo.lock` pin changing what compiles. `--log-format json` is unaffected either
/// way -- `serde_json` escapes whatever string this produces when it serializes the
/// event -- so this wrapper exists for the text formatter, which has no such guarantee.
///
/// Implemented as a `Display` wrapper around a borrowed error, rather than
/// pre-sanitizing into an owned `String` at the [`DecodedFrame::Malformed`] call site,
/// so formatting only happens if the event is actually recorded -- preserving the
/// laziness [`DecodedFrame::Malformed`]'s own doc comment calls out.
///
/// No test drives this through the real `bounded_request_stream` call site with a
/// genuine `JsonRpcMessageCodecError`: given the `rmcp` behavior above, no real stdin
/// input can currently produce a `Serde` error carrying a control character or text
/// over [`MAX_UNTRUSTED_FIELD_LEN`], so such an assertion would pass identically with
/// this wrapper removed -- zero regression value. The tests below instead exercise this
/// type directly against a synthetic `serde_json::Error` from a minimal, strictly-typed
/// local enum, which tests what `SanitizedCodecError` does with the class of `Display`
/// output `serde_json` documents producing, without depending on whether `rmcp`'s
/// current shapes happen to trigger it today.
struct SanitizedCodecError<'a>(&'a JsonRpcMessageCodecError);

impl fmt::Display for SanitizedCodecError<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&sanitize_untrusted_text(
            &self.0.to_string(),
            MAX_UNTRUSTED_FIELD_LEN,
        ))
    }
}

/// Wraps [`JsonRpcMessageCodec`] so an outcome that would otherwise leave
/// `tokio_util`'s `FramedImpl` believing the stream needs a fresh read — a recoverable
/// decode failure, or the inner codec silently ignoring a non-standard line — is
/// instead folded into an `Ok` item, so a request already sitting in the buffer right
/// behind such a line is decoded on the very next poll instead of stalling until more
/// bytes arrive (possibly forever, if the peer is otherwise idle; #273).
///
/// Two independent `FramedImpl` behaviors (`framed_impl.rs` in `tokio-util` 0.7.18)
/// motivate this, both of which clear its internal `is_readable` flag so the poll
/// after either one skips `decode()` and issues a real `poll_read` instead of
/// rescanning the already-buffered bytes:
/// - After a `Decoder::decode` `Err`, the *next* poll unconditionally clears
///   `is_readable` while returning the mandatory sentinel `None`.
/// - On *every* `Ok(None)`, `is_readable` is cleared unconditionally — including when
///   the inner codec's own non-standard-message compatibility handling
///   (`try_parse_with_compatibility` in rmcp's `async_rw.rs`) has already consumed and
///   discarded a line via `Ok(None)` without decoding anything from it.
///
/// [`JsonRpcMessageCodecError::MaxLineLengthExceeded`] and `::Serde` mean "one bad
/// line" and are folded to `Malformed`; unknown future non-exhaustive variants are
/// folded the same way, consistent with these two. `::Io` means the underlying reader
/// itself is broken (e.g. a closed or orphaned fd erroring on every poll) and is left
/// as a real `Err` — `Decoder::Error = std::io::Error` makes this the only variant
/// that can still surface that way — matching the prior `stdio()` transport's
/// behavior on a read error; folding it too would spin the task at 100% CPU forever.
/// An `Ok(None)` whose call left the buffer shorter than it started is folded to
/// `Skipped`; an `Ok(None)` that left the buffer unchanged (no full line buffered yet)
/// passes through unchanged, since that is the ordinary "need more bytes" case every
/// `Decoder` uses. The wrapped codec instance is reused across calls (never
/// reconstructed), since `MaxLineLengthExceeded`'s `is_discarding` skip-state lives
/// inside it.
///
/// The buffer-shrink check is a behavioral inference about `JsonRpcMessageCodec`, not
/// a contract `rmcp` or `tokio_util` documents or guarantees: it holds because the
/// inner codec's current implementation always advances the buffer past any line it
/// consumes, whether or not that line becomes a decoded item. A future `rmcp` version
/// that instead tracked a consumed line via an internal cursor without shrinking the
/// buffer would silently reopen the #273 gap `Skipped` closes here — re-verify this
/// assumption against `JsonRpcMessageCodec::decode`'s implementation on any `rmcp`
/// upgrade.
///
/// A blank or whitespace-only line makes the inner codec report
/// [`JsonRpcMessageCodecError::Serde`] (it is not valid JSON), which `fold` would
/// otherwise turn into `Malformed` and thus a `tracing::warn!` per line — the same
/// log-volume amplification fixed for the introspector's symmetric decoder in #275/#282.
/// `blank_scan_from`/`assume_mid_discard` and [`Self::peek_blank_line`] port that same
/// read-only peek so such a `Serde` error is instead folded to the already-silent
/// `DecodedFrame::Skipped` path.
struct RecoveringCodec {
    inner: JsonRpcMessageCodec<RxJsonRpcMessage<RoleServer>>,
    /// Resumable cursor for [`Self::peek_blank_line`]: leading byte count already known
    /// to contain no newline, so a long non-blank line built up over many small reads is
    /// peeked once in total instead of re-scanned from the front on every call.
    blank_scan_from: usize,
    /// Set once the inner codec reports `MaxLineLengthExceeded` and cleared once it
    /// reports a message or a `Serde` error. While set, the bytes at the front of `buf`
    /// may still be the tail of an oversized line the inner codec is discarding rather
    /// than a genuine next line, so the blank-line peek is not trusted — treating that
    /// tail as blank could misattribute the following line's own parse error to
    /// blank-line suppression instead of logging it.
    assume_mid_discard: bool,
}

impl RecoveringCodec {
    fn new(max_length: usize) -> Self {
        Self {
            inner: JsonRpcMessageCodec::new_with_max_length(max_length),
            blank_scan_from: 0,
            assume_mid_discard: false,
        }
    }

    /// Read-only check for whether the *next* line the inner codec is about to consume
    /// is empty or whitespace-only. Returns `Some(true/false)` once a full line (a `\n`
    /// within `max_length + 1` bytes of the front of `buf`) is buffered; `None` if no
    /// newline is in reach yet, in which case `scan_from` is advanced to the bound
    /// already checked so the next call resumes instead of re-scanning. Never mutates
    /// `buf`: all buffer consumption stays inside the inner codec, so this peek cannot
    /// desynchronize its own line-scan state across split reads.
    fn peek_blank_line(scan_from: &mut usize, max_length: usize, buf: &BytesMut) -> Option<bool> {
        let bound = std::cmp::min(max_length.saturating_add(1), buf.len());
        if *scan_from >= bound {
            return None;
        }
        if let Some(offset) = buf[*scan_from..bound]
            .iter()
            .position(|&byte| byte == b'\n')
        {
            let newline_at = *scan_from + offset;
            Some(buf[..newline_at].iter().all(u8::is_ascii_whitespace))
        } else {
            *scan_from = bound;
            None
        }
    }

    fn fold(
        result: Result<Option<RxJsonRpcMessage<RoleServer>>, JsonRpcMessageCodecError>,
        len_before: usize,
        len_after: usize,
        is_blank: bool,
    ) -> std::io::Result<Option<DecodedFrame>> {
        match result {
            Ok(Some(message)) => Ok(Some(DecodedFrame::Message(Box::new(message)))),
            Ok(None) if len_after < len_before => Ok(Some(DecodedFrame::Skipped)),
            Ok(None) => Ok(None),
            Err(JsonRpcMessageCodecError::Io(error)) => Err(error),
            Err(JsonRpcMessageCodecError::Serde(_)) if is_blank => Ok(Some(DecodedFrame::Skipped)),
            Err(other) => Ok(Some(DecodedFrame::Malformed(other))),
        }
    }

    fn drive(
        &mut self,
        buf: &mut BytesMut,
        decode_step: impl FnOnce(
            &mut JsonRpcMessageCodec<RxJsonRpcMessage<RoleServer>>,
            &mut BytesMut,
        ) -> Result<
            Option<RxJsonRpcMessage<RoleServer>>,
            JsonRpcMessageCodecError,
        >,
    ) -> std::io::Result<Option<DecodedFrame>> {
        let max_length = self.inner.max_length();
        let is_blank = !self.assume_mid_discard
            && Self::peek_blank_line(&mut self.blank_scan_from, max_length, buf).unwrap_or(false);

        let len_before = buf.len();
        let result = decode_step(&mut self.inner, buf);
        let len_after = buf.len();
        if len_after < len_before {
            self.blank_scan_from = 0;
        }
        match &result {
            Ok(Some(_)) | Err(JsonRpcMessageCodecError::Serde(_)) => {
                self.assume_mid_discard = false;
            }
            Err(JsonRpcMessageCodecError::MaxLineLengthExceeded) => {
                self.assume_mid_discard = true;
            }
            Ok(None) | Err(_) => {}
        }

        Self::fold(result, len_before, len_after, is_blank)
    }
}

impl Decoder for RecoveringCodec {
    type Item = DecodedFrame;
    type Error = std::io::Error;

    fn decode(&mut self, buf: &mut BytesMut) -> std::io::Result<Option<DecodedFrame>> {
        self.drive(buf, JsonRpcMessageCodec::decode)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> std::io::Result<Option<DecodedFrame>> {
        self.drive(buf, JsonRpcMessageCodec::decode_eof)
    }
}

/// Wraps a size-bounded [`FramedRead`] so one oversized or malformed line drops that
/// request without ending the session, while a genuine I/O error still ends it; also
/// gates admission of decoded requests behind `concurrency_limit` so `rmcp`'s
/// unbounded per-request `tokio::spawn` cannot be driven arbitrarily high by a
/// pipelining client. Callers own the [`Semaphore`], so tests can inspect
/// `available_permits` directly instead of relying on the stream's internal state.
///
/// Recoverable decode failures are handled by [`RecoveringCodec`] (see its doc comment
/// for why this stream never observes the `tokio_util` "stranded buffered request"
/// stall). The concurrency gate runs strictly after that recovery, so a rejected
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
    let mut framed = FramedRead::new(reader, RecoveringCodec::new(max_length));
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
                Poll::Ready(Some(Ok(DecodedFrame::Message(message)))) => {
                    let message = *message;
                    if matches!(message, JsonRpcMessage::Request(_)) {
                        pending_admission.push_back(message);
                        continue;
                    }
                    Poll::Ready(Some(message))
                }
                Poll::Ready(Some(Ok(DecodedFrame::Malformed(reason)))) => {
                    tracing::warn!(
                        reason = %SanitizedCodecError(&reason),
                        "dropping oversized or malformed request line"
                    );
                    continue;
                }
                Poll::Ready(Some(Ok(DecodedFrame::Skipped))) => {
                    tracing::trace!("inner codec consumed input without producing a message");
                    continue;
                }
                Poll::Ready(Some(Err(error))) => {
                    tracing::error!(%error, "stdin read failed; ending session");
                    Poll::Ready(None)
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
    let args = ServerArgs::parse();
    let log_format = resolve_log_format(&args);

    // Initialize logging to stderr (stdout is for MCP protocol)
    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_writer(std::io::stderr)
        .with_target(true);
    let layer = match log_format {
        LogFormat::Json => fmt_layer.json().boxed(),
        LogFormat::Text => fmt_layer.boxed(),
    };

    tracing_subscriber::registry()
        .with(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("info,mcp_execution_server=debug")),
        )
        .with(
            // Not wrapped in `mcp-execution-cli::runner`'s URL-redacting writer (see #353):
            // this process only ever builds a stdio server config from `IntrospectServerParams`
            // (see `service::build_stdio_server_config`), which has no `url` field, and no other
            // path here constructs an http/sse config -- so there is no `reqwest`/`rmcp` transport
            // error whose `Display` could embed a secret-bearing URL for this writer to reach. The
            // #209 regression test (`service.rs`) guards that invariant; if a future change adds
            // an http/sse client path to this crate, wire the same writer in at that point.
            //
            // `MCP_EXECUTION_LOG_FORMAT` (see `resolve_log_format`) is a second,
            // narrower untrusted-text-into-log path this process does have, but the warning
            // logged below never echoes its rejected raw value -- only a fixed diagnostic string
            // naming the environment variable is logged -- so it does not need this writer either.
            layer,
        )
        .init();

    warn_on_rejected_log_format(log_format_env_is_invalid(&args));

    tracing::info!("Starting mcp-execution v{}", env!("CARGO_PKG_VERSION"));

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
        AsyncRead, GetExtensions, JsonRpcMessage, LOG_FORMAT_ENV_VAR, LogFormat,
        MAX_CONCURRENT_REQUESTS, MAX_REQUEST_LINE_SIZE, MAX_UNTRUSTED_FIELD_LEN,
        OwnedSemaphorePermit, RoleServer, RxJsonRpcMessage, SanitizedCodecError, Semaphore,
        ServerArgs, Stream, StreamExt, bounded_request_stream, log_format_env_is_invalid,
        resolve_log_format, warn_on_rejected_log_format,
    };
    use clap::Parser as _;
    use mcp_execution_server::service::GeneratorService;
    use rmcp::ServiceExt;
    use rmcp::model::NumberOrString;
    use rmcp::service::TxJsonRpcMessage;
    use rmcp::transport::async_rw::{JsonRpcMessageCodec, JsonRpcMessageCodecError};
    use std::io;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::task::{Context, Poll, Waker};
    use std::time::Duration;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader, ReadBuf};
    use tokio_util::codec::FramedWrite;
    use tracing_subscriber::layer::SubscriberExt;

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

    /// `AsyncRead` that yields a fixed script of chunks in order, then `Poll::Pending`
    /// forever — an open-but-currently-idle connection, unlike [`Script`]'s clean EOF.
    ///
    /// This distinction matters for #273 regression coverage: on a clean EOF,
    /// `tokio_util`'s `FramedImpl` calls `decode_eof` on the remaining buffered bytes
    /// regardless of `is_readable`, which decodes a trailing buffered request anyway and
    /// would mask the stall entirely. Only a reader that stays open (no EOF, no further
    /// bytes) exercises the `is_readable`-cleared path the bug lives in.
    struct ScriptThenIdle {
        chunks: Vec<Vec<u8>>,
        idx: usize,
    }

    impl AsyncRead for ScriptThenIdle {
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
            Poll::Pending
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
    async fn oversized_line_and_valid_request_in_one_chunk_do_not_stall() {
        // Regression test for #273: unlike `recovers_from_oversized_lines_and_keeps_serving`
        // above (which delivers the bad and good lines as *separate* `poll_read` calls, and
        // so accidentally gives `FramedImpl` the extra read it needs to recover), this
        // delivers both lines in a single chunk so the valid request is already fully
        // buffered behind the oversized one before any decoding happens. Before the fix,
        // `tokio_util`'s `is_readable` flag was left cleared after unwinding the mandatory
        // sentinel `None` following the codec error, so the next poll issued a real
        // `poll_read` instead of rescanning the buffer, and the buffered valid request
        // never decoded until further bytes arrived — `ScriptThenIdle` never delivers
        // any, staying open (`Poll::Pending`) instead of signaling EOF, since EOF would
        // mask the bug via `decode_eof`. `next_or_timeout` fails loudly if that stall
        // recurs.
        let mut one_chunk = oversized_line();
        one_chunk.extend(valid_request_line(1));
        let script = ScriptThenIdle {
            chunks: vec![one_chunk],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        let message = next_or_timeout(&mut stream)
            .await
            .expect("the valid request sharing a chunk with the oversized line must decode without waiting for more input");
        assert_eq!(request_id(&message), 1);
    }

    #[tokio::test]
    async fn malformed_json_and_valid_request_in_one_chunk_do_not_stall() {
        // Same #273 regression as `oversized_line_and_valid_request_in_one_chunk_do_not_stall`,
        // but for a `Serde` decode error instead of `MaxLineLengthExceeded` — the debugger's
        // diagnosis confirmed both error variants hit the identical `has_errored` unwind path
        // in `tokio_util`, so both need coverage.
        let mut one_chunk = b"not valid json\n".to_vec();
        one_chunk.extend(valid_request_line(1));
        let script = ScriptThenIdle {
            chunks: vec![one_chunk],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        let message = next_or_timeout(&mut stream)
            .await
            .expect("the valid request sharing a chunk with the malformed line must decode without waiting for more input");
        assert_eq!(request_id(&message), 1);
    }

    #[tokio::test]
    async fn multiple_malformed_lines_then_valid_request_in_one_chunk_do_not_stall() {
        // Edge case beyond the single-bad-line regressions above: several
        // consecutive malformed/oversized lines must all be folded to
        // `DecodedFrame::Malformed` and looped past within the *same* poll,
        // not just one. If the loop only handled one `Malformed` per poll
        // before returning `Pending`, this would stall exactly like #273
        // since `ScriptThenIdle` never delivers another chunk.
        let mut one_chunk = oversized_line();
        one_chunk.extend(b"not valid json\n");
        one_chunk.extend(oversized_line());
        one_chunk.extend(valid_request_line(1));
        let script = ScriptThenIdle {
            chunks: vec![one_chunk],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        let message = next_or_timeout(&mut stream).await.expect(
            "the valid request behind three consecutive malformed lines in one chunk must decode without waiting for more input",
        );
        assert_eq!(request_id(&message), 1);
    }

    #[tokio::test]
    async fn chunk_with_only_malformed_lines_and_no_valid_request_stays_pending() {
        // Edge case: a chunk containing *only* malformed lines and no valid
        // request at all. There is no buffered valid request to strand, so
        // this isn't the #273 stall itself, but it confirms the stream
        // correctly reports `Pending` (waiting for more input) rather than
        // erroneously ending the stream or erroring once every malformed
        // line in the buffer has been discarded.
        let mut one_chunk = oversized_line();
        one_chunk.extend(b"not valid json\n");
        let script = ScriptThenIdle {
            chunks: vec![one_chunk],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        let waker = Waker::noop();
        let mut cx = Context::from_waker(waker);
        match Pin::new(&mut stream).poll_next(&mut cx) {
            Poll::Pending => {}
            Poll::Ready(item) => panic!(
                "expected Poll::Pending after discarding only-malformed lines with no valid request behind them, got Some={}",
                item.is_some()
            ),
        }
    }

    /// Minimal `tracing::Subscriber` that counts WARN-level events on the calling
    /// thread, so a test can assert how many warnings `bounded_request_stream` logged
    /// without pulling in a tracing-capture crate for one assertion. Install via
    /// `tracing::subscriber::set_default`, which is thread-local; `#[tokio::test]` uses
    /// a current-thread runtime by default, so the whole test body — including every
    /// `.await` — runs on the thread the guard was set on.
    struct WarnCounter(Arc<AtomicUsize>);

    impl tracing::Subscriber for WarnCounter {
        fn enabled(&self, metadata: &tracing::Metadata<'_>) -> bool {
            *metadata.level() == tracing::Level::WARN
        }

        fn new_span(&self, _span: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }

        fn record(&self, _span: &tracing::span::Id, _values: &tracing::span::Record<'_>) {}

        fn record_follows_from(&self, _span: &tracing::span::Id, _follows: &tracing::span::Id) {}

        fn event(&self, event: &tracing::Event<'_>) {
            if *event.metadata().level() == tracing::Level::WARN {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        fn enter(&self, _span: &tracing::span::Id) {}

        fn exit(&self, _span: &tracing::span::Id) {}
    }

    #[tokio::test]
    async fn blank_lines_are_skipped_without_a_warn_log() {
        // Bare newlines, a whitespace-only line, and a CRLF blank line must all be
        // dropped without a `tracing::warn!` (issue #284, same amplification class as
        // #275/#282) — unlike a malformed line, they must not surface a warning at all.
        let warn_count = Arc::new(AtomicUsize::new(0));
        let _tracing_guard = tracing::subscriber::set_default(WarnCounter(Arc::clone(&warn_count)));

        let script = Script {
            chunks: vec![
                b"\n".to_vec(),
                b"   \n".to_vec(),
                b"\r\n".to_vec(),
                valid_notification_line(),
            ],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        assert!(
            stream.next().await.is_some(),
            "the valid line must decode with no item emitted for the blank lines before it"
        );
        assert!(stream.next().await.is_none());
        assert_eq!(
            warn_count.load(Ordering::SeqCst),
            0,
            "blank lines must not produce a warn!-level log record"
        );
    }

    #[tokio::test]
    async fn malformed_non_blank_line_still_warns() {
        // Contrast case for `blank_lines_are_skipped_without_a_warn_log`: a genuinely
        // malformed (non-blank) line must still warn, so the blank-line fix doesn't
        // silently swallow real diagnostics too.
        let warn_count = Arc::new(AtomicUsize::new(0));
        let _tracing_guard = tracing::subscriber::set_default(WarnCounter(Arc::clone(&warn_count)));

        let mut one_chunk = b"not valid json\n".to_vec();
        one_chunk.extend(valid_notification_line());
        let script = Script {
            chunks: vec![one_chunk],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        assert!(stream.next().await.is_some());
        assert!(stream.next().await.is_none());
        assert_eq!(
            warn_count.load(Ordering::SeqCst),
            1,
            "a genuinely malformed line must still be warned about"
        );
    }

    /// Stand-in for a strictly-typed, no-catch-all string enum embedded somewhere in a
    /// JSON-RPC message's params (e.g. `RxJsonRpcMessage<RoleServer>`'s own
    /// `logging/setLevel` request carries exactly this shape via `LoggingLevel`). Both
    /// `RxJsonRpcMessage<RoleServer>`'s top-level `JsonRpcMessage` and its request/
    /// notification payload types (`ClientRequest`/`ClientNotification`) are
    /// `#[serde(untagged)]` in the pinned `rmcp` 3.1.2, so a mismatch inside a *known*
    /// method's params falls through to a `Custom*` catch-all instead of surfacing the
    /// inner variant's real error -- see [`SanitizedCodecError`]'s doc comment. That
    /// makes it impossible to force a `Serde` error carrying attacker-controlled text
    /// through the full type today; this narrower local type reproduces the same class
    /// of `serde_json` error `JsonRpcMessageCodecError::Serde` wraps verbatim regardless
    /// of which concrete type triggers it, so the test targets `SanitizedCodecError`
    /// itself rather than depending on `rmcp`'s current shapes.
    #[derive(Debug, serde::Deserialize)]
    #[serde(rename_all = "lowercase")]
    enum StrictLevel {
        Debug,
        Info,
    }

    /// `serde_json`'s `unknown variant` error interpolates the offending value verbatim
    /// via `Display`, not `Debug` -- unlike most of its other error variants, it does
    /// not escape embedded control characters. A hostile value can therefore carry a
    /// literal newline straight into a [`JsonRpcMessageCodecError::Serde`]'s own
    /// `Display` output -- confirmed below on the *unsanitized* error first.
    /// `SanitizedCodecError` strips every control character from that same text and
    /// caps its length, as defense-in-depth against a line reported through this path
    /// forging additional log lines in the text log format -- see its doc comment for
    /// why the pinned `rmcp` version does not exercise this today (#415).
    #[test]
    fn sanitized_codec_error_neutralizes_control_characters_and_truncates() {
        let hostile_line = "\"bogus\\nWARN forged log line\\u0007\\u001b[31mred\\u001b[0m\"";
        let error = serde_json::from_str::<StrictLevel>(hostile_line)
            .expect_err("an unrecognized level must fail to deserialize");
        let raw = error.to_string();
        assert!(
            raw.contains('\n'),
            "premise check: the unsanitized error must embed the raw newline verbatim, got: {raw:?}"
        );

        let reason = JsonRpcMessageCodecError::Serde(error);
        let sanitized = SanitizedCodecError(&reason).to_string();

        assert!(
            sanitized.chars().all(|c| !c.is_control()),
            "sanitized reason must contain no control characters, got: {sanitized:?}"
        );
        assert!(
            sanitized.chars().count() <= MAX_UNTRUSTED_FIELD_LEN,
            "sanitized reason must be capped at MAX_UNTRUSTED_FIELD_LEN chars, got {} chars",
            sanitized.chars().count()
        );
        assert!(
            sanitized.contains("WARN forged log line"),
            "sanitization must preserve the surrounding diagnostic text, got: {sanitized:?}"
        );
    }

    /// A hostile value long enough that the sanitized, truncated reason must actually be
    /// shorter than the raw `Display` output -- otherwise `MAX_UNTRUSTED_FIELD_LEN` above
    /// could pass merely because the input never exceeded it.
    #[test]
    fn sanitized_codec_error_truncation_actually_engages() {
        let hostile_line = format!("\"bogus\\n{}\"", "a".repeat(MAX_UNTRUSTED_FIELD_LEN * 2));
        let error = serde_json::from_str::<StrictLevel>(&hostile_line)
            .expect_err("an unrecognized, oversized level must fail to deserialize");
        let raw_len = error.to_string().chars().count();

        let reason = JsonRpcMessageCodecError::Serde(error);
        let sanitized = SanitizedCodecError(&reason).to_string();

        assert!(sanitized.chars().count() < raw_len);
        assert_eq!(sanitized.chars().count(), MAX_UNTRUSTED_FIELD_LEN);
    }

    #[tokio::test]
    async fn splits_whitespace_only_line_across_reads_without_panicking() {
        // Regression for the read-only-peek design itself: `peek_blank_line` must only ever
        // read `buf`, never mutate it, so a whitespace-only line split across two separate
        // `poll_read` calls cannot desync the resumable `blank_scan_from` cursor from the
        // inner codec's own line-scan state (the introspector's symmetric fix, #282, had a
        // panic regression here from an earlier version that advanced the buffer directly).
        let script = Script {
            chunks: vec![
                b"          ".to_vec(), // 10 spaces, no newline yet
                b"\n".to_vec(),
                valid_notification_line(),
            ],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        assert!(
            stream.next().await.is_some(),
            "the valid line must still decode after a blank line split across two reads"
        );
        assert!(stream.next().await.is_none());
    }

    #[tokio::test]
    async fn warns_for_malformed_line_immediately_after_oversized_discard() {
        // Regression for `assume_mid_discard`: right after the inner codec finishes
        // discarding an oversized line, the bytes now at the front of `buf` are the *next*
        // line, not a genuine blank one — but a blank-line peek computed and trusted in the
        // same call (ignoring `assume_mid_discard`) could see the discard's own terminating
        // `\n` at offset 0 and misattribute the next line's own parse error to blank-line
        // suppression, silently swallowing its warning even though no message was lost.
        // Both the oversized-run warning and the malformed-line warning must be reported.
        let oversized_whitespace_run = vec![b' '; TEST_MAX * 3]; // no trailing newline
        let mut rest = b"\nnot json at all\n".to_vec();
        rest.extend(valid_notification_line());
        let script = Script {
            chunks: vec![oversized_whitespace_run, rest],
            idx: 0,
        };

        let warn_count = Arc::new(AtomicUsize::new(0));
        let _tracing_guard = tracing::subscriber::set_default(WarnCounter(Arc::clone(&warn_count)));
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        assert!(
            stream.next().await.is_some(),
            "the valid notification after the malformed line must still decode"
        );
        assert!(stream.next().await.is_none());
        assert_eq!(
            warn_count.load(Ordering::SeqCst),
            2,
            "one warning for the oversized run and one for the malformed line that follows \
             it -- neither is blank, so neither may be suppressed"
        );
    }

    #[tokio::test]
    async fn ignored_notification_and_valid_request_in_one_chunk_do_not_stall() {
        // A second, distinct entrance to the same #273 stall class: a well-formed JSON
        // line that rmcp's own compatibility layer silently ignores as non-standard
        // (`try_parse_with_compatibility` in `async_rw.rs` returns `Ok(None)` after
        // consuming the line) also left `tokio_util`'s `is_readable` cleared, since
        // `FramedImpl` clears it on *every* `Ok(None)`, not only after a `Decoder::Err`.
        // No oversized or malformed data is needed to trigger it. `{"jsonrpc":"1.0",...}`
        // has a JSON-RPC version rmcp's compatibility handling does not recognize as a
        // standard request, so it is consumed and dropped rather than decoded.
        let mut one_chunk = br#"{"jsonrpc":"1.0","method":"foo"}"#.to_vec();
        one_chunk.push(b'\n');
        one_chunk.extend(valid_request_line(1));
        let script = ScriptThenIdle {
            chunks: vec![one_chunk],
            idx: 0,
        };
        let mut stream = bounded_request_stream(script, TEST_MAX, semaphore(TEST_CONCURRENCY));

        let message = next_or_timeout(&mut stream)
            .await
            .expect("the valid request sharing a chunk with an ignored non-standard line must decode without waiting for more input");
        assert_eq!(request_id(&message), 1);
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

    // ── #399: `--log-format`/`MCP_EXECUTION_LOG_FORMAT` wiring ──

    #[test]
    fn test_server_args_log_format_default_unset() {
        let args = ServerArgs::parse_from(["mcp-execution"]);
        assert_eq!(args.log_format, None);
    }

    #[test]
    fn test_server_args_log_format_json_parses() {
        let args = ServerArgs::parse_from(["mcp-execution", "--log-format", "json"]);
        assert_eq!(args.log_format, Some(LogFormat::Json));
    }

    #[test]
    fn test_server_args_log_format_case_insensitive() {
        let args = ServerArgs::parse_from(["mcp-execution", "--log-format", "JSON"]);
        assert_eq!(args.log_format, Some(LogFormat::Json));
    }

    #[test]
    fn test_server_args_log_format_invalid_rejected_by_clap() {
        let result = ServerArgs::try_parse_from(["mcp-execution", "--log-format", "xml"]);
        assert!(result.is_err());
    }

    #[test]
    fn test_server_args_help_documents_env_var() {
        use clap::CommandFactory;

        let mut command = ServerArgs::command();
        let help = command.render_long_help().to_string();
        assert!(
            help.contains(LOG_FORMAT_ENV_VAR),
            "--help must document {LOG_FORMAT_ENV_VAR} per FR-004"
        );
    }

    #[test]
    fn test_server_args_command_name_matches_installed_binary() {
        use clap::CommandFactory;

        // `CARGO_PKG_NAME` is `mcp-execution-server`, but the installed binary (see
        // `crates/mcp-server/Cargo.toml`'s `[[bin]]`) is `mcp-execution` -- `#[command(name =
        // ...)]` on `ServerArgs` must override clap's default so `--help`/error output names
        // the binary users actually run.
        assert_eq!(ServerArgs::command().get_name(), "mcp-execution");
    }

    /// Serializes tests in this module that mutate `MCP_EXECUTION_LOG_FORMAT`, mirroring the
    /// `HOME_ENV_LOCK`/`BACKTRACE_ENV_LOCK` pattern `mcp-cli`'s tests already use for
    /// env-var mutation: a safety net for plain `cargo test` (which shares one process across a
    /// crate's tests), not required by the mandated `cargo nextest run` (which isolates every
    /// test in its own process).
    static LOG_FORMAT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn resolve_log_format_flag_wins_over_bad_env() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "xml");
        }

        let args = ServerArgs {
            log_format: Some(LogFormat::Json),
        };
        let format = resolve_log_format(&args);
        let is_invalid = log_format_env_is_invalid(&args);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert_eq!(format, LogFormat::Json);
        assert!(
            !is_invalid,
            "a bad env value must not be reported once the flag has already decided"
        );
    }

    /// Proves the environment variable is actually consulted end to end -- not just that
    /// `LogFormat::resolve` itself works in isolation: a `resolve_log_format` that forgot to
    /// call `std::env::var(LOG_FORMAT_ENV_VAR)` would still pass every test that only exercises
    /// the pure resolver directly with a hand-built `Option<&str>`.
    #[test]
    fn resolve_log_format_reads_env_var_when_flag_unset() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "json");
        }

        let args = ServerArgs { log_format: None };
        let format = resolve_log_format(&args);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert_eq!(format, LogFormat::Json);
    }

    #[test]
    fn resolve_log_format_bad_env_value_falls_back_to_text() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "xml");
        }

        let args = ServerArgs { log_format: None };
        let format = resolve_log_format(&args);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert_eq!(format, LogFormat::Text);
    }

    /// Proves `log_format_env_is_invalid` -- the function that actually gates
    /// `warn_on_rejected_log_format` in `main` -- reads the real environment variable itself,
    /// not just that `LogFormat::is_invalid_env_value` works given a hand-built `&str`.
    #[test]
    fn log_format_env_is_invalid_true_for_bad_value_when_flag_unset() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "xml");
        }

        let args = ServerArgs { log_format: None };
        let is_invalid = log_format_env_is_invalid(&args);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert!(is_invalid);
    }

    #[test]
    fn log_format_env_is_invalid_false_for_valid_value() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "json");
        }

        let args = ServerArgs { log_format: None };
        let is_invalid = log_format_env_is_invalid(&args);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert!(!is_invalid);
    }

    #[test]
    fn log_format_env_is_invalid_false_when_unset() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::remove_var(LOG_FORMAT_ENV_VAR);
        }

        let args = ServerArgs { log_format: None };
        let is_invalid = log_format_env_is_invalid(&args);

        // SAFETY: see above.
        unsafe {
            if let Some(v) = &original {
                std::env::set_var(LOG_FORMAT_ENV_VAR, v);
            }
        }

        assert!(!is_invalid);
    }

    #[test]
    fn warn_on_rejected_log_format_fires_for_a_rejected_value() {
        let warn_count = Arc::new(AtomicUsize::new(0));
        let _tracing_guard = tracing::subscriber::set_default(WarnCounter(Arc::clone(&warn_count)));

        warn_on_rejected_log_format(true);

        assert_eq!(warn_count.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn warn_on_rejected_log_format_silent_when_nothing_was_rejected() {
        let warn_count = Arc::new(AtomicUsize::new(0));
        let _tracing_guard = tracing::subscriber::set_default(WarnCounter(Arc::clone(&warn_count)));

        warn_on_rejected_log_format(false);

        assert_eq!(warn_count.load(Ordering::SeqCst), 0);
    }

    /// JSON-mode coverage for the `.boxed()` branch `main` takes when `LogFormat::Json` is
    /// selected: wires a real `fmt::layer().json()` into a scoped subscriber and asserts every
    /// emitted line parses via `serde_json` -- the assertion that would have caught the C1
    /// regression (an unescaped `"` left behind when a redacted URL sits inside a
    /// JSON-escaped string; not exercised directly here since this binary has no
    /// `RedactingWriter`, but the JSON-validity property must hold regardless).
    #[test]
    fn json_log_format_emits_serde_json_parseable_lines() {
        use std::sync::Mutex;

        #[derive(Clone)]
        struct SharedBuf(Arc<Mutex<Vec<u8>>>);

        impl std::io::Write for SharedBuf {
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
                std::io::Write::write(&mut *self.0.lock().unwrap(), buf)
            }

            fn flush(&mut self) -> std::io::Result<()> {
                std::io::Write::flush(&mut *self.0.lock().unwrap())
            }
        }

        let buf = Arc::new(Mutex::new(Vec::new()));
        let make_writer = {
            let buf = buf.clone();
            move || SharedBuf(buf.clone())
        };

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(make_writer)
                .with_target(true)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::info!("Starting mcp-execution v0.0.0-test");
            tracing::warn!(
                "invalid value for MCP_EXECUTION_LOG_FORMAT (expected 'text' or 'json'), falling back to text"
            );
        });

        let written = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        let lines: Vec<&str> = written.lines().filter(|line| !line.is_empty()).collect();
        assert_eq!(lines.len(), 2, "expected two emitted lines, got: {written}");
        for line in lines {
            serde_json::from_str::<serde_json::Value>(line)
                .unwrap_or_else(|e| panic!("invalid JSON line: {e}\n{line}"));
        }
    }
}
