//! Command execution and runtime logic.
//!
//! Contains the main command execution loop and logging initialization.

use anyhow::Result;
use mcp_execution_core::Error as CoreError;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_files::FilesError;
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::cli::Commands;
use crate::commands;
use crate::commands::common::ServerSource;
use crate::formatters::escape_error_text;

/// Initializes logging infrastructure.
///
/// Sets up tracing with appropriate log levels based on verbosity flag.
/// Writes log messages to stderr.
///
/// # Arguments
///
/// * `verbose` - If true, sets log level to DEBUG; otherwise uses INFO or
///   environment variable override via `RUST_LOG`
///
/// # Errors
///
/// This function cannot fail—it always returns `Ok(())`. Multiple calls
/// in the same process will panic rather than returning an error, but this
/// is not a recoverable condition and indicates a programming error.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::runner;
///
/// // `no_run`: this installs a process-global tracing subscriber, which
/// // panics if called more than once in the same process.
/// runner::init_logging(false)?;
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn init_logging(verbose: bool) -> Result<()> {
    let filter = if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .init();

    Ok(())
}

/// Executes the specified CLI command.
///
/// Routes commands to their respective handlers. On success, returns the exit
/// code reported by the handler. If the handler fails, the error is printed
/// to stderr and classified into a semantic [`ExitCode`] via
/// `classify_exit_code` rather than propagated — this lets `main` always
/// turn the result into a process exit code without falling back to anyhow's
/// default behavior of collapsing every `Err` to exit code 1.
///
/// # Arguments
///
/// * `command` - The parsed CLI command to execute
/// * `output_format` - Output format preference (JSON, text, or pretty)
///
/// # Errors
///
/// This function does not propagate command execution failures as `Err` —
/// see above. It is fallible in signature to match this crate's convention
/// of using `Result` consistently across command handlers.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::cli::Commands;
/// use mcp_execution_cli::runner;
/// use mcp_execution_core::cli::OutputFormat;
///
/// # async fn example() -> anyhow::Result<()> {
/// let exit_code = runner::execute_command(
///     Commands::Setup,
///     OutputFormat::Pretty,
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn execute_command(command: Commands, output_format: OutputFormat) -> Result<ExitCode> {
    Ok(match dispatch(command, output_format).await {
        Ok(code) => code,
        Err(err) => report_and_classify(&err),
    })
}

/// Routes `command` to its handler and returns the handler's result unclassified.
///
/// # Errors
///
/// Returns whatever error the dispatched command handler produces.
async fn dispatch(command: Commands, output_format: OutputFormat) -> Result<ExitCode> {
    match command {
        Commands::Introspect { flags, detailed } => {
            let source = ServerSource::try_from(flags)?;
            commands::introspect::run(source, detailed, output_format).await
        }
        Commands::Skill {
            server,
            servers_dir,
            output,
            skill_name,
            hints,
            overwrite,
        } => {
            commands::skill::run(
                server,
                servers_dir,
                output,
                skill_name,
                hints,
                overwrite,
                output_format,
            )
            .await
        }
        Commands::Generate {
            flags,
            name,
            progressive_output,
            dry_run,
        } => {
            let source = ServerSource::try_from(flags)?;
            commands::generate::run(source, name, progressive_output, dry_run, output_format).await
        }
        Commands::Server { action } => commands::server::run(action, output_format).await,
        Commands::Setup => commands::setup::run(output_format).await,
        Commands::Completions { shell } => run_completions(shell).await,
    }
}

/// Runs the `completions` subcommand: builds the clap command tree and generates the shell
/// completion script for it.
async fn run_completions(shell: clap_complete::Shell) -> Result<ExitCode> {
    use crate::cli::Cli;
    use clap::CommandFactory;
    let mut cmd = Cli::command();
    commands::completions::run(shell, &mut cmd).await
}

