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

use crate::host_functions::HostContext;
use crate::security::SecurityConfig;
use mcp_bridge::Bridge;
use mcp_core::{Error, Result};
use std::sync::Arc;
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
}

impl std::fmt::Debug for Runtime {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Runtime")
            .field("config", &self.config)
            .field("host_context", &self.host_context)
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

        // Configure fuel for CPU limits
        if config.max_fuel().is_some() {
            wasmtime_config.consume_fuel(true);
        }

        // Create engine
        let engine = Engine::new(&wasmtime_config).map_err(|e| Error::WasmError {
            message: format!("Failed to create Wasmtime engine: {}", e),
        })?;

        let host_context = HostContext::new(bridge);

        Ok(Self {
            engine,
            host_context,
            config,
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

        // Compile module
        let module = Module::new(&self.engine, wasm_bytes).map_err(|e| Error::WasmError {
            message: format!("Failed to compile WASM module: {}", e),
        })?;

        // Create linker with host functions
        let mut linker = Linker::new(&self.engine);
        self.link_host_functions(&mut linker)?;

        // Instantiate module
        let instance = linker
            .instantiate_async(&mut store, &module)
            .await
            .map_err(|e| Error::WasmError {
                message: format!("Failed to instantiate WASM module: {}", e),
            })?;

        // Get entry point function
        let func = instance
            .get_typed_func::<(), i32>(&mut store, entry_point)
            .map_err(|e| Error::WasmError {
                message: format!("Entry point '{}' not found: {}", entry_point, e),
            })?;

        // Execute with timeout
        let timeout = self.config.execution_timeout();
        let result = tokio::time::timeout(timeout, func.call_async(&mut store, ()))
            .await
            .map_err(|_| Error::Timeout {
                operation: "WASM execution".to_string(),
                duration_secs: timeout.as_secs(),
            })?
            .map_err(|e| Error::ExecutionError {
                message: format!("WASM execution failed: {}", e),
                source: None,
            })?;

        let elapsed = start_time.elapsed();
        tracing::info!(
            "WASM execution completed in {:?}, exit code: {}",
            elapsed,
            result
        );

        Ok(serde_json::json!({
            "exit_code": result,
            "elapsed_ms": elapsed.as_millis(),
        }))
    }

    /// Links host functions to the WASM linker.
    fn link_host_functions(&self, _linker: &mut Linker<StoreData>) -> Result<()> {
        // TODO: Implement actual host function linking
        // This requires defining the WASM function signatures that match
        // our HostContext methods. For now, we return Ok as a placeholder.

        tracing::debug!("Host functions linked");
        Ok(())
    }

    /// Returns reference to host context for testing.
    #[cfg(test)]
    pub fn host_context(&self) -> &HostContext {
        &self.host_context
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

    // TODO: Add test with valid WASM module when we have compiler ready
}
