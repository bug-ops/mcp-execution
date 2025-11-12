# ADR-002: Wasmtime as WASM Runtime

## Status

Accepted

## Context

The project requires a secure, performant WASM runtime for executing untrusted code with strict memory/CPU limits. Two major options exist:

1. **Wasmtime** - Bytecode Alliance, used by Fastly, Cloudflare
2. **Wasmer** - Independent, focuses on ease of use

## Decision

We will use **Wasmtime 26.0** with `wasmtime-wasi` for WASI support.

## Rationale

**Wasmtime advantages:**

1. **Security focus**: Developed by Bytecode Alliance with rigorous security audits
2. **Pooling allocator**: Built-in support for memory limits and instance pooling
3. **Fuel metering**: CPU limiting via fuel consumption tracking
4. **Production proven**: Used by Fastly edge compute, Cloudflare Workers
5. **Strong WASI support**: Mature `wasmtime-wasi` crate with filesystem isolation
6. **Performance**: Cranelift compiler optimized for ahead-of-time compilation
7. **Documentation**: Excellent book and API documentation

**Compared to Wasmer:**

- Wasmtime has better pooling allocator for multi-tenant scenarios
- Wasmtime fuel metering is more granular
- Wasmer has simpler API but less fine-grained control
- Wasmtime is more actively maintained (Bytecode Alliance backing)

## Consequences

**Positive:**

- Strong security guarantees for sandbox
- Fine-grained control over memory and CPU
- Production-ready stability
- Active maintenance and security updates
- Module caching via `Module::serialize()`

**Negative:**

- API is more complex than Wasmer
- Requires careful configuration of pooling allocator
- Fuel constants need tuning for target workloads

**Mitigation:**

- Provide default `SandboxConfig` with sensible limits
- Document fuel consumption patterns
- Include examples of pooling allocator setup

## Configuration

```rust
let mut config = Config::new();
config
    .consume_fuel(true)
    .allocation_strategy(InstanceAllocationStrategy::pooling({
        let mut pooling = PoolingAllocationConfig::default();
        pooling.instance_memory_pages(4096);  // 256MB
        pooling
    }));
```

## References

- Wasmtime Book: <https://docs.wasmtime.dev/>
- Security audit: <https://bytecodealliance.org/articles/security-audit-2024>
- .local/mcp-wasm-sandbox-technical.md specifications
