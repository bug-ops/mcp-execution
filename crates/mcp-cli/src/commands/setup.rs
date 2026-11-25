//! Setup command implementation.
//!
//! Validates the runtime environment for MCP tool execution:
//! - Checks a Node.js that runs `.ts` files without a flag (22.18.0+) is installed
//! - Verifies generated files are executable
//! - Provides helpful error messages and suggestions

use anyhow::Result;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
#[cfg(unix)]
use mcp_execution_core::sanitize_path_for_error;
use serde::Serialize;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use crate::commands::common::ServersDir;
use crate::commands::common::mcp_config_path;
use crate::runtime::helper::{CliBinary, discover_cli_binary};
use crate::runtime::node::check_node_version;
use mcp_execution_core::helper_protocol::CLI_PATH_ENV;

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
/// use mcp_execution_cli::runtime::helper::CliBinary;
///
/// let result = SetupResult {
///     node_version: "20.10.0".to_string(),
///     mcp_config_path: "/home/user/.claude/mcp.json".to_string(),
///     mcp_config_found: true,
///     servers_dir_found: true,
///     files_made_executable: 3,
///     skipped_entries: 0,
///     failed_entries: 0,
///     helper_binary: CliBinary::NotFound,
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
    /// Number of non-symlink entries under `~/.claude/servers/` whose file type or
    /// permissions could not be read or changed, so a `.ts` file may not be executable.
    /// Always `0` on non-Unix platforms.
    pub failed_entries: usize,
    /// The `mcp-execution-cli` binary a generated tool run outside `run` would start as its
    /// helper (`MCP_EXECUTION_CLI`, else `PATH`), or why none is usable.
    pub helper_binary: CliBinary,
}

