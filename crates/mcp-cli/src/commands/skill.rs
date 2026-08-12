//! Skill command implementation.
//!
//! Generates Claude Code instruction skill files (SKILL.md) from progressive loading
//! TypeScript tools. This command:
//! 1. Scans generated TypeScript files in `~/.claude/servers/{server}/`
//! 2. Extracts tool metadata and categories
//! 3. Generates structured context for skill creation
//! 4. Returns a prompt for Claude to generate optimal SKILL.md content

use anyhow::{Context, Result, bail};
use mcp_execution_core::Error as CoreError;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_skill::{
    GenerateSkillResult, ParsedToolFile, ScanResult, build_skill_context, render_skill_md,
    scan_tools_directory, validate_server_id, validate_skill_name,
};
use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::{debug, info};

/// Output of a successful `skill` command invocation.
#[derive(Debug, Serialize)]
struct SkillWriteResult {
    success: bool,
    output_path: String,
    bytes_written: usize,
    tool_count: usize,
    /// Non-fatal drift warnings, e.g. `.ts` files excluded from generation
    /// because `_meta.json` has no matching entry for them (issue #161).
    warnings: Vec<String>,
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
/// # Side Effects
///
/// When no custom `--output` is given, resolving the confined default path creates and
/// confines the `{server}/` segment directory under the skills directory *before* the
/// `--overwrite` check and before rendering - unavoidably, since that resolution is what the
/// `--overwrite` check itself runs against. A refused (existing file, no `--overwrite`) or
/// otherwise failed run can therefore leave an empty `{server}/` directory behind - the same
/// side effect `save_skill`'s own default-path resolution has. A custom `--output` path has no
/// such side effect: its parent directory is created only after the `--overwrite` gate and
/// rendering succeed, matching this command's behavior before issue #501.
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
// One argument per CLI flag; clap already destructures flags for us, and grouping them into a
// struct would only benefit this function, not caller ergonomics.
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
    // A malformed `server` id is a CLI-argument mistake, so this is wrapped
    // as `CoreError::InvalidArgument` (rather than a bare anyhow string) to
    // classify as `ExitCode::INVALID_INPUT` in `runner::classify_exit_code`.
    validate_server_id(&server)
        .map_err(|e| CoreError::InvalidArgument(format!("Invalid server ID: {e}")))?;
    info!("Server ID validated: {}", server);

    let tool_dir = resolve_tool_dir(&server, servers_dir.as_deref())?;

    let scan_result = scan_server_tools(&tool_dir, &server).await?;

    let (context, custom_output_path) = prepare_skill_context(
        &server,
        &scan_result.tools,
        hints,
        skill_name.as_deref(),
        output_path,
    )?;

    // See this function's own doc comment (`# Side Effects`) for why the default branch below
    // creates `{server}/` this early, and why the custom branch's own directory creation is
    // deferred past the `--overwrite` gate instead of happening here.
    let had_custom_output_path = custom_output_path.is_some();
    let output_path = if let Some(path) = custom_output_path {
        // A custom `--output` path is a CLI operator's own flag, evaluated with the operator's
        // own filesystem permissions - confining it would not defend against anything a symlink
        // swapped in by a *racing local process* could not already do to any path this process
        // touches, so it keeps its existing narrower, traversal-only validation
        // (`validate_output_path`) instead of the confined walk below.
        path
    } else {
        // No `--output` override: confine and create the default `{server}` segment directory
        // the same way `save_skill`'s own default path is confined, rejecting it outright if it
        // already exists as a symlink - `write_skill_md`'s O_NOFOLLOW guard on its temp file
        // only protects that file's own terminal component, not an ancestor directory planted
        // as a symlink ahead of time (issue #501). See `resolve_default_output_path`'s own doc
        // comment for why this leaves the terminal `SKILL.md` component's pre-existing-symlink
        // case untouched.
        let skills_dir = resolve_skills_dir()?;
        resolve_default_output_path(&skills_dir, &server).await?
    };

    // Check if output file exists and overwrite flag
    if output_path.exists() && !overwrite {
        bail!(
            "Output file already exists: {}\n\
             Use --overwrite to replace existing file.",
            output_path.display()
        );
    }

    // Step 7: Render SKILL.md and write atomically.
    let rendered = render_skill_md(&context).context("failed to render SKILL.md template")?;

