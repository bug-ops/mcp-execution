//! Acceptance tests of the single `_meta.json` reader as seen through the CLI (#618): a schema
//! mismatch is invalid input (exit 2) in `drift` and `scripts check`, and a symlinked sidecar is
//! refused by both instead of being read.
//!
//! The fixture's tool list is fixed, so every failure is staged by rewriting the sidecar.
//!
//! Unix only: the symlinked-sidecar tests, which create the link with a Unix API.

#![cfg(feature = "test-fixtures")]
#![cfg_attr(
    unix,
    expect(
        clippy::unwrap_used,
        reason = "test, bench and example code may panic on failure"
    )
)]

mod common;

use common::{Out, Sandbox, script_text};

fn prepared() -> Sandbox {
    let sb = Sandbox::new();
    sb.generate();
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
    sb
}

fn drift(sb: &Sandbox) -> Out {
    sb.cli(&["drift", "--from-config", "fixture"])
}

fn scripts_check(sb: &Sandbox) -> Out {
    let skills = sb.skills.display().to_string();
    sb.cli(&[
        "scripts",
        "check",
        "--server",
        "fixture",
        "--skills-dir",
        &skills,
    ])
}

#[cfg(unix)]
fn meta_path(sb: &Sandbox) -> std::path::PathBuf {
    sb.servers.join("fixture").join("_meta.json")
}

/// Moves the real sidecar next to itself and leaves a symlink to it in its place.
#[cfg(unix)]
fn replace_with_symlink(sb: &Sandbox) {
    let meta = meta_path(sb);
    let real = meta.with_file_name("_meta.real.json");
    std::fs::rename(&meta, &real).unwrap();
    std::os::unix::fs::symlink(&real, &meta).unwrap();
}

#[cfg(unix)]
fn assert_refused(out: &Out, what: &str) {
    assert!(
        out.code.is_some_and(|code| code != 0 && code != 5),
        "{what} must be refused with an error, not read; code {:?}, stdout {:.500}",
        out.code,
        out.stdout
    );
}

#[test]
fn drift_exits_2_on_a_sidecar_schema_mismatch() {
    let sb = prepared();
    sb.edit_meta("fixture", |meta| meta["schema_version"] = 1.into());

    let out = drift(&sb);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
}

#[test]
fn scripts_check_exits_2_on_a_sidecar_schema_mismatch() {
    let sb = prepared();
    sb.edit_meta("fixture", |meta| meta["schema_version"] = 99.into());

    let out = scripts_check(&sb);

    assert_eq!(out.code, Some(2), "stderr: {}", out.stderr);
}

#[cfg(unix)]
#[test]
fn drift_refuses_a_symlinked_sidecar() {
    let sb = prepared();
    replace_with_symlink(&sb);

    assert_refused(&drift(&sb), "a symlinked _meta.json");
}

#[cfg(unix)]
#[test]
fn scripts_check_refuses_a_symlinked_sidecar() {
    let sb = prepared();
    replace_with_symlink(&sb);

    assert_refused(&scripts_check(&sb), "a symlinked _meta.json");
}
