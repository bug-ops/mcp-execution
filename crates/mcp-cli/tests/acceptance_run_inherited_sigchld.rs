//! Acceptance test of an inherited ignored SIGCHLD (#663): with `SIGCHLD = SIG_IGN` the kernel
//! reaps every child at exit, so waiting for the script leader fails unless the runner replaces
//! the disposition. A script that ends normally must still give exit 0.
//!
//! Red before the fix on Linux only: macOS keeps children waitable under `SIG_IGN` for `waitid`.
//!
//! Unix only: signals.

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use common::{Sandbox, WATCHDOG, require_node, run_with_watchdog};

#[test]
fn run_succeeds_when_sigchld_was_ignored_at_startup() {
    if !require_node("run_succeeds_when_sigchld_was_ignored_at_startup") {
        return;
    }
    let sb = Sandbox::new();
    let script = sb.script("ok.mts", "console.log('done');\n");

    let out = run_with_watchdog(
        &mut sb.runner_ignoring("CHLD", &script, &[]),
        None,
        WATCHDOG,
    );

    assert_eq!(out.code, Some(0), "stderr: {}", out.stderr);
    assert!(out.stdout.contains("done"), "stdout: {}", out.stdout);
}
