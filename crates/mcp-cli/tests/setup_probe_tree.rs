//! `setup` without a suitable Node.js (#685): a Node.js below the floor falls back to Bun, and a
//! probe that hangs behind a shell shim is stopped with its whole process tree, on the timeout and
//! on a stop signal. Unix only: the fake runtimes are `sh` scripts.

#![cfg(all(feature = "test-fixtures", unix))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use std::io::Read as _;
use std::os::unix::fs::PermissionsExt as _;
use std::os::unix::process::CommandExt as _;
use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::time::Duration;

use common::{
    KillOnDrop, Sandbox, WATCHDOG, assert_gone, run_with_watchdog, send_signal, wait_for_exit,
    wait_for_pid,
};
use serde_json::{Value, json};

struct Fakes {
    sb: Sandbox,
    bin: PathBuf,
}

impl Fakes {
    fn new() -> Self {
        let sb = Sandbox::new();
        let bin = sb.root.join("fakebin");
        std::fs::create_dir_all(&bin).unwrap();
        Self { sb, bin }
    }

    fn install(&self, program: &str, body: &str) {
        let path = self.bin.join(program);
        std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A `bun` shim that starts a long-running child without `exec`; returns the child's pid file.
    fn install_hanging_shim(&self) -> PathBuf {
        let pid_file = self.sb.root.join("grandchild.pid");
        self.install(
            "bun",
            &format!("/bin/sleep 60 &\necho $! > '{}'\nwait", pid_file.display()),
        );
        pid_file
    }

    fn setup(&self) -> Command {
        let mut cmd = self.sb.command();
        cmd.env("PATH", &self.bin);
        cmd
    }
}

#[test]
fn setup_falls_back_to_bun_when_node_is_too_old() {
    let fakes = Fakes::new();
    fakes.install("node", "echo v20.10.0");
    fakes.install("bun", "echo 'mcp-execution-probe 1.3.0'");

    let out = run_with_watchdog(
        fakes.setup().args(["--format", "json", "setup"]),
        None,
        WATCHDOG,
    );

    assert_eq!(out.code, Some(0), "{} {}", out.stdout, out.stderr);
    let result: Value = serde_json::from_str(&out.stdout).unwrap();
    assert_eq!(
        result["runtime"],
        json!({ "name": "bun", "version": "1.3.0" }),
        "{result}"
    );
    assert!(result["caveat"].is_string(), "{result}");
}

/// Takes the 10 s probe timeout.
#[test]
fn a_timed_out_probe_leaves_no_process_of_its_tree() {
    let fakes = Fakes::new();
    let pid_file = fakes.install_hanging_shim();

    let out = run_with_watchdog(fakes.setup().arg("setup"), None, WATCHDOG);

    assert_ne!(out.code, Some(0), "{}", out.stdout);
    assert!(
        out.stderr.contains("no usable runtime") && out.stderr.contains("Caused by:"),
        "{}",
        out.stderr
    );
    let pid = wait_for_pid(&pid_file, Duration::from_secs(1), "no grandchild pid");
    assert_gone(pid, Duration::from_secs(5), "the probe's child");
    assert_eq!(fakes.sb.state_files("probe"), [] as [PathBuf; 0]);
}

#[test]
fn a_stop_signal_stops_the_probe_tree_and_removes_the_probe_file() {
    for (signal, code) in [("INT", 130), ("TERM", 143)] {
        let fakes = Fakes::new();
        let pid_file = fakes.install_hanging_shim();
        let mut cmd = fakes.setup();
        cmd.args(["--format", "json", "setup"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0);
        let mut setup = KillOnDrop(cmd.spawn().unwrap());
        let pid = wait_for_pid(
            &pid_file,
            Duration::from_secs(30),
            "the probe did not start",
        );

        send_signal(signal, &setup.0.id().to_string());
        // Below the probe timeout, so the signal and not the timeout ended the run.
        let exit = wait_for_exit(&mut setup.0, Duration::from_secs(8), "setup did not stop");

        let (mut stdout, mut stderr) = (String::new(), String::new());
        setup
            .0
            .stdout
            .take()
            .unwrap()
            .read_to_string(&mut stdout)
            .unwrap();
        setup
            .0
            .stderr
            .take()
            .unwrap()
            .read_to_string(&mut stderr)
            .unwrap();
        assert_eq!(exit, Some(code), "SIG{signal}: {stderr}");
        assert!(stdout.is_empty(), "SIG{signal}: {stdout}");
        assert!(
            stderr.contains(&format!("setup cancelled by SIG{signal}")),
            "{stderr}"
        );
        assert_gone(pid, Duration::from_secs(5), "the probe's child");
        assert_eq!(
            fakes.sb.state_files("probe"),
            [] as [PathBuf; 0],
            "SIG{signal}"
        );
    }
}