/// Prints `err` to stderr, then classifies it into a semantic [`ExitCode`].
///
/// Structurally matches anyhow's default `main`-error format (a summary line, then a numbered
/// "Caused by:" section for any further causes), but with each cause's own text — not anyhow's
/// surrounding structure — passed through [`escape_error_text`] before printing. Classification is
/// via `classify_exit_code`.
///
/// Shared by [`execute_command`] (command-handler failures) and `main`
/// (pre-dispatch failures, e.g. an invalid `--format` value), so every
/// failure this CLI can produce is reported and exits the same way. An
/// error's cause chain can embed content from an untrusted MCP server (e.g.
/// a JSON-RPC error `message`), and both `anyhow::Error`'s `Debug` rendering
/// and the `thiserror`-derived `Display` impls it walks interpolate that
/// content verbatim — so `err`'s formatted report is sanitized via
/// `sanitized_error_report` before printing.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runner;
/// use mcp_execution_core::Error as CoreError;
/// use mcp_execution_core::cli::ExitCode;
///
/// let err = anyhow::Error::from(CoreError::InvalidArgument(
///     "invalid output format: 'xml' (expected: json, text, or pretty)".to_string(),
/// ));
/// assert_eq!(runner::report_and_classify(&err), ExitCode::INVALID_INPUT);
/// ```
#[must_use]
pub fn report_and_classify(err: &anyhow::Error) -> ExitCode {
    eprintln!("Error: {}", sanitized_error_report(err));
    classify_exit_code(err)
}

/// Renders `err`'s cause chain — and, if captured, its backtrace — exactly as
/// [`report_and_classify`] prints them: a summary line, then (if there are further causes) a
/// "Caused by:" section listing each one, numbered from 0, then (if `RUST_BACKTRACE`/
/// `RUST_LIB_BACKTRACE` caused one to be captured) a "Stack backtrace:" section. Each cause's own
/// rendered text is sanitized individually via [`escape_error_text`] (capped to 4000 chars each);
/// the backtrace is not, and is not length-capped either.
///
/// An earlier version of this function sanitized anyhow's fully-rendered `{err:?}` report as one
/// blob. That could not tell anyhow's own trusted structural newlines/indentation (between `Caused
/// by:` frames, and throughout a backtrace) apart from a `\n` embedded in one cause's own
/// untrusted `Display` text (e.g. a hostile MCP server's JSON-RPC error `message`, which
/// `anyhow`/`thiserror` interpolate verbatim) — so it neutralized both alike, collapsing a
/// legitimate multi-cause chain, and any backtrace, onto one line and truncating the result well
/// short of a typical backtrace's length, for no security benefit. Building the report from
/// [`anyhow::Error::chain`] instead sanitizes only each cause's own text and rejoins with
/// `\n\nCaused by:\n{n:>5}: ` separators this function itself writes, so a hostile cause cannot
/// forge those separators (any `\n` in *its* text is still neutralized) while a chain with only
/// trusted causes keeps its real multi-line structure. [`anyhow::Error::backtrace`] is not part of
/// `chain()` — it is captured once from the local call stack at the point `err` was constructed —
/// so it carries nothing an external MCP server could have influenced, and is appended verbatim.
///
/// Deliberately does not reproduce one thing anyhow's own `{err:?}` output has: it always numbers
/// every cause, where anyhow omits the number when there is exactly one. That's structural/local
/// formatting with nothing untrusted in it, so it isn't a correctness concern for this function's
/// purpose — a simplification to avoid depending on anyhow's private formatting internals, not the
/// reason this exists. The backtrace section's own layout mirrors anyhow's
/// (`anyhow-1.0.104/src/fmt.rs`'s `ErrorImpl::debug`) via the same public
/// [`anyhow::Error::backtrace`] accessor it uses internally.
///
/// Factored out of [`report_and_classify`] (rather than inlined) so tests can assert on precisely
/// the string that reaches stderr by calling this directly, instead of recomputing the same
/// pipeline independently and asserting against that — which would silently drift from the real
/// code path if either implementation changed without the other.
fn sanitized_error_report(err: &anyhow::Error) -> String {
    use std::backtrace::BacktraceStatus;
    use std::fmt::Write as _;

    let mut links = err.chain();

    let mut report = links
        .next()
        .map_or_else(String::new, |top| escape_error_text(&top.to_string()));

    let causes: Vec<_> = links.collect();
    if !causes.is_empty() {
        report.push_str("\n\nCaused by:");
        for (n, cause) in causes.into_iter().enumerate() {
            // `write!` into a `String` is infallible.
            let _ = write!(
                report,
                "\n{n:>5}: {}",
                escape_error_text(&cause.to_string())
            );
        }
    }

    let backtrace = err.backtrace();
    if backtrace.status() == BacktraceStatus::Captured {
        // Trusted, locally-generated content (source file paths, function names from this
        // binary's own stack) — deliberately not sanitized or length-capped, unlike the chain
        // links above. Mirrors anyhow's own `ErrorImpl::debug` header handling: some Rust/backtrace
        // versions' `Backtrace::to_string()` already starts with a lowercase "stack backtrace:"
        // header, others don't.
        let mut backtrace_text = backtrace.to_string();
        report.push_str("\n\n");
        if backtrace_text.starts_with("stack backtrace:") {
            backtrace_text.replace_range(0..1, "S");
        } else {
            report.push_str("Stack backtrace:\n");
        }
        backtrace_text.truncate(backtrace_text.trim_end().len());
        report.push_str(&backtrace_text);
    }

    report
}

