//! Acceptance tests of the call journal (#572): no values or server text on disk, an in-flight
//! call at failure reads as unknown, the summary appears on failure only and carries a stable
//! digest prefix, and the journal is bounded and private.
//!
//! Unix only: the file-mode and retention tests, which check permission bits and seed files
//! through Unix-only helpers.

#![cfg(feature = "test-fixtures")]

mod common;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt as _;

use common::{Out, Sandbox, require_node};
use mcp_execution_cli::runtime::journal::{MAX_JOURNAL_BYTES, canonical_args};
use serde_json::json;

const SECRET: &str = "SECRET-7c1e-argument-value";

fn prepared(test: &str) -> Option<Sandbox> {
    if !require_node(test) {
        return None;
    }
    let sb = Sandbox::new();
    sb.generate();
    Some(sb)
}

fn run_body(sb: &Sandbox, body: &str, extra: &[&str]) -> Out {
    let script = sb.script(
        "journal-script.ts",
        &format!("import {{ echo, error, slow }} from 'mcp:fixture';\n{body}\n"),
    );
    sb.run_script(&script, extra)
}

/// Whitespace and punctuation separated words of `text` that look like a 12 hex digit prefix.
fn digest_prefixes(text: &str) -> Vec<&str> {
    text.split(|c: char| !c.is_ascii_alphanumeric())
        .filter(|w| w.len() == 12 && w.chars().all(|c| c.is_ascii_hexdigit()))
        .collect()
}

#[test]
fn canonical_args_is_independent_of_key_order_at_every_depth() {
    let one = canonical_args(&json!({"b": 2, "a": {"y": [1, {"q": 1, "p": 2}], "x": 2}}));
    let reordered = canonical_args(&json!({"a": {"x": 2, "y": [1, {"p": 2, "q": 1}]}, "b": 2}));
    let different = canonical_args(&json!({"a": {"x": 3, "y": [1, {"p": 2, "q": 1}]}, "b": 2}));

    assert_eq!(one, reordered);
    assert_ne!(one, different);
}

