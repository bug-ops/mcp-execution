//! Demonstrates runtime statistics collection.
//!
//! Run with: `cargo run --example stats_demo`

use mcp_bridge::Bridge;
use mcp_wasm_runtime::{Runtime, security::SecurityConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .init();

    println!("=== MCP WASM Runtime Statistics Demo ===\n");

    // Create runtime
    let bridge = Bridge::new(1000);
    let config = SecurityConfig::default();
    let runtime = Runtime::new(Arc::new(bridge), config)?;

    // Initial stats (empty runtime)
    let stats = runtime.collect_stats();
    println!("Initial stats:");
    println!("  Total executions: {}", stats.total_executions);
    println!("  Execution failures: {}", stats.execution_failures);
    println!("  Cache hits: {}", stats.cache_hits);
    println!();

    // Create two different WASM modules
    let wat1 = r#"
        (module
            (func (export "main") (result i32)
                (i32.const 42)
            )
        )
    "#;

    let wat2 = r#"
        (module
            (func (export "main") (result i32)
                (i32.const 100)
            )
        )
    "#;

    let wasm1 = wat::parse_str(wat1)?;
    let wasm2 = wat::parse_str(wat2)?;

    // Execute first module
    println!("Executing module 1...");
    runtime.execute(&wasm1, "main", &[]).await?;

    let stats = runtime.collect_stats();
    println!("After 1 execution:");
    println!("  Total executions: {}", stats.total_executions);
    println!("  Cache hits: {}", stats.cache_hits);
    println!("  Avg execution time: {:?}", stats.avg_execution_time());
    println!();

    // Execute same module again (cache hit)
    println!("Executing module 1 again (cached)...");
    runtime.execute(&wasm1, "main", &[]).await?;

    // Execute second module
    println!("Executing module 2...");
    runtime.execute(&wasm2, "main", &[]).await?;

    let stats = runtime.collect_stats();
    println!("\nAfter 3 executions:");
    println!("  Total executions: {}", stats.total_executions);
    println!("  Cache hits: {}", stats.cache_hits);
    println!("  Execution failures: {}", stats.execution_failures);
    println!("  Compilation failures: {}", stats.compilation_failures);
    println!("  Avg execution time: {:?}", stats.avg_execution_time());

    if let Some(hit_rate) = stats.cache_hit_rate() {
        println!("  Cache hit rate: {:.1}%", hit_rate * 100.0);
    }

    if let Some(success_rate) = stats.execution_success_rate() {
        println!("  Execution success rate: {:.1}%", success_rate * 100.0);
    }

    // Try invalid WASM to demonstrate failure tracking
    println!("\nExecuting invalid WASM...");
    let invalid = vec![0x00, 0x01, 0x02, 0x03];
    let _ = runtime.execute(&invalid, "main", &[]).await;

    let stats = runtime.collect_stats();
    println!("\nFinal stats:");
    println!("  Total executions: {}", stats.total_executions);
    println!("  Execution failures: {}", stats.execution_failures);
    println!("  Compilation failures: {}", stats.compilation_failures);

    if let Some(success_rate) = stats.execution_success_rate() {
        println!("  Execution success rate: {:.1}%", success_rate * 100.0);
    }

    println!("\n=== Demo Complete ===");

    Ok(())
}