    // A custom `--output` path's parent directory is created only now, after the `--overwrite`
    // gate and rendering above - matching this branch's pre-#501 behavior, where directory
    // creation lived inside `write_skill_md` and so ran after both. Unlike the default branch,
    // nothing here needs to run earlier: `validate_output_path` already ran in
    // `prepare_skill_context`, and `output_path.exists()` above returns `false` regardless of
    // whether the parent exists yet, so deferring this creates no gate-ordering hazard.
    if had_custom_output_path && let Some(parent) = output_path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .with_context(|| format!("failed to create directory: {}", parent.display()))?;
    }

    write_skill_md(&rendered, &output_path).await?;

    let bytes_written = rendered.len();
    info!(
        "SKILL.md written to {} ({} bytes, {} tools)",
        output_path.display(),
        bytes_written,
        context.tool_count,
    );

    // Two independent, additive warning sources (issue #473): scan-time drift
    // (`scan_result.warnings`) and `use_case_hints` sanitization warnings already carried on
    // `context.warnings` (populated by `build_skill_context`, see its doc comment). Neither
    // overwrites the other.
    let mut warnings = scan_result.warnings;
    warnings.extend(context.warnings.iter().cloned());

    let result = SkillWriteResult {
        success: true,
        output_path: output_path.display().to_string(),
        bytes_written,
        tool_count: context.tool_count,
        warnings,
    };

    crate::formatters::emit(&result, output_format, ExitCode::SUCCESS)
}

/// Resolves and validates the server's tool directory under `servers_dir` (or its default).
///
/// # Errors
///
/// Returns an error if the home directory cannot be determined, the resolved path escapes
/// its base via a symlink, or the server directory does not exist.
fn resolve_tool_dir(server: &str, servers_dir: Option<&Path>) -> Result<PathBuf> {
    // Step 2: Resolve servers directory
    let servers_base = resolve_servers_dir(servers_dir)?;
    debug!("Servers base directory: {}", servers_base.display());

    // Step 3: Build and validate server path
    let tool_dir = servers_base.join(server);
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

    Ok(tool_dir)
}

