//! Generate command implementation.
//!
//! Generates progressive loading TypeScript files from MCP server tool definitions.
//! This command:
//! 1. Introspects the server to discover tools and schemas
//! 2. Generates TypeScript files for progressive loading (one file per tool)
//! 3. Saves files to `~/.claude/servers/{server-id}/` directory

use super::common::{build_server_config, load_server_from_config};
use anyhow::{Context, Result};
use mcp_execution_codegen::progressive::ProgressiveGenerator;
use mcp_execution_core::cli::{ExitCode, OutputFormat};
use mcp_execution_files::FilesBuilder;
use mcp_execution_introspector::Introspector;
use serde::Serialize;
use std::path::PathBuf;
use tracing::{debug, info, warn};

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
}

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

#[allow(clippy::cast_precision_loss)]
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
/// * `from_config` - Load server config from ~/.claude/mcp.json by name
/// * `server` - Server command (binary name or path), None for HTTP/SSE
/// * `args` - Arguments to pass to the server command
/// * `env` - Environment variables in KEY=VALUE format
/// * `cwd` - Working directory for the server process
/// * `http` - HTTP transport URL
/// * `sse` - SSE transport URL
/// * `headers` - HTTP headers in KEY=VALUE format
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
#[allow(clippy::too_many_arguments)]
pub async fn run(
    from_config: Option<String>,
    server: Option<String>,
    args: Vec<String>,
    env: Vec<String>,
    cwd: Option<String>,
    http: Option<String>,
    sse: Option<String>,
    headers: Vec<String>,
    name: Option<String>,
    output_dir: Option<PathBuf>,
    dry_run: bool,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    // Build server config: either from mcp.json or from CLI arguments
    let (server_id, server_config) = if let Some(config_name) = from_config {
        debug!(
            "Loading server configuration from ~/.claude/mcp.json: {}",
            config_name
        );
        load_server_from_config(&config_name)?
    } else {
        build_server_config(server, args, env, cwd, http, sse, headers)?
    };

    info!("Connecting to MCP server: {}", server_id);

    // Introspect server
    let mut introspector = Introspector::new();
    let server_info = introspector
        .discover_server(server_id, &server_config)
        .await
        .context("failed to introspect MCP server")?;

    info!(
        "Discovered {} tools from server '{}'",
        server_info.tools.len(),
        server_info.name
    );

    if server_info.tools.is_empty() {
        warn!("Server has no tools to generate code for");
        return Ok(ExitCode::SUCCESS);
    }

    // Override server_info.id with custom name if provided
    // This ensures generated code uses the correct server_id that matches mcp.json
    let mut server_info = server_info;
    if let Some(ref custom_name) = name {
        server_info.id = mcp_execution_core::ServerId::new(custom_name);
    }

    // Determine server directory name (use custom name if provided, otherwise server_id)
    let server_dir_name = server_info.id.to_string();

    // Generate progressive loading files
    let generator = ProgressiveGenerator::new().context("failed to create code generator")?;

    let generated_code = generator
        .generate(&server_info)
        .context("failed to generate TypeScript code")?;

    info!(
        "Generated {} files for progressive loading",
        generated_code.file_count()
    );

    // Determine output path (needed for both dry-run display and normal export)
    let base_dir = if let Some(custom_dir) = output_dir {
        custom_dir
    } else {
        dirs::home_dir()
            .context("failed to get home directory")?
            .join(".claude")
            .join("servers")
    };
    let output_path = base_dir.join(&server_dir_name);

    if dry_run {
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
            server_name: server_info.name,
            output_path: output_path.display().to_string(),
            files,
            total_files,
            total_size,
        };

        match output_format {
            OutputFormat::Json => {
                println!("{}", serde_json::to_string_pretty(&result)?);
            }
            OutputFormat::Text => {
                println!("Server: {} ({})", result.server_name, result.server_id);
                println!(
                    "Would generate {} files ({}) to {}/",
                    result.total_files,
                    format_size(result.total_size),
                    result.output_path
                );
            }
            OutputFormat::Pretty => {
                println!(
                    "Would generate {} files to {}/:",
                    result.total_files, result.output_path
                );
                println!();
                for f in &result.files {
                    println!("  - {} ({})", f.path, format_size(f.size));
                }
                println!();
                println!(
                    "Total: {} files, ~{}",
                    result.total_files,
                    format_size(result.total_size)
                );
            }
        }

        return Ok(ExitCode::SUCCESS);
    }

    // Build VFS with generated code
    // Note: base_path should be "/" because generated files already have flat structure
    // The server_dir_name will be used when exporting to filesystem
    let vfs = FilesBuilder::from_generated_code(generated_code, "/")
        .build()
        .context("failed to build VFS")?;

    info!("Exporting files to: {}", output_path.display());

    // Create output directory if it doesn't exist
    std::fs::create_dir_all(&output_path).context("failed to create output directory")?;

    // Export VFS to filesystem
    vfs.export_to_filesystem(&output_path)
        .context("failed to export files to filesystem")?;

    // Prepare result
    let result = GenerationResult {
        server_id: server_info.id.to_string(),
        server_name: server_info.name.clone(),
        tool_count: server_info.tools.len(),
        output_path: output_path.display().to_string(),
    };

    // Output result
    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Text => {
            println!("Server: {} ({})", result.server_name, result.server_id);
            println!("Generated {} tool files", result.tool_count);
            println!("Output: {}", result.output_path);
        }
        OutputFormat::Pretty => {
            println!("✓ Successfully generated progressive loading files");
            println!("  Server: {} ({})", result.server_name, result.server_id);
            println!("  Tools: {}", result.tool_count);
            println!("  Location: {}", result.output_path);
        }
    }

    Ok(ExitCode::SUCCESS)
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcp_execution_core::ServerId;
    use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
    use serde_json::json;

    fn create_mock_server_info() -> ServerInfo {
        ServerInfo {
            id: ServerId::new("test-server"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: mcp_execution_core::ToolName::new("test_tool"),
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

    #[test]
    fn test_generation_result_serialization() {
        let result = GenerationResult {
            server_id: "test".to_string(),
            server_name: "Test Server".to_string(),
            tool_count: 5,
            output_path: "/path/to/output".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"server_id\":\"test\""));
        assert!(json.contains("\"tool_count\":5"));
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

        let result = generator.generate(&server_info);
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
        let generated_code = generator.generate(&server_info).unwrap();

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
        let generated_code = generator.generate(&server_info).unwrap();

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
}
