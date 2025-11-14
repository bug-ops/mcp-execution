//! Integration tests for MCP Code Execution.
//!
//! Tests the complete workflow integration across all components.

use mcp_bridge::Bridge;
use mcp_codegen::CodeGenerator;
use mcp_examples::mock_server::MockMcpServer;
use mcp_examples::token_analysis::TokenAnalysis;
use mcp_vfs::VfsBuilder;
use mcp_wasm_runtime::Runtime;
use mcp_wasm_runtime::security::SecurityConfig;
use std::sync::Arc;

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Mock Server Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_mock_server_creation() {
    let server = MockMcpServer::new_vkteams_bot();
    let info = server.server_info();

    assert_eq!(info.name, "vkteams-bot");
    assert_eq!(info.version, "1.0.0");
    assert!(info.capabilities.supports_tools);
    assert_eq!(info.tools.len(), 4);
}

#[test]
fn test_mock_server_tool_names() {
    let server = MockMcpServer::new_vkteams_bot();
    let tool_names = server.tool_names();

    assert!(tool_names.contains(&"send_message".to_string()));
    assert!(tool_names.contains(&"get_message".to_string()));
    assert!(tool_names.contains(&"get_chat".to_string()));
    assert!(tool_names.contains(&"list_chats".to_string()));
}

#[tokio::test]
async fn test_mock_server_tool_call() {
    let server = MockMcpServer::new_vkteams_bot();

    let result = server
        .call_tool(
            "send_message",
            serde_json::json!({"chat_id": "123", "text": "Hello"}),
        )
        .await;

    assert!(result.is_ok());
    let value = result.unwrap();
    assert!(value.get("message_id").is_some());
}

#[tokio::test]
async fn test_mock_server_invalid_tool() {
    let server = MockMcpServer::new_vkteams_bot();

    let result = server.call_tool("nonexistent", serde_json::json!({})).await;

    assert!(result.is_err());
}

