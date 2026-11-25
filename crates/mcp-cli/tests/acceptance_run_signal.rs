//! Acceptance tests of runner cancellation (#627): SIGINT and SIGTERM stop `run`, kill the script
//! (and a running `tsc`), and exit 130 or 143, also when the signal reaches the whole process
//! group and while the runner waits on a standard input that never closes.
//!
//! The tests assert only the exit code and that the script process is gone. They do not pin
//! whether the notice says the script was "not started" or "cancelled", because a signal that
//! arrives just before the spawn can take either path.
//!
//! Unix only: signals and process groups.

#![cfg(all(feature = "test-fixtures", unix))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use common::{Sandbox, require_node};

/// How long a cancelled runner may take to exit (its wrap-up is bounded by a few seconds).
const EXIT_WITHIN: Duration = Duration::from_secs(10);
/// How long the killed script may take to disappear.
const GONE_WITHIN: Duration = Duration::from_secs(5);
/// How long the test waits for the script to report that it started.
const START_WITHIN: Duration = Duration::from_secs(60);

/// Writes its pid to `argv[2]` and then idles until killed.
const PID_SCRIPT: &str = "import { writeFileSync } from 'node:fs';\n\
     writeFileSync(process.argv[2], String(process.pid));\n\
     setInterval(() => {}, 1000);\n";

/// Like [`PID_SCRIPT`], but exits 0 as soon as it receives SIGINT.
const TRAPPING_SCRIPT: &str = "import { writeFileSync } from 'node:fs';\n\
     process.on('SIGINT', () => process.exit(0));\n\
     writeFileSync(process.argv[2], String(process.pid));\n\
     setInterval(() => {}, 1000);\n";

/// A runner in its own process group, with its output collected on background threads.
struct Runner {
    child: Child,
    stdin: Option<ChildStdin>,
    out: Option<JoinHandle<String>>,
    err: Option<JoinHandle<String>>,
}

#[derive(Debug)]
struct Finished {
    code: Option<i32>,
    stdout: String,
    elapsed: Duration,
}

fn collect(mut pipe: impl std::io::Read + Send + 'static) -> JoinHandle<String> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        let _ = pipe.read_to_end(&mut buf);
        String::from_utf8_lossy(&buf).into_owned()
    })
}

impl Runner {
    /// Starts `cmd` as the leader of a new process group. `stdin_piped` keeps a pipe open.
    fn start(mut cmd: Command, stdin_piped: bool) -> Self {
        cmd.process_group(0)
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if stdin_piped {
            cmd.stdin(Stdio::piped());
        }
        let mut child = cmd.spawn().unwrap();
        let stdin = child.stdin.take();
        let out = Some(collect(child.stdout.take().unwrap()));
        let err = Some(collect(child.stderr.take().unwrap()));
        Self {
            child,
            stdin,
            out,
            err,
        }
    }

    fn pid(&self) -> u32 {
        self.child.id()
    }

    /// Sends `signal` to `target` (a pid, or `-pgid` for a whole group) through the shell builtin.
    fn signal(signal: &str, target: &str) {
        let status = Command::new("sh")
            .args(["-c", "kill -s \"$1\" -- \"$2\"", "sh", signal, target])
            .status()
            .unwrap();
        assert!(status.success(), "kill -s {signal} -- {target} failed");
    }

    fn signal_runner(&self, signal: &str) {
        Self::signal(signal, &self.pid().to_string());
    }

    fn signal_group(&self, signal: &str) {
        Self::signal(signal, &format!("-{}", self.pid()));
    }

    /// Waits for the exit, failing (and killing the group) after `limit`.
    fn finish(mut self, limit: Duration) -> Finished {
        let started = Instant::now();
        let code = loop {
            if let Some(status) = self.child.try_wait().unwrap() {
                break status.code();
            }
            if started.elapsed() >= limit {
                Self::signal("KILL", &format!("-{}", self.pid()));
                let _ = self.child.wait();
                panic!("runner still alive {limit:?} after the signal");
            }
            std::thread::sleep(Duration::from_millis(20));
        };
        let elapsed = started.elapsed();
        drop(self.stdin.take());
        Finished {
            code,
            stdout: self.out.take().unwrap().join().unwrap(),
            elapsed,
        }
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        let _ = Command::new("sh")
            .args([
                "-c",
                "kill -s KILL -- \"-$1\" 2>/dev/null",
                "sh",
                &self.child.id().to_string(),
            ])
            .status();
        let _ = self.child.kill();
        let _ = self.child.wait();
        if let Some(err) = self.err.take() {
            let _ = err.join();
        }
    }
}

