# GitHub Copilot Path-Specific Instructions

This directory contains path-specific instructions for GitHub Copilot using the official format.

## Structure

```text
.github/
├── copilot-instructions.md           # Global instructions (all files)
└── instructions/                     # Path-specific instructions
    ├── mcp-execution-core.instructions.md     # crates/mcp-execution-core/**/*.rs
    ├── mcp-cli.instructions.md      # crates/mcp-cli/**/*.rs
    ├── mcp-bridge.instructions.md   # crates/mcp-bridge/**/*.rs
    ├── mcp-execution-introspector.instructions.md  # crates/mcp-execution-introspector/**/*.rs
    ├── mcp-execution-codegen.instructions.md  # crates/mcp-execution-codegen/**/*.rs
    └── mcp-wasm-runtime.instructions.md  # crates/mcp-wasm-runtime/**/*.rs
```

## Format Requirements

All path-specific instruction files follow the official GitHub format:

1. **Filename**: `{name}.instructions.md`
2. **YAML Frontmatter**: Required with `applyTo` glob pattern
3. **Content**: Markdown with specific coding guidelines

### Example Format

```markdown
---
applyTo: "crates/mcp-execution-core/**/*.rs"
---

# Copilot Instructions: mcp-execution-core

Instructions content here...
```

## How It Works

When you edit a file, GitHub Copilot automatically loads:

1. **Global instructions** from `copilot-instructions.md`
2. **Path-specific instructions** from any file whose `applyTo` pattern matches

### Example

Editing `crates/mcp-cli/src/main.rs`:

- **Loaded**: `copilot-instructions.md` (global)
- **Loaded**: `mcp-cli.instructions.md` (matches pattern `crates/mcp-cli/**/*.rs`)
- **Result**: Copilot knows to use `anyhow::Result` in this crate

## Instruction Priorities by Crate

| Crate | Error Handling | Key Requirements |
|-------|---------------|------------------|
| **mcp-execution-core** | `thiserror` only | Strong types, no business logic |
| **mcp-cli** | `anyhow` only | User-friendly errors, CLI patterns |
| **mcp-bridge** | `thiserror` | rmcp SDK, caching, thread-safe |
| **mcp-execution-introspector** | `thiserror` | rmcp ServiceExt, server discovery |
| **mcp-execution-codegen** | `thiserror` | Feature flags, Handlebars templates |
| **mcp-wasm-runtime** | `thiserror` + `anyhow` | Security-critical, resource limits |

## Maintenance

Update instructions when:

- New patterns emerge in a crate
- Error handling strategy changes
- SDK integration patterns evolve
- Security requirements change
- New dependencies are added

## References

- **Official Documentation**: <https://docs.github.com/en/copilot/tutorials/use-custom-instructions>
- **Project Guidelines**: `../CLAUDE.md`
- **Microsoft Rust Guidelines**: <https://microsoft.github.io/rust-guidelines/agents/all.txt>

## Verification

To verify instructions are working:

1. Open a file in a specific crate
2. Start typing code
3. Check Copilot suggestions match crate-specific patterns

For example, in `crates/mcp-execution-core/src/types.rs`:

```rust
pub struct ServerId(/* Copilot should suggest String or similar, not u64 */)
```

In `crates/mcp-cli/src/main.rs`:

```rust
fn main() -> Result<()> {
    // Copilot should suggest anyhow::Result, .context(), etc.
}
```
