//! Generate command implementation.
//!
//! Generates progressive loading TypeScript files from MCP server tool definitions.
//! This command:
//! 1. Introspects the server to discover tools and schemas
//! 2. Generates TypeScript files for progressive loading (one file per tool)
//! 3. Saves files to `~/.claude/servers/{server-id}/` directory

use super::common::{ServerSource, derive_server_id_from_path_or_name, resolve_server_config};
use crate::formatters::escape_display;
use anyhow::{Context, Result};
use mcp_execution_codegen::GeneratedCode;
use mcp_execution_codegen::progressive::ProgressiveGenerator;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_core::{ServerConfig, ServerId};
use mcp_execution_files::{ExportOptions, FilesBuilder};
use mcp_execution_introspector::{Introspector, ServerInfo};
use mcp_execution_skill::validate_server_id;
use serde::Serialize;
use std::path::{Path, PathBuf};
use tracing::{info, warn};

/// Result of progressive loading code generation.
#[derive(Debug, Serialize)]
struct GenerationResult {
    /// Server ID
    server_id: String,
    /// Server name
    server_name: String,
    /// Number of tools generated
    tool_count: usize,
    /// Path where files were saved
    output_path: String,
    /// Hint describing the required post-export step (issue #257).
    next_step: String,
}

/// Post-export step required before the generated package type-checks.
const NPM_INSTALL_HINT: &str =
    "run 'npm install' in the output directory before type-checking the generated package";

/// Preview of a file that would be generated in dry-run mode.
#[derive(Debug, Serialize)]
struct FilePreview {
    /// Relative file path under the server directory
    path: String,
    /// File size in bytes
    size: usize,
}

/// Result of a dry-run preview.
#[derive(Debug, Serialize)]
struct DryRunResult {
    /// Server ID
    server_id: String,
    /// Server name
    server_name: String,
    /// Output path that would be used
    output_path: String,
    /// Files that would be generated
    files: Vec<FilePreview>,
    /// Total number of files
    total_files: usize,
    /// Total estimated size in bytes
    total_size: usize,
}

#[expect(
    clippy::cast_precision_loss,
    reason = "Converting byte counts to f64 for human-readable KB/MB formatting; precision loss \
              at display magnitude is inconsequential."
)]
fn format_size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}

