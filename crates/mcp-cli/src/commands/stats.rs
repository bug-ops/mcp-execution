//! `stats`: deterministic size figures of a generated server at each loading level.
//!
//! Reads `_meta.json` (schema v3) and the sizes of the generated files, never their contents,
//! and prints bytes plus an estimated token count per discovery level (ADR-562). It writes
//! nothing and prints no text taken from the generated files.

use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, bail};
use clap::Args;
use mcp_execution_core::ServerId;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_core::metadata::{CATALOG_FILE_NAME, MAX_TOOL_COUNT, ResultTypeOrigin};

use super::common::{ServersDir, read_generated_metadata};
use crate::cli::parse_server_id;

/// Divisor of the token estimator: one token per four bytes (ADR-562, option A).
const BYTES_PER_TOKEN_ESTIMATE: u64 = 4;

/// Options of the `stats` command.
///
/// # Examples
///
/// ```
/// use clap::Parser;
/// use mcp_execution_cli::cli::{Cli, Commands};
///
/// let cli = Cli::parse_from(["mcp-execution-cli", "stats", "github"]);
/// let Commands::Stats(args) = cli.command else { panic!("expected stats") };
/// assert_eq!(args.server.as_str(), "github");
/// ```
#[derive(Debug, Clone, Args)]
pub struct StatsArgs {
    /// Server identifier of a generated server
    #[arg(value_parser = parse_server_id)]
    pub server: ServerId,

    /// Base directory of generated servers (default: ~/.claude/servers)
    #[arg(long)]
    pub servers_dir: Option<PathBuf>,
}

/// A size in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Bytes(u64);

/// An estimated token count, never an exact one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct TokenEstimate(u64);

impl Bytes {
    /// Estimates the tokens of `self` as bytes divided by four, rounded up so a non-empty text
    /// never reads as zero tokens.
    const fn estimate_tokens(self) -> TokenEstimate {
        TokenEstimate(self.0.div_ceil(BYTES_PER_TOKEN_ESTIMATE))
    }
}

/// A figure that is either measured or absent; absence never reads as zero (FR-046.11).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Measure {
    Available(Bytes),
    NotAvailable,
}

impl Measure {
    /// Sum of `sizes`; not available when any is, or when there are none.
    fn total(sizes: &[Self]) -> Self {
        let mut sum = 0_u64;
        for size in sizes {
            match size {
                Self::Available(Bytes(n)) => sum = sum.saturating_add(*n),
                Self::NotAvailable => return Self::NotAvailable,
            }
        }
        if sizes.is_empty() {
            Self::NotAvailable
        } else {
            Self::Available(Bytes(sum))
        }
    }

    /// Mean of `sizes` rounded down; not available when any is, or when there are none.
    fn mean(sizes: &[Self]) -> Self {
        match Self::total(sizes) {
            Self::Available(Bytes(sum)) => {
                // `total` is available only for a non-empty slice, so the length is non-zero.
                let count = u64::try_from(sizes.len()).unwrap_or(u64::MAX);
                Self::Available(Bytes(sum / count))
            }
            Self::NotAvailable => Self::NotAvailable,
        }
    }
}

/// Figures of one generated server at each discovery level.
#[derive(Debug, Clone, PartialEq, Eq)]
struct StatsReport {
    server: ServerId,
    tool_count: usize,
    definitions: Measure,
    tool_files: Measure,
    one_tool_file: Measure,
    catalog: Measure,
    with_output_schema: usize,
}

impl StatsReport {
    /// Renders the report as fixed-width text: the same input gives the same bytes.
    fn render(&self) -> String {
        let mut out = format!(
            "Context cost of '{}' ({} tools)\n\
             Tokens are an estimate (bytes/4); bytes are exact. Each figure is the size of the \
             text a model reads if it reads that level in full; it does not show that a model \
             does, nor a real token count.\n\n",
            self.server, self.tool_count
        );
        let _ = writeln!(
            out,
            "{:<32}{:>14}{:>20}",
            "Level", "Bytes", "Tokens (estimate)"
        );
        for (label, measure) in [
            ("server definitions (recorded)", self.definitions),
            ("all tool files", self.tool_files),
            ("one tool file (mean)", self.one_tool_file),
            ("catalog (catalog.ts)", self.catalog),
        ] {
            let _ = match measure {
                Measure::Available(bytes) => writeln!(
                    out,
                    "{label:<32}{:>14}{:>20}",
                    bytes.0,
                    bytes.estimate_tokens().0
                ),
                Measure::NotAvailable => writeln!(out, "{label:<32}{:>14}", "not available"),
            };
        }
        let share = (self.with_output_schema * 100).checked_div(self.tool_count);
        let _ = match share {
            Some(percent) => writeln!(
                out,
                "\nTools declaring an output schema: {} of {} ({percent}%)",
                self.with_output_schema, self.tool_count
            ),
            None => writeln!(out, "\nTools declaring an output schema: not available"),
        };
        out
    }
}

