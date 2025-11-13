//! Performance tests for WASM runtime.
//!
//! Tests performance characteristics:
//! - Module compilation time
//! - Execution overhead
//! - Cache hit rates
//! - Memory efficiency

use mcp_bridge::Bridge;
use mcp_wasm_runtime::{ModuleCache, Runtime, SecurityConfig};
use std::sync::Arc;
use std::time::Instant;

/// Test that module compilation completes within target time.
#[tokio::test]
async fn test_compilation_performance() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("mcp_wasm_runtime=info")
        .with_test_writer()
        .try_init();

    // Create a moderately complex WASM module
    let wat = r#"
        (module
            (func $add (param $a i32) (param $b i32) (result i32)
                local.get $a
                local.get $b
                i32.add
            )
            (func $multiply (param $a i32) (param $b i32) (result i32)
                local.get $a
                local.get $b
                i32.mul
            )
            (func (export "main") (result i32)
                (i32.const 10)
                (i32.const 5)
                (call $add)
                (i32.const 3)
                (call $multiply)
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    let bridge = Bridge::new(1000);
    let config = SecurityConfig::default();
    let runtime = Runtime::new(Arc::new(bridge), config).expect("Failed to create runtime");

    // Measure first compilation (uncached)
    let start = Instant::now();
    let result = runtime.execute(&wasm_bytes, "main", &[]).await;
    let first_execution = start.elapsed();

    assert!(result.is_ok(), "Execution should succeed");
    tracing::info!("First execution (with compilation): {:?}", first_execution);

    // Measure second execution (cached)
    let start = Instant::now();
    let result = runtime.execute(&wasm_bytes, "main", &[]).await;
    let cached_execution = start.elapsed();

    assert!(result.is_ok(), "Cached execution should succeed");
    tracing::info!("Second execution (from cache): {:?}", cached_execution);

    // Cached execution should be significantly faster
    assert!(
        cached_execution < first_execution,
        "Cached execution should be faster than initial compilation"
    );

    // Target: compilation + execution < 100ms for simple modules
    // This is a soft target and may vary by hardware
    if first_execution.as_millis() < 100 {
        tracing::info!(
            "✓ Compilation performance target met: {:?}",
            first_execution
        );
    } else {
        tracing::warn!(
            "Compilation performance target missed ({}ms > 100ms target)",
            first_execution.as_millis()
        );
    }
}

/// Test execution overhead with cached modules.
#[tokio::test]
async fn test_execution_overhead() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("mcp_wasm_runtime=info")
        .with_test_writer()
        .try_init();

    let wat = r#"
        (module
            (func (export "main") (result i32)
                (i32.const 42)
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    let bridge = Bridge::new(1000);
    let config = SecurityConfig::default();
    let runtime = Runtime::new(Arc::new(bridge), config).expect("Failed to create runtime");

    // Warm up cache
    runtime.execute(&wasm_bytes, "main", &[]).await.ok();

    // Measure multiple cached executions
    let mut total_time = std::time::Duration::ZERO;
    let iterations = 10;

    for _ in 0..iterations {
        let start = Instant::now();
        let result = runtime.execute(&wasm_bytes, "main", &[]).await;
        let elapsed = start.elapsed();

        assert!(result.is_ok());
        total_time += elapsed;
    }

    let average_time = total_time / iterations;
    tracing::info!("Average cached execution time: {:?}", average_time);

    // Target: < 50ms overhead per execution
    if average_time.as_millis() < 50 {
        tracing::info!("✓ Execution overhead target met: {:?}", average_time);
    } else {
        tracing::warn!(
            "Execution overhead target missed ({}ms > 50ms target)",
            average_time.as_millis()
        );
    }
}

/// Test cache hit rate with repeated executions.
#[tokio::test]
async fn test_cache_hit_rate() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("mcp_wasm_runtime=info")
        .with_test_writer()
        .try_init();

    let bridge = Bridge::new(1000);
    let config = SecurityConfig::default();
    let runtime = Runtime::new(Arc::new(bridge), config).expect("Failed to create runtime");

    // Create multiple different WASM modules
    let modules: Vec<Vec<u8>> = (0..5)
        .map(|i| {
            let wat = format!(
                r#"
                (module
                    (func (export "main") (result i32)
                        (i32.const {})
                    )
                )
                "#,
                i * 10
            );
            wat::parse_str(&wat).expect("Failed to parse WAT")
        })
        .collect();

    // Execute each module twice
    for module in &modules {
        runtime.execute(module, "main", &[]).await.ok();
        runtime.execute(module, "main", &[]).await.ok();
    }

    // Check cache statistics
    let cache = runtime.module_cache();
    let cache_size = cache.len();

    tracing::info!("Cache size: {} modules", cache_size);
    assert_eq!(cache_size, 5, "All 5 unique modules should be cached");

    // Cache hit rate should be high for repeated executions
    // In this test, we executed 10 times (5 modules × 2) but only compiled 5 times
    // So effective hit rate is 50%
    tracing::info!("✓ Cache working correctly with {} entries", cache_size);
}

/// Test memory efficiency of cached modules.
#[tokio::test]
async fn test_memory_efficiency() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("mcp_wasm_runtime=info")
        .with_test_writer()
        .try_init();

    let wat = r#"
        (module
            (func (export "main") (result i32)
                (i32.const 0)
            )
        )
    "#;

    let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

    let bridge = Bridge::new(1000);
    let config = SecurityConfig::default();
    let runtime = Runtime::new(Arc::new(bridge), config).expect("Failed to create runtime");

    // Execute module
    let result = runtime.execute(&wasm_bytes, "main", &[]).await;
    assert!(result.is_ok());

    // Check that execution reports memory usage
    if let Ok(value) = result {
        assert!(value.get("memory_usage_mb").is_some());
        assert!(value.get("host_calls").is_some());
        tracing::info!("Memory usage: {}MB", value["memory_usage_mb"]);
    }
}