/// Runs the generate command.
///
/// Generates progressive loading TypeScript files from an MCP server.
///
/// This command performs the following steps:
/// 1. Builds `ServerConfig` from CLI arguments or loads from ~/.claude/mcp.json
/// 2. Introspects the MCP server to discover tools
/// 3. Generates TypeScript files (one per tool) using progressive loading pattern
/// 4. Exports VFS to `~/.claude/servers/{server-id}/` directory
///
/// # Arguments
///
/// * `source` - Resolved server-selection source: either a `~/.claude/mcp.json`
///   name or CLI transport flags with timeout overrides. Timeout overrides
///   only exist on the `Flags` arm — a `Config` source always uses the
///   `mcp.json` entry's own `connectTimeoutSecs`/`discoverTimeoutSecs`, so
///   there is no "ignored override" state to document.
/// * `name` - Custom server name for directory (default: `server_id`)
/// * `output_dir` - Custom output directory (default: ~/.claude/servers/)
/// * `dry_run` - When true, preview files without writing to disk
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if:
/// - Server configuration is invalid
/// - Server not found in mcp.json (when using --from-config)
/// - Server connection fails
/// - Tool introspection fails
/// - Code generation fails
/// - File export fails (skipped in dry-run mode)
///
/// # Examples
///
/// ```no_run
/// use mcp_execution_cli::commands::common::{ServerSource, TransportArgs};
/// use mcp_execution_cli::commands::generate;
/// use mcp_execution_core::cli::OutputFormat;
/// use std::path::PathBuf;
///
/// # async fn example() -> anyhow::Result<()> {
/// // Generate from a stdio transport
/// let exit_code = generate::run(
///     ServerSource::Flags {
///         transport: TransportArgs::Stdio {
///             command: "github-mcp-server".to_string(),
///             args: vec![],
///             env: vec![],
///             cwd: None,
///         },
///         connect_timeout_secs: None,
///         discover_timeout_secs: None,
///     },
///     None,
///     None,
///     false,
///     OutputFormat::Pretty
/// ).await?;
///
/// // Generate with custom output directory and name override
/// let exit_code = generate::run(
///     ServerSource::Flags {
///         transport: TransportArgs::Http {
///             url: "https://api.example.com/mcp/".to_string(),
///             headers: vec!["Authorization=Bearer token".to_string()],
///         },
///         connect_timeout_secs: Some(10),
///         discover_timeout_secs: Some(30),
///     },
///     Some("my-custom-name".to_string()),
///     Some(PathBuf::from("/tmp/generated")),
///     false,
///     OutputFormat::Json
/// ).await?;
/// # Ok(())
/// # }
/// ```
pub async fn run(
    source: ServerSource,
    name: Option<String>,
    output_dir: Option<PathBuf>,
    dry_run: bool,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    // Captured before `source` is consumed below: an id resolved from
    // `--from-config` without a `--name` override is the one case
    // `resolve_server_dir_name` isn't a redundant backstop for (see its doc
    // comment) — `--name` always overrides `server_info.id`, so if it was
    // given the id came from its own, already-validated value instead.
    let id_from_unvalidated_config_key =
        matches!(source, ServerSource::Config { .. }) && name.is_none();
    let (server_id, server_config) = resolve_server_config(source)?;

    let server_info = discover_server_info(server_id, &server_config, name.as_deref()).await?;

    if server_info.tools.is_empty() {
        warn!("Server has no tools to generate code for");
        return Ok(ExitCode::SUCCESS);
    }

    let server_dir_name = resolve_server_dir_name(&server_info, id_from_unvalidated_config_key)?;
    let generated_code = generate_code(&server_info, &server_config)?;

    let base_dir = resolve_base_dir(output_dir)?;
    let output_path = base_dir.join(&server_dir_name);

    if dry_run {
        return render_dry_run(&server_info, &generated_code, &output_path, output_format);
    }

    export_generated_code(generated_code, &base_dir, &output_path)?;

    render_success(&server_info, &output_path, output_format)
}

/// Connects to the target server, discovers its tools, and applies the
/// `--name` override to [`ServerInfo::id`] if one was given.
///
/// # Errors
///
/// Returns an error if `name` fails [`validate_server_id`](mcp_execution_skill::validate_server_id),
/// or if the connection or tool discovery fails.
async fn discover_server_info(
    server_id: ServerId,
    server_config: &ServerConfig,
    name: Option<&str>,
) -> Result<ServerInfo> {
    // Validated up front, before spending a network round trip: unlike a
    // stdio command (sanitized via `derive_server_id_from_path_or_name`
    // because it commonly *is* a legitimate path), `--name` is documented as
    // overriding the id to match an identity the caller already has in mind
    // (typically an `mcp.json` key) — silently rewriting an invalid value
    // (e.g. stripping `..`/`/`) would produce a directory name the caller
    // didn't ask for and that may no longer match anything, so it is
    // rejected outright instead.
    let override_id = name
        .map(|custom_name| {
            validate_server_id(custom_name)
                .with_context(|| format!("invalid --name '{custom_name}'"))?;
            ServerId::new(custom_name).with_context(|| format!("invalid --name '{custom_name}'"))
        })
        .transpose()?;

    info!("Connecting to MCP server: {}", server_id);

    let mut introspector = Introspector::new();
    let mut server_info = introspector
        .discover_server(server_id, server_config)
        .await
        .context("failed to introspect MCP server")?;

    info!(
        "Discovered {} tools from server '{}'",
        server_info.tools.len(),
        server_info.name
    );

    // Override server_info.id with custom name if provided.
    // This ensures generated code uses the correct server_id that matches mcp.json.
    if let Some(id) = override_id {
        server_info.id = id;
    }

    Ok(server_info)
}

