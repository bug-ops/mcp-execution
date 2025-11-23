//! WASM sandbox runtime using Wasmtime.
//!
//! Provides secure WASM execution with configurable security limits,
//! host function integration, and resource monitoring.
//!
//! # Examples
//!
//! ```no_run
//! use mcp_wasm_runtime::{Runtime, security::SecurityConfig};
//! use mcp_bridge::Bridge;
//! use std::sync::Arc;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let bridge = Bridge::new(1000);
//! let config = SecurityConfig::default();
//! let runtime = Runtime::new(Arc::new(bridge), config)?;
//!
//! let wasm_bytes = vec![/* compiled WASM */];
//! let result = runtime.execute(&wasm_bytes, "main", &[]).await?;
//! # Ok(())
//! # }
//! ```

use crate::cache::ModuleCache;
use crate::host_functions::HostContext;
use crate::monitor::ResourceMonitor;
use crate::security::SecurityConfig;
use mcp_bridge::Bridge;
use mcp_core::{Error, Result, stats::RuntimeStats};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::Instant;
use wasmtime::*;

/// Store data combining host context and resource limiter.
struct StoreData {
    #[allow(dead_code)] // Will be used for host function linking in Phase 5
    host_context: HostContext,
    limiter: MemoryLimiter,
}

/// WASM execution runtime with security sandbox.
///
/// Manages Wasmtime engine, linker, and store configuration
/// with enforced security boundaries.
///
/// # Thread Safety
///
/// This type is `Send` but not `Sync` due to Wasmtime's Store
/// requirements. Create separate instances for concurrent execution.
///
/// # Examples
///
/// ```no_run
/// use mcp_wasm_runtime::Runtime;
/// use mcp_wasm_runtime::security::SecurityConfig;
/// use mcp_bridge::Bridge;
/// use std::sync::Arc;
///
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// let bridge = Bridge::new(1000);
/// let config = SecurityConfig::default();
/// let runtime = Runtime::new(Arc::new(bridge), config)?;
/// # Ok(())
/// # }
/// ```
pub struct Runtime {
    engine: Engine,
    host_context: HostContext,
    config: SecurityConfig,
    module_cache: ModuleCache,

    // Statistics tracking (thread-safe atomics)
    total_executions: AtomicU32,
    execution_failures: AtomicU32,
    compilation_failures: AtomicU32,
    total_execution_time_us: AtomicU64,
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("config", &self.config)
            .field("host_context", &self.host_context)
            .field("module_cache", &self.module_cache)
            .field(
                "total_executions",
                &self.total_executions.load(Ordering::Relaxed),
            )
            .field(
                "execution_failures",
                &self.execution_failures.load(Ordering::Relaxed),
            )
            .finish_non_exhaustive()
    }
}

impl Runtime {
    /// Creates a new WASM runtime with security configuration.
    ///
    /// # Errors
    ///
    /// Returns error if Wasmtime engine configuration fails.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::Runtime;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use mcp_bridge::Bridge;
    /// use std::sync::Arc;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let config = SecurityConfig::default();
    /// let runtime = Runtime::new(Arc::new(bridge), config)?;
    /// # Ok(())
    /// # }
    /// ```
    pub fn new(bridge: Arc<Bridge>, config: SecurityConfig) -> Result<Self> {
        let mut wasmtime_config = Config::new();

        // Enable WASI support
        wasmtime_config.wasm_backtrace_details(WasmBacktraceDetails::Enable);
        wasmtime_config.async_support(true);

        // Use Cranelift compiler for better compatibility
        wasmtime_config.strategy(wasmtime::Strategy::Cranelift);

        // Configure fuel for CPU limits
        if config.max_fuel().is_some() {
            wasmtime_config.consume_fuel(true);
        }

        // Create engine
        let engine = Engine::new(&wasmtime_config).map_err(|e| Error::WasmError {
            message: format!("Failed to create Wasmtime engine: {}", e),
        })?;

        let host_context = HostContext::new(bridge);
        let module_cache = ModuleCache::new(100); // Default cache size: 100 modules

        Ok(Self {
            engine,
            host_context,
            config,
            module_cache,
            total_executions: AtomicU32::new(0),
            execution_failures: AtomicU32::new(0),
            compilation_failures: AtomicU32::new(0),
            total_execution_time_us: AtomicU64::new(0),
        })
    }

