//! Acceptance test of the tsc process tree on cancel (#633): a descendant of `tsc` is stopped
//! together with it when the runner is interrupted during the type check.
//!
//! The tsc shim starts `sleep 300` in the background and waits, so the grandchild is not the
//! direct child of the runner and only a process-group kill reaches it.
//!
//! Unix only: process groups and the `sh` tsc shim. The Windows half of #633 is deferred.

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use common::{Sandbox, require_node};

const START_WITHIN: Duration = Duration::from_secs(60);
const EXIT_WITHIN: Duration = Duration::from_secs(10);
const GONE_WITHIN: Duration = Duration::from_secs(5);

fn kill(signal: &str, target: &str) {
    let _ = Command::new("sh")
        .args([
            "-c",
            "kill -s \"$1\" -- \"$2\" 2>/dev/null",
            "sh",
            signal,
            target,
        ])
        .status();
}

fn is_alive(pid: u32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// Kills the runner group and the grandchild when the test ends, whatever the outcome.
struct Cleanup {
    runner: u32,
    grandchild: Option<u32>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        kill("KILL", &format!("-{}", self.runner));
        if let Some(pid) = self.grandchild {
            kill("KILL", &pid.to_string());
        }
    }
}

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
            "tsc never reported its grandchild"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn run_stops_a_tsc_descendant_when_interrupted_during_the_type_check() {
    if !require_node("run_stops_a_tsc_descendant_when_interrupted_during_the_type_check") {
        return;
    }
    let sb = Sandbox::new();
    let grandchild_file = sb.root.join("grandchild.pid");
    let shim_dir = sb.root.join("shim");
    std::fs::create_dir_all(&shim_dir).unwrap();
    let shim = shim_dir.join("tsc");
    std::fs::write(
        &shim,
        format!(
            "#!/bin/sh\nsleep 300 &\necho $! > '{}'\nwait\n",
            grandchild_file.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(shim_dir).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let script = sb.script("typed.ts", "export {};\n");
    let mut cmd = sb.command();
    cmd.args(["run", &script.display().to_string()])
        .args(["--servers-dir", &sb.servers.display().to_string()])
        .arg("--typecheck")
        .env("PATH", path)
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut runner = cmd.spawn().unwrap();
    let mut cleanup = Cleanup {
        runner: runner.id(),
        grandchild: None,
    };
    let grandchild = wait_for_pid(&grandchild_file);
    cleanup.grandchild = Some(grandchild);

    // Only the runner is signalled: the grandchild can disappear only because the runner kills
    // the whole tsc tree.
    kill("INT", &runner.id().to_string());
    let started = Instant::now();
    let code = loop {
        if let Some(status) = runner.try_wait().unwrap() {
            break status.code();
        }
        assert!(
            started.elapsed() < EXIT_WITHIN,
            "runner still alive after SIGINT"
        );
        std::thread::sleep(Duration::from_millis(20));
    };

    assert_eq!(code, Some(130));
    let deadline = Instant::now() + GONE_WITHIN;
    while is_alive(grandchild) {
        assert!(
            Instant::now() < deadline,
            "the tsc grandchild {grandchild} is still alive {GONE_WITHIN:?} after the runner exited"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}