/// Size of the regular file `path`, not following a symlink; not available when it is missing,
/// is a link or is not a file.
fn file_size(path: &Path) -> Measure {
    match std::fs::symlink_metadata(path) {
        Ok(meta) if meta.is_file() => Measure::Available(Bytes(meta.len())),
        _ => Measure::NotAvailable,
    }
}

/// Measures the generated server `server` under `servers_dir` with blocking I/O; the caller runs
/// it on one blocking task so no file system call occupies an async worker.
fn measure(servers_dir: &ServersDir, server: ServerId) -> Result<StatsReport> {
    let dir = servers_dir.server(&server);
    let metadata = read_generated_metadata(&dir, &server)?;
    if metadata.tools.len() > MAX_TOOL_COUNT {
        bail!(
            "'{server}' lists {} tools, over the {MAX_TOOL_COUNT} limit",
            metadata.tools.len()
        );
    }

    let sizes: Vec<Measure> = metadata
        .tools
        .iter()
        .map(|tool| file_size(&dir.join(format!("{}.ts", tool.typescript_name))))
        .collect();
    let recorded: Vec<Measure> = metadata
        .tools
        .iter()
        .map(|tool| Measure::Available(Bytes(tool.manifest_size_bytes)))
        .collect();
    let with_output_schema = metadata
        .tools
        .iter()
        .filter(|tool| tool.result_type_origin != ResultTypeOrigin::Undeclared)
        .count();

    Ok(StatsReport {
        server,
        tool_count: metadata.tools.len(),
        definitions: Measure::total(&recorded),
        tool_files: Measure::total(&sizes),
        one_tool_file: Measure::mean(&sizes),
        catalog: file_size(&dir.join(CATALOG_FILE_NAME)),
        with_output_schema,
    })
}

/// Runs the `stats` command.
///
/// Prints, for each discovery level (the server's recorded definitions, all tool files, one
/// tool file and the catalog), the size in bytes and the estimated tokens (bytes divided by
/// four, ADR-562), and how many tools declare an output schema. A level whose data is missing
/// prints "not available", never zero. Output is identical for every [`OutputFormat`].
///
/// # Errors
///
/// Returns an error when the server's `_meta.json` is missing, unreadable, over the size or
/// tool-count bound, or not a regular file. A `_meta.json` older than schema v3 is an
/// `InvalidArgument` error (exit 2) that asks for regeneration; no figures are printed.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::stats::{StatsArgs, run};
/// use mcp_execution_core::cli::OutputFormat;
///
/// # async fn example(args: StatsArgs) -> anyhow::Result<()> {
/// run(args, OutputFormat::Text).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run(args: StatsArgs, _format: OutputFormat) -> Result<ExitCode> {
    let servers_dir = ServersDir::resolve(args.servers_dir)?;
    let report = tokio::task::spawn_blocking(move || measure(&servers_dir, args.server))
        .await
        .context("stats task failed")??;
    print!("{}", report.render());
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn report(catalog: Measure) -> StatsReport {
        StatsReport {
            server: ServerId::new("demo").unwrap(),
            tool_count: 3,
            definitions: Measure::Available(Bytes(1000)),
            tool_files: Measure::Available(Bytes(3001)),
            one_tool_file: Measure::Available(Bytes(1000)),
            catalog,
            with_output_schema: 1,
        }
    }

    #[test]
    fn estimate_rounds_up_and_never_zeroes_a_non_empty_text() {
        assert_eq!(Bytes(1).estimate_tokens(), TokenEstimate(1));
        assert_eq!(Bytes(4).estimate_tokens(), TokenEstimate(1));
        assert_eq!(Bytes(5).estimate_tokens(), TokenEstimate(2));
    }

    #[test]
    fn total_and_mean_are_not_available_when_any_part_or_all_is_missing() {
        let parts = [Measure::Available(Bytes(10)), Measure::NotAvailable];
        assert_eq!(Measure::total(&parts), Measure::NotAvailable);
        assert_eq!(Measure::mean(&parts), Measure::NotAvailable);
        assert_eq!(Measure::total(&[]), Measure::NotAvailable);
        let ok = [Measure::Available(Bytes(10)), Measure::Available(Bytes(15))];
        assert_eq!(Measure::total(&ok), Measure::Available(Bytes(25)));
        assert_eq!(Measure::mean(&ok), Measure::Available(Bytes(12)));
    }

    #[test]
    fn render_prints_not_available_and_share_without_the_word_saving() {
        let text = report(Measure::NotAvailable).render();
        assert!(text.contains("not available"));
        assert!(text.contains("1 of 3 (33%)"));
        assert!(text.contains("bytes/4"));
        assert!(!text.to_lowercase().contains("saving"));
        assert_eq!(text, report(Measure::NotAvailable).render());
    }
}
