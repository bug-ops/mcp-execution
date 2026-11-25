//! Acceptance tests of the typed home-directory failure of `FilesBuilder::build_and_export` (#657):
//! a `~name` base path is reported as `FilesError::HomeDirectory` whose source is the core
//! `HomeDirError`, so callers can classify it instead of reading an I/O kind.

#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use std::error::Error as _;
use std::path::Path;

use mcp_execution_core::HomeDirError;
use mcp_execution_files::{FilesBuilder, FilesError};

fn export_to_user_home() -> FilesError {
    FilesBuilder::new()
        .add_file("/a.ts", "export {};")
        .build_and_export("~other/x")
        .unwrap_err()
}

#[test]
fn build_and_export_reports_a_user_home_base_path_as_a_home_directory_error() {
    let error = export_to_user_home();

    let FilesError::HomeDirectory { source } = &error else {
        panic!("expected FilesError::HomeDirectory, got {error:?}");
    };
    assert!(
        matches!(source, HomeDirError::UnsupportedUserHome { path } if path == Path::new("~other/x")),
        "unexpected home error: {source:?}"
    );
}

#[test]
fn the_home_directory_error_is_reachable_through_the_error_source() {
    let error = export_to_user_home();

    let source = error.source().expect("the error has a source");
    assert!(
        source.downcast_ref::<HomeDirError>().is_some(),
        "the source is not a HomeDirError: {source:?}"
    );
}

#[test]
fn the_home_directory_error_message_does_not_echo_the_base_path() {
    let message = export_to_user_home().to_string();

    assert!(
        message.contains("cannot expand the home directory in the export base path"),
        "{message}"
    );
    assert!(!message.contains("other"), "{message}");
}
