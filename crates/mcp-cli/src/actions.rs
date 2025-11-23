//! Action type definitions for CLI commands.
//!
//! Defines the action enums used by various commands.

use clap::Subcommand;

/// Server management actions.
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

/// Debug actions.
#[derive(Subcommand, Debug, Clone)]
pub enum DebugAction {
    /// Inspect Bridge cache state (size, hit rate, entries)
    Cache,

    /// Inspect Runtime module cache (compiled WASM modules)
    Modules,

    /// Inspect active MCP server connections
    Connections,

    /// Show system diagnostics (versions, paths, permissions)
    System,
}

/// Configuration actions.
#[derive(Subcommand, Debug)]
pub enum ConfigAction {
    /// Initialize configuration file
    Init,

    /// Show current configuration
    Show,

    /// Set a configuration value
    Set {
        /// Configuration key
        key: String,
        /// Configuration value
        value: String,
    },

    /// Get a configuration value
    Get {
        /// Configuration key
        key: String,
    },
}