/// Runs the setup command.
///
/// Validates that the runtime environment is ready for MCP tool execution
/// and renders the results according to `output_format`.
///
/// # Checks Performed
///
/// 1. **Node.js version**: Ensures Node.js 22.18.0 or higher (23.6.0 or higher on the 23 line) is
///    installed, the first versions that run generated `.ts` files without a flag
/// 2. **File permissions**: Makes TypeScript files executable (Unix only)
/// 3. **Configuration**: Checks if ~/.claude/mcp.json exists
/// 4. **Helper binary**: Reports whether generated tools run outside `run` can find
///    `mcp-execution-cli` (`MCP_EXECUTION_CLI`, else `PATH`); a miss is a warning, not an error
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
/// - Node.js version cannot run `.ts` files without a flag (below 22.18.0, or 23.0.0 to 23.5.x)
/// - Home directory cannot be determined
/// - Output formatting fails (serialization error)
pub async fn run(output_format: OutputFormat) -> Result<ExitCode> {
    if output_format == OutputFormat::Pretty {
        println!("Checking runtime environment...\n");
    }

    let node_version = check_node_version().await?;

    let mcp_config_path = mcp_config_path()?;
    let mcp_config_found = mcp_config_path.exists();

    let scan = check_files_executable().await?;

    let result = SetupResult {
        node_version,
        mcp_config_path: mcp_config_path.display().to_string(),
        mcp_config_found,
        servers_dir_found: scan.is_some(),
        files_made_executable: scan.map_or(0, |report| report.made_executable),
        skipped_entries: scan.map_or(0, |report| report.skipped_entries),
        failed_entries: scan.map_or(0, |report| report.failed_entries),
        helper_binary: discover_cli_binary(),
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
            if result.failed_entries > 0 {
                println!(
                    "⚠ Could not make {} entr{} executable (see warnings above); \
                     generated tools there may not run directly",
                    result.failed_entries,
                    if result.failed_entries == 1 {
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

    match &result.helper_binary {
        CliBinary::Found(path) => println!("✓ Helper binary found: {}", path.display()),
        CliBinary::OverrideInvalid => println!(
            "⚠ {CLI_PATH_ENV} is set but is not an absolute path to an executable file; \
             generated tools run outside 'run' will not find the helper"
        ),
        CliBinary::NotFound => {
            println!("⚠ No mcp-execution-cli binary found for generated tools run outside 'run'");
            println!(
                "  Put it on PATH or set {CLI_PATH_ENV} to its absolute path; 'run' does not need it"
            );
        }
    }

    if result.failed_entries > 0 {
        println!("\n⚠ Runtime setup finished with warnings");
    } else {
        println!("\n✓ Runtime setup complete");
    }
    println!("  Claude Code can now execute MCP tools via:");
    println!("  node ~/.claude/servers/<server>/<tool>.ts '{{\"param\":\"value\"}}'");
    println!("\nNext steps:");
    println!("  1. Generate tools: mcp-execution-cli generate <server>");
    println!("  2. Configure servers in ~/.claude/mcp.json");
    println!("  3. Execute tools autonomously via Node.js");
}

/// Tally of one walk over the servers directory.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
struct WalkReport {
    /// `.ts` files made executable.
    made_executable: usize,
    /// Symlinked entries left untouched.
    skipped_entries: usize,
    /// Entries whose file type or permissions could not be read or changed.
    failed_entries: usize,
}

/// Checks for and makes TypeScript files executable (Unix only).
///
/// Sets executable permissions (0755) on all .ts files in the servers directory
/// This allows files to be executed with shebang: `./tool.ts`
///
/// # Platform Support
///
/// - Unix/Linux/macOS: Sets permissions, returns the walk tally, or `None` when the
///   servers directory does not exist
/// - Windows: No-op, always returns `None`
///
/// # Errors
///
/// Returns error if:
/// - Home directory cannot be determined
/// - The servers directory root cannot be read
#[cfg(unix)]
async fn check_files_executable() -> Result<Option<WalkReport>> {
    let servers_dir = ServersDir::resolve(None)?;
    check_files_executable_in(servers_dir.as_path()).await
}

/// Checks for and makes TypeScript files executable (Unix only).
///
/// No-op on non-Unix platforms, since file permissions are not checked there.
#[cfg(not(unix))]
async fn check_files_executable() -> Result<Option<WalkReport>> {
    Ok(None)
}

/// Recursively walks `servers_dir` and makes every `.ts` file executable
/// (0755) at any depth, rejecting symlinked entries at every recursion level
/// rather than following them.
///
/// A symlinked entry — a server-id directory, an intermediate subdirectory,
/// or a `.ts` file, at any depth — is skipped and counted in
/// [`WalkReport::skipped_entries`] rather than chmod'd or descended into, since following
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
/// Returns `None` when `servers_dir` does not exist.
///
/// # Errors
///
/// Returns an error if `servers_dir` cannot be canonicalized or the root
/// directory itself cannot be opened for reading. A root-open failure is
/// fatal (there are no sibling directories to protect at the root, so
/// tolerating it would only hide a real error). Every other failure is not
/// propagated but logged as a warning, and the walk moves on rather than
/// aborting the whole `setup` run: a directory below the root that fails to
/// open, a read error mid-iteration over any directory's entries (root
/// included; the affected directory's remaining entries are skipped), an entry
/// whose file type cannot be determined, and a `.ts` file whose permissions
/// cannot be read or changed are each logged and counted once in
/// [`WalkReport::failed_entries`] (such a file is not counted as executable).
#[cfg(unix)]
async fn check_files_executable_in(servers_dir: &Path) -> Result<Option<WalkReport>> {
    use tokio::fs;

    if !servers_dir.exists() {
        return Ok(None);
    }

    let root = fs::canonicalize(servers_dir).await?;
    // The root's own read_dir failure is fatal: unlike a nested server-id
    // directory, there are no siblings at the root to protect by tolerating
    // it, so propagating is the only way the caller learns setup didn't run.
    let entries = fs::read_dir(&root).await?;

    let mut report = WalkReport::default();
    walk_entries(&root, entries, &mut report).await?;

    Ok(Some(report))
}

/// Recursion step for [`check_files_executable_in`]: opens `dir` — skipping
/// it with a warning rather than propagating if it cannot be opened, since
/// (unlike the walk's root) it always has siblings whose processing should
/// continue — then delegates to [`walk_entries`] for the shared per-entry
/// logic.
#[cfg(unix)]
async fn walk_and_chmod(dir: &Path, report: &mut WalkReport) -> Result<()> {
    use tokio::fs;

    let entries = match fs::read_dir(dir).await {
        Ok(entries) => entries,
        Err(error) => {
            tracing::warn!(
                path = %sanitize_path_for_error(dir),
                %error,
                "skipping unreadable directory under the servers directory"
            );
            report.failed_entries += 1;
            return Ok(());
        }
    };

    walk_entries(dir, entries, report).await
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
    report: &mut WalkReport,
) -> Result<()> {
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
                report.failed_entries += 1;
                break;
            }
        };

        let path = entry.path();
        let file_type = match entry.file_type().await {
            Ok(file_type) => file_type,
            Err(error) => {
                tracing::warn!(
                    path = %sanitize_path_for_error(&path),
                    %error,
                    "skipping entry whose file type cannot be determined"
                );
                report.failed_entries += 1;
                continue;
            }
        };
        if file_type.is_symlink() {
            tracing::warn!(
                path = %sanitize_path_for_error(&path),
                "skipping symlinked entry under the servers directory"
            );
            report.skipped_entries += 1;
            continue;
        }

        if file_type.is_dir() {
            Box::pin(walk_and_chmod(&path, report)).await?;
            continue;
        }

        if !file_type.is_file() || path.extension().and_then(|s| s.to_str()) != Some("ts") {
            continue;
        }

        match make_executable(&path).await {
            Ok(()) => report.made_executable += 1,
            Err(error) => {
                tracing::warn!(
                    path = %sanitize_path_for_error(&path),
                    %error,
                    "skipping file whose permissions cannot be changed"
                );
                report.failed_entries += 1;
            }
        }
    }

    Ok(())
}

/// Sets `path` to `0o755` (rwxr-xr-x).
#[cfg(unix)]
async fn make_executable(path: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    use tokio::fs;

    let mut perms = fs::metadata(path).await?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(path, perms).await
}

#[cfg(test)]
mod tests {
    use super::*;
    #[cfg(unix)]
    use crate::commands::common::test_isolation::process_per_test;

    #[cfg(unix)]
    #[tokio::test]
    async fn make_executable_sets_mode_and_reports_missing_file() {
        use std::os::unix::fs::PermissionsExt;

        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("tool.ts");
        std::fs::write(&file, "x").unwrap();
        make_executable(&file).await.unwrap();
        let mode = std::fs::metadata(&file).unwrap().permissions().mode();
        assert_eq!(mode & 0o777, 0o755);

        assert!(make_executable(&dir.path().join("gone.ts")).await.is_err());
    }

    #[test]
    fn test_mcp_config_path() {
        let path = mcp_config_path();
        assert!(path.is_ok());

        let path = path.unwrap();
        assert!(path.to_string_lossy().contains(".claude"));
        assert!(path.to_string_lossy().contains("mcp.json"));
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

        let report = check_files_executable_in(servers_dir.path())
            .await
            .unwrap()
            .expect("servers dir exists");

        assert_eq!(
            report,
            WalkReport {
                made_executable: 1,
                skipped_entries: 0,
                failed_entries: 0,
            }
        );
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

        let report = check_files_executable_in(servers_dir.path())
            .await
            .unwrap()
            .expect("servers dir exists");

        assert_eq!(
            report,
            WalkReport {
                made_executable: 1,
                skipped_entries: 0,
                failed_entries: 0,
            }
        );
        let mode = tokio::fs::metadata(&bridge_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    // Note: the `read_dir`-open-failure tests do not exercise `walk_entries`'s mid-iteration
    // `next_entry()` `Err` branch — that requires deterministic fault injection with no seam
    // this test suite has today, and is left untested (see #490's handoff notes).
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

        let report = result.unwrap().expect("servers dir exists");

        assert_eq!(
            report,
            WalkReport {
                made_executable: 1,
                skipped_entries: 0,
                failed_entries: 1,
            },
            "the healthy sibling is processed and the unlistable directory is a failed entry"
        );
        let mode = tokio::fs::metadata(&good_tool_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_counts_a_directory_without_read_bit_as_failed() {
        use std::os::unix::fs::PermissionsExt;

        let servers_dir = tempfile::TempDir::new().unwrap();
        let write_only_dir = servers_dir.path().join("x");
        tokio::fs::create_dir_all(&write_only_dir).await.unwrap();
        tokio::fs::write(write_only_dir.join("tool.ts"), "// tool")
            .await
            .unwrap();
        tokio::fs::set_permissions(&write_only_dir, std::fs::Permissions::from_mode(0o300))
            .await
            .unwrap();

        let result = if std::fs::read_dir(&write_only_dir).is_ok() {
            None
        } else {
            Some(check_files_executable_in(servers_dir.path()).await)
        };

        tokio::fs::set_permissions(&write_only_dir, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();

        let Some(result) = result else { return };
        assert_eq!(
            result.unwrap().expect("servers dir exists"),
            WalkReport {
                made_executable: 0,
                skipped_entries: 0,
                failed_entries: 1,
            }
        );
    }

    /// Collects formatted log output in memory for assertions.
    #[cfg(unix)]
    #[derive(Clone, Default)]
    struct LogBuffer(std::sync::Arc<std::sync::Mutex<Vec<u8>>>);

    #[cfg(unix)]
    impl std::io::Write for LogBuffer {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }

        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    // A directory without the search bit still lists its entries (`read_dir`, `d_type`) but
    // fails `metadata()` on each one, so it forces a per-entry failure inside the walk.
    #[cfg(unix)]
    #[tokio::test]
    async fn check_files_executable_in_warns_and_continues_when_a_file_cannot_be_chmodded() {
        use std::os::unix::fs::PermissionsExt;

        if !process_per_test(
            "check_files_executable_in_warns_and_continues_when_a_file_cannot_be_chmodded",
        ) {
            return;
        }
        let servers_dir = tempfile::TempDir::new().unwrap();
        let good_tool_path = servers_dir.path().join("good.ts");
        tokio::fs::write(&good_tool_path, "// tool").await.unwrap();

        let stat_locked_dir = servers_dir.path().join("stat-locked");
        tokio::fs::create_dir_all(&stat_locked_dir).await.unwrap();
        tokio::fs::write(stat_locked_dir.join("bad.ts"), "// tool")
            .await
            .unwrap();
        tokio::fs::set_permissions(&stat_locked_dir, std::fs::Permissions::from_mode(0o600))
            .await
            .unwrap();

        // Root bypasses permission checks, so the property under test does not hold there.
        if tokio::fs::metadata(stat_locked_dir.join("bad.ts"))
            .await
            .is_ok()
        {
            tokio::fs::set_permissions(&stat_locked_dir, std::fs::Permissions::from_mode(0o755))
                .await
                .unwrap();
            return;
        }

        let logs = LogBuffer::default();
        let subscriber = tracing_subscriber::fmt()
            .with_writer({
                let logs = logs.clone();
                move || logs.clone()
            })
            .with_ansi(false)
            .finish();
        let guard = tracing::subscriber::set_default(subscriber);
        let result = check_files_executable_in(servers_dir.path()).await;
        drop(guard);

        tokio::fs::set_permissions(&stat_locked_dir, std::fs::Permissions::from_mode(0o755))
            .await
            .unwrap();

        let report = result.unwrap().expect("servers dir exists");
        assert_eq!(
            report,
            WalkReport {
                made_executable: 1,
                skipped_entries: 0,
                failed_entries: 1,
            },
            "only the healthy file counts as executable; the failing one is a failed entry"
        );

        let mode = tokio::fs::metadata(&good_tool_path)
            .await
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o755);

        let output = String::from_utf8(logs.0.lock().unwrap().clone()).unwrap();
        assert!(
            output.contains("permissions cannot be changed"),
            "expected a skip warning, got: {output}"
        );
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

        let report = check_files_executable_in(servers_dir.path())
            .await
            .unwrap()
            .expect("servers dir exists");

        assert_eq!(
            report,
            WalkReport {
                made_executable: 0,
                skipped_entries: 1,
                failed_entries: 0,
            }
        );
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

        let report = check_files_executable_in(servers_dir.path())
            .await
            .unwrap()
            .expect("servers dir exists");

        assert_eq!(
            report,
            WalkReport {
                made_executable: 0,
                skipped_entries: 1,
                failed_entries: 0,
            }
        );
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

        let report = check_files_executable_in(servers_dir.path())
            .await
            .unwrap()
            .expect("servers dir exists");

        assert_eq!(
            report,
            WalkReport {
                made_executable: 0,
                skipped_entries: 1,
                failed_entries: 0,
            }
        );
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
            failed_entries: 0,
            helper_binary: CliBinary::NotFound,
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
            failed_entries: 0,
            helper_binary: CliBinary::NotFound,
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
            failed_entries: 0,
            helper_binary: CliBinary::NotFound,
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