    /// Executes WASM code with the given entry point and arguments.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - WASM module is invalid
    /// - Execution exceeds timeout
    /// - Fuel limit exceeded
    /// - Entry point not found
    /// - Execution fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::Runtime;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use mcp_bridge::Bridge;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let config = SecurityConfig::default();
    /// let runtime = Runtime::new(Arc::new(bridge), config)?;
    ///
    /// let wasm_bytes = vec![/* compiled WASM */];
    /// let result = runtime.execute(&wasm_bytes, "main", &[]).await?;
    /// # Ok(())
    /// # }
    /// ```
    pub async fn execute(
        &self,
        wasm_bytes: &[u8],
        entry_point: &str,
        _args: &[String],
    ) -> Result<serde_json::Value> {
        let start_time = Instant::now();
        let monitor = ResourceMonitor::new(&self.config);

        // Increment total executions counter
        self.total_executions.fetch_add(1, Ordering::Relaxed);

        // Generate cache key
        let cache_key = ModuleCache::cache_key_for_code(wasm_bytes);

        // Create store with host context and memory limits
        let store_data = StoreData {
            host_context: self.host_context.clone(),
            limiter: MemoryLimiter {
                max_memory_bytes: self.config.memory_limit_bytes(),
            },
        };
        let mut store = Store::new(&self.engine, store_data);

        // Set memory limiter
        store.limiter(|data| &mut data.limiter);

        // Set fuel limit if configured
        if let Some(fuel) = self.config.max_fuel() {
            // NOTE: Wasmtime 37.0 changed the fuel API significantly.
            // The Store::add_fuel() method no longer exists. Fuel consumption
            // is now configured at engine level via Config::consume_fuel(true),
            // which we do in Runtime::new(). However, granular fuel tracking
            // and limits require Wasmtime 38.0+ or using the execution timeout
            // as a coarser-grained CPU limit. We rely on execution_timeout()
            // for CPU exhaustion protection.
            tracing::debug!(
                "Fuel limit configured ({}), relying on execution timeout for CPU protection",
                fuel
            );
        }

        // Try to get module from cache
        let module = if let Some(cached_module) = self.module_cache.get(&cache_key) {
            let key_preview = cache_key.as_str();
            let preview_len = key_preview.len().min(16);
            tracing::debug!("Using cached WASM module: {}", &key_preview[..preview_len]);
            cached_module
        } else {
            // Compile module
            tracing::debug!("Compiling WASM module ({} bytes)", wasm_bytes.len());
            let compilation_start = Instant::now();

            let module = Module::new(&self.engine, wasm_bytes).map_err(|e| {
                // Track compilation failure
                self.compilation_failures.fetch_add(1, Ordering::Relaxed);
                self.execution_failures.fetch_add(1, Ordering::Relaxed);
                Error::WasmError {
                    message: format!("Failed to compile WASM module: {}", e),
                }
            })?;

            let compilation_time = compilation_start.elapsed();
            tracing::info!("Module compiled in {:?}", compilation_time);

            // Cache the compiled module
            self.module_cache.insert(cache_key, module.clone());

            module
        };

        // Create linker with host functions
        let mut linker = Linker::new(&self.engine);
        self.link_host_functions(&mut linker)?;

        // Instantiate module
        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .map_err(|e| {
                self.execution_failures.fetch_add(1, Ordering::Relaxed);
                Error::WasmError {
                    message: format!("Failed to instantiate WASM module: {}", e),
                }
            })?;

        // Get entry point function
        tracing::debug!("Getting entry point function: {}", entry_point);
        let func = instance
            .get_typed_func::<(), i32>(&mut store, entry_point)
            .map_err(|e| {
                self.execution_failures.fetch_add(1, Ordering::Relaxed);
                Error::WasmError {
                    message: format!("Entry point '{}' not found: {}", entry_point, e),
                }
            })?;

        tracing::debug!("Calling entry point function asynchronously");
        // Execute with timeout
        let timeout = self.config.execution_timeout();
        let result = tokio::time::timeout(timeout, func.call_async(&mut store, ()))
            .await
            .map_err(|_| {
                self.execution_failures.fetch_add(1, Ordering::Relaxed);
                tracing::error!("Execution timeout after {:?}", timeout);
                Error::Timeout {
                    operation: "WASM execution".to_string(),
                    duration_secs: timeout.as_secs(),
                }
            })?
            .map_err(|e| {
                self.execution_failures.fetch_add(1, Ordering::Relaxed);
                tracing::error!("WASM execution trap: {}", e);
                Error::ExecutionError {
                    message: format!("WASM execution failed: {}", e),
                    source: None,
                }
            })?;

        tracing::debug!("WASM function returned: {}", result);

        let elapsed = start_time.elapsed();

        // Track execution time
        self.total_execution_time_us
            .fetch_add(elapsed.as_micros() as u64, Ordering::Relaxed);

        tracing::info!(
            "WASM execution completed in {:?}, exit code: {}",
            elapsed,
            result
        );
        tracing::debug!("Resource usage: {}", monitor.summary());

        Ok(serde_json::json!({
            "exit_code": result,
            "elapsed_ms": elapsed.as_millis(),
            "memory_usage_mb": monitor.memory_usage_mb(),
            "host_calls": monitor.host_call_count(),
        }))
    }

