//! Acceptance tests of `read_server_metadata` for a `_meta.json` that is a symlink to something
//! that would block or mislead a reader (#618, TOCTOU hardening): the link is refused as `Link`
//! without opening its target, so a FIFO behind it never blocks the read, and a dangling link is
//! a refused link, not a missing sidecar.
//!
//! Unix only.

#![cfg(unix)]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::path::Path;
use std::sync::mpsc;
use std::time::Duration;

use mcp_execution_core::metadata::{
    METADATA_FILE_NAME, MetadataReadError, ServerMetadata, read_server_metadata,
};

/// Reads `dir` on a helper thread and fails the test if the read does not return within 10 s.
fn read_promptly(dir: &Path) -> Result<ServerMetadata, MetadataReadError> {
    let (tx, rx) = mpsc::channel();
    let dir = dir.to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(read_server_metadata(&dir));
    });
    let received = rx.recv_timeout(Duration::from_secs(10));
    assert!(received.is_ok(), "the reader blocked");
    received.unwrap()
}

#[test]
fn read_server_metadata_refuses_a_symlink_to_a_fifo_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join("pipe");
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo failed");
    std::os::unix::fs::symlink(&fifo, dir.path().join(METADATA_FILE_NAME)).unwrap();

    let outcome = read_promptly(dir.path());

    assert!(
        matches!(outcome, Err(MetadataReadError::Link)),
        "{outcome:?}"
    );
}

#[test]
fn read_server_metadata_refuses_a_symlink_to_a_device_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/dev/zero", dir.path().join(METADATA_FILE_NAME)).unwrap();

    let outcome = read_promptly(dir.path());

    assert!(
        matches!(outcome, Err(MetadataReadError::Link)),
        "{outcome:?}"
    );
}

#[test]
fn read_server_metadata_reports_a_dangling_symlink_as_a_link_not_as_missing() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(
        dir.path().join("does-not-exist"),
        dir.path().join(METADATA_FILE_NAME),
    )
    .unwrap();

    let outcome = read_promptly(dir.path());

    assert!(
        matches!(outcome, Err(MetadataReadError::Link)),
        "{outcome:?}"
    );
}
