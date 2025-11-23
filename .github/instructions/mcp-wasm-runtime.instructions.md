---
applyTo: "crates/mcp-wasm-runtime/**/*.rs"
---
# Copilot Instructions: mcp-wasm-runtime

This crate provides the **WASM execution runtime** with security sandbox. It is **SECURITY-CRITICAL** and requires special attention to safety, resource limits, and isolation.

## Security - CRITICAL

This crate implements the security boundary between untrusted WASM code and the host system. **Every change must maintain these security guarantees:**

### Security Principles

1. **Memory isolation** - WASM cannot access host memory
2. **CPU limits** - Prevent infinite loops and DoS
3. **Filesystem isolation** - WASI preopened directories only
4. **No network access** - WASM can only call MCP tools via bridge
5. **Input validation** - All data crossing WASM boundary is validated
6. **Resource cleanup** - No resource leaks even on error

### Security Configuration

```rust
use mcp_wasm_runtime::security::SecurityConfig;

// ✅ GOOD: Explicit security boundaries
let config = SecurityConfig {
    max_memory: 64 * 1024 * 1024,  // 64 MB
    max_fuel: 1_000_000,            // CPU limit
    timeout: Duration::from_secs(30),
    allowed_paths: vec![PathBuf::from("/tmp/wasm-sandbox")],
};

// ❌ BAD: Unlimited resources
let config = SecurityConfig {
    max_memory: usize::MAX,  // NEVER!
    max_fuel: u64::MAX,      // NEVER!
    ..
};
```

### Host Function Safety

All host functions exposed to WASM must be carefully reviewed:

```rust
// ✅ SAFE: Validated input, bounded execution
fn host_call_tool(
    caller: Caller<'_, RuntimeState>,
    tool_name_ptr: i32,
    tool_name_len: i32,
    args_ptr: i32,
    args_len: i32,
) -> Result<i32, Error> {
    // 1. Validate pointers and lengths
    if tool_name_len < 0 || tool_name_len > MAX_TOOL_NAME_LEN {
        return Err(Error::InvalidInput("tool name too long".into()));
    }

    // 2. Read from WASM memory safely
    let memory = caller.get_export("memory")
        .ok_or(Error::WasmMemoryNotFound)?
        .into_memory()
        .ok_or(Error::InvalidMemory)?;

    let tool_name = read_string_from_memory(
        &memory,
        &caller,
        tool_name_ptr as usize,
        tool_name_len as usize,
    )?;

    // 3. Validate extracted data
    if !is_valid_tool_name(&tool_name) {
        return Err(Error::InvalidToolName(tool_name));
    }

    // 4. Execute with resource limits
    let result = tokio::time::timeout(
        Duration::from_secs(30),
        bridge.call_tool(&tool_name, args)
    ).await??;

    Ok(write_to_wasm_memory(&memory, &caller, &result)?)
}
```

## Error Handling

**UNUSUAL**: This crate uses **both** `thiserror` and `anyhow`:

- **`thiserror`** for public library errors that callers need to handle
- **`anyhow`** internally for complex error chains during WASM execution

```rust
use thiserror::Error;

// ✅ Public API uses thiserror
#[derive(Error, Debug)]
pub enum RuntimeError {
    #[error("WASM compilation failed: {0}")]
    CompilationFailed(String),

    #[error("execution timeout after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("security violation: {0}")]
    SecurityViolation(String),

    #[error("resource limit exceeded: {resource}")]
    ResourceLimitExceeded { resource: String },
}

pub type Result<T> = std::result::Result<T, RuntimeError>;

// Internal implementation can use anyhow for complex chains
use anyhow::Context;

impl Runtime {
    async fn execute_internal(&self, wasm: &[u8]) -> anyhow::Result<Value> {
        let module = self.compile(wasm)
            .context("failed to compile WASM module")?;

        let instance = self.instantiate(module)
            .context("failed to instantiate WASM")?;

        // ... complex execution with multiple potential failures

        Ok(result)
    }

    // Public API converts to thiserror
    pub async fn execute(&self, wasm: &[u8]) -> Result<Value> {
        self.execute_internal(wasm)
            .await
            .map_err(|e| RuntimeError::CompilationFailed(e.to_string()))
    }
}
```

## Wasmtime Integration

### Engine Configuration

```rust
use wasmtime::*;

// ✅ GOOD: Secure engine configuration
fn create_engine() -> Result<Engine> {
    let mut config = Config::new();

    // Enable fuel metering for CPU limits
    config.consume_fuel(true);

    // Async support for timeouts
    config.async_support(true);

    // Disable features that could leak info
    config.cranelift_opt_level(OptLevel::Speed);
    config.debug_info(false);

    Engine::new(&config)
}
```

### Module Caching

Use Blake3 for cache keys:

```rust
use blake3::Hasher;
use lru::LruCache;

pub struct ModuleCache {
    cache: LruCache<blake3::Hash, Module>,
}

impl ModuleCache {
    pub fn get_or_compile(&mut self, wasm: &[u8], engine: &Engine) -> Result<Module> {
        let hash = blake3::hash(wasm);

        if let Some(module) = self.cache.get(&hash) {
            return Ok(module.clone());
        }

        let module = Module::new(engine, wasm)?;
        self.cache.put(hash, module.clone());
        Ok(module)
    }
}
```

### Resource Limits

**Always** enforce limits:

