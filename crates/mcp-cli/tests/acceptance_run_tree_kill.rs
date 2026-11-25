//! Acceptance tests of the script process tree (#646): a process the script started is stopped
//! together with it on the run timeout and when the runner gets SIGINT, SIGHUP or SIGQUIT.
//!
//! The script spawns a detached-stdio `sleep 300` and idles, so the grandchild is not the direct
//! child of the runner and only a process-group kill reaches it.
//!
//! Unix only: process groups and signals. The Windows half is tracked separately (#649).

#![cfg(all(feature = "test-fixtures", unix))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::os::unix::process::CommandExt as _;
use std::path::Path;
use std::process::Stdio;
use std::time::Duration;

use common::{Sandbox, assert_gone, require_node, send_signal, wait_for_exit, wait_for_pid};

const START_WITHIN: Duration = Duration::from_secs(60);
const EXIT_WITHIN: Duration = Duration::from_secs(10);
const GONE_WITHIN: Duration = Duration::from_secs(5);

/// Starts `sleep 300` with ignored stdio, records its pid in `argv[2]`, and idles.
const SPAWNING_SCRIPT: &str = "import { spawn } from 'node:child_process';\n\
     import { writeFileSync } from 'node:fs';\n\
     const child = spawn('sleep', ['300'], { stdio: 'ignore' });\n\
     writeFileSync(process.argv[2], String(child.pid));\n\
     await new Promise(() => {});\n";

/// Kills the descendant (and the runner group, when given) when the test ends, whatever the outcome.
struct Cleanup {
    descendant: u32,
    runner_group: Option<u32>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        if let Some(group) = self.runner_group {
            send_signal("KILL", &format!("-{group}"));
        }
        send_signal("KILL", &self.descendant.to_string());
    }
}

fn wait_for_descendant(pid_file: &Path) -> u32 {
    wait_for_pid(
        pid_file,
        START_WITHIN,
        "the script never reported its descendant",
    )
}

#[test]
fn run_stops_a_script_descendant_on_timeout() {
    if !require_node("run_stops_a_script_descendant_on_timeout") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("spawning.ts", SPAWNING_SCRIPT);
    let pid_file = sb.root.join("descendant.pid");
    let mut cmd = sb.command();
    cmd.args(["run", &script.display().to_string()])
        .args(["--servers-dir", &sb.servers.display().to_string()])
        .args(["--timeout", "5"])
        .args(["--", &pid_file.display().to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut runner = cmd.spawn().unwrap();
    let descendant = wait_for_descendant(&pid_file);
    let _cleanup = Cleanup {
        descendant,
        runner_group: None,
    };

    let code = wait_for_exit(
        &mut runner,
        EXIT_WITHIN,
        "runner still alive after the run timeout",
    );

    assert_eq!(code, Some(4));
    assert_gone(descendant, GONE_WITHIN, "the script descendant");
}

#[test]
fn run_stops_a_script_descendant_when_interrupted() {
    assert_signal_stops_descendant("run_stops_a_script_descendant_when_interrupted", "INT", 130);
}

#[test]
fn run_stops_a_script_descendant_on_sighup_and_exits_129() {
    assert_signal_stops_descendant(
        "run_stops_a_script_descendant_on_sighup_and_exits_129",
        "HUP",
        129,
    );
}

#[test]
fn run_stops_a_script_descendant_on_sigquit_and_exits_131() {
    assert_signal_stops_descendant(
        "run_stops_a_script_descendant_on_sigquit_and_exits_131",
        "QUIT",
        131,
    );
}

/// Signals only the runner and asserts its exit code and that the script's descendant is gone.
fn assert_signal_stops_descendant(test: &str, signal: &str, expected: i32) {
    if !require_node(test) {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("spawning.ts", SPAWNING_SCRIPT);
    let pid_file = sb.root.join("descendant.pid");
    let mut cmd = sb.command();
    cmd.args(["run", &script.display().to_string()])
        .args(["--servers-dir", &sb.servers.display().to_string()])
        .args(["--", &pid_file.display().to_string()])
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    let mut runner = cmd.spawn().unwrap();
    let runner_pid = runner.id();
    let descendant = wait_for_descendant(&pid_file);
    let _cleanup = Cleanup {
        descendant,
        runner_group: Some(runner_pid),
    };

    send_signal(signal, &runner_pid.to_string());
    let code = wait_for_exit(
        &mut runner,
        EXIT_WITHIN,
        &format!("runner still alive after SIG{signal}"),
    );

    assert_eq!(code, Some(expected));
    assert_gone(descendant, GONE_WITHIN, "the script descendant");
}
