//! The helper server: the runner-side end of the helper protocol.
//!
//! [`HelperServer::bind`] opens the per-run channel and returns an endpoint to hand to the
//! script process plus a handle to finish the run with. `serve` is the protocol loop shared
//! with the hidden `helper` command: hello and version check, bounded request lines, calls
//! through the [`SessionPool`] with bounded concurrency, responses matched by id. A journal and a
//! dry run, when configured in [`HelperConfig`], wrap every call.

use std::future::Future;
use std::path::PathBuf;
use std::process::Stdio;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result, bail};
#[cfg(unix)]
use mcp_execution_core::helper_protocol::HELPER_ENV;
#[cfg(windows)]
use mcp_execution_core::helper_protocol::HELPER_TOKEN_ENV;
use mcp_execution_core::helper_protocol::{
    CLI_PATH_ENV, HELPER_HELLO_TIMEOUT, HELPER_PROTOCOL_VERSION, MAX_HELPER_LINE_BYTES,
};
use mcp_execution_introspector::CallTimeout;
use serde::Deserialize;
use serde_json::Value;
use tokio::io::{AsyncBufRead, AsyncRead, AsyncWrite, AsyncWriteExt as _, BufReader};
use tokio::sync::{OwnedSemaphorePermit, Semaphore, mpsc, oneshot};
use tokio::task::{JoinHandle, JoinSet};

use super::journal::{CallStatus, Journal, canonical_args};
use super::path_lookup::find_on_path;
use super::policy::{DryRun, Policy};
use super::pool::{CallReply, MAX_INFLIGHT_CALLS, SessionPool};
use super::protocol::{
    ErrorBody, ErrorKind, Hello, HelloReply, Line, Request, Response, ResponseBody,
    encode_response, read_line,
};

/// Upper bound of the per-call timeout. `run` passes the smaller of its run timeout and this
/// value (fixed when the run starts); the `helper` command and `call` use it as is.
pub const DEFAULT_CALL_TIMEOUT: Duration = Duration::from_secs(60);

/// How long `finish` waits for the channel task to stop before aborting it.
const FINISH_GRACE: Duration = Duration::from_secs(2);

/// Longest accepted hello line.
const MAX_HELLO_BYTES: usize = 4096;

/// Finished responses waiting for the writer.
const RESPONSE_QUEUE: usize = 64;

/// A response queued for the writer; the permit of the call that produced it is released only
/// once the line is written, so a script that never reads its socket cannot make the runner
/// accumulate work.
type Outgoing = (Response, Option<OwnedSemaphorePermit>);

/// How the channel reaches the script process.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::helper::Carrier;
///
/// assert_ne!(Carrier::Fd0Pair, Carrier::NamedPipe);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carrier {
    /// Unix: a socketpair whose child end becomes the script's file descriptor 0.
    Fd0Pair,
    /// Windows: a single-instance named pipe, guarded by a per-run token.
    NamedPipe,
}

impl Carrier {
    /// The carrier for the current platform.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::helper::Carrier;
    ///
    /// let carrier = Carrier::native();
    /// assert!(cfg!(windows) == (carrier == Carrier::NamedPipe));
    /// ```
    #[must_use]
    pub const fn native() -> Self {
        if cfg!(windows) {
            Self::NamedPipe
        } else {
            Self::Fd0Pair
        }
    }
}

/// Whether the helper executes every call or only the ones a policy classes as reads.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::helper::DryRunMode;
/// use mcp_execution_cli::runtime::policy::Policy;
///
/// assert!(matches!(DryRunMode::default(), DryRunMode::Off));
/// let _on = DryRunMode::On(Policy::default());
/// ```
#[derive(Debug, Clone, Default)]
pub enum DryRunMode {
    /// Every call is executed.
    #[default]
    Off,
    /// Only calls the policy classes as reads are executed; the rest are skipped and reported.
    On(Policy),
}

