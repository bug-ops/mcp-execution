//! Host logging example
//!
//! Demonstrates:
//! - Logging from WASM to host
//! - Memory exports and data sections
//! - String handling across WASM boundary
//!
//! Run with:
//! ```bash
//! cargo run --example host_logging
//! ```

use mcp_bridge::Bridge;
use mcp_wasm_runtime::{Runtime, SecurityConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing to see host_log output
    tracing_subscriber::fmt()
        .with_env_filter("mcp_wasm_runtime=info")
        .init();

    println!("🎤 MCP Code Execution - Host Logging Example\n");

    // Setup
    let bridge = Bridge::new(1000);
    let config = SecurityConfig::default();
    let runtime = Runtime::new(Arc::new(bridge), config)?;

    // WASM module that logs multiple messages
    let wat = r#"
        (module
            ;; Import host logging function
            (import "env" "host_log" (func $log (param i32 i32)))

            ;; Export memory (required for host to read strings)
            (memory (export "memory") 1)

            ;; Store messages in memory
            (data (i32.const 0) "Hello from WASM!")
            (data (i32.const 16) "Executing in secure sandbox")
            (data (i32.const 48) "All systems operational")

            (func (export "main") (result i32)
                ;; Log message 1 (offset 0, length 16)
                (call $log (i32.const 0) (i32.const 16))

                ;; Log message 2 (offset 16, length 27)
                (call $log (i32.const 16) (i32.const 27))

                ;; Log message 3 (offset 48, length 23)
                (call $log (i32.const 48) (i32.const 23))

                ;; Return success
                (i32.const 0)
            )
        )
    "#;

    println!("📝 Compiling WASM module with embedded strings...\n");
    let wasm_bytes = wat::parse_str(wat)?;

    println!("🚀 Executing WASM (watch for log messages below):\n");
    println!("─────────────────────────────────────────────────");

    let result = runtime.execute(&wasm_bytes, "main", &[]).await?;

    println!("─────────────────────────────────────────────────\n");

    println!("📊 Execution completed:");
    println!("   Exit code: {}", result["exit_code"]);
    println!("   Time: {}ms", result["elapsed_ms"]);

    if result["exit_code"] == 0 {
        println!("\n✅ All messages logged successfully!");
    }

    Ok(())
}
