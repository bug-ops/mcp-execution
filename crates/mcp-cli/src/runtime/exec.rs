//! Running the script process: spawn, bounded output capture, timeout and exit mapping.
//!
//! Output is read by async tasks that keep at most [`MAX_SPILL_FILE_BYTES`] per stream. After
//! the script ends the readers get [`DRAIN_GRACE`]; a process the script left behind that still
//! holds a pipe cannot keep the runner waiting, its reader is aborted and the loss is reported.

use std::convert::Infallible;
use std::ffi::OsString;
use std::fmt;
use std::future::Future;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result};
use mcp_execution_core::helper_protocol::{HELPER_ENV, HELPER_TOKEN_ENV, RUN_MARKER_ENV};
use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::process::{Child, Command};
use tokio::task::JoinHandle;

use super::OutOfRange;
use super::helper::HelperEndpoint;
use super::signal::RunnerSignal;
use super::spill::MAX_SPILL_FILE_BYTES;
use super::tty::{ForegroundGroup, terminal_cancellation};

/// Shortest accepted run timeout, in seconds.
pub const MIN_RUN_TIMEOUT_SECS: u64 = 1;

/// Longest accepted run timeout, in seconds.
pub const MAX_RUN_TIMEOUT_SECS: u64 = 600;

/// Default run timeout, in seconds.
pub const DEFAULT_RUN_TIMEOUT_SECS: u64 = 60;

/// How long the runner keeps reading a script's output pipes after the script ended.
pub const DRAIN_GRACE: Duration = Duration::from_secs(2);

/// Wall-clock limit for one script run, validated to `1..=600` seconds.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::exec::RunTimeout;
///
/// let timeout: RunTimeout = "30".parse().unwrap();
/// assert_eq!(timeout.get().as_secs(), 30);
/// assert!("0".parse::<RunTimeout>().is_err());
/// assert!("601".parse::<RunTimeout>().is_err());
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RunTimeout(Duration);

impl RunTimeout {
    /// Validates `secs` as a run timeout.
    ///
    /// # Errors
    ///
    /// Returns [`OutOfRange`] when `secs` is outside `1..=600`.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::exec::RunTimeout;
    ///
    /// assert!(RunTimeout::from_secs(600).is_ok());
    /// assert!(RunTimeout::from_secs(601).is_err());
    /// ```
    pub fn from_secs(secs: u64) -> Result<Self, OutOfRange> {
        if (MIN_RUN_TIMEOUT_SECS..=MAX_RUN_TIMEOUT_SECS).contains(&secs) {
            Ok(Self(Duration::from_secs(secs)))
        } else {
            Err(OutOfRange::new(
                "run timeout (seconds)",
                MIN_RUN_TIMEOUT_SECS,
                MAX_RUN_TIMEOUT_SECS,
            ))
        }
    }

    /// The default run timeout.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::exec::RunTimeout;
    ///
    /// assert_eq!(RunTimeout::default_timeout().get().as_secs(), 60);
    /// ```
    #[must_use]
    pub const fn default_timeout() -> Self {
        Self(Duration::from_secs(DEFAULT_RUN_TIMEOUT_SECS))
    }

    /// The validated duration.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::exec::RunTimeout;
    ///
    /// assert_eq!(RunTimeout::from_secs(5).unwrap().get().as_secs(), 5);
    /// ```
    #[must_use]
    pub const fn get(self) -> Duration {
        self.0
    }
}

impl fmt::Display for RunTimeout {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.as_secs().fmt(f)
    }
}

impl FromStr for RunTimeout {
    type Err = OutOfRange;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        raw.parse::<u64>()
            .map_err(|_| {
                OutOfRange::new(
                    "run timeout (seconds)",
                    MIN_RUN_TIMEOUT_SECS,
                    MAX_RUN_TIMEOUT_SECS,
                )
            })
            .and_then(Self::from_secs)
    }
}

/// How the script process ended, as reported in the trailer and the JSON envelope.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::exec::ScriptStatus;
/// use mcp_execution_cli::runtime::signal::RunnerSignal;
///
/// assert_eq!(ScriptStatus::Exit(3).to_string(), "exit 3");
/// assert_eq!(ScriptStatus::Signal(9).to_string(), "signal 9");
/// assert_eq!(ScriptStatus::TimedOut.to_string(), "timed out");
/// assert_eq!(
///     ScriptStatus::Cancelled(RunnerSignal::Interrupt).to_string(),
///     "cancelled by SIGINT"
/// );
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptStatus {
    /// The script exited with this code.
    Exit(i32),
    /// The script was killed by this signal.
    Signal(i32),
    /// The runner killed the script because the run timeout elapsed.
    TimedOut,
    /// The runner was interrupted or terminated and killed the script.
    Cancelled(RunnerSignal),
}

