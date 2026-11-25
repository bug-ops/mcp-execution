//! Acceptance tests of the terminal keys during a hand-over (#693): when SIGINT or SIGQUIT was
//! ignored at startup, the matching terminal key is disabled while the script runs and restored
//! afterwards.
//!
//! The runner runs under `sh -c` on the pseudo-terminal of `fixtures/pty_driver.py`; `stty -a` is
//! printed to the terminal before the run, by the script during it, and after it.
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

/// Prints the terminal settings between `DURING` markers, then acts by `argv[2]`:
/// - `exit`: exits at once;
/// - `wait`: announces `ready-for-key` and exits 0 after 2 s;
/// - `idle`: waits until stopped;
/// - `term-runner`: sends SIGTERM to the runner and waits;
/// - `set-intr`: a child sets `intr` to `^G`.
const SCRIPT: &str = r"import { spawnSync } from 'node:child_process';
import { writeFileSync } from 'node:fs';
spawnSync('sh', ['-c', '{ echo DURING-BEGIN; stty -a < /dev/tty; echo DURING-END; } > /dev/tty'], { stdio: 'ignore' });
const mode = process.argv[2];
if (mode === 'wait') {
  writeFileSync('/dev/tty', 'ready-for-key\n');
  setTimeout(() => process.exit(0), 2000);
} else if (mode === 'idle') {
  setInterval(() => {}, 1000);
} else if (mode === 'term-runner') {
  process.kill(process.ppid, 'SIGTERM');
  setInterval(() => {}, 1000);
} else if (mode === 'set-intr') {
  spawnSync('sh', ['-c', 'stty intr ^G < /dev/tty'], { stdio: 'ignore' });
}
";

/// The `intr` and `quit` characters as `stty -a` prints them.
#[derive(Debug, Clone, PartialEq, Eq)]
struct Keys {
    intr: String,
    quit: String,
}

#[derive(Debug)]
struct Session {
    output: String,
    runner_code: i32,
    before: Keys,
    during: Keys,
    after: Keys,
}

fn key(section: &str, name: &str) -> String {
    let (_, rest) = section
        .split_once(&format!("{name} = "))
        .unwrap_or_else(|| panic!("no {name} in {section:?}"));
    rest.split(';').next().unwrap().trim().to_string()
}

fn keys(output: &str, label: &str) -> Keys {
    let (_, rest) = output
        .split_once(&format!("{label}-BEGIN"))
        .unwrap_or_else(|| panic!("no {label} settings in {output:?}"));
    let (section, _) = rest.split_once(&format!("{label}-END")).unwrap();
    Keys {
        intr: key(section, "intr"),
        quit: key(section, "quit"),
    }
}

/// Runs the script in `mode` on a pseudo-terminal, with the signals in `ignored` (for example
/// `INT QUIT`) ignored in the shell that execs the runner, typing `typed` at `ready-for-key`.
fn session(test: &str, ignored: &str, mode: &str, typed: Option<&str>) -> Option<Session> {
    if !(require_node(test) && require_python(test)) {
        return None;
    }
    let sb = Sandbox::new();
    let script = sb.script("keys.mts", SCRIPT);
    let run = format!(
        "'{CLI_BIN}' run '{}' --servers-dir '{}' --timeout 30 -- {mode}",
        script.display(),
        sb.servers.display()
    );
    let trap = if ignored.is_empty() {
        String::new()
    } else {
        format!("trap '' {ignored}; ")
    };
    let report = |label: &str| format!("echo {label}-BEGIN; stty -a; echo {label}-END");
    let shell = format!(
        "{}; ({trap}exec {run}); echo rc=$?; {}",
        report("BEFORE"),
        report("AFTER")
    );
    let mut cmd = sb.command_for("python3");
    cmd.args(["-I", DRIVER, &shell]);
    if let Some(text) = typed {
        cmd.arg(format!("ready-for-key={text}"));
    }
    let out = run_with_watchdog(&mut cmd, None, WATCHDOG);
    assert_eq!(out.code, Some(0), "pty driver failed: {}", out.stderr);
    let output = out.stdout;
    let runner_code = output
        .lines()
        .find_map(|line| line.trim().split_once("rc=").map(|(_, code)| code))
        .unwrap_or_else(|| panic!("no rc line in {output:?}"))
        .parse()
        .unwrap();
    Some(Session {
        runner_code,
        before: keys(&output, "BEFORE"),
        during: keys(&output, "DURING"),
        after: keys(&output, "AFTER"),
        output,
    })
}

