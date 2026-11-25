//! Acceptance tests of the sidecar rule of `scan_tools_directory` after it moved to the shared
//! core reader (#618): a `_meta.json` that is not a regular file (a symlink, even one that stays
//! inside the server directory, or a directory) is reported as
//! `ScanError::MetadataEscapesDirectory`, whose message says "not a regular file".

#![cfg(unix)]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use mcp_execution_core::metadata::{
    METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ResultTypeOrigin, ServerMetadata, ToolMetadata,
};
use mcp_execution_core::provenance::GenerationProvenance;
use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_skill::{ScanError, scan_tools_directory};
use std::assert_matches;

fn sidecar_json() -> String {
    let config = ServerConfig::builder()
        .command("docker".to_string())
        .build()
        .unwrap();
    let meta = ServerMetadata::new(
        METADATA_SCHEMA_VERSION,
        ServerId::new("acme").unwrap(),
        "Acme".to_string(),
        "1.0.0".to_string(),
        vec![ToolMetadata::new(
            ToolName::new("get_thing").unwrap(),
            "getThing".to_string(),
            vec![],
            vec![],
            ResultTypeOrigin::Undeclared,
            0,
        )],
        GenerationProvenance::capture(&config, &[]),
    );
    serde_json::to_string(&meta).unwrap()
}

fn package(dir: &std::path::Path) {
    std::fs::write(dir.join("getThing.ts"), "export {};\n").unwrap();
    std::fs::write(dir.join("index.ts"), "export {};\n").unwrap();
}

#[tokio::test]
async fn scan_refuses_a_symlinked_sidecar_that_stays_inside_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    package(dir.path());
    let real = dir.path().join("_meta.real.json");
    std::fs::write(&real, sidecar_json()).unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join(METADATA_FILE_NAME)).unwrap();

    let err = scan_tools_directory(dir.path()).await.unwrap_err();

    assert_matches!(err, ScanError::MetadataEscapesDirectory { .. }, "{err:?}");
    assert!(err.to_string().contains("not a regular file"), "{err}");
}

#[tokio::test]
async fn scan_refuses_a_directory_named_like_the_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    package(dir.path());
    std::fs::create_dir(dir.path().join(METADATA_FILE_NAME)).unwrap();

    let err = scan_tools_directory(dir.path()).await.unwrap_err();

    assert_matches!(err, ScanError::MetadataEscapesDirectory { .. }, "{err:?}");
}