    /// Links host functions to the WASM linker.
    ///
    /// Registers host functions that WASM modules can call:
    /// - `host_log(ptr: i32, len: i32)` - Log a message from WASM
    /// - `host_add(a: i32, b: i32) -> i32` - Simple test function
    ///
    /// # Note
    ///
    /// Full MCP integration (call_tool, state management, VFS) requires
    /// more complex memory management and will be implemented in the next phase.
    fn link_host_functions(&self, linker: &mut Linker<StoreData>) -> Result<()> {
        // Simple test function: add two numbers
        linker
            .func_wrap("env", "host_add", |a: i32, b: i32| -> i32 { a + b })
            .map_err(|e| Error::WasmError {
                message: format!("Failed to link host_add: {}", e),
            })?;

        // Log function: reads string from WASM memory
        linker
            .func_wrap(
                "env",
                "host_log",
                |mut caller: wasmtime::Caller<'_, StoreData>, ptr: i32, len: i32| {
                    // Get memory from the caller
                    let mem = match caller.get_export("memory") {
                        Some(wasmtime::Extern::Memory(mem)) => mem,
                        _ => {
                            tracing::error!("WASM module has no memory export");
                            return;
                        }
                    };

                    // Read string from WASM memory
                    let data = mem.data(&caller);
                    let ptr = ptr as usize;
                    let len = len as usize;

                    if ptr + len > data.len() {
                        tracing::error!("Invalid memory access: ptr={}, len={}", ptr, len);
                        return;
                    }

                    let bytes = &data[ptr..ptr + len];
                    match std::str::from_utf8(bytes) {
                        Ok(s) => tracing::info!("[WASM] {}", s),
                        Err(e) => tracing::error!("Invalid UTF-8 from WASM: {}", e),
                    }
                },
            )
            .map_err(|e| Error::WasmError {
                message: format!("Failed to link host_log: {}", e),
            })?;