```rust
use wasmtime::Store;

fn create_store(engine: &Engine, config: &SecurityConfig) -> Result<Store<RuntimeState>> {
    let mut store = Store::new(engine, RuntimeState::new());

    // Set fuel limit (CPU time)
    store.set_fuel(config.max_fuel)?;

    // Set memory limit
    store.limiter(|state| &mut state.limiter);

    Ok(store)
}

struct RuntimeLimiter {
    max_memory: usize,
}

impl ResourceLimiter for RuntimeLimiter {
    fn memory_growing(&mut self, current: usize, desired: usize, _maximum: Option<usize>) -> Result<bool> {
        Ok(desired <= self.max_memory)
    }

    fn table_growing(&mut self, _current: u32, _desired: u32, _maximum: Option<u32>) -> Result<bool> {
        Ok(false)  // Deny table growth
    }
}
```

## WASI Integration

Only allow minimal WASI capabilities:

```rust
use wasmtime_wasi::{WasiCtx, WasiCtxBuilder};

fn create_wasi_ctx(config: &SecurityConfig) -> Result<WasiCtx> {
    let mut builder = WasiCtxBuilder::new();

    // ✅ Allow specific directories only
    for path in &config.allowed_paths {
        builder.preopened_dir(
            Dir::open_ambient_dir(path, ambient_authority())?,
            path.display().to_string(),
        )?;
    }

    // ✅ Inherit stdio for debugging (optional)
    builder.inherit_stdio();

    // ❌ NEVER allow network
    // ❌ NEVER allow arbitrary filesystem access

    Ok(builder.build())
}
```

## Async Execution

Use Tokio with timeouts:

```rust
use tokio::time::timeout;

impl Runtime {
    pub async fn execute(
        &self,
        wasm: &[u8],
        function: &str,
        args: &[Val],
    ) -> Result<Vec<Val>> {
        // Compile or get from cache
        let module = self.module_cache.get_or_compile(wasm, &self.engine)?;

        // Create isolated store
        let mut store = create_store(&self.engine, &self.config)?;

        // Instantiate with timeout
        let instance = timeout(
            self.config.timeout,
            Instance::new_async(&mut store, &module, &[])
        ).await
        .map_err(|_| RuntimeError::Timeout { timeout: self.config.timeout })??;

        // Get function
        let func = instance
            .get_func(&mut store, function)
            .ok_or_else(|| RuntimeError::FunctionNotFound(function.to_string()))?;

        // Execute with timeout and fuel limit
        let mut results = vec![Val::I32(0); func.ty(&store).results().len()];
        timeout(
            self.config.timeout,
            func.call_async(&mut store, args, &mut results)
        ).await
        .map_err(|_| RuntimeError::Timeout { timeout: self.config.timeout })??;

        // Check remaining fuel
        let fuel_consumed = self.config.max_fuel - store.get_fuel()?;
        tracing::debug!("Fuel consumed: {}", fuel_consumed);

        Ok(results)
    }
}
```

## Testing Security

**CRITICAL**: Every security boundary must have tests:

```rust
#[cfg(test)]
mod security_tests {
    use super::*;

    #[tokio::test]
    async fn test_memory_limit_enforced() {
        let config = SecurityConfig {
            max_memory: 1024,  // 1KB limit
            ..Default::default()
        };

        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        // WASM that tries to allocate 1MB
        let wasm = wat::parse_str(r#"
            (module
                (memory (export "memory") 16)  // 16 pages = 1MB
            )
        "#).unwrap();

        let result = runtime.execute(&wasm, "start", &[]).await;
        assert!(matches!(result, Err(RuntimeError::ResourceLimitExceeded { .. })));
    }

    #[tokio::test]
    async fn test_cpu_limit_enforced() {
        let config = SecurityConfig {
            max_fuel: 1000,
            ..Default::default()
        };

        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        // WASM with infinite loop
        let wasm = wat::parse_str(r#"
            (module
                (func $infinite_loop
                    (loop $loop
                        br $loop
                    )
                )
                (export "start" (func $infinite_loop))
            )
        "#).unwrap();

        let result = runtime.execute(&wasm, "start", &[]).await;
        assert!(matches!(result, Err(RuntimeError::ResourceLimitExceeded { .. })));
    }

    #[tokio::test]
    async fn test_timeout_enforced() {
        let config = SecurityConfig {
            timeout: Duration::from_millis(100),
            ..Default::default()
        };

        let runtime = Runtime::new(Arc::new(bridge), config).unwrap();

        // WASM that sleeps for 1 second
        let result = runtime.execute(&wasm, "sleep_function", &[]).await;
        assert!(matches!(result, Err(RuntimeError::Timeout { .. })));
    }

    #[tokio::test]
    async fn test_filesystem_isolation() {
        // Test that WASM cannot access paths outside allowed_paths
        // ...
    }
}
```

## Performance Considerations

Security must not be sacrificed for performance, but we can optimize:

1. **Cache compiled modules** - Use Blake3 + LRU
2. **Reuse stores** when safe - But never across different WASM modules
3. **Pre-allocate memory** - Within security limits
4. **Profile fuel consumption** - Adjust limits based on real workloads

## Summary

- This is a **SECURITY-CRITICAL** crate
- **All resource limits must be enforced**
- **Validate all inputs from WASM**
- **Use timeouts for all async operations**
- **Test every security boundary**
- **Document security assumptions**
- Uses **both thiserror (public API) and anyhow (internal)**
