//! Benchmarks for mcp-introspector serialization performance
//!
//! These benchmarks measure JSON serialization/deserialization of
//! ServerInfo and ToolInfo structures.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use mcp_core::{ServerId, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::hint::black_box;

/// Creates a sample ServerInfo for benchmarking
fn create_server_info(tool_count: usize) -> ServerInfo {
    let tools = (0..tool_count)
        .map(|i| ToolInfo {
            name: ToolName::new(format!("tool_{}", i)),
            description: format!("Test tool number {}", i),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "arg1": {"type": "string"},
                    "arg2": {"type": "number"}
                },
                "required": ["arg1"]
            }),
            output_schema: Some(json!({"type": "boolean"})),
        })
        .collect();

    ServerInfo {
        id: ServerId::new("test-server"),
        name: "Test Server".to_string(),
        version: "1.0.0".to_string(),
        tools,
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: true,
            supports_prompts: false,
        },
    }
}

/// Benchmarks ServerInfo serialization
fn bench_server_info_serialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("server_info_serialization");

    for tool_count in [1, 10, 50, 100].iter() {
        let server_info = create_server_info(*tool_count);

        group.throughput(Throughput::Elements(*tool_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(tool_count),
            &server_info,
            |b, info| {
                b.iter(|| {
                    serde_json::to_string(black_box(info)).unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks ServerInfo deserialization
fn bench_server_info_deserialization(c: &mut Criterion) {
    let mut group = c.benchmark_group("server_info_deserialization");

    for tool_count in [1, 10, 50, 100].iter() {
        let server_info = create_server_info(*tool_count);
        let json = serde_json::to_string(&server_info).unwrap();

        group.throughput(Throughput::Elements(*tool_count as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(tool_count),
            &json,
            |b, json_str| {
                b.iter(|| {
                    let _: ServerInfo = serde_json::from_str(black_box(json_str)).unwrap();
                });
            },
        );
    }

    group.finish();
}

/// Benchmarks ToolInfo serialization
fn bench_tool_info_serialization(c: &mut Criterion) {
    let tool = ToolInfo {
        name: ToolName::new("test_tool"),
        description: "A test tool for benchmarking".to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "user": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "name": {"type": "string"}
                    }
                },
                "message": {"type": "string"}
            }
        }),
        output_schema: Some(json!({"type": "object"})),
    };

    c.bench_function("tool_info_serialization", |b| {
        b.iter(|| {
            serde_json::to_string(black_box(&tool)).unwrap();
        });
    });
}

/// Benchmarks ToolInfo deserialization
fn bench_tool_info_deserialization(c: &mut Criterion) {
    let tool = ToolInfo {
        name: ToolName::new("test_tool"),
        description: "A test tool for benchmarking".to_string(),
        input_schema: json!({"type": "object"}),
        output_schema: None,
    };

    let json = serde_json::to_string(&tool).unwrap();

    c.bench_function("tool_info_deserialization", |b| {
        b.iter(|| {
            let _: ToolInfo = serde_json::from_str(black_box(&json)).unwrap();
        });
    });
}

/// Benchmarks ServerCapabilities serialization
fn bench_capabilities_serialization(c: &mut Criterion) {
    let caps = ServerCapabilities {
        supports_tools: true,
        supports_resources: true,
        supports_prompts: false,
    };

    c.bench_function("capabilities_serialization", |b| {
        b.iter(|| {
            serde_json::to_string(black_box(&caps)).unwrap();
        });
    });
}

criterion_group!(
    benches,
    bench_server_info_serialization,
    bench_server_info_deserialization,
    bench_tool_info_serialization,
    bench_tool_info_deserialization,
    bench_capabilities_serialization
);
criterion_main!(benches);
