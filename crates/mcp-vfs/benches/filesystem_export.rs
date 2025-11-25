//! Benchmarks for filesystem export operations.
//!
//! Measures performance of VFS → disk export with various file counts
//! and optimization strategies.
//!
//! # Target Metrics
//!
//! - Export 30 files (GitHub server): <50ms
//! - Per-file overhead: <2ms
//! - Memory usage: <10MB for 100 files
//!
//! # Run Benchmarks
//!
//! ```bash
//! cargo bench --bench filesystem_export
//! ```
//!
//! # View Results
//!
//! ```bash
//! open target/criterion/report/index.html
//! ```

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mcp_vfs::{ExportOptions, VfsBuilder};
use std::hint::black_box;
use tempfile::TempDir;

/// Benchmark export performance across different file counts.
///
/// This is the primary benchmark for progressive loading pattern.
/// Target: 30 files in <50ms (GitHub server typical case).
fn bench_export_file_count(c: &mut Criterion) {
    let mut group = c.benchmark_group("export_by_file_count");

    // Test common scenarios
    for file_count in [1, 10, 30, 50, 100] {
        let mut builder = VfsBuilder::new();

        // Generate files similar to GitHub server tools
        for i in 0..file_count {
            builder = builder.add_file(
                format!("/tools/tool{i}.ts"),
                format!(
                    r#"/**
 * Tool {i} implementation
 */
export async function tool{i}(params: Params{i}): Promise<Result{i}> {{
    return await callMCPTool('server', 'tool{i}', params);
}}

export type Params{i} = {{
    id: string;
    data: unknown;
}};

export type Result{i} = {{
    success: boolean;
    data: unknown;
}};
"#
                ),
            );
        }

        let vfs = builder.build().unwrap();

        group.bench_with_input(
            BenchmarkId::new("sequential", file_count),
            &file_count,
            |b, _| {
                b.iter_batched(
                    || TempDir::new().unwrap(),
                    |temp| {
                        vfs.export_to_filesystem(black_box(temp.path())).unwrap();
                        temp
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

/// Benchmark atomic vs non-atomic writes.
///
/// Atomic writes are safer but slower. Measures the overhead.
fn bench_export_atomic_vs_direct(c: &mut Criterion) {
    let mut group = c.benchmark_group("export_atomic_vs_direct");

    let mut builder = VfsBuilder::new();
    for i in 0..30 {
        builder = builder.add_file(format!("/tool{i}.ts"), format!("export const N = {i};"));
    }
    let vfs = builder.build().unwrap();

    // Atomic writes
    group.bench_function("atomic_writes", |b| {
        b.iter_batched(
            || TempDir::new().unwrap(),
            |temp| {
                let options = ExportOptions::default().with_atomic_writes(true);
                vfs.export_to_filesystem_with_options(black_box(temp.path()), &options)
                    .unwrap();
                temp
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Direct writes
    group.bench_function("direct_writes", |b| {
        b.iter_batched(
            || TempDir::new().unwrap(),
            |temp| {
                let options = ExportOptions::default().with_atomic_writes(false);
                vfs.export_to_filesystem_with_options(black_box(temp.path()), &options)
                    .unwrap();
                temp
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark parallel vs sequential export.
///
/// Parallel export is faster for large file counts (>50 files).
#[cfg(feature = "parallel")]
fn bench_export_parallel_vs_sequential(c: &mut Criterion) {
    let mut group = c.benchmark_group("export_parallel_vs_sequential");

    for file_count in [50, 100, 200] {
        let mut builder = VfsBuilder::new();
        for i in 0..file_count {
            builder = builder.add_file(
                format!("/tool{i}.ts"),
                format!("export function tool{i}() {{ return {i}; }}"),
            );
        }
        let vfs = builder.build().unwrap();

        // Sequential
        group.bench_with_input(
            BenchmarkId::new("sequential", file_count),
            &file_count,
            |b, _| {
                b.iter_batched(
                    || TempDir::new().unwrap(),
                    |temp| {
                        vfs.export_to_filesystem(black_box(temp.path())).unwrap();
                        temp
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );

        // Parallel
        group.bench_with_input(
            BenchmarkId::new("parallel", file_count),
            &file_count,
            |b, _| {
                b.iter_batched(
                    || TempDir::new().unwrap(),
                    |temp| {
                        vfs.export_to_filesystem_parallel(black_box(temp.path()))
                            .unwrap();
                        temp
                    },
                    criterion::BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

/// Benchmark export with different directory nesting levels.
///
/// Measures directory creation overhead.
fn bench_export_directory_depth(c: &mut Criterion) {
    let mut group = c.benchmark_group("export_directory_depth");

    for depth in [1, 5, 10] {
        let mut builder = VfsBuilder::new();

        // Create 30 files at various depths
        for i in 0..30 {
            let path_parts: Vec<String> = (0..depth).map(|d| format!("dir{d}")).collect();
            let path = format!("/{}/file{i}.ts", path_parts.join("/"));
            builder = builder.add_file(path, format!("export const N = {i};"));
        }

        let vfs = builder.build().unwrap();

        group.bench_with_input(BenchmarkId::new("depth", depth), &depth, |b, _| {
            b.iter_batched(
                || TempDir::new().unwrap(),
                |temp| {
                    vfs.export_to_filesystem(black_box(temp.path())).unwrap();
                    temp
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// Benchmark export with different file sizes.
///
/// Measures I/O scaling characteristics.
fn bench_export_file_size(c: &mut Criterion) {
    let mut group = c.benchmark_group("export_file_size");

    for size_kb in [1, 10, 50, 100] {
        let content = "x".repeat(size_kb * 1024);
        let mut builder = VfsBuilder::new();

        // Create 10 files of given size
        for i in 0..10 {
            builder = builder.add_file(format!("/file{i}.ts"), content.clone());
        }

        let vfs = builder.build().unwrap();

        group.bench_with_input(BenchmarkId::new("size_kb", size_kb), &size_kb, |b, _| {
            b.iter_batched(
                || TempDir::new().unwrap(),
                |temp| {
                    vfs.export_to_filesystem(black_box(temp.path())).unwrap();
                    temp
                },
                criterion::BatchSize::SmallInput,
            );
        });
    }

    group.finish();
}

/// Benchmark typical GitHub server export (30 tools).
///
/// This is the real-world scenario we're optimizing for.
fn bench_export_github_scenario(c: &mut Criterion) {
    // Simulate GitHub server with 30 realistic tool files
    let mut builder = VfsBuilder::new();

    // Add index.ts
    let mut exports = Vec::new();
    for i in 0..30 {
        exports.push(format!("export {{ tool{i} }} from './tool{i}.js';"));
    }
    builder = builder.add_file("/index.ts", exports.join("\n"));

    // Add manifest
    builder = builder.add_file(
        "/manifest.json",
        r#"{
  "name": "github",
  "version": "1.0.0",
  "tools": 30
}"#,
    );

    // Add 30 tool files (realistic size)
    for i in 0..30 {
        builder = builder.add_file(
            format!("/tool{i}.ts"),
            format!(
                r#"/**
 * GitHub tool {i}
 *
 * @param params - Tool parameters
 * @returns Tool result
 */
export async function tool{i}(params: {{
    repo: string;
    issue_number?: number;
    title?: string;
    body?: string;
    state?: "open" | "closed";
    labels?: string[];
}}) {{
    return await callMCPTool('github', 'tool{i}', params);
}}

type ToolParams{i} = {{
    repo: string;
    issue_number?: number;
    title?: string;
    body?: string;
    state?: "open" | "closed";
    labels?: string[];
}};

type ToolResult{i} = {{
    id: number;
    title: string;
    body: string;
    state: string;
    created_at: string;
    updated_at: string;
}};

async function callMCPTool(server: string, tool: string, params: unknown): Promise<unknown> {{
    // Bridge implementation
    return {{}};
}}
"#
            ),
        );
    }

    let vfs = builder.build().unwrap();

    c.bench_function("github_30_tools", |b| {
        b.iter_batched(
            || TempDir::new().unwrap(),
            |temp| {
                vfs.export_to_filesystem(black_box(temp.path())).unwrap();
                temp
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

/// Benchmark export with overwrite vs skip.
fn bench_export_overwrite_behavior(c: &mut Criterion) {
    let mut group = c.benchmark_group("export_overwrite");

    let mut builder = VfsBuilder::new();
    for i in 0..30 {
        builder = builder.add_file(format!("/file{i}.ts"), format!("export const N = {i};"));
    }
    let vfs = builder.build().unwrap();

    // Overwrite existing files
    group.bench_function("overwrite_existing", |b| {
        b.iter_batched(
            || {
                let temp = TempDir::new().unwrap();
                // Pre-create files
                vfs.export_to_filesystem(temp.path()).unwrap();
                temp
            },
            |temp| {
                let options = ExportOptions::default().with_overwrite(true);
                vfs.export_to_filesystem_with_options(black_box(temp.path()), &options)
                    .unwrap();
                temp
            },
            criterion::BatchSize::SmallInput,
        );
    });

    // Skip existing files
    group.bench_function("skip_existing", |b| {
        b.iter_batched(
            || {
                let temp = TempDir::new().unwrap();
                // Pre-create files
                vfs.export_to_filesystem(temp.path()).unwrap();
                temp
            },
            |temp| {
                let options = ExportOptions::default().with_overwrite(false);
                vfs.export_to_filesystem_with_options(black_box(temp.path()), &options)
                    .unwrap();
                temp
            },
            criterion::BatchSize::SmallInput,
        );
    });

    group.finish();
}

/// Benchmark build + export workflow (typical usage).
fn bench_export_full_workflow(c: &mut Criterion) {
    c.bench_function("build_and_export_30_files", |b| {
        b.iter_batched(
            || TempDir::new().unwrap(),
            |temp| {
                let mut builder = VfsBuilder::new();
                for i in 0..30 {
                    builder = builder.add_file(
                        format!("/tool{i}.ts"),
                        format!("export function tool{i}() {{ return {i}; }}"),
                    );
                }
                let vfs = builder.build().unwrap();
                vfs.export_to_filesystem(black_box(temp.path())).unwrap();
                temp
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(
    benches,
    bench_export_file_count,
    bench_export_atomic_vs_direct,
    bench_export_directory_depth,
    bench_export_file_size,
    bench_export_github_scenario,
    bench_export_overwrite_behavior,
    bench_export_full_workflow,
);

#[cfg(feature = "parallel")]
criterion_group!(parallel_benches, bench_export_parallel_vs_sequential,);

#[cfg(not(feature = "parallel"))]
criterion_main!(benches);

#[cfg(feature = "parallel")]
criterion_main!(benches, parallel_benches);
