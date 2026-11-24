//! Acceptance tests of the shared servers and skills directory options (#618, #619):
//! `scripts check --servers-dir` reads a non-default servers directory, and a leading `~/` in
//! `--servers-dir` and `--skills-dir` is expanded against the home directory by every command.
//!
//! The sandbox home holds the generated package, so a literal (unexpanded) `~` path points at
//! nothing and the command fails.
//!
//! Unix only: the tests need `mcp.json`, which cannot be redirected on Windows (#604).

#![cfg(all(feature = "test-fixtures", unix))]
mod common;

use common::{Sandbox, require_node, script_text};

const GOOD_SCRIPT: &str = "good";

fn with_saved_script(sb: &Sandbox) {
    sb.saved_script(
        "fixture",
        GOOD_SCRIPT,
        &script_text(
            GOOD_SCRIPT,
            "uses a real tool",
            &["fixture/list_items"],
            "export {};",
        ),
    );
}

/// Runs `args` with the sandbox root as the working directory, so a literal `~` finds nothing.
fn cli_in_root(sb: &Sandbox, args: &[&str]) -> common::Out {
    common::run_with_watchdog(
        sb.command().current_dir(&sb.root).args(args),
        None,
        common::WATCHDOG,
    )
}

#[test]
fn scripts_check_reads_a_server_generated_into_a_custom_servers_directory() {
    let sb = Sandbox::new();
    let custom = sb.root.join("custom-servers");
    let custom_arg = custom.display().to_string();
    let generated = sb.cli(&[
        "generate",
        "--from-config",
        "fixture",
        "--progressive-output",
        &custom_arg,
    ]);
    assert_eq!(generated.code, Some(0), "generate: {}", generated.stderr);
    assert!(
        !sb.servers.join("fixture").exists(),
        "the default servers directory must stay empty"
    );
    with_saved_script(&sb);
    let skills = sb.skills.display().to_string();

    let out = sb.cli(&[
        "scripts",
        "check",
        "--server",
        "fixture",
        "--skills-dir",
        &skills,
        "--servers-dir",
        &custom_arg,
    ]);

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
}

#[test]
fn stats_expands_a_leading_tilde_in_servers_dir() {
    let sb = Sandbox::new();
    sb.generate();

    let out = cli_in_root(
        &sb,
        &["stats", "fixture", "--servers-dir", "~/.claude/servers"],
    );

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
}

#[test]
fn scripts_list_expands_a_leading_tilde_in_skills_dir() {
    let sb = Sandbox::new();
    sb.generate();
    with_saved_script(&sb);

    let out = cli_in_root(
        &sb,
        &[
            "scripts",
            "list",
            "--server",
            "fixture",
            "--skills-dir",
            "~/.claude/skills",
        ],
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains(GOOD_SCRIPT), "{}", out.stdout);
}

#[test]
fn scripts_check_expands_a_leading_tilde_in_both_directory_options() {
    let sb = Sandbox::new();
    sb.generate();
    with_saved_script(&sb);

    let out = cli_in_root(
        &sb,
        &[
            "scripts",
            "check",
            "--server",
            "fixture",
            "--skills-dir",
            "~/.claude/skills",
            "--servers-dir",
            "~/.claude/servers",
        ],
    );

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
}

#[test]
fn run_expands_a_leading_tilde_in_servers_dir() {
    if !require_node("run_expands_a_leading_tilde_in_servers_dir") {
        return;
    }
    let sb = Sandbox::new();
    sb.generate();
    let script = sb.script(
        "echo.ts",
        "import { echo } from 'mcp:fixture';\n\
         console.log('RESULT=' + JSON.stringify(await echo({ a: 1 })));\n",
    );
    let script_arg = script.display().to_string();

    let out = cli_in_root(
        &sb,
        &["run", &script_arg, "--servers-dir", "~/.claude/servers"],
    );

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
    assert!(out.stdout.contains("RESULT="), "{}", out.stdout);
}