impl ScriptStatus {
    /// Classifies a finished process: a code, or on Unix the signal that killed it.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_execution_cli::runtime::exec::ScriptStatus;
    ///
    /// # fn example(status: std::process::ExitStatus) {
    /// let _status = ScriptStatus::from_exit(status);
    /// # }
    /// ```
    #[must_use]
    pub fn from_exit(status: ExitStatus) -> Self {
        if let Some(code) = status.code() {
            return Self::Exit(code);
        }
        #[cfg(unix)]
        {
            use std::os::unix::process::ExitStatusExt as _;
            if let Some(signal) = status.signal() {
                return Self::Signal(signal);
            }
        }
        Self::Exit(-1)
    }
}

impl fmt::Display for ScriptStatus {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exit(code) => write!(f, "exit {code}"),
            Self::Signal(signal) => write!(f, "signal {signal}"),
            Self::TimedOut => f.write_str("timed out"),
            Self::Cancelled(signal) => write!(f, "cancelled by {signal}"),
        }
    }
}

/// Bytes read from one output pipe, bounded to [`MAX_SPILL_FILE_BYTES`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::exec::Captured;
///
/// let captured = Captured::default();
/// assert!(!captured.is_cut());
/// ```
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Captured {
    /// The leading bytes kept.
    pub bytes: Vec<u8>,
    /// Bytes the process wrote in total.
    pub total: u64,
}

impl Captured {
    fn push(&mut self, chunk: &[u8]) {
        self.total += chunk.len() as u64;
        let room = MAX_SPILL_FILE_BYTES.saturating_sub(self.bytes.len());
        self.bytes
            .extend_from_slice(&chunk[..chunk.len().min(room)]);
    }

    /// Whether the process wrote more than was kept.
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::exec::Captured;
    ///
    /// assert!(!Captured { bytes: vec![1], total: 1 }.is_cut());
    /// assert!(Captured { bytes: vec![1], total: 2 }.is_cut());
    /// ```
    #[must_use]
    pub const fn is_cut(&self) -> bool {
        self.total > self.bytes.len() as u64
    }
}

/// What to run: the script, its arguments and the node preload.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::exec::{RunTimeout, ScriptProcess};
///
/// let spec = ScriptProcess {
///     script: "/tmp/script.ts".into(),
///     args: vec!["a".to_string()],
///     import_argument: "data:text/javascript,".to_string(),
///     node_args: vec![],
///     timeout: RunTimeout::default_timeout(),
/// };
/// assert_eq!(spec.args, ["a"]);
/// ```
#[derive(Debug, Clone)]
pub struct ScriptProcess {
    /// Absolute path of the script.
    pub script: PathBuf,
    /// Arguments passed to the script as data.
    pub args: Vec<String>,
    /// Value of node's `--import` option (see [`super::import_hook::import_argument`]).
    pub import_argument: String,
    /// Extra node options placed before the script, e.g. the `--restrict` permission flags.
    pub node_args: Vec<OsString>,
    /// Wall-clock limit for the script.
    pub timeout: RunTimeout,
}

/// What a finished script run produced.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::exec::{ProcessOutcome, ScriptStatus};
///
/// let outcome = ProcessOutcome {
///     status: ScriptStatus::Exit(0),
///     stdout: Default::default(),
///     stderr: Default::default(),
///     drain_abandoned: false,
/// };
/// assert!(!outcome.drain_abandoned);
/// ```
#[derive(Debug, Clone)]
pub struct ProcessOutcome {
    /// How the script ended.
    pub status: ScriptStatus,
    /// Captured stdout.
    pub stdout: Captured,
    /// Captured stderr.
    pub stderr: Captured,
    /// Whether a reader was abandoned because something still held its pipe after the grace.
    pub drain_abandoned: bool,
}

type SharedCapture = Arc<Mutex<Captured>>;

fn lock(capture: &Mutex<Captured>) -> MutexGuard<'_, Captured> {
    capture.lock().unwrap_or_else(PoisonError::into_inner)
}

/// A task reading one output pipe into a bounded [`Captured`].
#[derive(Debug)]
pub(crate) struct PipeCapture {
    capture: SharedCapture,
    task: JoinHandle<()>,
}

