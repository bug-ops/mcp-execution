//! Acceptance tests of the `drift` command (#580): exit 0 without drift, exit 5 with drift or a
//! different endpoint, names bounded and printed without descriptions, nothing written, and saved
//! scripts that reference a removed tool listed by name.
//!
//! Drift is staged by editing the recorded `_meta.json` (the fixture's tool list is fixed).
//!
//! Unix only: the tests need `mcp.json`, which cannot be redirected on Windows (#604).

#![cfg(all(feature = "test-fixtures", unix))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use common::{Sandbox, script_text, snapshot};
use serde_json::{Value, json};

const ECHO_DESCRIPTION: &str = "Echoes its arguments back as JSON text";
const DIFFERENT_DIGEST: &str = "0000000000000000000000000000000000000000000000000000000000000000";

fn prepared() -> Sandbox {
    let sb = Sandbox::new();
    sb.generate();
    sb
}

fn drift(sb: &Sandbox) -> common::Out {
    sb.cli(&["drift", "--from-config", "fixture"])
}

/// Replaces the recorded digest and tool list: drops `echo` and records `ghosts` instead.
fn stage_drift(sb: &Sandbox, ghosts: &[String]) {
    sb.edit_meta("fixture", |meta| {
        meta["provenance"]["tool_digest"] = json!(DIFFERENT_DIGEST);
        let tools = meta["tools"].as_array_mut().unwrap();
        let template = tools.iter().find(|t| t["name"] == "echo").unwrap().clone();
        tools.retain(|t| t["name"] != "echo");
        for ghost in ghosts {
            let mut entry: Value = template.clone();
            entry["name"] = json!(ghost);
            entry["typescript_name"] = json!(ghost.replace('_', ""));
            tools.push(entry);
        }
    });
}

#[test]
fn drift_exits_0_when_the_server_matches_the_recorded_generation() {
    let sb = prepared();

    let out = drift(&sb);

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
}

#[test]
fn drift_names_added_and_removed_tools_and_exits_5() {
    let sb = prepared();
    stage_drift(&sb, &["ghost_tool".to_string()]);

    let out = drift(&sb);

    assert_eq!(out.code, Some(5), "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("echo"),
        "live-only tool: {}",
        out.stdout
    );
    assert!(
        out.stdout.contains("ghost_tool"),
        "recorded-only tool: {}",
        out.stdout
    );
    assert!(
        !out.stdout.contains(ECHO_DESCRIPTION),
        "descriptions must not be printed"
    );
    let lower = out.stdout.to_lowercase();
    assert!(
        lower.contains("server changed") || lower.contains("tool list differs"),
        "{}",
        out.stdout
    );
    assert!(!lower.contains("out of date"), "{}", out.stdout);
}

#[test]
fn drift_reports_a_different_endpoint_instead_of_a_tool_difference() {
    let sb = prepared();
    sb.edit_meta("fixture", |meta| {
        meta["provenance"]["config_fingerprint"] = json!(DIFFERENT_DIGEST);
    });

    let out = drift(&sb);

    assert_eq!(out.code, Some(5), "stderr: {}", out.stderr);
    assert!(
        out.stdout.to_lowercase().contains("endpoint"),
        "{}",
        out.stdout
    );
}

#[test]
fn drift_caps_printed_names_at_twenty_with_an_and_n_more_count() {
    let sb = prepared();
    let ghosts: Vec<String> = (0..25).map(|n| format!("ghost_{n:02}")).collect();
    stage_drift(&sb, &ghosts);

    let out = drift(&sb);

    assert_eq!(out.code, Some(5), "stderr: {}", out.stderr);
    assert_eq!(out.stdout.matches("ghost_").count(), 20, "{}", out.stdout);
    assert!(out.stdout.contains("and 5 more"), "{}", out.stdout);
}

#[test]
fn drift_writes_nothing_not_even_the_sidecar() {
    let sb = prepared();
    stage_drift(&sb, &["ghost_tool".to_string()]);
    let before = snapshot(&sb.home.join(".claude"));

    let out = drift(&sb);

    assert_eq!(out.code, Some(5), "stderr: {}", out.stderr);
    assert_eq!(snapshot(&sb.home.join(".claude")), before);
}

#[test]
fn drift_lists_saved_scripts_that_reference_a_removed_tool_by_name_only() {
    let sb = prepared();
    stage_drift(&sb, &["ghost_tool".to_string()]);
    sb.saved_script(
        "fixture",
        "uses-ghost",
        &script_text(
            "uses-ghost",
            "calls the ghost",
            &["fixture/ghost_tool"],
            "// BODY-ghost-9d",
        ),
    );
    sb.saved_script(
        "fixture",
        "uses-list",
        &script_text(
            "uses-list",
            "calls list",
            &["fixture/list_items"],
            "// BODY-list-9d",
        ),
    );

    let out = drift(&sb);

    assert_eq!(out.code, Some(5), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("uses-ghost"), "{}", out.stdout);
    assert!(!out.stdout.contains("uses-list"), "{}", out.stdout);
    assert!(!out.stdout.contains("BODY-") && !out.stdout.contains("calls the ghost"));
}

#[test]
fn drift_that_cannot_complete_has_its_own_exit_code() {
    let sb = Sandbox::new();

    let out = drift(&sb);

    assert!(
        matches!(out.code, Some(1..=4)),
        "no generated directory: expected a could-not-complete code, got {:?}",
        out.code
    );
}