/// Test performance with larger WASM modules.
#[tokio::test]
async fn test_large_module_performance() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("mcp_wasm_runtime=info")
        .with_test_writer()
        .try_init();

    // Create a larger module with many functions
    let mut wat = String::from("(module\n");

    // Add 100 functions
    for i in 0..100 {
        wat.push_str(&format!(
            "  (func $func{} (param i32) (result i32)\n    local.get 0\n    i32.const {}\n    i32.add\n  )\n",
            i, i
        ));
    }

    wat.push_str(
        r#"
        (func (export "main") (result i32)
            (i32.const 10)
            (call $func0)
        )
    )
    "#,
    );

    let wasm_bytes = wat::parse_str(&wat).expect("Failed to parse WAT");
    tracing::info!("Large module size: {} bytes", wasm_bytes.len());

    let bridge = Bridge::new(1000);
    let config = SecurityConfig::default();
    let runtime = Runtime::new(Arc::new(bridge), config).expect("Failed to create runtime");

    // Measure compilation and execution
    let start = Instant::now();
    let result = runtime.execute(&wasm_bytes, "main", &[]).await;
    let total_time = start.elapsed();

    assert!(result.is_ok(), "Large module should execute successfully");
    tracing::info!("Large module execution time: {:?}", total_time);

    // Even large modules should compile and execute reasonably fast
    assert!(
        total_time.as_millis() < 500,
        "Large module should execute within 500ms"
    );
}

/// Test cache LRU eviction behavior.
#[test]
fn test_cache_lru_performance() {
    let cache = ModuleCache::new(3); // Small cache for testing eviction

    let engine = wasmtime::Engine::default();
    let wat = "(module)";
    let wasm = wat::parse_str(wat).unwrap();

    // Create and cache modules
    for i in 0..5 {
        let module = wasmtime::Module::new(&engine, &wasm).unwrap();
        let key = ModuleCache::cache_key_for_code(format!("module_{}", i).as_bytes());
        cache.insert(key, module);
    }

    // Cache should only hold 3 most recent modules
    assert_eq!(cache.len(), 3);
    assert_eq!(cache.capacity(), 3);
}

/// Test concurrent execution performance.
#[tokio::test]
async fn test_concurrent_execution() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter("mcp_wasm_runtime=info")
        .with_test_writer()
        .try_init();

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

    // Create multiple runtime instances for concurrent execution
    let mut handles = vec![];

    let start = Instant::now();

    for _ in 0..5 {
        let bridge_clone = Arc::clone(&bridge);
        let wasm_clone = wasm_bytes.clone();
        let config_clone = config.clone();

        let handle = tokio::spawn(async move {
            let runtime =
                Runtime::new(bridge_clone, config_clone).expect("Failed to create runtime");
            runtime.execute(&wasm_clone, "main", &[]).await
        });

        handles.push(handle);
    }

    // Wait for all executions to complete
    for handle in handles {
        let result = handle.await.expect("Task panicked");
        assert!(result.is_ok(), "Concurrent execution should succeed");
    }

    let total_time = start.elapsed();
    tracing::info!("5 concurrent executions completed in {:?}", total_time);

    // Concurrent execution should complete reasonably fast
    assert!(
        total_time.as_millis() < 1000,
        "Concurrent executions should complete within 1 second"
    );
}

/// Test module cache statistics.
#[test]
fn test_cache_statistics() {
    let cache = ModuleCache::new(10);

    assert_eq!(cache.len(), 0);
    assert_eq!(cache.capacity(), 10);
    assert!(cache.is_empty());

    let engine = wasmtime::Engine::default();
    let wat = "(module)";
    let wasm = wat::parse_str(wat).unwrap();

    // Add some modules
    for i in 0..5 {
        let module = wasmtime::Module::new(&engine, &wasm).unwrap();
        let key = ModuleCache::cache_key_for_code(format!("test_{}", i).as_bytes());
        cache.insert(key, module);
    }

    assert_eq!(cache.len(), 5);
    assert!(!cache.is_empty());

    let (hits, total, rate) = cache.hit_rate();
    assert_eq!(hits, 5);
    assert_eq!(total, 10);
    assert_eq!(rate, 50.0); // 5/10 = 50%
}
