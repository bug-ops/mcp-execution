//! Criterion benchmarks for WASM runtime performance.
//!
//! Run with: cargo bench --package mcp-wasm-runtime

use criterion::{Criterion, criterion_group, criterion_main};
use mcp_bridge::Bridge;
use mcp_wasm_runtime::{ModuleCache, Runtime, SecurityConfig};
use std::hint::black_box;
use std::sync::Arc;

/// Benchmark WASM module compilation time.
fn bench_module_compilation(c: &mut Criterion) {
    let wat = r#"
        (module
            (func $add (param $a i32) (param $b i32) (result i32)
                local.get $a
                local.get $b
                i32.add
            )
            (func (export "main") (result i32)
                (i32.const 10)
                (i32.const 32)
                (call $add)
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    c.bench_function("module_compilation", |b| {
        b.iter(|| {
            let engine = wasmtime::Engine::default();
            let module = wasmtime::Module::new(&engine, black_box(&wasm_bytes))
                .expect("Failed to compile module");
            black_box(module)
        });
    });
}

/// Benchmark cached module execution.
fn bench_cached_execution(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let wat = r#"
        (module
            (func (export "main") (result i32)
                (i32.const 42)
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    let bridge = Arc::new(Bridge::new(1000));
    let config = SecurityConfig::default();
    let wasm_runtime = Runtime::new(bridge, config).expect("Failed to create runtime");

    // Warm up cache
    runtime.block_on(async {
        let _ = wasm_runtime.execute(&wasm_bytes, "main", &Vec::new()).await;
    });

    c.bench_function("cached_execution", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let result = wasm_runtime
                    .execute(black_box(&wasm_bytes), "main", &Vec::new())
                    .await;
                black_box(result)
            })
        });
    });
}

/// Benchmark first-time execution (compilation + execution).
fn bench_first_execution(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    let wat = r#"
        (module
            (func (export "main") (result i32)
                (i32.const 42)
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    c.bench_function("first_execution", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let bridge = Arc::new(Bridge::new(1000));
                let config = SecurityConfig::default();
                let wasm_runtime = Runtime::new(bridge, config).expect("Failed to create runtime");

                let result = wasm_runtime
                    .execute(black_box(&wasm_bytes), "main", &Vec::new())
                    .await;
                black_box(result)
            })
        });
    });
}

/// Benchmark cache key generation.
fn bench_cache_key_generation(c: &mut Criterion) {
    let wasm_bytes = vec![0u8; 1024]; // 1KB of data

    c.bench_function("cache_key_generation", |b| {
        b.iter(|| {
            let key = ModuleCache::cache_key_for_code(black_box(&wasm_bytes));
            black_box(key)
        });
    });
}

/// Benchmark cache operations.
fn bench_cache_operations(c: &mut Criterion) {
    let cache = ModuleCache::new(100);
    let engine = wasmtime::Engine::default();
    let wat = "(module)";
    let wasm = wat::parse_str(wat).expect("Failed to parse WAT");
    let module = wasmtime::Module::new(&engine, &wasm).expect("Failed to create module");

    c.bench_function("cache_insert", |b| {
        b.iter(|| {
            let key = ModuleCache::cache_key_for_code(b"test");
            cache.insert(black_box(key), black_box(module.clone()));
        });
    });

    // Populate cache for get benchmark
    let test_key = ModuleCache::cache_key_for_code(b"test_get");
    cache.insert(test_key.clone(), module.clone());

    c.bench_function("cache_get", |b| {
        b.iter(|| {
            let result = cache.get(black_box(&test_key));
            black_box(result)
        });
    });
}

/// Benchmark complex WASM execution.
fn bench_complex_execution(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    // More complex module with loops and function calls
    let wat = r#"
        (module
            (func $fibonacci (param $n i32) (result i32)
                (local $a i32)
                (local $b i32)
                (local $temp i32)
                (local $i i32)

                (local.set $a (i32.const 0))
                (local.set $b (i32.const 1))
                (local.set $i (i32.const 0))

                (block $exit
                    (loop $continue
                        (br_if $exit (i32.ge_u (local.get $i) (local.get $n)))

                        (local.set $temp (local.get $b))
                        (local.set $b (i32.add (local.get $a) (local.get $b)))
                        (local.set $a (local.get $temp))

                        (local.set $i (i32.add (local.get $i) (i32.const 1)))
                        (br $continue)
                    )
                )

                (local.get $a)
            )

            (func (export "main") (result i32)
                (i32.const 10)
                (call $fibonacci)
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    let bridge = Arc::new(Bridge::new(1000));
    let config = SecurityConfig::default();
    let wasm_runtime = Runtime::new(bridge, config).expect("Failed to create runtime");

    // Warm up cache
    runtime.block_on(async {
        let _ = wasm_runtime.execute(&wasm_bytes, "main", &Vec::new()).await;
    });

    c.bench_function("complex_execution", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let result = wasm_runtime
                    .execute(black_box(&wasm_bytes), "main", &Vec::new())
                    .await;
                black_box(result)
            })
        });
    });
}

/// Benchmark host function calls (simplified - measures overhead).
fn bench_host_function_overhead(c: &mut Criterion) {
    let runtime = tokio::runtime::Runtime::new().unwrap();

    // Module that calls host function
    let wat = r#"
        (module
            (import "env" "host_add" (func $host_add (param i32 i32) (result i32)))
            (func (export "main") (result i32)
                (i32.const 10)
                (i32.const 32)
                (call $host_add)
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    let bridge = Arc::new(Bridge::new(1000));
    let config = SecurityConfig::default();
    let wasm_runtime = Runtime::new(bridge, config).expect("Failed to create runtime");

    // Warm up cache
    runtime.block_on(async {
        let _ = wasm_runtime.execute(&wasm_bytes, "main", &Vec::new()).await;
    });

    c.bench_function("host_function_overhead", |b| {
        b.iter(|| {
            runtime.block_on(async {
                let result = wasm_runtime
                    .execute(black_box(&wasm_bytes), "main", &Vec::new())
                    .await;
                black_box(result)
            })
        });
    });
}

criterion_group!(
    benches,
    bench_module_compilation,
    bench_cached_execution,
    bench_first_execution,
    bench_cache_key_generation,
    bench_cache_operations,
    bench_complex_execution,
    bench_host_function_overhead,
);

criterion_main!(benches);