const DISABLED: &str = "<undef>";

fn assert_disabled_then_restored(session: &Session) {
    assert_eq!(session.during.intr, DISABLED, "{session:?}");
    assert_eq!(session.during.quit, DISABLED, "{session:?}");
    assert_eq!(session.after, session.before, "{session:?}");
}

#[test]
fn hand_over_ignored_sigint_makes_ctrl_c_harmless() {
    let Some(session) = session(
        "hand_over_ignored_sigint_makes_ctrl_c_harmless",
        "INT",
        "wait",
        Some("\\x03"),
    ) else {
        return;
    };

    assert_eq!(session.runner_code, 0, "{}", session.output);
    assert_eq!(session.during.intr, DISABLED, "{session:?}");
    assert_eq!(session.after, session.before, "{session:?}");
}

#[test]
fn hand_over_ignored_sigquit_makes_ctrl_backslash_harmless() {
    let Some(session) = session(
        "hand_over_ignored_sigquit_makes_ctrl_backslash_harmless",
        "QUIT",
        "wait",
        Some("\\x1c"),
    ) else {
        return;
    };

    assert_eq!(session.runner_code, 0, "{}", session.output);
    assert_eq!(session.during.quit, DISABLED, "{session:?}");
    assert_eq!(session.after, session.before, "{session:?}");
}

/// Only the ignored signal's key is disabled, so Ctrl-C still cancels when only SIGQUIT is
/// ignored.
#[test]
fn hand_over_ignored_sigquit_only_leaves_ctrl_c_working() {
    let Some(session) = session(
        "hand_over_ignored_sigquit_only_leaves_ctrl_c_working",
        "QUIT",
        "wait",
        Some("\\x03"),
    ) else {
        return;
    };

    assert_eq!(session.runner_code, 130, "{}", session.output);
    assert_eq!(session.during.intr, session.before.intr, "{session:?}");
    assert_eq!(session.during.quit, DISABLED, "{session:?}");
    assert_eq!(session.after, session.before, "{session:?}");
}

#[test]
fn restore_after_a_script_that_exits_at_once() {
    let Some(session) = session(
        "restore_after_a_script_that_exits_at_once",
        "INT QUIT",
        "exit",
        None,
    ) else {
        return;
    };

    assert_eq!(session.runner_code, 0, "{}", session.output);
    assert_disabled_then_restored(&session);
}

#[test]
fn restore_after_a_run_cancelled_by_sigterm() {
    let Some(session) = session(
        "restore_after_a_run_cancelled_by_sigterm",
        "INT QUIT",
        "term-runner",
        None,
    ) else {
        return;
    };

    assert_eq!(session.runner_code, 143, "{}", session.output);
    assert_disabled_then_restored(&session);
}

#[test]
fn restore_keeps_a_key_the_script_set_itself() {
    let Some(session) = session(
        "restore_keeps_a_key_the_script_set_itself",
        "INT",
        "set-intr",
        None,
    ) else {
        return;
    };

    assert_eq!(session.runner_code, 0, "{}", session.output);
    assert_eq!(session.during.intr, DISABLED, "{session:?}");
    assert_eq!(session.after.intr, "^G", "{session:?}");
    assert_eq!(session.after.quit, session.before.quit, "{session:?}");
}
