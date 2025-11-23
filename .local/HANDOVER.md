# Handover Document: Cache Separation Implementation

**From**: copilot (GitHub Copilot - Rust Project Architect)  
**To**: rust-developer agent  
**Date**: 2025-11-23  
**Issue**: Separate public skills from internal cache storage

---

## Executive Summary

This implementation separates MCP skill storage into two distinct directories:
- **Public Skills** (`~/.claude/skills/`): User-facing SKILL.md/REFERENCE.md files
- **Internal Cache** (`~/.mcp-execution/cache/`): System-managed WASM/VFS/metadata

**Status**: ✅ Architecture design complete, ready for implementation  
**Estimated Effort**: 2-3 days  
**Risk Level**: Low (backward compatible)

---

## 📋 What You Need to Know

### 1. Repository Structure

This is a **Rust workspace** with 10 crates:

```
crates/
├── mcp-core/           ← Add CacheManager here (Phase 1)
├── mcp-skill-store/    ← Update to use cache (Phase 2)
├── mcp-codegen/        ← Write to cache (Phase 3)
├── mcp-wasm-runtime/   ← Read from cache (Phase 4)
├── mcp-vfs/            ← Load from cache (Phase 5)
├── mcp-cli/            ← Add cache commands (Phase 7)
└── ...
```

### 2. Current State

**Already Exists**:
- ✅ `RuntimeConfig` has `cache_dir: Option<PathBuf>` (can repurpose)
- ✅ `SkillStore` manages `~/.claude/skills/` directory
- ✅ Build system works (cargo build succeeds)
- ✅ Tests run (cargo test)

**Needs Implementation**:
- ❌ `CacheManager` abstraction (new)
- ❌ Cache-aware storage methods
- ❌ Migration logic
- ❌ CLI commands

### 3. Key Constraints

**MUST Follow**:
1. **Microsoft Rust Guidelines** (see `.github/instructions/`)
2. **Strong types** over primitives (e.g., `CacheDir`, not `PathBuf`)
3. **thiserror** for libraries, **anyhow** only for mcp-cli
4. **Unit tests in same file** as code (`#[cfg(test)] mod tests`)
5. **Doc comments** for all public items

**MUST NOT**:
- Don't use `unwrap()` in library code (use `?` and proper errors)
- Don't add new dependencies (use existing ones)
- Don't break existing tests
- Don't use raw `String` where strong types fit

---

## 📖 Implementation Guide

### Read First

1. **Implementation Plan**: `docs/implementation-plan-cache-separation.md`
   - 30,000+ words of detailed guidance
   - Code examples for every phase
   - Test strategies
   - Risk mitigation

2. **Project Instructions**: `.github/instructions/`
   - `mcp-core.instructions.md` - Core types guidelines
   - `mcp-skill-store.instructions.md` - Storage guidelines
   - Other crate-specific instructions

### Implementation Order

```
Phase 1: mcp-core/src/cache_manager.rs         (Day 1 morning)
         └─ Tests for CacheManager             (Day 1 afternoon)

Phase 2: mcp-skill-store updates               (Day 1 evening)
         └─ save_skill_with_cache()
         └─ load_skill_with_cache()

Phase 3: mcp-codegen updates                   (Day 2 morning)
Phase 4: mcp-wasm-runtime updates              (Day 2 morning)
Phase 5: mcp-vfs updates                       (Day 2 afternoon)

Phase 6: mcp-core/src/migration.rs             (Day 2 evening)
Phase 7: mcp-cli/src/commands/cache.rs         (Day 3 morning)

Phase 8: Integration tests                     (Day 3 afternoon)
Phase 9: Documentation                         (Day 3 evening)
```

### How to Start

```bash
# 1. Review the implementation plan
cat docs/implementation-plan-cache-separation.md

# 2. Start with Phase 1
# Create: crates/mcp-core/src/cache_manager.rs
# See implementation plan for full code structure

# 3. Run tests frequently
cargo test --package mcp-core

# 4. Build incrementally
cargo build --package mcp-core
```

---

## 🎯 Success Criteria

