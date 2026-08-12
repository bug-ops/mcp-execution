//! Setup command implementation.
//!
//! Validates the runtime environment for MCP tool execution:
//! - Checks Node.js 18+ is installed
//! - Verifies generated files are executable
//! - Provides helpful error messages and suggestions

use anyhow::{Context, Result};
use mcp_execution_core::cli::{ExitCode, OutputFormat};
#[cfg(unix)]
use mcp_execution_core::sanitize_path_for_error;
use serde::Serialize;
#[cfg(unix)]
use std::path::Path;
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
///     skipped_entries: 0,
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
    /// Number of symlinked entries skipped while walking
    /// `~/.claude/servers/` — any symlinked entry (a server-id directory, an
    /// intermediate subdirectory, or a file) encountered at any depth.
    /// Always `0` on non-Unix platforms.
    pub skipped_entries: usize,
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

    let (servers_dir_found, files_made_executable, skipped_entries) =
        check_files_executable().await?;

    let result = SetupResult {
        node_version,
        mcp_config_path: mcp_config_path.display().to_string(),
        mcp_config_found,
        servers_dir_found,
        files_made_executable,
        skipped_entries,
    };

    if output_format == OutputFormat::Pretty {
        print_pretty_summary(&result);
        return Ok(ExitCode::SUCCESS);
    }

    crate::formatters::emit(&result, output_format, ExitCode::SUCCESS)
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
            if result.skipped_entries > 0 {
                println!(
                    "⚠ Skipped {} symlinked entr{} under the servers directory (see warnings above)",
                    result.skipped_entries,
                    if result.skipped_entries == 1 {
                        "y"
                    } else {
                        "ies"
                    }
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
/// - Unix/Linux/macOS: Sets permissions, returns
///   `(servers_dir_found, files_made_executable, skipped_entries)`
/// - Windows: No-op, always returns `(false, 0, 0)`
///
/// # Errors
///
/// Returns error if:
/// - Home directory cannot be determined
/// - Permission changes fail
#[cfg(unix)]
async fn check_files_executable() -> Result<(bool, usize, usize)> {
    let servers_dir = get_servers_dir()?;
    check_files_executable_in(&servers_dir).await
}

/// Checks for and makes TypeScript files executable (Unix only).
///
/// No-op on non-Unix platforms, since file permissions are not checked there.
#[cfg(not(unix))]
async fn check_files_executable() -> Result<(bool, usize, usize)> {
    Ok((false, 0, 0))
}

/// Recursively walks `servers_dir` and makes every `.ts` file executable
/// (0755) at any depth, rejecting symlinked entries at every recursion level
/// rather than following them.
///
/// A symlinked entry — a server-id directory, an intermediate subdirectory,
/// or a `.ts` file, at any depth — is skipped and counted in
/// `skipped_entries` rather than chmod'd or descended into, since following
/// one would let a planted symlink redirect a permission change, or a
/// directory descent, to anywhere the process can reach outside
/// `servers_dir`. Entry kind is checked with
/// [`std::fs::DirEntry::file_type`] (via its `tokio` equivalent), which —
/// like `symlink_metadata` — does not traverse symlinks, so the check itself
/// cannot be tricked into following the entry it's inspecting. Because a
/// symlinked directory is therefore never descended into, the walk cannot
/// cycle back to an ancestor through a symlink; ordinary (non-symlink)
/// directories cannot form cycles on their own.
///
/// This is a check against pre-existing state, not a concurrency guarantee:
/// it does not defend against a symlink planted by a racing process between
/// this function's kind check and the subsequent `set_permissions` call (see
/// `mcp_execution_core::confinement`'s equivalent TOCTOU note). A hardlink
/// inside `servers_dir` pointing at a file outside it is also indistinguishable
/// from a regular file by `file_type` and is not defended against; both are
/// accepted as out of scope.
///
/// # Errors
///
/// Returns an error if `servers_dir` cannot be canonicalized, the root
/// directory itself cannot be opened for reading, an entry's file type
/// cannot be determined, or a `.ts` file's permissions cannot be read or
/// changed. A root-open failure is fatal (there are no sibling directories
/// to protect at the root, so tolerating it would only hide a real error).
/// Below the root, a directory that fails to open, or a read error
/// encountered mid-iteration over any directory's entries (root included),
/// is not propagated: it is logged as a warning and the walk moves on (to
/// the next sibling entry, or simply stops if the affected directory is the
/// walk's root) rather than aborting the whole `setup` run.
#[cfg(unix)]
async fn check_files_executable_in(servers_dir: &Path) -> Result<(bool, usize, usize)> {
    use tokio::fs;

    // Check if servers directory exists
    if !servers_dir.exists() {
        return Ok((false, 0, 0));
    }

    let root = fs::canonicalize(servers_dir).await?;
    // The root's own read_dir failure is fatal: unlike a nested server-id
    // directory, there are no siblings at the root to protect by tolerating
    // it, so propagating is the only way the caller learns setup didn't run.
    let entries = fs::read_dir(&root).await?;

    let mut count = 0;
    let mut skipped = 0;
    walk_entries(&root, entries, &mut count, &mut skipped).await?;

    Ok((true, count, skipped))
}

/// Recursion step for [`check_files_executable_in`]: opens `dir` — skipping
/// it with a warning rather than propagating if it cannot be opened, since
/// (unlike the walk's root) it always has siblings whose processing should
/// continue — then delegates to [`walk_entries`] for the shared per-entry
/// logic.
#[cfg(unix)]
async fn walk_and_chmod(dir: &Path, count: &mut usize, skipped: &mut usize) -> Result<()> {
    use tokio::fs;

    let entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(
                path = %sanitize_path_for_error(dir),
                %error,
                "skipping unreadable directory under the servers directory"
            );
            return Ok(());
        }
    };

    walk_entries(dir, entries, count, skipped).await
}

