//! Acceptance tests of the terminal foreground hand-over (#646): on a terminal the script's
//! process group becomes the foreground group, so a descendant reading `/dev/tty` is not stopped
//! by SIGTTIN, and the terminal goes back to the runner afterwards.
//!
//! The runner runs under `sh -c` on a pseudo-terminal created by `fixtures/pty_driver.py`
//! (Python 3 is needed; CI hard-fails without it). The script reports its own process group and
//! the terminal's foreground group through `ps`.
//!
//! Unix only: terminals and process groups.

#![cfg(all(feature = "test-fixtures", unix))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use common::{CLI_BIN, Sandbox, WATCHDOG, require_node, require_python, run_with_watchdog};

const DRIVER: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/pty_driver.py");

/// Reports `PGRP <own group> <terminal foreground group>`, then acts by `argv[2]`: `read` makes
/// a child read a line from `/dev/tty`; `idle` announces itself on the terminal and waits.
const SCRIPT: &str = "import { spawnSync } from 'node:child_process';\n\
     import { writeFileSync } from 'node:fs';\n\
     const ps = spawnSync('ps', ['-o', 'pgid=,tpgid=', '-p', String(process.pid)], { encoding: 'utf8' });\n\
     console.log('PGRP ' + ps.stdout.trim().split(/\\s+/).join(' '));\n\
     const mode = process.argv[2];\n\
     if (mode === 'read') {\n\
       const r = spawnSync('sh', ['-c', 'echo ready-for-input > /dev/tty; read l < /dev/tty; echo got:$l'], { encoding: 'utf8' });\n\
       console.log(r.stdout.trim());\n\
     } else if (mode === 'idle') {\n\
       writeFileSync('/dev/tty', 'ready-for-signal\\n');\n\
       setInterval(() => {}, 1000);\n\
     }\n";

/// What `ps -o pgid=,tpgid=` reported for a process.
#[derive(Debug, PartialEq, Eq)]
struct Groups {
    own: u32,
    foreground: u32,
}

impl Groups {
    const fn is_foreground(&self) -> bool {
        self.own == self.foreground
    }
}

/// The terminal output of one run, with its exit code and group reports.
#[derive(Debug)]
struct Session {
    output: String,
    /// `rc=` echoed by the shell after the runner ended.
    runner_code: i32,
    /// The script's own report.
    script: Groups,
    /// The shell's report after the runner ended.
    shell: Groups,
}

fn parse_groups(text: &str) -> Groups {
    let mut numbers = text.split_whitespace().map(|n| n.parse::<u32>().unwrap());
    Groups {
        own: numbers.next().unwrap(),
        foreground: numbers.next().unwrap(),
    }
}

fn parse(output: &str) -> Session {
    let lines: Vec<&str> = output.lines().map(str::trim).collect();
    let script = lines
        .iter()
        .find_map(|line| line.split_once("PGRP ").map(|(_, groups)| groups))
        .unwrap_or_else(|| panic!("no PGRP line in {output:?}"));
    let rc_at = lines
        .iter()
        .position(|line| line.starts_with("rc="))
        .unwrap_or_else(|| panic!("no rc line in {output:?}"));
    let shell = lines[rc_at + 1..]
        .iter()
        .find(|line| !line.is_empty())
        .unwrap_or_else(|| panic!("no shell report in {output:?}"));
    Session {
        output: output.to_string(),
        runner_code: lines[rc_at]["rc=".len()..].parse().unwrap(),
        script: parse_groups(script),
        shell: parse_groups(shell),
    }
}

/// Runs `shell_command` on a pseudo-terminal, typing `TEXT` once `MARKER` shows up for each
/// `(marker, text)` pair.
fn on_pty(sb: &Sandbox, shell_command: &str, typed: &[(&str, &str)]) -> Session {
    let mut cmd = sb.command_for("python3");
    cmd.args(["-I", DRIVER, shell_command]);
    cmd.args(
        typed
            .iter()
            .map(|(marker, text)| format!("{marker}={text}")),
    );
    let out = run_with_watchdog(&mut cmd, None, WATCHDOG);
    assert_eq!(out.code, Some(0), "pty driver failed: {}", out.stderr);
    parse(&out.stdout)
}

