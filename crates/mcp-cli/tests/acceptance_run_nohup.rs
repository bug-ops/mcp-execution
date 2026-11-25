//! Acceptance test of an inherited ignored SIGHUP and SIGQUIT (#655): a runner started with both
//! signals set to `SIG_IGN` (what `nohup` does for SIGHUP) keeps ignoring them, so the run is not
//! cancelled and ends with the script's own exit code.
//!
//! Only the runner is signalled, never its group, so the script is not affected. The script
//! exits by itself as soon as the test creates a stop file.
//!
//! Unix only: signals and process groups.

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use std::os::unix::process::CommandExt as _;
use std::process::Stdio;
use std::time::Duration;

use common::{CLI_BIN, Sandbox, require_node, send_signal, wait_for_exit, wait_for_pid};

const START_WITHIN: Duration = Duration::from_secs(60);
const EXIT_WITHIN: Duration = Duration::from_secs(20);
/// How long a runner that honours a signal needs to react; far longer than a cancel takes.
const REACTION_WINDOW: Duration = Duration::from_millis(1500);

/// Writes its pid to `argv[2]` and exits 0 once the file `argv[3]` exists.
const WAITING_SCRIPT: &str = "import { existsSync, writeFileSync } from 'node:fs';\n\
     writeFileSync(process.argv[2], String(process.pid));\n\
     setInterval(() => { if (existsSync(process.argv[3])) process.exit(0); }, 100);\n";

#[test]
fn run_ignores_sighup_and_sigquit_that_were_ignored_at_startup() {
    if !require_node("run_ignores_sighup_and_sigquit_that_were_ignored_at_startup") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("waiting.mts", WAITING_SCRIPT);
    let pid_file = sb.root.join("script.pid");
    let stop_file = sb.root.join("stop");
    let mut runner = sb
        .command_for("sh")
        .args(["-c", "trap '' HUP QUIT; exec \"$0\" \"$@\""])
        .arg(CLI_BIN)
        .args(["run", &script.display().to_string()])
        .args(["--servers-dir", &sb.servers.display().to_string()])
        .args(["--timeout", "60", "--"])
        .args([&pid_file, &stop_file].map(|path| path.display().to_string()))
        .process_group(0)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let runner_pid = runner.id();
    let script_pid = wait_for_pid(&pid_file, START_WITHIN, "the script never started");

    send_signal("HUP", &runner_pid.to_string());
    send_signal("QUIT", &runner_pid.to_string());
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
        "the runner reacted to an ignored signal"
    );
    assert_eq!(code, Some(0));
}
