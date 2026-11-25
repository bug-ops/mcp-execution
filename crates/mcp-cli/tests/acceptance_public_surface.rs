//! Acceptance tests of the narrowed library surface (#675): `Cli` at the crate root and
//! `run(Cli)`, which initializes logging once and reports a second initialization as an error
//! instead of panicking.
#![expect(
    clippy::expect_used,
    reason = "test, bench and example code may panic on failure"
)]

use clap::Parser as _;
use mcp_execution_cli::{Cli, run};

fn completions_cli() -> Cli {
    Cli::try_parse_from(["mcp-execution-cli", "completions", "bash"])
        .expect("`completions bash` parses")
}

// One process initializes the global logger once, so both calls share a single test.
#[tokio::test]
async fn run_succeeds_once_then_reports_the_second_logging_init_as_an_error() {
    let code = run(completions_cli())
        .await
        .expect("the first run initializes logging and completes");
    assert_eq!(code.as_i32(), 0);

    assert!(run(completions_cli()).await.is_err());
}
