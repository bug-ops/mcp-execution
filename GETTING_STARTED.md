# Getting Started with MCP Code Execution

This guide walks you through using the MCP Code Execution library step by step, from basic WASM execution to full MCP server integration.

## Table of Contents

1. [Prerequisites](#prerequisites)
2. [Installation](#installation)
3. [Basic WASM Execution](#basic-wasm-execution)
4. [Security Configuration](#security-configuration)
5. [Host Functions](#host-functions)
6. [MCP Server Integration](#mcp-server-integration)
7. [Code Generation](#code-generation)
8. [Advanced Usage](#advanced-usage)
9. [Troubleshooting](#troubleshooting)

## Prerequisites

Before you begin, ensure you have:

- **Rust 1.88+** with Edition 2024 support
- **Tokio** async runtime knowledge
- Basic understanding of:
  - WebAssembly concepts
  - Model Context Protocol (MCP)
  - Async/await in Rust

### Install Rust

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Update to latest stable
rustup update stable

# Verify installation
rustc --version  # Should be 1.88.0 or higher
```

## Installation

### Add to Your Project

Add dependencies to your `Cargo.toml`:

```toml
[dependencies]
mcp-wasm-runtime = { path = "path/to/mcp-execution/crates/mcp-wasm-runtime" }
mcp-bridge = { path = "path/to/mcp-execution/crates/mcp-bridge" }
mcp-core = { path = "path/to/mcp-execution/crates/mcp-core" }

tokio = { version = "1.48", features = ["full"] }
anyhow = "1.0"

[dev-dependencies]
wat = "1.219"  # For testing with WAT format
```

### Clone and Build

```bash
# Clone the repository
git clone https://github.com/bug-ops/mcp-execution
cd mcp-execution

# Build all crates
cargo build --workspace

# Run tests to verify
cargo nextest run --workspace
# Or: cargo test --workspace

# Expected output: 696/696 tests passing
```

## Basic WASM Execution

Let's start with the simplest possible example: executing a WASM module that returns 42.

### Step 1: Create a New Binary

```bash
cargo new --bin mcp-hello
cd mcp-hello
```

### Step 2: Add Dependencies

Edit `Cargo.toml`:

```toml
[dependencies]
mcp-wasm-runtime = { path = "../mcp-execution/crates/mcp-wasm-runtime" }
mcp-bridge = { path = "../mcp-execution/crates/mcp-bridge" }
tokio = { version = "1.48", features = ["full"] }
wat = "1.219"
```

### Step 3: Write Your First Program

Edit `src/main.rs`:

```rust
use mcp_wasm_runtime::{Runtime, SecurityConfig};
use mcp_bridge::Bridge;
use std::sync::Arc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Create MCP bridge (handles communication with MCP servers)
    let bridge = Bridge::new(1000); // 1000ms timeout

    // 2. Configure security (memory limit, execution timeout)
    let config = SecurityConfig::default();

    // 3. Create WASM runtime
    let runtime = Runtime::new(Arc::new(bridge), config)?;

    // 4. Define a simple WASM module in WAT (WebAssembly Text)
    let wat = r#"
        (module
            ;; Export a function that returns 42
            (func (export "main") (result i32)
                (i32.const 42)
            )
        )
    "#;

    // 5. Parse WAT to WASM bytecode
    let wasm_bytes = wat::parse_str(wat)?;

    // 6. Execute the module
    let result = runtime.execute(&wasm_bytes, "main", &[]).await?;

    // 7. Display result
    println!("✓ Execution successful!");
    println!("  Exit code: {}", result["exit_code"]);
    println!("  Time: {}ms", result["elapsed_ms"]);

    Ok(())
}
```

### Step 4: Run It

```bash
cargo run
```

Expected output:

```text
✓ Execution successful!
  Exit code: 42
  Time: 2ms
```

**What just happened?**

1. Created a Bridge for MCP communication
2. Configured a secure sandbox (256MB memory, 60s timeout)
3. Initialized the Wasmtime runtime
4. Compiled WAT → WASM
5. Executed in isolated sandbox
6. Got result as JSON

## Security Configuration

The default security config is fine for development, but you can customize it:

### Default Configuration

```rust
use mcp_wasm_runtime::SecurityConfig;

let config = SecurityConfig::default();

// Default values:
// - Memory limit: 256MB
// - Execution timeout: 60 seconds
// - Fuel: Disabled (use timeout for CPU protection)
// - Network access: Denied
// - Max host calls: 1000
```

### Custom Configuration

```rust
use mcp_wasm_runtime::SecurityConfig;
use std::time::Duration;
use std::path::PathBuf;

let config = SecurityConfig::builder()
    .memory_limit_mb(512)                          // 512MB memory
    .execution_timeout(Duration::from_secs(30))    // 30s timeout
    .max_fuel(10_000_000)                          // Enable fuel limit
    .preopen_dir(PathBuf::from("/tmp/wasm"))       // Allow file access
    .allow_network(false)                          // Still deny network
    .max_host_calls(500)                           // Limit host calls
    .build();

let runtime = Runtime::new(Arc::new(bridge), config)?;
```

### Security Best Practices

1. **Always use timeouts**: Protects against infinite loops
2. **Limit memory**: Prevents DoS via memory exhaustion
3. **Minimize host calls**: Rate limit prevents abuse
4. **Deny network by default**: Only allow via MCP bridge
5. **Validate all inputs**: Never trust WASM code

## Host Functions

Host functions allow WASM modules to call Rust functions. Currently implemented:

### Available Host Functions

#### 1. `host_add(a: i32, b: i32) -> i32`

Simple arithmetic for testing:

```rust
let wat = r#"
    (module
        (import "env" "host_add" (func $add (param i32 i32) (result i32)))

        (func (export "main") (result i32)
            ;; 10 + 32 = 42
            (call $add (i32.const 10) (i32.const 32))
        )
    )
"#;

let wasm = wat::parse_str(wat)?;
let result = runtime.execute(&wasm, "main", &[]).await?;

assert_eq!(result["exit_code"], 42);
```

#### 2. `host_log(ptr: i32, len: i32)`

Log strings from WASM:

```rust
let wat = r#"
    (module
        (import "env" "host_log" (func $log (param i32 i32)))

        ;; Allocate memory
        (memory (export "memory") 1)

        ;; Store "Hello from WASM!" at offset 0
        (data (i32.const 0) "Hello from WASM!")

        (func (export "main") (result i32)
            ;; Log the message (ptr=0, len=16)
            (call $log (i32.const 0) (i32.const 16))
            (i32.const 0)
        )
    )
"#;

let wasm = wat::parse_str(wat)?;
let result = runtime.execute(&wasm, "main", &[]).await?;

// Check logs for: [WASM] Hello from WASM!
```

### Using Host Functions

1. **Import in WASM**: Declare functions in `import` section
2. **Export memory**: WASM must export memory for data transfer
3. **Validate bounds**: Host checks ptr + len < memory.len()
4. **Handle errors**: Invalid UTF-8 logs error, doesn't crash

## MCP Server Integration

Integrate with real MCP servers for tool discovery and execution.

### Step 1: Discover MCP Server

```rust
use mcp_introspector::Introspector;
use mcp_core::ServerId;

let mut introspector = Introspector::new();

// Discover server (stdio transport)
let server_id = ServerId::new("github");
let server_command = vec!["node".to_string(), "server.js".to_string()];

let server_info = introspector
    .discover_server(&server_id, server_command)
    .await?;

println!("✓ Discovered server: {}", server_info.name);
println!("  Tools: {}", server_info.tools.len());

for tool in &server_info.tools {
    println!("    - {} : {}", tool.name, tool.description);
}
```

### Step 2: Generate TypeScript Code

```rust
use mcp_codegen::CodeGenerator;
use std::fs;

// Initialize code generator
let generator = CodeGenerator::new()?;

// Generate TypeScript interfaces
let generated = generator.generate(&server_info)?;

// Write to disk
for file in &generated.files {
    let path = format!("/mcp-tools/servers/github/{}", file.relative_path);
    fs::create_dir_all(std::path::Path::new(&path).parent().unwrap())?;
    fs::write(&path, &file.content)?;
    println!("✓ Generated: {}", path);
}
```

**Generated structure:**

```text
/mcp-tools/servers/github/
├── manifest.json       # Server metadata
├── types.ts            # Shared types
├── tools/
│   ├── sendMessage.ts
│   ├── getMessage.ts
│   └── getChat.ts
└── index.ts            # Barrel export
```

### Step 3: Call MCP Tools from WASM

```rust
// Create bridge with server connection
let bridge = Bridge::new(1000);

// Connect to server
bridge.connect_server(
    &ServerId::new("github"),
    server_command
).await?;

// Use host context to call tools
use mcp_wasm_runtime::HostContext;

let context = HostContext::new(Arc::new(bridge));

let result = context.call_tool(
    &ServerId::new("github"),
    &ToolName::new("send_message"),
    serde_json::json!({
        "chat_id": "123",
        "text": "Hello from MCP!"
    })
).await?;

println!("Tool result: {:?}", result);
```

## Code Generation

The code generator creates type-safe TypeScript interfaces from MCP tool schemas.

### Input: MCP Tool Schema

```json
{
  "name": "send_message",
  "description": "Send a message to a chat",
  "inputSchema": {
    "type": "object",
    "properties": {
      "chat_id": { "type": "string" },
      "text": { "type": "string" }
    },
    "required": ["chat_id", "text"]
  }
}
```

### Output: TypeScript Interface

```typescript
// tools/sendMessage.ts

import { callTool } from '../bridge';

export interface SendMessageParams {
  chat_id: string;
  text: string;
}

export interface SendMessageResult {
  // Result type from schema
}

export async function sendMessage(params: SendMessageParams): Promise<SendMessageResult> {
  const result = await callTool('send_message', params);
  return result as SendMessageResult;
}
```

### Customize Generation

```rust
use mcp_codegen::{CodeGenerator, TemplateEngine};

let mut engine = TemplateEngine::new()?;

// Register custom template
engine.register_template("custom_tool", r#"
// Custom template for {{name}}
export const {{typescript_name}} = async (params) => {
  // Your custom logic
};
"#)?;

let generator = CodeGenerator::with_engine(engine);
```

## Plugin Persistence

Save and load pre-generated MCP plugins to avoid regenerating code on every use.

### Save Plugin During Generation

Generate code and save as a reusable plugin:

```bash
# Generate and save
mcp-cli generate github --save-plugin

# Custom plugin directory
mcp-cli generate github --save-plugin --plugin-dir ~/.mcp-plugins
```

This creates a plugin directory structure:

```text
./plugins/github/
├── plugin.json       # Metadata with checksums
├── module.wasm       # WASM module
└── generated/        # TypeScript files
    ├── index.ts
    ├── send_message.ts
    └── ...
```

### Load Saved Plugin

Load a plugin without regenerating:

```bash
# Load from default directory (./plugins)
mcp-cli plugin load github

# Load from custom directory
mcp-cli plugin load github --plugin-dir ~/.mcp-plugins

# JSON output
mcp-cli plugin load github -o json
```

### List Available Plugins

See all saved plugins:

```bash
# List plugins
mcp-cli plugin list

# JSON output for scripting
mcp-cli plugin list -o json
```

Output:

```text
Available plugins (2):
  • github (v1.0.0)
    Tools: 10 | Files: 15 | Generated: 2025-11-21
  • github (v2.0.0)
    Tools: 8 | Files: 12 | Generated: 2025-11-20
```

### Show Plugin Details

Get detailed information about a plugin:

```bash
# Show info
mcp-cli plugin info github

# Pretty JSON output
mcp-cli plugin info github -o pretty
```

### Remove Plugin

Delete a saved plugin:

```bash
# Remove with confirmation
mcp-cli plugin remove github

# Skip confirmation
mcp-cli plugin remove github -y
```

### Security Features

Plugin persistence includes security measures:

1. **Blake3 Checksums**: All files verified on load
2. **Constant-Time Comparison**: Prevents timing attacks
3. **Atomic Operations**: No race conditions during save
4. **Path Validation**: Rejects malicious paths (../, etc.)
5. **Control Character Rejection**: Prevents injection attacks

### Programmatic Usage

Use plugins from Rust code:

```rust
use mcp_plugin_store::PluginStore;

// Create store
let store = PluginStore::new("./plugins")?;

// Load plugin
let plugin = store.load_plugin("github")?;

println!("WASM size: {} bytes", plugin.wasm_module.len());
println!("VFS files: {}", plugin.vfs.file_count());
println!("Tools: {}", plugin.metadata.tools.len());

// Use the loaded VFS and WASM
let runtime = Runtime::new(Arc::new(bridge), config)?;
let result = runtime.execute(&plugin.wasm_module, "main", &[]).await?;
```

## Advanced Usage

### Compilation Caching

The compiler caches WASM modules using BLAKE3 hashes:

```rust
use mcp_wasm_runtime::Compiler;

let compiler = Compiler::new();

// First compilation (slow)
let wasm1 = compiler.compile("export function main(): i32 { return 42; }")?;

// Second compilation (instant - cache hit)
let wasm2 = compiler.compile("export function main(): i32 { return 42; }")?;

// Different code (cache miss)
let wasm3 = compiler.compile("export function main(): i32 { return 123; }")?;

// Check cache stats
let (count, size_bytes) = compiler.cache_stats();
println!("Cache: {} modules, {} bytes", count, size_bytes);

// Clear cache
compiler.clear_cache();
```

### Session State Management

Store per-session state:

```rust
use mcp_core::SessionId;
use serde_json::json;

let session = SessionId::generate();

// Set state
context.set_state(&session, "counter", json!(42)).await?;

// Get state
let value = context.get_state(&session, "counter").await?;
assert_eq!(value, json!(42));

// Clear session
context.clear_session(&session).await;
```

### Virtual Filesystem

Populate VFS with files:

```rust
use std::collections::HashMap;

let mut files = HashMap::new();
files.insert("/config.json".to_string(), b"{}".to_vec());
files.insert("/data.txt".to_string(), b"hello".to_vec());

context.populate_vfs(files).await;

// WASM can now read these files
let content = context.read_file("/config.json").await?;
```

### Multiple Servers

Connect to multiple MCP servers:

```rust
let bridge = Bridge::new(1000);

// Server 1: github
bridge.connect_server(
    &ServerId::new("github"),
    vec!["node".into(), "vk-server.js".into()]
).await?;

// Server 2: github
bridge.connect_server(
    &ServerId::new("github"),
    vec!["node".into(), "github-server.js".into()]
).await?;

// Call tools from different servers
let vk_result = context.call_tool(
    &ServerId::new("github"),
    &ToolName::new("send_message"),
    json!({"chat_id": "123", "text": "Hi"})
).await?;

let gh_result = context.call_tool(
    &ServerId::new("github"),
    &ToolName::new("create_issue"),
    json!({"repo": "user/repo", "title": "Bug"})
).await?;
```

## Troubleshooting

### Common Issues

#### 1. "Failed to compile WASM module"

**Cause**: Invalid WAT syntax or corrupted WASM bytecode.

**Solution**:

```rust
// Validate WAT before parsing
let wat = r#"..."#;
match wat::parse_str(wat) {
    Ok(wasm) => println!("✓ Valid WAT"),
    Err(e) => eprintln!("✗ Invalid WAT: {}", e),
}
```

#### 2. "Entry point 'main' not found"

**Cause**: WASM module doesn't export the function you're calling.

**Solution**:

```rust
// Always export your functions
let wat = r#"
    (module
        (func (export "main") (result i32)  ;; <-- 'export "main"'
            (i32.const 42)
        )
    )
"#;
```

#### 3. "Memory limit exceeded"

**Cause**: WASM module tries to allocate more memory than allowed.

**Solution**:

```rust
// Increase memory limit
let config = SecurityConfig::builder()
    .memory_limit_mb(512)  // Increase from 256MB
    .build();
```

#### 4. "WASM execution failed: all fuel consumed"

**Cause**: Fuel was enabled but not initialized (Wasmtime issue).

**Solution**:

```rust
// Disable fuel (default in SecurityConfig)
let config = SecurityConfig::default();

// OR explicitly disable
let config = SecurityConfig::builder()
    .unlimited_fuel()
    .build();
```

#### 5. "Timeout: WASM execution exceeded 60 seconds"

**Cause**: WASM code has infinite loop or is too slow.

**Solution**:

```rust
// Increase timeout
let config = SecurityConfig::builder()
    .execution_timeout(Duration::from_secs(120))
    .build();

// OR debug the WASM code
```

### Debug Logging

Enable detailed logging:

```rust
use tracing_subscriber;

tracing_subscriber::fmt()
    .with_env_filter("mcp_wasm_runtime=trace")
    .init();

// Now run your code - you'll see detailed logs:
// [TRACE] Memory growing: 0 -> 65536 bytes
// [DEBUG] Getting entry point function: main
// [DEBUG] Calling entry point function asynchronously
// [DEBUG] WASM function returned: 42
// [INFO] WASM execution completed in 2.1ms, exit code: 42
```

### Performance Tips

1. **Precompile WASM**: Use `Compiler::load_precompiled()` for production
2. **Reuse Runtime**: Create once, execute many times
3. **Enable caching**: Compiler caches automatically
4. **Batch operations**: Group multiple tool calls
5. **Profile first**: Use `cargo flamegraph` to find bottlenecks

### Getting Help

- **Documentation**: Run `cargo doc --open`
- **Examples**: Check `examples/` directory
- **Tests**: Look at integration tests in `tests/`
- **Issues**: <https://github.com/bug-ops/mcp-execution/issues>
- **MCP Spec**: <https://spec.modelcontextprotocol.io/>

## Next Steps

1. **Try the examples**: Explore `examples/` directory
2. **Read the docs**: `cargo doc --workspace --open`
3. **Check ADRs**: See `docs/adr/` for design decisions
4. **Contribute**: See `CLAUDE.md` for guidelines
5. **Build CLI**: Implement your own CLI tool

Happy coding! 🚀
