//! MCP CLI library.
//!
//! The library surface is [`Cli`] (the parsed command line) and [`run`]; everything else is
//! private, so the binary and tests share one compiled module tree without freezing it as API.
#![warn(missing_docs, missing_debug_implementations)]
// `commands::completions::run` and `commands::server::list_servers` are `async fn` to match
// the signature every other command handler needs (they're all dispatched uniformly from
// `runner::execute_command`), even though these two happen to do no `.await`ing themselves.
#![allow(clippy::unused_async)]
// `commands::generate` tests collect into a `Vec` before an `is_empty()`/`len()` check for
// readability at the assertion site; the iterator itself isn't reused afterward.
#![allow(clippy::needless_collect)]

mod actions;
mod cli;
mod commands;
mod formatters;
mod runner;
#[doc(hidden)]
pub mod runtime;

pub use cli::Cli;

use mcp_execution_core::cli::ExitCode;

/// Runs the command line `cli` and reports the process exit code the binary should end with.
///
/// Installs the process-global logging subscriber first (so the call is the single entry point
/// of one process, not something to repeat), then dispatches the subcommand. A command failure
/// is reported on stderr and mapped to an [`ExitCode`], not returned as `Err`.
///
/// # Errors
///
/// Returns an error when logging cannot be initialized, for instance because a global
/// `tracing` subscriber is already installed (a second call in the same process).
///
/// # Examples
///
/// ```no_run
/// use clap::Parser as _;
/// use mcp_execution_cli::{Cli, run};
///
/// # async fn demo() -> anyhow::Result<()> {
/// let cli = Cli::try_parse_from(["mcp-execution-cli", "completions", "bash"])?;
/// let code = run(cli).await?;
/// assert_eq!(code.as_i32(), 0);
/// # Ok(())
/// # }
/// ```
pub async fn run(cli: Cli) -> anyhow::Result<ExitCode> {
    runner::init_logging(cli.verbose, cli.log_format, cli.command.log_profile())?;
    runner::execute_command(cli.command, cli.format).await
}
