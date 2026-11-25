//! Acceptance tests of the `run --help` notes added for 1.0: the `.mts` workaround for Node.js
//! 22.18 (#690) and the recovery from keys left disabled by a killed runner (#693).

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
    String::from_utf8_lossy(&out.stdout)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn run_help_names_mts_for_top_level_for_await_on_node_22_18() {
    let help = run_help();

    assert!(help.contains("22.18"), "{help}");
    assert!(help.contains("for await"), "{help}");
    assert!(help.contains(".mts"), "{help}");
}

#[test]
fn run_help_names_stty_sane_for_keys_left_disabled() {
    let help = run_help();

    assert!(help.contains("stty sane"), "{help}");
}
