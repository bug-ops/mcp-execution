//! Benchmarks for concurrent operations.
//!
//! Tests performance of concurrent save/load operations with different plugins
//! to validate thread safety and measure scaling characteristics.

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mcp_skill_store::{ServerInfo, SkillStore};
use mcp_vfs::VfsBuilder;
use std::hint::black_box;
use std::sync::Arc;
use std::thread;
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

fn create_test_vfs() -> mcp_vfs::Vfs {
    VfsBuilder::new()
        .add_file("/index.ts", "export * from './tools';")
        .add_file("/tools/sendMessage.ts", "export function sendMessage() {}")
        .add_file("/tools/getChatInfo.ts", "export function getChatInfo() {}")
        .add_file("/types.ts", "export type Message = { id: string };")
        .build()
        .unwrap()
}

fn bench_concurrent_save_different_plugins(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_save");

    for thread_count in [1, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(thread_count),
            &thread_count,
            |b, &threads| {
                b.iter(|| {
                    let temp = TempDir::new().unwrap();
                    let store = Arc::new(SkillStore::new(temp.path()).unwrap());

                    let handles: Vec<_> = (0..threads)
                        .map(|i| {
                            let store = Arc::clone(&store);
                            thread::spawn(move || {
                                let vfs = create_test_vfs();
                                let wasm = create_wasm_module(1024 * 1024);
                                let server_info = ServerInfo {
                                    name: format!("plugin-{i}"),
                                    version: "1.0.0".to_string(),
                                    protocol_version: "2024-11-05".to_string(),
                                };

                                store
                                    .save_skill(
                                        &format!("plugin-{i}"),
                                        black_box(&vfs),
                                        black_box(&wasm),
                                        black_box(server_info),
                                        vec![],
                                    )
                                    .unwrap();
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_concurrent_load_same_plugin(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_load");

    // Setup: Create one plugin that will be loaded concurrently
    let temp = TempDir::new().unwrap();
    let store = Arc::new(SkillStore::new(temp.path()).unwrap());

    let vfs = create_test_vfs();
    let wasm = create_wasm_module(1024 * 1024);
    let server_info = ServerInfo {
        name: "shared-plugin".to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
    };

    store
        .save_skill("shared-plugin", &vfs, &wasm, server_info, vec![])
        .unwrap();

    for thread_count in [1, 2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(thread_count),
            &thread_count,
            |b, &threads| {
                b.iter(|| {
                    let handles: Vec<_> = (0..threads)
                        .map(|_| {
                            let store = Arc::clone(&store);
                            thread::spawn(move || {
                                let plugin = store.load_skill(black_box("shared-plugin")).unwrap();
                                black_box(plugin);
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_mixed_concurrent_operations(c: &mut Criterion) {
    let mut group = c.benchmark_group("concurrent_mixed");

    for thread_count in [2, 4, 8] {
        group.bench_with_input(
            BenchmarkId::from_parameter(thread_count),
            &thread_count,
            |b, &threads| {
                b.iter(|| {
                    let temp = TempDir::new().unwrap();
                    let store = Arc::new(SkillStore::new(temp.path()).unwrap());

                    // Pre-create some plugins for reading
                    for i in 0..threads / 2 {
                        let vfs = create_test_vfs();
                        let wasm = create_wasm_module(1024 * 1024);
                        let server_info = ServerInfo {
                            name: format!("existing-{i}"),
                            version: "1.0.0".to_string(),
                            protocol_version: "2024-11-05".to_string(),
                        };
                        store
                            .save_skill(&format!("existing-{i}"), &vfs, &wasm, server_info, vec![])
                            .unwrap();
                    }

                    let handles: Vec<_> = (0..threads)
                        .map(|i| {
                            let store = Arc::clone(&store);
                            thread::spawn(move || {
                                if i % 2 == 0 {
                                    // Even threads: save new plugins
                                    let vfs = create_test_vfs();
                                    let wasm = create_wasm_module(1024 * 1024);
                                    let server_info = ServerInfo {
                                        name: format!("new-{i}"),
                                        version: "1.0.0".to_string(),
                                        protocol_version: "2024-11-05".to_string(),
                                    };
                                    store
                                        .save_skill(
                                            &format!("new-{i}"),
                                            black_box(&vfs),
                                            black_box(&wasm),
                                            black_box(server_info),
                                            vec![],
                                        )
                                        .unwrap();
                                } else {
                                    // Odd threads: load existing plugins
                                    let plugin = store
                                        .load_skill(black_box(&format!("existing-{}", i / 2)))
                                        .unwrap();
                                    black_box(plugin);
                                }
                            })
                        })
                        .collect();

                    for handle in handles {
                        handle.join().unwrap();
                    }
                });
            },
        );
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_concurrent_save_different_plugins,
    bench_concurrent_load_same_plugin,
    bench_mixed_concurrent_operations
);
criterion_main!(benches);
