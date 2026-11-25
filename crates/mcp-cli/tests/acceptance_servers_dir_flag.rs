//! Acceptance tests of the `--servers-dir` rename (#680): `generate` and `drift` take
//! `--servers-dir`, and the old `--progressive-output` spelling is gone (pre-1.0, no alias).

use clap::Parser as _;
use mcp_execution_cli::Cli;

#[test]
fn generate_accepts_servers_dir_and_rejects_progressive_output() {
    assert!(
        Cli::try_parse_from([
            "mcp-execution-cli",
            "generate",
            "server",
            "--servers-dir",
            "/tmp/x"
        ])
        .is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "mcp-execution-cli",
            "generate",
            "server",
            "--progressive-output",
            "/tmp/x",
        ])
        .is_err()
    );
}

#[test]
fn drift_accepts_servers_dir_and_rejects_progressive_output() {
    assert!(
        Cli::try_parse_from([
            "mcp-execution-cli",
            "drift",
            "--from-config",
            "github",
            "--servers-dir",
            "/tmp/x",
        ])
        .is_ok()
    );
    assert!(
        Cli::try_parse_from([
            "mcp-execution-cli",
            "drift",
            "--from-config",
            "github",
            "--progressive-output",
            "/tmp/x",
        ])
        .is_err()
    );
}
