# ADR-004: Use RMCP Official SDK Instead of Custom Protocol Implementation

## Status
**Accepted** (2025-11-12)

## Context

The initial architecture planned to implement a custom `mcp-protocol` crate to handle Model Context Protocol (MCP) communication. However, research revealed the existence of **rmcp** (v0.8.5) - the official Rust SDK for MCP maintained by the Model Context Protocol organization.

### Available Options

1. **Custom Implementation** (initially planned)
   - Full control over implementation
   - Requires deep protocol knowledge
   - Risk of incompatibility with MCP spec updates
   - Significant development time investment
   - Maintenance burden for protocol changes

2. **rmcp Official SDK** (github.com/modelcontextprotocol/rust-sdk)
   - Official implementation, spec-compliant
   - Version 0.8.5 (actively maintained)
   - Supports all transports: stdio, SSE, HTTP streaming
   - Provides `#[tool]` macro for declarative APIs
   - Both client and server support
   - Tokio-based async

3. **Alternative Libraries**
   - **mcpr** (v0.2.3): Community implementation, less mature
   - **mcp-client-rs**: Client-only, limited features

## Decision

**We will use rmcp v0.8 as a workspace dependency** instead of implementing custom protocol handling.

### Rationale

**Advantages:**
- ✅ **Spec Compliance**: Official SDK guarantees compatibility with MCP specification
- ✅ **Reduced Development Time**: Eliminates 1-2 weeks of protocol implementation work (Phase 2)
- ✅ **Maintenance**: Protocol updates handled by upstream maintainers
- ✅ **Quality**: Battle-tested implementation used by community
- ✅ **Features**: Full transport support (stdio/SSE/HTTP) out of box
- ✅ **Documentation**: Official docs and examples
- ✅ **Macros**: `#[tool]`, `#[tool_router]` reduce boilerplate
- ✅ **Community**: Support and ecosystem alignment

**Trade-offs:**
- ⚠️ External dependency (mitigated: official, actively maintained)
- ⚠️ Less control over internals (not needed for our use case)
- ⚠️ Documentation coverage 29.79% (acceptable: key APIs documented)

## Implementation

### Architecture Changes

**Before:**
```
mcp-cli → mcp-wasm-runtime → mcp-bridge → mcp-protocol → mcp-core
```

**After:**
```
mcp-cli → mcp-wasm-runtime → mcp-bridge → rmcp (external)
                                         ↘ mcp-core
```

### Workspace Structure

Remove `crates/mcp-protocol/` entirely. Add to `Cargo.toml`:

```toml
[workspace.dependencies]
# Official MCP Protocol Implementation (v0.8.5 as of Nov 2025)
rmcp = "0.8"
```

### Affected Crates

- **mcp-bridge**: Use `rmcp::client` for MCP server connections
- **mcp-introspector**: Use `rmcp::ServiceExt` for server introspection
- **mcp-codegen**: Use rmcp types for tool definitions
- **mcp-cli**: Use rmcp for direct MCP operations

### Code Example

```rust
use rmcp::client::TokioChildProcess;
use rmcp::ServiceExt;

// Connect to MCP server via stdio
let transport = TokioChildProcess::new("github-server")?;
let mut client = rmcp::client::Client::new(transport);

// Discover tools
let server_info = client.get_server_info().await?;
let tools = server_info.capabilities.tools;

// Call tool
let result = client.call_tool("send_message", params).await?;
```

## Consequences

### Positive

1. **Faster Development**: Skip Phase 2 (MCP Protocol implementation) entirely
2. **Better Quality**: Official implementation vs custom code
3. **Future-Proof**: Automatic spec compliance with updates
4. **Focus on Innovation**: Spend time on WASM sandbox and code generation (our unique value)
5. **Ecosystem**: Can leverage rmcp examples and community knowledge

### Negative

1. **Dependency**: Add external crate (acceptable risk for official SDK)
2. **API Learning Curve**: Team must learn rmcp APIs (minimal: well-documented)
3. **Version Updates**: Must track rmcp releases (manageable: semantic versioning)

### Neutral

1. **Documentation Gap**: 29.79% coverage means some APIs need code exploration (mitigated: examples exist)
2. **Abstraction Layer**: May still need thin wrapper in mcp-core for our domain types

## Implementation Timeline

- ✅ **Immediate**: Remove mcp-protocol from workspace (completed)
- ✅ **Immediate**: Add rmcp = "0.8" to Cargo.toml (completed)
- 🔄 **Week 1**: Update mcp-bridge to use rmcp client API
- 🔄 **Week 1**: Update mcp-introspector to use rmcp for discovery
- 🔄 **Week 2**: Update mcp-codegen to use rmcp types
- 🔄 **Week 2**: Integration testing with github server

## References

- **rmcp Documentation**: https://docs.rs/rmcp/0.8.5
- **GitHub Repository**: https://github.com/modelcontextprotocol/rust-sdk
- **MCP Specification**: https://github.com/modelcontextprotocol/specification
- **Tutorial**: https://www.shuttle.dev/blog/2025/07/18/how-to-build-a-stdio-mcp-server-in-rust

## Review

This ADR should be reviewed when:
- rmcp releases a major version (1.0.0)
- MCP spec introduces breaking changes
- Performance issues emerge with rmcp
- Project requirements change significantly

---

**Decision Date**: 2025-11-12
**Decided By**: Architecture Team
**Supersedes**: Original plan to implement custom mcp-protocol crate
