//! `run`: execute a TypeScript script against generated servers with bounded output.
//!
//! The runner validates the script, starts node with a preload that makes `mcp:<id>` and
//! `mcp:helpers` importable, captures its output within budgets (spilling the rest to private
//! files) and reports through a tokenized anchor line and trailer that the script cannot forge.
//! It serves the script's tool calls over the helper channel (journal and dry run included),
//! optionally type-checks the script (`--typecheck`), narrows its permissions (`--restrict`)
//! and saves it as a recipe after a successful run (`--save`).

use std::convert::Infallible;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Args;
use futures_util::FutureExt as _;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_core::{
    Error as CoreError, ServerId, sanitize_path_for_error, write_confined_file,
};
use mcp_execution_introspector::CallTimeout;
use mcp_execution_skill::scripts::{
    MAX_SCRIPT_BYTES as MAX_SAVED_SCRIPT_BYTES, MAX_SCRIPTS_PER_SKILL, Overwrite, RecipeName,
    ScriptError, parse_header, save,
};
use serde::Serialize;
use tokio::io::AsyncReadExt as _;

use super::common::{ServersDir, SkillsDir, refuse_inside_run};
use crate::formatters::format_output;
use crate::runtime::budget::OutputBudget;
use crate::runtime::dirs::{RunId, RuntimeDirs, ensure_private_dir};
use crate::runtime::exec::{
    Captured, ProcessOutcome, RunTimeout, ScriptProcess, ScriptStatus, run_script_process,
};
use crate::runtime::helper::{
    Carrier, DEFAULT_CALL_TIMEOUT, DryRunMode, HelperConfig, HelperServer, RunCallReport,
};
use crate::runtime::import_hook::import_argument;
use crate::runtime::journal::Journal;
use crate::runtime::node::check_node_version;
use crate::runtime::notice::RunToken;
use crate::runtime::policy::Policy;
use crate::runtime::restrict::permission_args;
use crate::runtime::signal::{RunnerSignal, Termination};
use crate::runtime::spill::Spill;
use crate::runtime::typecheck::{TypecheckOutcome, typecheck};

/// Names the file a `test-fixtures` build creates once it is ready to read a script from stdin.
#[cfg(feature = "test-fixtures")]
pub const TEST_READY_FILE_ENV: &str = "MCP_EXECUTION_TEST_READY_FILE";

#[cfg(feature = "test-fixtures")]
fn announce_readiness() {
    if let Some(path) = std::env::var_os(TEST_READY_FILE_ENV)
        && let Err(error) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
    {
        tracing::debug!("could not create the readiness file: {error}");
    }
}

/// Largest script the runner accepts, in bytes.
pub const MAX_SCRIPT_BYTES: u64 = 1024 * 1024;

/// Where the script text comes from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScriptSource {
    /// Read the script from standard input.
    Stdin,
    /// Read the script from this file.
    File(PathBuf),
}

impl FromStr for ScriptSource {
    type Err = Infallible;

    fn from_str(raw: &str) -> Result<Self, Self::Err> {
        Ok(if raw == "-" {
            Self::Stdin
        } else {
            Self::File(PathBuf::from(raw))
        })
    }
}

/// Options of the `run` command.
///
/// The script runs with the user's privileges: neither the journal nor `--dry-run` is a
/// sandbox. Under `run` the script's own stdin is reserved for the helper channel.
#[derive(Debug, Clone, Args)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent command-line switches"
)]
pub struct RunArgs {
    /// Script file to run, or `-` to read the script from stdin
    pub script: ScriptSource,

    /// Base directory of generated servers (default: ~/.claude/servers)
    #[arg(long)]
    pub servers_dir: Option<PathBuf>,

    /// Run timeout in seconds, 1 to 600
    #[arg(long, default_value_t = RunTimeout::default_timeout())]
    pub timeout: RunTimeout,

    /// Bytes of script stdout printed before the rest is spilled to a file
    #[arg(long, default_value_t = OutputBudget::stdout_default())]
    pub max_output: OutputBudget,

    /// Bytes of script stderr printed before the rest is spilled to a file
    #[arg(long, default_value_t = OutputBudget::stderr_default())]
    pub max_stderr: OutputBudget,

    /// Skip calls the policy does not classify as reads and report them
    #[arg(long)]
    pub dry_run: bool,