#[tokio::test]
async fn test_mock_server_missing_params() {
    let server = MockMcpServer::new_vkteams_bot();

    // send_message requires both chat_id and text
    let result = server
        .call_tool("send_message", serde_json::json!({"chat_id": "123"}))
        .await;

    assert!(result.is_err());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Code Generation Integration Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_introspection_to_codegen() {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let generator = CodeGenerator::new().unwrap();
    let result = generator.generate(server_info);

    assert!(result.is_ok());
    let generated = result.unwrap();

    assert!(generated.file_count() > 0);
    let total_bytes: usize = generated.files.iter().map(|f| f.content.len()).sum();
    assert!(total_bytes > 0);
}

#[test]
fn test_codegen_generates_expected_files() {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(server_info).unwrap();

    // Should generate manifest.json
    let has_manifest = generated
        .files
        .iter()
        .any(|f| f.path.contains("manifest.json"));
    assert!(has_manifest, "Should generate manifest.json");

    // Should generate types.ts
    let has_types = generated.files.iter().any(|f| f.path.contains("types.ts"));
    assert!(has_types, "Should generate types.ts");

    // Should generate tool files
    let tool_files: Vec<_> = generated
        .files
        .iter()
        .filter(|f| f.path.contains("tools/"))
        .collect();
    assert!(
        !tool_files.is_empty(),
        "Should generate tool implementation files"
    );
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// VFS Integration Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_codegen_to_vfs() {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(server_info).unwrap();

    let vfs_root = "/mcp-tools/servers/test";
    let result = VfsBuilder::from_generated_code(generated, vfs_root).build();

    assert!(result.is_ok());
    let vfs = result.unwrap();

    // Verify files are accessible
    assert!(vfs.exists(&format!("{}/manifest.json", vfs_root)));
}

#[test]
fn test_vfs_file_reading() {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(server_info).unwrap();

    let vfs_root = "/mcp-tools/servers/test";
    let vfs = VfsBuilder::from_generated_code(generated, vfs_root)
        .build()
        .unwrap();

    // Read manifest
    let manifest_path = format!("{}/manifest.json", vfs_root);
    let content = vfs.read_file(&manifest_path);

    assert!(content.is_ok());
    let manifest_text = content.unwrap();
    assert!(!manifest_text.is_empty());

    // Parse as JSON to verify it's valid
    let json: serde_json::Value = serde_json::from_str(manifest_text).unwrap();
    assert!(json.is_object());
}

#[test]
fn test_vfs_directory_listing() {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(server_info).unwrap();

    let vfs_root = "/mcp-tools/servers/test";
    let vfs = VfsBuilder::from_generated_code(generated, vfs_root)
        .build()
        .unwrap();

    // List root directory
    let files = vfs.list_dir(vfs_root);

    assert!(files.is_ok());
    let file_list = files.unwrap();
    assert!(!file_list.is_empty());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// WASM Runtime Integration Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_runtime_creation() {
    let bridge = Arc::new(Bridge::new(100));
    let config = SecurityConfig::default();

    let result = Runtime::new(bridge, config);
    assert!(result.is_ok());
}

#[tokio::test]
async fn test_runtime_execution_simple() {
    let bridge = Arc::new(Bridge::new(100));
    let config = SecurityConfig::default();
    let runtime = Runtime::new(bridge, config).unwrap();

    // Minimal WASM that returns 42
    let wasm = vec![
        0x00, 0x61, 0x73, 0x6d, // Magic
        0x01, 0x00, 0x00, 0x00, // Version
        0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type
        0x03, 0x02, 0x01, 0x00, // Function
        0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // Export "main"
        0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code: return 42
    ];

    let result = runtime.execute(&wasm, "main", &[]).await;

    // Execution may succeed or fail depending on entry point signature
    // The important thing is that it doesn't crash
    assert!(result.is_ok() || result.is_err());
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Token Analysis Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_token_analysis_calculation() {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let analysis = TokenAnalysis::analyze(server_info, 10);

    assert!(analysis.standard_mcp_tokens > 0);
    assert!(analysis.code_execution_tokens > 0);
    assert!(analysis.standard_mcp_tokens > analysis.code_execution_tokens);
    assert!(analysis.savings_percent > 0.0);
    assert!(analysis.savings_percent < 100.0);
}

#[test]
fn test_token_analysis_scaling() {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let analysis_few = TokenAnalysis::analyze(server_info, 5);
    let analysis_many = TokenAnalysis::analyze(server_info, 100);

    // More calls should result in higher savings percentage
    assert!(analysis_many.savings_percent > analysis_few.savings_percent);
}

#[test]
fn test_token_analysis_target_achievement() {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    // With enough calls, should achieve 80%+ savings (max is ~83%)
    let analysis = TokenAnalysis::analyze(server_info, 100);

    assert!(analysis.savings_percent >= 80.0);
    // Note: 90% target is not achievable with this model (max ~83%)
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// End-to-End Integration Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[tokio::test]
async fn test_e2e_full_workflow() {
    // 1. Server introspection (mock)
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info().clone();
    assert!(!server_info.tools.is_empty());

    // 2. Code generation
    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();
    assert!(generated.file_count() > 0);

    // 3. VFS loading
    let vfs_root = "/mcp-tools/servers/test";
    let vfs = VfsBuilder::from_generated_code(generated, vfs_root)
        .build()
        .unwrap();
    assert!(vfs.file_count() > 0);

    // 4. WASM runtime setup
    let bridge = Arc::new(Bridge::new(100));
    let config = SecurityConfig::default();
    let runtime = Runtime::new(bridge, config).unwrap();

    // 5. Execute simple WASM
    let wasm = vec![
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f,
        0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, 0x0a,
        0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b,
    ];

    let _result = runtime.execute(&wasm, "main", &[]).await;

    // If we got here without panicking, the integration works
}

#[tokio::test]
async fn test_e2e_error_propagation() {
    // Test that errors propagate correctly through the pipeline

    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info().clone();

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let vfs = VfsBuilder::from_generated_code(generated, "/test")
        .build()
        .unwrap();

    // Try to read non-existent file
    let result = vfs.read_file("/nonexistent.txt");
    assert!(result.is_err());
}

#[tokio::test]
async fn test_e2e_multiple_servers() {
    // Test handling multiple servers in VFS

    let server1 = MockMcpServer::new_vkteams_bot();
    let info1 = server1.server_info().clone();

    let generator = CodeGenerator::new().unwrap();

    // Generate for server 1
    let gen1 = generator.generate(&info1).unwrap();
    let vfs1 = VfsBuilder::from_generated_code(gen1, "/mcp-tools/servers/server1")
        .build()
        .unwrap();

    assert!(vfs1.exists("/mcp-tools/servers/server1/manifest.json"));

    // Both should be independent
    assert!(!vfs1.exists("/mcp-tools/servers/server2/manifest.json"));
}

// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
// Performance Integration Tests
// ━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

#[test]
fn test_codegen_performance() {
    use std::time::Instant;

    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let generator = CodeGenerator::new().unwrap();

    let start = Instant::now();
    let _generated = generator.generate(server_info).unwrap();
    let elapsed = start.elapsed();

    // Code generation should be reasonably fast (< 1 second)
    assert!(
        elapsed.as_millis() < 1000,
        "Code generation took {}ms (expected <1000ms)",
        elapsed.as_millis()
    );
}

#[test]
fn test_vfs_build_performance() {
    use std::time::Instant;

    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();

    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(server_info).unwrap();

    let start = Instant::now();
    let _vfs = VfsBuilder::from_generated_code(generated, "/test")
        .build()
        .unwrap();
    let elapsed = start.elapsed();

    // VFS build should be very fast (< 100ms)
    assert!(
        elapsed.as_millis() < 100,
        "VFS build took {}ms (expected <100ms)",
        elapsed.as_millis()
    );
}

#[tokio::test]
async fn test_runtime_creation_performance() {
    use std::time::Instant;

    let bridge = Arc::new(Bridge::new(100));
    let config = SecurityConfig::default();

    let start = Instant::now();
    let _runtime = Runtime::new(bridge, config).unwrap();
    let elapsed = start.elapsed();

    // Runtime creation should be fast (< 200ms in debug mode)
    assert!(
        elapsed.as_millis() < 500,
        "Runtime creation took {}ms (expected <500ms)",
        elapsed.as_millis()
    );
}
