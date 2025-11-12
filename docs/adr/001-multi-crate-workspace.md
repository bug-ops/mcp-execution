# ADR-001: Multi-Crate Workspace Architecture

## Status

Accepted

## Context

The MCP Code Execution project requires a modular architecture that supports:

- Fast incremental compilation during development
- Clear separation of concerns between components
- Ability to publish individual crates independently
- Testability and mockability of each component
- Prevention of circular dependencies

The project consists of 5 major components (Introspector, Code Generator, WASM Runtime, Bridge, VFS) plus shared core types and a CLI application.

## Decision

We will use a **multi-crate workspace** with 8 separate crates:

1. **mcp-core** - Core types, traits, and errors (foundation)
2. **mcp-protocol** - MCP protocol implementation
3. **mcp-introspector** - Server analysis and discovery
4. **mcp-codegen** - Code generation from schemas
5. **mcp-bridge** - MCP proxy with caching and pooling
6. **mcp-vfs** - Virtual filesystem
7. **mcp-wasm-runtime** - WASM sandbox execution
8. **mcp-cli** - CLI application binary

Dependency graph ensures no circular dependencies:

```text
mcp-cli → mcp-wasm-runtime → {mcp-bridge, mcp-vfs, mcp-codegen} → mcp-protocol → mcp-core
                           └→ mcp-introspector → mcp-protocol
```

## Rationale

**Advantages:**

1. **Faster compilation**: Each crate compiles independently, enabling parallel builds
2. **Clear boundaries**: Explicit dependencies prevent coupling
3. **Reusability**: Individual crates can be published to crates.io
4. **Testability**: Each crate has isolated test suite
5. **Microsoft Guidelines compliance**: "Err toward too many crates rather than too few"
6. **Team scalability**: Different developers can work on different crates

**Compared to alternatives:**

- **Monolithic single crate**: Would have slow compilation, tight coupling, no clear boundaries
- **Feature flags for components**: Would violate additive feature requirement, increase complexity

## Consequences

**Positive:**

- Fast incremental builds (only changed crates recompile)
- Clear API boundaries enforced by compiler
- Easy to add new crates for future features
- Can version and publish crates independently

**Negative:**

- More Cargo.toml files to maintain (8 total)
- Workspace-level dependency management required
- Slightly more boilerplate for re-exports
- Need to manage internal API stability

**Mitigation:**

- Use workspace dependencies to centralize version management
- Use workspace-level lints for consistency
- Document public API contracts clearly
- Use semantic versioning for internal crates

## Alternatives Considered

### Alternative 1: Single Crate with Modules

**Rejected because:**

- Slow compilation for large projects (100k+ lines)
- No enforcement of module boundaries
- Cannot publish components independently
- Violates Microsoft Rust Guidelines

### Alternative 2: Binary Crate + Single Library

**Rejected because:**

- Still has slow compilation issues
- Doesn't separate concerns adequately
- Difficult to test components in isolation

## References

- Microsoft Rust Guidelines: "Crate Splitting Philosophy"
- Tokio workspace structure (industry example)
- Existing .local/mcp-code-execution-architecture.md specifications
