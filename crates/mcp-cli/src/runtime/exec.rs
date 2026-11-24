//! Running the script process: spawn, bounded output capture, timeout and exit mapping.
//!
//! Output is read by async tasks that keep at most [`MAX_SPILL_FILE_BYTES`] per stream. After
//! the script ends the readers get [`DRAIN_GRACE`]; a process the script left behind that still
//! holds a pipe cannot keep the runner waiting, its reader is aborted and the loss is reported.

use std::ffi::OsString;
use std::fmt;
use std::path::PathBuf;
use std::process::{ExitStatus, Stdio};
use std::str::FromStr;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::Duration;

use anyhow::{Context, Result};
use mcp_execution_core::helper_protocol::{HELPER_ENV, HELPER_TOKEN_ENV, RUN_MARKER_ENV};
use tokio::io::{AsyncRead, AsyncReadExt as _};
use tokio::process::Command;
use tokio::task::JoinHandle;

use super::OutOfRange;
use super::helper::HelperEndpoint;
use super::spill::MAX_SPILL_FILE_BYTES;

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
///
/// assert_eq!(ScriptStatus::Exit(3).to_string(), "exit 3");
/// assert_eq!(ScriptStatus::Signal(9).to_string(), "signal 9");
/// assert_eq!(ScriptStatus::TimedOut.to_string(), "timed out");
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScriptStatus {
    /// The script exited with this code.
    Exit(i32),
    /// The script was killed by this signal.
    Signal(i32),
    /// The runner killed the script because the run timeout elapsed.
    TimedOut,
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

fn spawn_reader<R>(mut reader: R) -> (SharedCapture, JoinHandle<()>)
where
    R: AsyncRead + Unpin + Send + 'static,
{
    let capture = SharedCapture::default();
    let sink = Arc::clone(&capture);
    let handle = tokio::spawn(async move {
        let mut buf = [0_u8; 8192];
        loop {
            match reader.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(read) => lock(&sink).push(&buf[..read]),
            }
        }
    });
    (capture, handle)
}

/// Starts node on the script and waits for it within the run timeout.
///
/// Without a `helper` the script gets a null stdin and no helper environment; with one, the
/// endpoint's environment variables and, on the Unix carrier, its stdin (the channel). On timeout
/// the script is killed. Afterwards
/// the output readers get [`DRAIN_GRACE`] before they are abandoned.
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
/// let outcome = run_script_process(&spec, None).await?;
/// assert_eq!(outcome.status, ScriptStatus::Exit(0));
/// # Ok(())
/// # }
/// ```
pub async fn run_script_process(
    spec: &ScriptProcess,
    helper: Option<&mut HelperEndpoint>,
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
        .env(RUN_MARKER_ENV, "1")
        .kill_on_drop(true);
    if let Some(endpoint) = helper {
        command.envs(endpoint.env());
        if let Some(stdin) = endpoint.take_stdin() {
            command.stdin(stdin);
        }
    }
    // The command, and with it the parent's copy of the channel's child end, is dropped here.
    let mut child = command.spawn().context("cannot start node")?;
    drop(command);
    let stdout = child
        .stdout
        .take()
        .context("node stdout was not captured")?;
    let stderr = child
        .stderr
        .take()
        .context("node stderr was not captured")?;
    let (out_capture, mut out_task) = spawn_reader(stdout);
    let (err_capture, mut err_task) = spawn_reader(stderr);

    let status = match tokio::time::timeout(spec.timeout.get(), child.wait()).await {
        Ok(waited) => ScriptStatus::from_exit(waited.context("cannot wait for node")?),
        Err(_elapsed) => {
            if let Err(error) = child.kill().await {
                tracing::debug!("could not kill the timed-out script: {error}");
            }
            ScriptStatus::TimedOut
        }
    };

    let drained = tokio::time::timeout(DRAIN_GRACE, async {
        let (out_done, err_done) = tokio::join!(&mut out_task, &mut err_task);
        out_done.is_ok() && err_done.is_ok()
    })
    .await
    .unwrap_or(false);
    if !drained {
        out_task.abort();
        err_task.abort();
    }

    let stdout = std::mem::take(&mut *lock(&out_capture));
    let stderr = std::mem::take(&mut *lock(&err_capture));
    Ok(ProcessOutcome {
        status,
        stdout,
        stderr,
        drain_abandoned: !drained,
    })
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
}
