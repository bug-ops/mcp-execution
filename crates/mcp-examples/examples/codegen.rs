//! Code generation example.
//!
//! Demonstrates how to:
//! 1. Discover an MCP server using mcp-introspector
//! 2. Generate TypeScript code using mcp-codegen
//! 3. Write generated files to disk
//!
//! Usage:
//!   cargo run --example codegen -- <server-path>
//!
//! Example:
//!   cargo run --example codegen -- /usr/local/bin/github-server

use anyhow::{Context, Result};
use mcp_codegen::CodeGenerator;
use mcp_core::{ServerConfig, ServerId};
use mcp_introspector::Introspector;
use std::env;
use std::fs;
use std::path::PathBuf;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .with_target(false)
        .init();

    // Get server command from args
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: {} <server-command>", args[0]);
        eprintln!("\nExample:");
        eprintln!("  {} /usr/local/bin/github-server", args[0]);
        std::process::exit(1);
    }

    let server_command = &args[1];
    let server_id = ServerId::new("example-server");
    let server_config = ServerConfig::builder()
        .command(server_command.clone())
        .build();

    tracing::info!("Discovering MCP server: {}", server_command);

    // Step 1: Discover the server
    let mut introspector = Introspector::new();
    let server_info = introspector
        .discover_server(server_id.clone(), &server_config)
        .await
        .context("Failed to discover MCP server")?;

    tracing::info!(
        "Discovered server '{}' v{} with {} tools",
        server_info.name,
        server_info.version,
        server_info.tools.len()
    );

    for tool in &server_info.tools {
        tracing::info!("  - {}: {}", tool.name, tool.description);
    }

    // Step 2: Generate TypeScript code
    tracing::info!("Generating TypeScript code...");
    let generator = CodeGenerator::new().context("Failed to create code generator")?;

    let generated = generator
        .generate(&server_info)
        .context("Failed to generate code")?;

    tracing::info!("Generated {} files", generated.file_count());

    // Step 3: Write files to disk
    let output_dir = PathBuf::from("generated")
        .join("mcp-tools")
        .join("servers")
        .join(server_info.name);

    fs::create_dir_all(&output_dir).context("Failed to create output directory")?;

    for file in &generated.files {
        let file_path = output_dir.join(&file.path);

        // Create parent directories if needed
        if let Some(parent) = file_path.parent() {
            fs::create_dir_all(parent).context("Failed to create file directory")?;
        }

        // Write file
        fs::write(&file_path, &file.content).context("Failed to write file")?;

        tracing::info!("  ✓ {}", file.path);
    }

    tracing::info!(
        "\n✅ Code generation complete! Files written to: {}",
        output_dir.display()
    );

    Ok(())
}
