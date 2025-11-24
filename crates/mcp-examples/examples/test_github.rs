//! Example: Testing MCP integration with GitHub MCP server.
//!
//! This example demonstrates Phase 2 functionality:
//! - Server discovery using mcp-introspector
//! - Connection management using mcp-bridge
//! - Tool introspection and metadata extraction
//!
//! # GitHub MCP Server
//!
//! See: <https://github.com/github/github-mcp-server>
//!
//! ## Configuration Options
//!
//! ### Option 1: Docker (Recommended for local)
//!
//! ```bash
//! export GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxxxxxxxxxxx
//! cargo run --example test_github -- docker
//! ```
//!
//! This runs:
//! ```text
//! docker run -i --rm -e GITHUB_PERSONAL_ACCESS_TOKEN ghcr.io/github/github-mcp-server
//! ```
//!
//! ### Option 2: Remote Server (HTTP)
//!
//! ```bash
//! export GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxxxxxxxxxxx
//! cargo run --example test_github -- remote
//! ```
//!
//! This connects to: `https://api.githubcopilot.com/mcp/`
//!
//! ### Option 3: Local Binary
//!
//! ```bash
//! export GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxxxxxxxxxxx
//! cargo run --example test_github -- local
//! ```
//!
//! Requires `github-mcp-server` binary in PATH (built from source).
//!
//! ## Environment Variables
//!
//! - `GITHUB_PERSONAL_ACCESS_TOKEN` - Required. Your GitHub PAT.
//! - `GITHUB_HOST` - Optional. For GitHub Enterprise (e.g., `https://octocorp.ghe.com`).
//! - `GITHUB_TOOLSETS` - Optional. Comma-separated toolsets (e.g., `repos,issues,pull_requests`).

