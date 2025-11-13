//! Simple WASM execution example
//!
//! Demonstrates:
//! - Creating a WASM runtime with security config
//! - Executing a simple WASM module
//! - Using host functions (host_add)
//!
//! Run with:
//! ```bash
//! cargo run --example simple_execution
//! ```

use mcp_bridge::Bridge;
use mcp_wasm_runtime::{Runtime, SecurityConfig};
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize tracing for logs
    tracing_subscriber::fmt()
        .with_env_filter("mcp_wasm_runtime=info")
        .init();

    println!("🚀 MCP Code Execution - Simple Example\n");

    // Step 1: Create MCP bridge
    println!("1️⃣  Creating MCP bridge...");
    let bridge = Bridge::new(1000);
    println!("   ✓ Bridge created with 1000ms timeout\n");

    // Step 2: Configure security
    println!("2️⃣  Configuring security sandbox...");
    let config = SecurityConfig::default();
    println!("   ✓ Memory limit: 256MB");
    println!("   ✓ Execution timeout: 60s");
    println!("   ✓ Host call limit: 1000\n");

    // Step 3: Create runtime
    println!("3️⃣  Creating WASM runtime...");
    let runtime = Runtime::new(Arc::new(bridge), config)?;
    println!("   ✓ Runtime initialized\n");

    // Step 4: Define WASM module (WAT format)
    println!("4️⃣  Compiling WASM module...");
    let wat = r#"
        (module
            ;; Import host function for addition
            (import "env" "host_add" (func $add (param i32 i32) (result i32)))

            ;; Main entry point
            (func (export "main") (result i32)
                ;; Calculate 10 + 32 using host function
                (call $add (i32.const 10) (i32.const 32))
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat)?;
    println!("   ✓ Compiled {} bytes of WASM\n", wasm_bytes.len());

    // Step 5: Execute
    println!("5️⃣  Executing WASM module...");
    let result = runtime.execute(&wasm_bytes, "main", &[]).await?;
    println!("   ✓ Execution completed\n");

    // Step 6: Display results
    println!("📊 Results:");
    println!("   Exit code: {}", result["exit_code"]);
    println!("   Elapsed time: {}ms", result["elapsed_ms"]);
    println!("   Expected: 42 (10 + 32)");

    if result["exit_code"] == 42 {
        println!("\n✅ Success! The result is correct.");
    } else {
        println!("\n❌ Error! Expected 42, got {}", result["exit_code"]);
    }

    Ok(())
}