/// Settings of one helper server.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::helper::{DryRunMode, HelperConfig};
/// use mcp_execution_introspector::CallTimeout;
/// use std::time::Duration;
///
/// let config = HelperConfig {
///     servers_dir: "/home/user/.claude/servers".into(),
///     call_timeout: CallTimeout::new(Duration::from_secs(60)).unwrap(),
///     journal: None,
///     dry_run: DryRunMode::Off,
/// };
/// assert_eq!(config.call_timeout.get().as_secs(), 60);
/// ```
#[derive(Debug, Clone)]
pub struct HelperConfig {
    /// Directory of generated servers. The preload resolves `mcp:<id>` imports against it; the
    /// helper reads each server's recorded `_meta.json` there for dry-run suggestions and
    /// resolves servers themselves through `~/.claude/mcp.json`.
    pub servers_dir: PathBuf,
    /// Per-call timeout for the whole run (`run` passes `min(run timeout, DEFAULT_CALL_TIMEOUT)`).
    pub call_timeout: CallTimeout,
    /// Journal every call (value-free); the summary comes back in [`RunCallReport`].
    pub journal: Option<Arc<Journal>>,
    /// Dry-run mode; skipped writes are reported in [`RunCallReport`].
    pub dry_run: DryRunMode,
}

/// What the script process needs to reach the helper.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::helper::{Carrier, HelperConfig, HelperServer};
///
/// # fn example(config: HelperConfig) -> anyhow::Result<()> {
/// let (_handle, endpoint) = HelperServer::bind(Carrier::native(), config)?;
/// for (name, value) in endpoint.env() {
///     println!("{name}={value}");
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct HelperEndpoint {
    env: Vec<(&'static str, String)>,
    stdin: Option<Stdio>,
}

impl HelperEndpoint {
    /// Environment variables to set on the script process.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::helper::HelperEndpoint;
    /// # fn example(endpoint: &HelperEndpoint) {
    /// assert!(!endpoint.env().is_empty());
    /// # }
    /// ```
    #[must_use]
    pub fn env(&self) -> Vec<(&'static str, String)> {
        self.env.clone()
    }

    /// The script's stdin: the channel's child end on the Unix carrier, `None` on Windows or
    /// once taken.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::helper::HelperEndpoint;
    /// # fn example(endpoint: &mut HelperEndpoint) {
    /// let _stdin = endpoint.take_stdin();
    /// # }
    /// ```
    #[must_use]
    pub const fn take_stdin(&mut self) -> Option<Stdio> {
        self.stdin.take()
    }
}

/// Where a generated tool run outside `run` finds the `mcp-execution-cli` binary it starts as
/// its helper.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::helper::CliBinary;
///
/// assert_ne!(CliBinary::NotFound, CliBinary::OverrideInvalid);
/// ```
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
#[serde(tag = "state", content = "path", rename_all = "snake_case")]
pub enum CliBinary {
    /// The binary that would be started.
    Found(PathBuf),
    /// `MCP_EXECUTION_CLI` is set but is not an absolute path to an executable file. A set
    /// override never falls back to `PATH`, which would run a binary other than the one named.
    OverrideInvalid,
    /// No override is set and no `mcp-execution-cli` is on `PATH`.
    NotFound,
}

/// Finds the `mcp-execution-cli` binary a generated tool starts as its helper outside `run`.
///
/// `MCP_EXECUTION_CLI` wins when set and must be an absolute path to an executable file;
/// otherwise the first `mcp-execution-cli` on `PATH` is used. No shell is involved.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::helper::{CliBinary, discover_cli_binary};
///
/// if let CliBinary::Found(path) = discover_cli_binary() {
///     println!("{}", path.display());
/// }
/// ```
#[must_use]
pub fn discover_cli_binary() -> CliBinary {
    std::env::var_os(CLI_PATH_ENV).map_or_else(
        || {
            find_on_path("mcp-execution-cli", &[".exe"])
                .map_or(CliBinary::NotFound, CliBinary::Found)
        },
        |value| {
            let path = PathBuf::from(value);
            if path.is_absolute() && is_executable_file(&path) {
                CliBinary::Found(path)
            } else {
                CliBinary::OverrideInvalid
            }
        },
    )
}

fn is_executable_file(path: &std::path::Path) -> bool {
    let Ok(meta) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        meta.is_file() && meta.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        meta.is_file()
    }
}

