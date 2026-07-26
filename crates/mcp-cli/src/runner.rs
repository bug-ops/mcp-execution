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
    let result = match command {
        Commands::Introspect {
            from_config,
            server,
            args,
            env,
            cwd,
            http,
            sse,
            headers,
            detailed,
            connect_timeout_secs,
            discover_timeout_secs,
        } => {
            commands::introspect::run(
                from_config,
                server,
                args,
                env,
                cwd,
                http,
                sse,
                headers,
                detailed,
                connect_timeout_secs,
                discover_timeout_secs,
                output_format,
            )
            .await
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
            from_config,
            server,
            server_args,
            server_env,
            server_cwd,
            http_url,
            sse_url,
            server_headers,
            name,
            progressive_output,
            dry_run,
            connect_timeout_secs,
            discover_timeout_secs,
        } => {
            commands::generate::run(
                from_config,
                server,
                server_args,
                server_env,
                server_cwd,
                http_url,
                sse_url,
                server_headers,
                name,
                progressive_output,
                dry_run,
                connect_timeout_secs,
                discover_timeout_secs,
                output_format,
            )
            .await
        }
        Commands::Server { action } => commands::server::run(action, output_format).await,
        Commands::Setup => commands::setup::run(output_format).await,
        Commands::Completions { shell } => {
            use crate::cli::Cli;
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            commands::completions::run(shell, &mut cmd).await
        }
    };

    Ok(match result {
        Ok(code) => code,
        Err(err) => report_and_classify(&err),
    })
}

/// Prints `err` to stderr (matching anyhow's default `main`-error format) and
/// classifies it into a semantic [`ExitCode`] via `classify_exit_code`.
///
/// Shared by [`execute_command`] (command-handler failures) and `main`
/// (pre-dispatch failures, e.g. an invalid `--format` value), so every
/// failure this CLI can produce is reported and exits the same way.
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
    eprintln!("Error: {err:?}");
    classify_exit_code(err)
}

/// Classifies an [`anyhow::Error`] returned by a command handler into a
/// semantic [`ExitCode`].
///
/// Walks the error's cause chain looking for a [`CoreError`] — the concrete
/// type every command handler ultimately produces via `?` — and maps its
/// variant to the exit code that best communicates the failure category to
/// scripts consuming this CLI. Falls back to checking for a [`FilesError`]
/// (the `generate` command's `export_to_filesystem` errors are wrapped via `anyhow::Context`
/// rather than converted to `CoreError`, so they would otherwise never match the first check
/// and always fall through to the generic [`ExitCode::ERROR`] — issue #198 M6). Errors that
/// match neither (e.g. CLI argument parsing, serialization) fall back to [`ExitCode::ERROR`].
fn classify_exit_code(error: &anyhow::Error) -> ExitCode {
    if let Some(core_error) = error
        .chain()
        .find_map(|cause| cause.downcast_ref::<CoreError>())
    {
        return match core_error {
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
            CoreError::SerializationError { .. } | CoreError::ScriptGenerationError { .. } => {
                ExitCode::ERROR
            }
        };
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
            | FilesError::IoError { .. } => ExitCode::ERROR,
        };
    }

    ExitCode::ERROR
}

#[cfg(test)]
mod tests {
    use super::*;

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
            resource: "tool count".to_string(),
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
        let result = execute_command(
            Commands::Introspect {
                from_config: None,
                server: Some("nonexistent-server-for-exit-code-test".to_string()),
                args: vec![],
                env: vec![],
                cwd: None,
                http: None,
                sse: None,
                headers: vec![],
                detailed: false,
                connect_timeout_secs: None,
                discover_timeout_secs: None,
            },
            OutputFormat::Json,
        )
        .await;

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
}
