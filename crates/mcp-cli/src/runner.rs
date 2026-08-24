//! Command execution and runtime logic.
//!
//! Contains the main command execution loop and logging initialization.

use std::io::{self, Write};

use anyhow::Result;
use mcp_execution_core::Error as CoreError;
use mcp_execution_core::cli::{ExitCode, LOG_FORMAT_ENV_VAR, LogFormat, OutputFormat};
use mcp_execution_files::FilesError;
use tracing_subscriber::{EnvFilter, Layer as _, layer::SubscriberExt, util::SubscriberInitExt};

use crate::cli::Commands;
use crate::commands;
use crate::commands::common::ServerSource;
use crate::formatters::escape_error_text;

/// [`Write`] wrapper that redacts embedded secrets out of each buffer before forwarding it to the
/// inner sink.
///
/// Exists because `rmcp`'s own `tracing` targets (e.g. `rmcp::transport::worker`'s `ERROR` line on
/// a connection failure) format a `reqwest::Error` whose `Display` embeds the full request URL,
/// query string included, and log it directly — bypassing every redacting `Debug` impl this
/// project applies to its own types, since this project never constructs that line's text.
/// `tracing-subscriber`'s fmt layer formats each event into a buffer and issues exactly one
/// [`write_all`](Write::write_all) call per event (verified against `tracing-subscriber` 0.3.23's
/// `fmt_layer` internals), so `write` here always receives one whole formatted event line, which
/// [`mcp_execution_core::redact_urls_in_text`] can scan and redact as a unit.
///
/// Generic over the inner writer so tests can redirect to an in-memory buffer instead of the real
/// `stderr` [`init_logging`] wraps it around.
struct RedactingWriter<W>(W);

impl<W: Write> Write for RedactingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let text = String::from_utf8_lossy(buf);
        self.0
            .write_all(mcp_execution_core::redact_urls_in_text(&text).as_bytes())?;
        // The whole input was consumed and forwarded (redaction only ever
        // changes the byte count written *downstream*, not how much of
        // `buf` this call accounts for), so report all of it as written.
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        self.0.flush()
    }
}

/// Resolves the effective log format from `log_format` (the `--log-format` flag value) and the
/// `MCP_EXECUTION_LOG_FORMAT` environment variable, mirroring `mcp-execution-server`'s
/// `resolve_log_format`. Kept as its own function (rather than inlined in [`init_logging`]) so a
/// test can assert the environment variable is actually consulted -- see
/// `resolve_log_format_reads_env_var_when_flag_unset` -- rather than only exercising the pure
/// `LogFormat::resolve` this delegates to.
fn resolve_log_format(log_format: Option<LogFormat>) -> LogFormat {
    let env_value = std::env::var(LOG_FORMAT_ENV_VAR).ok();
    LogFormat::resolve(log_format, env_value.as_deref())
}

/// Whether [`init_logging`] should warn about a rejected `MCP_EXECUTION_LOG_FORMAT` value: the
/// flag was not passed (matching `LogFormat::resolve`'s own precedence -- a bad env value is not
/// even inspected once the flag has decided, so it must not warn either) and the environment
/// variable is set to a non-empty value [`LogFormat::is_invalid_env_value`] rejects. Kept
/// separate from [`resolve_log_format`] (rather than a second return value there) since
/// production code needs only a yes/no answer, not the rejected value itself -- see
/// [`LogFormat::resolve`]'s own doc comment on why that value isn't threaded through.
fn log_format_env_is_invalid(log_format: Option<LogFormat>) -> bool {
    log_format.is_none()
        && std::env::var(LOG_FORMAT_ENV_VAR).is_ok_and(|raw| LogFormat::is_invalid_env_value(&raw))
}

