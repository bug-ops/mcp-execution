//! Skill command implementation.
//!
//! Generates Claude Code instruction skill files (SKILL.md) from progressive loading
//! TypeScript tools. This command:
//! 1. Scans generated TypeScript files in `~/.claude/servers/{server}/`
//! 2. Extracts tool metadata and categories
//! 3. Generates structured context for skill creation
//! 4. Returns a prompt for Claude to generate optimal SKILL.md content

use anyhow::{Context, Result, bail};
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_skill::{
    build_skill_context, render_skill_md, scan_tools_directory, validate_server_id,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

use crate::formatters::format_output;

/// Output of a successful `skill` command invocation.
#[derive(Debug, Serialize)]
struct SkillWriteResult {
    success: bool,
    output_path: String,
    bytes_written: usize,
    tool_count: usize,
}

/// Default base directory for generated servers.
const DEFAULT_SERVERS_DIR: &str = ".claude/servers";

/// Default base directory for skills.
const DEFAULT_SKILLS_DIR: &str = ".claude/skills";

/// Runs the skill command.
///
/// Scans generated progressive loading TypeScript files and prepares context
/// for generating a Claude Code instruction skill (SKILL.md).
///
/// # Process
///
/// 1. Validates server ID format
/// 2. Determines servers directory (default: ~/.claude/servers)
/// 3. Validates path security (no symlink escape)
/// 4. Scans TypeScript files in `{servers_dir}/{server}/`
/// 5. Builds skill generation context
/// 6. Returns structured output with generation prompt
///
/// # Arguments
///
/// * `server` - Server identifier (e.g., "github")
/// * `servers_dir` - Base directory for generated servers (default: ~/.claude/servers)
/// * `output_path` - Custom output path for SKILL.md (default: ~/.claude/skills/{server}/SKILL.md)
/// * `skill_name` - Custom skill name (default: {server}-progressive)
/// * `hints` - Use case hints for skill generation
/// * `overwrite` - Whether to overwrite existing SKILL.md file
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if:
/// - Server ID format is invalid
/// - Servers directory does not exist
/// - Server subdirectory does not exist
/// - Path traversal detected
/// - TypeScript files cannot be scanned
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::skill;
/// use mcp_execution_core::cli::OutputFormat;
///
/// # async fn example() -> anyhow::Result<()> {
/// // Generate skill for GitHub server
/// let exit_code = skill::run(
///     "github".to_string(),
///     None,
///     None,
///     None,
///     vec![],
///     false,
///     OutputFormat::Json
/// ).await?;
/// # Ok(())
/// # }
/// ```
// One argument per CLI flag; clap already destructures flags for us, and
// grouping them into a struct would only benefit this function, not caller ergonomics.
#[allow(clippy::too_many_arguments)]
pub async fn run(
    server: String,
    servers_dir: Option<PathBuf>,
    output_path: Option<PathBuf>,
    skill_name: Option<String>,
    hints: Vec<String>,
    overwrite: bool,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    debug!("Generating skill for server: {}", server);
    debug!("Servers directory: {:?}", servers_dir);
    debug!("Output path: {:?}", output_path);
    debug!("Skill name: {:?}", skill_name);
    debug!("Hints: {:?}", hints);
    debug!("Overwrite: {}", overwrite);
    debug!("Output format: {}", output_format);

    // Step 1: Validate server ID
    validate_server_id(&server).map_err(|e| anyhow::anyhow!("Invalid server ID: {e}"))?;
    info!("Server ID validated: {}", server);

    // Step 2: Resolve servers directory
    let servers_base = resolve_servers_dir(servers_dir.as_deref())?;
    debug!("Servers base directory: {}", servers_base.display());

    // Step 3: Build and validate server path
    let tool_dir = servers_base.join(&server);
    let tool_dir = validate_path_security(&tool_dir, &servers_base)?;
    debug!("Server directory: {}", tool_dir.display());

    // Step 4: Check server directory exists
    if !tool_dir.exists() {
        bail!(
            "Server directory not found: {}\n\
             Run 'mcp-execution-cli generate --from-config {}' first to generate TypeScript files.",
            tool_dir.display(),
            server
        );
    }

    // Step 5: Scan TypeScript files
    info!("Scanning TypeScript files in {}", tool_dir.display());
    let tools = scan_tools_directory(&tool_dir)
        .await
        .context("Failed to scan tools directory")?;

    if tools.is_empty() {
        bail!(
            "No TypeScript tool files found in {}\n\
             Run 'mcp-execution-cli generate --from-config {}' first.",
            tool_dir.display(),
            server
        );
    }

    // `tools.len()` reflects sidecar entries that were cross-checked against an
    // actual `.ts` file on disk by `scan_tools_directory` (issue #154) — not a
    // raw sidecar entry count.
    info!("Verified {} tool files against sidecar", tools.len());

    // Step 6: Build skill context
    let hints_ref: Option<Vec<String>> = if hints.is_empty() { None } else { Some(hints) };

    let mut context = build_skill_context(&server, &tools, hints_ref.as_deref());

    // Apply custom skill name if provided
    if let Some(name) = skill_name {
        context.skill_name = name;
    }

    // Apply custom output path if provided
    if let Some(path) = output_path {
        // Validate output path for path traversal
        validate_output_path(&path)?;
        context.output_path = path.display().to_string();
    } else {
        // Use default skills directory
        let skills_dir = resolve_skills_dir()?;
        let default_output = skills_dir.join(&server).join("SKILL.md");
        context.output_path = default_output.display().to_string();
    }

    // Check if output file exists and overwrite flag
    let output_path = PathBuf::from(&context.output_path);
    if output_path.exists() && !overwrite {
        bail!(
            "Output file already exists: {}\n\
             Use --overwrite to replace existing file.",
            output_path.display()
        );
    }

    // Step 7: Render SKILL.md and write atomically.
    let rendered = render_skill_md(&context).context("failed to render SKILL.md template")?;

    if let Some(parent) = output_path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    // Atomic write: write to a temp file in the same directory, then rename.
    // `with_added_extension` appends `.tmp` without removing `.md` (N1).
    let tmp_path = output_path.with_added_extension("tmp");
    std::fs::write(&tmp_path, &rendered)
        .with_context(|| format!("failed to write temp file: {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, &output_path)
        .with_context(|| format!("failed to rename to: {}", output_path.display()))?;

    let bytes_written = rendered.len();
    info!(
        "SKILL.md written to {} ({} bytes, {} tools)",
        output_path.display(),
        bytes_written,
        context.tool_count,
    );

    let result = SkillWriteResult {
        success: true,
        output_path: output_path.display().to_string(),
        bytes_written,
        tool_count: context.tool_count,
    };

    let output = format_output(&result, output_format)?;
    println!("{output}");

    Ok(ExitCode::SUCCESS)
}

/// Resolve servers directory from provided path or default.
///
/// # Arguments
///
/// * `servers_dir` - Optional custom servers directory
///
/// # Returns
///
/// Resolved path to servers directory.
///
/// # Errors
///
/// Returns error if home directory cannot be determined.
fn resolve_servers_dir(servers_dir: Option<&Path>) -> Result<PathBuf> {
    if let Some(dir) = servers_dir {
        // Use provided path, expand ~ if needed
        if let Some(stripped) = dir.to_str().and_then(|s| s.strip_prefix("~/")) {
            let home = dirs::home_dir().context("Could not determine home directory")?;
            Ok(home.join(stripped))
        } else {
            Ok(dir.to_path_buf())
        }
    } else {
        // Use default: ~/.claude/servers
        let home = dirs::home_dir().context("Could not determine home directory")?;
        Ok(home.join(DEFAULT_SERVERS_DIR))
    }
}

/// Resolve skills directory (default: ~/.claude/skills).
///
/// # Returns
///
/// Resolved path to skills directory.
///
/// # Errors
///
/// Returns error if home directory cannot be determined.
fn resolve_skills_dir() -> Result<PathBuf> {
    let home = dirs::home_dir().context("Could not determine home directory")?;
    Ok(home.join(DEFAULT_SKILLS_DIR))
}

/// Validate path security to prevent path traversal attacks.
///
/// Ensures the resolved path is within the expected base directory.
///
/// # Arguments
///
/// * `path` - Path to validate
/// * `base` - Expected base directory
///
/// # Returns
///
/// Canonicalized path if valid.
///
/// # Errors
///
/// Returns error if:
/// - Path cannot be canonicalized
/// - Path is outside the base directory (symlink escape)
fn validate_path_security(path: &Path, base: &Path) -> Result<PathBuf> {
    // Check for path traversal in components (more robust than string check)
    if has_path_traversal(path) {
        bail!("Path traversal detected: {}", path.display());
    }

    // If the path doesn't exist yet, validation passed
    if !path.exists() {
        return Ok(path.to_path_buf());
    }

    // Canonicalize to resolve symlinks
    let canonical_path = path
        .canonicalize()
        .with_context(|| format!("Failed to canonicalize path: {}", path.display()))?;

    let canonical_base = if base.exists() {
        base.canonicalize()
            .with_context(|| format!("Failed to canonicalize base: {}", base.display()))?
    } else {
        // Base doesn't exist, path components already validated
        return Ok(path.to_path_buf());
    };

    // Verify path is within base directory
    if !canonical_path.starts_with(&canonical_base) {
        bail!(
            "Security error: path {} is outside base directory {}",
            canonical_path.display(),
            canonical_base.display()
        );
    }

    Ok(canonical_path)
}

/// Validate output path for path traversal attacks.
///
/// # Arguments
///
/// * `path` - Output path to validate
///
/// # Errors
///
/// Returns error if path contains traversal components (`..`).
fn validate_output_path(path: &Path) -> Result<()> {
    if has_path_traversal(path) {
        bail!(
            "Invalid output path (path traversal detected): {}",
            path.display()
        );
    }
    Ok(())
}

/// Check if path contains traversal components.
///
/// Uses path component analysis instead of string matching for robustness.
fn has_path_traversal(path: &Path) -> bool {
    use std::path::Component;
    path.components().any(|c| matches!(c, Component::ParentDir))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::metadata::{
        METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ParameterMetadata, ServerMetadata,
        ToolMetadata,
    };
    use tempfile::TempDir;

    /// Writes a minimal `_meta.json` sidecar with a single tool into `server_dir`,
    /// matching what `mcp-execution-codegen` would emit for a generated server.
    ///
    /// Also writes a matching stub `{typescript_name}.ts` file, since
    /// `scan_tools_directory` cross-checks the sidecar against files on disk.
    fn write_meta_sidecar(server_dir: &Path, server_id: &str, tool_name: &str) {
        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: server_id.to_string(),
            server_name: server_id.to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![ToolMetadata {
                name: tool_name.to_string(),
                typescript_name: tool_name.to_string(),
                category: Some("testing".to_string()),
                keywords: vec!["test".to_string()],
                description: Some(format!("Test tool: {tool_name}")),
                parameters: vec![ParameterMetadata {
                    name: "input".to_string(),
                    typescript_type: "string".to_string(),
                    required: true,
                    description: Some("Test input".to_string()),
                }],
            }],
        };

        let content = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(server_dir.join(METADATA_FILE_NAME), content).unwrap();
        std::fs::write(server_dir.join(format!("{tool_name}.ts")), "export {}").unwrap();
    }

    #[test]
    fn test_resolve_servers_dir_default() {
        let result = resolve_servers_dir(None);
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains(".claude/servers"));
    }

    #[test]
    fn test_resolve_servers_dir_custom() {
        let custom = PathBuf::from("/custom/servers");
        let result = resolve_servers_dir(Some(&custom));
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), custom);
    }

    #[test]
    fn test_resolve_servers_dir_tilde() {
        let custom = PathBuf::from("~/custom/servers");
        let result = resolve_servers_dir(Some(&custom));
        assert!(result.is_ok());
        let path = result.unwrap();
        // Should expand ~ to home directory
        assert!(!path.to_string_lossy().starts_with('~'));
        assert!(path.to_string_lossy().contains("custom/servers"));
    }

    #[test]
    fn test_validate_path_security_valid() {
        let temp = TempDir::new().unwrap();
        let base = temp.path();
        let subdir = base.join("server");
        std::fs::create_dir(&subdir).unwrap();

        let result = validate_path_security(&subdir, base);
        assert!(result.is_ok());
    }

    #[test]
    fn test_validate_path_security_traversal() {
        let temp = TempDir::new().unwrap();
        let base = temp.path();
        let evil_path = base.join("..").join("etc").join("passwd");

        let result = validate_path_security(&evil_path, base);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("traversal"));
    }

    #[test]
    fn test_validate_path_security_nonexistent() {
        let temp = TempDir::new().unwrap();
        let base = temp.path();
        let new_path = base.join("new-server");

        // Non-existent paths without .. should be allowed
        let result = validate_path_security(&new_path, base);
        assert!(result.is_ok());
    }

    #[test]
    fn test_resolve_skills_dir() {
        let result = resolve_skills_dir();
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.to_string_lossy().contains(".claude/skills"));
    }

    #[test]
    fn test_has_path_traversal() {
        // Should detect traversal
        assert!(has_path_traversal(Path::new("../etc/passwd")));
        assert!(has_path_traversal(Path::new("/tmp/../etc/passwd")));
        assert!(has_path_traversal(Path::new("foo/../../bar")));

        // Should not flag valid paths
        assert!(!has_path_traversal(Path::new("/etc/passwd")));
        assert!(!has_path_traversal(Path::new("foo/bar/baz")));
        assert!(!has_path_traversal(Path::new("./foo/bar")));
        assert!(!has_path_traversal(Path::new("...")));
        assert!(!has_path_traversal(Path::new("..foo")));
    }

    #[test]
    fn test_validate_output_path_valid() {
        assert!(validate_output_path(Path::new("/tmp/skill.md")).is_ok());
        assert!(validate_output_path(Path::new("~/.claude/skills/github/SKILL.md")).is_ok());
        assert!(validate_output_path(Path::new("./output.md")).is_ok());
    }

    #[test]
    fn test_validate_output_path_traversal() {
        let result = validate_output_path(Path::new("../../../etc/passwd"));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));

        let result = validate_output_path(Path::new("/tmp/../etc/passwd"));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_output_path_traversal() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("github");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "github", "test");

        // Try to use path traversal in output path
        let evil_output = temp
            .path()
            .join("..")
            .join("..")
            .join("etc")
            .join("evil.md");

        let result = run(
            "github".to_string(),
            Some(temp.path().to_path_buf()),
            Some(evil_output),
            None,
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("path traversal"));
    }

    #[tokio::test]
    async fn test_run_invalid_server_id() {
        let result = run(
            "INVALID_ID".to_string(), // uppercase not allowed
            None,
            None,
            None,
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid server ID")
        );
    }

    #[tokio::test]
    async fn test_run_server_not_found() {
        let temp = TempDir::new().unwrap();
        let result = run(
            "nonexistent-server".to_string(),
            Some(temp.path().to_path_buf()),
            None,
            None,
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Server directory not found")
        );
    }

    #[tokio::test]
    async fn test_run_no_typescript_files() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("empty-server");
        std::fs::create_dir(&server_dir).unwrap();

        // No `_meta.json` sidecar: the directory exists but was never generated
        // (or predates the sidecar), so scanning must hard-error.
        let result = run(
            "empty-server".to_string(),
            Some(temp.path().to_path_buf()),
            None,
            None,
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Failed to scan tools directory")
        );
    }

    #[tokio::test]
    async fn test_run_with_valid_typescript_files() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("test-server");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "test-server", "test_tool");

        let output_path = temp.path().join("SKILL.md");

        let result = run(
            "test-server".to_string(),
            Some(temp.path().to_path_buf()),
            Some(output_path.clone()),
            None,
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(
            result.is_ok(),
            "Expected success but got: {:?}",
            result.err()
        );
        assert!(output_path.exists(), "SKILL.md must be written to disk");
        let content = std::fs::read_to_string(&output_path).unwrap();
        assert!(
            content.starts_with("---\n"),
            "SKILL.md must start with YAML frontmatter"
        );
    }

    #[tokio::test]
    async fn test_run_with_custom_skill_name() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("github");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "github", "create_issue");

        // Use custom output path to avoid conflicts with real files
        let output_path = temp.path().join("SKILL.md");

        let result = run(
            "github".to_string(),
            Some(temp.path().to_path_buf()),
            Some(output_path),
            Some("github-advanced".to_string()),
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(
            result.is_ok(),
            "Expected success but got: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_run_with_hints() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("github");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "github", "list_prs");

        // Use custom output path to avoid conflicts with real files
        let output_path = temp.path().join("SKILL.md");

        let result = run(
            "github".to_string(),
            Some(temp.path().to_path_buf()),
            Some(output_path),
            None,
            vec!["code review".to_string(), "CI/CD".to_string()],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(
            result.is_ok(),
            "Expected success but got: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_run_output_exists_no_overwrite() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("github");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "github", "test");

        // Create existing output file
        let output_path = temp.path().join("SKILL.md");
        std::fs::write(&output_path, "existing content").unwrap();

        let result = run(
            "github".to_string(),
            Some(temp.path().to_path_buf()),
            Some(output_path),
            None,
            vec![],
            false, // no overwrite
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[tokio::test]
    async fn test_run_output_exists_with_overwrite() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("github");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "github", "test");

        // Create existing output file
        let output_path = temp.path().join("SKILL.md");
        std::fs::write(&output_path, "existing content").unwrap();

        let result = run(
            "github".to_string(),
            Some(temp.path().to_path_buf()),
            Some(output_path),
            None,
            vec![],
            true, // overwrite
            OutputFormat::Json,
        )
        .await;

        assert!(
            result.is_ok(),
            "Expected success but got: {:?}",
            result.err()
        );
    }

    #[tokio::test]
    async fn test_run_all_output_formats() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("test");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "test", "test");

        for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
            let output_path = temp.path().join(format!("SKILL-{format}.md"));
            let result = run(
                "test".to_string(),
                Some(temp.path().to_path_buf()),
                Some(output_path),
                None,
                vec![],
                false,
                format,
            )
            .await;

            assert!(
                result.is_ok(),
                "Format {:?} should succeed: {:?}",
                format,
                result.err()
            );
        }
    }

    #[tokio::test]
    async fn test_run_stale_metadata_fails_instead_of_silently_succeeding() {
        // Issue #154 repro: a `_meta.json` sidecar that has drifted from the
        // `.ts` files on disk (one entry's file was deleted, an unrelated file
        // was added) must now make `skill` fail loudly instead of silently
        // generating a SKILL.md with stale/missing tool references.
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("github");
        std::fs::create_dir(&server_dir).unwrap();

        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: "github".to_string(),
            server_name: "GitHub".to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![
                ToolMetadata {
                    name: "create_issue".to_string(),
                    typescript_name: "createIssue".to_string(),
                    category: Some("issues".to_string()),
                    keywords: vec!["create".to_string()],
                    description: Some("Create an issue".to_string()),
                    parameters: vec![ParameterMetadata {
                        name: "title".to_string(),
                        typescript_type: "string".to_string(),
                        required: true,
                        description: Some("Issue title".to_string()),
                    }],
                },
                ToolMetadata {
                    name: "list_repos".to_string(),
                    typescript_name: "listRepos".to_string(),
                    category: Some("repos".to_string()),
                    keywords: vec!["list".to_string()],
                    description: Some("List repos".to_string()),
                    parameters: vec![],
                },
            ],
        };
        let content = serde_json::to_string_pretty(&meta).unwrap();
        std::fs::write(server_dir.join(METADATA_FILE_NAME), content).unwrap();

        // Generate as normal for `list_repos`, but simulate a `.ts` file that
        // was deleted (or never written, e.g. an interrupted `generate`) for
        // `create_issue` — this is the drift the sidecar must now catch.
        std::fs::write(server_dir.join("listRepos.ts"), "export {}").unwrap();
        // An unrelated `.ts` file left over on disk, not referenced by the
        // sidecar at all — must not mask the missing-file error above.
        std::fs::write(server_dir.join("orphanTool.ts"), "export {}").unwrap();

        let output_path = temp.path().join("SKILL.md");

        let result = run(
            "github".to_string(),
            Some(temp.path().to_path_buf()),
            Some(output_path.clone()),
            None,
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(
            result.is_err(),
            "drifted sidecar must fail instead of silently succeeding"
        );
        let err = result.unwrap_err();
        // `anyhow::Error`'s `Display` only shows the outer context; the full
        // chain (including the `ScanError::StaleMetadata` source) is in `{err:?}`.
        let message = format!("{err:?}");
        assert!(
            message.contains("create_issue") || message.contains("createIssue.ts"),
            "error must identify the tool/file with the missing .ts: {message}"
        );
        assert!(
            !output_path.exists(),
            "SKILL.md must not be written when the sidecar is stale"
        );
    }

    #[tokio::test]
    async fn test_run_path_traversal_server_id() {
        let temp = TempDir::new().unwrap();

        // Server ID validation should reject path traversal attempts
        let result = run(
            "../etc".to_string(),
            Some(temp.path().to_path_buf()),
            None,
            None,
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err());
        // Should fail at server ID validation (contains invalid chars)
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid server ID")
        );
    }
}