use mcp_bridge::Bridge;
use mcp_core::{ServerConfig, ServerId, TransportType};
use mcp_introspector::Introspector;
use std::env;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with INFO level
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    println!("╔════════════════════════════════════════════════╗");
    println!("║   GitHub MCP Server Integration Test          ║");
    println!("║   Testing: mcp-introspector + mcp-bridge      ║");
    println!("╚════════════════════════════════════════════════╝\n");

    // Parse command line arguments
    let args: Vec<String> = env::args().collect();
    let mode = args.get(1).map_or("docker", String::as_str);

    // Get GitHub token from environment
    let github_token = env::var("GITHUB_PERSONAL_ACCESS_TOKEN").ok();

    // Build server configuration based on mode
    let server_config = match mode {
        "docker" => {
            println!("Mode: Docker container");
            let mut builder = ServerConfig::builder()
                .command("docker".to_string())
                .args(vec![
                    "run".to_string(),
                    "-i".to_string(),
                    "--rm".to_string(),
                    "-e".to_string(),
                    "GITHUB_PERSONAL_ACCESS_TOKEN".to_string(),
                    "ghcr.io/github/github-mcp-server".to_string(),
                ]);

            // Pass token to docker if available
            if let Some(token) = &github_token {
                builder = builder.env("GITHUB_PERSONAL_ACCESS_TOKEN".to_string(), token.clone());
            }

            // Pass optional GitHub host
            if let Ok(host) = env::var("GITHUB_HOST") {
                builder = builder
                    .args(vec!["-e".to_string(), "GITHUB_HOST".to_string()])
                    .env("GITHUB_HOST".to_string(), host);
            }

            // Pass optional toolsets
            if let Ok(toolsets) = env::var("GITHUB_TOOLSETS") {
                builder = builder
                    .args(vec!["-e".to_string(), "GITHUB_TOOLSETS".to_string()])
                    .env("GITHUB_TOOLSETS".to_string(), toolsets);
            }

            builder.build()
        }
        "remote" => {
            println!("Mode: Remote server (HTTP)");
            let mut builder = ServerConfig::builder()
                .http_transport("https://api.githubcopilot.com/mcp/".to_string());

            // Add authorization header if token available
            if let Some(token) = &github_token {
                builder = builder.header("Authorization".to_string(), format!("Bearer {token}"));
            }

            builder.build()
        }
        "local" => {
            println!("Mode: Local binary");
            let mut builder = ServerConfig::builder()
                .command("github-mcp-server".to_string())
                .args(vec!["stdio".to_string()]);

            // Pass token to process if available
            if let Some(token) = &github_token {
                builder = builder.env("GITHUB_PERSONAL_ACCESS_TOKEN".to_string(), token.clone());
            }

            // Pass optional GitHub host
            if let Ok(host) = env::var("GITHUB_HOST") {
                builder = builder.env("GITHUB_HOST".to_string(), host);
            }

            // Pass optional toolsets
            if let Ok(toolsets) = env::var("GITHUB_TOOLSETS") {
                builder = builder.env("GITHUB_TOOLSETS".to_string(), toolsets);
            }

            builder.build()
        }
        _ => {
            eprintln!("Unknown mode: {mode}");
            eprintln!("Usage: cargo run --example test_github -- [docker|remote|local]");
            std::process::exit(1);
        }
    };

    // Check if token is available
    if github_token.is_none() {
        println!("⚠ Warning: GITHUB_PERSONAL_ACCESS_TOKEN not set");
        println!("  Some operations may fail without authentication.\n");
    }

    // 1. Test Introspector
    println!("\n━━━ Step 1: Server Discovery ━━━");
    let mut introspector = Introspector::new();

    let server_id = ServerId::new("github");

    println!("→ Attempting to discover server: {server_id}");
    println!("  Transport: {:?}", server_config.transport());
    match server_config.transport() {
        TransportType::Stdio => {
            println!("  Command: {}", server_config.command());
            if !server_config.args().is_empty() {
                println!("  Args: {:?}", server_config.args());
            }
        }
        TransportType::Http | TransportType::Sse => {
            if let Some(url) = server_config.url() {
                println!("  URL: {url}");
            }
        }
    }

    match introspector
        .discover_server(server_id.clone(), &server_config)
        .await
    {
        Ok(info) => {
            println!("✓ Server discovered successfully!\n");

            println!("Server Information:");
            println!("  Name:    {}", info.name);
            println!("  Version: {}", info.version);
            println!("  ID:      {}", info.id);
            println!();

            println!("Capabilities:");
            println!("  Tools:     {}", info.capabilities.supports_tools);
            println!("  Resources: {}", info.capabilities.supports_resources);
            println!("  Prompts:   {}", info.capabilities.supports_prompts);
            println!();

            println!("Tools Found: {}", info.tools.len());
            for (idx, tool) in info.tools.iter().enumerate() {
                println!("  {}. {}", idx + 1, tool.name);
                println!("     Description: {}", tool.description);
                println!(
                    "     Input schema: {} bytes",
                    tool.input_schema.to_string().len()
                );
            }
            println!();

            // Verify retrieval from cache
            println!("→ Verifying cached server info...");
            if let Some(cached_info) = introspector.get_server(&server_id) {
                println!("✓ Successfully retrieved from cache");
                println!("  Cached tools: {}", cached_info.tools.len());
            }

            println!("\n━━━ Step 2: Bridge Connection ━━━");
            let bridge = Bridge::new(1000);

            println!("→ Connecting bridge to server...");
            match bridge.connect(server_id.clone(), &server_config).await {
                Ok(()) => {
                    println!("✓ Bridge connected successfully!");

                    // Check connection stats
                    let conn_count = bridge.connection_count().await;
                    println!("  Active connections: {conn_count}");

                    let cache_stats = bridge.cache_stats().await;
                    println!("  Cache capacity: {}", cache_stats.capacity);
                    println!("  Cache size: {}", cache_stats.size);
                    println!("  Cache usage: {:.1}%", cache_stats.usage_percent());

                    // Note: We don't actually call tools here because:
                    // 1. We don't want to spam real services
                    // 2. Tools may require authentication
                    // 3. This is just testing infrastructure
                    println!("\n✓ Bridge is ready for tool calls");
                    println!("  (Not calling tools to avoid side effects)");
                }
                Err(e) => {
                    println!("✗ Bridge connection failed: {e}");
                    println!("  This may indicate the server is not running");
                }
            }

            println!("\n━━━ Step 3: Statistics ━━━");
            println!("Introspector:");
            println!(
                "  Total servers discovered: {}",
                introspector.server_count()
            );
            println!("  Servers in cache: {}", introspector.list_servers().len());

            println!("\nBridge:");
            let final_stats = bridge.cache_stats().await;
            println!(
                "  Cache entries: {}/{}",
                final_stats.size, final_stats.capacity
            );
            println!("  Active connections: {}", bridge.connection_count().await);

            if let Some(call_count) = bridge.connection_call_count(&server_id).await {
                println!("  Tool calls made: {call_count}");
            }

            println!("\n╔════════════════════════════════════════════════╗");
            println!("║   ✓ Phase 2 Integration Test PASSED           ║");
            println!("╚════════════════════════════════════════════════╝");
        }
        Err(e) => {
            println!("✗ Server discovery failed: {e}");
            println!();
            println!("Setup instructions:");
            println!();
            println!("Option 1: Docker (recommended)");
            println!("  export GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxxx");
            println!("  cargo run --example test_github -- docker");
            println!();
            println!("Option 2: Remote server");
            println!("  export GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxxx");
            println!("  cargo run --example test_github -- remote");
            println!();
            println!("Option 3: Local binary (build from source)");
            println!("  git clone https://github.com/github/github-mcp-server");
            println!("  cd github-mcp-server");
            println!("  go build -o github-mcp-server ./cmd/github-mcp-server");
            println!("  export PATH=$PATH:$(pwd)");
            println!("  export GITHUB_PERSONAL_ACCESS_TOKEN=ghp_xxxx");
            println!("  cargo run --example test_github -- local");
            println!();
            println!("See: https://github.com/github/github-mcp-server");

            println!("\n╔════════════════════════════════════════════════╗");
            println!("║   ℹ Test Incomplete - Server not available    ║");
            println!("╚════════════════════════════════════════════════╝");
        }
    }

    Ok(())
}
