//! An invalid `--name` is invalid input (exit 2) for `generate` and `drift`, and is rejected
//! before any server is started.

#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::process::Command;

fn run(command: &str, name: &str) -> (Option<i32>, String) {
    let home = tempfile::tempdir().unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-execution-cli"))
        .args([command, "never-started", "--name", name])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .output()
        .unwrap();
    (
        output.status.code(),
        String::from_utf8_lossy(&output.stderr).into_owned(),
    )
}

#[test]
fn every_invalid_name_exits_2_on_generate_and_drift() {
    for command in ["generate", "drift"] {
        for name in ["con", "My_Server"] {
            let (code, stderr) = run(command, name);

            assert_eq!(code, Some(2), "{command} {name}: {stderr}");
            assert!(stderr.contains("invalid --name"), "{stderr}");
        }
    }
}

#[test]
fn rejected_name_is_not_echoed_raw() {
    let (code, stderr) = run("drift", "a\u{1b}[31m<b>");

    assert_eq!(code, Some(2), "{stderr}");
    assert!(
        !stderr.contains('\u{1b}') && !stderr.contains("<b>"),
        "{stderr}"
    );
}