/// Per-call hooks of a run: journal entries around every call and the dry-run decision.
#[derive(Debug, Default)]
pub(crate) struct CallHooks {
    journal: Option<Arc<Journal>>,
    dry_run: Option<Arc<DryRun>>,
}

impl CallHooks {
    fn new(
        journal: Option<Arc<Journal>>,
        dry_run: DryRunMode,
        servers_dir: &std::path::Path,
    ) -> Self {
        Self {
            journal,
            dry_run: match dry_run {
                DryRunMode::Off => None,
                DryRunMode::On(policy) => {
                    Some(Arc::new(DryRun::new(policy, servers_dir.to_path_buf())))
                }
            },
        }
    }

    /// Makes one call through `pool`, journaling it and skipping writes under a dry run.
    async fn execute(&self, pool: &SessionPool, request: &Request) -> ResponseBody {
        if self.journal.is_none() && self.dry_run.is_none() {
            return pool.call(request).await.body;
        }
        let arguments = Value::Object(request.arguments.clone());
        let (digest, size) = self.journal.as_ref().map_or_else(
            || (None, canonical_args(&arguments).len()),
            |journal| {
                let (digest, size) = journal.digest(&arguments);
                (Some(digest), size)
            },
        );
        let started = Instant::now();
        let token =
            self.journal.as_ref().zip(digest).map(|(journal, digest)| {
                journal.start(&request.server, &request.tool, digest, size)
            });
        let skipped = self
            .dry_run
            .as_ref()
            .is_some_and(|dry| dry.skips(&request.server, &request.tool, digest, size));
        let reply = if skipped {
            CallReply {
                body: ResponseBody::Error {
                    error: ErrorBody {
                        kind: ErrorKind::WriteSkipped,
                        message: format!(
                            "Skipped by --dry-run: {}.{} is not listed as a read in the policy",
                            request.server, request.tool
                        ),
                        server_message: None,
                        server_message_truncated: None,
                    },
                },
                status: CallStatus::Skipped,
            }
        } else {
            pool.call(request).await
        };
        if let (Some(journal), Some(token)) = (&self.journal, token) {
            let result_bytes = match &reply.body {
                ResponseBody::Value { value } => {
                    serde_json::to_string(value).map_or(0, |s| s.len())
                }
                _ => 0,
            };
            journal.end(token, reply.status, started.elapsed(), result_bytes);
        }
        reply.body
    }

    async fn report(&self) -> RunCallReport {
        RunCallReport {
            journal_summary: match &self.journal {
                Some(journal) => journal.finish().await,
                None => None,
            },
            dry_run_report: match &self.dry_run {
                // The report reads `_meta.json` files with blocking I/O.
                Some(dry_run) => {
                    let dry_run = Arc::clone(dry_run);
                    tokio::task::spawn_blocking(move || dry_run.report())
                        .await
                        .unwrap_or_else(|error| {
                            tracing::warn!(%error, "dry-run report task failed");
                            None
                        })
                }
                None => None,
            },
        }
    }
}

/// What the helper observed during a run, for the runner to report.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::helper::RunCallReport;
///
/// assert!(RunCallReport::default().journal_summary.is_none());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RunCallReport {
    /// Failure-only call summary from the journal (#572).
    pub journal_summary: Option<String>,
    /// Dry-run report of skipped writes (#573).
    pub dry_run_report: Option<String>,
}

/// The running helper; finish it when the script process ends.
///
/// # Examples
///
/// ```no_run
/// # use mcp_execution_cli::runtime::helper::HelperHandle;
/// # async fn example(handle: HelperHandle) {
/// let report = handle.finish().await;
/// println!("{:?}", report.journal_summary);
/// # }
/// ```
#[derive(Debug)]
pub struct HelperHandle {
    task: JoinHandle<()>,
    shutdown: oneshot::Sender<()>,
    pool: Arc<SessionPool>,
    hooks: Arc<CallHooks>,
}