    /// Policy file for --dry-run (default: the per-user policy file)
    #[arg(long, requires = "dry_run")]
    pub policy: Option<PathBuf>,

    /// Narrow file, process and (Node 25+) network access of the script (Unix only)
    #[arg(long)]
    pub restrict: bool,

    /// Type-check the script with tsc before running it
    #[arg(long)]
    pub typecheck: bool,

    /// Save the script as a named recipe after it exits 0
    #[arg(long, value_name = "NAME", conflicts_with = "dry_run", value_parser = parse_recipe_name)]
    pub save: Option<RecipeName>,

    /// Replace an existing recipe of the same name when saving
    #[arg(long, requires = "save")]
    pub overwrite: bool,

    /// Arguments passed to the script, after `--`
    #[arg(last = true)]
    pub args: Vec<String>,
}

/// One output stream after the budget was applied.
#[derive(Debug, PartialEq, Eq)]
struct StreamReport {
    head: String,
    total_bytes: u64,
    spill: Option<SpillRef>,
    notice: Option<String>,
}

/// A spill file and whether it holds the whole stream.
#[derive(Debug, PartialEq, Eq)]
struct SpillRef {
    path: PathBuf,
    complete: bool,
}

#[derive(Serialize)]
struct StreamEnvelope<'a> {
    text: &'a str,
    bytes: u64,
    truncated: bool,
    spill: Option<String>,
    spill_complete: Option<bool>,
}

impl<'a> StreamEnvelope<'a> {
    fn of(report: &'a StreamReport) -> Self {
        Self {
            text: &report.head,
            bytes: report.total_bytes,
            truncated: report.notice.is_some(),
            spill: report
                .spill
                .as_ref()
                .map(|spill| spill.path.display().to_string()),
            spill_complete: report.spill.as_ref().map(|spill| spill.complete),
        }
    }
}

#[derive(Serialize)]
struct Envelope<'a> {
    token: &'a str,
    script_status: String,
    exit_code: i32,
    stdout: StreamEnvelope<'a>,
    stderr: StreamEnvelope<'a>,
    notices: &'a [String],
}

/// Everything a finished run prints.
struct RunReport {
    token: RunToken,
    status: ScriptStatus,
    stdout: StreamReport,
    stderr: StreamReport,
    notices: Vec<String>,
}

fn invalid(message: impl Into<String>) -> anyhow::Error {
    anyhow::Error::new(CoreError::InvalidArgument(message.into()))
}

/// Clap value parser: a validated recipe name, so a bad `--save` fails at parse time (exit 2).
fn parse_recipe_name(raw: &str) -> std::result::Result<RecipeName, String> {
    RecipeName::new(raw).map_err(|error| error.to_string())
}

fn script_error(error: &ScriptError) -> anyhow::Error {
    invalid(error.to_string())
}

/// What `--save` will store, validated before the script runs.
struct SavePlan {
    name: RecipeName,
    skill_dir: PathBuf,
    text: String,
    overwrite: Overwrite,
}

async fn plan_save(args: &RunArgs, script: &PreparedScript) -> Result<Option<SavePlan>> {
    let Some(requested) = &args.save else {
        return Ok(None);
    };
    let name = requested.clone();
    let text = script
        .text
        .clone()
        .context("the script text was not loaded for --save")?;
    if text.len() > MAX_SAVED_SCRIPT_BYTES {
        return Err(invalid(format!(
            "the script is too large to save (limit {MAX_SAVED_SCRIPT_BYTES} bytes)"
        )));
    }
    let header = parse_header(&text).map_err(|e| script_error(&e))?;
    if header.name != name {
        return Err(invalid(
            "the script header names a different script than --save",
        ));
    }
    let server: &ServerId = header
        .server()
        .ok_or_else(|| invalid("header lists no tools; cannot choose a skill directory"))?;
    let skill_dir = SkillsDir::resolve(None)?.skill(server);
    // Pre-checks only, so a long run is not wasted; `save` checks again.
    let scripts_dir = skill_dir.join("scripts");
    let exists = scripts_dir.join(format!("{}.ts", name.as_str())).exists();
    if exists && !args.overwrite {
        return Err(invalid(format!(
            "a script named {} already exists (use --overwrite to replace it)",
            name.as_str()
        )));
    }
    if !exists && count_scripts(&scripts_dir).await >= MAX_SCRIPTS_PER_SKILL {
        return Err(invalid(format!(
            "the skill already holds {MAX_SCRIPTS_PER_SKILL} scripts"
        )));
    }
    Ok(Some(SavePlan {
        name,
        skill_dir,
        text,
        overwrite: if args.overwrite {
            Overwrite::Replace
        } else {
            Overwrite::Refuse
        },
    }))
}

