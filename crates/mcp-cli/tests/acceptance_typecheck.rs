//! Acceptance tests of `run --typecheck` (#577): a real type error or an unknown `mcp:` server
//! stops the run with exit 2 before node starts and before any call; a missing `@types/node` or
//! a missing `tsc` is skipped with a notice and the run proceeds.
//!
//! Requires the `test-fixtures` feature, Node.js 22.18+ and `tsc` (hard-fails under CI).

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Sandbox, require_node, require_tsc};

fn ready(test: &str) -> bool {
    require_node(test) && require_tsc(test)
}

#[test]
#[cfg_attr(
    windows,
    ignore = "mcp.json cannot be redirected on Windows; tracked in #604"
)]
fn typecheck_rejects_a_type_error_against_the_generated_sdk_before_any_call() {
    if !ready("typecheck_rejects_a_type_error_against_the_generated_sdk_before_any_call") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    let script = sb.script(
        "bad-types.ts",
        "import { echo } from 'mcp:fixture';\n\
         const n: number = await echo({});\n\
         export { n };\n",
    );

    let out = sb.run_script(&script, &["--typecheck"]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    let shown = format!("{}{}", out.stdout, out.stderr);
    assert!(shown.contains("TS2322"), "diagnostics expected: {shown}");
    assert!(!shown.contains("run started"), "node must not have started");
    assert!(sb.fixture_log().is_empty(), "no call may be made");
    assert!(
        shown.lines().count() <= 40,
        "diagnostics must be bounded: {shown}"
    );
}

#[test]
#[cfg_attr(
    windows,
    ignore = "mcp.json cannot be redirected on Windows; tracked in #604"
)]
fn typecheck_rejects_an_unknown_server_import() {
    if !ready("typecheck_rejects_an_unknown_server_import") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    let script = sb.script(
        "typo.ts",
        "import { echo } from 'mcp:githbu';\nexport { echo };\n",
    );

    let out = sb.run_script(&script, &["--typecheck"]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(
        format!("{}{}", out.stdout, out.stderr).contains("mcp:githbu"),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(sb.fixture_log().is_empty(), "no call may be made");
}

#[test]
fn typecheck_skips_with_a_notice_when_node_types_are_missing_and_runs_the_script() {
    if !ready("typecheck_skips_with_a_notice_when_node_types_are_missing_and_runs_the_script") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script(
        "needs-node-types.ts",
        "process.stdout.write('SKIP-OK\\n');\nexport {};\n",
    );

    let out = sb.run_script(&script, &["--typecheck"]);

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains("SKIP-OK"));
    let lower = out.stdout.to_lowercase();
    assert!(
        lower.contains("typecheck") && lower.contains("skipped"),
        "a skip notice is expected: {}",
        out.stdout
    );
}

#[cfg(unix)]
#[test]
fn typecheck_is_skipped_with_a_notice_when_tsc_is_not_on_path() {
    if !require_node("typecheck_is_skipped_with_a_notice_when_tsc_is_not_on_path") {
        return;
    }
    let sb = Sandbox::new();
    let only_node = common::node_only_dir(&sb.root);
    let script = sb.script(
        "bad-but-unchecked.ts",
        "const n: number = 'not a number';\nconsole.log('RAN-UNCHECKED ' + n);\n",
    );
    let servers = sb.servers.display().to_string();
    let script_arg = script.display().to_string();

    let out = common::run_with_watchdog(
        sb.command().env("PATH", &only_node).args([
            "run",
            &script_arg,
            "--servers-dir",
            &servers,
            "--typecheck",
        ]),
        None,
        common::WATCHDOG,
    );

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains("RAN-UNCHECKED"));
    let lower = out.stdout.to_lowercase();
    assert!(
        lower.contains("typecheck") && lower.contains("skipped"),
        "a skip notice is expected: {}",
        out.stdout
    );
}
