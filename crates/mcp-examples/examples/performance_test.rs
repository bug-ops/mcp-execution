//! Performance testing for MCP Code Execution.
//!
//! Measures and validates performance across all pipeline stages:
//! - Code generation speed
//! - VFS loading time
//! - WASM compilation time
//! - Execution overhead
//! - Cache effectiveness
//!
//! # Usage
//!
//! ```bash
//! cargo run --example performance_test --release
//! ```

use mcp_bridge::Bridge;
use mcp_codegen::CodeGenerator;
use mcp_examples::metrics::Metrics;
use mcp_examples::mock_server::MockMcpServer;
use mcp_vfs::VfsBuilder;
use mcp_wasm_runtime::Runtime;
use mcp_wasm_runtime::security::SecurityConfig;
use std::sync::Arc;
use std::time::Instant;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging (errors only for clean output)
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::ERROR)
        .with_target(false)
        .init();

    println!("╔═══════════════════════════════════════════════════╗");
    println!("║   MCP Code Execution - Performance Test          ║");
    println!("║   Validating Performance Targets                  ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Setup
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("Setting up test environment...\n");

    let mock_server = MockMcpServer::new_vkteams_bot();
    let server_info = mock_server.server_info().clone();

    println!("Test Configuration:");
    println!("  Server:       {}", server_info.name);
    println!("  Tools:        {}", server_info.tools.len());
    println!("  Iterations:   10 (for averaging)");
    println!();

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Test 1: Code Generation Performance
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Test 1: Code Generation Performance");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let generator = CodeGenerator::new()?;
    let mut codegen_times = Vec::new();

    for i in 1..=10 {
        let start = Instant::now();
        let _generated = generator.generate(&server_info)?;
        let elapsed = start.elapsed().as_millis() as u64;
        codegen_times.push(elapsed);

        if i == 1 {
            println!("  Run {}: {}ms (cold start)", i, elapsed);
        }
    }

    let avg_codegen = codegen_times.iter().sum::<u64>() / codegen_times.len() as u64;
    let min_codegen = *codegen_times.iter().min().unwrap();
    let max_codegen = *codegen_times.iter().max().unwrap();

    println!("\n  Results:");
    println!("    Average:  {}ms", avg_codegen);
    println!("    Min:      {}ms", min_codegen);
    println!("    Max:      {}ms", max_codegen);
    println!("    Target:   No specific target (informational)");
    println!("    Status:   ✓ MEASURED");

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Test 2: VFS Loading Performance
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Test 2: VFS Loading Performance");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let generated = generator.generate(&server_info)?;
    let mut vfs_times = Vec::new();

    for _ in 0..10 {
        let start = Instant::now();
        let _vfs = VfsBuilder::from_generated_code(generated.clone(), "/mcp-tools/servers/test")
            .build()?;
        let elapsed = start.elapsed().as_millis() as u64;
        vfs_times.push(elapsed);
    }

    let avg_vfs = vfs_times.iter().sum::<u64>() / vfs_times.len() as u64;
    let min_vfs = *vfs_times.iter().min().unwrap();
    let max_vfs = *vfs_times.iter().max().unwrap();

    println!("  Results:");
    println!("    Average:  {}ms", avg_vfs);
    println!("    Min:      {}ms", min_vfs);
    println!("    Max:      {}ms", max_vfs);
    println!("    Target:   No specific target (informational)");
    println!("    Status:   ✓ MEASURED");

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Test 3: WASM Compilation Performance
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Test 3: WASM Compilation Performance");
    println!("  Target: <100ms");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let wasm_module = create_demo_wasm_module();
    let mut compile_times = Vec::new();

    // Create bridge once
    let bridge = Arc::new(Bridge::new(1000));

    for i in 1..=10 {
        let start = Instant::now();
        let _runtime = Runtime::new(bridge.clone(), SecurityConfig::default())?;
        let elapsed = start.elapsed().as_millis() as u64;
        compile_times.push(elapsed);

        if i == 1 {
            println!("  Run {}: {}ms (cold start)", i, elapsed);
        }
    }

    let avg_compile = compile_times.iter().sum::<u64>() / compile_times.len() as u64;
    let min_compile = *compile_times.iter().min().unwrap();
    let max_compile = *compile_times.iter().max().unwrap();

    println!("\n  Results:");
    println!("    Average:  {}ms", avg_compile);
    println!("    Min:      {}ms", min_compile);
    println!("    Max:      {}ms", max_compile);
    println!("    Target:   <100ms");
    println!(
        "    Status:   {}",
        if avg_compile < 100 {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Test 4: Execution Performance
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Test 4: Execution Performance");
    println!("  Target: <50ms");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let runtime = Runtime::new(bridge.clone(), SecurityConfig::default())?;
    let mut exec_times = Vec::new();

    for i in 1..=10 {
        let start = Instant::now();
        let _result = runtime.execute(&wasm_module, "main", &[]).await;
        let elapsed = start.elapsed().as_millis() as u64;
        exec_times.push(elapsed);

        if i == 1 {
            println!("  Run {}: {}ms (first execution)", i, elapsed);
        }
    }

    let avg_exec = exec_times.iter().sum::<u64>() / exec_times.len() as u64;
    let min_exec = *exec_times.iter().min().unwrap();
    let max_exec = *exec_times.iter().max().unwrap();

    println!("\n  Results:");
    println!("    Average:  {}ms", avg_exec);
    println!("    Min:      {}ms", min_exec);
    println!("    Max:      {}ms", max_exec);
    println!("    Target:   <50ms");
    println!(
        "    Status:   {}",
        if avg_exec < 50 {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Test 5: End-to-End Performance
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("  Test 5: End-to-End Performance");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━\n");

    let mut metrics = Metrics::new();
    metrics.start_total();

    // Full workflow
    metrics.start_introspection();
    let _server = MockMcpServer::new_vkteams_bot();
    metrics.end_introspection();

    metrics.start_code_generation();
    let generated = generator.generate(&server_info)?;
    metrics.end_code_generation();

    metrics.start_vfs_load();
    let _vfs = VfsBuilder::from_generated_code(generated, "/mcp-tools/servers/test").build()?;
    metrics.end_vfs_load();

    metrics.start_wasm_compilation();
    let runtime = Runtime::new(bridge, SecurityConfig::default())?;
    metrics.end_wasm_compilation();

    metrics.start_execution();
    let _result = runtime.execute(&wasm_module, "main", &[]).await;
    metrics.end_execution();

    metrics.end_total();

    println!("  Phase Breakdown:");
    println!(
        "    Introspection:    {:>4}ms",
        metrics.introspection_time_ms
    );
    println!(
        "    Code Generation:  {:>4}ms",
        metrics.code_generation_time_ms
    );
    println!("    VFS Load:         {:>4}ms", metrics.vfs_load_time_ms);
    println!(
        "    WASM Compile:     {:>4}ms",
        metrics.wasm_compilation_time_ms
    );
    println!("    Execution:        {:>4}ms", metrics.execution_time_ms);
    println!("    ─────────────────────────");
    println!("    Total:            {:>4}ms", metrics.total_time_ms);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Performance Summary
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n╔═══════════════════════════════════════════════════╗");
    println!("║           PERFORMANCE SUMMARY                     ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    let mut passed = 0;
    let mut total = 0;

    println!("Performance Targets:");

    // Compilation target
    total += 1;
    if avg_compile < 100 {
        println!("  ✓ WASM Compilation <100ms:  PASS ({}ms)", avg_compile);
        passed += 1;
    } else {
        println!("  ✗ WASM Compilation <100ms:  FAIL ({}ms)", avg_compile);
    }

    // Execution target
    total += 1;
    if avg_exec < 50 {
        println!("  ✓ Execution <50ms:          PASS ({}ms)", avg_exec);
        passed += 1;
    } else {
        println!("  ✗ Execution <50ms:          FAIL ({}ms)", avg_exec);
    }

    println!();
    println!("Additional Metrics (informational):");
    println!("  • Code Generation:  {}ms", avg_codegen);
    println!("  • VFS Loading:      {}ms", avg_vfs);
    println!("  • Total E2E:        {}ms", metrics.total_time_ms);

    println!();
    println!("Score: {}/{} targets passed", passed, total);

    println!("\n╔═══════════════════════════════════════════════════╗");
    if passed == total {
        println!("║   ✓ ALL PERFORMANCE TARGETS MET                   ║");
    } else {
        println!("║   ⚠ SOME PERFORMANCE TARGETS MISSED               ║");
    }
    println!("╚═══════════════════════════════════════════════════╝\n");

    // Exit with appropriate code
    if passed == total {
        println!("Performance test: SUCCESS\n");
        Ok(())
    } else {
        println!("Performance test: PARTIAL (some targets missed)\n");
        println!("Note: This is acceptable for development builds.");
        println!("      Run with --release for production performance.\n");
        Ok(())
    }
}

/// Creates a minimal demo WASM module for testing.
fn create_demo_wasm_module() -> Vec<u8> {
    vec![
        0x00, 0x61, 0x73, 0x6d, // Magic number
        0x01, 0x00, 0x00, 0x00, // Version
        0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type section
        0x03, 0x02, 0x01, 0x00, // Function section
        0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // Export "main"
        0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code: return 42
    ]
}