/// Number of `.ts` files in `dir`; a missing directory holds none.
async fn count_scripts(dir: &Path) -> usize {
    let Ok(mut entries) = tokio::fs::read_dir(dir).await else {
        return 0;
    };
    let mut count = 0;
    while let Ok(Some(entry)) = entries.next_entry().await {
        if entry.path().extension().is_some_and(|ext| ext == "ts") {
            count += 1;
        }
    }
    count
}

/// A script ready to run, the run directory it may use, and whether that directory exists and
/// must be removed afterwards.
struct PreparedScript {
    path: PathBuf,
    run_dir: PathBuf,
    scratch: bool,
    /// The script text, read only when `--save` needs it.
    text: Option<String>,
}

/// Writes the script bytes to `<run dir>/script.ts` (the run directory is created private).
async fn write_snapshot(run_dir: &Path, bytes: &[u8]) -> Result<PathBuf> {
    ensure_private_dir(run_dir).await?;
    let path = run_dir.join("script.ts");
    write_confined_file(&path, bytes)
        .await
        .context("cannot write the script to the run directory")?;
    Ok(path)
}

fn script_text(bytes: &[u8]) -> Result<String> {
    String::from_utf8(bytes.to_vec()).map_err(|_| invalid("the script is not valid UTF-8"))
}

async fn prepare_script(
    args: &RunArgs,
    dirs: &RuntimeDirs,
    run_id: &RunId,
) -> Result<PreparedScript> {
    let run_dir = dirs.run_dir(run_id);
    let needs_run_dir = args.typecheck || args.restrict;
    let prepared = match &args.script {
        ScriptSource::File(path) => {
            let shown = sanitize_path_for_error(path);
            let meta = tokio::fs::metadata(path)
                .await
                .map_err(|_| invalid(format!("cannot read script file {shown}")))?;
            if !meta.is_file() {
                return Err(invalid(format!("{shown} is not a file")));
            }
            if meta.len() > MAX_SCRIPT_BYTES {
                return Err(invalid(format!("script exceeds {MAX_SCRIPT_BYTES} bytes")));
            }
            let absolute = std::path::absolute(path)
                .map_err(|_| invalid(format!("cannot resolve script path {shown}")))?;
            if args.save.is_some() {
                // Run exactly the bytes that will be saved, not a second read of the file.
                let bytes = tokio::fs::read(path)
                    .await
                    .map_err(|_| invalid(format!("cannot read script file {shown}")))?;
                if bytes.len() as u64 > MAX_SCRIPT_BYTES {
                    return Err(invalid(format!("script exceeds {MAX_SCRIPT_BYTES} bytes")));
                }
                let text = script_text(&bytes)?;
                let snapshot = write_snapshot(&run_dir, &bytes).await?;
                PreparedScript {
                    path: snapshot,
                    run_dir,
                    scratch: true,
                    text: Some(text),
                }
            } else {
                PreparedScript {
                    path: absolute,
                    run_dir,
                    scratch: false,
                    text: None,
                }
            }
        }
        ScriptSource::Stdin => {
            #[cfg(feature = "test-fixtures")]
            announce_readiness();
            let mut text = Vec::new();
            tokio::io::stdin()
                .take(MAX_SCRIPT_BYTES + 1)
                .read_to_end(&mut text)
                .await
                .context("cannot read the script from stdin")?;
            if text.len() as u64 > MAX_SCRIPT_BYTES {
                return Err(invalid(format!("script exceeds {MAX_SCRIPT_BYTES} bytes")));
            }
            let path = write_snapshot(&run_dir, &text).await?;
            let text = args
                .save
                .is_some()
                .then(|| script_text(&text))
                .transpose()?;
            PreparedScript {
                path,
                run_dir,
                scratch: true,
                text,
            }
        }
    };
    if needs_run_dir && !prepared.scratch {
        ensure_private_dir(&prepared.run_dir).await?;
        return Ok(PreparedScript {
            scratch: true,
            ..prepared
        });
    }
    Ok(prepared)
}

