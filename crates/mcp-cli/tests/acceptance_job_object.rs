//! Acceptance tests of the Windows job object around the script and `tsc` (#649): a descendant
//! is stopped with its parent.
//!
//! - The script leaves a detached `node` child and ends: the run closes the job, so the child is
//!   gone when `run` returns (Unix does the same through the process group, #656).
//! - `tsc` is a `tsc.cmd` shim that starts a `node` grandchild; the runner is terminated while it
//!   waits. Closing the job handle with the runner stops the grandchild, which `cmd.exe` alone
//!   would not.
//!
//! Windows only. They cannot run on a Unix host; CI's windows-latest leg is the oracle.

#![cfg(all(feature = "test-fixtures", windows))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{Sandbox, require_node};

const START_WITHIN: Duration = Duration::from_secs(60);
const GONE_WITHIN: Duration = Duration::from_secs(10);

const IDLE_NODE: &str = "setInterval(() => {}, 1000)";

const LEAVING_SCRIPT: &str = "import { spawn } from 'node:child_process';\n\
     import { writeFileSync } from 'node:fs';\n\
     const child = spawn(process.execPath, ['-e', 'setInterval(() => {}, 1000)'], { stdio: 'ignore' });\n\
     writeFileSync(process.argv[2], String(child.pid));\n\
     child.unref();\n";

fn read_pid(pid_file: &Path, within: Duration, what: &str) -> u32 {
    let deadline = Instant::now() + within;
    loop {
        if let Some(pid) = std::fs::read_to_string(pid_file)
            .ok()
            .and_then(|text| text.trim().parse().ok())
        {
            return pid;
        }
        assert!(Instant::now() < deadline, "{what}");
        std::thread::sleep(Duration::from_millis(50));
    }
}

fn is_alive(pid: u32) -> bool {
    let listing = Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH"])
        .output()
        .unwrap();
    String::from_utf8_lossy(&listing.stdout)
        .split_whitespace()
        .any(|word| word == pid.to_string())
}

fn assert_gone(pid: u32, what: &str) {
    let deadline = Instant::now() + GONE_WITHIN;
    while is_alive(pid) {
        assert!(
            Instant::now() < deadline,
            "{what} {pid} is still alive {GONE_WITHIN:?} after the runner ended"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
}

/// Kills the process and its children when the test ends, whatever the outcome.
struct Cleanup(u32);

impl Drop for Cleanup {
    fn drop(&mut self) {
        let _ = Command::new("taskkill")
            .args(["/F", "/T", "/PID", &self.0.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

#[test]
fn run_stops_a_process_the_script_left_after_a_normal_exit() {
    if !require_node("run_stops_a_process_the_script_left_after_a_normal_exit") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("leave.mts", LEAVING_SCRIPT);
    let pid_file = sb.root.join("leftover.pid");

    let out = sb.run_script(
        &script,
        &["--timeout", "30", "--", &pid_file.display().to_string()],
    );

    let leftover = read_pid(
        &pid_file,
        Duration::from_secs(1),
        "the script never reported its leftover",
    );
    let _cleanup = Cleanup(leftover);
    assert_eq!(out.code, Some(0), "{out:?}");
    assert_gone(leftover, "the process the script left behind");
}

#[test]
fn run_stops_a_tsc_cmd_shim_descendant_when_the_runner_is_terminated() {
    if !require_node("run_stops_a_tsc_cmd_shim_descendant_when_the_runner_is_terminated") {
        return;
    }
    let sb = Sandbox::new();
    let grandchild_file = sb.root.join("grandchild.pid");
    let shim_dir = sb.root.join("shim");
    std::fs::create_dir_all(&shim_dir).unwrap();
    std::fs::write(
        shim_dir.join("tsc.cmd"),
        format!(
            "@echo off\r\nnode -e \"require('fs').writeFileSync(process.argv[1], String(process.pid)); {IDLE_NODE}\" \"{}\"\r\n",
            grandchild_file.display()
        ),
    )
    .unwrap();
    let path = std::env::join_paths(std::iter::once(shim_dir).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let script = sb.script("typed.ts", "export {};\n");
    let mut runner = sb
        .command()
        .args(["run", &script.display().to_string()])
        .args(["--servers-dir", &sb.servers.display().to_string()])
        .arg("--typecheck")
        .env("PATH", path)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let _runner_cleanup = Cleanup(runner.id());
    let grandchild = read_pid(
        &grandchild_file,
        START_WITHIN,
        "tsc never reported its grandchild",
    );
    let _grandchild_cleanup = Cleanup(grandchild);

    runner.kill().unwrap();
    let _ = runner.wait();

    assert_gone(grandchild, "the tsc grandchild");
}