impl PipeCapture {
    /// Starts reading `reader` until EOF or a read error.
    pub(crate) fn start<R>(mut reader: R) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
    {
        let capture = SharedCapture::default();
        let sink = Arc::clone(&capture);
        let task = tokio::spawn(async move {
            let mut buf = [0_u8; 8192];
            loop {
                match reader.read(&mut buf).await {
                    Ok(0) | Err(_) => break,
                    Ok(read) => lock(&sink).push(&buf[..read]),
                }
            }
        });
        Self { capture, task }
    }
}

impl Drop for PipeCapture {
    fn drop(&mut self) {
        self.task.abort();
    }
}

/// A spawned child that is stopped together with its descendants when the guard is killed or
/// dropped.
///
/// On Unix the child leads its own process group and the whole group gets `SIGKILL`; a signal is
/// only sent while the leader has not been reaped, so a recycled group id is never hit. Elsewhere
/// the guard stops the direct child only.
#[derive(Debug)]
pub(crate) struct ProcessTree {
    child: Child,
    killed: bool,
}

impl ProcessTree {
    /// Spawns `command` as the leader of a new process group (Unix) that is killed on drop.
    pub(crate) fn spawn(command: &mut Command) -> std::io::Result<Self> {
        #[cfg(unix)]
        command.process_group(0);
        command.kill_on_drop(true);
        Ok(Self {
            child: command.spawn()?,
            killed: false,
        })
    }

    /// The leader process, for its pipes and `wait`.
    pub(crate) const fn child_mut(&mut self) -> &mut Child {
        &mut self.child
    }

    /// Stops the child and, on Unix, every process left in its group. Idempotent.
    pub(crate) fn kill(&mut self) {
        if self.killed {
            return;
        }
        self.killed = true;
        let Some(pid) = self.child.id() else {
            return;
        };
        #[cfg(unix)]
        kill_group(pid);
        #[cfg(not(unix))]
        let _ = pid;
        if let Err(error) = self.child.start_kill() {
            tracing::debug!("could not kill the child process: {error}");
        }
    }
}

impl Drop for ProcessTree {
    fn drop(&mut self) {
        self.kill();
    }
}

#[cfg(unix)]
fn kill_group(leader: u32) {
    use rustix::process::{Pid, Signal, kill_process_group};

    let Some(group) = i32::try_from(leader).ok().and_then(Pid::from_raw) else {
        return;
    };
    if let Err(error) = kill_process_group(group, Signal::KILL) {
        tracing::debug!("could not kill the process group {leader}: {error}");
    }
}

/// What two pipe readers captured, and whether one was abandoned at the grace.
#[derive(Debug)]
pub(crate) struct Drained {
    pub(crate) stdout: Captured,
    pub(crate) stderr: Captured,
    pub(crate) abandoned: bool,
}

/// Waits up to `grace` for both readers to reach EOF, aborts those that did not, and returns
/// what they captured.
pub(crate) async fn drain(mut out: PipeCapture, mut err: PipeCapture, grace: Duration) -> Drained {
    let finished = tokio::time::timeout(grace, async {
        let (out_done, err_done) = tokio::join!(&mut out.task, &mut err.task);
        out_done.is_ok() && err_done.is_ok()
    })
    .await
    .unwrap_or(false);
    if !finished {
        out.task.abort();
        err.task.abort();
    }
    Drained {
        stdout: std::mem::take(&mut *lock(&out.capture)),
        stderr: std::mem::take(&mut *lock(&err.capture)),
        abandoned: !finished,
    }
}

/// Resumes the script's group if a terminal Ctrl-Z stops it; pends forever without a terminal.
async fn keep_script_running(foreground: Option<&ForegroundGroup>) -> Infallible {
    match foreground {
        Some(group) => group.keep_running().await,
        None => std::future::pending().await,
    }
}

async fn kill_script(tree: &mut ProcessTree, why: &str) {
    tree.kill();
    if let Err(error) = tree.child_mut().wait().await {
        tracing::debug!("could not reap the {why} script: {error}");
    }
}

