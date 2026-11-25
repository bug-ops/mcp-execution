//! Acceptance tests of the saved-script commands (#579): `scripts list`, `scripts check`,
//! `run --save`, and the guarantee that `generate` and `skill` leave `scripts/` alone.
//!
//! ASSUMPTIONS: `run --save` writes under `<skills-dir>/<skill>/scripts/<name>.ts` where the
//! skill is not pinned by the plan, so the tests look for the file anywhere below the skills
//! directory; the script text carries its own ADR-579 header.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Sandbox, files_below, require_node, script_text, snapshot};
use std::assert_matches;

const BODY_MARKER: &str = "BODY-SECRET-MARKER-91ab";

fn prepared() -> Sandbox {
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    sb
}

fn skill_args<'a>(action: &'a str, skills: &'a str) -> Vec<&'a str> {
    vec![
        "scripts",
        action,
        "--server",
        "fixture",
        "--skills-dir",
        skills,
    ]
}

#[test]
fn scripts_check_exits_5_on_a_missing_tool_and_modifies_nothing() {
    let sb = prepared();
    sb.saved_script(
        "fixture",
        "good",
        &script_text(
            "good",
            "uses a real tool",
            &["fixture/list_items"],
            "export {};",
        ),
    );
    sb.saved_script(
        "fixture",
        "stale",
        &script_text(
            "stale",
            "uses a removed tool",
            &["fixture/removed_tool"],
            "export {};",
        ),
    );
    let before = snapshot(&sb.skills);
    let skills = sb.skills.display().to_string();

    let out = sb.cli(&skill_args("check", &skills));

    assert_eq!(out.code, Some(5), "stderr: {}", out.stderr);
    assert!(
        out.stdout.contains("stale") && out.stdout.contains("removed_tool"),
        "{}",
        out.stdout
    );
    assert!(
        !out.stdout.contains("good"),
        "a healthy script is not a problem: {}",
        out.stdout
    );
    assert_eq!(
        snapshot(&sb.skills),
        before,
        "check must not modify anything"
    );
    assert!(
        sb.fixture_log().is_empty(),
        "check must not connect to a server"
    );
}

#[test]
fn scripts_check_reports_an_invalid_header() {
    let sb = prepared();
    sb.saved_script("fixture", "headerless", "console.log('no header');\n");
    let skills = sb.skills.display().to_string();

    let out = sb.cli(&skill_args("check", &skills));

    assert_eq!(out.code, Some(5), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("headerless"), "{}", out.stdout);
}

#[test]
fn scripts_check_exits_0_when_every_script_is_sound() {
    let sb = prepared();
    sb.saved_script(
        "fixture",
        "good",
        &script_text(
            "good",
            "uses a real tool",
            &["fixture/list_items"],
            "export {};",
        ),
    );
    let skills = sb.skills.display().to_string();

    let out = sb.cli(&skill_args("check", &skills));

    assert_eq!(
        out.code,
        Some(0),
        "stdout: {} stderr: {}",
        out.stdout,
        out.stderr
    );
}

#[test]
fn scripts_list_prints_signatures_and_never_a_body() {
    let sb = prepared();
    sb.saved_script(
        "fixture",
        "triage",
        &script_text(
            "triage",
            "Lists items for triage",
            &["fixture/list_items", "fixture/echo"],
            &format!("console.log('{BODY_MARKER}');"),
        ),
    );
    let skills = sb.skills.display().to_string();

    let out = sb.cli(&skill_args("list", &skills));

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    for expected in [
        "triage",
        "Lists items for triage",
        "fixture/list_items",
        "fixture/echo",
    ] {
        assert!(
            out.stdout.contains(expected),
            "missing {expected}: {}",
            out.stdout
        );
    }
    assert!(
        !out.stdout.contains(BODY_MARKER),
        "a body leaked: {}",
        out.stdout
    );
}

#[test]
fn generate_and_skill_leave_saved_scripts_byte_identical() {
    let sb = prepared();
    sb.saved_script(
        "fixture",
        "keep",
        &script_text(
            "keep",
            "must survive",
            &["fixture/echo"],
            &format!("// {BODY_MARKER}"),
        ),
    );
    let before = snapshot(&sb.scripts_dir("fixture"));

    let generated = sb.cli(&["generate", "--from-config", "fixture"]);
    let skill = sb.cli(&["skill", "--server", "fixture", "--overwrite"]);

    assert_eq!(generated.code, Some(0), "stderr: {}", generated.stderr);
    assert_eq!(skill.code, Some(0), "stderr: {}", skill.stderr);
    assert_eq!(snapshot(&sb.scripts_dir("fixture")), before);
    let skills = sb.skills.display().to_string();
    let listed = sb.cli(&skill_args("list", &skills));
    assert_eq!(listed.code, Some(0), "stderr: {}", listed.stderr);
    assert!(
        listed.stdout.contains("keep"),
        "the script must still be listed: {}",
        listed.stdout
    );
}

