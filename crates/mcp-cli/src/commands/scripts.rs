//! `scripts list` and `scripts check`: inspect the saved scripts of a skill.
//!
//! Neither subcommand executes a script or connects to a server: `list` reads headers only, and
//! `check` compares them with the `_meta.json` recorded at generation.

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Args, Subcommand};
use mcp_execution_core::ServerId;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_core::untrusted::sanitize_untrusted_inline;
use mcp_execution_skill::scripts::{self, CheckReport, ScriptListing, ToolRef, WriteIntent};
use serde::Serialize;

use super::common::{ServersDir, SkillsDir, read_generated_metadata_async};
use crate::cli::parse_server_id;

/// Options shared by the `scripts` subcommands.
///
/// # Examples
///
/// ```
/// use clap::Parser;
/// use mcp_execution_cli::cli::{Cli, Commands};
/// use mcp_execution_cli::commands::scripts::ScriptsAction;
///
/// let cli = Cli::parse_from(["mcp-execution-cli", "scripts", "list", "--server", "github"]);
/// let Commands::Scripts { action: ScriptsAction::List(args) } = cli.command else {
///     panic!("expected scripts list")
/// };
/// assert_eq!(args.server.as_str(), "github");
/// ```
#[derive(Debug, Clone, Args)]
pub struct ScriptsArgs {
    /// Server whose skill holds the scripts
    #[arg(short, long, value_parser = parse_server_id)]
    pub server: ServerId,

    /// Base directory of skills (default: ~/.claude/skills)
    #[arg(long)]
    pub skills_dir: Option<PathBuf>,
}

/// Options of `scripts check`: the shared script options plus where the generated server lives.
///
/// # Examples
///
/// ```
/// use clap::Parser;
/// use mcp_execution_cli::cli::{Cli, Commands};
/// use mcp_execution_cli::commands::scripts::ScriptsAction;
///
/// let cli = Cli::parse_from([
///     "mcp-execution-cli", "scripts", "check", "--server", "github", "--servers-dir", "/srv",
/// ]);
/// let Commands::Scripts { action: ScriptsAction::Check(args) } = cli.command else {
///     panic!("expected scripts check")
/// };
/// assert_eq!(args.servers_dir.as_deref(), Some(std::path::Path::new("/srv")));
/// ```
#[derive(Debug, Clone, Args)]
pub struct CheckArgs {
    /// Options shared with `scripts list`
    #[command(flatten)]
    pub scripts: ScriptsArgs,

    /// Base directory of generated servers (default: ~/.claude/servers)
    #[arg(long)]
    pub servers_dir: Option<PathBuf>,
}

/// Saved-script actions.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::scripts::ScriptsAction;
///
/// fn is_check(action: &ScriptsAction) -> bool {
///     matches!(action, ScriptsAction::Check(_))
/// }
/// # let _ = is_check;
/// ```
#[derive(Debug, Clone, Subcommand)]
pub enum ScriptsAction {
    /// List saved scripts by signature, without their bodies
    List(ScriptsArgs),
    /// Check saved scripts against the server's current tools (exit 5 on problems)
    Check(CheckArgs),
}

/// Runs a `scripts` subcommand.
///
/// # Errors
///
/// Returns an error when the home directory is unknown, the `scripts/` directory cannot be read
/// or is a symlink, or (for `check`) the server's `_meta.json` is missing or unusable.
///
/// `check` returns [`ExitCode::CHECK_FAILED`] when a script references a missing tool or has an
/// invalid header, and modifies nothing.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::scripts::{ScriptsAction, run};
/// use mcp_execution_core::cli::OutputFormat;
///
/// # async fn example(action: ScriptsAction) -> anyhow::Result<()> {
/// run(action, OutputFormat::Text).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run(action: ScriptsAction, format: OutputFormat) -> Result<ExitCode> {
    match action {
        ScriptsAction::List(args) => list(&args, format).await,
        ScriptsAction::Check(args) => check(&args, format).await,
    }
}

fn skill_dir(args: &ScriptsArgs) -> Result<PathBuf> {
    Ok(SkillsDir::resolve(args.skills_dir.clone())?.skill(&args.server))
}

#[derive(Serialize)]
struct ListedScript {
    name: String,
    description: String,
    writes: &'static str,
    tools: Vec<String>,
}

#[derive(Serialize)]
struct ListOutput {
    scripts: Vec<ListedScript>,
}

fn tool_label(tool: &ToolRef) -> String {
    format!("{}/{}", tool.server, tool.tool)
}

const fn writes_label(writes: WriteIntent) -> &'static str {
    match writes {
        WriteIntent::ReadOnly => "read_only",
        WriteIntent::Writes => "writes",
    }
}

impl From<&ScriptListing> for ListedScript {
    fn from(listing: &ScriptListing) -> Self {
        Self {
            name: listing.name.as_str().to_owned(),
            description: sanitize_untrusted_inline(&listing.description),
            writes: writes_label(listing.writes),
            tools: listing.tools.iter().map(tool_label).collect(),
        }
    }
}

async fn list(args: &ScriptsArgs, format: OutputFormat) -> Result<ExitCode> {
    let listings = scripts::list(&skill_dir(args)?)
        .await
        .context("Failed to list saved scripts")?;
    let output = ListOutput {
        scripts: listings.iter().map(ListedScript::from).collect(),
    };
    if matches!(format, OutputFormat::Json) {
        return crate::formatters::emit(&output, format, ExitCode::SUCCESS);
    }
    if output.scripts.is_empty() {
        println!("No saved scripts for '{}'", args.server);
    }
    for script in &output.scripts {
        println!(
            "{} [{}]: {} ({})",
            script.name,
            script.writes,
            script.description,
            script.tools.join(", ")
        );
    }
    Ok(ExitCode::SUCCESS)
}

#[derive(Serialize)]
struct MissingToolOutput {
    script: String,
    tool: String,
}

#[derive(Serialize)]
struct CheckOutput {
    missing_tools: Vec<MissingToolOutput>,
    invalid_scripts: Vec<String>,
}

impl From<&CheckReport> for CheckOutput {
    fn from(report: &CheckReport) -> Self {
        Self {
            missing_tools: report
                .missing_tools
                .iter()
                .map(|missing| MissingToolOutput {
                    script: missing.script.as_str().to_owned(),
                    tool: tool_label(&missing.tool),
                })
                .collect(),
            invalid_scripts: report
                .invalid_scripts
                .iter()
                .map(|name| sanitize_untrusted_inline(name))
                .collect(),
        }
    }
}

async fn check(check_args: &CheckArgs, format: OutputFormat) -> Result<ExitCode> {
    let args = &check_args.scripts;
    let servers_dir = ServersDir::resolve(check_args.servers_dir.clone())?;
    let metadata =
        read_generated_metadata_async(servers_dir.server(&args.server), &args.server).await?;
    let report = scripts::check(&skill_dir(args)?, &metadata).await;
    let exit = if report.is_clean() {
        ExitCode::SUCCESS
    } else {
        ExitCode::CHECK_FAILED
    };
    let output = CheckOutput::from(&report);
    if matches!(format, OutputFormat::Json) {
        return crate::formatters::emit(&output, format, exit);
    }
    for missing in &output.missing_tools {
        println!("{}: missing tool {}", missing.script, missing.tool);
    }
    for name in &output.invalid_scripts {
        println!("{name}: invalid header");
    }
    if report.is_clean() {
        println!("All saved scripts of '{}' are sound", args.server);
    }
    Ok(exit)
}
