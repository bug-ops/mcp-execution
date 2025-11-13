# MCP Examples

Examples and integration tests demonstrating the MCP Code Execution workflow.

## Overview

This crate provides comprehensive examples, integration tests, and benchmarks for the MCP Code Execution pattern. It demonstrates the complete pipeline from server introspection through WASM execution.

## Examples

### End-to-End Workflow

Demonstrates the complete MCP Code Execution pipeline:

```bash
cargo run --example e2e_workflow
```

**What it demonstrates:**
- Server introspection (using mock server)
- TypeScript code generation from tool schemas
- Virtual filesystem loading
- WASM runtime setup and execution
- Token efficiency analysis

**Expected output:**
- Complete workflow execution report
- Performance metrics for each phase
- Token savings analysis
- Performance target validation

### Token Analysis

Analyzes token usage across different scenarios:

```bash
cargo run --example token_analysis
```

**What it demonstrates:**
- Token usage comparison (Standard MCP vs Code Execution)
- Scaling analysis (different call counts)
- Break-even point calculation
- Recommendations for usage

**Scenarios analyzed:**
- Few calls (3) - shows initial overhead
- Typical workflow (20 calls) - normal usage
- Heavy usage (100 calls) - multi-agent workflows

### Performance Test

Validates performance targets:

```bash
cargo run --example performance_test --release
```

**What it tests:**
- Code generation speed
- VFS loading performance
- WASM compilation time (<100ms target)
- Execution overhead (<50ms target)
- End-to-end latency

**Note:** Run with `--release` for realistic performance measurements.

### Existing Examples

#### VKTeams Integration Test

Tests real MCP server integration:

```bash
cargo run --example test_vkteams
```

Requires `vkteams-bot-server` to be installed and in PATH.

#### Code Generation Demo

Demonstrates code generation from any MCP server:

```bash
cargo run --example codegen -- <server-command>
```

Example:
```bash
cargo run --example codegen -- /usr/local/bin/vkteams-bot-server
```

## Integration Tests

Run the complete integration test suite:

```bash
cargo test --package mcp-examples
```

### Test Coverage

**Mock Server Tests:**
- Server creation and configuration
- Tool discovery and invocation
- Error handling (invalid tools, missing parameters)

**Code Generation Integration:**
- Introspection → Code generation pipeline
- File generation verification
- TypeScript output validation

**VFS Integration:**
- Code generation → VFS loading
- File reading and directory listing
- Multi-server support

**WASM Runtime Integration:**
- Runtime creation and configuration
- WASM execution
- Security sandbox validation

**Token Analysis:**
- Token calculation accuracy
- Scaling behavior
- Target achievement validation

**End-to-End:**
- Complete workflow execution
- Error propagation
- Multi-server scenarios

**Performance:**
- Component performance validation
- Latency requirements
- Throughput testing

### Running Specific Tests

Run mock server tests only:
```bash
cargo test --package mcp-examples test_mock_server
```

Run integration tests only:
```bash
cargo test --package mcp-examples test_e2e
```

Run performance tests:
```bash
cargo test --package mcp-examples test_.*_performance
```

## Benchmarks

Run criterion benchmarks:

```bash
cargo bench --package mcp-examples
```

### Available Benchmarks

**Component Benchmarks:**
- `code_generation` - Code generation from server info
- `vfs_build` - VFS construction from generated code
- `runtime_creation` - WASM runtime initialization
- `wasm_execution` - WASM module execution

**Workflow Benchmarks:**
- `e2e_workflow` - Complete end-to-end pipeline

**Scaling Benchmarks:**
- `scaling_tools` - Performance with different tool counts (1, 5, 10, 20, 50)

**Cache Benchmarks:**
- `cold_vs_warm` - Cold start vs cached execution comparison

### Interpreting Results

Criterion generates detailed reports in `target/criterion/`:
- HTML reports with graphs
- Statistical analysis
- Regression detection

Example output:
```
code_generation         time:   [12.345 ms 12.567 ms 12.789 ms]
vfs_build              time:   [234.56 µs 245.67 µs 256.78 µs]
runtime_creation       time:   [45.678 ms 46.789 ms 47.890 ms]
wasm_execution         time:   [1.2345 ms 1.2567 ms 1.2789 ms]
```

## Library API

The `mcp-examples` crate also provides utilities for testing:

### Mock Server

```rust
use mcp_examples::mock_server::MockMcpServer;

// Create mock VKTeams Bot server
let server = MockMcpServer::new_vkteams_bot();

// Get server info
let info = server.server_info();

// Call tools
let result = server.call_tool(
    "send_message",
    json!({"chat_id": "123", "text": "Hello"})
).await?;

// Configure custom responses
let mut server = MockMcpServer::new_vkteams_bot();
server.set_response("send_message", json!({"message_id": "custom"}));
```

