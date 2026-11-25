//! Runner termination signals.
//!
//! `run` must stop its script when the runner itself is interrupted, terminated, hung up or quit,
//! also when the signal reaches the whole process group. `Termination` is the one handle the run
//! phases race against: reading the script, the type check and the script process.

use std::fmt;
use std::future::pending;

use mcp_execution_core::cli::ExitCode;

/// A signal that asked the runner to stop.
///
/// SIGHUP stops the run even under `nohup`, because registering the handler overrides an
/// inherited `SIG_IGN`; use `setsid` or tmux for a run that must survive the terminal closing.
///
/// On Windows, Ctrl-C and Ctrl-Break both report as [`RunnerSignal::Interrupt`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::signal::RunnerSignal;
///
/// assert_eq!(RunnerSignal::Interrupt.to_string(), "SIGINT");
/// assert_eq!(RunnerSignal::Terminate.exit_code().as_i32(), 143);
/// assert_eq!(RunnerSignal::Hangup.exit_code().as_i32(), 129);
/// assert_eq!(RunnerSignal::Quit.exit_code().as_i32(), 131);
/// ```
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunnerSignal {
    /// SIGINT (Ctrl-C).
    Interrupt,
    /// SIGTERM.
    Terminate,
    /// SIGHUP (terminal closed). Raised on Unix only.
    Hangup,
    /// SIGQUIT (Ctrl-\). Raised on Unix only.
    Quit,
}

impl RunnerSignal {
    /// The exit code a runner stopped by this signal reports (`128 + signal number`).
    ///
    /// # Examples
    ///
    /// ```
    /// use mcp_execution_cli::runtime::signal::RunnerSignal;
    ///
    /// assert_eq!(RunnerSignal::Interrupt.exit_code().as_i32(), 130);
    /// ```
    #[must_use]
    pub const fn exit_code(self) -> ExitCode {
        match self {
            Self::Interrupt => ExitCode::INTERRUPTED,
            Self::Terminate => ExitCode::TERMINATED,
            Self::Hangup => ExitCode::HANGUP,
            Self::Quit => ExitCode::QUIT,
        }
    }
}

impl fmt::Display for RunnerSignal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Interrupt => "SIGINT",
            Self::Terminate => "SIGTERM",
            Self::Hangup => "SIGHUP",
            Self::Quit => "SIGQUIT",
        })
    }
}

/// A stream that never yields once registration failed or the stream closed.
async fn never() -> RunnerSignal {
    pending().await
}

#[cfg(unix)]
mod platform {
    use tokio::signal::unix::{Signal, SignalKind, signal};

    use super::{RunnerSignal, never};

    #[derive(Debug)]
    pub(super) struct Sources {
        interrupt: Option<Signal>,
        terminate: Option<Signal>,
        hangup: Option<Signal>,
        quit: Option<Signal>,
    }

    fn register(kind: SignalKind, name: &str) -> Option<Signal> {
        signal(kind)
            .inspect_err(|error| tracing::warn!("cannot listen for {name}: {error}"))
            .ok()
    }

    async fn next(source: &mut Option<Signal>, signal: RunnerSignal) -> RunnerSignal {
        if let Some(stream) = source
            && stream.recv().await.is_some()
        {
            return signal;
        }
        never().await
    }

    impl Sources {
        pub(super) fn install() -> Self {
            Self {
                interrupt: register(SignalKind::interrupt(), "SIGINT"),
                terminate: register(SignalKind::terminate(), "SIGTERM"),
                hangup: register(SignalKind::hangup(), "SIGHUP"),
                quit: register(SignalKind::quit(), "SIGQUIT"),
            }
        }

        pub(super) async fn recv(&mut self) -> RunnerSignal {
            tokio::select! {
                signal = next(&mut self.interrupt, RunnerSignal::Interrupt) => signal,
                signal = next(&mut self.terminate, RunnerSignal::Terminate) => signal,
                signal = next(&mut self.hangup, RunnerSignal::Hangup) => signal,
                signal = next(&mut self.quit, RunnerSignal::Quit) => signal,
            }
        }
    }
}