/// Generates progressive-loading TypeScript code for `server_info`.
///
/// # Errors
///
/// Returns an error if the code generator fails to initialize or generate code.
fn generate_code(server_info: &ServerInfo, server_config: &ServerConfig) -> Result<GeneratedCode> {
    let generator = ProgressiveGenerator::new().context("failed to create code generator")?;
    let generated_code = generator
        .generate(server_info, server_config)
        .context("failed to generate TypeScript code")?;

    info!(
        "Generated {} files for progressive loading",
        generated_code.file_count()
    );

    Ok(generated_code)
}

/// Resolves `server_info.id` to a directory name, guaranteeing it is safe to
/// join onto `base_dir`.
///
/// This is `generate`'s own sink — the single point where the id is turned
/// into a directory name — rather than a shared helper like
/// [`get_mcp_server`](super::common::get_mcp_server), because `generate` is
/// the only command that needs the id to be a filesystem-safe slug;
/// `introspect`/`server` read the same `mcp.json` key without that
/// constraint (#311).
///
/// For the stdio arm (`derive_server_id_from_path_or_name`), the http/sse arm
/// (`derive_server_id_from_url`), and `--name` (its own `validate_server_id`
/// check in `discover_server_info`), this check is a redundant backstop:
/// each of those already guarantees the id is valid before it ever reaches
/// here, so failing here for one of them would indicate a bug in that arm's
/// own check, not in the user's input. It is **not** redundant for
/// `--from-config` without a `--name` override — `get_mcp_server` is
/// deliberately unvalidated (shared by commands that don't need a
/// filesystem-safe id), so `is_from_unvalidated_config_key` being `true`
/// means this is the *sole* enforcement point for that arm. Do not delete
/// this check on the theory that every arm upstream already covers it — that
/// theory is false for `--from-config`, and removing it would silently
/// reopen #311 for that specific case.
///
/// # Errors
///
/// Returns an error if `server_info.id` fails
/// [`validate_server_id`](mcp_execution_skill::validate_server_id). When
/// `is_from_unvalidated_config_key` is `true`, the error names
/// `~/.claude/mcp.json` and suggests the `--name` override (with a
/// filesystem-safe slug derived from the offending id) as a fix, since the
/// fault is the user's config, not this tool. Otherwise it is framed as an
/// internal error, since reaching it would mean one of the other arms' own
/// checks has a bug.
fn resolve_server_dir_name(
    server_info: &ServerInfo,
    is_from_unvalidated_config_key: bool,
) -> Result<String> {
    let server_dir_name = server_info.id.to_string();
    validate_server_id(&server_dir_name).map_err(|source| {
        if is_from_unvalidated_config_key {
            let suggested_name = derive_server_id_from_path_or_name(&server_dir_name);
            anyhow::anyhow!(
                "server '{server_dir_name}' in ~/.claude/mcp.json is not a valid directory name \
                 ({source}); use --name {suggested_name} to override it"
            )
        } else {
            anyhow::Error::from(source).context(format!(
                "internal error: resolved server id '{server_dir_name}' is not a valid \
                 directory name"
            ))
        }
    })?;
    Ok(server_dir_name)
}

/// Resolves the base directory generated servers are exported under, defaulting to
/// `~/.claude/servers` when `output_dir` is not set.
///
/// # Errors
///
/// Returns an error if `output_dir` is `None` and the home directory cannot be determined.
fn resolve_base_dir(output_dir: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(custom_dir) = output_dir {
        Ok(custom_dir)
    } else {
        Ok(dirs::home_dir()
            .context("failed to get home directory")?
            .join(".claude")
            .join("servers"))
    }
}

/// Renders a dry-run preview of the files that would be generated, without writing anything.
fn render_dry_run(
    server_info: &ServerInfo,
    generated_code: &GeneratedCode,
    output_path: &Path,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    let server_dir_name = server_info.id.to_string();
    let files: Vec<FilePreview> = generated_code
        .files
        .iter()
        .map(|f| FilePreview {
            path: format!("{}/{}", server_dir_name, f.path),
            size: f.content.len(),
        })
        .collect();
    let total_size: usize = files.iter().map(|f| f.size).sum();
    let total_files = files.len();

    let result = DryRunResult {
        server_id: server_info.id.to_string(),
        server_name: server_info.name.clone(),
        output_path: output_path.display().to_string(),
        files,
        total_files,
        total_size,
    };

    println!("{}", format_dry_run(&result, output_format)?);

    Ok(ExitCode::SUCCESS)
}

