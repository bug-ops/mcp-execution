//! Acceptance tests of `setup`'s `failed_entries` (#610): an entry the walk could not make
//! executable is counted in the result instead of living only in the log, and the exit code
//! stays 0.
//!
//! The failure is staged with a servers subdirectory that has no search bit, so listing works
//! but `chmod` of its `.ts` file fails. Unix only; skipped when the process is allowed to
//! ignore permissions (root).

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use std::os::unix::fs::PermissionsExt as _;
use std::path::PathBuf;

use common::{Sandbox, require_node};
use serde_json::Value;

/// Restores the directory mode so the temporary directory can be removed.
struct Unlock(PathBuf);

impl Drop for Unlock {
    fn drop(&mut self) {
        let _ = std::fs::set_permissions(&self.0, std::fs::Permissions::from_mode(0o755));
    }
}

fn setup_json(sb: &Sandbox) -> (Option<i32>, Value) {
    let out = sb.cli(&["--format", "json", "setup"]);
    let json = serde_json::from_str(&out.stdout)
        .unwrap_or_else(|e| panic!("setup output is not JSON ({e}): {}", out.stdout));
    (out.code, json)
}

#[test]
fn setup_counts_an_entry_it_could_not_make_executable_in_failed_entries() {
    if !require_node("setup_counts_an_entry_it_could_not_make_executable_in_failed_entries") {
        return;
    }
    let sb = Sandbox::new();
    let locked = sb.servers.join("x");
    std::fs::create_dir_all(&locked).unwrap();
    std::fs::write(locked.join("tool.ts"), "export {};\n").unwrap();
    std::fs::set_permissions(&locked, std::fs::Permissions::from_mode(0o600)).unwrap();
    let _unlock = Unlock(locked.clone());
    if std::fs::metadata(locked.join("tool.ts")).is_ok() {
        eprintln!("skipped: permissions are not enforced for this user");
        return;
    }

    let (code, result) = setup_json(&sb);

    assert_eq!(
        code,
        Some(0),
        "a failed entry must not change the exit code"
    );
    assert_eq!(result["failed_entries"], 1, "{result}");
    assert_eq!(result["files_made_executable"], 0, "{result}");
    assert_eq!(result["skipped_entries"], 0, "symlinks only: {result}");
}

#[test]
fn setup_reports_zero_failed_entries_when_every_entry_was_processed() {
    if !require_node("setup_reports_zero_failed_entries_when_every_entry_was_processed") {
        return;
    }
    let sb = Sandbox::new();
    std::fs::create_dir_all(sb.servers.join("x")).unwrap();
    std::fs::write(sb.servers.join("x").join("tool.ts"), "export {};\n").unwrap();

    let (code, result) = setup_json(&sb);

    assert_eq!(code, Some(0));
    assert_eq!(result["failed_entries"], 0, "{result}");
    assert_eq!(result["files_made_executable"], 1, "{result}");
}
