//! Acceptance test of the `rmcp` log cap in the server binary (#678, D2 (c)): with a broad
//! `RUST_LOG=debug`, `rmcp`'s transport must not echo a malformed peer line, which it logs
//! verbatim at debug level. A more specific `rmcp::transport=debug` directive is the documented
//! escape hatch and serves as the positive control, proving the sentinel is observable.
//!
//! Drives the real binary over stdio, since the filter construction is private to `main.rs`.
#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::io::Write as _;
use std::process::{Command, Stdio};

const SENTINEL: &str = "SENTINEL-rmcp-cap-7f3a";

fn stderr_for(rust_log: &str) -> String {
    let home = tempfile::tempdir().expect("temp home");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-execution"))
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("RUST_LOG", rust_log)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-execution");

    let mut stdin = child.stdin.take().expect("stdin");
    writeln!(stdin, "{SENTINEL} is not json").expect("write malformed line");
    drop(stdin);

    let output = child
        .wait_with_output()
        .expect("wait for exit at stdin EOF");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn broad_debug_filter_does_not_echo_a_malformed_peer_line() {
    assert!(!stderr_for("debug").contains(SENTINEL));
}

#[test]
fn specific_rmcp_transport_directive_still_reaches_the_peer_line() {
    assert!(stderr_for("rmcp::transport=debug").contains(SENTINEL));
}

/// Like [`stderr_for`], but completes the MCP handshake, which `rmcp` reports at INFO.
fn handshake_stderr_for(rust_log: &str) -> String {
    let home = tempfile::tempdir().expect("temp home");
    let mut child = Command::new(env!("CARGO_BIN_EXE_mcp-execution"))
        .env("HOME", home.path())
        .env("USERPROFILE", home.path())
        .env("RUST_LOG", rust_log)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn mcp-execution");

    let mut stdin = child.stdin.take().expect("stdin");
    for message in [
        r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"cap-test","version":"1"}}}"#,
        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
    ] {
        writeln!(stdin, "{message}").expect("write handshake");
    }
    drop(stdin);

    let output = child
        .wait_with_output()
        .expect("wait for exit at stdin EOF");
    String::from_utf8_lossy(&output.stderr).into_owned()
}

fn has_rmcp_info(stderr: &str) -> bool {
    stderr
        .lines()
        .any(|line| line.contains("INFO") && line.contains("rmcp::"))
}

/// Acceptance test of #702: the cap only lowers `rmcp` verbosity.
#[test]
fn the_cap_does_not_raise_rmcp_above_a_stricter_rust_log() {
    let control = handshake_stderr_for("info");
    assert!(
        has_rmcp_info(&control),
        "rmcp INFO is not observable: {control}"
    );

    for rust_log in ["error", "warn", "rmcp=error", "info,rmcp=error"] {
        let stderr = handshake_stderr_for(rust_log);

        assert!(!has_rmcp_info(&stderr), "{rust_log}: {stderr}");
    }
}