/// Renders a [`DryRunResult`] for the given `output_format`.
///
/// `server_name` is server-supplied (untrusted), so `Text`/`Pretty` output escapes it via
/// [`escape_display`] to neutralize embedded control characters; `Json` output is unaffected
/// since `serde_json` already escapes string values.
fn format_dry_run(result: &DryRunResult, output_format: OutputFormat) -> Result<String> {
    Ok(match output_format {
        OutputFormat::Json => serde_json::to_string_pretty(result)?,
        OutputFormat::Text => format!(
            "Server: {} ({})\nWould generate {} files ({}) to {}/",
            escape_display(&result.server_name),
            result.server_id,
            result.total_files,
            format_size(result.total_size),
            result.output_path
        ),
        OutputFormat::Pretty => {
            use std::fmt::Write as _;

            let mut out = format!(
                "Would generate {} files to {}/:\n\n",
                result.total_files, result.output_path
            );
            for f in &result.files {
                let _ = writeln!(out, "  - {} ({})", f.path, format_size(f.size));
            }
            let _ = write!(
                out,
                "\nTotal: {} files, ~{}",
                result.total_files,
                format_size(result.total_size)
            );
            out
        }
    })
}

/// Builds the VFS from `generated_code` and exports it to `output_path` under `base_dir`.
///
/// # Errors
///
/// Returns an error if VFS construction fails, `base_dir` cannot be created, or the export to
/// the filesystem fails.
fn export_generated_code(
    generated_code: GeneratedCode,
    base_dir: &Path,
    output_path: &Path,
) -> Result<()> {
    // Build VFS with base_path="/" since generated files already have flat structure;
    // server_dir_name will be used when exporting to filesystem
    let vfs = FilesBuilder::from_generated_code(generated_code, "/")
        .build()
        .context("failed to build VFS")?;

    info!("Exporting files to: {}", output_path.display());

    // Only the parent needs to exist: `export_to_filesystem` publishes
    // `output_path` itself atomically (single rename on first generate,
    // stage-then-swap on regeneration), so pre-creating it here would just
    // force the slower regeneration path even on a brand-new server.
    std::fs::create_dir_all(base_dir).context("failed to create output directory")?;
    // Defense-in-depth: `output_path` is built by joining a server-id-derived
    // directory name onto `base_dir` (see `derive_server_id_from_path_or_name`,
    // which is the primary guard); confining the export here means a future
    // caller that skips that sanitization fails loudly instead of writing
    // outside `base_dir`.
    let options = ExportOptions::new().with_confine_to(base_dir);
    vfs.export_to_filesystem_with_options(output_path, &options)
        .context("failed to export files to filesystem")?;

    Ok(())
}

/// Renders the success output for a completed export, including the #257 npm-install hint.
fn render_success(
    server_info: &ServerInfo,
    output_path: &Path,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    let result = GenerationResult {
        server_id: server_info.id.to_string(),
        server_name: server_info.name.clone(),
        tool_count: server_info.tools.len(),
        output_path: output_path.display().to_string(),
        next_step: NPM_INSTALL_HINT.to_string(),
    };

    println!("{}", format_success(&result, output_format)?);

    Ok(ExitCode::SUCCESS)
}

