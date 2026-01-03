//! Command execution and runtime logic.
//!
//! Contains the main command execution loop and logging initialization.

use anyhow::Result;
use mcp_core::cli::{ExitCode, OutputFormat};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use crate::cli::Commands;
use crate::commands;

/// Initializes logging infrastructure.
///
/// Sets up tracing with appropriate log levels based on verbosity flag.
///
/// # Errors
///
/// Returns an error if logging initialization fails.
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
/// Routes commands to their respective handlers and returns an exit code.
///
/// # Errors
///
/// Returns an error if command execution fails.
pub async fn execute_command(command: Commands, output_format: OutputFormat) -> Result<ExitCode> {
    match command {
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
                output_format,
            )
            .await
        }
        Commands::Server { action } => commands::server::run(action, output_format).await,
        Commands::Setup => commands::setup::run().await,
        Commands::Completions { shell } => {
            use crate::cli::Cli;
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            commands::completions::run(shell, &mut cmd).await
        }
    }
}