/// Classifies an [`anyhow::Error`] returned by a command handler into a
/// semantic [`ExitCode`].
///
/// Walks the error's cause chain looking for a [`CoreError`] — the concrete
/// type every command handler ultimately produces via `?` — and delegates to
/// [`classify_core_error`] for the variant-to-exit-code mapping. Falls back to checking for a
/// [`FilesError`] (the `generate` command's `export_to_filesystem` errors are wrapped via
/// `anyhow::Context` rather than converted to `CoreError`, so they would otherwise never match
/// the first check and always fall through to the generic [`ExitCode::ERROR`] — issue #198 M6).
/// Errors that match neither (e.g. CLI argument parsing, serialization) fall back to
/// [`ExitCode::ERROR`].
fn classify_exit_code(error: &anyhow::Error) -> ExitCode {
    if let Some(core_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<CoreError>())
    {
        return classify_core_error(core_error);
    }

    if let Some(files_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<FilesError>())
    {
        return match files_error {
            // Same "the server is at fault" classification as `CoreError::ResourceLimitExceeded`
            // above — the export this bounds is sized by what the (possibly hostile or
            // misbehaving) introspected server returned, not by CLI-caller-supplied input.
            FilesError::ResourceLimitExceeded { .. } => ExitCode::SERVER_ERROR,
            FilesError::FileNotFound { .. }
            | FilesError::NotADirectory { .. }
            | FilesError::InvalidPath { .. }
            | FilesError::PathNotAbsolute { .. }
            | FilesError::InvalidPathComponent { .. }
            | FilesError::PathEscapesBase { .. }
            | FilesError::IoError { .. } => ExitCode::ERROR,
        };
    }

    ExitCode::ERROR
}

