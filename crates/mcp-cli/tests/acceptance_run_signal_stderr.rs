//! Acceptance tests of cancellation when the runner cannot write its notice (#627): with a
//! standard input that never closes, a SIGINT or SIGTERM must still end `run` with exit 130 or 143
//! within seconds, whether stderr is open, closed, or a pipe whose reader has gone away.
//!
//! A failed notice write must never turn the cancel into an `Err`: `main` would then drop the
//! runtime, which waits for the uncancellable stdin read and hangs until stdin reaches EOF. The
//! open and closed stderr cases are controls that already pass; the broken-pipe cases reproduce
//! the hang.
//!
//! Unix only: signals, process groups and `mcp.json` redirection (#604).

#![cfg(all(feature = "test-fixtures", unix))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::os::unix::process::CommandExt as _;
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use common::{Sandbox, require_node};

/// How long a signalled runner may take to exit.
const EXIT_WITHIN: Duration = Duration::from_secs(5);
/// Time for the runner to install its handlers and block on the stdin read.
const SETTLE: Duration = Duration::from_secs(2);

#[derive(Clone, Copy, Debug)]
enum Stderr {
    Open,
    Closed,
    BrokenPipe,
}

/// A runner that is the leader of its own process group, killed with the group on drop.
struct Runner {
    child: Child,
    _stdin: std::process::ChildStdin,
}

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = Command::new("sh")
            .args(["-c", "kill -s KILL -- \"-$1\" 2>/dev/null", "sh"])
            .arg(self.child.id().to_string())
            .status();
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn start(sb: &Sandbox, stderr: Stderr) -> Runner {
    let servers = sb.servers.display().to_string();
    let mut cmd = match stderr {
        Stderr::Closed => {
            // `2>&-` closes fd 2 before the runner starts; `exec` keeps the pid.
            let mut cmd = sb.command_for("sh");
            cmd.args(["-c", "exec \"$@\" 2>&-", "sh", common::CLI_BIN]);
            cmd
        }
        Stderr::Open | Stderr::BrokenPipe => sb.command(),
    };
    cmd.args(["run", "-", "--servers-dir", &servers])
        .process_group(0)
        .stdin(Stdio::piped())
        .stdout(Stdio::null());
    if !matches!(stderr, Stderr::Closed) {
        cmd.stderr(Stdio::piped());
    }
    let mut child = cmd.spawn().unwrap();
    match stderr {
        Stderr::BrokenPipe => drop(child.stderr.take()),
        Stderr::Open => {
            let mut pipe = child.stderr.take().unwrap();
            std::thread::spawn(move || {
                let _ = std::io::copy(&mut pipe, &mut std::io::sink());
            });
        }
        Stderr::Closed => {}
    }
    let stdin = child.stdin.take().unwrap();
    Runner {
        child,
        _stdin: stdin,
    }
}

fn is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Starts `run -` with stdin held open, signals the runner, and asserts the exit code and that
/// the process is gone.
fn assert_cancelled(test: &str, stderr: Stderr, signal: &str, expected: i32) {
    if !require_node(test) {
        return;
    }
    let sb = Sandbox::new();
    let mut runner = start(&sb, stderr);
    std::thread::sleep(SETTLE);
    let pid = runner.child.id();

    let status = Command::new("sh")
        .args(["-c", "kill -s \"$1\" -- \"$2\"", "sh", signal])
        .arg(pid.to_string())
        .status()
        .unwrap();
    assert!(status.success(), "could not signal the runner");
    let started = Instant::now();
    let code = loop {
        if let Some(status) = runner.child.try_wait().unwrap() {
            break status.code();
        }
        assert!(
            started.elapsed() < EXIT_WITHIN,
            "{stderr:?}: runner still alive {EXIT_WITHIN:?} after SIG{signal} with stdin held open"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(code, Some(expected), "{stderr:?} after SIG{signal}");
    assert!(!is_alive(pid), "runner {pid} must be gone");
}

#[test]
fn run_exits_130_on_sigint_when_stderr_is_a_broken_pipe() {
    assert_cancelled(
        "run_exits_130_on_sigint_when_stderr_is_a_broken_pipe",
        Stderr::BrokenPipe,
        "INT",
        130,
    );
}

#[test]
fn run_exits_143_on_sigterm_when_stderr_is_a_broken_pipe() {
    assert_cancelled(
        "run_exits_143_on_sigterm_when_stderr_is_a_broken_pipe",
        Stderr::BrokenPipe,
        "TERM",
        143,
    );
}

#[test]
fn run_exits_130_on_sigint_when_stderr_is_open() {
    assert_cancelled(
        "run_exits_130_on_sigint_when_stderr_is_open",
        Stderr::Open,
        "INT",
        130,
    );
}

#[test]
fn run_exits_130_on_sigint_when_stderr_is_closed() {
    assert_cancelled(
        "run_exits_130_on_sigint_when_stderr_is_closed",
        Stderr::Closed,
        "INT",
        130,
    );
}
