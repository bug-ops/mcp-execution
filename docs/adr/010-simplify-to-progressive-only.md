# ADR-010: Simplify to Progressive Loading Only

## Status

**Accepted** (2025-01-25)

Supersedes: ADR-005, ADR-006 (partially)

## Context

The original MCP Code Execution architecture included three major components:

1. **WASM Runtime** (`mcp-wasm-runtime`) - WebAssembly sandbox for secure code execution
2. **Skills Categorization** (`mcp-skill-generator`, `mcp-skill-store`) - LLM-based and dictionary-based tool categorization
3. **Progressive Loading** (`mcp-codegen` progressive feature) - One TypeScript file per tool

### Initial Goals

- **Primary**: Achieve 90-98% token savings vs loading all MCP tools
- **Secondary**: Provide secure execution environment
- **Tertiary**: Organize tools into meaningful categories

### Implementation Reality

After completing Phases 1-5 (production-ready implementation with 1,035 passing tests), analysis revealed:

| Component | Lines of Code | Complexity | Value Delivered |
|-----------|---------------|------------|-----------------|
| **Progressive Loading** | ~2,000 | Low | **98% token savings** ✅ |
| **WASM Runtime** | ~15,000 | High | ~50ms execution overhead ⚠️ |
| **Skills Categorization** | ~19,000 | Very High | Unclear user benefit ❓ |

**Key Finding**: Progressive loading alone achieves the primary goal (98% token savings) with minimal complexity.

### Problems with Over-Engineering

1. **WASM Runtime**:
   - Added 50ms overhead per tool call
   - Required complex sandbox setup (memory limits, WASI preopens, host functions)
   - Security benefits unclear for TypeScript code generation use case
   - 15,000+ lines of Wasmtime integration code

2. **Skills Categorization**:
   - LLM-based categorization required external API (Anthropic)
   - Dictionary-based categorization needed manual maintenance
   - User value proposition unclear (does categorizing tools help?)
   - 19,000+ lines of orchestration and generation code

3. **Maintenance Burden**:
   - 10 crates to maintain (now reduced to 6)
   - 36,000+ lines of non-essential code
   - Complex feature flag combinations
   - Integration test explosion (WASM × Skills × Progressive)

### Performance Analysis

From Phase 5 benchmarks:

```
Progressive Loading Performance:
- File generation: 2ms (526x faster than 1-second target)
- VFS export: 1.2ms average
- Token savings: 98% (30,000 → 500-1,500 tokens per tool)

WASM Runtime Performance:
- Module compilation: 45ms
- Module caching: 2-4ms (with Blake3 verification)
- Execution overhead: ~50ms per call
- Total: ~50ms overhead vs direct MCP call

Comparison:
- Progressive loading: 98% token savings, ~3ms overhead
- WASM runtime: 98% token savings, ~50ms overhead
- **Conclusion**: WASM adds 16x latency for same token savings
```

## Decision

**Remove WASM runtime and skills categorization. Focus exclusively on progressive loading.**

### What Gets Removed

1. **Crates** (4 deleted):
   - `mcp-wasm-runtime` - Wasmtime sandbox
   - `mcp-skill-generator` - IDE skill generation
   - `mcp-skill-store` - Skill persistence
   - `mcp-examples` - Example workflows

2. **Code** (36,209 lines deleted, 346 added):
   - WASM code generation (templates, tests, benchmarks)
   - Skills categorization (LLM, dictionary, orchestrator)
   - WASM-specific CLI commands (execute, debug)
   - Skills-specific CLI commands (skill management)

3. **Features** (simplified):
   - Remove `wasm` and `skills` feature flags from `mcp-codegen`
   - Make progressive loading the only generation mode

### What Gets Kept

1. **Core Functionality**:
   - Progressive loading code generation
   - MCP server introspection (`mcp-introspector`)
   - MCP proxy with caching (`mcp-bridge`)
   - Virtual filesystem (`mcp-files`)
   - CLI (`mcp-cli` with `generate` command)

2. **Directory Structure**:
   - Keep `~/.claude/skills/` for future instruction-style skills
   - Keep `~/.claude/servers/` for progressive loading output
   - Keep `~/.mcp-execution/cache/` for VFS and metadata caching

3. **Quality**:
   - All 684 tests passing
   - Zero clippy warnings
   - Full documentation coverage

## Consequences

### Positive

1. **Simplified Architecture**:
   - 6 crates instead of 10 (40% reduction)
   - 36,000 fewer lines of code to maintain
   - No complex feature flag combinations
   - Clearer project focus

2. **Maintained Core Value**:
   - 98% token savings still achieved
   - Progressive loading pattern fully functional
   - TypeScript code generation working
   - MCP server integration via rmcp SDK

