//! Action type definitions for CLI commands.
//!
//! Defines the action enums used by various commands.

use clap::Subcommand;

/// Server management actions.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::actions::ServerAction;
/// use clap::Subcommand;
///
/// let list_action = ServerAction::List;
/// // Can be used with clap for CLI parsing
/// ```
#[derive(Subcommand, Debug)]
pub enum ServerAction {
    /// List all configured servers
    List,

    /// Show detailed information about a server
    Info {
        /// Server name
        server: String,
    },

    /// Validate a server command
    Validate {
        /// Server command to validate
        command: String,
    },
}
