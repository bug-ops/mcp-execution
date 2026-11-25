//! Acceptance test of the `tsc` output bound (#622, item 1): a compiler that prints far more than
//! the capture cap does not grow runner memory without bound, and the run is rejected with a line
//! saying the diagnostics were cut.
//!
//! A shim named `tsc` first on `PATH` stands in for the compiler, so the test needs no TypeScript
//! installation. Unix only (shell shim, `mcp.json` redirection).

#![cfg(all(feature = "test-fixtures", unix))]

mod common;

use std::os::unix::fs::PermissionsExt as _;

use common::{Sandbox, require_node};

/// Per-stream capture cap of the type check.
const TSC_OUTPUT_CAP: usize = 8 * 1024 * 1024;

#[test]
fn typecheck_rejects_a_run_whose_tsc_output_exceeds_the_cap_and_says_so() {
    if !require_node("typecheck_rejects_a_run_whose_tsc_output_exceeds_the_cap_and_says_so") {
        return;
    }
    let sb = Sandbox::new();
    let shim_dir = sb.root.join("shim");
    std::fs::create_dir_all(&shim_dir).unwrap();
    let shim = shim_dir.join("tsc");
    std::fs::write(
        &shim,
        "#!/bin/sh\nyes 'error TS9999: padding padding padding' | head -c 20971520\nexit 1\n",
    )
    .unwrap();
    std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::join_paths(std::iter::once(shim_dir).chain(std::env::split_paths(
        &std::env::var_os("PATH").unwrap_or_default(),
    )))
    .unwrap();
    let script = sb.script("noisy.ts", "console.log('SHOULD-NOT-RUN');\nexport {};\n");
    let script_arg = script.display().to_string();
    let servers = sb.servers.display().to_string();

    let out = common::run_with_watchdog(
        sb.command().env("PATH", path).args([
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
        Some(2),
        "stdout: {:.1000} stderr: {:.1000}",
        out.stdout,
        out.stderr
    );
    let shown = format!("{}{}", out.stdout, out.stderr);
    assert!(
        shown.contains(&format!(
            "tsc output exceeded {TSC_OUTPUT_CAP} bytes; remaining diagnostics were not read"
        )),
        "the cut must be reported: {shown:.2000}"
    );
    assert!(!shown.contains("SHOULD-NOT-RUN"));
}