/// Scans `tool_dir` for generated TypeScript tool files.
///
/// # Errors
///
/// Returns an error if the directory cannot be scanned or no tool files are found.
async fn scan_server_tools(tool_dir: &Path, server: &str) -> Result<ScanResult> {
    // Step 5: Scan TypeScript files
    info!("Scanning TypeScript files in {}", tool_dir.display());
    let scan_result = scan_tools_directory(tool_dir)
        .await
        .context("Failed to scan tools directory")?;

    if scan_result.tools.is_empty() {
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
    info!(
        "Verified {} tool files against sidecar",
        scan_result.tools.len()
    );

    Ok(scan_result)
}

/// Builds the skill generation context and validates a custom `--output` path, if the caller
/// supplied one.
///
/// Returns `Some(path)` only when `output_path` was supplied - traversal-validated, but
/// otherwise returned unchanged. `None` means the caller wants the default path; resolving that
/// confined default requires an async filesystem walk (`resolve_default_output_path`) this
/// function cannot perform itself, since it stays synchronous for its own unit tests, so `run`
/// resolves it separately instead of this function pre-computing a plain, unconfined join.
///
/// The returned path is kept separate from `GenerateSkillResult` rather than written back into
/// its `default_output_path_hint` field: that field is a non-authoritative display hint
/// `build_skill_context` computes (see its doc comment), not a slot for this command's actual
/// write target — overwriting it here would resurrect the same field-reuse-across-semantics
/// pattern issue #436 eliminated from the MCP tool pair.
///
/// # Errors
///
/// Returns an error if a custom `output_path` fails traversal validation.
fn prepare_skill_context(
    server: &str,
    tools: &[ParsedToolFile],
    hints: Vec<String>,
    skill_name: Option<&str>,
    output_path: Option<PathBuf>,
) -> Result<(GenerateSkillResult, Option<PathBuf>)> {
    // Step 6: Build skill context. Custom skill name is validated up front (same pattern as
    // `validate_server_id` above) and passed into `build_skill_context` itself — not applied as
    // a post-hoc override — so an oversized name fails fast here instead of being rendered and
    // written to disk only for `extract_skill_metadata` to reject it later (issue #413), and so
    // the name is consistently reflected in `generation_prompt` as well as `skill_name` (issue
    // #435).
    let hints_ref: Option<Vec<String>> = if hints.is_empty() { None } else { Some(hints) };

    if let Some(name) = skill_name {
        validate_skill_name(name)
            .map_err(|e| CoreError::InvalidArgument(format!("Invalid skill name: {e}")))?;
    }

    let context = build_skill_context(server, tools, hints_ref.as_deref(), skill_name);

    if let Some(path) = &output_path {
        validate_output_path(path)?;
    }

    Ok((context, output_path))
}

/// Writes `rendered` SKILL.md content to `output_path` atomically (write-temp then rename).
///
/// The caller is responsible for `output_path`'s parent directory already existing: the two call
/// sites create it themselves, with different guarantees (`resolve_default_output_path`'s
/// confined walk for the default path, a plain `create_dir_all` for a custom `--output` path) -
/// duplicating that step here would either weaken the default path's confinement or run it
/// twice.
///
/// # Errors
///
/// Returns an error if the temp file cannot be written or the rename fails.
async fn write_skill_md(rendered: &str, output_path: &Path) -> Result<()> {
    // Atomic write: the temp file itself is written through `write_confined_file`, which opens
    // it with `O_NOFOLLOW` on Unix, so a symlink pre-planted at the predictable `.tmp` path is
    // rejected instead of followed (issue #501) - the same primitive `save_skill` uses for the
    // equivalent race on its own write step (issue #496). The final `std::fs::rename` needs no
    // equivalent guard: it replaces whatever directory entry is at `output_path` rather than
    // following it, so a pre-existing symlink at `output_path` itself (e.g. a dotfiles setup
    // symlinking `SKILL.md` into a repo) is safely replaced rather than followed or rejected -
    // matching this function's pre-#501 behavior for the final path, on both the default and a
    // custom `--output` path.
    let tmp_path = output_path.with_added_extension("tmp");
    mcp_execution_core::write_confined_file(&tmp_path, rendered.as_bytes())
        .await
        .with_context(|| format!("failed to write temp file: {}", tmp_path.display()))?;
    std::fs::rename(&tmp_path, output_path)
        .with_context(|| format!("failed to rename to: {}", output_path.display()))?;

    Ok(())
}

/// Resolves and confines the default `SKILL.md` output path under `skills_dir`
/// (`skills_dir/{server}/SKILL.md`), creating and confining the `{server}` segment directory the
/// same way `save_skill`'s own default path is confined via `resolve_skill_output_path` -
/// rejecting it outright if it already exists as a symlink, regardless of where it points (issue
/// #217/#501). Deliberately does *not* use `resolve_skill_output_path` itself: that helper also
/// confinement-checks the terminal `SKILL.md` component and rejects it outright if it is already
/// a symlink, which would break the existing `skill --overwrite` dotfiles pattern (symlinking
/// `SKILL.md` into a repo) that `write_skill_md`'s `rename` already replaces safely. Calling
/// `resolve_confined_path` directly with `target: None` confines and creates only the segment
/// directory, leaving the terminal component's pre-existing-symlink case exactly as it was
/// before this function existed.
///
/// Only used when the caller did not supply a custom `--output` path; a custom path keeps its
/// existing narrower, traversal-only validation (`validate_output_path`) instead - see `run`'s
/// call site for why confining it would not add anything.
///
/// # Errors
///
/// Returns an error if the resolved path escapes `skills_dir` - including via a pre-existing
/// symlink at the server's own segment directory.
async fn resolve_default_output_path(skills_dir: &Path, server: &str) -> Result<PathBuf> {
    // `server` is already validated by `validate_server_id` (a `validate_server_id_slug`
    // passthrough) at `run`'s entry, before this is ever reached, so `resolve_confined_path`'s
    // own structural `validate_path_segment` check below is the only validation this call gets -
    // intentionally: re-running `validate_server_id_slug` here would just re-check what already
    // passed.
    let segment_dir =
        mcp_execution_core::resolve_confined_path(skills_dir, server, Path::new(""), None)
            .await
            .with_context(|| {
                format!("failed to resolve default skills directory for server: {server}")
            })?;

    Ok(segment_dir.join("SKILL.md"))
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
    // Check for path traversal in components (more robust than string check).
    // A traversal attempt is a malicious/invalid argument, so it is wrapped
    // as `CoreError::SecurityViolation` to classify as
    // `ExitCode::INVALID_INPUT` in `runner::classify_exit_code`.
    if has_path_traversal(path) {
        return Err(CoreError::SecurityViolation {
            reason: format!("path traversal detected: {}", path.display()),
        }
        .into());
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
        return Err(CoreError::SecurityViolation {
            reason: format!(
                "path {} is outside base directory {}",
                canonical_path.display(),
                canonical_base.display()
            ),
        }
        .into());
    }

    Ok(canonical_path)
}

/// Validate output path for path traversal attacks.
///
/// This only rejects traversal escapes (`..`), not absolute paths — callers must ensure `path`
/// originates from a trusted source (e.g. an interactive CLI operator's own flag), not
/// agent/LLM-supplied input. This is a narrower contract than
/// `mcp_execution_skill::output_path::relative_target`, which additionally rejects absolute
/// paths because it confines the MCP-server-exposed `save_skill` tool, reachable from
/// agent/LLM-supplied arguments.
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
        return Err(CoreError::SecurityViolation {
            reason: format!(
                "invalid output path (path traversal detected): {}",
                path.display()
            ),
        }
        .into());
    }
    Ok(())
}