/// Caps `rmcp`'s own `tracing` targets at `info`, on top of whatever base filter is already in
/// effect.
///
/// `rmcp` 3.1.2's transport layer logs raw, unsanitized peer input at `debug` level. This crate is
/// a *client* of third-party MCP servers (see `mcp_execution_introspector::Introspector`), so
/// without this cap, `--verbose` alone -- with no `RUST_LOG` involved -- streams an untrusted
/// server's raw stdout lines into stderr; [`RedactingWriter`] only rewrites embedded URLs, it does
/// not neutralize this (issue #421). This closes the *debug-level, raw-line* logging specifically
/// -- `rmcp` also logs a `Debug`-formatted peer notification at `info`, which this cap does not
/// and cannot suppress (`rmcp=info` still allows `info`); that site is mitigated by
/// `Debug`-escaping control characters, not eliminated.
///
/// Applied via [`EnvFilter::add_directive`] to the filter already selected by [`init_logging`]'s
/// verbose/non-verbose branches, not folded into only one of them: a directive added solely to
/// the non-verbose fallback string would never apply to `--verbose`'s `EnvFilter::new("debug")`,
/// which does not consult `RUST_LOG` at all.
///
/// Directive sets order by target specificity, so this `rmcp=info` directive (more specific than
/// a bare global `debug`) wins over it. An operator who explicitly sets a *more specific*
/// directive, e.g. `RUST_LOG=rmcp::transport=debug`, still wins over this one -- that is
/// intentional: this cap closes the accidental broad-`debug` case, not an operator's deliberate
/// request for `rmcp` transport debug logs -- **this is the escape hatch**: a target under
/// `rmcp::` (not the bare `rmcp` target this cap sets) survives the cap and can be raised back to
/// `debug` explicitly. Note this is target *specificity*, not level: an equally-specific
/// `RUST_LOG=rmcp=debug` (same target as this cap, different level) is *replaced* by this cap's
/// `rmcp=info`, not merged with it -- `tracing_subscriber`'s `Directive` ordering does not compare
/// level, so a same-target `add_directive` call overwrites the existing entry. Both behaviors are
/// pinned by tests below rather than assumed.
fn cap_rmcp_log_level(filter: EnvFilter) -> EnvFilter {
    filter.add_directive(
        "rmcp=info"
            .parse()
            .expect("static \"rmcp=info\" directive string is always valid"),
    )
}

