//! Benchmarks for WASM module handling with various sizes.
//!
//! Tests checksum calculation and I/O performance for different WASM module
//! sizes (500KB - 10MB range typical for real-world plugins).

#![allow(clippy::cast_possible_truncation)]

use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use mcp_skill_store::checksum::calculate_checksum;
use mcp_skill_store::{ServerInfo, SkillStore};
use mcp_vfs::VfsBuilder;
use std::hint::black_box;
use tempfile::TempDir;

/// Creates a WASM module of the specified size with varied content.
fn create_wasm_module(size_bytes: usize) -> Vec<u8> {
    let mut wasm = vec![0u8; size_bytes];
    // Add WASM magic bytes
    if size_bytes >= 4 {
        wasm[0..4].copy_from_slice(&[0x00, 0x61, 0x73, 0x6D]);
    }
    // Add some variation to avoid cache effects
    for (i, byte) in wasm.iter_mut().enumerate().skip(4) {
        *byte = (i % 256) as u8;
    }
    wasm
}

fn bench_wasm_checksum(c: &mut Criterion) {
    let mut group = c.benchmark_group("wasm_checksum");

    for size_mb in [1, 2, 5, 10] {
        let size_bytes = size_mb * 1024 * 1024;
        let wasm = create_wasm_module(size_bytes);

        group.bench_with_input(BenchmarkId::from_parameter(size_mb), &size_mb, |b, _| {
            b.iter(|| {
                let checksum = calculate_checksum(black_box(&wasm));
                black_box(checksum);
            });
        });
    }

    group.finish();
}

fn bench_wasm_save(c: &mut Criterion) {
    let mut group = c.benchmark_group("wasm_save");

    for size_mb in [1, 2, 5, 10] {
        let size_bytes = size_mb * 1024 * 1024;
        let wasm = create_wasm_module(size_bytes);

        // Minimal VFS (focus on WASM performance)
        let vfs = VfsBuilder::new()
            .add_file("/index.ts", "export const VERSION = '1.0';")
            .build()
            .unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size_mb), &size_mb, |b, _| {
            let temp = TempDir::new().unwrap();
            let store = SkillStore::new(temp.path()).unwrap();

            let server_info = ServerInfo {
                name: format!("wasm-bench-{size_mb}mb"),
                version: "1.0.0".to_string(),
                protocol_version: "2024-11-05".to_string(),
            };

            let mut iteration = 0;
            b.iter(|| {
                let server_name = format!("wasm-{size_mb}mb-{iteration}");
                store
                    .save_skill(
                        &server_name,
                        black_box(&vfs),
                        black_box(&wasm),
                        black_box(server_info.clone()),
                        vec![],
                    )
                    .unwrap();

                store.remove_skill(&server_name).unwrap();
                iteration += 1;
            });
        });
    }

    group.finish();
}

fn bench_wasm_load(c: &mut Criterion) {
    let mut group = c.benchmark_group("wasm_load");

    for size_mb in [1, 2, 5, 10] {
        let size_bytes = size_mb * 1024 * 1024;
        let wasm = create_wasm_module(size_bytes);

        let vfs = VfsBuilder::new()
            .add_file("/index.ts", "export const VERSION = '1.0';")
            .build()
            .unwrap();

        // Setup: Create plugin once
        let temp = TempDir::new().unwrap();
        let store = SkillStore::new(temp.path()).unwrap();

        let server_info = ServerInfo {
            name: format!("wasm-load-{size_mb}mb"),
            version: "1.0.0".to_string(),
            protocol_version: "2024-11-05".to_string(),
        };

        let server_name = format!("wasm-load-{size_mb}mb");
        store
            .save_skill(&server_name, &vfs, &wasm, server_info, vec![])
            .unwrap();

        group.bench_with_input(BenchmarkId::from_parameter(size_mb), &size_mb, |b, _| {
            b.iter(|| {
                let plugin = store.load_skill(black_box(&server_name)).unwrap();
                black_box(plugin);
            });
        });
    }

    group.finish();
}

criterion_group!(
    benches,
    bench_wasm_checksum,
    bench_wasm_save,
    bench_wasm_load
);
criterion_main!(benches);