3. **Easier Onboarding**:
   - Single clear purpose: generate TypeScript for MCP tools
   - No WASM knowledge required
   - No LLM API keys needed
   - Simpler documentation

4. **Faster Development**:
   - Less code to review
   - Faster compile times
   - Simpler test matrix
   - Clearer bug isolation

### Negative

1. **No Executable Skills**:
   - Cannot execute WASM-compiled tools
   - No sandbox security for generated code
   - Planned feature removed before user adoption

2. **No Automatic Categorization**:
   - Cannot auto-categorize tools using LLM
   - Cannot use dictionary-based categories
   - Manual organization required

3. **`callMCPTool()` Stub**:
   - Runtime bridge function not implemented
   - Generated TypeScript shows structure but can't execute
   - Requires Phase 2.3 (mcp-cli bridge) for full functionality

### Neutral

1. **Documentation Debt**:
   - Must update all docs to remove WASM/skills mentions
   - Must create new ADR explaining decision
   - Must mark old ADRs as superseded

2. **User Migration**:
   - No users yet (pre-1.0)
   - No migration path needed
   - Clean break before public release

## Implementation

### Commit

```
commit 5cda3dd
Author: Assistant
Date:   2025-01-25

    refactor: simplify to progressive loading only, remove WASM and skills

    Remove over-engineered WASM sandbox and skills categorization features
    to focus exclusively on progressive loading code generation. This
    reduces complexity while maintaining core functionality.

    Changes:
    - Remove 4 crates: mcp-wasm-runtime, mcp-skill-generator,
      mcp-skill-store, mcp-examples
    - Remove WASM code generation (templates, tests, benchmarks)
    - Remove skills categorization (LLM, dictionary, categories)
    - Simplify mcp-codegen to progressive-only (no feature flags)
    - Update CLI commands to remove Execute, Debug, Skill variants
    - Update Stats command to show only MCP Bridge statistics
    - Update Generate command to output only progressive TypeScript
    - Fix all tests and documentation

    All quality checks pass:
    - cargo fmt: ✓
    - cargo clippy --workspace -- -D warnings: ✓
    - cargo nextest run --workspace: ✓ (684 tests)
    - cargo doc --workspace --no-deps: ✓

    110 files changed, 346 insertions(+), 36209 deletions(-)
```

### Documentation Updates

Following this ADR, documentation must be updated:

1. **Mark Superseded**:
   - ADR-005 (Skills generation)
   - ADR-006 (Skill persistence)

2. **Update Main Docs**:
   - `CLAUDE.md` - Remove WASM/skills, update workspace structure
   - `README.md` - Update project description
   - `docs/ARCHITECTURE.md` - Rewrite for progressive-only

3. **Create Instruction Skill**:
   - `~/.claude/skills/mcp-progressive-loading/SKILL.md` - How to use progressive loading

4. **Update Crate Docs**:
   - `crates/mcp-codegen/README.md` - Remove feature flags
   - `crates/mcp-cli/README.md` - Update command list

## Alternatives Considered

### Alternative 1: Keep WASM, Remove Skills

**Pros**:
- Maintains executable skills capability
- Sandbox security preserved

**Cons**:
- Still 15,000+ lines to maintain
- 50ms overhead for same token savings
- Limited user value (no execution needed for TypeScript generation)

**Rejected**: WASM overhead (50ms) vs value (execution we don't need) doesn't justify complexity.

### Alternative 2: Keep Skills, Remove WASM

**Pros**:
- Automatic tool categorization
- Better organization

**Cons**:
- 19,000+ lines to maintain
- Unclear user value
- Requires LLM API or manual dictionary maintenance

**Rejected**: User value of categorization unclear. Users can organize manually if needed.

### Alternative 3: Keep Both, Add Feature Flags

**Pros**:
- Maximum flexibility
- Users choose what they need

**Cons**:
- Test matrix explosion (2³ = 8 combinations)
- Complex documentation
- Maintenance burden

**Rejected**: Flexibility without clear use case adds complexity without value.

## Future Work

### Phase 2.3: MCP Bridge Command

To make generated TypeScript fully functional:

1. Implement `mcp-cli bridge call <server-id> <tool-name> <params>` command
2. Update `runtime-bridge.ts` template to call bridge command
3. Add integration tests with real MCP servers

### Possible Future Extensions

If user demand emerges:

1. **Executable Skills** (WASM-based):
   - Could resurrect mcp-wasm-runtime if security becomes critical
   - Decision would require clear security threat model

2. **Tool Categorization**:
   - Could add simple directory-based organization
   - Decision would require user research showing value

3. **Alternative Runtimes**:
   - Deno/Bun support for TypeScript execution
   - Would need performance comparison

## Review

**Reviewers**: Self-review (architectural decision)

**Approval Date**: 2025-01-25

**Next Review**: When user feedback indicates need for removed features
