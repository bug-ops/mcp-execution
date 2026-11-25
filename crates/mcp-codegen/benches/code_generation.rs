//! Comprehensive performance benchmarks for mcp-codegen.
//!
//! Tests code generation performance across different:
//! - Tool counts (1, 10, 50, 100, 1000)
//! - Schema complexity (simple, moderate, complex)
//! - Operations (full generation, individual components)
//!
//! Run with: cargo bench --package mcp-execution-codegen
//!
//! Note: VFS-related benchmarks (`bench_vfs_loading`, `bench_end_to_end`) are in
//! mcp-execution-files to avoid circular dependencies.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use mcp_execution_codegen::package::PackageGenerator;
use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::hint::black_box;

/// `ServerConfig` passed alongside `ServerInfo` so `generate` can stamp generation provenance.
fn bench_config() -> ServerConfig {
    ServerConfig::builder()
        .command("bench-command".to_string())
        .build()
        .unwrap()
}

// ============================================================================
// Test Data Generators
// ============================================================================

/// Creates a simple tool with minimal schema.
fn create_simple_tool(index: usize) -> ToolInfo {
    ToolInfo::new(
        ToolName::new(format!("simple_tool_{index}")).unwrap(),
        format!("Simple tool {index}"),
        json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"}
            },
            "required": ["id"]
        }),
    )
}

/// Creates a tool with moderate schema complexity.
fn create_moderate_tool(index: usize) -> ToolInfo {
    {
        let mut value = ToolInfo::new(
            ToolName::new(format!("moderate_tool_{index}")).unwrap(),
            format!("Moderate complexity tool {index}"),
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "name": {"type": "string"},
                    "count": {"type": "number"},
                    "active": {"type": "boolean"},
                    "tags": {
                        "type": "array",
                        "items": {"type": "string"}
                    }
                },
                "required": ["id", "name"]
            }),
        );
        value.output_schema = Some(json!({
            "type": "object",
            "properties": {
                "result": {"type": "string"},
                "code": {"type": "number"}
            }
        }));
        value
    }
}

/// Creates a tool with complex nested schema.
fn create_complex_tool(index: usize) -> ToolInfo {
    {
        let mut value = ToolInfo::new(
            ToolName::new(format!("complex_tool_{index}")).unwrap(),
            format!("Complex nested tool {index}"),
            json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "metadata": {
                        "type": "object",
                        "properties": {
                            "name": {"type": "string"},
                            "priority": {"type": "number"},
                            "tags": {
                                "type": "array",
                                "items": {"type": "string"}
                            },
                            "attributes": {
                                "type": "object",
                                "properties": {
                                    "color": {"type": "string"},
                                    "size": {"type": "number"}
                                }
                            }
                        }
                    },
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": {"type": "string"},
                                "value": {"type": "string"},
                                "enabled": {"type": "boolean"}
                            }
                        }
                    },
                    "config": {
                        "type": "object",
                        "properties": {
                            "timeout": {"type": "number"},
                            "retries": {"type": "number"},
                            "fallback": {
                                "type": "object",
                                "properties": {
                                    "url": {"type": "string"}
                                }
                            }
                        }
                    }
                },
                "required": ["id", "metadata"]
            }),
        );
        value.output_schema = Some(json!({
            "type": "object",
            "properties": {
                "success": {"type": "boolean"},
                "data": {
                    "type": "object",
                    "properties": {
                        "items": {
                            "type": "array",
                            "items": {"type": "string"}
                        }
                    }
                }
            }
        }));
        value
    }
}

/// Creates server info with specified number of tools and complexity.
fn create_server_info(tool_count: usize, tool_creator: fn(usize) -> ToolInfo) -> ServerInfo {
    let tools: Vec<_> = (0..tool_count).map(tool_creator).collect();

    ServerInfo::new(
        ServerId::new(format!("bench-server-{tool_count}")).unwrap(),
        format!("Benchmark Server (n={tool_count})"),
        "1.0.0".to_string(),
        tools,
        {
            let mut value = ServerCapabilities::default();
            value.supports_tools = true;
            value
        },
    )
}

// ============================================================================
// Benchmark Functions
// ============================================================================