fn saved(sb: &Sandbox, name: &str) -> Vec<std::path::PathBuf> {
    files_below(&sb.skills)
        .into_iter()
        .filter(|p| {
            p.file_name()
                .is_some_and(|f| f == format!("{name}.ts").as_str())
        })
        .collect()
}

#[test]
fn run_save_stores_the_executed_text_only_after_the_script_exits_zero() {
    if !require_node("run_save_stores_the_executed_text_only_after_the_script_exits_zero") {
        return;
    }
    let sb = prepared();
    let ok_text = script_text(
        "my-recipe",
        "saved by run",
        &["fixture/echo"],
        "console.log('ran');",
    );
    let bad_text = script_text(
        "bad-recipe",
        "must not be saved",
        &["fixture/echo"],
        "process.exit(1);",
    );
    let ok = sb.script("ok.ts", &ok_text);
    let bad = sb.script("bad.ts", &bad_text);

    let saved_run = sb.run_script(&ok, &["--save", "my-recipe"]);
    let failed_run = sb.run_script(&bad, &["--save", "bad-recipe"]);

    assert_eq!(saved_run.code, Some(0), "stderr: {}", saved_run.stderr);
    let files = saved(&sb, "my-recipe");
    assert_eq!(files.len(), 1, "{files:?}");
    assert!(files[0].to_string_lossy().contains("scripts"));
    assert_eq!(std::fs::read_to_string(&files[0]).unwrap(), ok_text);
    assert_eq!(failed_run.code, Some(6));
    assert!(
        saved(&sb, "bad-recipe").is_empty(),
        "a failed run must not be saved"
    );
}

#[test]
fn run_save_refuses_an_existing_name_unless_overwrite_is_given() {
    if !require_node("run_save_refuses_an_existing_name_unless_overwrite_is_given") {
        return;
    }
    let sb = prepared();
    let first = script_text("again", "first", &["fixture/echo"], "console.log('v1');");
    let second = script_text("again", "second", &["fixture/echo"], "console.log('v2');");
    let first_path = sb.script("first.ts", &first);
    let second_path = sb.script("second.ts", &second);
    assert_eq!(
        sb.run_script(&first_path, &["--save", "again"]).code,
        Some(0)
    );

    let refused = sb.run_script(&second_path, &["--save", "again"]);

    assert_matches!(refused.code, Some(c) if c != 0 && c != 101,
        "expected a classified refusal, got {:?}",
        refused.code);
    assert_eq!(
        std::fs::read_to_string(&saved(&sb, "again")[0]).unwrap(),
        first
    );
    let replaced = sb.run_script(&second_path, &["--save", "again", "--overwrite"]);
    assert_eq!(replaced.code, Some(0), "stderr: {}", replaced.stderr);
    assert_eq!(
        std::fs::read_to_string(&saved(&sb, "again")[0]).unwrap(),
        second
    );
}

#[test]
fn run_save_writes_nothing_when_the_header_name_differs_from_the_requested_name() {
    if !require_node("run_save_writes_nothing_when_the_header_name_differs_from_the_requested_name")
    {
        return;
    }
    let sb = prepared();
    let text = script_text(
        "header-name",
        "mismatch",
        &["fixture/echo"],
        "console.log('x');",
    );
    let path = sb.script("mismatch.ts", &text);

    let out = sb.run_script(&path, &["--save", "requested-name"]);

    assert_matches!(out.code, Some(c) if c != 0 && c != 101,
        "expected a classified refusal, got {:?}",
        out.code);
    assert!(saved(&sb, "requested-name").is_empty() && saved(&sb, "header-name").is_empty());
}

#[test]
fn skill_rejects_a_custom_name_that_differs_from_the_target_directory_before_writing() {
    let sb = prepared();

    let out = sb.cli(&[
        "skill",
        "--server",
        "fixture",
        "--skill-name",
        "some-other-name",
    ]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(
        files_below(&sb.skills).is_empty(),
        "nothing may be written: {:?}",
        files_below(&sb.skills)
    );
}
