//! Acceptance tests of `spill_complete` in the `run --format json` stream objects (#635): it says
//! whether the spill file holds the whole stream, and is `null` exactly when `spill` is `null`.
//!
//! Requires the `test-fixtures` feature (implied by `--all-features`) and Node.js 22.18+.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Sandbox, require_node};
use serde_json::Value;

/// Runs `body` as a script with `--format json` and the given `--max-output`, returns the envelope.
fn json_run(test: &str, body: &str, max_output: &str) -> Option<Value> {
    if !require_node(test) {
        return None;
    }
    let sb = Sandbox::new();
    let script = sb.script("spill.ts", body);
    let script_arg = script.display().to_string();
    let servers = sb.servers.display().to_string();
    let out = sb.cli(&[
        "--format",
        "json",
        "run",
        &script_arg,
        "--servers-dir",
        &servers,
        "--max-output",
        max_output,
        "--timeout",
        "60",
    ]);
    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    Some(
        serde_json::from_str(out.stdout.trim()).unwrap_or_else(|e| {
            panic!("stdout is not one JSON document ({e}): {:.300}", out.stdout)
        }),
    )
}

#[test]
fn run_json_reports_spill_complete_false_when_the_capture_limit_cut_the_stream() {
    let Some(envelope) = json_run(
        "run_json_reports_spill_complete_false_when_the_capture_limit_cut_the_stream",
        "process.stdout.write('x'.repeat(9 * 1024 * 1024));\n",
        "100",
    ) else {
        return;
    };

    let stdout = &envelope["stdout"];
    assert!(stdout["spill"].is_string(), "{stdout:.300}");
    assert_eq!(stdout["spill_complete"], Value::Bool(false));
}

#[test]
fn run_json_reports_spill_complete_true_when_only_the_budget_cut_the_output() {
    let Some(envelope) = json_run(
        "run_json_reports_spill_complete_true_when_only_the_budget_cut_the_output",
        "process.stdout.write('0123456789'.repeat(100));\n",
        "100",
    ) else {
        return;
    };

    let stdout = &envelope["stdout"];
    assert!(stdout["spill"].is_string(), "{stdout:.300}");
    assert_eq!(stdout["spill_complete"], Value::Bool(true));
}

#[test]
fn run_json_reports_spill_complete_null_when_nothing_spilled() {
    let Some(envelope) = json_run(
        "run_json_reports_spill_complete_null_when_nothing_spilled",
        "console.log('short');\n",
        "100000",
    ) else {
        return;
    };

    for stream in ["stdout", "stderr"] {
        let object = envelope[stream]
            .as_object()
            .expect("the stream is an object");
        assert!(object["spill"].is_null(), "{stream}");
        assert_eq!(
            object.get("spill_complete"),
            Some(&Value::Null),
            "{stream}: the key is always serialized, null together with spill"
        );
    }
}
