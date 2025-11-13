//! Criterion benchmarks for MCP Code Execution pipeline.
//!
//! Measures performance of individual components and end-to-end workflow.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mcp_bridge::Bridge;
use mcp_codegen::CodeGenerator;
use mcp_examples::mock_server::MockMcpServer;
use mcp_vfs::VfsBuilder;
use mcp_wasm_runtime::Runtime;
use mcp_wasm_runtime::security::SecurityConfig;
use std::sync::Arc;

/// Benchmarks code generation from server info.
fn bench_code_generation(c: &mut Criterion) {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();
    let generator = CodeGenerator::new().unwrap();

    c.bench_function("code_generation", |b| {
        b.iter(|| {
            let _generated = generator.generate(server_info).unwrap();
        });
    });
}

/// Benchmarks VFS building from generated code.
fn bench_vfs_build(c: &mut Criterion) {
    let server = MockMcpServer::new_vkteams_bot();
    let server_info = server.server_info();
    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(server_info).unwrap();

    c.bench_function("vfs_build", |b| {
        b.iter(|| {
            let _vfs =
                VfsBuilder::from_generated_code(generated.clone(), "/mcp-tools/servers/test")
                    .build()
                    .unwrap();
        });
    });
}

/// Benchmarks WASM runtime creation.
fn bench_runtime_creation(c: &mut Criterion) {
    let bridge = Arc::new(Bridge::new(100));
    let config = SecurityConfig::default();

    c.bench_function("runtime_creation", |b| {
        b.iter(|| {
            let _runtime = Runtime::new(bridge.clone(), config.clone()).unwrap();
        });
    });
}

/// Benchmarks WASM execution.
fn bench_wasm_execution(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let bridge = Arc::new(Bridge::new(100));
    let config = SecurityConfig::default();
    let runtime = Runtime::new(bridge, config).unwrap();

    // Minimal WASM module
    let wasm = vec![
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f,
        0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, 0x0a,
        0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b,
    ];

    c.bench_function("wasm_execution", |b| {
        b.to_async(&rt).iter(|| async {
            let _result = runtime.execute(&wasm, "main", &[]).await;
        });
    });
}

/// Benchmarks end-to-end workflow.
fn bench_e2e_workflow(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();

    c.bench_function("e2e_workflow", |b| {
        b.to_async(&rt).iter(|| async {
            // Full workflow
            let server = MockMcpServer::new_vkteams_bot();
            let server_info = server.server_info();

            let generator = CodeGenerator::new().unwrap();
            let generated = generator.generate(server_info).unwrap();

            let _vfs = VfsBuilder::from_generated_code(generated, "/mcp-tools/servers/test")
                .build()
                .unwrap();

            let bridge = Arc::new(Bridge::new(100));
            let config = SecurityConfig::default();
            let runtime = Runtime::new(bridge, config).unwrap();

            let wasm = vec![
                0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01,
                0x7f, 0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00,
                0x00, 0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b,
            ];

            let _result = runtime.execute(&wasm, "main", &[]).await;
        });
    });
}

/// Benchmarks with different numbers of tools.
fn bench_scaling_with_tools(c: &mut Criterion) {
    use mcp_core::{ServerId, ToolName};
    use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
    use serde_json::json;

    let mut group = c.benchmark_group("scaling_tools");

    for num_tools in [1, 5, 10, 20, 50].iter() {
        let tools: Vec<ToolInfo> = (0..*num_tools)
            .map(|i| ToolInfo {
                name: ToolName::new(&format!("tool_{}", i)),
                description: format!("Tool number {}", i),
                input_schema: json!({"type": "object"}),
                output_schema: None,
            })
            .collect();

        let server_info = ServerInfo {
            id: ServerId::new("test"),
            name: "Test Server".to_string(),
            version: "1.0.0".to_string(),
            capabilities: ServerCapabilities {
                supports_tools: true,
                supports_resources: false,
                supports_prompts: false,
            },
            tools,
        };

        let generator = CodeGenerator::new().unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(num_tools), num_tools, |b, _| {
            b.iter(|| {
                let _generated = generator.generate(&server_info).unwrap();
            });
        });
    }

    group.finish();
}

/// Benchmarks cold start vs warm cache.
fn bench_cold_vs_warm(c: &mut Criterion) {
    let rt = tokio::runtime::Runtime::new().unwrap();
    let mut group = c.benchmark_group("cold_vs_warm");

    let wasm = vec![
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f,
        0x03, 0x02, 0x01, 0x00, 0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, 0x0a,
        0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b,
    ];

    // Cold start: create new runtime each time
    group.bench_function("cold_start", |b| {
        b.to_async(&rt).iter(|| async {
            let bridge = Arc::new(Bridge::new(100));
            let config = SecurityConfig::default();
            let runtime = Runtime::new(bridge, config).unwrap();
            let _result = runtime.execute(&wasm, "main", &[]).await;
        });
    });

    // Warm: reuse runtime
    let bridge = Arc::new(Bridge::new(100));
    let config = SecurityConfig::default();
    let runtime = Runtime::new(bridge, config).unwrap();

    group.bench_function("warm_cache", |b| {
        b.to_async(&rt).iter(|| async {
            let _result = runtime.execute(&wasm, "main", &[]).await;
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_code_generation,
    bench_vfs_build,
    bench_runtime_creation,
    bench_wasm_execution,
    bench_e2e_workflow,
    bench_scaling_with_tools,
    bench_cold_vs_warm
);

criterion_main!(benches);
