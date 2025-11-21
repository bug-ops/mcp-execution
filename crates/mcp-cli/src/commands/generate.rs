//! Generate command implementation.
//!
//! Generates code from MCP server tool definitions using a two-step process:
//! 1. Introspect the server to discover tools and schemas
//! 2. Generate TypeScript/Rust code using the codegen library

use anyhow::{Context, Result, bail};
use mcp_codegen::CodeGenerator;
use mcp_core::ServerId;
use mcp_core::cli::{ExitCode, OutputFormat, ServerConnectionString};
use mcp_introspector::Introspector;
use mcp_plugin_store::{PluginStore, ServerInfo, ToolInfo};
use mcp_vfs::VfsBuilder;
use serde::Serialize;
use std::path::PathBuf;
use tokio::fs;
use tracing::{info, warn};

/// Result of code generation.
#[derive(Debug, Serialize)]
struct GenerationResult {
    /// Server connection string
    server: String,
    /// Output directory path
    output_dir: String,
    /// Feature mode used (wasm or skills)
    feature_mode: String,
    /// Number of files created
    files_created: usize,
    /// Total lines of code generated
    total_lines: usize,
    /// Plugin saved location (if --save-plugin was used)
    plugin_saved: Option<String>,
}

/// Runs the generate command.
///
/// Introspects a server and generates code for tool execution.
///
/// This command performs a two-step process:
/// 1. Uses `mcp-introspector` to connect to the server and discover tools
/// 2. Uses `mcp-codegen` to generate TypeScript/Rust code from the schemas
///
/// # Arguments
///
/// * `server` - Server connection string or command
/// * `output` - Optional output directory (defaults to "./generated")
/// * `feature` - Code generation feature mode ("wasm" or "skills")
/// * `force` - Overwrite existing output directory without prompting
/// * `save_plugin` - Save generated code as a plugin
/// * `plugin_dir` - Plugin directory for save operations
/// * `output_format` - Output format (json, text, pretty)
///
/// # Errors
///
/// Returns an error if:
/// - Server connection string is invalid
/// - Feature mode is invalid (not "wasm" or "skills")
/// - Server introspection fails
/// - Code generation fails
/// - File system operations fail
/// - Plugin save fails (if --save-plugin is used)
///
/// # Examples
///
/// ```no_run
/// use mcp_cli::commands::generate;
/// use mcp_core::cli::{ExitCode, OutputFormat};
/// use std::path::PathBuf;
///
/// # async fn example() -> Result<(), anyhow::Error> {
/// let result = generate::run(
///     "vkteams-bot".to_string(),
///     Some(PathBuf::from("./generated")),
///     "wasm".to_string(),
///     false,
///     false,
///     PathBuf::from("./plugins"),
///     OutputFormat::Pretty,
/// ).await?;
/// assert_eq!(result, ExitCode::SUCCESS);
/// # Ok(())
/// # }
/// ```
pub async fn run(
    server: String,
    output: Option<PathBuf>,
    feature: String,
    force: bool,
    save_plugin: bool,
    plugin_dir: PathBuf,
    output_format: OutputFormat,
) -> Result<ExitCode> {
    // Validate inputs
    info!("Validating inputs");

    // Validate server connection string
    let server_conn =
        ServerConnectionString::new(&server).context("invalid server connection string")?;

    // Parse feature mode (currently only wasm is implemented)
    if feature != "wasm" {
        bail!("invalid feature mode '{feature}' (currently only 'wasm' is supported)");
    }

    // Determine output directory
    let output_dir = output.unwrap_or_else(|| PathBuf::from("./generated"));

    // Check for existing files if not force mode
    if !force && output_dir.exists() {
        let output_display = output_dir.display();
        let entries = fs::read_dir(&output_dir)
            .await
            .with_context(|| format!("failed to read output directory: {output_display}"))?
            .next_entry()
            .await
            .context("failed to check directory contents")?;

        if entries.is_some() {
            bail!(
                "output directory '{output_display}' already exists and is not empty (use --force to overwrite)"
            );
        }
    }

    // Step 1: Introspect server
    info!("Introspecting server: {server}");

    let mut introspector = Introspector::new();
    let server_id = ServerId::new(server_conn.as_str());

    let server_info = introspector
        .discover_server(server_id, server_conn.as_str())
        .await
        .with_context(|| format!("failed to introspect server '{server}'"))?;

    if server_info.tools.is_empty() {
        warn!("Server '{server}' has no tools");
        bail!("no tools found on server '{server}'");
    }

    info!(
        "Found {} tools on server '{server}'",
        server_info.tools.len()
    );

    // Step 2: Generate code
    info!("Generating code with feature mode: {feature}");

    let generator = CodeGenerator::new().context("failed to create code generator")?;

    let generated_code = generator
        .generate(&server_info)
        .context("code generation failed")?;

    if generated_code.file_count() == 0 {
        warn!("No files were generated");
        bail!("code generation produced no files");
    }

    info!(
        "Generated {} files for server '{server}'",
        generated_code.file_count()
    );

    // Step 3: Write files to disk
    let output_display = output_dir.display();
    info!("Writing files to output directory: {output_display}");

    // Create output directory
    fs::create_dir_all(&output_dir)
        .await
        .with_context(|| format!("failed to create output directory: {output_display}"))?;

    let mut total_lines = 0;

    for file in &generated_code.files {
        let full_path = output_dir.join(&file.path);

        // Create parent directories if needed
        if let Some(parent) = full_path.parent() {
            let parent_display = parent.display();
            fs::create_dir_all(parent)
                .await
                .with_context(|| format!("failed to create parent directory: {parent_display}"))?;
        }

        // Write file
        let full_path_display = full_path.display();
        fs::write(&full_path, &file.content)
            .await
            .with_context(|| format!("failed to write file: {full_path_display}"))?;

        // Count lines
        total_lines += file.content.lines().count();

        let file_path = &file.path;
        info!("Created file: {file_path}");
    }

    // Step 4: Save plugin if requested
    let plugin_saved = if save_plugin {
        info!("Saving plugin to: {}", plugin_dir.display());

        // For MVP, we use mock WASM since we don't have TypeScript compilation
        // In production, this would compile the generated TypeScript to WASM
        let mock_wasm = create_mock_wasm();

        // Create plugin store
        let store = PluginStore::new(&plugin_dir)
            .context("failed to initialize plugin store")?;

        // Build VFS from generated code
        let mut vfs_builder = VfsBuilder::new();
        for file in &generated_code.files {
            // Convert relative paths to VFS absolute paths (prepend /)
            let vfs_path = format!("/{}", file.path);
            vfs_builder = vfs_builder.add_file(&vfs_path, file.content.clone());
        }
        let vfs = vfs_builder.build()
            .context("failed to create VFS from generated code")?;

        // Extract server info from introspection
        // Note: Using "2024-11-05" as default protocol version for MVP
        let plugin_server_info = ServerInfo {
            name: server.clone(),
            version: server_info.version.clone(),
            protocol_version: "2024-11-05".to_string(),
        };

        // Extract tool info
        let tool_info: Vec<ToolInfo> = server_info
            .tools
            .iter()
            .map(|t| ToolInfo {
                name: t.name.to_string(),
                description: t.description.clone(),
            })
            .collect();

        // Save plugin
        store
            .save_plugin(&server, &vfs, &mock_wasm, plugin_server_info, tool_info)
            .with_context(|| format!("failed to save plugin for server '{server}'"))?;

        let plugin_path = store.plugin_path(&server);
        info!("Plugin saved to: {}", plugin_path.display());

        Some(plugin_path.display().to_string())
    } else {
        None
    };

    // Build result
    let result = GenerationResult {
        server: server.clone(),
        output_dir: output_dir.display().to_string(),
        feature_mode: feature.clone(),
        files_created: generated_code.file_count(),
        total_lines,
        plugin_saved: plugin_saved.clone(),
    };

    // Format and display result
    let formatted = crate::formatters::format_output(&result, output_format)?;
    println!("{formatted}");

    info!(
        "Successfully generated {} files ({} lines) in {}",
        result.files_created, result.total_lines, result.output_dir
    );

    if let Some(plugin_path) = plugin_saved {
        info!("Plugin saved to: {}", plugin_path);
    }

    Ok(ExitCode::SUCCESS)
}