/// Classifies a single [`CoreError`] variant.
///
/// [`CoreError::ScriptGenerationError`] wraps an arbitrary underlying failure (schema
/// extraction, template rendering, output tracking) behind one variant so a codegen error can
/// always be attributed to the tool that caused it; that wrapping must not also collapse the
/// wrapped cause's own exit-code classification (e.g. a wrapped
/// [`CoreError::ResourceLimitExceeded`] should still report [`ExitCode::SERVER_ERROR`], not the
/// generic code every other `ScriptGenerationError` gets). Recursing into `source` when it
/// downcasts to another `CoreError` preserves that.
fn classify_core_error(core_error: &CoreError) -> ExitCode {
    match core_error {
        CoreError::Timeout { .. } => ExitCode::TIMEOUT,
        // A resource limit is exceeded by data the remote MCP server returned (tool
        // count, schema size, etc.), not by the CLI caller's own arguments — same
        // "the server is at fault" classification as `ConnectionFailed`.
        CoreError::ConnectionFailed { .. } | CoreError::ResourceLimitExceeded { .. } => {
            ExitCode::SERVER_ERROR
        }
        CoreError::ValidationError { .. }
        | CoreError::SecurityViolation { .. }
        | CoreError::InvalidArgument(_) => ExitCode::INVALID_INPUT,
        // A duplicate generated-file path indicates a codegen invariant was violated (e.g. a
        // reserved output filename not seeded into name-collision resolution), not something
        // caused by the remote server's data or the CLI caller's own arguments.
        CoreError::SerializationError { .. } | CoreError::DuplicateGeneratedFilePath { .. } => {
            ExitCode::ERROR
        }
        CoreError::ScriptGenerationError { source, .. } => source
            .as_deref()
            .and_then(|source| source.downcast_ref::<CoreError>())
            .map_or(ExitCode::ERROR, classify_core_error),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::ResourceKind;
    use mcp_execution_core::ServerId;

    fn wrap(core_error: CoreError) -> anyhow::Error {
        anyhow::Error::new(core_error)
    }

    #[test]
    fn test_classify_exit_code_timeout() {
        let err = wrap(CoreError::Timeout {
            operation: "discover".to_string(),
            duration_secs: 30,
        });
        assert_eq!(classify_exit_code(&err), ExitCode::TIMEOUT);
    }

    #[test]
    fn test_classify_exit_code_connection_failed() {
        let err = wrap(CoreError::ConnectionFailed {
            server: "test".to_string(),
            source: "refused".into(),
        });
        assert_eq!(classify_exit_code(&err), ExitCode::SERVER_ERROR);
    }

    #[test]
    fn test_classify_exit_code_resource_limit_exceeded() {
        let err = wrap(CoreError::ResourceLimitExceeded {
            resource: ResourceKind::ToolCount {
                server_id: ServerId::new("github").unwrap(),
            },
            actual: 1500,
            limit: 1000,
        });
        assert_eq!(classify_exit_code(&err), ExitCode::SERVER_ERROR);
    }

    /// #198 M6 — `FilesError` (e.g. from `generate`'s `export_to_filesystem`, wrapped via
    /// `anyhow::Context` rather than converted to `CoreError`) must be classified too, not
    /// fall through to the generic `ExitCode::ERROR` unconditionally.
    #[test]
    fn test_classify_exit_code_files_error_resource_limit_exceeded() {
        let files_error = FilesError::ResourceLimitExceeded {
            resource: "export file count".to_string(),
            actual: 3000,
            limit: 2000,
        };
        // Mirrors how `commands::generate::run` actually wraps this error.
        let err: anyhow::Error =
            anyhow::Error::new(files_error).context("failed to export files to filesystem");

        assert_eq!(classify_exit_code(&err), ExitCode::SERVER_ERROR);
    }

    #[test]
    fn test_classify_exit_code_files_error_other_variant_falls_back_to_error() {
        let err = anyhow::Error::new(FilesError::FileNotFound {
            path: "/missing".to_string(),
        });
        assert_eq!(classify_exit_code(&err), ExitCode::ERROR);
    }

    #[test]
    fn test_classify_exit_code_validation_error() {
        let err = wrap(CoreError::ValidationError {
            field: "connect_timeout".to_string(),
            reason: "must be greater than zero".to_string(),
        });
        assert_eq!(classify_exit_code(&err), ExitCode::INVALID_INPUT);
    }

    #[test]
    fn test_classify_exit_code_security_violation() {
        let err = wrap(CoreError::SecurityViolation {
            reason: "forbidden env var".to_string(),
        });
        assert_eq!(classify_exit_code(&err), ExitCode::INVALID_INPUT);
    }

    #[test]
    fn test_classify_exit_code_invalid_argument() {
        let err = wrap(CoreError::InvalidArgument("bad flag".to_string()));
        assert_eq!(classify_exit_code(&err), ExitCode::INVALID_INPUT);
    }

    #[test]
    fn test_classify_exit_code_other_core_errors_fall_back_to_error() {
        let err = wrap(CoreError::SerializationError {
            message: "bad json".to_string(),
            source: None,
        });
        assert_eq!(classify_exit_code(&err), ExitCode::ERROR);

        let err = wrap(CoreError::ScriptGenerationError {
            tool: "example_tool".to_string(),
            message: "template rendering failed".to_string(),
            source: None,
        });
        assert_eq!(classify_exit_code(&err), ExitCode::ERROR);
    }

    /// `ScriptGenerationError` wraps its cause via `source` (see
    /// `ProgressiveGenerator::wrap_tool_generation_error`) precisely so this recursion can
    /// still find the original classification instead of collapsing every wrapped cause to the
    /// generic exit code.
    #[test]
    fn test_classify_exit_code_script_generation_error_recurses_into_wrapped_source() {
        let err = wrap(CoreError::ScriptGenerationError {
            tool: "example_tool".to_string(),
            message: "failed to track generated tool file".to_string(),
            source: Some(Box::new(CoreError::ResourceLimitExceeded {
                resource: ResourceKind::GeneratedOutputSize,
                actual: 10,
                limit: 5,
            })),
        });
        assert_eq!(classify_exit_code(&err), ExitCode::SERVER_ERROR);
    }

    #[test]
    fn test_classify_exit_code_non_core_error_falls_back_to_error() {
        let err = anyhow::anyhow!("plain CLI-layer failure");
        assert_eq!(classify_exit_code(&err), ExitCode::ERROR);
    }

    #[test]
    fn test_classify_exit_code_finds_core_error_through_context_chain() {
        // The command handlers wrap `mcp_execution_core::Error` with
        // `.with_context(...)` before it reaches `execute_command` — the
        // classifier must find it through that wrapping, not just at the top.
        let err = wrap(CoreError::Timeout {
            operation: "connect".to_string(),
            duration_secs: 5,
        })
        .context("failed to connect to server 'test' - ensure the server is installed");
        assert_eq!(classify_exit_code(&err), ExitCode::TIMEOUT);
    }

    #[tokio::test]
    async fn test_execute_command_converts_failure_into_classified_exit_code_not_err() {
        // Regression test for #195: a failing command must surface as
        // `Ok(non_success_exit_code)`, never as `Err`, so `main` can always
        // reach `std::process::exit` with the classified code instead of
        // falling back to anyhow's default exit-code-1 handling. Asserting
        // the exact `SERVER_ERROR` value (not just `!is_success()`) so a
        // regression to the generic `ExitCode::ERROR` fallback is caught.
        //
        // Built via real clap parsing (rather than a `Commands::Introspect`
        // literal): `ServerFlags`'s fields are private outside `cli.rs` by
        // design, so this is the only way an external module can produce one.
        use clap::Parser as _;
        let cli = crate::cli::Cli::parse_from([
            "mcp-execution-cli",
            "introspect",
            "nonexistent-server-for-exit-code-test",
        ]);
        let result = execute_command(cli.command, OutputFormat::Json).await;

        let exit_code = result.expect("execute_command must not propagate Err");
        assert_eq!(exit_code, ExitCode::SERVER_ERROR);
    }

    #[test]
    fn test_report_and_classify_prints_and_classifies() {
        // Regression test for #195/S2: `main` routes pre-dispatch failures
        // (e.g. an invalid `--format` value) through this same function, not
        // just command-handler failures via `execute_command`.
        let err = anyhow::Error::from(CoreError::InvalidArgument(
            "invalid output format: 'xml' (expected: json, text, or pretty)".to_string(),
        ));
        assert_eq!(report_and_classify(&err), ExitCode::INVALID_INPUT);
    }

    #[test]
    fn test_report_and_classify_escapes_control_chars_in_error_chain() {
        // Regression test for #308: a malicious/compromised MCP server can embed raw ANSI/control
        // escape sequences in a JSON-RPC error message, which end up in the `Display` string of a
        // `CoreError::ConnectionFailed`'s wrapped `source` — a distinct link in `err.chain()`,
        // since `ConnectionFailed`'s own `#[error(...)]` message never interpolates `{source}` —
        // and, by extension, in the "Caused by:" section of `sanitized_error_report`'s output.
        // `report_and_classify` must neutralize those bytes before printing to stderr rather than
        // passing them through verbatim. Calls `sanitized_error_report` directly — the exact
        // helper `report_and_classify` prints — rather than recomputing the same pipeline inline,
        // so this can't silently drift from the real code path.
        let source: Box<dyn std::error::Error + Send + Sync> =
            "boom\u{1b}[2J\u{1b}]0;pwned\u{7}msg".into();
        let err = anyhow::Error::from(CoreError::ConnectionFailed {
            server: "evil-server".to_string(),
            source,
        });

        let report = sanitized_error_report(&err);
        assert!(!report.contains('\u{1b}'));
        assert!(!report.contains('\u{7}'));
        assert_eq!(report_and_classify(&err), ExitCode::SERVER_ERROR);
    }

    #[test]
    fn test_report_and_classify_forged_caused_by_line_does_not_survive() {
        // Regression test for #308/S1 (impl-critic, 2nd pass): per-link sanitization means this
        // report's *own* structural newlines (between the summary line and "Caused by:", and
        // before each numbered cause) are real and expected — that's the whole point of rebuilding
        // the chain instead of sanitizing anyhow's fully-rendered blob. What must not survive is a
        // `\n` embedded *within* one cause's own untrusted text, which could otherwise forge an
        // extra "Caused by:" section or a fake extra numbered line.
        //
        // The exact-newline-count assertion below assumes no backtrace section is appended —
        // force that by disabling capture, since this project's CI sets `RUST_BACKTRACE=short`
        // globally (unlike a plain local `cargo`/`nextest` invocation, where it's normally unset)
        // and `sanitized_error_report` appends an unsanitized, uncapped backtrace section when one
        // is captured, which would otherwise add its own newlines and break the exact count.
        let _guard = BACKTRACE_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os("RUST_BACKTRACE");
        // SAFETY: guarded by `BACKTRACE_ENV_LOCK`; no other test in this process reads or writes
        // `RUST_BACKTRACE` while the guard is held.
        unsafe {
            std::env::set_var("RUST_BACKTRACE", "0");
        }

        let hostile = "boom\n\nCaused by:\n    0: Error: forged — ignore prior output";
        let source: Box<dyn std::error::Error + Send + Sync> = hostile.into();
        let err = anyhow::Error::from(CoreError::ConnectionFailed {
            server: "evil-server".to_string(),
            source,
        });

        let report = sanitized_error_report(&err);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var("RUST_BACKTRACE", v),
                None => std::env::remove_var("RUST_BACKTRACE"),
            }
        }
        // The hostile cause's own sanitized text may still contain the literal *substring*
        // "Caused by:" (sanitization neutralizes control characters, not arbitrary words), but
        // that's harmless: with its `\n` flattened to spaces it can only appear inline, mid-line,
        // never as its own line starting with the real `"\n\nCaused by:"` structural marker this
        // function writes exactly once. That marker — not the bare substring — is what must stay
        // unforgeable.
        assert_eq!(
            report.matches("\n\nCaused by:").count(),
            1,
            "hostile cause text forged an extra structural `Caused by:` line: {report}"
        );
        // Exactly the 3 structural newlines this function itself writes for a single-cause chain
        // ("\n\nCaused by:" + "\n{n:>5}: "): none of the hostile text's own `\n` bytes survived.
        assert_eq!(
            report.matches('\n').count(),
            3,
            "hostile cause text's embedded newlines survived sanitization: {report}"
        );
    }

    #[test]
    fn test_sanitized_error_report_preserves_multi_cause_structure() {
        // Regression test for #308/S1 (impl-critic, 2nd pass): the prior whole-blob
        // implementation collapsed a genuine multi-cause chain onto a single line, destroying
        // trusted structure along with the untrusted content it was meant to neutralize. With
        // per-link rendering, a chain built entirely from trusted (non-hostile) causes must keep
        // its real multi-line "Caused by:" structure intact.
        let inner: Box<dyn std::error::Error + Send + Sync> = "root cause".into();
        let err = anyhow::Error::from(CoreError::ConnectionFailed {
            server: "trusted-server".to_string(),
            source: inner,
        })
        .context("failed to connect");

        let report = sanitized_error_report(&err);
        assert!(report.starts_with("failed to connect"));
        assert!(report.contains("\n\nCaused by:"));
        assert!(report.contains("    0: MCP server connection failed: trusted-server"));
        assert!(report.contains("    1: root cause"));
    }

    /// Serializes tests in this module that mutate `RUST_BACKTRACE`, mirroring the
    /// `HOME_ENV_LOCK` pattern `commands::common`/`commands::server`'s tests already use for
    /// env-var mutation: a safety net for plain `cargo test` (which shares one process across a
    /// crate's tests), not required by the mandated `cargo nextest run` (which isolates every
    /// test in its own process).
    static BACKTRACE_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn test_sanitized_error_report_preserves_backtrace_when_captured() {
        // Regression test for #308/S1 (impl-critic, 3rd pass): a backtrace anyhow captures under
        // `RUST_BACKTRACE=1` is fully local, trusted content (source paths, function names from
        // this binary's own stack) with nothing an external MCP server could have influenced, so
        // it must survive `sanitized_error_report` untouched rather than being dropped or
        // sanitized/truncated like a chain link. `RUST_BACKTRACE` must be set *before* the error
        // is constructed — anyhow captures the backtrace (if any) at that point, not lazily at
        // format time.
        let _guard = BACKTRACE_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os("RUST_BACKTRACE");
        // SAFETY: guarded by `BACKTRACE_ENV_LOCK`; no other test in this process reads or writes
        // `RUST_BACKTRACE` while the guard is held.
        unsafe {
            std::env::set_var("RUST_BACKTRACE", "1");
        }

        let err = anyhow::Error::msg("boom");
        let captured = err.backtrace().status() == std::backtrace::BacktraceStatus::Captured;

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var("RUST_BACKTRACE", v),
                None => std::env::remove_var("RUST_BACKTRACE"),
            }
        }

        // Best-effort: some environments (e.g. certain sandboxes, targets without frame-pointer
        // unwind info) leave backtrace capture `Disabled`/`Unsupported` even with the env var set
        // — nothing this function controls, so only assert the positive case when it applies.
        if captured {
            let report = sanitized_error_report(&err);
            assert!(
                report.contains("tack backtrace:"),
                "captured backtrace did not survive: {report}"
            );
        }
    }
}
