//! Shell completion generation command.
//!
//! Generates shell completion scripts for bash, zsh, fish, and `PowerShell`.

use anyhow::Result;
use clap::Command;
use clap_complete::{Shell, generate};
use mcp_execution_core::cli::ExitCode;
use std::io;
use tracing::info;

/// Generates shell completion script for the specified shell.
///
/// Prints the completion script to stdout, which can be sourced or saved
/// to the appropriate location for the shell.
///
/// # Arguments
///
/// * `shell` - Target shell (bash, zsh, fish, powershell, elvish)
/// * `cmd` - Command to generate completions for
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::completions;
/// use clap_complete::Shell;
/// use clap::Command;
///
/// let cmd = Command::new("mcp-cli");
/// completions::generate_completions(Shell::Bash, &mut cmd.clone());
/// ```
pub fn generate_completions(shell: Shell, cmd: &mut Command) {
    info!("Generating {} completions", shell);
    generate(shell, cmd, cmd.get_name().to_string(), &mut io::stdout());
}

/// Runs the completions command.
///
/// # Arguments
///
/// * `shell` - Target shell to generate completions for
/// * `cmd` - CLI command structure for generating completions
///
/// # Returns
///
/// Returns `Ok(ExitCode::SUCCESS)` on successful generation.
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::completions;
/// use clap::{Command, CommandFactory};
/// use clap_complete::Shell;
///
/// # #[tokio::main]
/// # async fn main() {
/// let mut cmd = Command::new("test");
/// let result = completions::run(Shell::Bash, &mut cmd).await;
/// assert!(result.is_ok());
/// # }
/// ```
pub async fn run(shell: Shell, cmd: &mut Command) -> Result<ExitCode> {
    info!("Completions command for shell: {shell}");
    generate_completions(shell, cmd);
    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Command;

    #[test]
    fn test_generate_completions_bash() {
        let mut cmd = Command::new("test-cli");
        // This should not panic
        generate_completions(Shell::Bash, &mut cmd);
    }

    #[test]
    fn test_generate_completions_zsh() {
        let mut cmd = Command::new("test-cli");
        generate_completions(Shell::Zsh, &mut cmd);
    }

    #[test]
    fn test_generate_completions_fish() {
        let mut cmd = Command::new("test-cli");
        generate_completions(Shell::Fish, &mut cmd);
    }

    #[test]
    fn test_generate_completions_powershell() {
        let mut cmd = Command::new("test-cli");
        generate_completions(Shell::PowerShell, &mut cmd);
    }

    #[tokio::test]
    async fn test_run_bash() {
        let mut cmd = Command::new("test-cli");
        let result = run(Shell::Bash, &mut cmd).await;
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }

    #[tokio::test]
    async fn test_run_zsh() {
        let mut cmd = Command::new("test-cli");
        let result = run(Shell::Zsh, &mut cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_fish() {
        let mut cmd = Command::new("test-cli");
        let result = run(Shell::Fish, &mut cmd).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_run_powershell() {
        let mut cmd = Command::new("test-cli");
        let result = run(Shell::PowerShell, &mut cmd).await;
        assert!(result.is_ok());
    }
}