/// Polls `pid_file` until it holds a pid.
fn wait_for_pid(pid_file: &Path) -> u32 {
    let deadline = Instant::now() + START_WITHIN;
    loop {
        if let Some(pid) = std::fs::read_to_string(pid_file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
        {
            return pid;
        }
        assert!(
            Instant::now() < deadline,
            "the script never reported its pid"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

fn assert_gone(pid: u32) {
    let deadline = Instant::now() + GONE_WITHIN;
    while is_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "process {pid} still alive {GONE_WITHIN:?} after the runner exited"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn run_command(sb: &Sandbox, script: &Path, pid_file: &Path, flags: &[&str]) -> Command {
    let mut cmd = sb.command();
    cmd.args(["run", &script.display().to_string()])
        .args(["--servers-dir", &sb.servers.display().to_string()])
        .args(flags)
        .args(["--", &pid_file.display().to_string()]);
    cmd
}

fn pid_file(sb: &Sandbox) -> PathBuf {
    sb.root.join("script.pid")
}

#[test]
fn run_exits_143_on_sigterm_and_the_script_is_gone() {
    if !require_node("run_exits_143_on_sigterm_and_the_script_is_gone") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("idle.ts", PID_SCRIPT);
    let pid_file = pid_file(&sb);
    let runner = Runner::start(run_command(&sb, &script, &pid_file, &[]), false);
    let script_pid = wait_for_pid(&pid_file);

    runner.signal_runner("TERM");
    let finished = runner.finish(EXIT_WITHIN);

    assert_eq!(finished.code, Some(143), "stdout: {}", finished.stdout);
    assert_gone(script_pid);
}

#[test]
fn run_exits_130_on_sigint_sent_to_the_whole_process_group() {
    if !require_node("run_exits_130_on_sigint_sent_to_the_whole_process_group") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("idle.ts", PID_SCRIPT);
    let pid_file = pid_file(&sb);
    let runner = Runner::start(run_command(&sb, &script, &pid_file, &[]), false);
    let script_pid = wait_for_pid(&pid_file);

    runner.signal_group("INT");
    let finished = runner.finish(EXIT_WITHIN);

    assert_eq!(
        finished.code,
        Some(130),
        "the script dying of the same SIGINT must not read as a script failure; stdout: {}",
        finished.stdout
    );
    assert_gone(script_pid);
}

#[test]
fn run_exits_130_when_the_script_traps_sigint_and_exits_zero() {
    if !require_node("run_exits_130_when_the_script_traps_sigint_and_exits_zero") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("trap.ts", TRAPPING_SCRIPT);
    let pid_file = pid_file(&sb);
    let runner = Runner::start(run_command(&sb, &script, &pid_file, &[]), false);
    let script_pid = wait_for_pid(&pid_file);

    runner.signal_group("INT");
    let finished = runner.finish(EXIT_WITHIN);

    assert_eq!(
        finished.code,
        Some(130),
        "a cancelled run must not report the script's own exit 0; stdout: {}",
        finished.stdout
    );
    assert_gone(script_pid);
}

#[test]
fn run_exits_130_on_sigint_while_stdin_is_held_open() {
    if !require_node("run_exits_130_on_sigint_while_stdin_is_held_open") {
        return;
    }
    let sb = Sandbox::new();
    let mut cmd = sb.command();
    cmd.args([
        "run",
        "-",
        "--servers-dir",
        &sb.servers.display().to_string(),
    ]);
    let ready = sb.ready_file(&mut cmd);
    let runner = Runner::start(cmd, true);
    // The runner blocks reading a script from stdin, which stays open for the whole test.
    ready.wait();

    runner.signal_group("INT");
    let finished = runner.finish(Duration::from_secs(5));

    assert_eq!(finished.code, Some(130), "stdout: {}", finished.stdout);
}

#[test]
fn run_removes_the_run_directory_of_a_stdin_script_when_cancelled() {
    if !require_node("run_removes_the_run_directory_of_a_stdin_script_when_cancelled") {
        return;
    }
    let sb = Sandbox::new();
    let pid_file = pid_file(&sb);
    let mut cmd = sb.command();
    cmd.args([
        "run",
        "-",
        "--servers-dir",
        &sb.servers.display().to_string(),
    ])
    .args(["--", &pid_file.display().to_string()]);
    let mut runner = Runner::start(cmd, true);
    {
        use std::io::Write as _;
        let mut stdin = runner.stdin.take().unwrap();
        stdin.write_all(PID_SCRIPT.as_bytes()).unwrap();
    }
    let script_pid = wait_for_pid(&pid_file);

    runner.signal_runner("TERM");
    let finished = runner.finish(EXIT_WITHIN);

    assert_eq!(finished.code, Some(143), "stdout: {}", finished.stdout);
    assert_gone(script_pid);
    assert!(
        sb.state_files("runs").is_empty(),
        "the scratch run directory must be removed: {:?}",
        sb.state_files("runs")
    );
}

#[test]
fn run_exits_130_and_stops_tsc_when_interrupted_during_the_type_check() {
    if !require_node("run_exits_130_and_stops_tsc_when_interrupted_during_the_type_check") {
        return;
    }
    let sb = Sandbox::new();
    let tsc_pid_file = sb.root.join("tsc.pid");
    let shim_dir = sb.root.join("shim");
    std::fs::create_dir_all(&shim_dir).unwrap();
    let shim = shim_dir.join("tsc");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\necho $$ > '{}'\nexec sleep 300\n",
            tsc_pid_file.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(shim_dir).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let script = sb.script("typed.ts", "console.log('SHOULD-NOT-RUN');\nexport {};\n");
    let ignored = pid_file(&sb);
    let mut cmd = run_command(&sb, &script, &ignored, &["--typecheck"]);
    cmd.env("PATH", path);
    let runner = Runner::start(cmd, false);
    let tsc_pid = wait_for_pid(&tsc_pid_file);

    // Only the runner is signalled, so `tsc` can disappear only because the runner stops it.
    runner.signal_runner("INT");
    let finished = runner.finish(EXIT_WITHIN);

    assert_eq!(finished.code, Some(130), "stdout: {}", finished.stdout);
    assert!(
        !finished.stdout.contains("SHOULD-NOT-RUN"),
        "the script must not run after a cancelled type check: {}",
        finished.stdout
    );
    assert_gone(tsc_pid);
    assert!(finished.elapsed < EXIT_WITHIN);
}
