# Contributing to MCP Code Execution

Thank you for your interest in contributing to MCP Code Execution! This document provides guidelines and instructions for contributing to the project.

## Table of Contents

- [Code of Conduct](#code-of-conduct)
- [Getting Started](#getting-started)
  - [Prerequisites](#prerequisites)
  - [Development Setup](#development-setup)
- [Development Workflow](#development-workflow)
  - [Code Style](#code-style)
  - [Testing Requirements](#testing-requirements)
  - [Documentation Requirements](#documentation-requirements)
- [Submitting Changes](#submitting-changes)
  - [Commit Messages](#commit-messages)
  - [Pull Request Process](#pull-request-process)
- [Running Benchmarks](#running-benchmarks)
- [Project Architecture](#project-architecture)
- [Getting Help](#getting-help)

---

## Code of Conduct

This project adheres to a code of conduct that we expect all contributors to follow. Please be respectful and constructive in all interactions.

---

## Getting Started

### Prerequisites

**Required:**
- Rust 1.89 or higher (Edition 2024)
- Cargo (comes with Rust)
- Node.js 18+ (for testing generated TypeScript code)
- Git

**Recommended:**
- Rust nightly toolchain (for formatting)
- cargo-nextest (faster test execution)
- cargo-llvm-cov (code coverage)

### Development Setup

1. **Clone the repository:**

```bash
git clone https://github.com/bug-ops/mcp-execution.git
cd mcp-execution
```

2. **Install required Rust toolchains:**

```bash
# Install stable toolchain (default)
rustup install stable

# Install nightly toolchain (for rustfmt)
rustup install nightly
```

3. **Install development tools:**

```bash
# Fast test runner (highly recommended)
cargo install cargo-nextest

# Code coverage tool
cargo install cargo-llvm-cov

# Optional: Security audit tools
cargo install cargo-deny cargo-audit

# Optional: Performance profiling
cargo install cargo-flamegraph
```

4. **Verify installation:**

```bash
# Run all tests
cargo nextest run --workspace

# Run doc tests
cargo test --doc --workspace

# Check code formatting
cargo +nightly fmt --all -- --check

# Run clippy lints
cargo clippy --all-targets --all-features --workspace -- -D warnings
```

If all commands pass, you're ready to contribute!

---

## Development Workflow

### Code Style

**This project strictly follows the [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt).**

Key requirements:

1. **Strong Types Over Primitives**
   - Use `ServerId`, `ToolName`, `SessionId` instead of raw `String`
   - Create newtype wrappers for domain-specific values
   - Example: `pub struct ToolName(String);`

2. **Error Handling**
   - Use `thiserror` for library crates (mcp-execution-core, mcp-execution-codegen, etc.)
   - Use `anyhow` ONLY in CLI crates (mcp-cli)
   - All errors must implement `std::error::Error`

3. **Public Types Must Be:**
   - `Send + Sync` (thread-safe)
   - `Debug` (implement or derive Debug)
   - Well-documented with examples

4. **Safety**
   - Zero `unsafe` blocks (current status: 0 unsafe)
   - Any `unsafe` code requires detailed documentation and justification

5. **Formatting**
   - Use nightly rustfmt: `cargo +nightly fmt --all`
   - Configuration in `rustfmt.toml`

6. **Lints**
   - Workspace-level lints in `Cargo.toml`
   - Clippy pedantic level enabled
   - All warnings treated as errors in CI

**Before committing:**

```bash
# Format code with nightly rustfmt
cargo +nightly fmt --all

# Run clippy with pedantic lints
cargo clippy --all-targets --all-features --workspace -- -D warnings

# Ensure no warnings in documentation
cargo doc --no-deps --all-features --workspace
```

### Testing Requirements

**All contributions must include tests.**

#### Unit Tests

- Place tests in the same file as the code: `#[cfg(test)] mod tests { ... }`
- Test all public functions
- Test error cases explicitly
- Use descriptive test names: `test_tool_generation_with_optional_params`

Example:
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_server_id_from_string() {
        let id = ServerId::from("github");
        assert_eq!(id.as_str(), "github");
    }

    #[test]
    fn test_invalid_tool_name_returns_error() {
        let result = ToolName::new("");
        assert!(result.is_err());
    }
}
```

#### Integration Tests

- Place in workspace-level `tests/` directory
- Test end-to-end workflows
- Use `tempfile` for filesystem operations
- Clean up resources in tests

#### Running Tests

```bash
# Run all tests with nextest (fast, recommended)
cargo nextest run --workspace

# Run doc tests (nextest doesn't support these)
cargo test --doc --workspace

# Run specific test
cargo nextest run --test test_name

# Run tests with logging output
cargo nextest run --workspace -- --nocapture

# Run tests for specific crate
cargo nextest run -p mcp-execution-codegen
```

#### Code Coverage

```bash
# Generate coverage report
cargo llvm-cov --all-features --workspace --lcov \
  --output-path lcov.info nextest

# View HTML report
cargo llvm-cov --all-features --workspace --html nextest
open target/llvm-cov/html/index.html
```

**Coverage Requirements:**
- Aim for 70%+ overall coverage
- New code should have 80%+ coverage
- Critical paths must have 100% coverage

### Documentation Requirements

**Every public item must be documented.**

#### Documentation Standards

1. **Module-level documentation:**
```rust
//! Brief module description.
//!
//! Detailed explanation of what this module provides,
//! when to use it, and key concepts.
//!
//! # Examples
//!
//! ```
//! use mcp_execution_codegen::Generator;
//!
//! let gen = Generator::new();
//! ```
```

2. **Function documentation:**
```rust
/// Generates TypeScript code for an MCP tool.
///
/// Creates a standalone TypeScript file that can be executed
/// directly via Node.js CLI.
///
/// # Arguments
///
/// * `tool` - The MCP tool definition
/// * `options` - Code generation options
///
/// # Returns
///
/// Returns the generated TypeScript code as a `String`.
///
/// # Errors
///
/// Returns an error if:
/// - Tool schema is invalid
/// - Template rendering fails
///
/// # Examples
///
/// ```
/// use mcp_execution_codegen::{Generator, Tool};
///
/// let tool = Tool::new("my-tool");
/// let code = Generator::generate(&tool)?;
/// ```
pub fn generate(tool: &Tool, options: &Options) -> Result<String> {
    // ...
}
```

3. **Type documentation:**
```rust
/// Unique identifier for an MCP server.
///
/// Server IDs must be valid directory names and are used
/// to organize generated code files.
///
/// # Examples
///
/// ```
/// use mcp_execution_core::ServerId;
///
/// let id = ServerId::from("github");
/// assert_eq!(id.as_str(), "github");
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerId(String);
```

#### Checking Documentation

```bash
# Build documentation (warnings as errors)
RUSTDOCFLAGS="-D warnings" cargo doc --no-deps --all-features --workspace

# Open documentation in browser
cargo doc --open

# Check for missing documentation
cargo rustdoc -- -D missing_docs
```

---

## Submitting Changes

### Commit Messages

We use [Conventional Commits](https://www.conventionalcommits.org/) format:

```
<type>(<scope>): <subject>

<body>

<footer>
```

**Types:**
- `feat`: New feature
- `fix`: Bug fix
- `docs`: Documentation changes
- `style`: Code style changes (formatting, etc.)
- `refactor`: Code refactoring
- `perf`: Performance improvements
- `test`: Adding or updating tests
- `chore`: Maintenance tasks
- `ci`: CI/CD changes

**Scopes:**
- `core`: mcp-execution-core crate
- `codegen`: mcp-execution-codegen crate
- `introspector`: mcp-execution-introspector crate
- `files`: mcp-execution-files crate
- `server`: mcp-execution-server crate
- `cli`: mcp-cli crate
- `deps`: Dependency updates
- `docs`: Documentation
- `ci`: CI/CD workflows

**Examples:**

```
feat(codegen): add support for TypeScript generics

Implements generic type generation for tools with
parameterized schemas.

Closes #123
```

```
fix(cli): correct error handling in generate command

Previously, the command would panic on invalid input.
Now returns a proper error message.

Fixes #456
```

```
docs(README): update installation instructions

Added Node.js requirement and clarified MSRV.
```

### Pull Request Process

1. **Create a feature branch:**

```bash
git checkout -b feature/my-feature
# or
git checkout -b fix/issue-123
```

2. **Make your changes:**
   - Write code following style guidelines
   - Add tests for new functionality
   - Update documentation
   - Run local checks (format, clippy, tests)

3. **Before submitting:**

```bash
# Format code
cargo +nightly fmt --all

# Run all quality checks
cargo clippy --all-targets --all-features --workspace -- -D warnings
cargo nextest run --workspace
cargo test --doc --workspace
cargo doc --no-deps --all-features --workspace
```

4. **Create pull request:**
   - Fill out the PR template completely
   - Reference related issues
   - Describe changes clearly
   - List any breaking changes

5. **PR Review Checklist:**

- [ ] Code follows Microsoft Rust Guidelines
- [ ] All tests pass locally
- [ ] Code coverage is adequate (70%+)
- [ ] Documentation is complete and accurate
- [ ] Clippy lints pass with no warnings
- [ ] Code is formatted with nightly rustfmt
- [ ] Commit messages follow Conventional Commits
- [ ] No breaking changes (or clearly documented)
- [ ] Performance impact considered
- [ ] Security implications reviewed

6. **CI Checks:**

All PRs must pass:
- Formatting check (nightly rustfmt)
- Clippy lints (all targets, all features)
- Tests on Linux, macOS, Windows
- Tests on stable and beta Rust
- Code coverage upload to codecov
- MSRV check (Rust 1.89)
- Security audit (cargo-deny)
- Documentation build
- Benchmark build

7. **Review Process:**
   - Maintainers will review your PR
   - Address feedback promptly
   - Keep PR focused and small (easier to review)
   - Be patient and respectful

8. **After Approval:**
   - Maintainer will merge using squash merge
   - Delete your feature branch

---

## Running Benchmarks

The project includes Criterion benchmarks for performance-critical code.

### Running Benchmarks Locally

```bash
# Run all benchmarks
cargo bench --workspace

# Run specific benchmark
cargo bench --bench codegen_benchmarks

# Run with profiling (for flamegraphs)
cargo bench --bench codegen_benchmarks -- --profile-time=5

# Build benchmarks only (fast, for CI)
cargo bench --no-run --profile bench-fast --workspace
```

### Benchmark Guidelines

1. **When to Add Benchmarks:**
   - Code generation functions
   - Template rendering
   - File system operations
   - JSON parsing/serialization
   - Any code in hot paths

2. **Benchmark Structure:**

```rust
use criterion::{black_box, criterion_group, criterion_main, Criterion};

fn benchmark_tool_generation(c: &mut Criterion) {
    c.bench_function("generate_10_tools", |b| {
        b.iter(|| {
            // Setup
            let generator = Generator::new();
            let tools = create_test_tools(10);

            // Benchmark (use black_box to prevent optimization)
            black_box(generator.generate_all(&tools))
        });
    });
}

criterion_group!(benches, benchmark_tool_generation);
criterion_main!(benches);
```

3. **Performance Targets:**
   - Code generation: <100ms for 10 tools
   - VFS export: <10ms per operation
   - Keep benchmarks fast (<30 seconds total)

---

## Project Architecture

### Workspace Structure

```
mcp-execution/
├── crates/
│   ├── mcp-execution-core/          # Foundation: types, traits, errors
│   ├── mcp-execution-introspector/  # MCP server analysis (uses rmcp SDK)
│   ├── mcp-execution-codegen/       # TypeScript code generation
│   ├── mcp-execution-files/         # Virtual filesystem
│   ├── mcp-execution-server/        # MCP server for generation
│   └── mcp-cli/           # CLI application
├── examples/              # Usage examples
├── docs/adr/              # Architecture Decision Records
└── tests/                 # Integration tests
```

### Dependency Graph

**No circular dependencies allowed:**

```
mcp-cli → {mcp-execution-server, mcp-execution-codegen, mcp-execution-introspector, mcp-execution-files, mcp-execution-core}
mcp-execution-server → {mcp-execution-codegen, mcp-execution-introspector, mcp-execution-files, mcp-execution-core}
mcp-execution-codegen → {mcp-execution-files, mcp-execution-core}
mcp-execution-introspector → {rmcp, mcp-execution-core}
mcp-execution-files → mcp-execution-core
```

### Key Technologies

- **rmcp 0.10+**: Official Rust MCP SDK (critical dependency)
- **Tokio**: Async runtime
- **Handlebars**: Template engine for code generation
- **Criterion**: Benchmarking framework
- **cargo-nextest**: Fast test runner

### Architecture Decision Records

Before making architectural changes, check existing ADRs in `docs/adr/`:

- ADR-004: Use rmcp official SDK
- ADR-010: Simplify to progressive loading only
- (See `docs/adr/` for complete list)

If proposing significant architectural changes, create a new ADR.

---

## Getting Help

- **Documentation**: Start with [README.md](README.md)
- **Examples**: Check [examples/](examples/) directory
- **Issues**: Search existing issues or create a new one
- **Discussions**: Use GitHub Discussions for questions
- **Security**: Report security issues privately (see SECURITY.md)

### Useful Resources

- [Microsoft Rust Guidelines](https://microsoft.github.io/rust-guidelines/agents/all.txt)
- [MCP Specification](https://spec.modelcontextprotocol.io/)
- [rmcp Documentation](https://docs.rs/rmcp)
- [Criterion Benchmarking](https://bheisler.github.io/criterion.rs/book/)
- [Conventional Commits](https://www.conventionalcommits.org/)

---

## Thank You!

Your contributions make this project better. We appreciate your time and effort! 🎉
