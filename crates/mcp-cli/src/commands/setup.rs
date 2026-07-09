//! Setup command implementation.
//!
//! Validates the runtime environment for MCP tool execution:
//! - Checks Node.js 18+ is installed
//! - Verifies generated files are executable
//! - Provides helpful error messages and suggestions

use anyhow::{Context, Result};
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use serde::Serialize;
use std::path::PathBuf;
use std::process::Stdio;
use tokio::process::Command;

/// Structured result of the environment setup checks.
///
/// Captures every check [`run`] performs so it can be rendered as JSON,
/// plain text, or the default human-readable pretty summary via
/// [`crate::formatters::format_output`].
///
/// # Examples
///
/// ```
/// use mcp_execution_cli::commands::setup::SetupResult;
///
/// let result = SetupResult {
///     node_version: "20.10.0".to_string(),
///     mcp_config_path: "/home/user/.claude/mcp.json".to_string(),
///     mcp_config_found: true,
///     servers_dir_found: true,
///     files_made_executable: 3,
/// };
///
/// assert_eq!(result.files_made_executable, 3);
/// ```
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct SetupResult {
    /// Detected Node.js version (e.g. `"20.10.0"`), without the leading `v`.
    pub node_version: String,
    /// Path where `~/.claude/mcp.json` is expected.
    pub mcp_config_path: String,
    /// Whether `~/.claude/mcp.json` exists.
    pub mcp_config_found: bool,
    /// Whether `~/.claude/servers/` exists. Always `false` on non-Unix
    /// platforms, since file permissions are not checked there.
    pub servers_dir_found: bool,
    /// Number of `.ts` files made executable under `~/.claude/servers/`.
    /// Always `0` on non-Unix platforms.
    pub files_made_executable: usize,
}

/// Runs the setup command.
///
/// Validates that the runtime environment is ready for MCP tool execution
/// and renders the results according to `output_format`.
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
/// # Run setup validation (default pretty output)
/// mcp-execution-cli setup
///
/// # Output:
/// # ✓ Node.js v20.10.0 detected
/// # ✓ Runtime setup complete
/// # Claude Code can now execute MCP tools via:
/// #   node ~/.claude/servers/<server>/<tool>.ts '{"param":"value"}'
///
/// # Structured output for scripting
/// mcp-execution-cli --format json setup
/// ```
///
/// # Errors
///
/// Returns an error if:
/// - Node.js is not installed
/// - Node.js version is less than 18.0.0
/// - Home directory cannot be determined
/// - Output formatting fails (serialization error)
pub async fn run(output_format: OutputFormat) -> Result<ExitCode> {
    if output_format == OutputFormat::Pretty {
        println!("Checking runtime environment...\n");
    }

    let node_version = check_node_version().await?;

    let mcp_config_path = get_mcp_config_path()?;
    let mcp_config_found = mcp_config_path.exists();

    let (servers_dir_found, files_made_executable) = check_files_executable().await?;

    let result = SetupResult {
        node_version,
        mcp_config_path: mcp_config_path.display().to_string(),
        mcp_config_found,
        servers_dir_found,
        files_made_executable,
    };

    if output_format == OutputFormat::Pretty {
        print_pretty_summary(&result);
    } else {
        let formatted = crate::formatters::format_output(&result, output_format)?;
        println!("{formatted}");
    }

    Ok(ExitCode::SUCCESS)
}

