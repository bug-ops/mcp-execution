//! Acceptance test of `--log-format json` in the server binary (#695): the installed logger, not
//! just the parsed flag, must emit JSON.
//!
//! Drives the real binary, since the subscriber setup is private to `main.rs`.

use std::process::{Command, Stdio};

use serde_json::Value;

const STARTUP_MESSAGE: &str = "Starting mcp-execution";

#[test]
fn log_format_json_emits_json_log_lines() {
    let home = tempfile::tempdir().expect("temp home");
    let output = Command::new(env!("CARGO_BIN_EXE_mcp-execution"))
        .args(["--log-format", "json"])
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("RUST_LOG", "info")
        .stdin(Stdio::null())
        .output()
        .expect("run mcp-execution");

    // Only the startup line is checked: stdin EOF before `initialize` also prints a plain error.
    let stderr = String::from_utf8_lossy(&output.stderr);
    let line = stderr
        .lines()
        .find(|line| line.contains(STARTUP_MESSAGE))
        .expect("startup log line");
    let record: Value = serde_json::from_str(line).expect("startup log line is JSON");
    assert_eq!(record["level"], "INFO", "{line}");
    assert!(record["fields"].is_object(), "{line}");
}
