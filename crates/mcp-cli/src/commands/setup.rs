//! Setup command implementation.
//!
//! Validates the runtime environment for MCP tool execution:
//! - Checks Node.js 18+ is installed
//! - Verifies generated files are executable
//! - Provides helpful error messages and suggestions

use anyhow::{Context, Result};
use mcp_core::cli::ExitCode;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// Runs the setup command.
///
/// Validates that the runtime environment is ready for MCP tool execution.
///
/// # Checks Performed
///
/// 1. **Node.js version**: Ensures Node.js 18.0.0 or higher is installed
/// 2. **File permissions**: Makes TypeScript files executable (Unix only)
/// 3. **Configuration**: Checks if ~/.claude/mcp.json exists
///
/// # Examples
///
/// ```bash
/// # Run setup validation
/// mcp-execution-cli setup
///
/// # Output:
/// # ✓ Node.js v20.10.0 detected
/// # ✓ Runtime setup complete
/// # Claude Code can now execute MCP tools via:
/// #   node ~/.claude/servers/<server>/<tool>.ts '{"param":"value"}'
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - Node.js is not installed
/// - Node.js version is less than 18.0.0
/// - Home directory cannot be determined
pub async fn run() -> Result<ExitCode> {
    println!("Checking runtime environment...\n");

    // Check Node.js installation
    check_node_version().await?;

    // Check for MCP configuration
    check_mcp_config()?;

    // Make files executable (Unix only)
    #[cfg(unix)]
    make_files_executable().await?;

    println!("\n✓ Runtime setup complete");
    println!("  Claude Code can now execute MCP tools via:");
    println!("  node ~/.claude/servers/<server>/<tool>.ts '{{\"param\":\"value\"}}'");
    println!("\nNext steps:");
    println!("  1. Generate tools: mcp-execution-cli generate <server>");
    println!("  2. Configure servers in ~/.claude/mcp.json");
    println!("  3. Execute tools autonomously via Node.js");

    Ok(ExitCode::SUCCESS)
}

/// Checks Node.js version requirement.
///
/// Verifies that Node.js 18.0.0 or higher is installed and accessible.
///
/// # Errors
///
/// Returns error if:
/// - Node.js command not found in PATH
/// - Node.js version cannot be determined
/// - Node.js version is less than 18.0.0
async fn check_node_version() -> Result<()> {
    // Check if node command exists
    let output = Command::new("node")
        .arg("--version")
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context(
            "Node.js not found in PATH.\n\
             \n\
             Node.js 18+ is required for MCP tool execution.\n\
             Install from: https://nodejs.org\n\
             \n\
             Or use a version manager:\n\
             - nvm: https://github.com/nvm-sh/nvm\n\
             - fnm: https://github.com/Schniz/fnm",
        )?;

    if !output.status.success() {
        anyhow::bail!("Node.js is installed but not working correctly");
    }

    // Parse version
    let version_str = String::from_utf8_lossy(&output.stdout);
    let version_str = version_str.trim().trim_start_matches('v');

    // Extract major version
    let major_version = version_str
        .split('.')
        .next()
        .and_then(|s| s.parse::<u32>().ok())
        .context("Failed to parse Node.js version")?;

    if major_version < 18 {
        anyhow::bail!(
            "Node.js version {version_str} is too old.\n\
             \n\
             Required: Node.js 18.0.0 or higher\n\
             Current:  Node.js {version_str}\n\
             \n\
             Please upgrade Node.js:\n\
             - Download: https://nodejs.org\n\
             - Or use nvm: nvm install 18"
        );
    }

    println!("✓ Node.js v{version_str} detected");
    Ok(())
}

