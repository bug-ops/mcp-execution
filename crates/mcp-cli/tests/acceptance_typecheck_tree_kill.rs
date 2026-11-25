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
use std::process::Stdio;
use std::time::Duration;

use common::{Sandbox, assert_gone, require_node, send_signal, wait_for_exit, wait_for_pid};

const START_WITHIN: Duration = Duration::from_secs(60);
const EXIT_WITHIN: Duration = Duration::from_secs(10);
const GONE_WITHIN: Duration = Duration::from_secs(5);

/// Kills the runner group and the grandchild when the test ends, whatever the outcome.
struct Cleanup {
    runner: u32,
    grandchild: Option<u32>,
}

impl Drop for Cleanup {
    fn drop(&mut self) {
        send_signal("KILL", &format!("-{}", self.runner));
        if let Some(pid) = self.grandchild {
            send_signal("KILL", &pid.to_string());
        }
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
    let grandchild = wait_for_pid(
        &grandchild_file,
        START_WITHIN,
        "tsc never reported its grandchild",
    );
    cleanup.grandchild = Some(grandchild);

    // Only the runner is signalled: the grandchild can disappear only because the runner kills
    // the whole tsc tree.
    send_signal("INT", &runner.id().to_string());
    let code = wait_for_exit(&mut runner, EXIT_WITHIN, "runner still alive after SIGINT");

    assert_eq!(code, Some(130));
    assert_gone(grandchild, GONE_WITHIN, "the tsc grandchild");
}