#[test]
fn journal_holds_no_argument_result_or_server_text() {
    let Some(sb) = prepared("journal_holds_no_argument_result_or_server_text") else {
        return;
    };

    let out = run_body(
        &sb,
        &format!(
            "await echo({{ marker: '{SECRET}' }});\n\
             try {{ await error({{}}); }} catch {{}}\n"
        ),
        &[],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let files = sb.state_files("journals");
    assert!(!files.is_empty(), "the run must leave a journal");
    for file in files {
        let text = std::fs::read_to_string(&file).unwrap();
        assert!(
            !text.contains(SECRET),
            "argument or result value in {}",
            file.display()
        );
        assert!(
            !text.contains("fixture tool error"),
            "server text in {}",
            file.display()
        );
    }
}

#[cfg(unix)]
#[test]
fn journal_files_are_readable_only_by_the_user() {
    let Some(sb) = prepared("journal_files_are_readable_only_by_the_user") else {
        return;
    };

    let out = run_body(&sb, "await echo({});", &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let files = sb.state_files("journals");
    assert!(!files.is_empty(), "the run must leave a journal");
    for file in files {
        let mode = std::fs::metadata(&file).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "{}", file.display());
    }
}

#[test]
fn journal_summary_marks_a_call_in_flight_at_timeout_as_unknown() {
    let Some(sb) = prepared("journal_summary_marks_a_call_in_flight_at_timeout_as_unknown") else {
        return;
    };

    let out = run_body(
        &sb,
        "await echo({});\n\
         slow({ ms: 30000 }).catch(() => {});\n\
         setInterval(() => {}, 1000);",
        &["--timeout", "2"],
    );

    assert_eq!(out.code, Some(4), "stderr: {}", out.stderr);
    let lower = out.stdout.to_lowercase();
    assert!(
        lower.contains("slow") && lower.contains("unknown"),
        "{}",
        out.stdout
    );
    assert!(
        lower.contains("echo"),
        "completed calls are listed: {}",
        out.stdout
    );
}

#[test]
fn journal_summary_is_printed_on_failure_only() {
    let Some(sb) = prepared("journal_summary_is_printed_on_failure_only") else {
        return;
    };

    let ok = run_body(&sb, "await echo({ n: 1 });", &[]);
    let failed = run_body(&sb, "await echo({ n: 1 });\nprocess.exit(1);", &[]);

    assert_eq!(ok.code, Some(0), "stderr: {}", ok.stderr);
    assert!(
        !ok.stdout.contains("echo"),
        "summary on success: {}",
        ok.stdout
    );
    assert_eq!(failed.code, Some(6), "stderr: {}", failed.stderr);
    assert!(failed.stdout.contains("echo"), "{}", failed.stdout);
    assert_eq!(
        digest_prefixes(&failed.stdout).len(),
        1,
        "{}",
        failed.stdout
    );
}

#[test]
fn journal_digest_ignores_key_order_and_is_keyed_per_user() {
    let Some(first) = prepared("journal_digest_ignores_key_order_and_is_keyed_per_user") else {
        return;
    };
    let body = "await echo({ a: 1, b: { x: 1, y: 2 } });\n\
                await echo({ b: { y: 2, x: 1 }, a: 1 });\n\
                await echo({ a: 2 });\n\
                process.exit(1);";

    let one = run_body(&first, body, &[]);
    let again = run_body(&first, body, &[]);
    let Some(other) = prepared("journal_digest_ignores_key_order_and_is_keyed_per_user") else {
        return;
    };
    let elsewhere = run_body(&other, body, &[]);

    let digests = digest_prefixes(&one.stdout);
    assert_eq!(digests.len(), 3, "{}", one.stdout);
    assert_eq!(
        digests[0], digests[1],
        "key order must not change the digest"
    );
    assert_ne!(digests[0], digests[2]);
    assert_eq!(
        digest_prefixes(&again.stdout),
        digests,
        "digests must be comparable across runs of one user"
    );
    assert_ne!(
        digest_prefixes(&elsewhere.stdout)[0],
        digests[0],
        "the digest must be keyed per user"
    );
}

#[test]
fn journal_is_bounded_and_marked_truncated_when_entries_are_dropped() {
    let Some(sb) = prepared("journal_is_bounded_and_marked_truncated_when_entries_are_dropped")
    else {
        return;
    };

    let out = run_body(
        &sb,
        "for (let batch = 0; batch < 25; batch++) {\n\
           await Promise.all(Array.from({ length: 100 }, () => echo({})));\n\
         }",
        &["--timeout", "120"],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let files = sb.state_files("journals");
    assert!(!files.is_empty(), "the run must leave a journal");
    let mut truncated = false;
    for file in files {
        let len = std::fs::metadata(&file).unwrap().len();
        assert!(
            len <= MAX_JOURNAL_BYTES,
            "{} is {len} bytes",
            file.display()
        );
        truncated |= std::fs::read_to_string(&file)
            .unwrap()
            .to_lowercase()
            .contains("truncated");
    }
    assert!(truncated, "a journal that dropped entries must say so");
}

#[test]
fn journal_lines_stay_whole_json_objects_under_concurrent_calls() {
    let Some(sb) = prepared("journal_lines_stay_whole_json_objects_under_concurrent_calls") else {
        return;
    };

    let out = run_body(
        &sb,
        "await Promise.all(Array.from({ length: 100 }, (_, n) => echo({ n })));",
        &[],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let files = sb.state_files("journals");
    assert!(!files.is_empty(), "the run must leave a journal");
    let mut lines = 0_usize;
    for file in files {
        for line in std::fs::read_to_string(&file).unwrap().lines() {
            let entry: serde_json::Value = serde_json::from_str(line)
                .unwrap_or_else(|e| panic!("interleaved or broken journal line {line:?}: {e}"));
            assert!(entry.is_object(), "{line}");
            lines += 1;
        }
    }
    assert!(
        lines >= 200,
        "a start and an end entry per call, got {lines} lines"
    );
}

#[cfg(unix)]
#[test]
fn journal_retention_drops_expired_files_and_keeps_at_most_the_file_limit() {
    let Some(sb) =
        prepared("journal_retention_drops_expired_files_and_keeps_at_most_the_file_limit")
    else {
        return;
    };
    let journals = sb.state_root().join("journals");
    let expired = common::seed_file(&journals, "seed-expired.jsonl", 10, 8 * 24 * 3600);
    let seeded: Vec<std::path::PathBuf> = (1..=60)
        .map(|n| common::seed_file(&journals, &format!("seed-{n:02}.jsonl"), 10, n * 360))
        .collect();

    let out = run_body(&sb, "await echo({});", &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(!expired.exists(), "an 8 day old journal must be removed");
    let remaining = common::files_below(&journals).len();
    assert!(
        remaining <= mcp_execution_cli::runtime::journal::MAX_JOURNAL_FILES,
        "{remaining} journals remain"
    );
    assert!(seeded[0].exists(), "the newest seeded journal must survive");
    assert!(
        !seeded[59].exists(),
        "the oldest seeded journal must be pruned"
    );
}
