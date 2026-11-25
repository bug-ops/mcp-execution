//! Acceptance test of the `rmcp` log-cap directive (#678): the core policy strings must be
//! valid `tracing-subscriber` directives, since the CLI filter parses them unconditionally.

#[cfg(feature = "test-fixtures")]
mod common;

use mcp_execution_core::cli::RmcpLogCap;
use tracing_subscriber::EnvFilter;

#[test]
fn every_cap_directive_parses_as_a_filter_directive() {
    for cap in [RmcpLogCap::Info, RmcpLogCap::Warn] {
        let directive = cap
            .directive()
            .parse()
            .expect("directive parses as a tracing directive");
        let filter = EnvFilter::new("debug").add_directive(directive);
        assert!(filter.to_string().contains(cap.directive()));
    }
}

/// Whether `stderr` holds an `rmcp` event of level INFO.
#[cfg(feature = "test-fixtures")]
fn has_rmcp_info(stderr: &str) -> bool {
    stderr
        .lines()
        .any(|line| line.contains("INFO") && line.contains("rmcp::"))
}

#[cfg(feature = "test-fixtures")]
fn introspect_stderr(sb: &common::Sandbox, rust_log: &str) -> String {
    let mut cmd = sb.command();
    cmd.env("RUST_LOG", rust_log)
        .args(["introspect", "--from-config", common::SERVER]);
    let out = common::run_with_watchdog(&mut cmd, None, common::WATCHDOG);
    assert_eq!(out.code, Some(0), "{rust_log}: {}", out.stderr);
    out.stderr
}

/// Acceptance test of #702: the cap only lowers `rmcp` verbosity.
#[cfg(feature = "test-fixtures")]
#[test]
fn the_cap_does_not_raise_rmcp_above_a_stricter_rust_log() {
    let sb = common::Sandbox::new();
    let control = introspect_stderr(&sb, "info");
    assert!(
        has_rmcp_info(&control),
        "rmcp INFO is not observable: {control}"
    );

    for rust_log in ["error", "warn", "rmcp=error", "info,rmcp=error"] {
        let stderr = introspect_stderr(&sb, rust_log);

        assert!(!has_rmcp_info(&stderr), "{rust_log}: {stderr}");
    }
}