fn spill_notice(stream: &str, shown: usize, captured: &Captured, spill: Option<&Path>) -> String {
    let saved = spill.map_or_else(
        || "could not be saved".to_string(),
        |path| format!("saved to {}", path.display()),
    );
    let kept = if captured.is_cut() {
        format!("first {} bytes", captured.bytes.len())
    } else {
        "full output".to_string()
    };
    format!(
        "{stream} cut at {shown} of {} bytes; {kept} {saved}",
        captured.total
    )
}

async fn report_stream(
    stream: &str,
    captured: &Captured,
    budget: OutputBudget,
    spill: &Spill,
) -> StreamReport {
    let text = String::from_utf8_lossy(&captured.bytes);
    let cut = budget.apply(&text);
    if !(cut.is_truncated() || captured.is_cut()) {
        return StreamReport {
            head: text.into_owned(),
            total_bytes: captured.total,
            spill: None,
            notice: None,
        };
    }
    let saved = match spill.write(&captured.bytes).await {
        Ok(file) => Some(SpillRef {
            complete: !captured.is_cut() && !file.truncated,
            path: file.path,
        }),
        Err(error) => {
            tracing::debug!("could not spill {stream}: {error:#}");
            None
        }
    };
    StreamReport {
        notice: Some(spill_notice(
            stream,
            cut.head.len(),
            captured,
            saved.as_ref().map(|spill| spill.path.as_path()),
        )),
        head: cut.head.to_string(),
        total_bytes: captured.total,
        spill: saved,
    }
}

const fn exit_code_of(status: ScriptStatus) -> ExitCode {
    match status {
        ScriptStatus::Exit(0) => ExitCode::SUCCESS,
        ScriptStatus::Exit(_) | ScriptStatus::Signal(_) => ExitCode::SCRIPT_FAILED,
        ScriptStatus::TimedOut => ExitCode::TIMEOUT,
        ScriptStatus::Cancelled(signal) => signal.exit_code(),
    }
}

fn with_line_end(text: &str) -> String {
    if text.is_empty() || text.ends_with('\n') {
        text.to_string()
    } else {
        format!("{text}\n")
    }
}

/// Renders the text-mode stdout and stderr of a finished run.
fn render_text(report: &RunReport) -> (String, String) {
    let token = &report.token;
    let mut stdout = with_line_end(&report.stdout.head);
    let stdout_notices = report.stdout.notice.iter().chain(&report.notices);
    for notice in stdout_notices {
        stdout.push_str(&token.line(notice));
        stdout.push('\n');
    }
    stdout.push_str(&token.line(&format!("script_status {}", report.status)));
    stdout.push('\n');

    let mut stderr = with_line_end(&report.stderr.head);
    if let Some(notice) = &report.stderr.notice {
        stderr.push_str(&token.line(notice));
        stderr.push('\n');
    }
    (stdout, stderr)
}

fn render_json(report: &RunReport, exit_code: ExitCode) -> Result<String> {
    let envelope = Envelope {
        token: report.token.as_str(),
        script_status: report.status.to_string(),
        exit_code: exit_code.as_i32(),
        stdout: StreamEnvelope::of(&report.stdout),
        stderr: StreamEnvelope::of(&report.stderr),
        notices: &report.notices,
    };
    format_output(&envelope, OutputFormat::Json)
}

fn write_all(text: &str, mut sink: impl std::io::Write) -> Result<()> {
    sink.write_all(text.as_bytes())?;
    sink.flush()?;
    Ok(())
}

async fn build_report(
    token: RunToken,
    outcome: ProcessOutcome,
    args: &RunArgs,
    spill: &Spill,
    pre_notices: Vec<String>,
    call_report: RunCallReport,
) -> RunReport {
    let timeout_secs = args.timeout.get().as_secs();
    let mut notices = pre_notices;
    if outcome.status == ScriptStatus::TimedOut {
        notices.push(format!(
            "script killed after the {timeout_secs}s run timeout"
        ));
    }
    if outcome.drain_abandoned {
        notices.push(
            "output after script exit discarded: a process started by the script still holds \
             the output pipe"
                .to_string(),
        );
    }
    if outcome.status != ScriptStatus::Exit(0) {
        notices.extend(call_report.journal_summary);
    }
    notices.extend(call_report.dry_run_report);
    RunReport {
        stdout: report_stream("stdout", &outcome.stdout, args.max_output, spill).await,
        stderr: report_stream("stderr", &outcome.stderr, args.max_stderr, spill).await,
        status: outcome.status,
        notices,
        token,
    }
}