/// Prints the human-readable setup summary (the `Pretty` format rendering).
fn print_pretty_summary(result: &SetupResult) {
    println!("✓ Node.js v{} detected", result.node_version);

    if result.mcp_config_found {
        println!("✓ MCP configuration found: {}", result.mcp_config_path);
    } else {
        println!("⚠ MCP configuration not found");
        println!("  Expected location: {}", result.mcp_config_path);
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

    #[cfg(unix)]
    {
        if result.servers_dir_found {
            if result.files_made_executable > 0 {
                println!(
                    "✓ Made {} TypeScript files executable",
                    result.files_made_executable
                );
            }
        } else {
            println!("⚠ No servers directory found");
            println!("  Run 'mcp-execution-cli generate <server>' to create tools");
        }
    }

    println!("\n✓ Runtime setup complete");
    println!("  Claude Code can now execute MCP tools via:");
    println!("  node ~/.claude/servers/<server>/<tool>.ts '{{\"param\":\"value\"}}'");
    println!("\nNext steps:");
    println!("  1. Generate tools: mcp-execution-cli generate <server>");
    println!("  2. Configure servers in ~/.claude/mcp.json");
    println!("  3. Execute tools autonomously via Node.js");
}

/// Checks Node.js version requirement.
///
/// Verifies that Node.js 18.0.0 or higher is installed and accessible, and
/// returns the detected version string (without the leading `v`).
///
/// # Errors
///
/// Returns error if:
/// - Node.js command not found in PATH
/// - Node.js version cannot be determined
/// - Node.js version is less than 18.0.0
async fn check_node_version() -> Result<String> {
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

    Ok(version_str.to_string())
}

/// Checks for and makes TypeScript files executable (Unix only).
///
/// Sets executable permissions (0755) on all .ts files in ~/.claude/servers/
/// This allows files to be executed with shebang: `./tool.ts`
///
/// # Platform Support
///
/// - Unix/Linux/macOS: Sets permissions, returns `(servers_dir_found, files_made_executable)`
/// - Windows: No-op, always returns `(false, 0)`
///
/// # Errors
///
/// Returns error if:
/// - Home directory cannot be determined
/// - Permission changes fail
#[cfg(unix)]
async fn check_files_executable() -> Result<(bool, usize)> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::fs;

    let servers_dir = get_servers_dir()?;

    // Check if servers directory exists
    if !servers_dir.exists() {
        return Ok((false, 0));
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

    Ok((true, count))
}

/// Checks for and makes TypeScript files executable (Unix only).
///
/// No-op on non-Unix platforms, since file permissions are not checked there.
#[cfg(not(unix))]
async fn check_files_executable() -> Result<(bool, usize)> {
    Ok((false, 0))
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

    #[tokio::test]
    async fn test_check_files_executable_no_panic() {
        // Should not panic regardless of whether ~/.claude/servers exists.
        let result = check_files_executable().await;
        assert!(result.is_ok());
    }

    #[test]
    fn test_setup_result_serialization() {
        let result = SetupResult {
            node_version: "20.10.0".to_string(),
            mcp_config_path: "/home/user/.claude/mcp.json".to_string(),
            mcp_config_found: true,
            servers_dir_found: true,
            files_made_executable: 3,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"node_version\":\"20.10.0\""));
        assert!(json.contains("\"mcp_config_found\":true"));
        assert!(json.contains("\"files_made_executable\":3"));
    }

    #[test]
    fn test_setup_result_format_output_json() {
        let result = SetupResult {
            node_version: "20.10.0".to_string(),
            mcp_config_path: "/home/user/.claude/mcp.json".to_string(),
            mcp_config_found: false,
            servers_dir_found: true,
            files_made_executable: 7,
        };

        let formatted =
            crate::formatters::format_output(&result, mcp_execution_core::cli::OutputFormat::Json)
                .unwrap();
        assert!(formatted.contains("\"node_version\": \"20.10.0\""));
        assert!(formatted.contains("\"mcp_config_path\": \"/home/user/.claude/mcp.json\""));
        assert!(formatted.contains("\"mcp_config_found\": false"));
        assert!(formatted.contains("\"servers_dir_found\": true"));
        assert!(formatted.contains("\"files_made_executable\": 7"));
    }

    #[test]
    fn test_setup_result_format_output_text() {
        let result = SetupResult {
            node_version: "20.10.0".to_string(),
            mcp_config_path: "/home/user/.claude/mcp.json".to_string(),
            mcp_config_found: true,
            servers_dir_found: false,
            files_made_executable: 0,
        };

        let formatted =
            crate::formatters::format_output(&result, mcp_execution_core::cli::OutputFormat::Text)
                .unwrap();
        // Text format is compact JSON (no newlines), unlike the pretty-printed
        // Json format checked above.
        assert!(!formatted.contains('\n'));
        assert!(formatted.contains("\"node_version\":\"20.10.0\""));
        assert!(formatted.contains("\"mcp_config_found\":true"));
        assert!(formatted.contains("\"servers_dir_found\":false"));
        assert!(formatted.contains("\"files_made_executable\":0"));
    }
}
