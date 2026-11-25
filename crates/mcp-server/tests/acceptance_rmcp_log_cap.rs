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
