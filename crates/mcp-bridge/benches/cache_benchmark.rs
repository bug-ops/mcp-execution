//! Benchmarks for mcp-bridge caching performance
//!
//! These benchmarks measure the performance characteristics of the LRU cache
//! and cache key generation.

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use mcp_core::CacheKey;
use std::hint::black_box;

/// Benchmarks cache key generation from components
fn bench_cache_key_generation(c: &mut Criterion) {
    let mut group = c.benchmark_group("cache_key_generation");

    // Test with varying parameter sizes
    for size in [10, 100, 1000, 10000].iter() {
        let params = "x".repeat(*size);

        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), &params, |b, params| {
            b.iter(|| {
                CacheKey::from_parts(
                    black_box("test-server"),
                    black_box("test-tool"),
                    black_box(params),
                )
            });
        });
    }

    group.finish();
}

/// Benchmarks cache key creation from string
fn bench_cache_key_new(c: &mut Criterion) {
    c.bench_function("cache_key_new", |b| {
        b.iter(|| CacheKey::new(black_box("test-cache-key")));
    });
}

/// Benchmarks cache key hash consistency
fn bench_cache_key_consistency(c: &mut Criterion) {
    c.bench_function("cache_key_consistency", |b| {
        b.iter(|| {
            let key1 = CacheKey::from_parts(
                black_box("server"),
                black_box("tool"),
                black_box(r#"{"arg": "value"}"#),
            );
            let key2 = CacheKey::from_parts(
                black_box("server"),
                black_box("tool"),
                black_box(r#"{"arg": "value"}"#),
            );
            assert_eq!(key1, key2);
        });
    });
}

/// Benchmarks cache key comparison
fn bench_cache_key_comparison(c: &mut Criterion) {
    let key1 = CacheKey::from_parts("server", "tool", "params1");
    let key2 = CacheKey::from_parts("server", "tool", "params2");

    c.bench_function("cache_key_eq", |b| {
        b.iter(|| black_box(&key1) == black_box(&key2));
    });
}

/// Benchmarks cache key hashing for HashMap lookups
fn bench_cache_key_hash(c: &mut Criterion) {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    let key = CacheKey::from_parts("server", "tool", "params");

    c.bench_function("cache_key_hash", |b| {
        b.iter(|| {
            let mut hasher = DefaultHasher::new();
            black_box(&key).hash(&mut hasher);
            hasher.finish()
        });
    });
}

criterion_group!(
    benches,
    bench_cache_key_generation,
    bench_cache_key_new,
    bench_cache_key_consistency,
    bench_cache_key_comparison,
    bench_cache_key_hash
);
criterion_main!(benches);
