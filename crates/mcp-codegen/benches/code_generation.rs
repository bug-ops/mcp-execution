//! Comprehensive performance benchmarks for mcp-codegen.
//!
//! Tests code generation performance across different:
//! - Tool counts (1, 10, 50, 100, 1000)
//! - Schema complexity (simple, moderate, complex)
//! - Operations (full generation, individual components)
//!
//! Run with: cargo bench --package mcp-codegen

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use mcp_codegen::CodeGenerator;
use mcp_core::{ServerId, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use mcp_vfs::VfsBuilder;
use serde_json::json;
use std::hint::black_box;

// ============================================================================
// Test Data Generators
// ============================================================================

/// Creates a simple tool with minimal schema.
fn create_simple_tool(index: usize) -> ToolInfo {
    ToolInfo {
        name: ToolName::new(&format!("simple_tool_{}", index)),
        description: format!("Simple tool {}", index),
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"}
            },
            "required": ["id"]
        }),
        output_schema: None,
    }
}

/// Creates a tool with moderate schema complexity.
fn create_moderate_tool(index: usize) -> ToolInfo {
    ToolInfo {
        name: ToolName::new(&format!("moderate_tool_{}", index)),
        description: format!("Moderate complexity tool {}", index),
        input_schema: json!({
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
        output_schema: Some(json!({
            "type": "object",
            "properties": {
                "result": {"type": "string"},
                "code": {"type": "number"}
            }
        })),
    }
}

/// Creates a tool with complex nested schema.
fn create_complex_tool(index: usize) -> ToolInfo {
    ToolInfo {
        name: ToolName::new(&format!("complex_tool_{}", index)),
        description: format!("Complex nested tool {}", index),
        input_schema: json!({
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
        output_schema: Some(json!({
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
        })),
    }
}

/// Creates server info with specified number of tools and complexity.
fn create_server_info(tool_count: usize, tool_creator: fn(usize) -> ToolInfo) -> ServerInfo {
    let tools: Vec<_> = (0..tool_count).map(tool_creator).collect();

    ServerInfo {
        id: ServerId::new(&format!("bench-server-{}", tool_count)),
        name: format!("Benchmark Server (n={})", tool_count),
        version: "1.0.0".to_string(),
        tools,
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
}

// ============================================================================
// Benchmark Functions
// ============================================================================

/// Benchmarks full code generation pipeline for different tool counts.
fn bench_full_generation_scaling(c: &mut Criterion) {
    let mut group = c.benchmark_group("full_generation_scaling");

    for count in [1, 10, 50, 100, 500, 1000] {
        let server_info = create_server_info(count, create_moderate_tool);
        let generator = CodeGenerator::new().expect("Generator should initialize");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _count| {
            b.iter(|| {
                let result = generator.generate(black_box(&server_info));
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
        let generator = CodeGenerator::new().expect("Generator should initialize");

        group.throughput(Throughput::Elements(tool_count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(name), name, |b, _| {
            b.iter(|| {
                let result = generator.generate(black_box(&server_info));
                assert!(result.is_ok());
            });
        });
    }

    group.finish();
}

/// Benchmarks VFS loading performance.
fn bench_vfs_loading(c: &mut Criterion) {
    let mut group = c.benchmark_group("vfs_loading");

    for count in [1, 10, 50, 100, 500] {
        let server_info = create_server_info(count, create_moderate_tool);
        let generator = CodeGenerator::new().expect("Generator should initialize");
        let generated = generator
            .generate(&server_info)
            .expect("Generation should succeed");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _count| {
            b.iter(|| {
                let vfs = VfsBuilder::from_generated_code(
                    black_box(generated.clone()),
                    "/mcp-tools/servers/bench",
                )
                .build();
                assert!(vfs.is_ok());
            });
        });
    }

    group.finish();
}

/// Benchmarks end-to-end workflow (generation + VFS loading).
fn bench_end_to_end(c: &mut Criterion) {
    let mut group = c.benchmark_group("end_to_end");

    for count in [1, 10, 50, 100] {
        let server_info = create_server_info(count, create_moderate_tool);
        let generator = CodeGenerator::new().expect("Generator should initialize");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _count| {
            b.iter(|| {
                // Generate code
                let generated = generator
                    .generate(black_box(&server_info))
                    .expect("Generation should succeed");

                // Load into VFS
                let vfs = VfsBuilder::from_generated_code(generated, "/mcp-tools/servers/bench")
                    .build()
                    .expect("VFS build should succeed");

                // Verify files exist
                assert!(vfs.exists("/mcp-tools/servers/bench/manifest.json"));
            });
        });
    }

    group.finish();
}

/// Benchmarks generator initialization overhead.
fn bench_generator_initialization(c: &mut Criterion) {
    c.bench_function("generator_initialization", |b| {
        b.iter(|| {
            let generator = CodeGenerator::new();
            assert!(generator.is_ok());
            black_box(generator)
        });
    });
}

/// Benchmarks type conversion performance.
fn bench_type_conversion(c: &mut Criterion) {
    use mcp_codegen::common::typescript;

    let mut group = c.benchmark_group("type_conversion");

    // Simple schema
    let simple_schema = json!({"type": "string"});
    group.bench_function("simple_schema", |b| {
        b.iter(|| {
            let result = typescript::json_schema_to_typescript(black_box(&simple_schema));
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
            let result = typescript::json_schema_to_typescript(black_box(&complex_schema));
            black_box(result)
        });
    });

    // Property extraction
    group.bench_function("property_extraction", |b| {
        b.iter(|| {
            let result = typescript::extract_properties(black_box(&complex_schema));
            black_box(result)
        });
    });

    // String conversion
    group.bench_function("snake_to_camel", |b| {
        b.iter(|| {
            let result = typescript::to_camel_case(black_box("send_message_to_chat_room"));
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
    let generator = CodeGenerator::new().expect("Generator should initialize");

    group.bench_function("generate_100_tools", |b| {
        b.iter(|| {
            let generated = generator
                .generate(black_box(&server_info))
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
        bench_vfs_loading,
        bench_end_to_end,
        bench_memory_patterns,
);

criterion_main!(benches);
