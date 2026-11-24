//! The dry-run report offers a read-only suggestion for a skipped write whose recorded
//! `_meta.json` annotations say read-only, without ever letting that hint execute the call.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`).

#![cfg(feature = "test-fixtures")]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use common::Sandbox;
use mcp_execution_cli::runtime::policy::{DryRun, Policy};
use mcp_execution_core::{ServerId, ToolName};

fn skip(dry_run: &DryRun, tool: &str) -> bool {
    dry_run.skips(
        &ServerId::new("fixture").unwrap(),
        &ToolName::new(tool).unwrap(),
        None,
        2,
    )
}

#[test]
fn a_skipped_write_that_claims_read_only_gets_a_suggestion_and_is_still_skipped() {
    let sb = Sandbox::new();
    sb.generate();
    let dry_run = DryRun::new(Policy::default(), sb.servers);

    assert!(skip(&dry_run, "delete_item"), "a hint must not execute");
    assert!(skip(&dry_run, "create_item"));

    let report = dry_run.report().unwrap();
    let line_of = |tool: &str| {
        report
            .lines()
            .find(|l| l.contains(&format!("fixture/{tool}")))
            .unwrap()
            .to_string()
    };
    assert!(
        line_of("delete_item").contains("marks it read-only"),
        "{report}"
    );
    assert!(!line_of("create_item").contains("read-only"), "{report}");
}

#[test]
fn without_recorded_metadata_there_is_no_suggestion() {
    let dry_run = DryRun::new(Policy::default(), "/nonexistent-servers-dir".into());

    assert!(skip(&dry_run, "delete_item"));

    assert!(!dry_run.report().unwrap().contains("read-only"));
}