/// Starts node on the script and waits for it within the run timeout.
///
/// Without a `helper` the script gets a null stdin and no helper environment; with one, the
/// endpoint's environment variables and, on the Unix carrier, its stdin (the channel). The script
/// runs through a `ProcessTree`, so on timeout, cancel or drop it is killed together with the
/// processes that stay in its process group (on Unix; a descendant that starts its own session or
/// group is not stopped, and elsewhere only the direct child is). It leads its own process group,
/// which becomes the terminal's foreground group while it runs (see `ForegroundGroup`), and a
/// terminal-generated SIGINT, SIGQUIT or SIGHUP that kills it is reported as a cancellation.
/// `cancel` is polled before the wait, so when it resolves the script is killed and the status is
/// [`ScriptStatus::Cancelled`] even if the script ended at the same moment. Afterwards the output
/// readers get [`DRAIN_GRACE`] before they are abandoned.
///
/// # Errors
///
/// Returns an error when node cannot be started or waited on.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runtime::exec::{
///     RunTimeout, ScriptProcess, ScriptStatus, run_script_process,
/// };
///
/// # async fn example() -> anyhow::Result<()> {
/// let spec = ScriptProcess {
///     script: "/tmp/script.ts".into(),
///     args: vec![],
///     import_argument: "data:text/javascript,".to_string(),
///     node_args: vec![],
///     timeout: RunTimeout::default_timeout(),
/// };
/// let outcome = run_script_process(&spec, None, std::future::pending()).await?;
/// assert_eq!(outcome.status, ScriptStatus::Exit(0));
/// # Ok(())
/// # }
/// ```
pub async fn run_script_process(
    spec: &ScriptProcess,
    helper: Option<&mut HelperEndpoint>,
    cancel: impl Future<Output = RunnerSignal>,
) -> Result<ProcessOutcome> {
    let mut command = Command::new("node");
    command
        .args(&spec.node_args)
        .arg("--import")
        .arg(&spec.import_argument)
        .arg("--disable-warning=ExperimentalWarning")
        .arg(&spec.script)
        .args(&spec.args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .env_remove(HELPER_ENV)
        .env_remove(HELPER_TOKEN_ENV)
        .env(RUN_MARKER_ENV, "1");
    if let Some(endpoint) = helper {
        command.envs(endpoint.env());
        if let Some(stdin) = endpoint.take_stdin() {
            command.stdin(stdin);
        }
    }
    // The command, and with it the parent's copy of the channel's child end, is dropped here.
    let mut tree = ProcessTree::spawn(&mut command).context("cannot start node")?;
    drop(command);
    let child = tree.child_mut();
    let foreground = child.id().and_then(ForegroundGroup::hand_over);
    let stdout = child
        .stdout
        .take()
        .context("node stdout was not captured")?;
    let stderr = child
        .stderr
        .take()
        .context("node stderr was not captured")?;
    let out = PipeCapture::start(stdout);
    let err = PipeCapture::start(stderr);

    let status = tokio::select! {
        biased;
        signal = cancel => {
            kill_script(&mut tree, "cancelled").await;
            ScriptStatus::Cancelled(signal)
        }
        waited = tokio::time::timeout(spec.timeout.get(), tree.child_mut().wait()) => match waited {
            Ok(waited) => {
                let status = ScriptStatus::from_exit(waited.context("cannot wait for node")?);
                if foreground.is_some() { terminal_cancellation(status) } else { status }
            }
            Err(_elapsed) => {
                kill_script(&mut tree, "timed-out").await;
                ScriptStatus::TimedOut
            }
        },
        never = keep_script_running(foreground.as_ref()) => match never {},
    };

    drop(foreground);
    let drained = drain(out, err, DRAIN_GRACE).await;
    Ok(ProcessOutcome {
        status,
        stdout: drained.stdout,
        stderr: drained.stderr,
        drain_abandoned: drained.abandoned,
    })
}

/// Unix process helpers shared by the tree-kill tests of this module and `typecheck`.
#[cfg(all(test, unix))]
pub(crate) mod tree_test_support {
    use std::path::Path;
    use std::time::{Duration, Instant};

    use rustix::process::{Pid, test_kill_process};

    /// Command running `sh` that starts a background `sleep`, records its pid in `pid_file`,
    /// and waits.
    pub fn sleeping_grandchild(pid_file: &Path) -> tokio::process::Command {
        let mut command = tokio::process::Command::new("sh");
        command
            .arg("-c")
            .arg("sleep 300 & echo $! > \"$1\"; wait")
            .arg("sh")
            .arg(pid_file);
        command
    }

    /// Waits until `pid_file` holds a pid and returns it.
    pub async fn recorded_pid(pid_file: &Path) -> i32 {
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            if let Some(pid) = std::fs::read_to_string(pid_file)
                .ok()
                .and_then(|text| text.trim().parse().ok())
            {
                return pid;
            }
            assert!(Instant::now() < deadline, "no pid was recorded");
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    }

    fn is_alive(pid: i32) -> bool {
        Pid::from_raw(pid).is_some_and(|pid| test_kill_process(pid).is_ok())
    }

    /// Asserts that `pid` is gone within five seconds.
    pub async fn assert_gone_soon(pid: i32) {
        let deadline = Instant::now() + Duration::from_secs(5);
        while is_alive(pid) {
            assert!(Instant::now() < deadline, "process {pid} is still alive");
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timeout_bounds_are_inclusive() {
        assert!(RunTimeout::from_secs(MIN_RUN_TIMEOUT_SECS).is_ok());
        assert!(RunTimeout::from_secs(MAX_RUN_TIMEOUT_SECS).is_ok());
        assert!(RunTimeout::from_secs(0).is_err());
        assert!("abc".parse::<RunTimeout>().is_err());
    }

    #[test]
    fn default_timeout_is_valid() {
        let default = RunTimeout::default_timeout();
        assert_eq!(RunTimeout::from_secs(default.get().as_secs()), Ok(default));
    }

    #[test]
    fn captured_keeps_the_head_and_counts_everything() {
        let mut captured = Captured::default();
        captured.push(&vec![7_u8; MAX_SPILL_FILE_BYTES]);
        captured.push(b"tail");
        assert_eq!(captured.bytes.len(), MAX_SPILL_FILE_BYTES);
        assert_eq!(captured.total, MAX_SPILL_FILE_BYTES as u64 + 4);
        assert!(captured.is_cut());
    }

    fn node_available() -> bool {
        std::process::Command::new("node")
            .arg("--version")
            .output()
            .is_ok_and(|output| output.status.success())
    }

    fn idle_spec(dir: &std::path::Path) -> ScriptProcess {
        let script = dir.join("idle.mjs");
        std::fs::write(&script, "setInterval(() => {}, 1000);\n").unwrap();
        ScriptProcess {
            script,
            args: vec![],
            import_argument: "data:text/javascript,".to_string(),
            node_args: vec![],
            timeout: RunTimeout::default_timeout(),
        }
    }

    #[tokio::test]
    async fn a_ready_cancel_kills_the_script_and_reports_the_signal() {
        if !node_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let started = std::time::Instant::now();
        let outcome = run_script_process(
            &idle_spec(dir.path()),
            None,
            std::future::ready(RunnerSignal::Terminate),
        )
        .await
        .unwrap();
        assert_eq!(
            outcome.status,
            ScriptStatus::Cancelled(RunnerSignal::Terminate)
        );
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[tokio::test]
    async fn a_pending_cancel_lets_the_script_finish_normally() {
        if !node_available() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let script = dir.path().join("done.mjs");
        std::fs::write(&script, "process.exit(3);\n").unwrap();
        let spec = ScriptProcess {
            script,
            ..idle_spec(dir.path())
        };
        let outcome = run_script_process(&spec, None, std::future::pending())
            .await
            .unwrap();
        assert_eq!(outcome.status, ScriptStatus::Exit(3));
    }

    #[tokio::test]
    async fn a_dropped_pipe_capture_aborts_its_reader() {
        let (reader, _writer) = tokio::io::duplex(64);
        let capture = PipeCapture::start(reader);
        let shared = Arc::clone(&capture.capture);
        assert_eq!(Arc::strong_count(&shared), 3);
        drop(capture);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while Arc::strong_count(&shared) > 1 {
            assert!(std::time::Instant::now() < deadline, "the reader survived");
            tokio::task::yield_now().await;
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn dropping_a_process_tree_stops_its_descendants() {
        use tree_test_support::{assert_gone_soon, recorded_pid, sleeping_grandchild};

        let dir = tempfile::tempdir().unwrap();
        let pid_file = dir.path().join("grandchild.pid");
        let mut command = sleeping_grandchild(&pid_file);
        let tree = ProcessTree::spawn(&mut command).unwrap();
        let grandchild = recorded_pid(&pid_file).await;
        drop(tree);
        assert_gone_soon(grandchild).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn killing_a_reaped_process_tree_signals_nothing() {
        let mut command = Command::new("sh");
        command.args(["-c", "exit 0"]);
        let mut tree = ProcessTree::spawn(&mut command).unwrap();
        tree.child_mut().wait().await.unwrap();
        assert!(tree.child_mut().id().is_none());
        tree.kill();
    }
}