/// Shared entry-processing loop for [`check_files_executable_in`]'s root
/// call and [`walk_and_chmod`]'s recursive calls: chmod's `.ts` files,
/// skips symlinks, and recurses into subdirectories. See
/// [`check_files_executable_in`]'s docs for the full symlink-rejection and
/// error-tolerance rationale, which applies here unchanged at every
/// recursion depth — including a `next_entry()` read error mid-iteration,
/// which is always skip-and-warn (only the initial directory open has
/// different fatality between the root and its descendants).
#[cfg(unix)]
async fn walk_entries(
    dir: &Path,
    mut entries: tokio::fs::ReadDir,
    count: &mut usize,
    skipped: &mut usize,
) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::fs;

    loop {
        let entry = match entries.next_entry().await {
            Ok(Some(entry)) => entry,
            Ok(None) => break,
            Err(error) => {
                tracing::warn!(
                    path = %sanitize_path_for_error(dir),
                    %error,
                    "stopping directory read after error; skipping any remaining entries"
                );
                break;
            }
        };

        let path = entry.path();
        let file_type = entry.file_type().await?;
        if file_type.is_symlink() {
            tracing::warn!(
                path = %sanitize_path_for_error(&path),
                "skipping symlinked entry under the servers directory"
            );
            *skipped += 1;
            continue;
        }

        if file_type.is_dir() {
            Box::pin(walk_and_chmod(&path, count, skipped)).await?;
            continue;
        }

        if !file_type.is_file() || path.extension().and_then(|s| s.to_str()) != Some("ts") {
            continue;
        }

        let metadata = fs::metadata(&path).await?;
        let mut perms = metadata.permissions();
        perms.set_mode(0o755); // rwxr-xr-x
        fs::set_permissions(&path, perms).await?;
        *count += 1;
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

    #[tokio::test]
    async fn test_check_files_executable_no_panic() {
        // Should not panic regardless of whether ~/.claude/servers exists.
        let result = check_files_executable().await;
        assert!(result.is_ok());
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_makes_real_ts_files_executable() {
        use std::os::unix::fs::PermissionsExt;

        let servers_dir = tempfile::TempDir::new().unwrap();
        let my_server_dir = servers_dir.path().join("my-server");
        tokio::fs::create_dir_all(&my_server_dir).await.unwrap();
        let tool_path = my_server_dir.join("tool.ts");
        tokio::fs::write(&tool_path, "// tool").await.unwrap();

        let (servers_dir_found, files_made_executable, skipped_entries) =
            check_files_executable_in(servers_dir.path()).await.unwrap();

        assert!(servers_dir_found);
        assert_eq!(files_made_executable, 1);
        assert_eq!(skipped_entries, 0);
        let mode = tokio::fs::metadata(&tool_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_recurses_into_nested_subdirectories() {
        use std::os::unix::fs::PermissionsExt;

        let servers_dir = tempfile::TempDir::new().unwrap();
        let runtime_dir = servers_dir.path().join("my-server").join("_runtime");
        tokio::fs::create_dir_all(&runtime_dir).await.unwrap();
        let bridge_path = runtime_dir.join("mcp-bridge.ts");
        tokio::fs::write(&bridge_path, "// bridge").await.unwrap();

        let (servers_dir_found, files_made_executable, skipped_entries) =
            check_files_executable_in(servers_dir.path()).await.unwrap();

        assert!(servers_dir_found);
        assert_eq!(files_made_executable, 1);
        assert_eq!(skipped_entries, 0);
        let mode = tokio::fs::metadata(&bridge_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    // Note: this and the sibling `read_dir`-open-failure tests below do not exercise
    // `walk_entries`'s mid-iteration `next_entry()` `Err` branch (setup.rs's skip-and-warn
    // path for a read error *after* a directory has already opened successfully) — that
    // requires deterministic fault injection with no seam this test suite has today, and is
    // left untested (see #490's handoff notes).
    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_skips_unreadable_nested_dir_processes_siblings() {
        use std::os::unix::fs::PermissionsExt;

        let servers_dir = tempfile::TempDir::new().unwrap();

        let good_server_dir = servers_dir.path().join("good-server");
        tokio::fs::create_dir_all(&good_server_dir).await.unwrap();
        let good_tool_path = good_server_dir.join("tool.ts");
        tokio::fs::write(&good_tool_path, "// tool").await.unwrap();

        let locked_server_dir = servers_dir.path().join("locked-server");
        tokio::fs::create_dir_all(&locked_server_dir).await.unwrap();
        tokio::fs::set_permissions(&locked_server_dir, std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();

        // Root (and some CI runners) bypass directory permission checks, in which case the
        // property under test does not hold; restore permissions and skip.
        if tokio::fs::read_dir(&locked_server_dir).await.is_ok() {
            tokio::fs::set_permissions(&locked_server_dir, std::fs::Permissions::from_mode(0o755))
                .await
                .unwrap();
            return;
        }

        let result = check_files_executable_in(servers_dir.path()).await;

        // Restore permissions unconditionally so `TempDir`'s drop can clean up.
        tokio::fs::set_permissions(&locked_server_dir, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();

        let (servers_dir_found, files_made_executable, skipped_entries) = result.unwrap();

        assert!(servers_dir_found);
        assert_eq!(
            files_made_executable, 1,
            "the healthy sibling directory must still be processed"
        );
        assert_eq!(skipped_entries, 0);
        let mode = tokio::fs::metadata(&good_tool_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_propagates_root_read_dir_failure() {
        use std::os::unix::fs::PermissionsExt;

        let servers_dir = tempfile::TempDir::new().unwrap();
        tokio::fs::set_permissions(servers_dir.path(), std::fs::Permissions::from_mode(0o000))
            .await
            .unwrap();

        // Root (and some CI runners) bypass directory permission checks, in which case the
        // property under test does not hold; restore permissions and skip.
        if tokio::fs::read_dir(servers_dir.path()).await.is_ok() {
            tokio::fs::set_permissions(servers_dir.path(), std::fs::Permissions::from_mode(0o755))
                .await
                .unwrap();
            return;
        }

        let result = check_files_executable_in(servers_dir.path()).await;

        // Restore permissions unconditionally so `TempDir`'s drop can clean up.
        tokio::fs::set_permissions(servers_dir.path(), std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();

        assert!(
            result.is_err(),
            "an unreadable servers directory root must propagate an error, not report success"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_skips_symlinked_nested_subdirectory() {
        let servers_dir = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        let target_path = outside.path().join("target.ts");
        tokio::fs::write(&target_path, "// outside").await.unwrap();

        let my_server_dir = servers_dir.path().join("my-server");
        tokio::fs::create_dir_all(&my_server_dir).await.unwrap();
        std::os::unix::fs::symlink(outside.path(), my_server_dir.join("_runtime")).unwrap();

        let (servers_dir_found, files_made_executable, skipped_entries) =
            check_files_executable_in(servers_dir.path()).await.unwrap();

        assert!(servers_dir_found);
        assert_eq!(files_made_executable, 0);
        assert_eq!(skipped_entries, 1);
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &tokio::fs::metadata(&target_path)
                .await
                .unwrap()
                .permissions(),
        );
        assert_eq!(
            mode & 0o111,
            0,
            "symlinked nested directory's target must not be descended into"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_skips_symlinked_server_dir() {
        let servers_dir = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        let target_path = outside.path().join("target.ts");
        tokio::fs::write(&target_path, "// outside").await.unwrap();
        std::os::unix::fs::symlink(outside.path(), servers_dir.path().join("evil-server")).unwrap();

        let (servers_dir_found, files_made_executable, skipped_entries) =
            check_files_executable_in(servers_dir.path()).await.unwrap();

        assert!(servers_dir_found);
        assert_eq!(files_made_executable, 0);
        assert_eq!(skipped_entries, 1);
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &tokio::fs::metadata(&target_path)
                .await
                .unwrap()
                .permissions(),
        );
        assert_eq!(
            mode & 0o111,
            0,
            "symlinked target must not become executable"
        );
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_skips_symlinked_ts_file() {
        let servers_dir = tempfile::TempDir::new().unwrap();
        let outside = tempfile::TempDir::new().unwrap();
        let target_path = outside.path().join("target.ts");
        tokio::fs::write(&target_path, "// outside").await.unwrap();

        let legit_server_dir = servers_dir.path().join("legit-server");
        tokio::fs::create_dir_all(&legit_server_dir).await.unwrap();
        std::os::unix::fs::symlink(&target_path, legit_server_dir.join("link.ts")).unwrap();

        let (servers_dir_found, files_made_executable, skipped_entries) =
            check_files_executable_in(servers_dir.path()).await.unwrap();

        assert!(servers_dir_found);
        assert_eq!(files_made_executable, 0);
        assert_eq!(skipped_entries, 1);
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &tokio::fs::metadata(&target_path)
                .await
                .unwrap()
                .permissions(),
        );
        assert_eq!(
            mode & 0o111,
            0,
            "symlinked target must not become executable"
        );
    }

    #[test]
    fn test_setup_result_serialization() {
        let result = SetupResult {
            node_version: "20.10.0".to_string(),
            mcp_config_path: "/home/user/.claude/mcp.json".to_string(),
            mcp_config_found: true,
            servers_dir_found: true,
            files_made_executable: 3,
            skipped_entries: 0,
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
            skipped_entries: 0,
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
            skipped_entries: 0,
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
