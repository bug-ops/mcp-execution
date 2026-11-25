//! `run --typecheck` accepts what node runs: a `.ts` script with top-level `await`, and a
//! correct script against a generated SDK whose own dependencies were never installed.
//!
//! Requires the `test-fixtures` feature, Node.js 22.18+ and `tsc`. Unix only: the relative
//! `PATH` entry test, which plants an executable `tsc` shim as a shell script.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Sandbox, require_node, require_tsc};

fn ready(test: &str) -> bool {
    require_node(test) && require_tsc(test)
}

#[test]
fn a_ts_script_with_top_level_await_passes_the_type_check() {
    if !ready("a_ts_script_with_top_level_await_passes_the_type_check") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script(
        "tla.ts",
        "const value: number = await Promise.resolve(41);\nconsole.log('TLA=' + (value + 1));\n",
    );

    let out = sb.run_script(&script, &["--typecheck"]);

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains("TLA=42"), "{}", out.stdout);
}

#[test]
fn a_correct_script_is_skipped_not_rejected_when_the_sdk_dependencies_are_missing() {
    if !ready("a_correct_script_is_skipped_not_rejected_when_the_sdk_dependencies_are_missing") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    let script = sb.script(
        "ok.ts",
        "import { echo } from 'mcp:fixture';\n\
         console.log('RESULT=' + JSON.stringify(await echo({ a: 1 })));\n",
    );

    let out = sb.run_script(&script, &["--typecheck"]);

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains(r#"RESULT={"a":1}"#), "{}", out.stdout);
    let lower = out.stdout.to_lowercase();
    assert!(
        lower.contains("typecheck") && lower.contains("skipped"),
        "a skip notice is expected: {}",
        out.stdout
    );
}

#[test]
fn a_real_type_error_exits_2_even_while_the_environment_is_unresolved() {
    if !ready("a_real_type_error_exits_2_even_while_the_environment_is_unresolved") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    let script = sb.script(
        "bad.ts",
        "import { echo } from 'mcp:fixture';\n\
         const n: number = 'not a number';\n\
         console.log(n, await echo({}));\n",
    );

    let out = sb.run_script(&script, &["--typecheck"]);

    assert_eq!(
        out.code,
        Some(2),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(out.stderr.contains("TS2322"), "{}", out.stderr);
    assert!(sb.fixture_log().is_empty(), "no call may be made");
}

#[cfg(unix)]
#[test]
fn a_relative_path_entry_is_never_searched_for_tsc() {
    use std::os::unix::fs::PermissionsExt as _;

    if !require_node("a_relative_path_entry_is_never_searched_for_tsc") {
        return;
    }
    let sb = Sandbox::new();
    let only_node = common::node_only_dir(&sb.root);
    let workdir = sb.root.join("work");
    let planted = workdir.join("bin");
    std::fs::create_dir_all(&planted).unwrap();
    let marker = sb.root.join("planted-tsc-ran");
    let fake = planted.join("tsc");
    std::fs::write(&fake, format!("#!/bin/sh\ntouch {}\n", marker.display())).unwrap();
    std::fs::set_permissions(&fake, std::fs::Permissions::from_mode(0o755)).unwrap();
    let script = sb.script("relative.ts", "console.log('RAN');\n");
    let script_arg = script.display().to_string();
    let servers = sb.servers.display().to_string();
    let path = std::env::join_paths(["bin".into(), only_node]).unwrap();

    let out = common::run_with_watchdog(
        sb.command().current_dir(&workdir).env("PATH", path).args([
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
    assert!(
        !marker.exists(),
        "a tsc found through a relative PATH entry must not run"
    );
    assert!(
        out.stdout.to_lowercase().contains("skipped"),
        "{}",
        out.stdout
    );
}