impl HelperHandle {
    /// Stops serving, cancels calls still in flight, closes the session pool (killing the
    /// server processes it started) and returns the run's call report.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # use mcp_execution_cli::runtime::helper::HelperHandle;
    /// # async fn example(handle: HelperHandle) {
    /// let _report = handle.finish().await;
    /// # }
    /// ```
    pub async fn finish(self) -> RunCallReport {
        if self.shutdown.send(()).is_err() {
            tracing::debug!("helper loop had already ended");
        }
        let mut task = self.task;
        match tokio::time::timeout(FINISH_GRACE, &mut task).await {
            Ok(Ok(())) => {}
            Ok(Err(error)) => tracing::warn!(%error, "helper task ended abnormally"),
            Err(_elapsed) => {
                tracing::warn!("helper task did not stop within the grace period; aborting it");
                task.abort();
            }
        }
        self.pool.close().await;
        self.hooks.report().await
    }
}

/// Entry point for opening the helper channel.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::helper::{Carrier, HelperConfig, HelperServer};
///
/// # fn example(config: HelperConfig) -> anyhow::Result<()> {
/// let (_handle, _endpoint) = HelperServer::bind(Carrier::native(), config)?;
/// # Ok(())
/// # }
/// ```
#[derive(Debug)]
pub struct HelperServer;

impl HelperServer {
    /// Opens the channel for `carrier` and starts serving it. Must be called inside a Tokio
    /// runtime. One connection is served per run and the channel is never re-created.
    ///
    /// # Errors
    ///
    /// Returns an error when the channel cannot be created or `carrier` is not supported on
    /// this platform.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::helper::{Carrier, HelperConfig, HelperServer};
    ///
    /// # fn example(config: HelperConfig) -> anyhow::Result<()> {
    /// let (_handle, _endpoint) = HelperServer::bind(Carrier::native(), config)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn bind(carrier: Carrier, config: HelperConfig) -> Result<(HelperHandle, HelperEndpoint)> {
        let pool = Arc::new(SessionPool::new(config.call_timeout));
        let hooks = Arc::new(CallHooks::new(
            config.journal,
            config.dry_run,
            &config.servers_dir,
        ));
        let (shutdown, stopped) = oneshot::channel();
        // A dropped handle also stops the loop.
        let stop = async move {
            match stopped.await {
                Ok(()) | Err(_) => {}
            }
        };
        let (task, endpoint) = match carrier {
            Carrier::Fd0Pair => bind_fd0(Arc::clone(&pool), Arc::clone(&hooks), stop)?,
            Carrier::NamedPipe => bind_pipe(Arc::clone(&pool), Arc::clone(&hooks), stop)?,
        };
        Ok((
            HelperHandle {
                task,
                shutdown,
                pool,
                hooks,
            },
            endpoint,
        ))
    }
}