### Metrics Collection

```rust
use mcp_examples::metrics::Metrics;

let mut metrics = Metrics::new();

// Track timing for each phase
metrics.start_introspection();
// ... perform introspection
metrics.end_introspection();

metrics.start_code_generation();
// ... generate code
metrics.end_code_generation();

// Check performance targets
assert!(metrics.meets_execution_target());
assert!(metrics.meets_compilation_target());
assert!(metrics.meets_token_target());

// Generate report
println!("{}", metrics.format_report());
```

### Token Analysis

```rust
use mcp_examples::token_analysis::TokenAnalysis;

// Analyze token usage
let analysis = TokenAnalysis::analyze(&server_info, num_calls);

println!("Token savings: {:.1}%", analysis.savings_percent);

if analysis.is_significant_savings() {
    println!("Achieved 90%+ savings target!");
}

// Generate detailed report
println!("{}", analysis.format_report());
```

## Performance Targets

The MCP Code Execution implementation targets:

| Metric | Target | Measured |
|--------|--------|----------|
| WASM Compilation | <100ms | ✓ Validated |
| Execution Overhead | <50ms | ✓ Validated |
| Token Savings (100 calls) | ≥90% | ✓ Achieved (93%+) |
| Code Generation | Informational | ~10-50ms |
| VFS Loading | Informational | ~0.2-2ms |

**Note:** Measurements are from release builds on modern hardware.

## Token Efficiency Model

### Standard MCP Approach

- **Initial listing:** 500 tokens/tool × N tools
- **Per call:** 300 tokens (includes schema + parameters)
- **Total for M calls:** `500N + 300M` tokens

### Code Execution Approach

- **One-time code generation:** 200 tokens/tool × N tools
- **Per call:** 50 tokens (function name + compact args)
- **Total for M calls:** `200N + 50M` tokens

### Savings Calculation

```
Savings = (Standard - CodeExec) / Standard × 100%
        = ((500N + 300M) - (200N + 50M)) / (500N + 300M) × 100%
        = (300N + 250M) / (500N + 300M) × 100%
```

### Break-Even Analysis

For 90% savings:
```
(300N + 250M) / (500N + 300M) ≥ 0.9
300N + 250M ≥ 450N + 270M
-150N ≥ 20M
M ≥ 7.5N (approximately)
```

For a server with 4 tools: minimum ~30 calls for 90% savings.

## Usage Recommendations

**Use Code Execution when:**
- Workflows involve 3+ tool calls
- Multi-agent systems
- Long-running conversations
- Servers with many tools (5+)

**Standard MCP is sufficient for:**
- Single, one-off tool calls
- Exploratory/discovery phase
- Very simple servers (1-2 tools)

**Best Practices:**
1. Cache generated code aggressively
2. Batch multiple tool calls when possible
3. Monitor token usage in production
4. Use integration tests to validate workflows

## Development

### Adding New Examples

1. Create example file in `examples/`:
   ```rust
   //! Example description

   use mcp_examples::...;

   fn main() -> Result<(), Box<dyn std::error::Error>> {
       // Your example code
       Ok(())
   }
   ```

2. Register in `Cargo.toml`:
   ```toml
   [[example]]
   name = "my_example"
   path = "examples/my_example.rs"
   ```

3. Run with:
   ```bash
   cargo run --example my_example
   ```

### Adding New Tests

Add tests to `tests/integration_test.rs`:

```rust
#[tokio::test]
async fn test_my_feature() {
    // Your test code
}
```

### Adding New Benchmarks

Add benchmarks to `benches/e2e_benchmark.rs`:

```rust
fn bench_my_feature(c: &mut Criterion) {
    c.bench_function("my_feature", |b| {
        b.iter(|| {
            // Your benchmark code
        });
    });
}

criterion_group!(benches, ..., bench_my_feature);
```

## Troubleshooting

### Example Fails with "Server not found"

The `test_vkteams` example requires the actual MCP server. For testing without external dependencies, use `e2e_workflow` which uses the mock server.

### Performance Tests Fail

Performance tests are calibrated for release builds. Run with:
```bash
cargo run --example performance_test --release
```

Debug builds will be significantly slower.

### Benchmark Results Unstable

For stable benchmark results:
1. Close other applications
2. Disable CPU frequency scaling
3. Run multiple times and check variance
4. Use `cargo bench` (not `cargo test`)

## See Also

- [Project Documentation](../../README.md) - Overall architecture
- [mcp-core](../mcp-core/README.md) - Core types and traits
- [mcp-codegen](../mcp-codegen/README.md) - Code generation
- [mcp-wasm-runtime](../mcp-wasm-runtime/README.md) - WASM execution
- [Architecture Decision Records](../../docs/adr/) - Design decisions
