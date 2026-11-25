//! Terminal foreground hand-over for the script process (#646).
//!
//! The script leads its own process group so that the whole group can be killed. On a terminal
//! that group would be a background group: a descendant reading `/dev/tty` (an `ssh` or `sudo`
//! password prompt, a git credential prompt) would be stopped by SIGTTIN. `ForegroundGroup`
//! makes the script's group the terminal's foreground group while the script runs and gives the
//! terminal back to the runner afterwards.
//!
//! The terminal is only taken from the runner's own group: when the runner is not the foreground
//! group (a background job) or has no controlling terminal, nothing changes. After a hand-over a
//! terminal-generated SIGINT, SIGQUIT or SIGHUP reaches the script's group, not the runner (see
//! [`terminal_cancellation`]), and so does Ctrl-Z, which `ForegroundGroup::keep_running` undoes
//! by sending the group `SIGCONT` every tick, so a run cannot be suspended and freeze the
//! terminal. Afterwards the terminal returns to the runner when the script's group, or a group
//! that no longer has any process, holds it.
//!
//! A `SIGKILL` of the runner cannot restore anything: the terminal stays with the script's group,
//! which is orphaned, and the shell may report an input/output error when it reads the terminal.

use crate::runtime::exec::ScriptStatus;
use crate::runtime::signal::RunnerSignal;

/// Maps a script that died of a terminal-generated signal to the cancellation it stands for.
///
/// While the script's group owns the terminal, Ctrl-C, Ctrl-\ and a terminal hang-up are
/// delivered to it and not to the runner, so the runner reports them as its own cancellation
/// (exit 130, 131 or 129) instead of a script failure. Other statuses are returned unchanged.
pub const fn terminal_cancellation(status: ScriptStatus) -> ScriptStatus {
    match status {
        ScriptStatus::Signal(1) => ScriptStatus::Cancelled(RunnerSignal::Hangup),
        ScriptStatus::Signal(2) => ScriptStatus::Cancelled(RunnerSignal::Interrupt),
        ScriptStatus::Signal(3) => ScriptStatus::Cancelled(RunnerSignal::Quit),
        other => other,
    }
}

#[cfg(unix)]
pub use unix::ForegroundGroup;

#[cfg(not(unix))]
pub use other::ForegroundGroup;

#[cfg(unix)]
mod unix {
    use std::convert::Infallible;
    use std::fs::{File, OpenOptions};
    use std::time::Duration;

    use nix::sys::signal::{SigSet, SigmaskHow, Signal, pthread_sigmask};
    use rustix::io::Errno;
    use rustix::process::{
        Pid, Signal as RustixSignal, getpgrp, kill_process_group, test_kill_process_group,
    };
    use rustix::termios::{tcgetpgrp, tcsetpgrp};

    /// How often the script group is sent `SIGCONT`.
    const STOP_POLL: Duration = Duration::from_millis(100);

    /// The script's process group holding the foreground of the runner's controlling terminal.
    ///
    /// Dropping the guard gives the terminal back to the runner's group when the script's group
    /// still holds it, or when the foreground group is neither the runner's nor the script's and
    /// has no process left (a descendant took the terminal into its own group and died). A live
    /// foreign group, such as the shell after a stop and `bg`, is left alone.
    #[derive(Debug)]
    pub struct ForegroundGroup {
        tty: File,
        runner: Pid,
        script: Pid,
    }

    impl ForegroundGroup {
        /// Makes the group led by `script_leader` the terminal's foreground group.
        ///
        /// Returns `None`, leaving the terminal untouched, when the runner has no controlling
        /// terminal, is not the terminal's foreground group itself, or the hand-over fails. The
        /// group is sent `SIGCONT` afterwards: the script may already have been stopped by
        /// SIGTTIN while it still was a background group.
        pub fn hand_over(script_leader: u32) -> Option<Self> {
            let script = i32::try_from(script_leader).ok().and_then(Pid::from_raw)?;
            let tty = OpenOptions::new()
                .read(true)
                .write(true)
                .open("/dev/tty")
                .ok()?;
            let runner = getpgrp();
            if tcgetpgrp(&tty).ok()? != runner {
                return None;
            }
            set_foreground(&tty, script)
                .inspect_err(|error| tracing::debug!("could not hand over the terminal: {error}"))
                .ok()?;
            let group = Self {
                tty,
                runner,
                script,
            };
            group.resume();
            Some(group)
        }