#[cfg(windows)]
mod platform {
    use tokio::signal::windows::{CtrlBreak, CtrlC, ctrl_break, ctrl_c};

    use super::{RunnerSignal, never};

    #[derive(Debug)]
    pub(super) struct Sources {
        ctrl_c: Option<CtrlC>,
        ctrl_break: Option<CtrlBreak>,
    }

    impl Sources {
        pub(super) fn install() -> Self {
            Self {
                ctrl_c: ctrl_c()
                    .inspect_err(|error| tracing::warn!("cannot listen for Ctrl-C: {error}"))
                    .ok(),
                ctrl_break: ctrl_break()
                    .inspect_err(|error| tracing::warn!("cannot listen for Ctrl-Break: {error}"))
                    .ok(),
            }
        }

        pub(super) async fn recv(&mut self) -> RunnerSignal {
            tokio::select! {
                signal = async {
                    if let Some(stream) = &mut self.ctrl_c
                        && stream.recv().await.is_some()
                    {
                        return RunnerSignal::Interrupt;
                    }
                    never().await
                } => signal,
                signal = async {
                    if let Some(stream) = &mut self.ctrl_break
                        && stream.recv().await.is_some()
                    {
                        return RunnerSignal::Interrupt;
                    }
                    never().await
                } => signal,
            }
        }
    }
}

/// The runner's stop-signal subscription (SIGINT, SIGTERM, SIGHUP and SIGQUIT on Unix; Ctrl-C and
/// Ctrl-Break on Windows).
///
/// A signal that cannot be registered is logged and never fires; it is not a run failure.
#[derive(Debug)]
pub(crate) struct Termination(platform::Sources);

impl Termination {
    /// Registers the signal handlers. Must run inside the tokio runtime.
    pub(crate) fn install() -> Self {
        Self(platform::Sources::install())
    }

    /// Resolves when a signal arrived since the last call; callable again in a later phase.
    ///
    /// Cancel-safe: a signal that arrived while the future was dropped is delivered to the next
    /// call.
    pub(crate) async fn recv(&mut self) -> RunnerSignal {
        self.0.recv().await
    }
}

/// Sends `signal` (for example `TERM`) to this process, which an installed [`Termination`] turns
/// into a [`RunnerSignal`].
#[cfg(all(test, unix))]
#[expect(clippy::unwrap_used, reason = "test helper")]
pub(crate) fn raise_signal(signal: &str) {
    let status = std::process::Command::new("kill")
        .args(["-s", signal, &std::process::id().to_string()])
        .status()
        .unwrap();
    assert!(status.success());
}

/// Sends SIGTERM to this process.
#[cfg(all(test, unix))]
pub(crate) fn raise_sigterm() {
    raise_signal("TERM");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn signals_map_to_shell_exit_codes() {
        assert_eq!(RunnerSignal::Interrupt.exit_code(), ExitCode::INTERRUPTED);
        assert_eq!(RunnerSignal::Terminate.exit_code(), ExitCode::TERMINATED);
        assert_eq!(ExitCode::INTERRUPTED.as_i32(), 130);
        assert_eq!(ExitCode::TERMINATED.as_i32(), 143);
        assert_eq!(RunnerSignal::Hangup.exit_code().as_i32(), 129);
        assert_eq!(RunnerSignal::Quit.exit_code().as_i32(), 131);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn recv_reports_each_delivered_stop_signal_and_stays_usable() {
        use futures_util::FutureExt as _;

        let mut termination = Termination::install();
        assert!(termination.recv().now_or_never().is_none());
        for (name, expected) in [
            ("TERM", RunnerSignal::Terminate),
            ("HUP", RunnerSignal::Hangup),
            ("QUIT", RunnerSignal::Quit),
        ] {
            super::raise_signal(name);
            assert_eq!(termination.recv().await, expected);
            assert!(termination.recv().now_or_never().is_none());
        }
    }
}
