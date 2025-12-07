//! Generate command implementation.
//!
//! Generates TypeScript files from MCP server tool definitions.
//! Supports two output formats:
//! - Progressive loading (one file per tool, for Claude Code)
//! - Claude Agent SDK (Zod schemas, for SDK integration)
//!
//! This command:
//! 1. Introspects the server to discover tools and schemas
//! 2. Generates TypeScript files in the selected format
//! 3. Saves files to the appropriate directory

use super::common::{build_server_config, load_server_from_config};
use crate::GeneratorFormat;
use anyhow::{Context, Result};
use mcp_codegen::claude_agent::ClaudeAgentGenerator;
use mcp_codegen::progressive::ProgressiveGenerator;
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_files::FilesBuilder;
use mcp_introspector::Introspector;
use serde::Serialize;
use std::path::PathBuf;
use tracing::{info, warn};

/// Result of code generation.
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
    /// Generator format used
    format: String,
}

/// Runs the generate command.
///
/// Generates TypeScript files from an MCP server.
///
/// This command performs the following steps:
/// 1. Builds `ServerConfig` from CLI arguments or loads from ~/.claude/mcp.json
/// 2. Introspects the MCP server to discover tools
/// 3. Generates TypeScript files in the selected format
/// 4. Exports files to the appropriate directory
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
/// * `output_dir` - Custom output directory
/// * `generator_format` - Code generation format (progressive or claude-agent)
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
/// - File export fails
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
    generator_format: GeneratorFormat,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    // Build server config: either from mcp.json or from CLI arguments
    let (server_id, server_config) = if let Some(config_name) = from_config {
        info!(
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
        server_info.id = mcp_core::ServerId::new(custom_name);
    }

    // Determine server directory name (use custom name if provided, otherwise server_id)
    let server_dir_name = server_info.id.to_string();

    // Generate code based on selected format
    let (generated_code, format_name, default_subdir) = match generator_format {
        GeneratorFormat::Progressive => {
            let generator =
                ProgressiveGenerator::new().context("failed to create progressive generator")?;
            let code = generator
                .generate(&server_info)
                .context("failed to generate progressive TypeScript code")?;
            info!(
                "Generated {} files for progressive loading",
                code.file_count()
            );
            (code, "progressive", "servers")
        }
        GeneratorFormat::ClaudeAgent => {
            let generator = ClaudeAgentGenerator::new()
                .context("failed to create Claude Agent SDK generator")?;
            let code = generator
                .generate(&server_info)
                .context("failed to generate Claude Agent SDK code")?;
            info!("Generated {} files for Claude Agent SDK", code.file_count());
            (code, "claude-agent", "agent-sdk")
        }
    };

    // Build VFS with generated code
    // Note: base_path should be "/" because generated files already have flat structure
    // The server_dir_name will be used when exporting to filesystem
    let vfs = FilesBuilder::from_generated_code(generated_code, "/")
        .build()
        .context("failed to build VFS")?;

    // Determine output directory
    // Always append server_dir_name to ensure proper structure
    let base_dir = if let Some(custom_dir) = output_dir {
        custom_dir
    } else {
        dirs::home_dir()
            .context("failed to get home directory")?
            .join(".claude")
            .join(default_subdir)
    };

    let output_path = base_dir.join(&server_dir_name);

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
        format: format_name.to_string(),
    };

    // Output result
    match output_format {
        OutputFormat::Json => {
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        OutputFormat::Text => {
            println!("Server: {} ({})", result.server_name, result.server_id);
            println!("Format: {}", result.format);
            println!("Generated {} tool files", result.tool_count);
            println!("Output: {}", result.output_path);
        }
        OutputFormat::Pretty => {
            let format_desc = match generator_format {
                GeneratorFormat::Progressive => "progressive loading",
                GeneratorFormat::ClaudeAgent => "Claude Agent SDK",
            };
            println!("✓ Successfully generated {format_desc} files");
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
    use mcp_core::ServerId;
    use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
    use serde_json::json;

    fn create_mock_server_info() -> ServerInfo {
        ServerInfo {
            id: ServerId::new("test-server"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![ToolInfo {
                name: mcp_core::ToolName::new("test_tool"),
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
            format: "progressive".to_string(),
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("\"server_id\":\"test\""));
        assert!(json.contains("\"tool_count\":5"));
        assert!(json.contains("\"format\":\"progressive\""));
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
    fn test_claude_agent_generator_creation() {
        let generator = ClaudeAgentGenerator::new();
        assert!(generator.is_ok());
    }

    #[test]
    fn test_claude_agent_code_generation() {
        let generator = ClaudeAgentGenerator::new().unwrap();
        let server_info = create_mock_server_info();

        let result = generator.generate(&server_info);
        assert!(result.is_ok());

        let code = result.unwrap();
        assert!(code.file_count() > 0);

        // Check that expected files are generated
        let paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();
        assert!(paths.contains(&"index.ts"));
        assert!(paths.contains(&"server.ts"));
        assert!(paths.iter().any(|p| p.starts_with("tools/")));
    }
}