### Must Have ✅
- [ ] `CacheManager` fully implemented and tested
- [ ] `SkillStore` uses cache for WASM/VFS
- [ ] Migration works for existing installations
- [ ] CLI commands functional (`cache info`, `cache clear`, `migrate`)
- [ ] All existing tests still pass
- [ ] Documentation updated

### Should Have 🎯
- [ ] Cache verification command
- [ ] Cross-platform tests
- [ ] Migration dry-run mode
- [ ] Good error messages

### Nice to Have 💡
- [ ] Cache compression
- [ ] LRU eviction
- [ ] Statistics visualization

---

## 🧪 Testing Strategy

### Unit Tests (in same file as code)

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_cache_manager_creation() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();
        
        assert!(cache.wasm_dir().exists());
        assert!(cache.vfs_dir().exists());
        assert!(cache.metadata_dir().exists());
    }
    
    #[test]
    fn test_cache_clear() {
        // ... test cache clearing
    }
}
```

### Integration Tests

```bash
# Create: tests/integration/cache_separation.rs
cargo test --test cache_separation
```

### Run Tests

```bash
# Unit tests
cargo test --workspace

# Specific crate
cargo test --package mcp-core

# With output
cargo test -- --nocapture

# Single test
cargo test test_cache_manager_creation
```

---

## 📝 Code Examples

### Example 1: CacheManager (Phase 1)

```rust
// crates/mcp-core/src/cache_manager.rs

use crate::error::{Error, Result};
use std::path::{Path, PathBuf};

/// Cache manager for internal MCP execution data.
///
/// Manages `~/.mcp-execution/cache/` with subdirectories:
/// - `wasm/`: Compiled WASM modules
/// - `vfs/`: Generated code files
/// - `metadata/`: Build timestamps and checksums
///
/// # Examples
///
/// ```
/// use mcp_core::CacheManager;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let cache = CacheManager::new()?;
/// let wasm_path = cache.wasm_path("vkteams-bot");
/// # Ok(())
/// # }
/// ```
#[derive(Debug, Clone)]
pub struct CacheManager {
    cache_root: PathBuf,
}

impl CacheManager {
    /// Creates cache manager with default directory.
    ///
    /// Default: `~/.mcp-execution/cache/`
    ///
    /// # Errors
    ///
    /// Returns error if home directory cannot be determined or
    /// cache directory cannot be created.
    pub fn new() -> Result<Self> {
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| Error::CacheError("Cannot determine cache directory".into()))?
            .join("mcp-execution");
        
        Self::with_directory(cache_dir)
    }
    
    /// Creates cache manager with custom directory.
    pub fn with_directory(path: impl AsRef<Path>) -> Result<Self> {
        let cache_root = path.as_ref().to_path_buf();
        
        // Create subdirectories
        std::fs::create_dir_all(cache_root.join("wasm"))?;
        std::fs::create_dir_all(cache_root.join("vfs"))?;
        std::fs::create_dir_all(cache_root.join("metadata"))?;
        
        Ok(Self { cache_root })
    }
    
    /// Returns path to WASM directory.
    pub fn wasm_dir(&self) -> PathBuf {
        self.cache_root.join("wasm")
    }
    
    /// Returns path to VFS directory.
    pub fn vfs_dir(&self) -> PathBuf {
        self.cache_root.join("vfs")
    }
    
    /// Returns path to metadata directory.
    pub fn metadata_dir(&self) -> PathBuf {
        self.cache_root.join("metadata")
    }
    
    /// Gets path to WASM module for a skill.
    pub fn wasm_path(&self, skill_name: &str) -> PathBuf {
        self.wasm_dir().join(format!("{skill_name}.wasm"))
    }
    
    /// Gets path to VFS directory for a skill.
    pub fn vfs_path(&self, skill_name: &str) -> PathBuf {
        self.vfs_dir().join(skill_name)
    }
    
    /// Gets path to metadata file for a skill.
    pub fn metadata_path(&self, skill_name: &str) -> PathBuf {
        self.metadata_dir().join(format!("{skill_name}.json"))
    }
    
    /// Clears all cache data.
    pub fn clear_all(&self) -> Result<()> {
        std::fs::remove_dir_all(&self.cache_root)?;
        std::fs::create_dir_all(&self.cache_root)?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_cache_manager_creation() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();
        
        assert!(cache.wasm_dir().exists());
        assert!(cache.vfs_dir().exists());
        assert!(cache.metadata_dir().exists());
    }
    
    #[test]
    fn test_cache_paths() {
        let temp = TempDir::new().unwrap();
        let cache = CacheManager::with_directory(temp.path()).unwrap();
        
        let wasm_path = cache.wasm_path("test-skill");
        assert!(wasm_path.ends_with("wasm/test-skill.wasm"));
        
        let vfs_path = cache.vfs_path("test-skill");
        assert!(vfs_path.ends_with("vfs/test-skill"));
    }
}
```

### Example 2: Update SkillStore (Phase 2)

```rust
// In crates/mcp-skill-store/src/store.rs

