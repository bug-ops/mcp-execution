# MCP Code Execution Examples

This directory contains practical examples demonstrating the MCP Code Execution library.

## Available Examples

### 1. Simple Execution (`simple_execution.rs`)

**What it demonstrates:**
- Creating a WASM runtime with default security config
- Compiling WAT (WebAssembly Text) to WASM
- Executing WASM with host function calls
- Using `host_add` for arithmetic

**Run:**
```bash
cargo run -p mcp-wasm-runtime --example simple_execution
```

**Expected output:**
```
🚀 MCP Code Execution - Simple Example

1️⃣  Creating MCP bridge...
   ✓ Bridge created with 1000ms timeout

2️⃣  Configuring security sandbox...
   ✓ Memory limit: 256MB
   ✓ Execution timeout: 60s
   ✓ Host call limit: 1000

3️⃣  Creating WASM runtime...
   ✓ Runtime initialized

4️⃣  Compiling WASM module...
   ✓ Compiled 89 bytes of WASM

5️⃣  Executing WASM module...
   ✓ Execution completed

📊 Results:
   Exit code: 42
   Elapsed time: 2ms
   Expected: 42 (10 + 32)

✅ Success! The result is correct.
```

### 2. Host Logging (`host_logging.rs`)

**What it demonstrates:**
- Logging from WASM to Rust host
- Memory exports and data sections
- String handling across WASM boundary
- Multiple log calls from single execution

**Run:**
```bash
cargo run -p mcp-wasm-runtime --example host_logging
```

**Expected output:**
```
🎤 MCP Code Execution - Host Logging Example

📝 Compiling WASM module with embedded strings...

🚀 Executing WASM (watch for log messages below):

─────────────────────────────────────────────────
INFO [WASM] Hello from WASM!
INFO [WASM] Executing in secure sandbox
INFO [WASM] All systems operational
─────────────────────────────────────────────────

📊 Execution completed:
   Exit code: 0
   Time: 1ms

✅ All messages logged successfully!
```

### 3. Code Generation (in `crates/mcp-examples/examples/codegen.rs`)

**What it demonstrates:**
- Discovering MCP servers with rmcp
- Extracting tool schemas
- Generating TypeScript interfaces
- Creating virtual filesystem structure

**Run:**
```bash
cd crates/mcp-examples
cargo run --example codegen
```

This example requires a running MCP server. See the code for details.

## Building All Examples

```bash
# From repository root
cargo build -p mcp-wasm-runtime --examples

# Run specific example
cargo run -p mcp-wasm-runtime --example simple_execution
cargo run -p mcp-wasm-runtime --example host_logging
```

## Common Patterns

### Security Configuration

All examples use secure defaults, but you can customize:

```rust
use mcp_wasm_runtime::SecurityConfig;
use std::time::Duration;

let config = SecurityConfig::builder()
    .memory_limit_mb(512)                       // Increase memory
    .execution_timeout(Duration::from_secs(30)) // Shorter timeout
    .build();
```

### Error Handling

Examples use `?` operator with `Result<(), Box<dyn std::error::Error>>`:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let result = runtime.execute(&wasm, "main", &[]).await?;
    // Errors propagate automatically
    Ok(())
}
```

### Logging

Enable detailed logs with environment variable:

```bash
RUST_LOG=mcp_wasm_runtime=trace cargo run -p mcp-wasm-runtime --example simple_execution
```

## Next Steps

1. **Modify examples**: Change the WAT code and experiment
2. **Add host functions**: Implement your own in `Runtime::link_host_functions`
3. **Integrate MCP**: Connect to real MCP servers
4. **Build CLI**: Use examples as reference for your tool

## Resources

- [Getting Started Guide](../GETTING_STARTED.md)
- [API Documentation](https://docs.rs/mcp-wasm-runtime)
- [WAT Specification](https://webassembly.github.io/spec/core/text/index.html)
- [MCP Specification](https://spec.modelcontextprotocol.io/)
