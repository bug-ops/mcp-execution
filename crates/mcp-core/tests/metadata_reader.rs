//! Acceptance tests of the single `_meta.json` reader (#618): it accepts only a regular,
//! non-symlink file of at most `MAX_METADATA_FILE_BYTES` with the current schema version, never
//! blocks on a FIFO, and reports each refusal as its own `MetadataReadError`.
//!
//! The reader takes the server directory that holds `_meta.json`; the directory itself may be a
//! symlink.

#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::path::Path;

use mcp_execution_core::metadata::{
    MAX_METADATA_FILE_BYTES, METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, MetadataReadError,
    ResultTypeOrigin, ServerMetadata, ToolMetadata, read_server_metadata,
    read_server_metadata_async,
};
use mcp_execution_core::provenance::GenerationProvenance;
use mcp_execution_core::{ServerConfig, ServerId, ToolName};

fn sidecar() -> ServerMetadata {
    let config = ServerConfig::builder()
        .command("docker".to_string())
        .build()
        .unwrap();
    ServerMetadata {
        schema_version: METADATA_SCHEMA_VERSION,
        server_id: ServerId::new("acme").unwrap(),
        server_name: "Acme".to_string(),
        server_version: "1.0.0".to_string(),
        tools: vec![ToolMetadata {
            name: ToolName::new("get_thing").unwrap(),
            typescript_name: "getThing".to_string(),
            category: None,
            keywords: vec![],
            description: None,
            parameters: vec![],
            annotations: None,
            result_type: None,
            result_type_origin: ResultTypeOrigin::Undeclared,
            manifest_size_bytes: 0,
        }],
        provenance: GenerationProvenance::capture(&config, &[]),
    }
}

fn write_sidecar(dir: &Path, text: &str) {
    std::fs::write(dir.join(METADATA_FILE_NAME), text).unwrap();
}

fn valid_json() -> String {
    serde_json::to_string(&sidecar()).unwrap()
}

#[test]
fn read_server_metadata_returns_a_valid_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    let expected = sidecar();
    write_sidecar(dir.path(), &serde_json::to_string(&expected).unwrap());

    let meta = read_server_metadata(dir.path()).unwrap();

    assert_eq!(meta, expected);
}

#[cfg(unix)]
#[test]
fn read_server_metadata_follows_a_symlinked_server_directory() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("real");
    std::fs::create_dir(&real).unwrap();
    write_sidecar(&real, &valid_json());
    let link = dir.path().join("link");
    std::os::unix::fs::symlink(&real, &link).unwrap();

    assert!(read_server_metadata(&link).is_ok());
}

#[test]
fn read_server_metadata_reports_a_missing_sidecar_as_not_found() {
    let dir = tempfile::tempdir().unwrap();

    let err = read_server_metadata(dir.path()).unwrap_err();

    assert!(matches!(err, MetadataReadError::NotFound), "{err:?}");
}

#[cfg(unix)]
#[test]
fn read_server_metadata_refuses_a_symlinked_sidecar_even_inside_the_directory() {
    let dir = tempfile::tempdir().unwrap();
    let real = dir.path().join("other.json");
    std::fs::write(&real, valid_json()).unwrap();
    std::os::unix::fs::symlink(&real, dir.path().join(METADATA_FILE_NAME)).unwrap();

    let err = read_server_metadata(dir.path()).unwrap_err();

    assert!(matches!(err, MetadataReadError::Link), "{err:?}");
}

#[test]
fn read_server_metadata_refuses_a_directory_named_like_the_sidecar() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir(dir.path().join(METADATA_FILE_NAME)).unwrap();

    let err = read_server_metadata(dir.path()).unwrap_err();

    assert!(matches!(err, MetadataReadError::NotRegularFile), "{err:?}");
}

