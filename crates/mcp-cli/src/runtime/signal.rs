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
/// On Unix a SIGINT, SIGHUP or SIGQUIT that was already ignored when the runner started (`nohup`
/// ignores SIGHUP, a background job of a non-interactive shell ignores SIGINT and SIGQUIT) stays
/// ignored and never produces a [`RunnerSignal`]; SIGTERM is always handled. The script is another
/// matter: node resets
/// an inherited SIGHUP disposition, so a foreground run on a terminal still ends when the terminal
/// hangs up; use `setsid` or tmux for a run that must survive it.
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

/// The disposition a signal had when the process started.
#[cfg(unix)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Inherited {
    /// `SIG_IGN`, for example set by `nohup`.
    Ignored,
    /// Default or a handler.
    Other,
}

#[cfg(unix)]
#[expect(
    unsafe_code,
    reason = "inherited signal disposition query (#655); no safe API in std, nix or rustix"
)]
mod inherited {
    use std::ptr;

    use super::Inherited;

    /// Reads the current disposition of `signal` without changing it.
    pub(super) fn query(signal: libc::c_int) -> Inherited {
        // SAFETY: `sigaction` is a plain C struct for which all-zero bytes are a valid value.
        let mut old: libc::sigaction = unsafe { std::mem::zeroed() };
        // SAFETY: a null `act` makes the call a pure query, and `old` is valid writable memory.
        let status = unsafe { libc::sigaction(signal, ptr::null(), &raw mut old) };
        if status == 0 && old.sa_sigaction == libc::SIG_IGN {
            Inherited::Ignored
        } else {
            Inherited::Other
        }
    }
}

#[cfg(unix)]
mod platform {
    use std::sync::OnceLock;

    use tokio::signal::unix::{Signal, SignalKind, signal};

    use super::{Inherited, RunnerSignal, inherited, never};

    /// SIGINT, SIGHUP and SIGQUIT dispositions as of the first install in this process, taken
    /// before any handler of ours exists.
    #[derive(Debug, Clone, Copy)]
    pub(super) struct StartupDispositions {
        pub(super) interrupt: Inherited,
        pub(super) hangup: Inherited,
        pub(super) quit: Inherited,
    }

    pub(super) fn startup_dispositions() -> StartupDispositions {
        static STARTUP: OnceLock<StartupDispositions> = OnceLock::new();
        *STARTUP.get_or_init(|| StartupDispositions {
            interrupt: inherited::query(libc::SIGINT),
            hangup: inherited::query(libc::SIGHUP),
            quit: inherited::query(libc::SIGQUIT),
        })
    }

    /// Keeps a SIGCHLD handler registered for the life of the process (#663).
    static CHILD: OnceLock<Option<Signal>> = OnceLock::new();

    pub(super) fn reset_inherited_child_disposition() {
        CHILD.get_or_init(|| register(SignalKind::child(), "SIGCHLD"));
    }

    fn register_unless_ignored(
        kind: SignalKind,
        name: &str,
        disposition: Inherited,
    ) -> Option<Signal> {
        if disposition == Inherited::Ignored {
            tracing::debug!("{name} was ignored at startup, leaving it ignored");
            return None;
        }
        register(kind, name)
    }

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
            let startup = startup_dispositions();
            Self {
                interrupt: register_unless_ignored(
                    SignalKind::interrupt(),
                    "SIGINT",
                    startup.interrupt,
                ),
                terminate: register(SignalKind::terminate(), "SIGTERM"),
                hangup: register_unless_ignored(SignalKind::hangup(), "SIGHUP", startup.hangup),
                quit: register_unless_ignored(SignalKind::quit(), "SIGQUIT", startup.quit),
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

/// Replaces an inherited `SIGCHLD = SIG_IGN` so exited children stay waitable.
///
/// With `SIG_IGN` the kernel reaps every child at exit, `waitid` then fails with `ECHILD` and the
/// process group of a reaped script leader could be signalled. Registering a tokio handler
/// installs a real disposition. Idempotent; must run inside the tokio runtime. No-op on Windows.
pub(crate) fn reset_inherited_child_disposition() {
    #[cfg(unix)]
    platform::reset_inherited_child_disposition();
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
    assert!(
        crate::commands::common::test_isolation::is_process_per_test(),
        "raising a signal needs one process per test; gate the test with process_per_test"
    );
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
    #[cfg(unix)]
    use crate::commands::common::test_isolation::process_per_test;

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

        use super::platform::startup_dispositions;

        if !process_per_test("recv_reports_each_delivered_stop_signal_and_stays_usable") {
            return;
        }
        let mut termination = Termination::install();
        let startup = startup_dispositions();
        assert!(termination.recv().now_or_never().is_none());
        for (name, expected, inherited) in [
            ("INT", RunnerSignal::Interrupt, startup.interrupt),
            ("TERM", RunnerSignal::Terminate, Inherited::Other),
            ("HUP", RunnerSignal::Hangup, startup.hangup),
            ("QUIT", RunnerSignal::Quit, startup.quit),
        ] {
            if inherited == Inherited::Ignored {
                continue;
            }
            super::raise_signal(name);
            assert_eq!(termination.recv().await, expected);
            assert!(termination.recv().now_or_never().is_none());
        }
    }
}