/// `run` of the test script in `mode` with the runner flags `flags`, as a shell command line.
fn run_line(sb: &Sandbox, mode: &str, flags: &str) -> String {
    let script = sb.script("tty.mts", SCRIPT);
    format!(
        "'{CLI_BIN}' run '{}' --servers-dir '{}' {flags} -- {mode}",
        script.display(),
        sb.servers.display()
    )
}

const REPORT_SHELL: &str = "echo rc=$?; ps -o pgid=,tpgid= -p $$";

fn sandbox(test: &str) -> Option<Sandbox> {
    (require_node(test) && require_python(test)).then(Sandbox::new)
}

fn assert_shell_owns_the_terminal(session: &Session) {
    assert!(
        session.shell.is_foreground(),
        "the terminal was not given back to the runner's group: {session:?}"
    );
}

#[test]
fn a_descendant_reading_dev_tty_gets_the_typed_line_and_the_terminal_returns() {
    let Some(sb) = sandbox("a_descendant_reading_dev_tty_gets_the_typed_line") else {
        return;
    };
    let run = run_line(&sb, "read", "--timeout 20");
    let session = on_pty(
        &sb,
        &format!("{run}; {REPORT_SHELL}"),
        &[("ready-for-input", "hi\\n")],
    );

    assert!(session.output.contains("got:hi"), "{session:?}");
    assert_eq!(session.runner_code, 0, "{session:?}");
    assert!(session.script.is_foreground(), "{session:?}");
    assert_shell_owns_the_terminal(&session);
}

#[test]
fn the_terminal_returns_to_the_runner_after_a_run_timeout() {
    let Some(sb) = sandbox("the_terminal_returns_to_the_runner_after_a_run_timeout") else {
        return;
    };
    let run = run_line(&sb, "idle", "--timeout 2");
    let session = on_pty(&sb, &format!("{run}; {REPORT_SHELL}"), &[]);

    assert_eq!(session.runner_code, 4, "{session:?}");
    assert!(session.script.is_foreground(), "{session:?}");
    assert_shell_owns_the_terminal(&session);
}

#[test]
fn ctrl_c_on_the_terminal_cancels_the_run_with_exit_130() {
    let Some(sb) = sandbox("ctrl_c_on_the_terminal_cancels_the_run_with_exit_130") else {
        return;
    };
    let run = run_line(&sb, "idle", "--timeout 60");
    let session = on_pty(
        &sb,
        &format!("{run}; {REPORT_SHELL}"),
        &[("ready-for-signal", "\\x03")],
    );

    assert_eq!(session.runner_code, 130, "{session:?}");
    assert_shell_owns_the_terminal(&session);
}

#[test]
fn ctrl_z_does_not_freeze_the_run_and_ctrl_c_still_cancels_it() {
    let Some(sb) = sandbox("ctrl_z_does_not_freeze_the_run_and_ctrl_c_still_cancels_it") else {
        return;
    };
    let run = run_line(&sb, "idle", "--timeout 20");
    let session = on_pty(
        &sb,
        &format!("{run}; {REPORT_SHELL}"),
        &[("ready-for-signal", "\\x1a"), ("^Z", "\\x03")],
    );

    assert_eq!(session.runner_code, 130, "{session:?}");
    assert_shell_owns_the_terminal(&session);
}

#[test]
fn a_background_runner_does_not_take_the_terminal() {
    let Some(sb) = sandbox("a_background_runner_does_not_take_the_terminal") else {
        return;
    };
    let run = run_line(&sb, "none", "--timeout 60");
    let session = on_pty(
        &sb,
        &format!("set -m; {run} & wait $!; {REPORT_SHELL}"),
        &[],
    );

    assert_eq!(session.runner_code, 0, "{session:?}");
    assert!(
        !session.script.is_foreground(),
        "a background runner took the terminal: {session:?}"
    );
}