/// Check if path contains traversal components.
///
/// Uses path component analysis instead of string matching for robustness.
fn has_path_traversal(path: &Path) -> bool {
    mcp_execution_core::contains_parent_dir(path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::formatters::format_output;
    use mcp_execution_core::metadata::{
        METADATA_FILE_NAME, METADATA_SCHEMA_VERSION, ParameterMetadata, ServerMetadata,
        ToolMetadata,
    };
    use mcp_execution_core::provenance::GenerationProvenance;
    use mcp_execution_core::{ServerConfig, ServerId, ToolName};
    use tempfile::TempDir;

    fn test_provenance() -> GenerationProvenance {
        let config = ServerConfig::builder()
            .command("test-command".to_string())
            .build()
            .unwrap();
        GenerationProvenance::capture(&config, &[])
    }

    /// Writes a minimal `_meta.json` sidecar with a single tool into `server_dir`,
    /// matching what `mcp-execution-codegen` would emit for a generated server.
    ///
    /// Also writes a matching stub `{typescript_name}.ts` file, since
    /// `scan_tools_directory` cross-checks the sidecar against files on disk.
    fn write_meta_sidecar(server_dir: &Path, server_id: &str, tool_name: &str) {
        let meta = ServerMetadata {
            schema_version: METADATA_SCHEMA_VERSION,
            server_id: ServerId::new(server_id).unwrap(),
            server_name: server_id.to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![ToolMetadata {
                name: ToolName::new(tool_name).unwrap(),
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
            provenance: test_provenance(),
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
        let err = result.unwrap_err();
        assert!(err.to_string().contains("traversal"));
        // Regression test for #195/S3: a path traversal attempt is a
        // malicious/invalid argument, so it must carry a `CoreError` that
        // `runner::classify_exit_code` maps to `ExitCode::INVALID_INPUT`.
        assert!(matches!(
            err.downcast_ref::<CoreError>(),
            Some(CoreError::SecurityViolation { .. })
        ));
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
        let err = result.unwrap_err();
        assert!(err.to_string().contains("Invalid server ID"));
        // Regression test for #195/S3: an invalid `server` id is a
        // CLI-argument mistake, so it must carry a `CoreError` that
        // `runner::classify_exit_code` maps to `ExitCode::INVALID_INPUT`.
        assert!(matches!(
            err.downcast_ref::<CoreError>(),
            Some(CoreError::InvalidArgument(_))
        ));
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

    /// A custom `--output` path whose parent directory does not exist yet must still have it
    /// created - `run`'s custom-path branch owns this now that `write_skill_md` no longer calls
    /// `create_dir_all` itself.
    #[tokio::test]
    async fn test_run_creates_nested_parent_directory_for_custom_output_path() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("test-server");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "test-server", "test_tool");

        let output_path = temp.path().join("nested").join("dir").join("SKILL.md");
        assert!(!output_path.parent().unwrap().exists());

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
    }

    #[tokio::test]
    async fn test_run_with_orphan_ts_file_succeeds() {
        // Issue #161: a `.ts` file not referenced by `_meta.json` remains
        // non-fatal (unlike a missing file, which is `ScanError::StaleMetadata`)
        // — `run` must still succeed and write SKILL.md.
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("test-server");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "test-server", "test_tool");
        std::fs::write(server_dir.join("orphanTool.ts"), "export {}").unwrap();

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
            "an orphaned .ts file must not fail the run: {:?}",
            result.err()
        );
        assert!(output_path.exists(), "SKILL.md must still be written");
    }

    #[test]
    fn test_skill_write_result_json_includes_warnings() {
        // Issue #161: the JSON output must name any excluded `.ts` file so a
        // caller relying only on `--format json` can detect the drift, since
        // it is no longer visible only via `tracing::warn!`.
        let result = SkillWriteResult {
            success: true,
            output_path: "/tmp/SKILL.md".to_string(),
            bytes_written: 42,
            tool_count: 1,
            warnings: vec![
                "'orphanTool.ts' is not referenced by _meta.json and was excluded from SKILL.md \
                 (re-run 'generate' to refresh the sidecar)"
                    .to_string(),
            ],
        };

        let output = format_output(&result, OutputFormat::Json).unwrap();

        assert!(
            output.contains("\"warnings\""),
            "JSON output must contain a warnings field: {output}"
        );
        assert!(
            output.contains("orphanTool.ts"),
            "warnings must name the excluded file: {output}"
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
            Some(output_path.clone()),
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

        // Issue #435: confirm the custom name actually landed in the written SKILL.md, not just
        // that the call reported success.
        let written = std::fs::read_to_string(&output_path).unwrap();
        assert!(
            written.contains("name: github-advanced"),
            "written SKILL.md must use the custom skill name: {written}"
        );
    }

    /// Issue #436: a custom `--skill-name` must be threaded into `build_skill_context` as a
    /// constructor input, not patched onto the result afterward — so `generation_prompt`
    /// reflects the requested name instead of the stale `{server}-progressive` default.
    /// `test_run_with_custom_skill_name` above only inspects the rendered `SKILL.md` file (which
    /// goes through `render_skill_md`, not `generation_prompt`); this test inspects
    /// `prepare_skill_context`'s returned `generation_prompt` directly.
    #[test]
    fn test_prepare_skill_context_with_custom_skill_name_reflects_it_in_generation_prompt() {
        let tools = vec![];

        let (context, _output_path) =
            prepare_skill_context("github", &tools, vec![], Some("github-advanced"), None).unwrap();

        assert_eq!(context.skill_name, "github-advanced");
        assert!(
            context.generation_prompt.contains("github-advanced"),
            "generation_prompt must reflect the custom skill_name, not the default: {}",
            context.generation_prompt
        );
        assert!(!context.generation_prompt.contains("github-progressive"));
    }

    /// Issue #436 (S1 follow-up): `prepare_skill_context`'s resolved output path must never be
    /// written back into `default_output_path_hint` — that field is a non-authoritative display
    /// hint, not this command's actual write target. Confirms the hint keeps its
    /// `build_skill_context`-computed default shape even when a custom `output_path` is
    /// supplied, and that the actual resolved path is returned separately.
    #[test]
    fn test_prepare_skill_context_does_not_overwrite_default_output_path_hint() {
        let tools = vec![];
        let custom_output = PathBuf::from("/tmp/custom/SKILL.md");

        let (context, resolved_output_path) =
            prepare_skill_context("github", &tools, vec![], None, Some(custom_output.clone()))
                .unwrap();

        assert_eq!(resolved_output_path, Some(custom_output));
        assert_eq!(
            context.default_output_path_hint, "~/.claude/skills/github/SKILL.md",
            "default_output_path_hint must stay build_skill_context's own default, not be \
             overwritten with the resolved write path"
        );
    }

    /// Critic finding S1 (issue #473 follow-up): hints dropped past `mcp_execution_skill::
    /// MAX_USE_CASE_HINTS` must not be silent. `build_skill_context` (called from
    /// `prepare_skill_context`) seeds `GenerateSkillResult::warnings` with a drop warning; `run`
    /// then merges it onto `SkillWriteResult.warnings` alongside `scan_result.warnings` (see
    /// `run`'s warning-merge comment) — the same channel `ScanResult::warnings` drift already
    /// surfaces on, so a caller inspecting `--format json` output sees both kinds of non-fatal
    /// data loss the same way.
    #[test]
    fn test_prepare_skill_context_surfaces_use_case_hint_cap_warning() {
        let tools = vec![];
        let hints: Vec<String> = (0..(mcp_execution_skill::types::MAX_USE_CASE_HINTS + 2))
            .map(|i| format!("hint-{i}"))
            .collect();

        let (context, _output_path) =
            prepare_skill_context("github", &tools, hints, None, None).unwrap();

        assert_eq!(context.warnings.len(), 1, "{:?}", context.warnings);
        assert!(
            context.warnings[0].contains("dropped"),
            "{:?}",
            context.warnings
        );
    }

    /// Issue #413: an oversized `--skill-name` must be rejected up front, before anything is
    /// rendered or written to disk — not left to fail later at `extract_skill_metadata`'s
    /// `MAX_FRONTMATTER_SIZE` check on a file that's already been written.
    #[tokio::test]
    async fn test_run_rejects_oversized_skill_name() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("github");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "github", "create_issue");

        let output_path = temp.path().join("SKILL.md");
        let oversized_name = "a".repeat(mcp_execution_skill::MAX_SKILL_NAME_LENGTH + 1);

        let result = run(
            "github".to_string(),
            Some(temp.path().to_path_buf()),
            Some(output_path.clone()),
            Some(oversized_name),
            vec![],
            false,
            OutputFormat::Json,
        )
        .await;

        assert!(result.is_err(), "oversized skill_name must be rejected");
        assert!(
            !output_path.exists(),
            "no SKILL.md should be written when skill_name validation fails"
        );
    }

    /// Issue #473: `--hint` must have a real, observable effect on the written SKILL.md, not
    /// just report success — before the fix, hints only reached the LLM-facing
    /// `generation_prompt`, which the CLI never uses (it renders `render_skill_md` directly),
    /// so a hint-bearing run and a hint-less run produced byte-identical output.
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
            Some(output_path.clone()),
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

        let written = std::fs::read_to_string(&output_path).unwrap();
        assert!(
            written.contains("## Use Cases"),
            "written SKILL.md must include a Use Cases section: {written}"
        );
        assert!(written.contains("code review"), "{written}");
        assert!(written.contains("CI/CD"), "{written}");
    }

    /// Sibling of `test_run_with_hints`: no `--hint` supplied must produce a written SKILL.md
    /// with no "Use Cases" section at all, confirming the fix does not force the section to
    /// always render.
    #[tokio::test]
    async fn test_run_without_hints_omits_use_cases_section() {
        let temp = TempDir::new().unwrap();
        let server_dir = temp.path().join("github");
        std::fs::create_dir(&server_dir).unwrap();
        write_meta_sidecar(&server_dir, "github", "list_prs");

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
            result.is_ok(),
            "Expected success but got: {:?}",
            result.err()
        );

        let written = std::fs::read_to_string(&output_path).unwrap();
        assert!(
            !written.contains("## Use Cases"),
            "written SKILL.md must not have a Use Cases section without --hint: {written}"
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
            server_id: ServerId::new("github").unwrap(),
            server_name: "GitHub".to_string(),
            server_version: "1.0.0".to_string(),
            tools: vec![
                ToolMetadata {
                    name: ToolName::new("create_issue").unwrap(),
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
                    name: ToolName::new("list_repos").unwrap(),
                    typescript_name: "listRepos".to_string(),
                    category: Some("repos".to_string()),
                    keywords: vec!["list".to_string()],
                    description: Some("List repos".to_string()),
                    parameters: vec![],
                },
            ],
            provenance: test_provenance(),
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

    #[tokio::test]
    async fn test_write_skill_md_writes_content() {
        let base = TempDir::new().unwrap();
        let output_path = base.path().join("SKILL.md");

        write_skill_md("rendered content", &output_path)
            .await
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(&output_path).unwrap(),
            "rendered content"
        );
        // The temp file must not survive a successful write.
        assert!(!output_path.with_added_extension("tmp").exists());
    }

    /// `write_skill_md` must preserve the overwrite semantics `run`'s own `--overwrite` gate
    /// relies on: a pre-existing regular `SKILL.md` is replaced with the new content, not
    /// rejected or merged with the old.
    #[tokio::test]
    async fn test_write_skill_md_overwrites_existing_regular_file() {
        let base = TempDir::new().unwrap();
        let output_path = base.path().join("SKILL.md");
        std::fs::write(&output_path, "old content").unwrap();

        write_skill_md("new content", &output_path).await.unwrap();

        assert_eq!(
            std::fs::read_to_string(&output_path).unwrap(),
            "new content"
        );
    }

    /// Issue #501: `write_skill_md`'s actual vulnerability was a symlink planted at its
    /// predictable temp path (`SKILL.md.tmp`), not at the final `SKILL.md` path - `rename`
    /// already replaces whatever entry sits at the final path rather than following it, so a
    /// symlink planted there was never the bug. Plants the symlink at the `.tmp` path instead and
    /// asserts the write is rejected without ever touching the symlink's target, and that no
    /// half-written `SKILL.md` is left behind at the final path.
    #[tokio::test]
    #[cfg(unix)]
    async fn test_write_skill_md_rejects_symlink_planted_at_tmp_path() {
        let base = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("real.md");

        let output_path = base.path().join("SKILL.md");
        let tmp_path = output_path.with_added_extension("tmp");
        std::os::unix::fs::symlink(&outside_file, &tmp_path).unwrap();

        let result = write_skill_md("attacker-controlled", &output_path).await;

        assert!(result.is_err());
        assert!(!outside_file.exists());
        assert!(!output_path.exists());
    }

    #[tokio::test]
    async fn test_resolve_default_output_path_creates_and_confines_segment_directory() {
        let skills_dir = TempDir::new().unwrap();

        let resolved = resolve_default_output_path(skills_dir.path(), "my-server")
            .await
            .unwrap();

        let canonical_base = skills_dir.path().canonicalize().unwrap();
        assert_eq!(resolved, canonical_base.join("my-server").join("SKILL.md"));
        assert!(canonical_base.join("my-server").is_dir());
    }

    /// Issue #501 (S3): the default output path must be confined the same way `save_skill`'s
    /// own default path is - a symlink already planted at the `{server}` segment directory (e.g.
    /// by an earlier process with write access to `~/.claude/skills`) must be rejected outright,
    /// not followed by the parent-directory creation a plain `create_dir_all` would have done.
    #[tokio::test]
    #[cfg(unix)]
    async fn test_resolve_default_output_path_rejects_symlinked_segment_directory() {
        let skills_dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        std::os::unix::fs::symlink(outside.path(), skills_dir.path().join("evil-server")).unwrap();

        let err = resolve_default_output_path(skills_dir.path(), "evil-server")
            .await
            .unwrap_err();

        // `{:#}` walks the full anyhow chain: `to_string()`/`{}` would only print the outer
        // `with_context` message, not the underlying `ConfinementError::SegmentIsSymlink` cause.
        assert!(format!("{err:#}").contains("symlink"), "{err:?}");
        assert!(!outside.path().join("SKILL.md").exists());
    }

    /// Preserves pre-#501 behavior by user decision: `resolve_default_output_path` confines only
    /// the `{server}` segment directory, not the terminal `SKILL.md` component, so a symlink
    /// already at `SKILL.md` itself (e.g. a dotfiles setup symlinking it into a repo) is left
    /// alone by resolution and then safely *replaced* by `write_skill_md`'s `rename` - not
    /// rejected the way a symlinked segment directory is.
    #[tokio::test]
    #[cfg(unix)]
    async fn test_default_path_symlinked_skill_md_is_replaced_not_rejected() {
        let skills_dir = TempDir::new().unwrap();
        let outside = TempDir::new().unwrap();
        let outside_file = outside.path().join("real.md");
        std::fs::write(&outside_file, "linked content").unwrap();

        let server_dir = skills_dir.path().join("my-server");
        std::fs::create_dir_all(&server_dir).unwrap();
        std::os::unix::fs::symlink(&outside_file, server_dir.join("SKILL.md")).unwrap();

        let output_path = resolve_default_output_path(skills_dir.path(), "my-server")
            .await
            .unwrap();
        write_skill_md("new content", &output_path).await.unwrap();

        assert!(!output_path.is_symlink());
        assert_eq!(
            std::fs::read_to_string(&output_path).unwrap(),
            "new content"
        );
        // The symlink's old target must be untouched - `rename` swaps the directory entry, it
        // never writes through the link.
        assert_eq!(
            std::fs::read_to_string(&outside_file).unwrap(),
            "linked content"
        );
    }
}