async fn remove_scratch(dir: &Path) {
    if let Err(error) = tokio::fs::remove_dir_all(dir).await {
        tracing::debug!("could not remove the run directory: {error}");
    }
}

/// Runs the `run` command.
///
/// Prints the anchor line, runs the script, then prints its budgeted output, runner notices and
/// a trailer carrying the script's status (or one JSON envelope with `--format json`).
///
/// # Errors
///
/// Returns an error for runner failures (input, node missing or too old, I/O); a failing script,
/// a timeout or a cancellation by SIGINT, SIGTERM, SIGHUP or SIGQUIT (exit 130, 143, 129 or 131)
/// is reported through the exit code instead.
pub async fn run(args: RunArgs, format: OutputFormat) -> Result<ExitCode> {
    refuse_inside_run()?;
    let mut termination = Termination::install();
    let servers_dir = ServersDir::resolve(args.servers_dir.clone())?;
    let dirs = RuntimeDirs::resolve()?;
    let run_id = RunId::new();
    // An uncancellable stdin read must not outlive a signal: returning `Ok` lets `main` exit.
    let script = tokio::select! {
        biased;
        signal = termination.recv() => {
            remove_scratch(&dirs.run_dir(&run_id)).await;
            return Ok(cancelled_exit(signal, "the script was not read"));
        }
        prepared = prepare_script(&args, &dirs, &run_id) => prepared?,
    };

    let result = execute(
        &args,
        format,
        servers_dir.as_path(),
        &dirs,
        &script,
        &run_id,
        &mut termination,
    )
    .await;
    if script.scratch {
        remove_scratch(&script.run_dir).await;
    }
    result
}

/// Saves the script when a save was planned and the run succeeded; returns the run's exit code.
async fn save_if_planned(save_plan: Option<SavePlan>, report: &mut RunReport) -> ExitCode {
    let exit_code = exit_code_of(report.status);
    let Some(plan) = save_plan else {
        return exit_code;
    };
    if exit_code != ExitCode::SUCCESS {
        report
            .notices
            .push("not saved: script did not succeed".to_string());
        return exit_code;
    }
    match save(&plan.skill_dir, &plan.name, &plan.text, plan.overwrite).await {
        Ok(path) => {
            report
                .notices
                .push(format!("saved script {}", path.display()));
            exit_code
        }
        Err(error) => {
            report
                .notices
                .push(format!("script succeeded; save failed: {error}"));
            ExitCode::ERROR
        }
    }
}

/// Reports a cancellation and returns its exit code.
///
/// A failed stderr write is only logged: a cancel path must always end in `Ok(exit code)`, or the
/// runtime would wait for the uncancellable stdin read instead of exiting.
fn cancelled_exit(signal: RunnerSignal, note: &str) -> ExitCode {
    if let Err(error) = write_all(
        &format!("run cancelled by {signal}; {note}\n"),
        std::io::stderr(),
    ) {
        tracing::debug!("could not write the cancel notice: {error:#}");
    }
    signal.exit_code()
}

/// Longest the runner spends closing the helper and its sessions after the script ended.
const WRAP_UP_BOUND: Duration = Duration::from_secs(4);

/// Waits for `finish` (closing the helper and its sessions).
///
/// A run that was not cancelled waits for it without a bound. Once a signal has arrived, whether
/// before (the script was cancelled) or during the wrap-up (the run becomes a cancelled one), the
/// wrap-up gets `bound`, and a further signal cuts it short; the call summary is then dropped.
async fn wrap_up(
    finish: impl Future<Output = RunCallReport>,
    termination: &mut Termination,
    outcome: &mut Result<ProcessOutcome>,
    notices: &mut Vec<String>,
    bound: Duration,
) -> RunCallReport {
    let mut finish = std::pin::pin!(finish);
    if !matches!(outcome, Ok(outcome) if matches!(outcome.status, ScriptStatus::Cancelled(_))) {
        tokio::select! {
            biased;
            signal = termination.recv() => {
                if let Ok(outcome) = outcome {
                    outcome.status = ScriptStatus::Cancelled(signal);
                }
            }
            report = &mut finish => return report,
        }
    }
    tokio::select! {
        biased;
        _ = termination.recv() => {}
        finished = tokio::time::timeout(bound, &mut finish) => {
            if let Ok(report) = finished {
                return report;
            }
        }
    }
    notices.push("call summary dropped: closing the MCP sessions was cut short".to_string());
    RunCallReport::default()
}

