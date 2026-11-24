//! `run --help` states the security, stdin and output-format caveats (FR-036.15).

#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::process::Command;

fn run_help() -> String {
    let out = Command::new(env!("CARGO_BIN_EXE_mcp-execution-cli"))
        .args(["run", "--help"])
        .output()
        .expect("run the CLI");
    assert!(out.status.success());
    String::from_utf8_lossy(&out.stdout).into_owned()
}

#[test]
fn help_states_full_privileges_and_no_sandbox() {
    let help = run_help().replace('\n', " ");
    assert!(help.contains("full privileges"), "{help}");
    assert!(help.contains("without a sandbox"), "{help}");
    assert!(help.contains("--restrict"), "{help}");
}

#[test]
fn help_states_that_stdin_is_unavailable() {
    let help = run_help().replace('\n', " ");
    assert!(help.contains("cannot read stdin"), "{help}");
}

#[test]
fn help_says_text_output_is_not_for_pipelines_and_points_to_json() {
    let help = run_help().replace('\n', " ");
    assert!(help.contains("not meant for pipelines"), "{help}");
    assert!(help.contains("--format json"), "{help}");
}

#[test]
fn help_states_that_child_and_worker_calls_are_refused() {
    let help = run_help().replace('\n', " ");
    assert!(help.contains("child process or a worker thread"), "{help}");
}