impl SkillStore {
    /// Saves skill with cache manager integration.
    ///
    /// Public files go to ~/.claude/skills/
    /// Cache files go to cache manager
    pub fn save_skill_with_cache(
        &self,
        server_name: &str,
        vfs: &Vfs,
        wasm_module: &[u8],
        server_info: ServerInfo,
        tool_info: Vec<ToolInfo>,
        cache_manager: &CacheManager,
    ) -> Result<SkillMetadata> {
        // 1. Validate
        validate_server_name(server_name)?;
        
        // 2. Create skill directory (public only)
        let skill_dir = self.skill_path(server_name);
        fs::create_dir(&skill_dir)?;
        let guard = SkillDirGuard::new(skill_dir.clone());
        
        // 3. Save WASM to cache (NOT skill dir)
        let wasm_path = cache_manager.wasm_path(server_name);
        fs::write(&wasm_path, wasm_module)?;
        
        // 4. Save VFS to cache (NOT skill dir)
        let vfs_dir = cache_manager.vfs_path(server_name);
        self.write_vfs_to_cache(&vfs, &vfs_dir)?;
        
        // 5. Save public metadata only
        let metadata = SkillMetadata {
            format_version: FORMAT_VERSION.to_string(),
            server: server_info,
            generated_at: Utc::now(),
            generator_version: env!("CARGO_PKG_VERSION").to_string(),
            tools: tool_info,
            // NO checksums in public metadata
        };
        
        let metadata_path = skill_dir.join(METADATA_FILE);
        fs::write(&metadata_path, serde_json::to_string_pretty(&metadata)?)?;
        
        guard.commit();
        Ok(metadata)
    }
}
```

---

## 🚨 Common Pitfalls

### 1. Path Separators

**❌ Wrong**:
```rust
let path = format!("{}/wasm/{}.wasm", base, skill);  // Unix-only!
```

**✅ Right**:
```rust
let path = base.join("wasm").join(format!("{skill}.wasm"));
```

### 2. Error Handling

**❌ Wrong**:
```rust
let file = fs::read(path).unwrap();  // NEVER in library code!
```

**✅ Right**:
```rust
let file = fs::read(path)?;  // Propagate error
```

### 3. String Types

**❌ Wrong**:
```rust
pub fn wasm_path(skill_name: String) -> PathBuf {  // Takes ownership
```

**✅ Right**:
```rust
pub fn wasm_path(&self, skill_name: &str) -> PathBuf {  // Borrows
```

### 4. Testing with Temp Directories

**❌ Wrong**:
```rust
#[test]
fn test_cache() {
    let cache = CacheManager::new().unwrap();  // Uses real home dir!
}
```

**✅ Right**:
```rust
#[test]
fn test_cache() {
    let temp = TempDir::new().unwrap();
    let cache = CacheManager::with_directory(temp.path()).unwrap();
}
```

---

## 🔧 Useful Commands

```bash
# Format code
cargo +nightly fmt --workspace

# Check for errors
cargo check --workspace

# Build
cargo build --workspace

# Run tests
cargo test --workspace

# Run specific test
cargo test test_cache_manager

# Run clippy
cargo clippy --workspace -- -D warnings

# Build docs
cargo doc --open

# Check a specific crate
cargo check --package mcp-core

# Watch for changes (install cargo-watch first)
cargo watch -x check -x test
```

---

## 📞 Getting Help

### Questions About Design

Refer to:
1. `docs/implementation-plan-cache-separation.md` (comprehensive)
2. `.github/instructions/` (crate-specific guidelines)
3. Microsoft Rust Guidelines: https://microsoft.github.io/rust-guidelines/

### Questions During Implementation

**If something is unclear**:
- Check implementation plan first (30k words!)
- Look at existing code patterns in the crate
- Ask for clarification (don't guess!)

**Common questions answered in plan**:
- Q: Should `cache_dir` in `RuntimeConfig` be deprecated?
- Q: Should old methods be removed or deprecated?
- Q: Should migration be automatic?
- See "Questions for Implementer" section in plan

---

## ✅ Definition of Done

Your work is complete when:

1. **All phases implemented** (1-9)
2. **All tests pass**: `cargo test --workspace`
3. **Clippy clean**: `cargo clippy --workspace -- -D warnings`
4. **Formatted**: `cargo +nightly fmt --workspace`
5. **Documentation complete**: Updated ARCHITECTURE.md, CLAUDE.md
6. **Manual testing**: CLI commands work as expected
7. **Migration tested**: Old installations can migrate
8. **Cross-platform**: Paths work on Windows/Mac/Linux

---

## 📦 Deliverables

### Code Changes

- [ ] `mcp-core/src/cache_manager.rs` (new)
- [ ] `mcp-core/src/migration.rs` (new)
- [ ] `mcp-core/src/lib.rs` (modified)
- [ ] `mcp-skill-store/src/store.rs` (modified)
- [ ] `mcp-skill-store/src/types.rs` (modified)
- [ ] `mcp-codegen/src/**/*.rs` (modified)
- [ ] `mcp-wasm-runtime/src/**/*.rs` (modified)
- [ ] `mcp-vfs/src/builder.rs` (modified)
- [ ] `mcp-cli/src/commands/cache.rs` (new)
- [ ] `mcp-cli/src/commands/migrate.rs` (new)
- [ ] `mcp-cli/src/main.rs` (modified)

### Tests

- [ ] Unit tests in each modified file
- [ ] `tests/integration/cache_separation.rs` (new)
- [ ] `tests/integration/cross_platform.rs` (new)

### Documentation

- [ ] `docs/ARCHITECTURE.md` (modified)
- [ ] `docs/CLAUDE.md` (modified)
- [ ] `docs/MIGRATION.md` (new)
- [ ] Doc comments in all public items

---

## 🎯 Final Checklist

Before submitting:

```bash
# 1. Format
cargo +nightly fmt --workspace

# 2. Lint
cargo clippy --workspace -- -D warnings

# 3. Test
cargo test --workspace

# 4. Build
cargo build --workspace --release

# 5. Manual smoke test
cargo run --bin mcp-cli -- cache info
cargo run --bin mcp-cli -- cache clear --yes
cargo run --bin mcp-cli -- migrate
```

---

## 🚀 Good Luck!

You have everything you need:
- ✅ Comprehensive implementation plan
- ✅ Code examples for every phase
- ✅ Test strategies
- ✅ Working build environment
- ✅ Clear success criteria

**Remember**:
- Work incrementally (phase by phase)
- Test frequently (`cargo test`)
- Follow Microsoft Rust Guidelines
- Ask if something is unclear

**You got this!** 💪

---

**Handover Complete**  
**Status**: Ready for rust-developer agent to begin implementation

---

## Appendix: Quick Reference

### Key Files to Read

1. `docs/implementation-plan-cache-separation.md` (main plan)
2. `.github/instructions/mcp-core.instructions.md`
3. `.github/instructions/mcp-skill-store.instructions.md`
4. `crates/mcp-core/src/config.rs` (existing cache config)
5. `crates/mcp-skill-store/src/store.rs` (current storage)

### Key Patterns

- **Strong types**: `CacheManager`, not `PathBuf`
- **Error handling**: `thiserror` for libs, `anyhow` for CLI
- **Testing**: `#[cfg(test)] mod tests` in same file
- **Docs**: `///` comments for all public items
- **Paths**: Use `Path::join()`, not string concatenation

### Key Crates

- `mcp-core`: Foundation types
- `mcp-skill-store`: Public skills
- `mcp-cli`: CLI interface
- `dirs`: Platform-specific directories
- `tempfile`: Testing with temp directories

---

**END OF HANDOVER**