/// Benchmarks full code generation pipeline for different tool counts.
fn bench_full_generation_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_generation_scaling");

    for count in [1, 10, 50, 100, 500, 1000] {
        let server_info = create_server_info(count, create_moderate_tool);
        let config = bench_config();
        let generator = PackageGenerator::new().expect("Generator should initialize");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _count| {
            b.iter(|| {
                let result = generator.generate(black_box(&server_info), black_box(&config));
                assert!(result.is_ok());
            });
        });
    }

    group.finish();
}

/// Benchmarks code generation with different schema complexities.
fn bench_schema_complexity(c: &mut Criterion) {
    let mut group = c.benchmark_group("schema_complexity");
    let tool_count = 50;

    let complexities = [
        ("simple", create_simple_tool as fn(usize) -> ToolInfo),
        ("moderate", create_moderate_tool),
        ("complex", create_complex_tool),
    ];

    for (name, tool_creator) in complexities {
        let server_info = create_server_info(tool_count, tool_creator);
        let config = bench_config();
        let generator = PackageGenerator::new().expect("Generator should initialize");

        group.throughput(Throughput::Elements(tool_count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), name, |b, _| {
            b.iter(|| {
                let result = generator.generate(black_box(&server_info), black_box(&config));
                assert!(result.is_ok());
            });
        });
    }

    group.finish();
}

/// Benchmarks generator initialization overhead.
fn bench_generator_initialization(c: &mut Criterion) {
    c.bench_function("generator_initialization", |b| {
        b.iter(|| {
            let generator = PackageGenerator::new();
            assert!(generator.is_ok());
            black_box(generator)
        });
    });
}

/// Benchmarks type conversion performance.
fn bench_type_conversion(c: &mut Criterion) {
    use mcp_execution_codegen::common::ts_types::{RenderStyle, convert_schema};

    let mut group = c.benchmark_group("type_conversion");

    // Simple schema
    let simple_schema = json!({"type": "string"});
    group.bench_function("simple_schema", |b| {
        b.iter(|| {
            let result = convert_schema(black_box(&simple_schema)).render(RenderStyle::Declaration);
            black_box(result)
        });
    });

    // Complex object schema
    let complex_schema = json!({
        "type": "object",
        "properties": {
            "name": {"type": "string"},
            "items": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "id": {"type": "string"},
                        "value": {"type": "number"}
                    }
                }
            }
        },
        "required": ["name"]
    });

    group.bench_function("complex_schema", |b| {
        b.iter(|| {
            let result =
                convert_schema(black_box(&complex_schema)).render(RenderStyle::Declaration);
            black_box(result)
        });
    });

    // Compact rendering, as used for the catalog and stored texts
    group.bench_function("compact_rendering", |b| {
        b.iter(|| {
            let result = convert_schema(black_box(&complex_schema)).render(RenderStyle::Compact);
            black_box(result)
        });
    });

    group.finish();
}

/// Benchmarks memory usage patterns (allocations per tool).
fn bench_memory_patterns(c: &mut Criterion) {
    let mut group = c.benchmark_group("memory_patterns");
    group.sample_size(50); // Fewer samples for memory-focused tests

    // Single generation with measurements
    let server_info = create_server_info(100, create_moderate_tool);
    let config = bench_config();
    let generator = PackageGenerator::new().expect("Generator should initialize");

    group.bench_function("generate_100_tools", |b| {
        b.iter(|| {
            let generated = generator
                .generate(black_box(&server_info), black_box(&config))
                .expect("Should succeed");
            // Force evaluation of all generated files
            for file in &generated.files {
                black_box(&file.path);
                black_box(&file.content);
            }
        });
    });

    group.finish();
}

// ============================================================================
// Benchmark Configuration
// ============================================================================

criterion_group!(
    name = benches;
    config = Criterion::default()
        .sample_size(100)           // Good balance between accuracy and speed
        .measurement_time(std::time::Duration::from_secs(10))
        .warm_up_time(std::time::Duration::from_secs(3));
    targets =
        bench_generator_initialization,
        bench_type_conversion,
        bench_schema_complexity,
        bench_full_generation_scaling,
        bench_memory_patterns,
);

criterion_main!(benches);