/// Initializes logging infrastructure.
///
/// Sets up tracing with appropriate log levels based on verbosity flag.
/// Writes log messages to stderr, with any embedded URL's credentials/query string redacted (via
/// a wrapping [`Write`] adapter around the fmt layer's writer) — this covers `rmcp` and any other
/// dependency's log lines, not just this
/// project's own, since a dependency's `tracing` output cannot go through this crate's
/// `Debug`/[`escape_error_text`] redaction paths.
///
/// `log_format` selects text or JSON output. When `None` (the flag was not passed),
/// [`LogFormat::resolve`] consults the `MCP_EXECUTION_LOG_FORMAT` environment variable; an unset
/// or invalid environment value falls back to text with a `WARN`-level log line (never echoing
/// the rejected raw value — see the project's log-injection hardening for this switch).
///
/// Both branches are passed through `cap_rmcp_log_level`, which caps `rmcp`'s own `tracing`
/// targets at `info` regardless of the base filter -- see that function's doc comment for why.
///
/// # Arguments
///
/// * `verbose` - If true, sets log level to DEBUG; otherwise uses INFO or
///   environment variable override via `RUST_LOG`
/// * `log_format` - `--log-format` flag value, or `None` to consult
///   `MCP_EXECUTION_LOG_FORMAT`
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
/// runner::init_logging(false, None)?;
/// # Ok::<(), anyhow::Error>(())
/// ```
pub fn init_logging(verbose: bool, log_format: Option<LogFormat>) -> Result<()> {
    let filter = cap_rmcp_log_level(if verbose {
        EnvFilter::new("debug")
    } else {
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"))
    });

    let format = resolve_log_format(log_format);

    let fmt_layer = tracing_subscriber::fmt::layer().with_writer(|| RedactingWriter(io::stderr()));
    let layer = match format {
        LogFormat::Json => fmt_layer.json().boxed(),
        LogFormat::Text => fmt_layer.boxed(),
    };

    tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .init();

    // No raw value interpolation: the rejected value already comes from an external environment
    // variable, and echoing it into a log line — even truncated — would open a log-injection
    // vector for whoever controls the process environment.
    if log_format_env_is_invalid(log_format) {
        tracing::warn!(
            "invalid value for {LOG_FORMAT_ENV_VAR} (expected 'text' or 'json'), falling back to text"
        );
    }

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
    use mcp_execution_files::FilesResourceKind;

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
            resource: FilesResourceKind::ExportFileCount,
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

    /// Leak B regression: `CoreError::ConnectionFailed`'s boxed `source` is an opaque
    /// `Box<dyn Error + Send + Sync>` that, for an http/sse transport, is really `rmcp`'s wrapped
    /// `reqwest::Error` — whose `Display` embeds the full request URL, query string included. This
    /// simulates that exact shape (the security audit's captured `rmcp` output) without a live
    /// network connection, and asserts the secret never reaches the printed report.
    #[test]
    fn test_sanitized_error_report_redacts_connection_failed_source_url_secret() {
        let source: Box<dyn std::error::Error + Send + Sync> = concat!(
            "Client error: error sending request for url ",
            "(http://127.0.0.1:1/mcp?token=REFUSEDSECRET), when send initialize request"
        )
        .into();
        let err = wrap(CoreError::ConnectionFailed {
            server: "test".to_string(),
            source,
        });

        let report = sanitized_error_report(&err);
        assert!(!report.contains("REFUSEDSECRET"), "secret leaked: {report}");
        assert!(report.contains("http://127.0.0.1:1/mcp?<redacted>"));
        assert!(report.contains("MCP server connection failed: test"));
    }

    /// C2 regression at this leak's real entry point: an IPv6-literal authority must not defeat
    /// redaction here either. Mirrors the critic's live repro
    /// (`introspect --http "http://[::1]:1/mcp?token=..."`), which printed the secret in this
    /// exact report on the unfixed version.
    #[test]
    fn test_sanitized_error_report_redacts_connection_failed_source_ipv6_url_secret() {
        let source: Box<dyn std::error::Error + Send + Sync> = concat!(
            "Client error: error sending request for url ",
            "(http://[::1]:1/mcp?token=IPV6LEAKTEST), when send initialize request"
        )
        .into();
        let err = wrap(CoreError::ConnectionFailed {
            server: "test".to_string(),
            source,
        });

        let report = sanitized_error_report(&err);
        assert!(!report.contains("IPV6LEAKTEST"), "secret leaked: {report}");
        assert!(report.contains("http://[::1]:1/mcp?<redacted>"));
    }

    #[test]
    fn test_redacting_writer_redacts_url_secret_before_forwarding() {
        let mut sink = Vec::new();
        {
            let mut writer = RedactingWriter(&mut sink);
            let line = "ERROR rmcp::transport::worker: worker quit with fatal: Client error: error sending request for url (https://api.example.invalid/mcp?token=hunter2secret), when send initialize request\n";
            let n = writer.write(line.as_bytes()).unwrap();
            assert_eq!(n, line.len());
        }
        let written = String::from_utf8(sink).unwrap();
        assert!(!written.contains("hunter2secret"));
        assert!(written.contains("https://api.example.invalid/mcp?<redacted>"));
        assert!(written.contains("worker quit with fatal"));
    }

    #[test]
    fn test_redacting_writer_passes_through_text_without_urls() {
        let mut sink = Vec::new();
        RedactingWriter(&mut sink)
            .write_all(b"INFO some ordinary log line\n")
            .unwrap();
        assert_eq!(sink, b"INFO some ordinary log line\n");
    }

    /// Pins the assumption `RedactingWriter`'s doc comment relies on but the two tests above
    /// don't exercise: that `tracing-subscriber`'s fmt layer issues exactly one `write_all` per
    /// event, so `RedactingWriter::write` always sees a whole formatted line. Wires the real
    /// `fmt::layer()` (not a direct `RedactingWriter::write` call) through a scoped subscriber
    /// into a shared buffer, so a future `tracing-subscriber` upgrade that splits an event across
    /// multiple writes -- which would let a URL straddling the split leak unredacted -- fails this
    /// test instead of failing silently in production.
    #[test]
    fn test_redacting_writer_wired_into_real_fmt_layer_redacts_full_event() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct SharedBuf(Arc<Mutex<Vec<u8>>>);

        impl Write for SharedBuf {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().write(buf)
            }

            fn flush(&mut self) -> io::Result<()> {
                self.0.lock().unwrap().flush()
            }
        }

        let buf = Arc::new(Mutex::new(Vec::new()));
        let make_writer = {
            let buf = buf.clone();
            move || RedactingWriter(SharedBuf(buf.clone()))
        };

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .with_writer(make_writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(
                "error sending request for url (https://api.example.invalid/mcp?token=hunter2secret), when send initialize request"
            );
        });

        let written = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(
            !written.contains("hunter2secret"),
            "secret leaked: {written}"
        );
        assert!(written.contains("https://api.example.invalid/mcp?<redacted>"));
        assert!(written.contains("when send initialize request"));
    }

    /// JSON-mode counterpart to `test_redacting_writer_wired_into_real_fmt_layer_redacts_full_event`:
    /// wires `RedactingWriter` through a real `fmt::layer().json()` (the `.boxed()` branch
    /// `init_logging` takes for `LogFormat::Json`) and asserts (a) the secret never reaches the
    /// sink, (b) the redaction marker is present, and (c) every emitted line parses as JSON via
    /// `serde_json` -- the assertion that would have caught the C1 regression (an unescaped `"`
    /// left behind when a redacted URL sits inside a JSON-escaped string).
    #[test]
    fn test_redacting_writer_wired_into_real_json_layer_emits_valid_json() {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct SharedBuf(Arc<Mutex<Vec<u8>>>);

        impl Write for SharedBuf {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().write(buf)
            }

            fn flush(&mut self) -> io::Result<()> {
                self.0.lock().unwrap().flush()
            }
        }

        let buf = Arc::new(Mutex::new(Vec::new()));
        let make_writer = {
            let buf = buf.clone();
            move || RedactingWriter(SharedBuf(buf.clone()))
        };

        let subscriber = tracing_subscriber::registry().with(
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(make_writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::error!(
                "failed to connect to \"https://api.example.invalid/mcp?token=hunter2secret\" after 3 tries"
            );
        });

        let written = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        assert!(
            !written.contains("hunter2secret"),
            "secret leaked: {written}"
        );
        assert!(written.contains("<redacted>"));

        for line in written.lines().filter(|line| !line.is_empty()) {
            serde_json::from_str::<serde_json::Value>(line)
                .unwrap_or_else(|e| panic!("invalid JSON line: {e}\n{line}"));
        }
    }

    /// Shared harness for the `cap_rmcp_log_level` regression tests below: builds
    /// `cap_rmcp_log_level(EnvFilter::new(base_filter))`, wires it into a real `fmt::layer()`
    /// over a scoped subscriber (mirroring
    /// `test_redacting_writer_wired_into_real_fmt_layer_redacts_full_event` above), emits one
    /// `rmcp::transport::async_rw`-targeted `debug!` (standing in for `rmcp`'s own raw-peer-line
    /// logging) and one `mcp_execution_cli`-targeted `debug!` (standing in for this crate's own
    /// diagnostics), and returns `(rmcp_line_visible, own_line_visible)`. Does not mutate
    /// `RUST_LOG` (parallel test threads share one process) -- `base_filter` plays the same role
    /// a real `RUST_LOG` value would, but only ever reaches `EnvFilter::new` directly.
    fn rmcp_capped_filter_captures(base_filter: &str) -> (bool, bool) {
        use std::sync::{Arc, Mutex};

        #[derive(Clone)]
        struct SharedBuf(Arc<Mutex<Vec<u8>>>);

        impl Write for SharedBuf {
            fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
                self.0.lock().unwrap().write(buf)
            }

            fn flush(&mut self) -> io::Result<()> {
                self.0.lock().unwrap().flush()
            }
        }

        let buf = Arc::new(Mutex::new(Vec::new()));
        let make_writer = {
            let buf = buf.clone();
            move || SharedBuf(buf.clone())
        };

        let filter = cap_rmcp_log_level(EnvFilter::new(base_filter));
        let subscriber = tracing_subscriber::registry().with(filter).with(
            tracing_subscriber::fmt::layer()
                .with_writer(make_writer)
                .with_ansi(false),
        );

        tracing::subscriber::with_default(subscriber, || {
            tracing::debug!(
                target: "rmcp::transport::async_rw",
                "raw untrusted peer line"
            );
            tracing::debug!(target: "mcp_execution_cli", "own debug event");
        });

        let written = String::from_utf8(buf.lock().unwrap().clone()).unwrap();
        (
            written.contains("raw untrusted peer line"),
            written.contains("own debug event"),
        )
    }

    /// Regression coverage for issue #421: `--verbose`'s `EnvFilter::new("debug")` branch (no
    /// `RUST_LOG` involved) must not let `rmcp`'s own `tracing` targets -- which log raw,
    /// unsanitized peer input at `debug` -- through, while this crate's own `debug` events must
    /// still pass.
    #[test]
    fn cap_rmcp_log_level_suppresses_rmcp_debug_but_keeps_own_debug() {
        let (rmcp_visible, own_visible) = rmcp_capped_filter_captures("debug");
        assert!(
            !rmcp_visible,
            "rmcp debug line was not suppressed under a global `debug` base"
        );
        assert!(
            own_visible,
            "own debug event was unexpectedly suppressed under a global `debug` base"
        );
    }

    /// Regression coverage for critic finding S1: `tracing_subscriber`'s `Directive` ordering
    /// does not compare level, so `EnvFilter::add_directive` *replaces* a same-target directive
    /// rather than merging it. An operator's explicit `RUST_LOG=rmcp=debug` is therefore silently
    /// downgraded to this cap's own `rmcp=info`, not left to coexist at `debug` -- this was named
    /// by the original security audit as the ambiguous case to verify with a test rather than
    /// assume. The escape hatch for an operator who needs this is a *more specific* target (see
    /// the test below) -- documented in `cap_rmcp_log_level`'s doc comment and the CHANGELOG.
    #[test]
    fn cap_rmcp_log_level_replaces_a_same_target_rmcp_debug_directive() {
        let (rmcp_visible, _) = rmcp_capped_filter_captures("rmcp=debug");
        assert!(
            !rmcp_visible,
            "RUST_LOG=rmcp=debug was expected to be replaced by this cap's rmcp=info, not merged \
             with it -- if this now fails, `tracing_subscriber`'s directive-merge behavior \
             changed and `cap_rmcp_log_level`'s doc comment needs updating"
        );
    }

    /// Counterpart to the test above: a target *more specific* than `rmcp` (e.g.
    /// `RUST_LOG=rmcp::transport=debug`) is not overwritten by this cap's `rmcp=info` --
    /// `tracing_subscriber` orders directives by target specificity, and a longer target wins.
    /// This is the documented escape hatch for an operator who deliberately wants rmcp transport
    /// debug output.
    #[test]
    fn cap_rmcp_log_level_does_not_override_a_more_specific_rmcp_target() {
        let (rmcp_visible, _) = rmcp_capped_filter_captures("rmcp::transport=debug");
        assert!(
            rmcp_visible,
            "RUST_LOG=rmcp::transport=debug should still surface rmcp debug output -- a more \
             specific target must win over this cap's rmcp=info"
        );
    }

    /// Serializes tests in this module that mutate `MCP_EXECUTION_LOG_FORMAT`, mirroring
    /// `BACKTRACE_ENV_LOCK` above and `mcp-execution-server`'s own `LOG_FORMAT_ENV_LOCK`: a
    /// safety net for plain `cargo test` (which shares one process across a crate's tests), not
    /// required by the mandated `cargo nextest run` (which isolates every test in its own
    /// process).
    static LOG_FORMAT_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// Proves `resolve_log_format` -- the function `init_logging` actually calls -- reads the
    /// real `MCP_EXECUTION_LOG_FORMAT` environment variable itself, not just that the pure
    /// `LogFormat::resolve` it delegates to works given a hand-built `Option<&str>`. A version of
    /// `resolve_log_format` that dropped the `std::env::var` call entirely would still pass every
    /// other test in this module.
    #[test]
    fn resolve_log_format_reads_env_var_when_flag_unset() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "json");
        }

        let format = resolve_log_format(None);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert_eq!(format, LogFormat::Json);
    }

    #[test]
    fn resolve_log_format_flag_wins_over_bad_env() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "xml");
        }

        let format = resolve_log_format(Some(LogFormat::Json));
        let is_invalid = log_format_env_is_invalid(Some(LogFormat::Json));

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert_eq!(format, LogFormat::Json);
        assert!(
            !is_invalid,
            "a bad env value must not be reported once the flag has already decided"
        );
    }

    #[test]
    fn resolve_log_format_bad_env_value_falls_back_to_text() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "xml");
        }

        let format = resolve_log_format(None);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert_eq!(format, LogFormat::Text);
    }

    /// Proves `log_format_env_is_invalid` -- the function that actually gates `init_logging`'s
    /// warning -- reads the real environment variable itself, not just that
    /// `LogFormat::is_invalid_env_value` works given a hand-built `&str`.
    #[test]
    fn log_format_env_is_invalid_true_for_bad_value_when_flag_unset() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "xml");
        }

        let is_invalid = log_format_env_is_invalid(None);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert!(is_invalid);
    }

    #[test]
    fn log_format_env_is_invalid_false_for_valid_value() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::set_var(LOG_FORMAT_ENV_VAR, "json");
        }

        let is_invalid = log_format_env_is_invalid(None);

        // SAFETY: see above.
        unsafe {
            match &original {
                Some(v) => std::env::set_var(LOG_FORMAT_ENV_VAR, v),
                None => std::env::remove_var(LOG_FORMAT_ENV_VAR),
            }
        }

        assert!(!is_invalid);
    }

    #[test]
    fn log_format_env_is_invalid_false_when_unset() {
        let _guard = LOG_FORMAT_ENV_LOCK.lock().unwrap();
        let original = std::env::var_os(LOG_FORMAT_ENV_VAR);
        // SAFETY: guarded by `LOG_FORMAT_ENV_LOCK`; no other test in this process reads or
        // writes `MCP_EXECUTION_LOG_FORMAT` while the guard is held.
        unsafe {
            std::env::remove_var(LOG_FORMAT_ENV_VAR);
        }

        let is_invalid = log_format_env_is_invalid(None);

        // SAFETY: see above.
        unsafe {
            if let Some(v) = &original {
                std::env::set_var(LOG_FORMAT_ENV_VAR, v);
            }
        }

        assert!(!is_invalid);
    }
}