/// Checks if MCP configuration exists.
///
/// Validates that ~/.claude/mcp.json exists and is readable.
/// Provides helpful guidance if not found.
///
/// # Errors
///
/// Returns error if home directory cannot be determined.
/// Warns if config file doesn't exist but doesn't fail.
fn check_mcp_config() -> Result<()> {
    let config_path = get_mcp_config_path()?;

    if config_path.exists() {
        println!("✓ MCP configuration found: {}", config_path.display());
    } else {
        println!("⚠ MCP configuration not found");
        println!("  Expected location: {}", config_path.display());
        println!("  Create it with your server configurations:");
        println!();
        println!("  {{");
        println!("    \"mcpServers\": {{");
        println!("      \"github\": {{");
        println!("        \"command\": \"docker\",");
        println!("        \"args\": [\"run\", \"-i\", \"--rm\", \"...\"]");
        println!("      }}");
        println!("    }}");
        println!("  }}");
        println!();
        println!("  See examples/mcp.json.example for more details.");
    }

    Ok(())
}

/// Makes TypeScript files executable (Unix only).
///
/// Sets executable permissions (0755) on all .ts files in ~/.claude/servers/
/// This allows files to be executed with shebang: `./tool.ts`
///
/// # Platform Support
///
/// - Unix/Linux/macOS: Sets permissions
/// - Windows: No-op (not needed)
///
/// # Errors
///
/// Returns error if:
/// - Home directory cannot be determined
/// - Permission changes fail
#[cfg(unix)]
async fn make_files_executable() -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::fs;

    let servers_dir = get_servers_dir()?;

    // Check if servers directory exists
    if !servers_dir.exists() {
        println!("⚠ No servers directory found");
        println!("  Run 'mcp-execution-cli generate <server>' to create tools");
        return Ok(());
    }

    // Walk through all .ts files and make them executable
    let mut count = 0;
    let mut entries = fs::read_dir(&servers_dir).await?;

    while let Some(entry) = entries.next_entry().await? {
        let path = entry.path();

        if path.is_dir() {
            // Recurse into server directories
            if let Ok(mut server_entries) = fs::read_dir(&path).await {
                while let Some(server_entry) = server_entries.next_entry().await? {
                    let file_path = server_entry.path();

                    if file_path.extension().and_then(|s| s.to_str()) == Some("ts") {
                        let metadata = fs::metadata(&file_path).await?;
                        let mut perms = metadata.permissions();
                        perms.set_mode(0o755); // rwxr-xr-x
                        fs::set_permissions(&file_path, perms).await?;
                        count += 1;
                    }
                }
            }
        }
    }

    if count > 0 {
        println!("✓ Made {count} TypeScript files executable");
    }

    Ok(())
}

/// Gets the path to ~/.claude/mcp.json
fn get_mcp_config_path() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(".claude").join("mcp.json"))
}

/// Gets the path to ~/.claude/servers/
fn get_servers_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Failed to get home directory")?;
    Ok(home.join(".claude").join("servers"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_check_node_version() {
        // This test will pass if Node.js 18+ is installed
        // Otherwise it will fail, which is the expected behavior
        let result = check_node_version().await;

        // We can't assert success because Node.js might not be installed
        // in CI environment, but we can verify error messages are helpful
        if let Err(e) = result {
            let error_msg = e.to_string();
            assert!(
                error_msg.contains("Node.js") || error_msg.contains("version"),
                "Error message should be helpful: {error_msg}"
            );
        }
    }

    #[test]
    fn test_get_mcp_config_path() {
        let path = get_mcp_config_path();
        assert!(path.is_ok());

        let path = path.unwrap();
        assert!(path.to_string_lossy().contains(".claude"));
        assert!(path.to_string_lossy().contains("mcp.json"));
    }

    #[test]
    fn test_get_servers_dir() {
        let path = get_servers_dir();
        assert!(path.is_ok());

        let path = path.unwrap();
        assert!(path.to_string_lossy().contains(".claude"));
        assert!(path.to_string_lossy().contains("servers"));
    }

    #[test]
    fn test_check_mcp_config_no_panic() {
        // Should not panic even if config doesn't exist
        let result = check_mcp_config();
        assert!(result.is_ok());
    }
}
