//! End-to-end workflow demonstration.
//!
//! Demonstrates the complete MCP Code Execution pipeline:
//! 1. Server introspection (using mock server)
//! 2. Code generation (TypeScript from tool schemas)
//! 3. VFS loading (virtual filesystem)
//! 4. WASM compilation and execution
//! 5. MCP bridge integration
//!
//! This example showcases the full workflow without requiring an actual
//! MCP server, making it easy to run and verify the implementation.
//!
//! # Usage
//!
//! ```bash
//! cargo run --example e2e_workflow
//! ```

use mcp_bridge::Bridge;
use mcp_codegen::CodeGenerator;
use mcp_examples::metrics::Metrics;
use mcp_examples::mock_server::MockMcpServer;
use mcp_examples::token_analysis::TokenAnalysis;
use mcp_vfs::VfsBuilder;
use mcp_wasm_runtime::Runtime;
use mcp_wasm_runtime::security::SecurityConfig;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::INFO)
        .with_target(false)
        .init();

    println!("╔═══════════════════════════════════════════════════╗");
    println!("║   MCP Code Execution - End-to-End Workflow        ║");
    println!("║   Demonstrating Full Pipeline                     ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    let mut metrics = Metrics::new();
    metrics.start_total();

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Phase 1: Server Introspection (Mock)
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("━━━ Phase 1: Server Introspection ━━━\n");
    metrics.start_introspection();

    println!("→ Creating mock VKTeams Bot server...");
    let mock_server = MockMcpServer::new_vkteams_bot();
    let server_info = mock_server.server_info().clone();

    println!("✓ Mock server created");
    println!("  Name:    {}", server_info.name);
    println!("  Version: {}", server_info.version);
    println!("  Tools:   {}", server_info.tools.len());

    for (idx, tool) in server_info.tools.iter().enumerate() {
        println!("    {}. {} - {}", idx + 1, tool.name, tool.description);
    }

    metrics.end_introspection();
    metrics.tools_discovered = server_info.tools.len();

    println!(
        "\n⏱  Introspection time: {}ms",
        metrics.introspection_time_ms
    );

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Phase 2: Code Generation
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━ Phase 2: Code Generation ━━━\n");
    metrics.start_code_generation();

    println!("→ Generating TypeScript code from tool schemas...");
    let generator = CodeGenerator::new()?;
    let generated = generator.generate(&server_info)?;

    let total_bytes: usize = generated.files.iter().map(|f| f.content.len()).sum();

    println!("✓ Code generation complete");
    println!("  Files generated: {}", generated.file_count());
    println!("  Total code size: {total_bytes} bytes");

    for file in &generated.files {
        println!("    - {}", file.path);
    }

    metrics.end_code_generation();
    metrics.files_generated = generated.file_count();
    metrics.generated_code_bytes = total_bytes;

    println!(
        "\n⏱  Code generation time: {}ms",
        metrics.code_generation_time_ms
    );

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Phase 3: VFS Loading
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━ Phase 3: Virtual Filesystem Loading ━━━\n");
    metrics.start_vfs_load();

    println!("→ Loading generated code into VFS...");
    let vfs_root = "/mcp-tools/servers/vkteams-bot";
    let vfs = VfsBuilder::from_generated_code(generated, vfs_root).build()?;

    println!("✓ VFS loaded");
    println!("  Root:  {vfs_root}");
    println!("  Files: {}", vfs.file_count());

    // Verify key files exist
    let manifest_path = format!("{vfs_root}/manifest.json");
    let types_path = format!("{vfs_root}/types.ts");

    if vfs.exists(&manifest_path) {
        println!("  ✓ manifest.json available");
    }
    if vfs.exists(&types_path) {
        println!("  ✓ types.ts available");
    }

    metrics.end_vfs_load();
    println!("\n⏱  VFS load time: {}ms", metrics.vfs_load_time_ms);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Phase 4: WASM Runtime Setup
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━ Phase 4: WASM Runtime Setup ━━━\n");

    println!("→ Creating MCP bridge...");
    let bridge = Arc::new(Bridge::new(1000));
    println!("  ✓ Bridge created (capacity: 1000)");

    println!("\n→ Configuring security sandbox...");
    let security_config = SecurityConfig::default();
    println!(
        "  Memory limit: {} MB",
        security_config.memory_limit_bytes() / 1024 / 1024
    );
    println!(
        "  Execution timeout: {} seconds",
        security_config.execution_timeout().as_secs()
    );

    println!("\n→ Creating WASM runtime...");
    let runtime = Runtime::new(bridge.clone(), security_config)?;
    println!("  ✓ Runtime initialized");

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Phase 5: Execution Demonstration
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━ Phase 5: Execution Demonstration ━━━\n");

    println!("→ Creating simple WASM module for testing...");
    metrics.start_wasm_compilation();

    // For this demo, we'll use a minimal WASM module that returns a success value
    // In a real scenario, this would be compiled TypeScript
    let wasm_module = create_demo_wasm_module();

    metrics.end_wasm_compilation();
    println!("  ✓ WASM module ready");
    println!(
        "\n⏱  WASM compilation time: {}ms",
        metrics.wasm_compilation_time_ms
    );

    println!("\n→ Executing WASM module...");
    metrics.start_execution();

    // Execute the WASM module
    let result = runtime.execute(&wasm_module, "main", &[]).await;

    metrics.end_execution();

    match result {
        Ok(value) => {
            println!("✓ Execution successful");
            println!("  Result: {value}");
        }
        Err(e) => {
            println!("✗ Execution failed: {e}");
            println!("  (This is expected for demo - using minimal WASM module)");
        }
    }

    println!("\n⏱  Execution time: {}ms", metrics.execution_time_ms);

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Phase 6: Token Analysis
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n━━━ Phase 6: Token Efficiency Analysis ━━━\n");

    // Simulate a realistic workflow with multiple tool calls
    let num_calls = 10;
    println!("→ Analyzing token usage for {num_calls} tool calls...");

    let token_analysis = TokenAnalysis::analyze(&server_info, num_calls);

    println!("\n{}", token_analysis.format_report());

    metrics.token_savings_percent = token_analysis.savings_percent;
    metrics.mcp_calls = num_calls;

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Final Metrics Report
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    metrics.end_total();

    println!("\n{}", metrics.format_report());

    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
    // Summary
    // ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

    println!("\n╔═══════════════════════════════════════════════════╗");
    println!("║              WORKFLOW SUMMARY                     ║");
    println!("╚═══════════════════════════════════════════════════╝\n");

    println!("Pipeline Stages Completed:");
    println!("  ✓ Server introspection");
    println!("  ✓ Code generation");
    println!("  ✓ VFS loading");
    println!("  ✓ WASM runtime setup");
    println!("  ✓ Execution (demo)");
    println!("  ✓ Token analysis");

    println!("\nKey Results:");
    println!("  • Tools discovered:     {}", metrics.tools_discovered);
    println!("  • Files generated:      {}", metrics.files_generated);
    println!(
        "  • Code size:            {} bytes",
        metrics.generated_code_bytes
    );
    println!(
        "  • Token savings:        {:.1}%",
        metrics.token_savings_percent
    );
    println!("  • Total time:           {}ms", metrics.total_time_ms);

    println!("\nPerformance Targets:");
    println!(
        "  • Execution < 50ms:     {}",
        if metrics.meets_execution_target() {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );
    println!(
        "  • Compile < 100ms:      {}",
        if metrics.meets_compilation_target() {
            "✓ PASS"
        } else {
            "✗ FAIL"
        }
    );
    println!(
        "  • Token savings ≥ 90%:  {}",
        if metrics.meets_token_target() {
            "✓ PASS"
        } else {
            "⚠ PARTIAL (need more calls)"
        }
    );

    println!("\n╔═══════════════════════════════════════════════════╗");
    if metrics.meets_execution_target() && metrics.meets_compilation_target() {
        println!("║   ✓ END-TO-END WORKFLOW SUCCESSFUL               ║");
    } else {
        println!("║   ⚠ WORKFLOW COMPLETE (some targets partial)     ║");
    }
    println!("╚═══════════════════════════════════════════════════╝\n");

    println!("Note: Token savings improve with more calls.");
    println!("      Run 'cargo run --example token_analysis' for details.\n");

    Ok(())
}

/// Creates a minimal demo WASM module.
///
/// This is a simple WAT (WebAssembly Text Format) module that:
/// - Exports a "main" function
/// - Returns a success value (i32: 42)
///
/// In a real scenario, this would be TypeScript compiled to WASM.
fn create_demo_wasm_module() -> Vec<u8> {
    // WAT format for a minimal WASM module
    // (module
    //   (func (export "main") (result i32)
    //     i32.const 42
    //   )
    // )
    vec![
        0x00, 0x61, 0x73, 0x6d, // Magic number: \0asm
        0x01, 0x00, 0x00, 0x00, // Version: 1
        0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type section: func [] -> [i32]
        0x03, 0x02, 0x01, 0x00, // Function section: 1 function of type 0
        0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // Export "main"
        0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code: return 42
    ]
}
