//! Acceptance test of `--log-format json` in the CLI binary (#695): the installed logger, not just
//! the parsed flag, must emit JSON.
//!
//! Drives the real binary, since the subscriber is installed once per process.

use std::process::Command;

use serde_json::Value;

#[test]
fn log_format_json_emits_json_log_lines() {
    let home = tempfile::tempdir().expect("temp home");
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-execution-cli"))
        .args(["--log-format", "json", "completions", "bash"])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("RUST_LOG", "info")
        .output()
        .expect("run mcp-execution-cli");
    assert!(output.status.success(), "{output:?}");

    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!stderr.trim().is_empty(), "no log line was written");
    for line in stderr.lines() {
        let record: Value = serde_json::from_str(line).expect("stderr line is JSON");
        assert!(record["level"].is_string(), "{line}");
        assert!(record["fields"].is_object(), "{line}");
    }
}
