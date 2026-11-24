//! Acceptance tests of `run --dry-run` and the write policy (#573): only exact `reads` policy
//! entries execute, a server's read-only hint never does, a skipped write yields `write_skipped`
//! and no value, and a bad policy fails before any call without echoing its contents.
//!
//! The policy file is `{"version": 1, "reads": ["<server>/<tool>"]}` (D19): unknown fields and any
//! other version are rejected, and each entry is split at the first `/` and validated as a
//! server id and a tool name.
//!
//! Unix only: the tests need `mcp.json`, which cannot be redirected on Windows (#604).

#![cfg(all(feature = "test-fixtures", unix))]
#![expect(
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

mod common;

use common::{Out, Sandbox, require_node};
use serde_json::json;

const POLICY_SECRET: &str = "POLICY-SECRET-4d2b";

fn prepared(test: &str) -> Option<Sandbox> {
    if !require_node(test) {
        return None;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    Some(sb)
}

fn policy(sb: &Sandbox, body: &serde_json::Value) -> String {
    let path = sb.root.join("policy.json");
    std::fs::write(&path, body.to_string()).unwrap();
    path.display().to_string()
}

fn dry_run(sb: &Sandbox, body: &str, extra: &[&str]) -> Out {
    let script = sb.script(
        "dry.ts",
        &format!(
            "import {{ listItems, createItem, deleteItem }} from 'mcp:fixture';\n\
             const attempt = async (name: string, call: () => Promise<unknown>) => {{\n\
               try {{ console.log('VALUE ' + name + '=' + JSON.stringify(await call())); }}\n\
               catch (e: any) {{ console.log('KIND ' + name + '=' + e.kind); }}\n\
             }};\n{body}\n"
        ),
    );
    let mut args = vec!["--dry-run"];
    args.extend_from_slice(extra);
    sb.run_script(&script, &args)
}

fn called(sb: &Sandbox, tool: &str) -> bool {
    sb.fixture_log()
        .iter()
        .any(|l| l == &format!("call {tool}"))
}

#[test]
fn dry_run_does_not_execute_a_write_that_claims_to_be_read_only() {
    let Some(sb) = prepared("dry_run_does_not_execute_a_write_that_claims_to_be_read_only") else {
        return;
    };

    let out = dry_run(&sb, "await attempt('delete', () => deleteItem({}));", &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(!called(&sb, "delete_item"), "log: {:?}", sb.fixture_log());
}

#[test]
fn dry_run_executes_a_tool_listed_under_reads_and_nothing_else() {
    let Some(sb) = prepared("dry_run_executes_a_tool_listed_under_reads_and_nothing_else") else {
        return;
    };
    let policy = policy(&sb, &json!({"version": 1, "reads": ["fixture/list_items"]}));

    let out = dry_run(
        &sb,
        "await attempt('list', () => listItems({}));\n\
         await attempt('create', () => createItem({}));\n\
         await attempt('delete', () => deleteItem({}));",
        &["--policy", &policy],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#"VALUE list=["alpha","beta"]"#),
        "{}",
        out.stdout
    );
    assert!(called(&sb, "list_items"));
    assert!(!called(&sb, "create_item") && !called(&sb, "delete_item"));
    for skipped in ["create_item", "delete_item"] {
        assert!(
            out.stdout.contains(skipped),
            "the report must name the skipped write {skipped}: {}",
            out.stdout
        );
    }
    assert!(
        !out.stdout.contains("list_items"),
        "an executed read is not a skipped write: {}",
        out.stdout
    );
    assert!(
        out.stdout.to_lowercase().contains("incomplete"),
        "{}",
        out.stdout
    );
}

#[test]
fn dry_run_with_only_reads_skips_nothing_and_reports_no_incompleteness() {
    let Some(sb) = prepared("dry_run_with_only_reads_skips_nothing_and_reports_no_incompleteness")
    else {
        return;
    };
    let policy = policy(&sb, &json!({"version": 1, "reads": ["fixture/list_items"]}));

    let out = dry_run(
        &sb,
        "await attempt('list', () => listItems({}));",
        &["--policy", &policy],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains(r#"VALUE list=["alpha","beta"]"#),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.to_lowercase().contains("incomplete"),
        "no write was skipped: {}",
        out.stdout
    );
}

#[test]
fn dry_run_policy_matches_exact_names_only() {
    let Some(sb) = prepared("dry_run_policy_matches_exact_names_only") else {
        return;
    };
    let policy = policy(
        &sb,
        &json!({"version": 1, "reads": ["fixture/list_item", "other/list_items", "fixture/list_items_2"]}),
    );

    let out = dry_run(
        &sb,
        "await attempt('list', () => listItems({}));",
        &["--policy", &policy],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(!called(&sb, "list_items"), "log: {:?}", sb.fixture_log());
}

#[test]
fn dry_run_without_a_policy_file_treats_every_call_as_a_write() {
    let Some(sb) = prepared("dry_run_without_a_policy_file_treats_every_call_as_a_write") else {
        return;
    };

    let out = dry_run(&sb, "await attempt('list', () => listItems({}));", &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(!called(&sb, "list_items"), "log: {:?}", sb.fixture_log());
}

#[test]
fn dry_run_skipped_write_rejects_with_write_skipped_and_returns_no_value() {
    let Some(sb) =
        prepared("dry_run_skipped_write_rejects_with_write_skipped_and_returns_no_value")
    else {
        return;
    };

    let out = dry_run(&sb, "await attempt('create', () => createItem({}));", &[]);

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("KIND create=write_skipped"),
        "{}",
        out.stdout
    );
    assert!(!out.stdout.contains("VALUE create="), "{}", out.stdout);
}

#[test]
fn dry_run_report_lists_each_skipped_write_and_says_it_is_incomplete() {
    let Some(sb) = prepared("dry_run_report_lists_each_skipped_write_and_says_it_is_incomplete")
    else {
        return;
    };

    let out = dry_run(
        &sb,
        "await attempt('create', () => createItem({ n: 1 }));\n\
         await attempt('delete', () => deleteItem({ n: 2 }));",
        &[],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    for tool in ["create_item", "delete_item"] {
        assert!(
            out.stdout.contains(tool),
            "report must name {tool}: {}",
            out.stdout
        );
    }
    assert!(out.stdout.contains("fixture"));
    assert!(
        out.stdout.to_lowercase().contains("incomplete"),
        "{}",
        out.stdout
    );
}

#[test]
fn dry_run_rejects_an_invalid_policy_before_any_call_without_echoing_it() {
    let Some(sb) = prepared("dry_run_rejects_an_invalid_policy_before_any_call_without_echoing_it")
    else {
        return;
    };
    let path = sb.root.join("bad-policy.json");
    std::fs::write(&path, format!(r#"{{"reads": ["{POLICY_SECRET}"#)).unwrap();

    let out = dry_run(
        &sb,
        "await attempt('list', () => listItems({}));",
        &["--policy", &path.display().to_string()],
    );

    assert_eq!(out.code, Some(2), "stdout: {}", out.stdout);
    assert!(!out.stdout.contains(POLICY_SECRET) && !out.stderr.contains(POLICY_SECRET));
    assert!(sb.fixture_log().is_empty(), "no call may be made");
}

#[test]
fn dry_run_rejects_a_policy_that_breaks_the_format_without_echoing_it() {
    let Some(sb) = prepared("dry_run_rejects_a_policy_that_breaks_the_format_without_echoing_it")
    else {
        return;
    };
    let path = sb.root.join("format-policy.json");
    let cases = [
        (
            "unknown field",
            json!({"version": 1, "reads": [], "writes": [POLICY_SECRET]}),
        ),
        (
            "missing version",
            json!({"reads": [format!("fixture/{POLICY_SECRET}")]}),
        ),
        (
            "other version",
            json!({"version": 2, "reads": [format!("fixture/{POLICY_SECRET}")]}),
        ),
        (
            "entry without a slash",
            json!({"version": 1, "reads": [POLICY_SECRET]}),
        ),
        (
            "invalid server name",
            json!({"version": 1, "reads": [format!("Bad Server!/{POLICY_SECRET}")]}),
        ),
        (
            "invalid tool name",
            json!({"version": 1, "reads": [format!("fixture/{POLICY_SECRET} bad!")]}),
        ),
    ];

    for (what, body) in cases {
        std::fs::write(&path, body.to_string()).unwrap();

        let out = dry_run(
            &sb,
            "await attempt('list', () => listItems({}));",
            &["--policy", &path.display().to_string()],
        );

        assert_eq!(out.code, Some(2), "{what}: stdout: {}", out.stdout);
        assert!(
            !out.stdout.contains(POLICY_SECRET) && !out.stderr.contains(POLICY_SECRET),
            "{what}: the policy text was echoed"
        );
        assert!(sb.fixture_log().is_empty(), "{what}: no call may be made");
    }
}

#[test]
fn dry_run_rejects_a_missing_explicit_policy_before_any_call() {
    let Some(sb) = prepared("dry_run_rejects_a_missing_explicit_policy_before_any_call") else {
        return;
    };
    let missing = sb.root.join("no-such-policy.json");

    let out = dry_run(
        &sb,
        "await attempt('list', () => listItems({}));",
        &["--policy", &missing.display().to_string()],
    );

    assert_eq!(out.code, Some(2), "stdout: {}", out.stdout);
    assert!(sb.fixture_log().is_empty(), "no call may be made");
}

#[test]
fn dry_run_report_lists_at_most_fifty_skipped_writes_and_counts_the_rest() {
    let Some(sb) =
        prepared("dry_run_report_lists_at_most_fifty_skipped_writes_and_counts_the_rest")
    else {
        return;
    };

    let out = dry_run(
        &sb,
        "for (let n = 0; n < 60; n++) {\n\
           try { await createItem({ n }); } catch {}\n\
         }",
        &[],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    let listed = out
        .stdout
        .lines()
        .filter(|l| l.trim_start().starts_with('#') && l.contains("fixture/create_item"))
        .count();
    assert_eq!(listed, 50, "{}", out.stdout);
    assert!(out.stdout.contains("and 10 more"), "{}", out.stdout);
    assert!(
        out.stdout.contains("60 write call(s) skipped"),
        "{}",
        out.stdout
    );
}