#[cfg(unix)]
#[test]
fn read_server_metadata_refuses_a_fifo_without_blocking() {
    let dir = tempfile::tempdir().unwrap();
    let fifo = dir.path().join(METADATA_FILE_NAME);
    let made = std::process::Command::new("mkfifo")
        .arg(&fifo)
        .status()
        .unwrap();
    assert!(made.success(), "mkfifo failed");
    let (tx, rx) = std::sync::mpsc::channel();
    let path = dir.path().to_path_buf();
    std::thread::spawn(move || {
        let _ = tx.send(read_server_metadata(&path));
    });

    let received = rx.recv_timeout(std::time::Duration::from_secs(10));

    assert!(received.is_ok(), "the reader blocked on a FIFO");
    let outcome = received.unwrap();
    assert!(
        matches!(outcome, Err(MetadataReadError::NotRegularFile)),
        "{outcome:?}"
    );
}

#[test]
fn read_server_metadata_accepts_exactly_the_size_cap_and_refuses_one_byte_more() {
    let cap = usize::try_from(MAX_METADATA_FILE_BYTES).unwrap();
    let json = valid_json();
    let dir = tempfile::tempdir().unwrap();

    write_sidecar(
        dir.path(),
        &format!("{json}{}", " ".repeat(cap - json.len())),
    );
    assert!(read_server_metadata(dir.path()).is_ok(), "exactly the cap");

    write_sidecar(
        dir.path(),
        &format!("{json}{}", " ".repeat(cap - json.len() + 1)),
    );
    let err = read_server_metadata(dir.path()).unwrap_err();
    assert!(
        matches!(err, MetadataReadError::TooLarge { limit, .. } if limit == MAX_METADATA_FILE_BYTES),
        "{err:?}"
    );
}

#[test]
fn read_server_metadata_checks_the_schema_version_before_the_typed_parse() {
    let dir = tempfile::tempdir().unwrap();
    write_sidecar(dir.path(), r#"{"schema_version":1,"server_id":"acme"}"#);

    let err = read_server_metadata(dir.path()).unwrap_err();

    assert!(
        matches!(
            err,
            MetadataReadError::SchemaMismatch { found: 1, expected }
                if expected == METADATA_SCHEMA_VERSION
        ),
        "{err:?}"
    );
}

#[test]
fn read_server_metadata_reports_json_that_is_not_a_sidecar_as_malformed() {
    let dir = tempfile::tempdir().unwrap();

    write_sidecar(dir.path(), "{not json");
    let broken = read_server_metadata(dir.path()).unwrap_err();
    write_sidecar(
        dir.path(),
        &format!(r#"{{"schema_version":{METADATA_SCHEMA_VERSION}}}"#),
    );
    let incomplete = read_server_metadata(dir.path()).unwrap_err();

    assert!(
        matches!(broken, MetadataReadError::Malformed(_)),
        "{broken:?}"
    );
    assert!(
        matches!(incomplete, MetadataReadError::Malformed(_)),
        "{incomplete:?}"
    );
}

#[test]
fn metadata_read_error_messages_do_not_contain_the_path() {
    let dir = tempfile::tempdir().unwrap();
    let needle = dir.path().display().to_string();
    write_sidecar(dir.path(), "{not json");
    let malformed = read_server_metadata(dir.path()).unwrap_err();
    let missing = read_server_metadata(&dir.path().join("absent")).unwrap_err();

    assert!(!malformed.to_string().contains(&needle), "{malformed}");
    assert!(!missing.to_string().contains(&needle), "{missing}");
}

#[tokio::test]
async fn read_server_metadata_async_matches_the_sync_reader() {
    let dir = tempfile::tempdir().unwrap();
    let expected = sidecar();
    write_sidecar(dir.path(), &serde_json::to_string(&expected).unwrap());

    let ok = read_server_metadata_async(dir.path().to_path_buf()).await;
    let missing = read_server_metadata_async(dir.path().join("absent")).await;

    assert_eq!(ok.unwrap(), expected);
    assert!(matches!(missing, Err(MetadataReadError::NotFound)));
}
