//! Script runtime: everything the `run`, `call` and hidden `helper` commands share.
//!
//! The runner process is also the helper: it serves tool calls to a script over a per-run
//! channel, so a script reaches MCP servers through the same single client the rest of the
//! workspace uses. Modules are split by responsibility:
//!
//! - [`dirs`], [`budget`], [`spill`]: per-OS state directories, output budgets and bounded spill
//!   files.
//! - [`node`]: Node.js detection shared with `setup`.
//! - [`exec`], [`notice`], [`import_hook`], [`typecheck`], [`restrict`]: running the script
//!   process.
//! - [`signal`], `tty`: the runner's own stop signals and the terminal's foreground group.
//! - [`protocol`], [`helper`], [`pool`], [`journal`], [`policy`]: the helper side of the
//!   protocol described in [`mcp_execution_core::helper_protocol`].

use std::fmt;

pub mod budget;
pub mod dirs;
pub mod exec;
pub mod helper;
pub mod import_hook;
pub mod journal;
pub mod node;
pub mod notice;
pub mod path_lookup;
pub mod policy;
pub mod pool;
pub mod protocol;
pub mod restrict;
pub mod signal;
pub mod spill;
pub(crate) mod tty;
pub mod typecheck;

/// A value rejected by a bounded runtime newtype.
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::runtime::budget::OutputBudget;
///
/// let err = "0".parse::<OutputBudget>().unwrap_err();
/// assert!(err.to_string().contains("output budget"));
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutOfRange {
    what: &'static str,
    min: u64,
    max: u64,
}

impl OutOfRange {
    /// Describes a bounded quantity `what` that must lie in `min..=max`.
    pub(crate) const fn new(what: &'static str, min: u64, max: u64) -> Self {
        Self { what, min, max }
    }
}

impl fmt::Display for OutOfRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} must be an integer between {} and {}",
            self.what, self.min, self.max
        )
    }
}

impl std::error::Error for OutOfRange {}