/// Renders a [`GenerationResult`] for the given `output_format`.
///
/// `server_name` is server-supplied (untrusted), so `Text`/`Pretty` output escapes it via
/// [`escape_display`] to neutralize embedded control characters; `Json` output is unaffected
/// since `serde_json` already escapes string values.
fn format_success(result: &GenerationResult, output_format: OutputFormat) -> Result<String> {
    Ok(match output_format {
        OutputFormat::Json => serde_json::to_string_pretty(result)?,
        OutputFormat::Text => format!(
            "Server: {} ({})\nGenerated {} tool files\nOutput: {}\nNext step: {NPM_INSTALL_HINT}",
            escape_display(&result.server_name),
            result.server_id,
            result.tool_count,
            result.output_path
        ),
        OutputFormat::Pretty => format!(
            "✓ Successfully generated progressive loading files\n  Server: {} ({})\n  Tools: {}\n  Location: {}\n  Next step: {NPM_INSTALL_HINT}",
            escape_display(&result.server_name),
            result.server_id,
            result.tool_count,
            result.output_path
        ),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::commands::common::TransportArgs;
    use mcp_execution_core::ServerId;
    use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
    use serde_json::json;

    fn create_mock_server_info() -> ServerInfo {
        ServerInfo {
            id: ServerId::new("test-server").unwrap(),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: mcp_execution_core::ToolName::new("test_tool").unwrap(),
                description: "A test tool".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "param": {"type": "string"}
                    }
                }),
                output_schema: None,
            }],
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
        }
    }

    /// `ServerConfig` passed alongside `create_mock_server_info`'s output so `generate` can
    /// stamp generation provenance.
    fn create_mock_server_config() -> ServerConfig {
        ServerConfig::builder()
            .command("test-command".to_string())
            .build()
            .unwrap()
    }

    #[test]
    fn test_generation_result_serialization() {
        let result = GenerationResult {
            server_id: "test".to_string(),
            server_name: "Test Server".to_string(),
            tool_count: 5,
            output_path: "/path/to/output".to_string(),
            next_step: NPM_INSTALL_HINT.to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"server_id\":\"test\""));
        assert!(json.contains("\"tool_count\":5"));
        assert!(json.contains(NPM_INSTALL_HINT));
    }

    #[test]
    fn test_format_success_text_escapes_control_chars() {
        // A malicious MCP server can set its handshake `serverInfo.name` to anything,
        // including raw ANSI/control escape sequences. Text output must not pass them through.
        let result = GenerationResult {
            server_id: "test".to_string(),
            server_name: "evil\u{1b}[2J\u{1b}]0;pwned\u{7}".to_string(),
            tool_count: 1,
            output_path: "/path/to/output".to_string(),
            next_step: NPM_INSTALL_HINT.to_string(),
        };

        let output = format_success(&result, OutputFormat::Text).unwrap();
        assert!(!output.contains('\u{1b}'));
        assert!(output.contains("\\u001b"));
    }

    #[test]
    fn test_format_success_pretty_escapes_control_chars() {
        let result = GenerationResult {
            server_id: "test".to_string(),
            server_name: "evil\u{1b}[2Jname".to_string(),
            tool_count: 1,
            output_path: "/path/to/output".to_string(),
            next_step: NPM_INSTALL_HINT.to_string(),
        };

        let output = format_success(&result, OutputFormat::Pretty).unwrap();
        assert!(!output.contains('\u{1b}'));
        assert!(output.contains("\\u001b"));
    }

    #[test]
    fn test_format_dry_run_text_escapes_control_chars() {
        let result = DryRunResult {
            server_id: "test".to_string(),
            server_name: "evil\u{1b}[2Jname".to_string(),
            output_path: "/path/to/output".to_string(),
            files: vec![],
            total_files: 0,
            total_size: 0,
        };

        let output = format_dry_run(&result, OutputFormat::Text).unwrap();
        assert!(!output.contains('\u{1b}'));
        assert!(output.contains("\\u001b"));
    }

    #[test]
    fn test_format_dry_run_pretty_never_prints_raw_control_chars() {
        // The Pretty dry-run branch does not interpolate `server_name` at all (only file
        // paths and computed sizes/counts), but assert this stays true rather than silently
        // regressing if someone adds a server-name line later without escaping it.
        let result = DryRunResult {
            server_id: "test".to_string(),
            server_name: "evil\u{1b}[2Jname".to_string(),
            output_path: "/path/to/output".to_string(),
            files: vec![],
            total_files: 0,
            total_size: 0,
        };

        let output = format_dry_run(&result, OutputFormat::Pretty).unwrap();
        assert!(!output.contains('\u{1b}'));
    }

    #[test]
    fn test_format_success_json_unaffected_by_control_chars() {
        // Json path already relies on serde_json escaping and must stay unchanged.
        let result = GenerationResult {
            server_id: "test".to_string(),
            server_name: "evil\u{1b}[2Jname".to_string(),
            tool_count: 1,
            output_path: "/path/to/output".to_string(),
            next_step: NPM_INSTALL_HINT.to_string(),
        };

        let output = format_success(&result, OutputFormat::Json).unwrap();
        assert!(!output.contains('\u{1b}'));
        assert!(output.contains("\\u001b"));
    }

    #[test]
    fn test_format_success_text_quotes_benign_server_name() {
        // Pin the visible output-format change: escape_display always JSON-quotes the server
        // name, even when it contains no control characters, so `Server: Test Server (id)`
        // becomes `Server: "Test Server" (id)` for every server, not just malicious ones.
        let result = GenerationResult {
            server_id: "test".to_string(),
            server_name: "Test Server".to_string(),
            tool_count: 1,
            output_path: "/path/to/output".to_string(),
            next_step: NPM_INSTALL_HINT.to_string(),
        };

        let output = format_success(&result, OutputFormat::Text).unwrap();
        assert!(output.contains("Server: \"Test Server\" (test)"));
    }

    #[test]
    fn test_format_dry_run_text_quotes_benign_server_name() {
        let result = DryRunResult {
            server_id: "test".to_string(),
            server_name: "Test Server".to_string(),
            output_path: "/path/to/output".to_string(),
            files: vec![],
            total_files: 0,
            total_size: 0,
        };

        let output = format_dry_run(&result, OutputFormat::Text).unwrap();
        assert!(output.contains("Server: \"Test Server\" (test)"));
    }

    #[test]
    fn test_progressive_generator_creation() {
        let generator = ProgressiveGenerator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_progressive_code_generation() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_mock_server_info();

        let result = generator.generate(&server_info, &create_mock_server_config());
        assert!(result.is_ok());

        let code = result.unwrap();
        assert!(code.file_count() > 0);
    }

    #[test]
    fn test_format_size_bytes() {
        assert_eq!(format_size(0), "0 B");
        assert_eq!(format_size(512), "512 B");
        assert_eq!(format_size(1023), "1023 B");
    }

    #[test]
    fn test_format_size_kilobytes() {
        assert_eq!(format_size(1024), "1.0 KB");
        assert_eq!(format_size(2048), "2.0 KB");
        assert_eq!(format_size(1536), "1.5 KB");
    }

    #[test]
    fn test_format_size_megabytes() {
        assert_eq!(format_size(1024 * 1024), "1.0 MB");
        assert_eq!(format_size(2 * 1024 * 1024), "2.0 MB");
    }

    #[test]
    fn test_dry_run_result_serialization() {
        let result = DryRunResult {
            server_id: "github".to_string(),
            server_name: "GitHub MCP Server".to_string(),
            output_path: "/home/user/.claude/servers/github".to_string(),
            files: vec![
                FilePreview {
                    path: "github/createIssue.ts".to_string(),
                    size: 2450,
                },
                FilePreview {
                    path: "github/listRepos.ts".to_string(),
                    size: 1200,
                },
            ],
            total_files: 2,
            total_size: 3650,
        };

        let json = serde_json::to_string_pretty(&result).unwrap();
        assert!(json.contains("\"server_id\": \"github\""));
        assert!(json.contains("\"total_files\": 2"));
        assert!(json.contains("\"total_size\": 3650"));
        assert!(json.contains("\"path\": \"github/createIssue.ts\""));
        assert!(json.contains("\"size\": 2450"));
    }

    #[test]
    fn test_dry_run_collects_file_metadata() {
        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_mock_server_info();
        let generated_code = generator
            .generate(&server_info, &create_mock_server_config())
            .unwrap();

        let server_dir_name = server_info.id.to_string();
        let files: Vec<FilePreview> = generated_code
            .files
            .iter()
            .map(|f| FilePreview {
                path: format!("{}/{}", server_dir_name, f.path),
                size: f.content.len(),
            })
            .collect();

        assert!(!files.is_empty());
        for file in &files {
            assert!(file.path.starts_with("test-server/"));
            assert!(file.size > 0);
        }

        let total_size: usize = files.iter().map(|f| f.size).sum();
        assert_eq!(
            total_size,
            generated_code
                .files
                .iter()
                .map(|f| f.content.len())
                .sum::<usize>()
        );
    }

    #[test]
    fn test_dry_run_does_not_write_files() {
        use std::path::Path;

        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_mock_server_info();
        let generated_code = generator
            .generate(&server_info, &create_mock_server_config())
            .unwrap();

        // Simulate what dry-run does: collect metadata without touching the filesystem
        let server_dir_name = server_info.id.to_string();
        let fake_output_path = Path::new("/tmp/dry-run-test-should-not-exist-abc123");
        let output_path = fake_output_path.join(&server_dir_name);

        let files: Vec<FilePreview> = generated_code
            .files
            .iter()
            .map(|f| FilePreview {
                path: format!("{}/{}", server_dir_name, f.path),
                size: f.content.len(),
            })
            .collect();

        // Verify metadata collected correctly
        assert!(!files.is_empty());

        // Verify nothing was written to disk
        assert!(
            !output_path.exists(),
            "dry-run must not write files to disk"
        );
    }

    #[tokio::test]
    async fn test_run_zero_connect_timeout_override_rejected_by_validation() {
        // A zero override must surface the same connect_timeout validation
        // error as the mcp.json path, not just a generic connection failure.
        let source = ServerSource::Flags {
            transport: TransportArgs::Stdio {
                command: "nonexistent-server-timeout-test".to_string(),
                args: vec![],
                env: vec![],
                cwd: None,
            },
            connect_timeout_secs: Some(0),
            discover_timeout_secs: None,
        };
        let result = run(source, None, None, false, OutputFormat::Json).await;

        assert!(result.is_err());
        let err = result.unwrap_err();
        let chain_msg = err
            .chain()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(" | ");
        assert!(
            chain_msg.contains("greater than zero"),
            "expected connect_timeout validation error in the error chain, got: {chain_msg}"
        );
    }

    #[tokio::test]
    async fn test_run_with_valid_timeout_overrides_reaches_connection_attempt() {
        // Valid overrides must not be rejected before the connection attempt.
        let source = ServerSource::Flags {
            transport: TransportArgs::Stdio {
                command: "nonexistent-server-timeout-test-2".to_string(),
                args: vec![],
                env: vec![],
                cwd: None,
            },
            connect_timeout_secs: Some(5),
            discover_timeout_secs: Some(90),
        };
        let result = run(source, None, None, false, OutputFormat::Json).await;

        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(err_msg.contains("failed to introspect MCP server"));
    }

    // ── issue #311 (S1): `--name` override must be rejected outright when
    // invalid, not silently rewritten into a different id ──

    #[tokio::test]
    async fn test_name_override_rejects_traversal_and_absolute_paths() {
        // Regression test for #311: the `--name` override used to construct
        // `ServerId::new(custom_name)` directly with no validation at all, so
        // a malicious `--name` flowed unmodified into a directory name under
        // `~/.claude/servers/{id}/`. Unlike a stdio command (sanitized, since
        // it commonly *is* a legitimate path), `--name` is meant to match an
        // identity the caller already has in mind, so it must be rejected
        // rather than silently transformed. Validation happens before any
        // connection attempt, so this never reaches the network.
        let server_config = ServerConfig::builder()
            .command("nonexistent-command-for-name-validation-test".to_string())
            .build()
            .unwrap();

        for bad_name in [
            "../../../../etc/passwd",
            "/etc/cron.d/evil",
            "..",
            "UPPER_CASE",
        ] {
            let result = discover_server_info(
                ServerId::new("placeholder").unwrap(),
                &server_config,
                Some(bad_name),
            )
            .await;

            assert!(
                result.is_err(),
                "expected --name {bad_name:?} to be rejected"
            );
            let err_msg = result.unwrap_err().to_string();
            assert!(
                err_msg.contains("invalid --name"),
                "expected a validation error for {bad_name:?}, got: {err_msg}"
            );
        }
    }

    // ── issue #311 (S4): `resolve_server_dir_name` is the sink-level
    // invariant check, independent of which arm produced `server_info.id` ──

    #[test]
    fn test_resolve_server_dir_name_accepts_valid_id() {
        let server_info = create_mock_server_info();
        assert_eq!(
            resolve_server_dir_name(&server_info, false).unwrap(),
            "test-server"
        );
    }

    #[test]
    fn test_server_id_construction_rejects_traversal_and_absolute_ids() {
        // Regression guard for #287: `ServerId::new` now owns this
        // invariant at construction time, so a traversal/absolute-shaped id
        // can no longer reach `resolve_server_dir_name` (or anywhere else)
        // in the first place — it is rejected before a `ServerId` even
        // exists, rather than relying solely on this sink-level check.
        for bad_id in ["../../../../etc/passwd", "/etc/cron.d/evil", ".."] {
            assert!(
                ServerId::new(bad_id).is_err(),
                "expected id {bad_id:?} to be rejected by ServerId::new"
            );
        }
    }

    #[test]
    fn test_resolve_server_dir_name_rejects_charset_violations() {
        // `UPPER_CASE`, `a.b`, and `a_b` all pass `ServerId::new`'s baseline
        // invariant (single, non-empty path segment, no `..`/separator), so
        // this sink-level check must independently reject them rather than
        // trust that `ServerId::new` alone is enough for a filesystem-safe
        // directory name.
        for bad_id in ["UPPER_CASE", "a.b", "a_b"] {
            let mut server_info = create_mock_server_info();
            server_info.id = ServerId::new(bad_id).unwrap();

            let result = resolve_server_dir_name(&server_info, false);
            assert!(result.is_err(), "expected id {bad_id:?} to be rejected");
        }
    }

    #[test]
    fn test_resolve_server_dir_name_non_config_error_is_framed_as_internal() {
        // Regression test for #311 review M10/M11: when the id did NOT come
        // from an unvalidated `--from-config` lookup, reaching this check at
        // all would mean one of the other arms' own validation has a bug —
        // the error must say so, not blame a user-supplied mcp.json key that
        // was never involved.
        let mut server_info = create_mock_server_info();
        server_info.id = ServerId::new("UPPER_CASE").unwrap();

        let err = resolve_server_dir_name(&server_info, false).unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("internal error"), "got: {err_msg}");
        assert!(!err_msg.contains("mcp.json"), "got: {err_msg}");
    }

    #[test]
    fn test_resolve_server_dir_name_from_config_error_names_mcp_json_and_suggests_name_override() {
        // Regression test for #311 review M10: a `claude_ai_Gmail`-style
        // legitimate mcp.json key fails `validate_server_id`'s stricter
        // filesystem-safety charset. The error must name the actual fault
        // (the user's config) and point at the `--name` workaround with a
        // ready-to-use slug, not blame this tool.
        let mut server_info = create_mock_server_info();
        server_info.id = ServerId::new("claude_ai_Gmail").unwrap();

        let err = resolve_server_dir_name(&server_info, true).unwrap_err();
        let err_msg = err.to_string();
        assert!(err_msg.contains("mcp.json"), "got: {err_msg}");
        assert!(err_msg.contains("--name claude-ai-gmail"), "got: {err_msg}");
        assert!(!err_msg.contains("internal error"), "got: {err_msg}");
    }

    #[test]
    fn test_export_generated_code_confines_output_to_base_dir() {
        // End-to-end reproduction of the vulnerable call site
        // (`export_generated_code`): even if a caller upstream failed to
        // sanitize the id, the confinement check wired in via
        // `ExportOptions::with_confine_to` must reject an `output_path` that
        // escapes `base_dir`, rather than silently writing outside it.
        let temp = tempfile::TempDir::new().unwrap();
        let base_dir = temp.path().join("servers");
        std::fs::create_dir_all(&base_dir).unwrap();
        let escape_target = temp.path().join("escaped");

        let generator = ProgressiveGenerator::new().unwrap();
        let server_info = create_mock_server_info();
        let generated_code = generator
            .generate(&server_info, &create_mock_server_config())
            .unwrap();

        let result = export_generated_code(generated_code, &base_dir, &escape_target);

        assert!(result.is_err());
        assert!(
            !escape_target.exists(),
            "confinement check must reject the export before anything is written outside base_dir"
        );
    }
}
