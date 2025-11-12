//! MCP Code Execution CLI.
//!
//! Command-line interface for executing code in MCP sandbox,
//! inspecting servers, and generating virtual filesystems.

use anyhow::Result;

fn main() -> Result<()> {
    println!("MCP Code Execution CLI");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    Ok(())
}
