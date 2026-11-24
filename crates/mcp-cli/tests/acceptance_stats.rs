//! Acceptance tests of the static `stats` command (#562, ADR-562): the discovery levels with
//! bytes and estimated tokens, "not available" for missing data, no figures for a pre-v3
//! `_meta.json`, no "savings" label, and deterministic output.
//!
//! Unix only: the generated package comes from `generate --from-config`, which needs a
//! redirected `mcp.json` (#604).

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use common::Sandbox;
use serde_json::json;

fn prepared() -> Sandbox {
    let sb = Sandbox::new();
    sb.generate();
    sb
}

fn stats(sb: &Sandbox) -> common::Out {
    let servers = sb.servers.display().to_string();
    sb.cli(&["stats", "fixture", "--servers-dir", &servers])
}

#[test]
fn stats_prints_every_level_with_bytes_tokens_and_the_estimator() {
    let sb = prepared();

    let out = stats(&sb);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let lower = out.stdout.to_lowercase();
    for expected in [
        "tool files",
        "one tool file",
        "catalog",
        "bytes",
        "estimate",
    ] {
        assert!(
            lower.contains(expected),
            "missing {expected:?}: {}",
            out.stdout
        );
    }
    assert!(
        out.stdout.contains("bytes/4") || out.stdout.contains("bytes / 4"),
        "{}",
        out.stdout
    );
    assert!(lower.contains("output schema"), "{}", out.stdout);
    assert!(!lower.contains("saving"), "{}", out.stdout);
    assert!(
        !out.stderr.to_lowercase().contains("saving"),
        "{}",
        out.stderr
    );
}

#[test]
fn stats_output_is_byte_identical_across_runs() {
    let sb = prepared();

    let first = stats(&sb);
    let second = stats(&sb);

    assert_eq!(first.code, Some(0), "stderr: {}", first.stderr);
    assert_eq!(first.stdout, second.stdout);
}

#[test]
fn stats_prints_not_available_for_a_missing_catalog_and_never_zero() {
    let sb = prepared();
    std::fs::remove_file(sb.servers.join("fixture").join("catalog.ts")).unwrap();

    let out = stats(&sb);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let line = out
        .stdout
        .lines()
        .find(|l| l.to_lowercase().contains("catalog"))
        .unwrap_or_else(|| panic!("no catalog line: {}", out.stdout));
    assert!(line.contains("not available"), "{line}");
    assert!(
        !line.contains(" 0 "),
        "an unavailable level must not read as zero: {line}"
    );
}

#[test]
fn stats_rejects_a_pre_v3_sidecar_with_exit_2_and_no_figures() {
    let sb = prepared();
    sb.edit_meta("fixture", |meta| meta["schema_version"] = json!(2));

    let out = stats(&sb);

    assert_eq!(out.code, Some(2), "stdout: {}", out.stdout);
    assert!(
        out.stdout.trim().is_empty(),
        "no figures expected: {}",
        out.stdout
    );
    assert!(
        out.stderr.to_lowercase().contains("regenerate"),
        "{}",
        out.stderr
    );
}
