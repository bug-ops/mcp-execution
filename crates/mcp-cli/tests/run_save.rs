//! `run --save` validates everything it can before node starts: an invalid header, a name
//! mismatch or an existing recipe exit 2 without a single tool call, and a failed run saves nothing.

#![cfg(feature = "test-fixtures")]

mod common;

use common::{Sandbox, files_below, require_node, script_text};

fn prepared() -> Sandbox {
    let sb = Sandbox::new();
    sb.generate();
    sb.clear_fixture_log();
    sb
}

const CALLING_BODY: &str = "import { echo } from 'mcp:fixture';\nawait echo({});";

#[test]
fn save_with_an_invalid_header_exits_2_before_any_call() {
    if !require_node("save_with_an_invalid_header_exits_2_before_any_call") {
        return;
    }
    let sb = prepared();
    let script = sb.script("headerless.ts", CALLING_BODY);

    let out = sb.run_script(&script, &["--save", "recipe"]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(!out.stdout.contains("run started"), "node must not start");
    assert!(sb.fixture_log().is_empty(), "no call may be made");
    assert_eq!(files_below(&sb.skills).len(), 0, "nothing may be saved");
}

#[test]
fn save_over_an_existing_recipe_without_overwrite_exits_2_before_any_call() {
    if !require_node("save_over_an_existing_recipe_without_overwrite_exits_2_before_any_call") {
        return;
    }
    let sb = prepared();
    let text = script_text("recipe", "d", &["fixture/echo"], CALLING_BODY);
    sb.saved_script("fixture", "recipe", &text);
    let script = sb.script("again.ts", &text);

    let out = sb.run_script(&script, &["--save", "recipe"]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(sb.fixture_log().is_empty(), "no call may be made");
}

#[test]
fn a_failed_run_saves_nothing_and_says_so() {
    if !require_node("a_failed_run_saves_nothing_and_says_so") {
        return;
    }
    let sb = prepared();
    let text = script_text("failing", "d", &["fixture/echo"], "process.exit(1);");
    let script = sb.script("failing.ts", &text);

    let out = sb.run_script(&script, &["--save", "failing"]);

    assert_eq!(out.code, Some(6));
    assert!(out.stdout.contains("not saved: script did not succeed"));
    assert_eq!(files_below(&sb.skills).len(), 0, "nothing may be saved");
}

#[test]
fn an_oversized_script_with_save_exits_2_before_any_call() {
    if !require_node("an_oversized_script_with_save_exits_2_before_any_call") {
        return;
    }
    let sb = prepared();
    let padding = format!("// {}\n", "x".repeat(100 * 1024));
    let text = script_text(
        "big",
        "d",
        &["fixture/echo"],
        &format!("{CALLING_BODY}\n{padding}"),
    );
    let script = sb.script("big.ts", &text);

    let out = sb.run_script(&script, &["--save", "big"]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(!out.stdout.contains("run started"), "node must not start");
    assert!(sb.fixture_log().is_empty(), "no call may be made");
    assert!(
        !out.stderr.contains("xxxxxxxx"),
        "the script must not be echoed"
    );
}

#[test]
fn a_full_skill_with_save_exits_2_before_any_call() {
    if !require_node("a_full_skill_with_save_exits_2_before_any_call") {
        return;
    }
    let sb = prepared();
    for index in 0..50 {
        let name = format!("recipe-{index}");
        let text = script_text(&name, "d", &["fixture/echo"], "export {};");
        sb.saved_script("fixture", &name, &text);
    }
    let text = script_text("one-too-many", "d", &["fixture/echo"], CALLING_BODY);
    let script = sb.script("extra.ts", &text);

    let out = sb.run_script(&script, &["--save", "one-too-many"]);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
    assert!(sb.fixture_log().is_empty(), "no call may be made");
}
