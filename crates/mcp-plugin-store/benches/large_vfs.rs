//! Benchmarks for large VFS operations.
//!
//! Tests save/load performance with varying numbers of files to identify
//! scaling characteristics and potential bottlenecks.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mcp_plugin_store::{PluginStore, ServerInfo};
use mcp_vfs::VfsBuilder;
use std::hint::black_box;
use tempfile::TempDir;

/// Creates a WASM module of the specified size.
fn create_wasm_module(size_bytes: usize) -> Vec<u8> {
    let mut wasm = vec![0u8; size_bytes];
    // Add WASM magic bytes at the start
    if size_bytes >= 4 {
        wasm[0..4].copy_from_slice(&[0x00, 0x61, 0x73, 0x6D]);
    }
    wasm
}

fn bench_save_large_vfs(c: &mut Criterion) {
    let mut group = c.benchmark_group("save_large_vfs");

    for file_count in [10, 50, 100, 500, 1000] {
        group.bench_with_input(
            BenchmarkId::from_parameter(file_count),
            &file_count,
            |b, &count| {
                // Setup happens outside the benchmark timing
                let temp = TempDir::new().unwrap();
                let store = PluginStore::new(temp.path()).unwrap();

                // Generate VFS with N files
                let mut builder = VfsBuilder::new();
                for i in 0..count {
                    let path = format!("/tools/tool_{i}.ts");
                    let content = format!(
                        "export function tool_{i}() {{ return {i}; }}\n\
                         // Additional content to simulate real files\n\
                         export const METADATA = {{ id: {i}, name: 'tool_{i}' }};"
                    );
                    builder = builder.add_file(&path, content);
                }
                let vfs = builder.build().unwrap();

                // 1MB WASM module (typical size)
                let wasm = create_wasm_module(1024 * 1024);

                let server_info = ServerInfo {
                    name: format!("bench-server-{count}"),
                    version: "1.0.0".to_string(),
                    protocol_version: "2024-11-05".to_string(),
                };

                let mut iteration = 0;
                b.iter(|| {
                    // Each iteration uses a different server name to avoid conflicts
                    let server_name = format!("test-{count}-{iteration}");
                    store
                        .save_plugin(
                            &server_name,
                            black_box(&vfs),
                            black_box(&wasm),
                            black_box(server_info.clone()),
                            vec![],
                        )
                        .unwrap();

                    // Clean up for next iteration
                    store.remove_plugin(&server_name).unwrap();
                    iteration += 1;
                });
            },
        );
    }

    group.finish();
}

fn bench_load_large_vfs(c: &mut Criterion) {
    let mut group = c.benchmark_group("load_large_vfs");

    for file_count in [10, 50, 100, 500, 1000] {
        // Setup: Create plugin once for all iterations
        let temp = TempDir::new().unwrap();
        let store = PluginStore::new(temp.path()).unwrap();

        let mut builder = VfsBuilder::new();
        for i in 0..file_count {
            let path = format!("/tools/tool_{i}.ts");
            let content = format!(
                "export function tool_{i}() {{ return {i}; }}\n\
                 export const METADATA = {{ id: {i}, name: 'tool_{i}' }};"
            );
            builder = builder.add_file(&path, content);
        }
        let vfs = builder.build().unwrap();

        let wasm = create_wasm_module(1024 * 1024);
        let server_info = ServerInfo {
            name: format!("bench-server-{file_count}"),
            version: "1.0.0".to_string(),
            protocol_version: "2024-11-05".to_string(),
        };

        let server_name = format!("bench-{file_count}");
        store
            .save_plugin(&server_name, &vfs, &wasm, server_info, vec![])
            .unwrap();

        group.bench_with_input(
            BenchmarkId::from_parameter(file_count),
            &file_count,
            |b, _| {
                b.iter(|| {
                    let plugin = store.load_plugin(black_box(&server_name)).unwrap();
                    black_box(plugin);
                });
            },
        );
    }

    group.finish();
}

fn bench_save_load_roundtrip(c: &mut Criterion) {
    let mut group = c.benchmark_group("save_load_roundtrip");

    for file_count in [10, 50, 100] {
        group.bench_with_input(
            BenchmarkId::from_parameter(file_count),
            &file_count,
            |b, &count| {
                let temp = TempDir::new().unwrap();
                let store = PluginStore::new(temp.path()).unwrap();

                let mut builder = VfsBuilder::new();
                for i in 0..count {
                    let path = format!("/tools/tool_{i}.ts");
                    let content = format!("export function tool_{i}() {{ return {i}; }}");
                    builder = builder.add_file(&path, content);
                }
                let vfs = builder.build().unwrap();

                let wasm = create_wasm_module(1024 * 1024);
                let server_info = ServerInfo {
                    name: format!("roundtrip-{count}"),
                    version: "1.0.0".to_string(),
                    protocol_version: "2024-11-05".to_string(),
                };

                let mut iteration = 0;
                b.iter(|| {
                    let server_name = format!("roundtrip-{count}-{iteration}");

                    // Save
                    store
                        .save_plugin(
                            &server_name,
                            black_box(&vfs),
                            black_box(&wasm),
                            black_box(server_info.clone()),
                            vec![],
                        )
                        .unwrap();

                    // Load
                    let plugin = store.load_plugin(black_box(&server_name)).unwrap();
                    black_box(plugin);

                    // Cleanup
                    store.remove_plugin(&server_name).unwrap();
                    iteration += 1;
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_save_large_vfs,
    bench_load_large_vfs,
    bench_save_load_roundtrip
);
criterion_main!(benches);