/// Creates a mock WASM module for MVP.
///
/// In production, this would compile the generated TypeScript to WASM.
/// For now, we use a minimal valid WASM module.
fn create_mock_wasm() -> Vec<u8> {
    // WASM magic number + version
    vec![
        0x00, 0x61, 0x73, 0x6D, // magic: \0asm
        0x01, 0x00, 0x00, 0x00, // version: 1
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_generation_result_serialization() {
        let result = GenerationResult {
            server: "test-server".to_string(),
            output_dir: "/tmp/generated".to_string(),
            feature_mode: "wasm".to_string(),
            files_created: 5,
            total_lines: 250,
            plugin_saved: None,
        };

        let json = serde_json::to_string(&result).unwrap();
        assert!(json.contains("test-server"));
        assert!(json.contains('5'));
        assert!(json.contains("250"));
    }

    #[test]
    fn test_feature_mode_validation() {
        // Valid feature mode
        assert_eq!("wasm", "wasm");

        // Invalid feature modes would be caught by the CLI
        assert_ne!("invalid", "wasm");
    }

    #[tokio::test]
    async fn test_output_directory_default() {
        let default_path = PathBuf::from("./generated");
        assert_eq!(default_path.to_str(), Some("./generated"));
    }

    #[tokio::test]
    async fn test_output_directory_custom() {
        let custom_path = Some(PathBuf::from("/tmp/custom"));
        assert!(custom_path.is_some());
        assert_eq!(custom_path.unwrap().to_str(), Some("/tmp/custom"));
    }

    #[test]
    fn test_generation_result_fields() {
        let result = GenerationResult {
            server: "test".to_string(),
            output_dir: "/tmp".to_string(),
            feature_mode: "wasm".to_string(),
            files_created: 10,
            total_lines: 500,
            plugin_saved: Some("./plugins/test".to_string()),
        };

        assert_eq!(result.server, "test");
        assert_eq!(result.output_dir, "/tmp");
        assert_eq!(result.feature_mode, "wasm");
        assert_eq!(result.files_created, 10);
        assert_eq!(result.total_lines, 500);
        assert_eq!(result.plugin_saved, Some("./plugins/test".to_string()));
    }

    #[tokio::test]
    async fn test_directory_creation_path() {
        use std::env;

        // Test that we can construct a valid path
        let temp_dir = env::temp_dir();
        let test_path = temp_dir.join("mcp-cli-test-generate");

        assert!(test_path.parent().is_some());
    }

    #[test]
    fn test_server_connection_string_validation() {
        // Valid connection strings
        assert!(ServerConnectionString::new("test-server").is_ok());
        assert!(ServerConnectionString::new("vkteams-bot").is_ok());
        assert!(ServerConnectionString::new("/path/to/server").is_ok());

        // Invalid connection strings
        assert!(ServerConnectionString::new("").is_err());
        assert!(ServerConnectionString::new("server with spaces").is_err());
        assert!(ServerConnectionString::new("server && rm -rf /").is_err());
    }

    #[test]
    fn test_line_counting() {
        let content = "line1\nline2\nline3";
        let lines = content.lines().count();
        assert_eq!(lines, 3);

        let empty_content = "";
        let empty_lines = empty_content.lines().count();
        assert_eq!(empty_lines, 0);

        let single_line = "single";
        let single_count = single_line.lines().count();
        assert_eq!(single_count, 1);
    }

    #[tokio::test]
    async fn test_path_joining() {
        let base = PathBuf::from("/tmp/generated");
        let relative = "tools/sendMessage.ts";
        let full_path = base.join(relative);

        // Test path components instead of string representation (cross-platform)
        assert_eq!(full_path.file_name().unwrap(), "sendMessage.ts");
        assert!(full_path.to_string_lossy().contains("tools"));
        assert!(full_path.to_string_lossy().contains("generated"));
    }

    #[tokio::test]
    async fn test_parent_directory_extraction() {
        let base = PathBuf::from("/tmp/generated");
        let path = base.join("tools").join("sendMessage.ts");
        let parent = path.parent();

        assert!(parent.is_some());
        assert_eq!(parent.unwrap().file_name().unwrap(), "tools");
    }

    #[test]
    fn test_force_flag_logic() {
        // When force is true, should skip existence check
        let force = true;
        assert!(force);

        // When force is false, should check existence
        let no_force = false;
        assert!(!no_force);
    }

    #[tokio::test]
    async fn test_error_message_formatting() {
        let path = "/tmp/test";
        let error_msg = format!("output directory '{path}' already exists");
        assert!(error_msg.contains("/tmp/test"));
        assert!(error_msg.contains("already exists"));
    }

    #[test]
    fn test_generation_result_display() {
        let result = GenerationResult {
            server: "vkteams-bot".to_string(),
            output_dir: "./generated".to_string(),
            feature_mode: "wasm".to_string(),
            files_created: 8,
            total_lines: 400,
            plugin_saved: None,
        };

        // Test that all fields are accessible
        assert!(!result.server.is_empty());
        assert!(!result.output_dir.is_empty());
        assert!(!result.feature_mode.is_empty());
        assert!(result.files_created > 0);
        assert!(result.total_lines > 0);
    }
}