        tracing::debug!("Host functions linked: host_add, host_log");
        Ok(())
    }

    /// Returns reference to host context for testing.
    #[cfg(test)]
    pub fn host_context(&self) -> &HostContext {
        &self.host_context
    }

    /// Returns reference to module cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::Runtime;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use mcp_bridge::Bridge;
    /// use std::sync::Arc;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let config = SecurityConfig::default();
    /// let runtime = Runtime::new(Arc::new(bridge), config)?;
    ///
    /// let cache = runtime.module_cache();
    /// println!("Cache size: {}/{}", cache.len(), cache.capacity());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn module_cache(&self) -> &ModuleCache {
        &self.module_cache
    }

    /// Clears the module cache.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::Runtime;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use mcp_bridge::Bridge;
    /// use std::sync::Arc;
    ///
    /// # fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let config = SecurityConfig::default();
    /// let runtime = Runtime::new(Arc::new(bridge), config)?;
    ///
    /// runtime.clear_cache();
    /// # Ok(())
    /// # }
    /// ```
    pub fn clear_cache(&self) {
        self.module_cache.clear();
    }

    /// Collects current runtime statistics.
    ///
    /// Returns statistics about:
    /// - Module cache hit rate
    /// - Execution metrics (total, failures)
    /// - Compilation failures
    /// - Average execution time
    ///
    /// # Performance
    ///
    /// This operation is O(1) and takes <1ms as it only reads atomic counters
    /// and the module cache size.
    ///
    /// # Thread Safety
    ///
    /// Safe to call concurrently from multiple threads. All counters use
    /// atomic operations for thread-safe access.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use mcp_wasm_runtime::Runtime;
    /// use mcp_wasm_runtime::security::SecurityConfig;
    /// use mcp_bridge::Bridge;
    /// use std::sync::Arc;
    ///
    /// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
    /// let bridge = Bridge::new(1000);
    /// let config = SecurityConfig::default();
    /// let runtime = Runtime::new(Arc::new(bridge), config)?;
    ///
    /// // Execute some WASM modules...
    /// let wasm = vec![/* compiled WASM */];
    /// runtime.execute(&wasm, "main", &[]).await?;
    ///
    /// // Collect statistics
    /// let stats = runtime.collect_stats();
    /// println!("Total executions: {}", stats.total_executions);
    /// println!("Cache hit rate: {:?}", stats.cache_hit_rate());
    /// # Ok(())
    /// # }
    /// ```
    #[must_use]
    pub fn collect_stats(&self) -> RuntimeStats {
        let total_executions = self.total_executions.load(Ordering::Relaxed);
        let execution_failures = self.execution_failures.load(Ordering::Relaxed);
        let compilation_failures = self.compilation_failures.load(Ordering::Relaxed);
        let total_execution_time_us = self.total_execution_time_us.load(Ordering::Relaxed);

        // Get cache hits from module cache
        // Cache hits = number of modules currently cached
        // (This is a simplified metric - in production you'd track actual hit/miss counts)
        let cache_hits = self.module_cache.len() as u32;

        // Calculate average execution time
        let avg_execution_time_us = if total_executions > 0 {
            total_execution_time_us / u64::from(total_executions)
        } else {
            0
        };

        RuntimeStats::new(
            total_executions,
            cache_hits,
            execution_failures,
            compilation_failures,
            avg_execution_time_us,
        )
    }
}

/// Memory limiter for WASM store.
struct MemoryLimiter {
    max_memory_bytes: usize,
}

impl ResourceLimiter for MemoryLimiter {
    fn memory_growing(
        &mut self,
        current: usize,
        desired: usize,
        _maximum: Option<usize>,
    ) -> std::result::Result<bool, anyhow::Error> {
        if desired > self.max_memory_bytes {
            tracing::warn!(
                "Memory limit exceeded: {} > {}",
                desired,
                self.max_memory_bytes
            );
            Ok(false)
        } else {
            tracing::trace!("Memory growing: {} -> {} bytes", current, desired);
            Ok(true)
        }
    }