#[cfg(unix)]
fn bind_fd0(
    pool: Arc<SessionPool>,
    hooks: Arc<CallHooks>,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(JoinHandle<()>, HelperEndpoint)> {
    let (server, child) = tokio::net::UnixStream::pair().context("cannot create the channel")?;
    let child = child.into_std().context("cannot detach the child end")?;
    // The child end is the script's stdin; it must not stay in tokio's non-blocking mode.
    child
        .set_nonblocking(false)
        .context("cannot make the child end blocking")?;
    let stdin = Stdio::from(std::os::fd::OwnedFd::from(child));
    let task = tokio::spawn(async move {
        let (reader, writer) = tokio::io::split(server);
        if let Err(error) = serve(reader, writer, pool, hooks, None, stop).await {
            tracing::warn!(error = %format!("{error:#}"), "helper channel closed");
        }
    });
    let endpoint = HelperEndpoint {
        env: vec![(HELPER_ENV, "fd:0".to_string())],
        stdin: Some(stdin),
    };
    Ok((task, endpoint))
}

#[cfg(not(unix))]
fn bind_fd0(
    _pool: Arc<SessionPool>,
    _hooks: Arc<CallHooks>,
    _stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(JoinHandle<()>, HelperEndpoint)> {
    bail!("the file-descriptor carrier is not supported on this platform")
}

/// The `MCP_EXECUTION_HELPER` value for a pipe: `pipe:` followed by the full pipe path, the form
/// the preload and the bridge accept (the Unix carrier uses `fd:0`).
#[cfg(any(windows, test))]
fn pipe_descriptor(name: &str) -> String {
    format!("pipe:{name}")
}

/// Name of a fresh per-run named pipe, `\\.\pipe\mcp-execution-<uuid>`.
#[cfg(any(windows, test))]
fn new_pipe_name() -> String {
    format!(r"\\.\pipe\mcp-execution-{}", uuid::Uuid::new_v4().simple())
}

/// Windows carrier: a single-instance pipe, rejecting remote clients, guarded by a per-run
/// token.
///
/// Residual: the pipe keeps the default security descriptor, so another local user can open
/// the single instance first and make this run's calls fail (denial of service only; the
/// token still blocks the hello). Restricting it to the current user needs a raw security
/// descriptor, which means FFI and conflicts with `unsafe_code = deny`.
#[cfg(windows)]
fn bind_pipe(
    pool: Arc<SessionPool>,
    hooks: Arc<CallHooks>,
    stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(JoinHandle<()>, HelperEndpoint)> {
    use mcp_execution_core::helper_protocol::HELPER_ENV;
    use tokio::net::windows::named_pipe::ServerOptions;

    let name = new_pipe_name();
    let token = uuid::Uuid::new_v4().simple().to_string();
    let pipe = ServerOptions::new()
        .first_pipe_instance(true)
        .reject_remote_clients(true)
        .max_instances(1)
        .create(&name)
        .context("cannot create the named pipe")?;
    let expected = token.clone();
    let task = tokio::spawn(async move {
        let mut stop = Box::pin(stop);
        // `finish` must not wait for a client that never connects.
        let accepted = tokio::select! {
            biased;
            () = &mut stop => return,
            accepted = pipe.connect() => accepted,
        };
        if let Err(error) = accepted {
            tracing::warn!(%error, "helper pipe accept failed");
            return;
        }
        let (reader, writer) = tokio::io::split(pipe);
        if let Err(error) = serve(reader, writer, pool, hooks, Some(expected), stop).await {
            tracing::warn!(error = %format!("{error:#}"), "helper channel closed");
        }
    });
    let endpoint = HelperEndpoint {
        env: vec![
            (HELPER_ENV, pipe_descriptor(&name)),
            (HELPER_TOKEN_ENV, token),
        ],
        stdin: None,
    };
    Ok((task, endpoint))
}

#[cfg(not(windows))]
fn bind_pipe(
    _pool: Arc<SessionPool>,
    _hooks: Arc<CallHooks>,
    _stop: impl Future<Output = ()> + Send + 'static,
) -> Result<(JoinHandle<()>, HelperEndpoint)> {
    bail!("the named-pipe carrier is only supported on Windows")
}

/// Serves one helper connection until the client disconnects, sends a bad line or `stop`
/// resolves. Calls still in flight at the end are cancelled.
///
/// `token` is the per-run secret a Windows client must repeat in its hello; `None` on a
/// carrier that needs none.
///
/// # Errors
///
/// Returns an error when the hello is missing, late, malformed, of another protocol version
/// or carries a wrong token; the channel is then closed without a reply.
pub(crate) async fn serve<R, W>(
    reader: R,
    mut writer: W,
    pool: Arc<SessionPool>,
    hooks: Arc<CallHooks>,
    token: Option<String>,
    stop: impl Future<Output = ()> + Send,
) -> Result<()>
where
    R: AsyncRead + Unpin + Send,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut reader = BufReader::new(reader);
    tokio::pin!(stop);
    tokio::select! {
        biased;
        () = &mut stop => return Ok(()),
        hello = handshake(&mut reader, &mut writer, token.as_deref()) => hello?,
    }

    let (responses, queued) = mpsc::channel(RESPONSE_QUEUE);
    let writer_task = tokio::spawn(write_loop(writer, queued));
    let permits = Arc::new(Semaphore::new(MAX_INFLIGHT_CALLS));
    let mut calls = JoinSet::new();

    loop {
        // Reaped here, not as a select branch: `read_line` keeps a partly read line in a local
        // buffer, so cancelling it would drop that prefix.
        while calls.try_join_next().is_some() {}
        let line = tokio::select! {
            biased;
            () = &mut stop => break,
            line = read_line(&mut reader, MAX_HELPER_LINE_BYTES) => line,
        };
        let bytes = match line {
            Ok(Line::Text(bytes)) => bytes,
            Ok(Line::Oversized) => {
                let message = format!("request exceeds {MAX_HELPER_LINE_BYTES} bytes");
                reject(&responses, 0, &message).await;
                break;
            }
            Ok(Line::Eof) | Err(_) => break,
        };
        let Ok(request) = serde_json::from_slice::<Request>(&bytes) else {
            let id = serde_json::from_slice::<IdOnly>(&bytes).map_or(0, |only| only.id);
            reject(&responses, id, "malformed request").await;
            break;
        };
        if request.id == 0 {
            reject(&responses, 0, "request id 0 is reserved").await;
            break;
        }
        let Ok(permit) = (tokio::select! {
            biased;
            () = &mut stop => break,
            permit = Arc::clone(&permits).acquire_owned() => permit,
        }) else {
            break;
        };
        let (pool, hooks, responses) = (Arc::clone(&pool), Arc::clone(&hooks), responses.clone());
        calls.spawn(async move {
            let body = hooks.execute(&pool, &request).await;
            let response = Response {
                id: request.id,
                body,
            };
            if responses.send((response, Some(permit))).await.is_err() {
                tracing::debug!("helper channel closed before a response was sent");
            }
        });
    }

    calls.shutdown().await;
    drop(responses);
    if let Err(error) = writer_task.await {
        tracing::debug!(%error, "helper writer task ended abnormally");
    }
    Ok(())
}

#[derive(Deserialize)]
struct IdOnly {
    id: u64,
}

async fn handshake<R, W>(reader: &mut R, writer: &mut W, token: Option<&str>) -> Result<()>
where
    R: AsyncBufRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let line = tokio::time::timeout(HELPER_HELLO_TIMEOUT, read_line(reader, MAX_HELLO_BYTES))
        .await
        .context("no hello within the hello timeout")?
        .context("cannot read the hello")?;
    let Line::Text(bytes) = line else {
        bail!("the channel ended or the hello was oversized");
    };
    let hello: Hello = serde_json::from_slice(&bytes).context("malformed hello")?;
    if hello.v != HELPER_PROTOCOL_VERSION {
        bail!(
            "hello speaks protocol version {}, expected {HELPER_PROTOCOL_VERSION}",
            hello.v
        );
    }
    if token.is_some_and(|expected| hello.token.as_deref() != Some(expected)) {
        bail!("hello carries a wrong token");
    }
    let mut reply = serde_json::to_vec(&HelloReply::current())?;
    reply.push(b'\n');
    writer.write_all(&reply).await?;
    writer.flush().await?;
    Ok(())
}

async fn reject(responses: &mpsc::Sender<Outgoing>, id: u64, message: &str) {
    let response = Response {
        id,
        body: ResponseBody::Error {
            error: ErrorBody {
                kind: ErrorKind::CallFailed,
                message: message.to_string(),
                server_message: None,
                server_message_truncated: None,
            },
        },
    };
    if responses.send((response, None)).await.is_err() {
        tracing::debug!("helper channel closed before a rejection was sent");
    }
}

async fn write_loop<W: AsyncWrite + Unpin>(mut writer: W, mut queued: mpsc::Receiver<Outgoing>) {
    while let Some((response, permit)) = queued.recv().await {
        let line = encode_response(&response);
        if let Err(error) = writer.write_all(&line).await {
            tracing::debug!(%error, "helper channel write failed");
            return;
        }
        if let Err(error) = writer.flush().await {
            tracing::debug!(%error, "helper channel flush failed");
            return;
        }
        drop(permit);
    }
    if let Err(error) = writer.shutdown().await {
        tracing::debug!(%error, "helper channel shutdown failed");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime::dirs::{RunId, RuntimeDirs};
    use crate::runtime::protocol::ResultKind;

    fn request(server: &str, tool: &str) -> Request {
        Request {
            id: 1,
            server: mcp_execution_core::ServerId::new(server).unwrap(),
            tool: mcp_execution_core::ToolName::new(tool).unwrap(),
            arguments: serde_json::Map::new(),
            result: ResultKind::Content,
        }
    }

    fn pool() -> SessionPool {
        SessionPool::new(CallTimeout::new(Duration::from_secs(5)).unwrap())
    }

    #[tokio::test]
    async fn a_dry_run_skips_a_write_with_write_skipped_and_reports_it() {
        let hooks = CallHooks::new(
            None,
            DryRunMode::On(Policy::default()),
            std::path::Path::new("/nonexistent"),
        );

        let body = hooks
            .execute(&pool(), &request("no-such-server-for-hooks", "create_item"))
            .await;

        let ResponseBody::Error { error } = body else {
            panic!("expected an error body");
        };
        assert_eq!(error.kind, ErrorKind::WriteSkipped);
        let report = hooks.report().await;
        assert!(report.dry_run_report.unwrap().contains("create_item"));
        assert_eq!(report.journal_summary, None);
    }

    #[tokio::test]
    async fn a_journaled_call_is_summarized_with_its_status() {
        let temp = tempfile::tempdir().unwrap();
        let dirs = RuntimeDirs::from_root(temp.path().to_path_buf());
        let journal = Arc::new(Journal::create(&dirs, &RunId::new()).await.unwrap());
        let hooks = CallHooks::new(
            Some(journal),
            DryRunMode::Off,
            std::path::Path::new("/nonexistent"),
        );

        let body = hooks
            .execute(&pool(), &request("no-such-server-for-hooks", "echo"))
            .await;

        assert!(matches!(body, ResponseBody::Error { .. }));
        let report = hooks.report().await;
        let summary = report.journal_summary.unwrap();
        assert!(
            summary.contains("no-such-server-for-hooks/echo"),
            "{summary}"
        );
        assert!(summary.contains("failed"), "{summary}");
        assert_eq!(report.dry_run_report, None);
    }

    #[test]
    fn the_exported_pipe_descriptor_is_the_form_the_preload_accepts() {
        let descriptor = pipe_descriptor(&new_pipe_name());
        assert!(
            descriptor.starts_with(r"pipe:\\.\pipe\mcp-execution-"),
            "{descriptor}"
        );
        assert!(
            include_str!("preload.js").contains("descriptor.startsWith('pipe:')"),
            "preload.js must accept the pipe: form"
        );
    }

    #[tokio::test]
    async fn finish_returns_promptly_when_no_client_ever_connects() {
        let (handle, _endpoint) = HelperServer::bind(
            Carrier::native(),
            HelperConfig {
                servers_dir: std::env::temp_dir(),
                call_timeout: CallTimeout::new(Duration::from_secs(5)).unwrap(),
                journal: None,
                dry_run: DryRunMode::Off,
            },
        )
        .unwrap();

        let report = tokio::time::timeout(FINISH_GRACE + Duration::from_secs(1), handle.finish())
            .await
            .expect("finish must return within its grace period");

        assert_eq!(report, RunCallReport::default());
    }
}