        /// Never returns: sends the script's group `SIGCONT` on every tick.
        ///
        /// Ctrl-Z stops the foreground group, and the runner, a background group by then, would
        /// wait for a script that cannot run while the shell gets no prompt. The leader is not
        /// probed: a stopped member such as a descendant that ignores `SIGTSTP` in the leader
        /// still freezes the group, and `SIGCONT` to a running process is harmless. The price is
        /// that a member stopped on purpose is resumed within one tick. Meant to be raced against
        /// the wait for the script.
        pub async fn keep_running(&self) -> Infallible {
            let mut tick = tokio::time::interval(STOP_POLL);
            loop {
                tick.tick().await;
                self.resume();
            }
        }

        fn resume(&self) {
            if let Err(error) = kill_process_group(self.script, RustixSignal::CONT) {
                tracing::debug!("could not resume the script group: {error}");
            }
        }
    }

    impl Drop for ForegroundGroup {
        fn drop(&mut self) {
            let Ok(foreground) = tcgetpgrp(&self.tty) else {
                return;
            };
            // EPERM means alive: Darwin answers it for a zombie-only group and Linux for a live
            // group of another user, so only ESRCH proves the group is gone.
            let abandoned = foreground != self.runner
                && matches!(test_kill_process_group(foreground), Err(Errno::SRCH));
            if foreground != self.script && !abandoned {
                return;
            }
            if let Err(error) = set_foreground(&self.tty, self.runner) {
                tracing::debug!("could not take the terminal back: {error}");
            }
        }
    }

    /// Sets the foreground group with SIGTTOU blocked on this thread.
    ///
    /// Changing the foreground group from a background group raises SIGTTOU unless the caller
    /// ignores or blocks it, and the runner is a background group again once the script owns the
    /// terminal.
    fn set_foreground(tty: &File, group: Pid) -> std::io::Result<()> {
        let mut blocked = SigSet::empty();
        blocked.add(Signal::SIGTTOU);
        let mut previous = SigSet::empty();
        pthread_sigmask(SigmaskHow::SIG_BLOCK, Some(&blocked), Some(&mut previous))
            .map_err(std::io::Error::from)?;
        let result = tcsetpgrp(tty, group).map_err(std::io::Error::from);
        if let Err(error) = pthread_sigmask(SigmaskHow::SIG_SETMASK, Some(&previous), None) {
            tracing::debug!("could not restore the signal mask: {error}");
        }
        result
    }
}

#[cfg(not(unix))]
mod other {
    use std::convert::Infallible;

    /// No terminal job control outside Unix: nothing is handed over.
    #[derive(Debug)]
    pub struct ForegroundGroup;

    impl ForegroundGroup {
        pub const fn hand_over(_script_leader: u32) -> Option<Self> {
            None
        }

        pub async fn keep_running(&self) -> Infallible {
            std::future::pending().await
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn terminal_signals_become_cancellations_and_other_statuses_pass_through() {
        assert_eq!(
            terminal_cancellation(ScriptStatus::Signal(1)),
            ScriptStatus::Cancelled(RunnerSignal::Hangup)
        );
        assert_eq!(
            terminal_cancellation(ScriptStatus::Signal(2)),
            ScriptStatus::Cancelled(RunnerSignal::Interrupt)
        );
        assert_eq!(
            terminal_cancellation(ScriptStatus::Signal(3)),
            ScriptStatus::Cancelled(RunnerSignal::Quit)
        );
        assert_eq!(
            terminal_cancellation(ScriptStatus::Signal(9)),
            ScriptStatus::Signal(9)
        );
        assert_eq!(
            terminal_cancellation(ScriptStatus::Exit(0)),
            ScriptStatus::Exit(0)
        );
    }
}
