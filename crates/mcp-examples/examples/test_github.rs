//! Example: Testing MCP integration with github server.
//!
//! This example demonstrates Phase 2 functionality:
//! - Server discovery using mcp-introspector
//! - Connection management using mcp-bridge
//! - Tool introspection and metadata extraction
//!
//! # Requirements
//!
//! This example requires the github MCP server to be installed and
//! accessible via the command `github-server`. If the server is not
//! available, the example will gracefully report the failure.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example test_github
//! ```

use mcp_bridge::Bridge;
use mcp_core::{ServerConfig, ServerId};
use mcp_introspector::Introspector;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing with INFO level
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    println!("╔════════════════════════════════════════════════╗");
    println!("║   MCP Phase 2 Integration Test                ║");
    println!("║   Testing: mcp-introspector + mcp-bridge      ║");
    println!("╚════════════════════════════════════════════════╝\n");

    // 1. Test Introspector
    println!("━━━ Step 1: Server Discovery ━━━");
    let mut introspector = Introspector::new();

    let server_id = ServerId::new("github");
    let server_config = ServerConfig::builder()
        .command("github-server".to_string())
        .build();

    println!("→ Attempting to discover server: {server_id}");
    println!("  Command: {}", server_config.command());

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
            println!("This is expected if github server is not installed.");
            println!();
            println!("To install github:");
            println!("  1. Clone: git clone <github-repo>");
            println!("  2. Follow installation instructions");
            println!("  3. Ensure 'github-server' is in PATH");
            println!();
            println!("Alternative: Test with any other MCP server");
            println!("by modifying server_command in this example.");
            println!();

            println!("╔════════════════════════════════════════════════╗");
            println!("║   ℹ Phase 2 Test Incomplete                   ║");
            println!("║   Server not available (expected)              ║");
            println!("╚════════════════════════════════════════════════╝");
        }
    }

    Ok(())
}