/// Prints the finished run's report. After a cancellation a write error (e.g. a closed pipe) is
/// only logged, so the cancel exit code is never lost.
fn emit_report(report: &RunReport, exit_code: ExitCode, format: OutputFormat) -> Result<()> {
    let written = if format == OutputFormat::Json {
        render_json(report, exit_code)
            .and_then(|json| write_all(&format!("{json}\n"), std::io::stdout()))
    } else {
        let (stdout, stderr) = render_text(report);
        write_all(&stdout, std::io::stdout()).and_then(|()| write_all(&stderr, std::io::stderr()))
    };
    match (written, report.status) {
        (Err(error), ScriptStatus::Cancelled(_)) => {
            tracing::debug!("could not write the cancelled run's report: {error:#}");
            Ok(())
        }
        (written, _) => written,
    }
}

async fn execute(
    args: &RunArgs,
    format: OutputFormat,
    servers_dir: &Path,
    dirs: &RuntimeDirs,
    script: &PreparedScript,
    run_id: &RunId,
    termination: &mut Termination,
) -> Result<ExitCode> {
    let save_plan = plan_save(args, script).await?;
    let node_args = if args.restrict {
        permission_args(&script.path, servers_dir, &script.run_dir)?
    } else {
        Vec::new()
    };
    tokio::select! {
        biased;
        signal = termination.recv() => {
            return Ok(cancelled_exit(signal, "the script was not started"));
        }
        checked = check_node_version() => checked?,
    };
    if let Err(error) = dirs.prune_runs().await {
        tracing::debug!("could not prune old run directories: {error:#}");
    }

    let policy = if args.dry_run {
        Some(Policy::load(args.policy.as_deref()).await?)
    } else {
        None
    };

    let mut notices = Vec::new();
    if args.typecheck {
        let checked = tokio::select! {
            biased;
            signal = termination.recv() => {
                return Ok(cancelled_exit(signal, "the script was not started"));
            }
            checked = typecheck(&script.path, servers_dir, &script.run_dir) => checked?,
        };
        match checked {
            TypecheckOutcome::Passed => {}
            TypecheckOutcome::Skipped { reason } => {
                notices.push(format!("typecheck skipped: {reason}"));
            }
            TypecheckOutcome::Failed { diagnostics } => {
                let summary = "type check failed; the script was not run\n";
                write_all(&format!("{diagnostics}{summary}"), std::io::stderr())?;
                return Ok(ExitCode::INVALID_INPUT);
            }
        }
    }

    if let Some(signal) = termination.recv().now_or_never() {
        return Ok(cancelled_exit(signal, "the script was not started"));
    }

    let token = RunToken::generate();
    if format != OutputFormat::Json {
        write_all(&format!("{}\n", token.anchor_line()), std::io::stdout())?;
    }
    let spec = ScriptProcess {
        script: script.path.clone(),
        args: args.args.clone(),
        import_argument: import_argument(servers_dir)?,
        node_args,
        timeout: args.timeout,
    };
    let journal = Arc::new(Journal::create(dirs, run_id).await?);
    let helper_config = HelperConfig {
        servers_dir: servers_dir.to_path_buf(),
        call_timeout: CallTimeout::new(args.timeout.get().min(DEFAULT_CALL_TIMEOUT))?,
        journal: Some(journal),
        dry_run: policy.map_or(DryRunMode::Off, DryRunMode::On),
    };
    let (helper, mut endpoint) = HelperServer::bind(Carrier::native(), helper_config)?;
    let mut outcome = run_script_process(&spec, Some(&mut endpoint), termination.recv()).await;
    let call_report = wrap_up(
        helper.finish(),
        termination,
        &mut outcome,
        &mut notices,
        WRAP_UP_BOUND,
    )
    .await;
    let mut report = build_report(
        token,
        outcome?,
        args,
        &Spill::new(dirs),
        notices,
        call_report,
    )
    .await;

    let exit_code = save_if_planned(save_plan, &mut report).await;
    emit_report(&report, exit_code, format)?;
    Ok(exit_code)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::commands::common::test_isolation::process_per_test;

    fn stream(head: &str, notice: Option<&str>) -> StreamReport {
        StreamReport {
            head: head.to_string(),
            total_bytes: head.len() as u64,
            spill: None,
            notice: notice.map(str::to_string),
        }
    }

    fn report(status: ScriptStatus) -> RunReport {
        RunReport {
            token: RunToken::generate(),
            status,
            stdout: stream("out", Some("stdout cut")),
            stderr: stream("err\n", Some("stderr cut")),
            notices: vec!["extra".to_string()],
        }
    }

    fn finished_outcome(status: ScriptStatus) -> ProcessOutcome {
        ProcessOutcome {
            status,
            stdout: Captured::default(),
            stderr: Captured::default(),
            drain_abandoned: false,
        }
    }

    async fn slow_report(delay: Duration) -> RunCallReport {
        tokio::time::sleep(delay).await;
        RunCallReport {
            journal_summary: Some("summary".to_string()),
            dry_run_report: None,
        }
    }

    #[tokio::test]
    async fn a_run_that_was_not_cancelled_waits_for_a_slow_wrap_up() {
        let mut termination = Termination::install();
        let mut outcome = Ok(finished_outcome(ScriptStatus::Exit(0)));
        let mut notices = Vec::new();
        let report = wrap_up(
            slow_report(Duration::from_millis(150)),
            &mut termination,
            &mut outcome,
            &mut notices,
            Duration::from_millis(20),
        )
        .await;
        assert_eq!(report.journal_summary.as_deref(), Some("summary"));
        assert_eq!(notices, Vec::<String>::new());
        assert_eq!(outcome.unwrap().status, ScriptStatus::Exit(0));
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_signal_during_the_wrap_up_cancels_the_run_and_a_second_one_cuts_it() {
        use crate::runtime::signal::raise_sigterm;

        if !process_per_test("a_signal_during_the_wrap_up_cancels_the_run_and_a_second_one_cuts_it")
        {
            return;
        }

        let mut termination = Termination::install();
        let mut outcome = Ok(finished_outcome(ScriptStatus::Exit(0)));
        let mut notices = Vec::new();
        raise_sigterm();
        // Signals coalesce, so the second one is raised only after the first was consumed.
        let second = tokio::spawn(async {
            tokio::time::sleep(Duration::from_millis(200)).await;
            raise_sigterm();
        });
        let report = tokio::time::timeout(
            Duration::from_secs(10),
            wrap_up(
                slow_report(Duration::from_secs(30)),
                &mut termination,
                &mut outcome,
                &mut notices,
                Duration::from_secs(30),
            ),
        )
        .await
        .expect("the second signal must cut the wrap-up short");
        second.await.unwrap();
        assert_eq!(report, RunCallReport::default());
        assert_eq!(notices.len(), 1);
        assert_eq!(
            outcome.unwrap().status,
            ScriptStatus::Cancelled(RunnerSignal::Terminate)
        );
    }

    #[tokio::test]
    async fn a_cancelled_run_bounds_a_slow_wrap_up_and_drops_the_summary() {
        let mut termination = Termination::install();
        let cancelled = ScriptStatus::Cancelled(RunnerSignal::Interrupt);
        let mut outcome = Ok(finished_outcome(cancelled));
        let mut notices = Vec::new();
        let report = wrap_up(
            slow_report(Duration::from_secs(30)),
            &mut termination,
            &mut outcome,
            &mut notices,
            Duration::from_millis(50),
        )
        .await;
        assert_eq!(report, RunCallReport::default());
        assert_eq!(notices.len(), 1);
        assert_eq!(outcome.unwrap().status, cancelled);
    }

    #[test]
    fn spill_notice_says_first_bytes_only_when_the_capture_was_cut() {
        let cut = Captured {
            bytes: vec![0; 10],
            total: 25,
        };
        let notice = spill_notice("stdout", 4, &cut, Some(Path::new("/tmp/s")));
        assert_eq!(
            notice,
            "stdout cut at 4 of 25 bytes; first 10 bytes saved to /tmp/s"
        );

        let whole = Captured {
            bytes: vec![0; 10],
            total: 10,
        };
        let notice = spill_notice("stderr", 4, &whole, None);
        assert_eq!(
            notice,
            "stderr cut at 4 of 10 bytes; full output could not be saved"
        );
    }

    #[test]
    fn save_takes_a_validated_recipe_name_at_parse_time() {
        use clap::Parser as _;
        let argv = ["mcp-execution-cli", "run", "s.ts", "--save", "../escape"];
        let error = crate::cli::Cli::try_parse_from(argv).unwrap_err();
        assert_eq!(error.exit_code(), 2);
        let argv = ["mcp-execution-cli", "run", "s.ts", "--save", "good-name"];
        assert!(crate::cli::Cli::try_parse_from(argv).is_ok());
    }

    #[test]
    fn save_cannot_be_combined_with_dry_run() {
        use clap::Parser as _;
        let argv = [
            "mcp-execution-cli",
            "run",
            "s.ts",
            "--dry-run",
            "--save",
            "x",
        ];
        let error = crate::cli::Cli::try_parse_from(argv).unwrap_err();
        assert_eq!(error.exit_code(), 2);
    }

    #[test]
    fn exit_codes_separate_script_failures_from_runner_timeouts() {
        assert_eq!(exit_code_of(ScriptStatus::Exit(0)), ExitCode::SUCCESS);
        assert_eq!(exit_code_of(ScriptStatus::Exit(4)), ExitCode::SCRIPT_FAILED);
        assert_eq!(
            exit_code_of(ScriptStatus::Signal(9)),
            ExitCode::SCRIPT_FAILED
        );
        assert_eq!(exit_code_of(ScriptStatus::TimedOut), ExitCode::TIMEOUT);
        assert_eq!(
            exit_code_of(ScriptStatus::Cancelled(RunnerSignal::Interrupt)),
            ExitCode::INTERRUPTED
        );
        assert_eq!(
            exit_code_of(ScriptStatus::Cancelled(RunnerSignal::Terminate)),
            ExitCode::TERMINATED
        );
    }

    #[test]
    fn text_report_puts_the_notices_and_trailer_after_the_script_output() {
        let report = report(ScriptStatus::Exit(3));
        let token = report.token.as_str().to_string();
        let (stdout, stderr) = render_text(&report);

        let lines: Vec<&str> = stdout.lines().collect();
        assert_eq!(lines[0], "out");
        assert_eq!(lines[1], format!("[mcp-execution {token}] stdout cut"));
        assert_eq!(lines[2], format!("[mcp-execution {token}] extra"));
        assert_eq!(
            lines[3],
            format!("[mcp-execution {token}] script_status exit 3")
        );
        assert_eq!(stderr, format!("err\n[mcp-execution {token}] stderr cut\n"));
    }

    #[test]
    fn json_report_is_one_object_with_the_status() {
        let report = report(ScriptStatus::Signal(9));
        let json = render_json(&report, ExitCode::SCRIPT_FAILED).unwrap();
        let value: serde_json::Value = serde_json::from_str(&json).unwrap();
        assert_eq!(value["script_status"], "signal 9");
        assert_eq!(value["exit_code"], 6);
        assert_eq!(value["stdout"]["truncated"], true);
    }

    #[test]
    fn spill_complete_is_null_with_spill_and_otherwise_says_whether_the_file_is_whole() {
        for (spill, expected) in [
            (None, serde_json::Value::Null),
            (
                Some(SpillRef {
                    path: PathBuf::from("/tmp/s"),
                    complete: false,
                }),
                serde_json::Value::Bool(false),
            ),
            (
                Some(SpillRef {
                    path: PathBuf::from("/tmp/s"),
                    complete: true,
                }),
                serde_json::Value::Bool(true),
            ),
        ] {
            let report = StreamReport {
                spill,
                ..stream("out", None)
            };
            let value = serde_json::to_value(StreamEnvelope::of(&report)).unwrap();
            assert_eq!(value["spill_complete"], expected);
            assert_eq!(value["spill"].is_null(), expected.is_null());
        }
    }
}