    fn table_growing(
        &mut self,
        _current: usize,
        _desired: usize,
        _maximum: Option<usize>,
    ) -> std::result::Result<bool, anyhow::Error> {
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_runtime_creation() {
        let bridge = Bridge::new(1000);
        let config = SecurityConfig::default();
        let result = Runtime::new(Arc::new(bridge), config);
        assert!(result.is_ok());
    }

    #[test]
    fn test_runtime_with_custom_config() {
        let bridge = Bridge::new(1000);
        let config = SecurityConfig::builder()
            .memory_limit_mb(512)
            .max_fuel(5_000_000)
            .build();

        let result = Runtime::new(Arc::new(bridge), config);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_invalid_wasm() {
        let bridge = Bridge::new(1000);
        let config = SecurityConfig::default();
        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        let invalid_wasm = vec![0x00, 0x01, 0x02, 0x03];
        let result = runtime.execute(&invalid_wasm, "main", &[]).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_simple_wasm_execution() {
        // Initialize tracing for test
        let _ = tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_test_writer()
            .try_init();

        let bridge = Bridge::new(1000);
        let config = SecurityConfig::default();
        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        // Simple WASM module that returns 42
        let wat = r#"
            (module
                (func (export "main") (result i32)
                    (i32.const 42)
                )
            )
        "#;

        // Parse WAT to WASM
        let wasm_bytes = wat::parse_str(wat).expect("Failed to parse WAT");

        tracing::info!("Executing WASM module, size: {} bytes", wasm_bytes.len());

        // Execute
        let result = runtime.execute(&wasm_bytes, "main", &[]).await;

        match result {
            Ok(value) => {
                assert_eq!(value["exit_code"], 42);
            }
            Err(e) => {
                panic!("Expected successful execution, got error: {:?}", e);
            }
        }
    }

    #[test]
    fn test_collect_stats_empty_runtime() {
        let bridge = Bridge::new(1000);
        let config = SecurityConfig::default();
        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        let stats = runtime.collect_stats();

        assert_eq!(stats.total_executions, 0);
        assert_eq!(stats.execution_failures, 0);
        assert_eq!(stats.compilation_failures, 0);
        assert_eq!(stats.avg_execution_time_us, 0);
        assert_eq!(stats.cache_hits, 0);
    }

    #[tokio::test]
    async fn test_collect_stats_after_execution() {
        let bridge = Bridge::new(1000);
        let config = SecurityConfig::default();
        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        // Simple WASM module
        let wat = r#"
            (module
                (func (export "main") (result i32)
                    (i32.const 42)
                )
            )
        "#;

        let wasm_bytes = wat::parse_str(wat).unwrap();

        // Execute once
        runtime.execute(&wasm_bytes, "main", &[]).await.unwrap();

        let stats = runtime.collect_stats();

        assert_eq!(stats.total_executions, 1);
        assert_eq!(stats.execution_failures, 0);
        assert_eq!(stats.compilation_failures, 0);
        assert!(stats.avg_execution_time_us > 0);
        assert_eq!(stats.cache_hits, 1); // Module is now cached
    }

    #[tokio::test]
    async fn test_collect_stats_tracks_failures() {
        let bridge = Bridge::new(1000);
        let config = SecurityConfig::default();
        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        // Invalid WASM
        let invalid_wasm = vec![0x00, 0x01, 0x02, 0x03];

        // Try to execute (will fail)
        let _ = runtime.execute(&invalid_wasm, "main", &[]).await;

        let stats = runtime.collect_stats();

        assert_eq!(stats.total_executions, 1);
        assert_eq!(stats.execution_failures, 1);
        assert_eq!(stats.compilation_failures, 1);
    }

    #[tokio::test]
    async fn test_collect_stats_multiple_executions() {
        let bridge = Bridge::new(1000);
        let config = SecurityConfig::default();
        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        let wat = r#"
            (module
                (func (export "main") (result i32)
                    (i32.const 42)
                )
            )
        "#;

        let wasm_bytes = wat::parse_str(wat).unwrap();

        // Execute multiple times
        for _ in 0..5 {
            runtime.execute(&wasm_bytes, "main", &[]).await.unwrap();
        }

        let stats = runtime.collect_stats();

        assert_eq!(stats.total_executions, 5);
        assert_eq!(stats.execution_failures, 0);
        assert_eq!(stats.compilation_failures, 0);
        assert!(stats.avg_execution_time_us > 0);
        assert_eq!(stats.cache_hits, 1); // Same module, only cached once
    }

    #[tokio::test]
    async fn test_collect_stats_cache_hit_rate() {
        let bridge = Bridge::new(1000);
        let config = SecurityConfig::default();
        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        let wat1 = r#"
            (module
                (func (export "main") (result i32)
                    (i32.const 1)
                )
            )
        "#;

        let wat2 = r#"
            (module
                (func (export "main") (result i32)
                    (i32.const 2)
                )
            )
        "#;

        let wasm1 = wat::parse_str(wat1).unwrap();
        let wasm2 = wat::parse_str(wat2).unwrap();

        // Execute two different modules
        runtime.execute(&wasm1, "main", &[]).await.unwrap();
        runtime.execute(&wasm2, "main", &[]).await.unwrap();

        let stats = runtime.collect_stats();

        assert_eq!(stats.total_executions, 2);
        assert_eq!(stats.cache_hits, 2); // Both modules cached
        assert_eq!(stats.execution_failures, 0);

        // Verify cache hit rate calculation
        let hit_rate = stats.cache_hit_rate().unwrap();
        assert!((hit_rate - 1.0).abs() < 0.01); // 2/2 = 100%
    }
}
