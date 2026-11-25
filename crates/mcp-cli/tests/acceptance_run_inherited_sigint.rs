//! Acceptance test of an inherited ignored SIGINT (#664): a runner started with SIGINT set to
//! `SIG_IGN` (what a background job of a non-interactive shell gets) keeps ignoring it, so the run
//! is not cancelled and ends with the script's own exit code, while SIGTERM is always handled.
//!
//! Only the runner is signalled, never its group, so the script is not affected. The script
//! exits by itself as soon as the test creates a stop file.
//!
//! Unix only: signals and process groups.

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use std::process::Stdio;
use std::time::Duration;

use common::{Sandbox, require_node, send_signal, wait_for_exit, wait_for_pid};

const START_WITHIN: Duration = Duration::from_secs(60);
const EXIT_WITHIN: Duration = Duration::from_secs(20);
/// How long a runner that honours a signal needs to react; far longer than a cancel takes.
const REACTION_WINDOW: Duration = Duration::from_millis(1500);

/// Writes its pid to `argv[2]` and exits 0 once the file `argv[3]` exists.
const WAITING_SCRIPT: &str = "import { existsSync, writeFileSync } from 'node:fs';\n\
     writeFileSync(process.argv[2], String(process.pid));\n\
     setInterval(() => { if (existsSync(process.argv[3])) process.exit(0); }, 100);\n";

#[test]
fn run_ignores_sigint_that_was_ignored_at_startup() {
    if !require_node("run_ignores_sigint_that_was_ignored_at_startup") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("waiting.mts", WAITING_SCRIPT);
    let pid_file = sb.root.join("script.pid");
    let stop_file = sb.root.join("stop");
    let mut runner = sb
        .runner_ignoring("INT", &script, &[&pid_file, &stop_file])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let runner_pid = runner.id();
    let script_pid = wait_for_pid(&pid_file, START_WITHIN, "the script never started");

    send_signal("INT", &runner_pid.to_string());
    std::thread::sleep(REACTION_WINDOW);
    let still_running = runner.try_wait().unwrap();
    std::fs::write(&stop_file, "").unwrap();
    let code = wait_for_exit(
        &mut runner,
        EXIT_WITHIN,
        "runner did not end after the stop file",
    );
    send_signal("KILL", &format!("-{runner_pid}"));
    send_signal("KILL", &script_pid.to_string());

    assert_eq!(
        still_running, None,
        "the runner reacted to an ignored SIGINT"
    );
    assert_eq!(code, Some(0));
}

#[test]
fn run_still_stops_on_sigterm_when_sigint_and_sigterm_were_ignored_at_startup() {
    if !require_node("run_still_stops_on_sigterm_when_sigint_and_sigterm_were_ignored_at_startup") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("waiting.mts", WAITING_SCRIPT);
    let pid_file = sb.root.join("script.pid");
    let stop_file = sb.root.join("stop");
    let mut runner = sb
        .runner_ignoring("INT TERM", &script, &[&pid_file, &stop_file])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let runner_pid = runner.id();
    let script_pid = wait_for_pid(&pid_file, START_WITHIN, "the script never started");

    send_signal("TERM", &runner_pid.to_string());
    let code = wait_for_exit(&mut runner, EXIT_WITHIN, "runner ignored SIGTERM");
    send_signal("KILL", &format!("-{runner_pid}"));
    send_signal("KILL", &script_pid.to_string());

    assert_eq!(code, Some(143));
}
