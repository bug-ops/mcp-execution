//! Multi-file skill generation performance benchmarks.
//!
//! Compares single-file (baseline) vs multi-file skill generation performance.
//! Tests the new multi-file structure:
//! - SKILL.md (main file)
//! - scripts/*.ts (individual tool scripts)
//! - reference.md (API documentation)
//!
//! Run with: `cargo bench --package mcp-codegen --bench multi_file_generation`

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use mcp_codegen::TemplateEngine;
use mcp_core::{ServerId, SkillDescription, SkillName, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::hint::black_box as bb;

// ============================================================================
// Test Data Generators (matching code_generation.rs for comparison)
// ============================================================================

/// Creates a tool with moderate schema complexity.
fn create_moderate_tool(index: usize) -> ToolInfo {
    ToolInfo {
        name: ToolName::new(format!("moderate_tool_{index}")),
        description: format!("Moderate complexity tool {index}"),
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

/// Creates server info with specified number of tools.
fn create_server_info(tool_count: usize) -> ServerInfo {
    let tools: Vec<_> = (0..tool_count).map(create_moderate_tool).collect();

    ServerInfo {
        id: ServerId::new(format!("bench-server-{tool_count}")),
        name: format!("Benchmark Server (n={tool_count})"),
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

/// Benchmarks multi-file skill generation vs single-file baseline.
///
/// NOTE: This benchmark will work once multi-file implementation is complete.
/// Currently uses claude skill format (similar structure to multi-file).
#[cfg(feature = "skills")]
fn bench_multi_file_vs_baseline(c: &mut Criterion) {
    use mcp_codegen::skills::claude::render_skill_md;
    use mcp_codegen::skills::converter::SkillConverter;

    let mut group = c.benchmark_group("multi_file_vs_baseline");

    for count in [1, 10, 50, 100] {
        let server_info = create_server_info(count);
        let skill_name = SkillName::new("benchmark-skill").unwrap();
        let skill_desc = SkillDescription::new("Benchmark skill for performance testing").unwrap();
        let engine = TemplateEngine::new().expect("Engine should initialize");

        group.throughput(Throughput::Elements(count as u64));

        // Benchmark current (baseline) implementation
        group.bench_with_input(BenchmarkId::new("baseline", count), &count, |b, _count| {
            b.iter(|| {
                let skill_data =
                    SkillConverter::convert(bb(&server_info), bb(&skill_name), bb(&skill_desc))
                        .expect("Conversion should succeed");
                let _skill_md = render_skill_md(bb(&engine), bb(&skill_data))
                    .expect("Rendering should succeed");
            });
        });

        // TODO: Add multi-file benchmark when implementation is ready
        // group.bench_with_input(
        //     BenchmarkId::new("multi_file", count),
        //     &count,
        //     |b, _count| {
        //         b.iter(|| {
        //             let bundle = generate_skill_bundle(bb(&server_info), bb(&skill_name), bb(&skill_desc))
        //                 .expect("Bundle generation should succeed");
        //             bb(bundle);
        //         });
        //     },
        // );
    }

    group.finish();
}

/// Benchmarks individual script generation (multi-file specific).
///
/// Measures the cost of generating individual TypeScript scripts for each tool.
#[cfg(feature = "skills")]
#[allow(dead_code, clippy::missing_const_for_fn)]
const fn bench_script_generation(_c: &mut Criterion) {
    // TODO: Implement when multi-file script generation is available
    // let mut group = c.benchmark_group("script_generation");
    //
    // for count in [1, 10, 50, 100] {
    //     let server_info = create_server_info(count);
    //
    //     group.throughput(Throughput::Elements(count as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _count| {
    //         b.iter(|| {
    //             let scripts = generate_tool_scripts(bb(&server_info))
    //                 .expect("Script generation should succeed");
    //             bb(scripts);
    //         });
    //     });
    // }
    //
    // group.finish();
}

/// Benchmarks file I/O overhead in multi-file structure.
///
/// Measures the cost of writing multiple files vs single file.
#[cfg(feature = "skills")]
#[allow(dead_code, clippy::missing_const_for_fn)]
const fn bench_file_io_overhead(_c: &mut Criterion) {
    // TODO: Implement when multi-file VFS integration is available
    // let mut group = c.benchmark_group("file_io_overhead");
    //
    // for count in [1, 10, 50, 100] {
    //     let bundle = create_skill_bundle(count);
    //
    //     group.throughput(Throughput::Elements(count as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _count| {
    //         b.iter(|| {
    //             let vfs = write_bundle_to_vfs(bb(&bundle))
    //                 .expect("VFS write should succeed");
    //             bb(vfs);
    //         });
    //     });
    // }
    //
    // group.finish();
}

/// Benchmarks template rendering for multi-file structure.
///
/// Compares template complexity between single-file and multi-file.
#[cfg(feature = "skills")]
#[allow(dead_code, clippy::missing_const_for_fn)]
const fn bench_template_complexity(_c: &mut Criterion) {
    // TODO: Implement when multi-file templates are available
    // let mut group = c.benchmark_group("template_complexity");
    //
    // let server_info = create_server_info(50);
    // let skill_name = SkillName::new("benchmark-skill").unwrap();
    // let skill_desc = SkillDescription::new("Benchmark skill").unwrap();
    // let engine = TemplateEngine::new().unwrap();
    //
    // group.bench_function("single_file_template", |b| {
    //     b.iter(|| {
    //         let skill_data = SkillConverter::convert(&server_info, &skill_name, &skill_desc).unwrap();
    //         let _skill_md = render_skill_md(&engine, &skill_data).unwrap();
    //     });
    // });
    //
    // group.bench_function("multi_file_templates", |b| {
    //     b.iter(|| {
    //         let bundle = generate_skill_bundle(&server_info, &skill_name, &skill_desc).unwrap();
    //         bb(bundle);
    //     });
    // });
    //
    // group.finish();
}

/// Benchmarks end-to-end skill generation with disk writing.
///
/// Measures full workflow including file system operations.
#[cfg(feature = "skills")]
#[allow(dead_code, clippy::missing_const_for_fn)]
const fn bench_end_to_end_multi_file(_c: &mut Criterion) {
    // TODO: Implement when multi-file implementation is complete
    // let mut group = c.benchmark_group("end_to_end_multi_file");
    //
    // for count in [1, 10, 50, 100] {
    //     let server_info = create_server_info(count);
    //     let skill_name = SkillName::new("benchmark-skill").unwrap();
    //     let skill_desc = SkillDescription::new("Benchmark skill").unwrap();
    //
    //     group.throughput(Throughput::Elements(count as u64));
    //     group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _count| {
    //         b.iter(|| {
    //             // Generate bundle
    //             let bundle = generate_skill_bundle(&server_info, &skill_name, &skill_desc)
    //                 .expect("Bundle generation should succeed");
    //
    //             // Write to VFS
    //             let vfs = write_bundle_to_vfs(&bundle)
    //                 .expect("VFS write should succeed");
    //
    //             // Verify files exist
    //             assert!(vfs.exists("/skills/benchmark-skill/SKILL.md"));
    //             bb(vfs);
    //         });
    //     });
    // }
    //
    // group.finish();
}

/// Benchmarks memory allocation patterns in multi-file generation.
#[cfg(feature = "skills")]
#[allow(dead_code, clippy::missing_const_for_fn)]
const fn bench_memory_patterns_multi_file(_c: &mut Criterion) {
    // TODO: Implement when multi-file implementation is complete
    // let mut group = c.benchmark_group("memory_patterns_multi_file");
    // group.sample_size(50); // Fewer samples for memory-focused tests
    //
    // let server_info = create_server_info(100);
    // let skill_name = SkillName::new("benchmark-skill").unwrap();
    // let skill_desc = SkillDescription::new("Benchmark skill").unwrap();
    //
    // group.bench_function("generate_100_tools_multi_file", |b| {
    //     b.iter(|| {
    //         let bundle = generate_skill_bundle(&server_info, &skill_name, &skill_desc)
    //             .expect("Should succeed");
    //
    //         // Force evaluation of all generated files
    //         for file in &bundle.files {
    //             bb(&file.path);
    //             bb(&file.content);
    //         }
    //     });
    // });
    //
    // group.finish();
}

// ============================================================================
// Comparison Benchmark: Single-File Baseline
// ============================================================================

/// Baseline benchmark for comparison (mirrors `code_generation.rs`).
///
/// This establishes the performance target for multi-file implementation.
#[cfg(feature = "skills")]
fn bench_baseline_reference(c: &mut Criterion) {
    use mcp_codegen::skills::claude::render_skill_md;
    use mcp_codegen::skills::converter::SkillConverter;

    let mut group = c.benchmark_group("baseline_reference");

    for count in [1, 10, 50, 100] {
        let server_info = create_server_info(count);
        let skill_name = SkillName::new("benchmark-skill").unwrap();
        let skill_desc = SkillDescription::new("Benchmark skill for performance testing").unwrap();
        let engine = TemplateEngine::new().expect("Engine should initialize");

        group.throughput(Throughput::Elements(count as u64));
        group.bench_with_input(BenchmarkId::from_parameter(count), &count, |b, _count| {
            b.iter(|| {
                let skill_data =
                    SkillConverter::convert(bb(&server_info), bb(&skill_name), bb(&skill_desc))
                        .expect("Conversion should succeed");
                let _skill_md = render_skill_md(bb(&engine), bb(&skill_data))
                    .expect("Rendering should succeed");
            });
        });
    }

    group.finish();
}

// ============================================================================
// Benchmark Configuration
// ============================================================================

#[cfg(feature = "skills")]
criterion_group!(
    name = benches;
    config = Criterion::default()
        .sample_size(100)
        .measurement_time(std::time::Duration::from_secs(10))
        .warm_up_time(std::time::Duration::from_secs(3))
        .significance_level(0.05)
        .noise_threshold(0.02);
    targets =
        bench_baseline_reference,
        bench_multi_file_vs_baseline,
        // TODO: Enable when implementations are ready
        // bench_script_generation,
        // bench_file_io_overhead,
        // bench_template_complexity,
        // bench_end_to_end_multi_file,
        // bench_memory_patterns_multi_file,
);

#[cfg(not(feature = "skills"))]
criterion_group!(
    name = benches;
    config = Criterion::default();
    targets = // No benchmarks when skills feature is disabled
);

criterion_main!(benches);
