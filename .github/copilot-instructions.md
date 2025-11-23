# GitHub Copilot Instructions for MCP Execution

**Last Updated**: 2025-11-23

This document provides comprehensive guidelines for GitHub Copilot to generate high-quality code for the MCP Execution project, synthesizing expertise from architectural design, development practices, testing strategies, performance optimization, security, code review, and CI/CD.

---

## Table of Contents

1. [Project Context](#project-context)
2. [Core Principles](#core-principles)
3. [Code Generation Guidelines](#code-generation-guidelines)
4. [Architecture & Design Patterns](#architecture--design-patterns)
5. [Error Handling](#error-handling)
6. [Testing Requirements](#testing-requirements)
7. [Performance Optimization](#performance-optimization)
8. [Security Requirements](#security-requirements)
9. [Documentation Standards](#documentation-standards)
10. [CI/CD Integration](#cicd-integration)
11. [Code Review Checklist](#code-review-checklist)

---

## Project Context

### Project Overview

**MCP Execution** is a Rust workspace implementing the Code Execution pattern for Model Context Protocol (MCP) using WebAssembly for secure, isolated code execution.

**Key Metrics**:
- 90%+ token reduction target
- <50ms execution overhead
- Zero sandbox escapes
- 314+ tests (100% passing)
- Cross-platform support (Linux, macOS, Windows)

### Workspace Structure

```
mcp-execution/
├── crates/
│   ├── mcp-core/          - Foundation: types, traits, errors
│   ├── mcp-introspector/  - MCP server analysis (rmcp v0.9)
│   ├── mcp-codegen/       - Code generation (wasm/skills features)
│   ├── mcp-bridge/        - WASM ↔ MCP proxy (rmcp client)
│   ├── mcp-wasm-runtime/  - Wasmtime sandbox (v39.0)
│   ├── mcp-vfs/           - Virtual filesystem
│   ├── mcp-cli/           - CLI application
│   ├── mcp-skill-store/   - Skill management
│   ├── mcp-skill-generator/ - Skill generation
│   └── mcp-examples/      - Usage examples
└── .local/                - Detailed documentation (not in git)
```

### Technology Stack

- **Rust Edition**: 2024 (MSRV: 1.89)
- **MCP SDK**: rmcp v0.9 (official Rust SDK)
- **WASM Runtime**: Wasmtime v39.0
- **Async Runtime**: Tokio v1.48
- **Testing**: cargo-nextest, criterion
- **CI/CD**: GitHub Actions with cargo-deny

---

## Core Principles

### 1. Microsoft Rust Guidelines Compliance

**CRITICAL**: All code MUST follow [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt).

**Key Requirements**:

- **Strong Types Over Primitives**: Use `ServerId`, `ToolName`, `SessionId`, etc., not `String` or `u64`
- **Error Handling**: `thiserror` for library crates, `anyhow` only for CLI (`mcp-cli`)
- **Public Types**: Must implement `Send + Sync + Debug`
- **Documentation**: Every public item needs doc comments with examples
- **Safety**: `unsafe` only for FFI or novel abstractions; must include `SAFETY` comments

**Example - Strong Types**:
```rust
// ✅ GOOD: Type-safe IDs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ServerId(u64);

impl ServerId {
    pub fn new(id: u64) -> Self {
        Self(id)
    }

    pub fn as_u64(&self) -> u64 {
        self.0
    }
}

// ❌ BAD: Primitive types
pub fn connect_server(server_id: u64) -> Result<Connection> { ... }
```

### 2. Official MCP SDK Usage

**CRITICAL**: Use **rmcp v0.9** for all MCP communication.

```rust
// ✅ GOOD: Use rmcp client
use rmcp::client::TokioChildProcess;

let transport = TokioChildProcess::new("github")?;
let client = rmcp::client::Client::new(transport);

// ❌ BAD: Custom MCP protocol implementation
// NEVER implement custom JSON-RPC for MCP
```

### 3. Security First

- WASM sandbox with memory/CPU limits
- No network access from WASM (only via MCP Bridge)
- WASI preopened directories only
- Rate limiting on all tool calls
- No hardcoded secrets
- Input validation on all external data

### 4. Naming Conventions

- **Crates**: `mcp-{feature}` (kebab-case)
- **Files & modules**: `snake_case`
- **Types & traits**: `PascalCase`
- **Functions & variables**: `snake_case`
- **Constants**: `SCREAMING_SNAKE_CASE`

### 5. Version Control & File Management

**CRITICAL**: Working and intermediate files must NEVER be committed to the repository.

**Working/Intermediate Files Location**:
- All temporary, draft, and working documents belong in `.local/` directory
- `.local/` is excluded from version control (in `.gitignore`)
- Use `.local/` for implementation plans, status reports, research notes, etc.

**What to commit**:
- ✅ Source code (`src/`, `tests/`, `benches/`)
- ✅ Configuration files (`Cargo.toml`, `.github/`, `deny.toml`)
- ✅ Documentation (`README.md`, `docs/`, ADRs)
- ✅ Project metadata (`LICENSE`, `CHANGELOG.md`)

**What NOT to commit**:
- ❌ Working documents and drafts (→ use `.local/`)
- ❌ Build artifacts (`target/`, `*.wasm`)
- ❌ IDE files (`.vscode/`, `.idea/`)
- ❌ Temporary files (`*.tmp`, `*.swp`, `*.bak`)
- ❌ Sensitive data (`.env`, credentials)
- ❌ Generated reports and analysis outputs (→ use `.local/`)

**Example**:
```bash
# ✅ GOOD: Working documents in .local/
.local/
├── implementation-plan.md
├── performance-analysis.md
├── security-audit-results.md
└── meeting-notes.md

# ❌ BAD: Don't commit these to root or docs/
implementation-plan.md          # → Move to .local/
docs/working-draft.md          # → Move to .local/
performance-notes.txt          # → Move to .local/
```

**When suggesting file creation**:
- If creating a working document, status update, or temporary analysis: suggest `.local/` directory
- If creating permanent documentation or code: suggest appropriate source directory
- Always check if `.local/` exists and use it for non-permanent files

---

## Code Generation Guidelines

### Ownership & Borrowing

**Default Preference Order**:

1. **Immutable borrow** `&T` - Use when you only need to read
2. **Mutable borrow** `&mut T` - Use when you need to modify
3. **Owned value** `T` - Use when you need to transfer ownership
4. **Clone** `.clone()` - Last resort, document why necessary

```rust
// ✅ GOOD: Accept borrowed data
pub fn process_tool(tool: &ToolDefinition) -> String {
    format!("Processing: {}", tool.name)
}

// ❌ BAD: Unnecessary ownership transfer
pub fn process_tool(tool: ToolDefinition) -> String {
    format!("Processing: {}", tool.name)
}

// ✅ GOOD: Use &str for parameters
pub fn validate_name(name: &str) -> bool {
    !name.is_empty() && name.len() < 256
}

// ❌ BAD: Don't require owned String
pub fn validate_name(name: String) -> bool {
    !name.is_empty() && name.len() < 256
}
```

### Iterator Patterns

**Prefer iterators over explicit loops**:

```rust
// ✅ GOOD: Iterator chains
let active_tools: Vec<_> = tools
    .iter()
    .filter(|t| t.is_enabled)
    .map(|t| t.name.clone())
    .collect();

// ✅ GOOD: Early return with iterator methods
fn find_tool_by_name<'a>(tools: &'a [Tool], name: &str) -> Option<&'a Tool> {
    tools.iter().find(|t| t.name == name)
}
```

### Memory Allocation

**Minimize allocations**:

```rust
// ✅ GOOD: Pre-allocate with known capacity
let mut vec = Vec::with_capacity(expected_size);

// ✅ GOOD: Reuse buffers
let mut buffer = String::new();
for item in items {
    buffer.clear();
    write!(&mut buffer, "{}", item)?;
    process(&buffer);
}

// ❌ BAD: Unnecessary allocations
for item in items {
    let s = format!("Item: {}", item); // New allocation each iteration
    process(&s);
}
```

### Async/Await Patterns

```rust
// ✅ GOOD: Mark async functions clearly
async fn fetch_server_info(client: &Client) -> Result<ServerInfo> {
    let response = client.get_server_info().await?;
    Ok(response)
}

// ✅ GOOD: Use tokio::spawn for concurrent work
async fn fetch_multiple_servers(ids: Vec<ServerId>) -> Result<Vec<ServerInfo>> {
    let tasks: Vec<_> = ids
        .into_iter()
        .map(|id| tokio::spawn(fetch_server(id)))
        .collect();

    let mut servers = Vec::new();
    for task in tasks {
        servers.push(task.await??);
    }
    Ok(servers)
}

// ❌ BAD: Blocking in async context
async fn process() {
    std::thread::sleep(Duration::from_secs(1)); // Blocks thread!
}

// ✅ GOOD: Use async sleep
async fn process() {
    tokio::time::sleep(Duration::from_secs(1)).await;
}
```

---

## Architecture & Design Patterns

### Dependency Graph

**CRITICAL**: No circular dependencies allowed.

```
mcp-cli → mcp-wasm-runtime → {mcp-bridge, mcp-vfs, mcp-codegen}
                           ↘ mcp-core

mcp-bridge → rmcp (official SDK)
mcp-introspector → rmcp
```

### Module Organization

```rust
// src/lib.rs - Public API
pub mod error;     // Error types
pub mod types;     // Core types
pub mod client;    // Client implementation
pub mod server;    // Server implementation

// Re-export main types
pub use error::{Error, Result};
pub use types::{ServerId, ToolName};
```

### Builder Pattern for Complex Construction

```rust
// ✅ GOOD: Builder for structs with >3 fields
pub struct WasmConfig {
    max_memory: usize,
    timeout: Duration,
    preopened_dirs: Vec<PathBuf>,
}

pub struct WasmConfigBuilder {
    max_memory: usize,
    timeout: Duration,
    preopened_dirs: Vec<PathBuf>,
}

impl WasmConfigBuilder {
    pub fn new() -> Self {
        Self {
            max_memory: 64 * 1024 * 1024, // 64MB
            timeout: Duration::from_secs(30),
            preopened_dirs: Vec::new(),
        }
    }

    pub fn max_memory(mut self, bytes: usize) -> Self {
        self.max_memory = bytes;
        self
    }

    pub fn timeout(mut self, duration: Duration) -> Self {
        self.timeout = duration;
        self
    }

    pub fn build(self) -> WasmConfig {
        WasmConfig {
            max_memory: self.max_memory,
            timeout: self.timeout,
            preopened_dirs: self.preopened_dirs,
        }
    }
}
```

---

## Error Handling

### Library Code - Use thiserror

**For all crates EXCEPT `mcp-cli`**:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum BridgeError {
    #[error("failed to connect to MCP server")]
    ConnectionFailed(#[source] std::io::Error),

    #[error("tool '{name}' not found")]
    ToolNotFound { name: String },

    #[error("timeout after {timeout:?}")]
    Timeout { timeout: Duration },

    #[error("invalid tool input: {0}")]
    InvalidInput(String),
}

pub type Result<T> = std::result::Result<T, BridgeError>;

// Usage in functions
pub fn call_tool(name: &str, args: Value) -> Result<Value> {
    let tool = registry.find(name)
        .ok_or_else(|| BridgeError::ToolNotFound {
            name: name.to_string()
        })?;

    tool.execute(args)
}
```

### Application Code - Use anyhow

**ONLY for `mcp-cli` crate**:

```rust
use anyhow::{Context, Result, bail};

fn load_config(path: &str) -> Result<Config> {
    let content = std::fs::read_to_string(path)
        .context("failed to read config file")?;

    let config: Config = toml::from_str(&content)
        .context("failed to parse config")?;

    if config.port == 0 {
        bail!("port cannot be zero");
    }

    Ok(config)
}
```

### Never Do This

```rust
// ❌ BAD: unwrap in library code
pub fn get_tool(id: u64) -> Tool {
    registry.find(id).unwrap() // NEVER!
}

// ❌ BAD: panic in library code
pub fn divide(a: f64, b: f64) -> f64 {
    if b == 0.0 {
        panic!("division by zero"); // NEVER!
    }
    a / b
}

// ✅ GOOD: Return Result
pub fn divide(a: f64, b: f64) -> Result<f64, String> {
    if b == 0.0 {
        return Err("division by zero".into());
    }
    Ok(a / b)
}
```

---

## Testing Requirements

### Unit Tests - In Same File

**CRITICAL**: Unit tests go in `#[cfg(test)]` module in SAME FILE as code.

```rust
// src/validator.rs

pub struct Validator;

impl Validator {
    pub fn validate_email(email: &str) -> bool {
        email.contains('@') && email.len() > 3
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_email_valid() {
        assert!(Validator::validate_email("test@example.com"));
    }

    #[test]
    fn test_validate_email_invalid() {
        assert!(!Validator::validate_email("invalid"));
        assert!(!Validator::validate_email("a@b"));
    }

    #[test]
    fn test_validate_email_edge_cases() {
        assert!(!Validator::validate_email(""));
        assert!(!Validator::validate_email("@"));
    }
}
```

### Test Coverage Requirements

**For each public function**:

1. **Happy path** - Normal, expected input
2. **Error cases** - Invalid input, error conditions
3. **Edge cases** - Boundaries, empty, extremes

```rust
#[cfg(test)]
mod tests {
    use super::*;

    // Happy path
    #[test]
    fn test_create_server_id_valid() {
        let id = ServerId::new(42);
        assert_eq!(id.as_u64(), 42);
    }

    // Error case
    #[test]
    fn test_parse_invalid_json() {
        let result = parse_tool_definition("invalid json");
        assert!(result.is_err());
    }

    // Edge cases
    #[test]
    fn test_empty_tool_list() {
        let tools = Vec::new();
        let result = find_tool(&tools, "test");
        assert_eq!(result, None);
    }

    #[test]
    fn test_max_server_id() {
        let id = ServerId::new(u64::MAX);
        assert_eq!(id.as_u64(), u64::MAX);
    }
}
```

### Async Testing

```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_async_fetch_server() {
        let server = fetch_server(ServerId::new(1)).await.unwrap();
        assert_eq!(server.id, ServerId::new(1));
    }

    #[tokio::test]
    async fn test_async_timeout() {
        let result = tokio::time::timeout(
            Duration::from_millis(100),
            very_slow_operation()
        ).await;
        assert!(result.is_err());
    }
}
```

### Integration Tests

**Location**: `tests/` directory in crate root.

```rust
// tests/integration_test.rs

use mcp_bridge::{Bridge, BridgeConfig};

#[tokio::test]
async fn test_full_bridge_workflow() {
    // Setup
    let config = BridgeConfig::default();
    let bridge = Bridge::new(config).await.unwrap();

    // Execute
    let result = bridge.call_tool("test_tool", json!({})).await.unwrap();

    // Verify
    assert!(result.is_object());

    // Cleanup
    bridge.shutdown().await.unwrap();
}
```

### Test Utilities

```rust
// tests/common/mod.rs

use std::sync::Once;

static INIT: Once = Once::new();

pub fn setup() {
    INIT.call_once(|| {
        tracing_subscriber::fmt()
            .with_test_writer()
            .init();
    });
}

pub fn test_config() -> Config {
    Config {
        max_memory: 1024,
        timeout: Duration::from_secs(5),
        ..Default::default()
    }
}
```

---

## Performance Optimization

### Profiling First

**CRITICAL**: Always profile before optimizing.

```bash
# Generate flamegraph
cargo install flamegraph
cargo flamegraph --bin mcp-cli

# Run benchmarks
cargo bench
```

### Pre-allocate Capacity

```rust
// ✅ GOOD: Single allocation
let mut vec = Vec::with_capacity(1000);
for i in 0..1000 {
    vec.push(i);
}

// ❌ BAD: Multiple reallocations
let mut vec = Vec::new();
for i in 0..1000 {
    vec.push(i);
}
```

### Avoid Cloning in Hot Paths

```rust
// ❌ BAD: Clone in loop
for item in items.iter() {
    let owned = item.clone();
    process(owned);
}

// ✅ GOOD: Use references
for item in items.iter() {
    process(item);
}

// ✅ GOOD: Consume if ownership needed
for item in items {
    process(item);
}
```

### Benchmarking with Criterion

```rust
// benches/my_benchmark.rs

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use mcp_core::parse_tool;

fn benchmark_parse_tool(c: &mut Criterion) {
    let json = include_str!("../fixtures/tool.json");

    c.bench_function("parse_tool", |b| {
        b.iter(|| parse_tool(black_box(json)))
    });
}

criterion_group!(benches, benchmark_parse_tool);
criterion_main!(benches);
```

---

## Security Requirements

### Input Validation

**Always validate external input**:

```rust
use validator::Validate;

#[derive(Validate)]
pub struct ToolInput {
    #[validate(length(min = 1, max = 256))]
    name: String,

    #[validate(range(min = 0, max = 1000000))]
    timeout_ms: u64,
}

pub fn process_tool_input(input: ToolInput) -> Result<Tool> {
    input.validate()
        .map_err(|e| BridgeError::ValidationFailed(e.to_string()))?;

    Ok(Tool::new(input.name, Duration::from_millis(input.timeout_ms)))
}
```

### Path Traversal Prevention

```rust
// ✅ SAFE: Validate and sanitize path
pub fn read_skill_file(filename: &str) -> Result<String> {
    // Remove any path components
    let filename = Path::new(filename)
        .file_name()
        .ok_or_else(|| VfsError::InvalidPath)?;

    // Build safe path
    let base_dir = Path::new("/var/mcp-skills");
    let path = base_dir.join(filename);

    // Ensure path is within base directory
    let canonical = path.canonicalize()?;
    if !canonical.starts_with(base_dir) {
        return Err(VfsError::PathTraversal);
    }

    std::fs::read_to_string(canonical).map_err(Into::into)
}
```

### Secrets Management

```rust
// ❌ NEVER hardcode secrets
const API_KEY: &str = "sk-1234567890abcdef"; // NEVER!

// ✅ GOOD: Load from environment
use std::env;

pub struct Config {
    api_key: secrecy::Secret<String>,
}

impl Config {
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            api_key: secrecy::Secret::new(
                env::var("MCP_API_KEY")
                    .context("MCP_API_KEY environment variable not set")?
            ),
        })
    }
}
```

### Unsafe Code

**CRITICAL**: All `unsafe` blocks require `SAFETY` comments.

```rust
/// Converts bytes to string without validation.
///
/// # Safety
///
/// Caller must ensure `bytes` contains valid UTF-8.
/// Passing invalid UTF-8 results in undefined behavior.
pub unsafe fn bytes_to_str_unchecked(bytes: &[u8]) -> &str {
    // SAFETY: Caller guarantees bytes are valid UTF-8
    std::str::from_utf8_unchecked(bytes)
}
```

---

## Documentation Standards

### Every Public Item Needs Doc Comments

```rust
/// Represents a tool in the MCP system.
///
/// Tools are executable units that can be invoked through the MCP bridge.
/// Each tool has a unique name and schema defining its input parameters.
///
/// # Examples
///
/// ```
/// use mcp_core::Tool;
///
/// let tool = Tool::new("send_message", schema)?;
/// assert_eq!(tool.name(), "send_message");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Clone)]
pub struct Tool {
    name: String,
    schema: Schema,
}

impl Tool {
    /// Creates a new tool with the given name and schema.
    ///
    /// # Errors
    ///
    /// Returns an error if the schema is invalid or the name is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// # use mcp_core::{Tool, Schema};
    /// let schema = Schema::default();
    /// let tool = Tool::new("test_tool", schema)?;
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn new(name: String, schema: Schema) -> Result<Self> {
        if name.is_empty() {
            return Err(Error::EmptyToolName);
        }
        Ok(Self { name, schema })
    }

    /// Returns the tool's name.
    pub fn name(&self) -> &str {
        &self.name
    }
}
```

### Documentation Sections

- **Summary** (first line) - What it does
- **Description** - More details
- `# Examples` - Working code examples (tested!)
- `# Errors` - When/why it returns errors
- `# Panics` - When/why it panics (avoid this!)
- `# Safety` - For unsafe functions (required!)

---

## CI/CD Integration

### Pre-commit Checks

**Run before committing**:

```bash
# Format code (nightly for unstable features)
cargo +nightly fmt --workspace

# Lint with clippy
cargo clippy --workspace -- -D warnings

# Run tests with nextest
cargo nextest run --workspace

# Run doc tests
cargo test --doc --workspace

# Security audit
cargo deny check
```

### CI Workflow

The project uses comprehensive GitHub Actions CI:

- **Quick checks**: Format, clippy, doc tests (fail fast)
- **Security**: cargo-deny for vulnerabilities, licenses, bans
- **Cross-platform tests**: Linux, macOS, Windows with cargo-nextest
- **Code coverage**: cargo-llvm-cov + codecov
- **MSRV check**: Rust 1.89
- **Benchmarks**: criterion (on main branch only)

### Dependency Management

```toml
# ✅ GOOD: Specific features only
tokio = { version = "1.48", features = ["rt", "net", "time", "macros"] }

# ❌ BAD: Include everything
tokio = { version = "1.48", features = ["full"] }
```

---

## Code Review Checklist

### Before Requesting Review

- [ ] Code formatted: `cargo +nightly fmt --workspace`
- [ ] No clippy warnings: `cargo clippy --workspace -- -D warnings`
- [ ] Tests pass: `cargo nextest run --workspace`
- [ ] Added tests for new functionality
- [ ] Added tests for edge cases
- [ ] Updated documentation
- [ ] No debug code (`println!`, `dbg!`)
- [ ] Commit messages clear

### Logic Verification

**Questions to ask**:

- [ ] Does the code actually solve the stated problem?
- [ ] Are all edge cases handled correctly?
- [ ] Is the algorithm correct?
- [ ] Are boundary conditions checked?
- [ ] Is the logic flow easy to follow?
- [ ] Are state transitions valid?
- [ ] Are assumptions documented and verified?

### Security Checks

- [ ] All `unsafe` blocks have `SAFETY` comments
- [ ] Input validation on all external inputs
- [ ] No hardcoded secrets in code
- [ ] SQL queries use parameters (if applicable)
- [ ] Path traversal prevention
- [ ] Errors don't leak sensitive information

---

## Common Patterns

### Result Transformation

```rust
// Convert Option to Result
let result: Result<Tool, _> = maybe_tool
    .ok_or_else(|| Error::ToolNotFound)?;

// Add context to errors
bridge.call_tool(name, args)
    .context("failed to call MCP tool")?;

// Map errors
io_result.map_err(|e| BridgeError::IoError(e))?;
```

### Collection Patterns

```rust
// Collect with error handling
let results: Result<Vec<_>, _> = items
    .iter()
    .map(|item| process(item))
    .collect();

// Filter-map combo
let valid: Vec<_> = items
    .iter()
    .filter_map(|item| validate(item).ok())
    .collect();
```

### Workspace Dependencies

**Use workspace dependencies**:

```toml
# In crate's Cargo.toml
[dependencies]
mcp-core = { workspace = true }
tokio = { workspace = true }
```

---

## Anti-Patterns to Avoid

**NEVER do these**:

- ❌ Using `.unwrap()` in library code without comment explaining why safe
- ❌ Using `.expect()` everywhere instead of proper error handling
- ❌ Cloning data unnecessarily
- ❌ Ignoring compiler warnings
- ❌ Skipping tests because "it's simple"
- ❌ Public APIs without documentation
- ❌ Functions longer than 50 lines
- ❌ Deep nesting (>3 levels) - extract functions
- ❌ Blocking operations in async functions
- ❌ Hardcoded secrets
- ❌ Implementing custom MCP protocol (use rmcp)

---

## Tools Quick Reference

```bash
# Development
cargo check                          # Fast compilation check
cargo +nightly fmt --workspace      # Format code
cargo clippy --workspace -- -D warnings  # Lint

# Testing
cargo nextest run --workspace       # Run tests (faster)
cargo test --doc --workspace        # Run doc tests
cargo bench                         # Run benchmarks

# Security
cargo deny check                    # Check advisories, licenses, bans

# Profiling
cargo flamegraph --bin mcp-cli     # CPU profiling
cargo build --timings              # Build time analysis

# Dependencies
cargo tree                         # View dependency tree
cargo outdated                     # Check for updates
```

---

## Additional Resources

- **Microsoft Rust Guidelines**: https://microsoft.github.io/rust-guidelines/agents/all.txt
- **rmcp Documentation**: https://docs.rs/rmcp/0.9
- **MCP Specification**: https://github.com/modelcontextprotocol/specification
- **Project Documentation**: `.local/` directory (not in git)
- **Architecture Decisions**: `docs/adr/`

---

## Summary

When generating code for this project:

1. **Follow Microsoft Rust Guidelines strictly**
2. **Use rmcp v0.9 for all MCP communication**
3. **Use thiserror for libraries, anyhow only for CLI**
4. **Write tests for all public functions (unit tests in same file)**
5. **Document all public items with examples**
6. **Validate all external input**
7. **No hardcoded secrets**
8. **Profile before optimizing**
9. **Use cargo-nextest for testing**
10. **Run cargo +nightly fmt before committing**

The spirit of these guidelines is more important than the letter - focus on writing **safe, idiomatic, well-tested, and documented Rust code** that follows industry best practices and project-specific requirements.
