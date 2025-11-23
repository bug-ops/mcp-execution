# Implementation Plan: Separate Public Skills from Internal Cache Storage

**Issue**: Separate public skills from internal cache storage  
**Status**: Ready for Implementation  
**Architect**: copilot (GitHub Copilot)  
**Implementer**: TBD (rust-developer agent)  
**Date**: 2025-11-23

---

## Executive Summary

This implementation separates skill storage into two distinct directories:
- **Public Skills** (`~/.claude/skills/`): User-facing, version-controllable SKILL.md/REFERENCE.md files
- **Internal Cache** (`~/.mcp-execution/cache/`): System-managed, regenerable WASM/VFS/metadata

**Estimated Effort**: 2-3 days  
**Priority**: Medium  
**Risk**: Low (backward compatible with migration)

---

## Current State Analysis

### Existing Directory Structure
```
~/.claude/skills/
  ├── vkteams-bot/
  │   ├── SKILL.md              # Public
  │   ├── REFERENCE.md          # Public
  │   ├── .metadata.json        # Mixed (public + internal)
  │   └── generated/            # Internal (should be cache)
  │       └── *.ts files
  └── (no WASM storage currently in code review)
```

### Key Files Reviewed

1. **mcp-core/src/config.rs**
   - Already has `cache_dir: Option<PathBuf>` in `RuntimeConfig` (line 69)
   - Used for compiled WASM module caching
   - Validates cache directory (lines 183-188)

2. **mcp-skill-store/src/store.rs**
   - Manages `~/.claude/skills/` directory structure
   - `save_skill()`: Saves VFS, WASM, metadata (lines 233-339)
   - `save_claude_skill()`: Saves SKILL.md, REFERENCE.md (lines 783-858)
   - Already has atomic writes with `SkillDirGuard` cleanup (lines 31-72)

3. **mcp-wasm-runtime/src/cache.rs**
   - Likely handles WASM module caching (need to review)

4. **mcp-vfs/**
   - Virtual filesystem for generated TypeScript code
   - Currently embedded in skill directories

### Current Issues

1. **Mixed Concerns**: `SkillStore` handles both public skills AND internal cache
2. **No Clear Cache**: WASM/VFS scattered across skill directories
3. **User Confusion**: Users might accidentally commit generated files
4. **No Cache Management**: No way to clear/verify cache

---

## Target State Design

### New Directory Structure

```
~/.claude/
  └── skills/                    # Public (user-facing)
      ├── vkteams-bot/
      │   ├── SKILL.md          # ✅ Keep - Claude Skill format
      │   ├── REFERENCE.md      # ✅ Keep - API reference
      │   └── .metadata.json    # ✅ Keep - Public metadata only
      └── github/
          ├── SKILL.md
          └── REFERENCE.md

~/.mcp-execution/
  └── cache/                     # Internal (system-managed)
      ├── wasm/                  # Compiled WASM modules
      │   ├── vkteams-bot.wasm
      │   └── github.wasm
      ├── vfs/                   # Generated TypeScript/code
      │   ├── vkteams-bot/
      │   │   ├── index.ts
      │   │   └── tools/*.ts
      │   └── github/
      │       └── *.ts
      └── metadata/              # Build metadata
          ├── vkteams-bot.json  # Timestamps, checksums, build info
          └── github.json
```

### Key Design Principles

1. **Separation of Concerns**
   - `SkillStore` → Public skills only (SKILL.md, REFERENCE.md)
   - `CacheManager` → Internal cache (WASM, VFS, metadata)

2. **Cache Properties**
   - Fully regenerable from skill definitions
   - Can be safely deleted without losing user data
   - Includes checksums for integrity verification

3. **Public Skill Properties**
   - User-editable
   - Version-controllable (git)
   - Shareable between users
   - Stable format

4. **Backward Compatibility**
   - Migration logic detects old format
   - Automatically moves cache files to new location
   - Preserves public skills in place

---

## Implementation Plan

### Phase 1: Core Types & Abstractions (mcp-core)

**Goal**: Create foundation for cache management

#### 1.1 Create `cache_manager.rs` in `mcp-core/src/`

```rust
//! Cache management for internal MCP execution data.
//!
//! Manages:
//! - Compiled WASM modules
//! - Generated VFS code
//! - Build metadata (timestamps, checksums)

use crate::error::{Error, Result};
use crate::ServerId;
use std::path::{Path, PathBuf};
use blake3::Hasher;

/// Cache manager for internal MCP execution data.
///
/// Manages the `~/.mcp-execution/cache/` directory with three subdirectories:
/// - `wasm/`: Compiled WASM modules
/// - `vfs/`: Generated TypeScript/code files
/// - `metadata/`: Build timestamps and checksums
#[derive(Debug, Clone)]
pub struct CacheManager {
    cache_root: PathBuf,
}

impl CacheManager {
    /// Creates a new cache manager with default directory.
    ///
    /// Default: `~/.mcp-execution/cache/`
    pub fn new() -> Result<Self>;
    
    /// Creates a cache manager with custom directory.
    pub fn with_directory(path: impl AsRef<Path>) -> Result<Self>;
    
    /// Returns path to WASM directory.
    pub fn wasm_dir(&self) -> PathBuf;
    
    /// Returns path to VFS directory.
    pub fn vfs_dir(&self) -> PathBuf;
    
    /// Returns path to metadata directory.
    pub fn metadata_dir(&self) -> PathBuf;
    
    /// Gets path to WASM module for a skill.
    pub fn wasm_path(&self, skill_name: &str) -> PathBuf;
    
    /// Gets path to VFS directory for a skill.
    pub fn vfs_path(&self, skill_name: &str) -> PathBuf;
    
    /// Gets path to metadata file for a skill.
    pub fn metadata_path(&self, skill_name: &str) -> PathBuf;
    
    /// Checks if WASM module exists in cache.
    pub fn has_wasm(&self, skill_name: &str) -> bool;
    
    /// Checks if VFS cache exists for a skill.
    pub fn has_vfs(&self, skill_name: &str) -> bool;
    
    /// Checks if metadata exists for a skill.
    pub fn has_metadata(&self, skill_name: &str) -> bool;
    
    /// Clears all cache data.
    pub fn clear_all(&self) -> Result<()>;
    
    /// Clears cache for a specific skill.
    pub fn clear_skill(&self, skill_name: &str) -> Result<()>;
    
    /// Gets cache statistics.
    pub fn stats(&self) -> Result<CacheStats>;
}

/// Statistics about cache usage.
#[derive(Debug, Clone)]
pub struct CacheStats {
    pub total_wasm_files: usize,
    pub total_vfs_files: usize,
    pub total_metadata_files: usize,
    pub total_size_bytes: u64,
}

/// Build metadata for cached skills.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BuildMetadata {
    pub skill_name: String,
    pub built_at: chrono::DateTime<chrono::Utc>,
    pub generator_version: String,
    pub wasm_checksum: String,
    pub vfs_checksums: HashMap<String, String>,
}
```

**Key Implementation Notes**:
- Use `dirs::cache_dir()` for cross-platform cache directory
- Create subdirectories lazily (on first write)
- Use atomic operations where possible
- Add comprehensive error handling with `thiserror`

#### 1.2 Update `mcp-core/src/lib.rs`

```rust
// Add to lib.rs
pub mod cache_manager;
pub use cache_manager::{CacheManager, CacheStats, BuildMetadata};
```

#### 1.3 Update `RuntimeConfig` if needed

The `cache_dir` field already exists in `RuntimeConfig` (line 69 of config.rs).  
Consider:
- Keep it for backward compatibility
- Add deprecation notice pointing to `CacheManager`
- Or repurpose it as override for `CacheManager` default

**Tests**:
```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    #[test]
    fn test_cache_manager_creation() { }
    
    #[test]
    fn test_cache_paths() { }
    
    #[test]
    fn test_clear_cache() { }
    
    #[test]
    fn test_cache_stats() { }
}
```

---

### Phase 2: Update mcp-skill-store

**Goal**: Separate public skill management from cache

#### 2.1 Refactor `SkillStore` to Public-Only

**Current** (`store.rs`):
- `save_skill()` saves VFS + WASM + metadata (lines 233-339)
- `load_skill()` loads everything (lines 365-481)

**Changes Needed**:

```rust
// In mcp-skill-store/src/store.rs

impl SkillStore {
    /// Saves a skill with cache manager integration.
    ///
    /// Public files (SKILL.md, REFERENCE.md) go to ~/.claude/skills/
    /// Internal cache (WASM, VFS) goes to cache manager
    pub fn save_skill_with_cache(
        &self,
        server_name: &str,
        vfs: &Vfs,
        wasm_module: &[u8],
        server_info: ServerInfo,
        tool_info: Vec<ToolInfo>,
        cache_manager: &CacheManager,  // NEW parameter
    ) -> Result<SkillMetadata>;
    
    /// Loads a skill using cache manager.
    pub fn load_skill_with_cache(
        &self,
        server_name: &str,
        cache_manager: &CacheManager,  // NEW parameter
    ) -> Result<LoadedSkill>;
}
```

**Implementation Steps**:

1. **Modify `save_skill_with_cache()`**:
   - Save SKILL.md/REFERENCE.md to `~/.claude/skills/{name}/`
   - Save WASM to `cache_manager.wasm_path(name)`
   - Save VFS to `cache_manager.vfs_path(name)/`
   - Save build metadata to `cache_manager.metadata_path(name)`
   - Update `.metadata.json` to NOT include WASM/VFS checksums

2. **Modify `load_skill_with_cache()`**:
   - Read SKILL.md/REFERENCE.md from public directory
   - Read WASM from cache manager
   - Read VFS from cache manager
   - Verify checksums from build metadata

3. **Keep Backward Compatible Methods**:
   - Mark old `save_skill()` as deprecated
   - Keep old `load_skill()` for migration

#### 2.2 Update `ClaudeSkillMetadata`

**Current** (in `types.rs`):
```rust
pub struct ClaudeSkillMetadata {
    pub skill_name: String,
    pub server_name: String,
    pub server_version: String,
    pub protocol_version: String,
    pub tool_count: usize,
    pub generated_at: DateTime<Utc>,
    pub generator_version: String,
    pub checksums: SkillChecksums,  // Remove from public metadata
}
```

**New**:
```rust
pub struct ClaudeSkillMetadata {
    pub skill_name: String,
    pub server_name: String,
    pub server_version: String,
    pub protocol_version: String,
    pub tool_count: usize,
    pub generated_at: DateTime<Utc>,
    pub generator_version: String,
    // Checksums moved to BuildMetadata in cache
}
```

**Tests**:
- Update all existing tests to use `with_cache` methods
- Add tests for cache integration
- Test migration from old format

---

### Phase 3: Update mcp-codegen

**Goal**: Write generated code to cache instead of skill directories

#### 3.1 Modify Code Generator

**File**: `mcp-codegen/src/wasm/` (if exists)

**Changes**:
- Accept `CacheManager` parameter
- Write WASM output to `cache_manager.wasm_path()`
- Write generated TypeScript to `cache_manager.vfs_path()`
- Generate build metadata

**Example**:
```rust
pub struct WasmCodegen {
    cache_manager: CacheManager,
}

impl WasmCodegen {
    pub fn generate(&self, skill_name: &str, tools: &[Tool]) -> Result<()> {
        // Generate WASM
        let wasm_bytes = self.compile_to_wasm(tools)?;
        
        // Write to cache
        let wasm_path = self.cache_manager.wasm_path(skill_name);
        fs::write(wasm_path, wasm_bytes)?;
        
        // Generate VFS
        let vfs = self.generate_typescript(tools)?;
        
        // Write VFS to cache
        let vfs_dir = self.cache_manager.vfs_path(skill_name);
        self.write_vfs(&vfs, &vfs_dir)?;
        
        Ok(())
    }
}
```

**Tests**:
- Verify generated files go to cache directory
- Verify old behavior still works (backward compat)

---

### Phase 4: Update mcp-wasm-runtime

**Goal**: Load WASM from cache

#### 4.1 Modify Runtime Initialization

**File**: `mcp-wasm-runtime/src/lib.rs` or `runtime.rs`

**Changes**:
- Accept `CacheManager` in constructor
- Load WASM modules from `cache_manager.wasm_path()`
- Use existing cache mechanism if present

**Example**:
```rust
pub struct WasmRuntime {
    cache_manager: CacheManager,
    // ... existing fields
}

impl WasmRuntime {
    pub fn new(cache_manager: CacheManager) -> Result<Self> {
        // ...
    }
    
    pub fn load_skill(&mut self, skill_name: &str) -> Result<()> {
        let wasm_path = self.cache_manager.wasm_path(skill_name);
        
        if !wasm_path.exists() {
            return Err(Error::WasmNotCached { skill_name: skill_name.to_string() });
        }
        
        let wasm_bytes = fs::read(&wasm_path)?;
        
        // Existing loading logic...
        Ok(())
    }
}
```

**Tests**:
- Test loading from cache
- Test error when WASM not cached
- Test cache invalidation

---

### Phase 5: Update mcp-vfs

**Goal**: Load VFS from cache

#### 5.1 Add Cache-Aware VFS Builder

**File**: `mcp-vfs/src/builder.rs`

**Changes**:
```rust
impl VfsBuilder {
    /// Loads VFS from cache directory.
    pub fn from_cache(cache_manager: &CacheManager, skill_name: &str) -> Result<Self> {
        let vfs_dir = cache_manager.vfs_path(skill_name);
        
        if !vfs_dir.exists() {
            return Err(VfsError::NotCached { skill_name: skill_name.to_string() });
        }
        
        let mut builder = Self::new();
        
        // Walk cache directory and load files
        for entry in WalkDir::new(&vfs_dir) {
            // ... load files into VFS
        }
        
        Ok(builder)
    }
}
```

**Tests**:
- Test loading VFS from cache
- Test error handling
- Cross-platform path handling

---

### Phase 6: Migration Logic

**Goal**: Handle existing installations seamlessly

#### 6.1 Create Migration Module

**File**: `mcp-core/src/migration.rs`

```rust
//! Migration logic for transitioning to cache-separated storage.

use crate::{CacheManager, Error, Result};
use std::path::Path;

/// Migrates skills from old format to new cache-separated format.
///
/// Old format: Everything in ~/.claude/skills/{name}/
/// New format: Public in ~/.claude/skills/, cache in ~/.mcp-execution/cache/
pub struct Migrator {
    cache_manager: CacheManager,
}

impl Migrator {
    pub fn new(cache_manager: CacheManager) -> Self {
        Self { cache_manager }
    }
    
    /// Checks if migration is needed.
    pub fn needs_migration(&self, skills_dir: &Path) -> Result<bool> {
        // Check for old-format indicators:
        // - .wasm files in skill directories
        // - generated/ folders in skill directories
        Ok(false) // placeholder
    }
    
    /// Migrates a single skill.
    pub fn migrate_skill(&self, skill_name: &str, skill_dir: &Path) -> Result<()> {
        tracing::info!("Migrating skill: {}", skill_name);
        
        // 1. Find WASM files in skill_dir
        let wasm_files = find_wasm_files(skill_dir)?;
        
        // 2. Move to cache
        for wasm_file in wasm_files {
            let cache_path = self.cache_manager.wasm_path(skill_name);
            fs::rename(wasm_file, cache_path)?;
        }
        
        // 3. Find generated/ directory
        let generated_dir = skill_dir.join("generated");
        if generated_dir.exists() {
            let vfs_cache = self.cache_manager.vfs_path(skill_name);
            fs::rename(generated_dir, vfs_cache)?;
        }
        
        // 4. Clean up old metadata
        let metadata_path = skill_dir.join(".metadata.json");
        if metadata_path.exists() {
            // Read, update (remove WASM checksums), write back
            update_metadata(&metadata_path)?;
        }
        
        tracing::info!("Migration complete for: {}", skill_name);
        Ok(())
    }
    
    /// Migrates all skills in a directory.
    pub fn migrate_all(&self, skills_dir: &Path) -> Result<MigrationReport> {
        let mut report = MigrationReport::default();
        
        for entry in fs::read_dir(skills_dir)? {
            let entry = entry?;
            if !entry.file_type()?.is_dir() {
                continue;
            }
            
            let skill_name = entry.file_name().to_string_lossy().to_string();
            
            match self.migrate_skill(&skill_name, &entry.path()) {
                Ok(()) => report.successes += 1,
                Err(e) => {
                    report.failures.push((skill_name, e));
                }
            }
        }
        
        Ok(report)
    }
}

#[derive(Debug, Default)]
pub struct MigrationReport {
    pub successes: usize,
    pub failures: Vec<(String, Error)>,
}
```

#### 6.2 Add Migration CLI Command

**File**: `mcp-cli/src/commands/migrate.rs`

```rust
use anyhow::Result;
use mcp_core::{CacheManager, migration::Migrator};
use mcp_skill_store::SkillStore;

pub async fn migrate_skills() -> Result<()> {
    let store = SkillStore::new_claude()?;
    let cache_manager = CacheManager::new()?;
    let migrator = Migrator::new(cache_manager);
    
    println!("Checking for skills to migrate...");
    
    if !migrator.needs_migration(store.base_dir())? {
        println!("No migration needed!");
        return Ok(());
    }
    
    println!("Starting migration...");
    let report = migrator.migrate_all(store.base_dir())?;
    
    println!("\nMigration complete!");
    println!("  Successful: {}", report.successes);
    println!("  Failed: {}", report.failures.len());
    
    for (skill, err) in &report.failures {
        eprintln!("  ❌ {}: {}", skill, err);
    }
    
    Ok(())
}
```

**Tests**:
- Test detection of old format
- Test migration of single skill
- Test migration of multiple skills
- Test idempotency (running migration twice)
- Test error handling (partial migration)

---

### Phase 7: CLI Commands

**Goal**: Provide cache management commands

#### 7.1 Add Cache Commands

**File**: `mcp-cli/src/commands/cache.rs`

```rust
use anyhow::Result;
use clap::Subcommand;
use mcp_core::CacheManager;
use colored::Colorize;

#[derive(Subcommand)]
pub enum CacheCommand {
    /// Show cache information
    Info,
    
    /// Clear all cached data
    Clear {
        /// Skill name (optional, clears all if not specified)
        skill: Option<String>,
        
        /// Skip confirmation prompt
        #[arg(long)]
        yes: bool,
    },
    
    /// Verify cache integrity
    Verify,
}

pub async fn handle_cache_command(cmd: CacheCommand) -> Result<()> {
    let cache_manager = CacheManager::new()?;
    
    match cmd {
        CacheCommand::Info => show_cache_info(&cache_manager).await?,
        CacheCommand::Clear { skill, yes } => clear_cache(&cache_manager, skill, yes).await?,
        CacheCommand::Verify => verify_cache(&cache_manager).await?,
    }
    
    Ok(())
}

async fn show_cache_info(cache: &CacheManager) -> Result<()> {
    let stats = cache.stats()?;
    
    println!("{}", "Cache Information".bold());
    println!("  Location: {}", cache.cache_root.display());
    println!();
    println!("  WASM modules: {}", stats.total_wasm_files);
    println!("  VFS caches: {}", stats.total_vfs_files);
    println!("  Metadata files: {}", stats.total_metadata_files);
    println!("  Total size: {} bytes", stats.total_size_bytes);
    
    Ok(())
}

async fn clear_cache(cache: &CacheManager, skill: Option<String>, yes: bool) -> Result<()> {
    if !yes {
        println!("This will delete all cached data. Continue? [y/N]");
        // ... confirmation logic
    }
    
    match skill {
        Some(name) => {
            cache.clear_skill(&name)?;
            println!("{} Cleared cache for skill: {}", "✓".green(), name);
        }
        None => {
            cache.clear_all()?;
            println!("{} Cleared all cache", "✓".green());
        }
    }
    
    Ok(())
}

async fn verify_cache(cache: &CacheManager) -> Result<()> {
    println!("Verifying cache integrity...");
    
    // Check each cached skill
    let stats = cache.stats()?;
    
    println!("{} Cache verification complete", "✓".green());
    println!("  All {} skills verified", stats.total_wasm_files);
    
    Ok(())
}
```

#### 7.2 Update Main CLI

**File**: `mcp-cli/src/main.rs` or `commands/mod.rs`

```rust
#[derive(Subcommand)]
enum Commands {
    // ... existing commands
    
    /// Manage cache
    Cache {
        #[command(subcommand)]
        command: CacheCommand,
    },
    
    /// Migrate skills to new format (one-time)
    Migrate,
}
```

**Tests**:
- Test `cache info` command
- Test `cache clear` command
- Test `cache verify` command
- Test confirmation prompts

---

### Phase 8: Testing Strategy

#### 8.1 Unit Tests

Each module should have comprehensive unit tests in the same file:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    
    // Example test structure
}
```

**Coverage Requirements**:
- `CacheManager`: 100% coverage
- `SkillStore` changes: 100% coverage
- Migration logic: 100% coverage
- CLI commands: Integration tests

#### 8.2 Integration Tests

**File**: `tests/integration/cache_separation.rs`

```rust
#[tokio::test]
async fn test_full_workflow_with_cache() {
    // 1. Create skill
    // 2. Generate code
    // 3. Verify public files in ~/.claude/skills/
    // 4. Verify cache files in ~/.mcp-execution/cache/
    // 5. Load skill
    // 6. Verify functionality
}

#[tokio::test]
async fn test_migration_from_old_format() {
    // 1. Create old-format skill
    // 2. Run migration
    // 3. Verify new structure
    // 4. Verify skill still works
}

#[tokio::test]
async fn test_cache_regeneration() {
    // 1. Create skill
    // 2. Clear cache
    // 3. Regenerate cache
    // 4. Verify skill works
}
```

#### 8.3 Cross-Platform Tests

**File**: `tests/integration/cross_platform.rs`

```rust
#[test]
fn test_cache_paths_windows() {
    // Test Windows paths
}

#[test]
fn test_cache_paths_unix() {
    // Test Unix paths
}

#[test]
fn test_path_separator_handling() {
    // Test forward/backward slashes
}
```

#### 8.4 Test Matrix

- **Platforms**: Linux, macOS, Windows
- **Scenarios**:
  - Fresh installation
  - Migration from old format
  - Cache regeneration
  - Concurrent access
  - Error recovery

---

### Phase 9: Documentation

#### 9.1 Update ARCHITECTURE.md

Add section on storage architecture:

```markdown
## Storage Architecture

MCP Execution uses a two-tier storage model:

### Public Skills Directory: `~/.claude/skills/`

User-facing files that should be version-controlled:
- `SKILL.md`: Claude Agent Skills format
- `REFERENCE.md`: Detailed API reference
- `.metadata.json`: Public metadata (server info, tool count)

### Internal Cache Directory: `~/.mcp-execution/cache/`

System-managed files that can be regenerated:
- `wasm/`: Compiled WASM modules
- `vfs/`: Generated TypeScript code
- `metadata/`: Build timestamps and checksums

The cache can be safely deleted at any time. All cached data can be regenerated from public skills.
```

#### 9.2 Update CLAUDE.md

Add user guide section:

```markdown
## Working with Skills

### Public Skills

Skills are stored in `~/.claude/skills/` as:
- `SKILL.md`: The skill definition
- `REFERENCE.md`: API documentation

These files are safe to edit, commit to git, and share.

### Cache Management

MCP Execution maintains an internal cache at `~/.mcp-execution/cache/`.

**View cache information**:
```bash
mcp-execution cache info
```

**Clear cache**:
```bash
mcp-execution cache clear
```

**Clear cache for specific skill**:
```bash
mcp-execution cache clear vkteams-bot
```

The cache will be automatically regenerated as needed.
```

#### 9.3 Create MIGRATION.md

```markdown
# Migration Guide: Cache Separation

## Overview

Version 0.3.0 introduces separated storage for public skills and internal cache.

## Automatic Migration

Run:
```bash
mcp-execution migrate
```

This will:
1. Move WASM modules to `~/.mcp-execution/cache/wasm/`
2. Move generated code to `~/.mcp-execution/cache/vfs/`
3. Update metadata files
4. Preserve `SKILL.md` and `REFERENCE.md` in place

## Manual Migration (if needed)

If automatic migration fails:

1. Back up your skills:
   ```bash
   cp -r ~/.claude/skills ~/.claude/skills.backup
   ```

2. Run migration with verbose logging:
   ```bash
   RUST_LOG=debug mcp-execution migrate
   ```

3. If issues persist, file a bug report with logs.

## Rollback

To revert to the old format:
1. Restore from backup: `cp -r ~/.claude/skills.backup ~/.claude/skills`
2. Downgrade to version 0.2.x

## Benefits

- ✅ Clear separation of user data vs system cache
- ✅ Faster skill sharing (no large cached files)
- ✅ Version control friendly (only SKILL.md in git)
- ✅ Better disk space management (cache can be cleared)
```

#### 9.4 Update ADR (if needed)

**File**: `docs/adr/009-cache-separation.md`

Document the architectural decision:
- Context
- Decision
- Consequences
- Alternatives considered

---

## Implementation Order

### Day 1: Foundation
1. ✅ Phase 1: Core types & `CacheManager` (mcp-core)
2. ✅ Write unit tests for `CacheManager`
3. ✅ Phase 2: Update `SkillStore` with cache support

### Day 2: Integration
4. ✅ Phase 3: Update `mcp-codegen`
5. ✅ Phase 4: Update `mcp-wasm-runtime`
6. ✅ Phase 5: Update `mcp-vfs`
7. ✅ Write integration tests

### Day 3: Migration & Polish
8. ✅ Phase 6: Migration logic
9. ✅ Phase 7: CLI commands
10. ✅ Phase 8: Full test suite
11. ✅ Phase 9: Documentation
12. ✅ Final review and testing

---

## Risk Mitigation

### Risk 1: Data Loss During Migration
**Mitigation**:
- Implement dry-run mode for migration
- Add backup step before migration
- Atomic operations with rollback

### Risk 2: Cross-Platform Path Issues
**Mitigation**:
- Use `std::path::Path` consistently
- Test on Windows, macOS, Linux
- Normalize path separators

### Risk 3: Cache Corruption
**Mitigation**:
- Store checksums in build metadata
- Implement cache verification
- Graceful fallback to regeneration

### Risk 4: Backward Compatibility
**Mitigation**:
- Keep deprecated methods functional
- Add compatibility layer
- Comprehensive migration tests

---

## Success Criteria

### Must Have ✅
- [ ] `CacheManager` fully implemented and tested
- [ ] `SkillStore` separated from cache concerns
- [ ] Migration works for existing installations
- [ ] CLI commands functional
- [ ] All existing tests pass
- [ ] Documentation updated

### Should Have 🎯
- [ ] Cache verification command works
- [ ] Cross-platform tests pass
- [ ] Performance benchmarks acceptable
- [ ] Memory usage reasonable

### Nice to Have 💡
- [ ] Cache compression
- [ ] Cache deduplication
- [ ] Cache statistics visualization
- [ ] Automatic cache cleanup (LRU)

---

## Testing Checklist

Before marking implementation complete:

- [ ] All unit tests pass (cargo test)
- [ ] Integration tests pass
- [ ] Cross-platform tests pass (Linux, macOS, Windows)
- [ ] Migration tests pass
- [ ] CLI commands tested manually
- [ ] Documentation reviewed
- [ ] Code review complete
- [ ] No regression in existing functionality

---

## Code Review Guidelines

### For Reviewer

Check for:
1. **Type Safety**: Using strong types (`CacheDir`, etc.)?
2. **Error Handling**: Using `thiserror` for libraries, `anyhow` for CLI?
3. **Testing**: Comprehensive coverage?
4. **Documentation**: All public items documented?
5. **Security**: Path traversal prevention?
6. **Performance**: Unnecessary allocations?
7. **Cross-Platform**: Windows compatibility?

### For Implementer

Follow:
1. **Microsoft Rust Guidelines**: Strong types, no primitive obsession
2. **Project Conventions**: See `.github/instructions/`
3. **Test-Driven**: Write tests first
4. **Incremental**: Small, reviewable commits
5. **Documentation**: Doc comments for all public items

---

## Questions for Implementer

If unsure about any of these, ask for clarification:

1. **Should `cache_dir` in `RuntimeConfig` be deprecated or repurposed?**
   - Current: Optional cache directory
   - Option A: Deprecate, use `CacheManager` default
   - Option B: Use as override for `CacheManager`

2. **Should old `save_skill()` methods be removed or deprecated?**
   - Recommendation: Deprecate in 0.3.0, remove in 0.4.0

3. **Should migration be automatic on first run or manual command?**
   - Recommendation: Manual command for user control

4. **Should cache have TTL or LRU eviction?**
   - Recommendation: Start simple, add later if needed

5. **Should we support custom cache locations via env var?**
   - Recommendation: Yes, `MCP_CACHE_DIR` for testing/CI

---

## Contact

**Architect**: GitHub Copilot (@copilot)  
**For Questions**: Open discussion in PR or issue  
**For Issues**: File bug report with logs and system info

---

## Appendix A: File Changes Summary

| File | Change Type | Description |
|------|------------|-------------|
| `mcp-core/src/cache_manager.rs` | New | Cache management implementation |
| `mcp-core/src/migration.rs` | New | Migration logic |
| `mcp-core/src/lib.rs` | Modified | Export new modules |
| `mcp-skill-store/src/store.rs` | Modified | Add cache-aware methods |
| `mcp-skill-store/src/types.rs` | Modified | Update metadata structure |
| `mcp-codegen/src/wasm/*.rs` | Modified | Write to cache instead of skills dir |
| `mcp-wasm-runtime/src/lib.rs` | Modified | Load WASM from cache |
| `mcp-vfs/src/builder.rs` | Modified | Load VFS from cache |
| `mcp-cli/src/commands/cache.rs` | New | Cache management commands |
| `mcp-cli/src/commands/migrate.rs` | New | Migration command |
| `mcp-cli/src/main.rs` | Modified | Add new commands |
| `docs/ARCHITECTURE.md` | Modified | Document storage architecture |
| `docs/CLAUDE.md` | Modified | User guide updates |
| `docs/MIGRATION.md` | New | Migration guide |
| `tests/integration/cache_separation.rs` | New | Integration tests |

---

## Appendix B: Key Dependencies

No new dependencies should be needed:
- `blake3`: Already in workspace (checksums)
- `chrono`: Already in workspace (timestamps)
- `dirs`: Already in workspace (default directories)
- `walkdir`: Already in workspace (directory traversal)
- `tempfile`: Already in workspace (testing)

---

## Appendix C: Estimated LOC Changes

| Component | New LOC | Modified LOC | Deleted LOC |
|-----------|---------|--------------|-------------|
| mcp-core | +800 | +50 | -0 |
| mcp-skill-store | +300 | +200 | -50 |
| mcp-codegen | +100 | +100 | -0 |
| mcp-wasm-runtime | +50 | +50 | -0 |
| mcp-vfs | +50 | +50 | -0 |
| mcp-cli | +400 | +50 | -0 |
| Tests | +800 | +200 | -0 |
| Docs | +500 | +100 | -0 |
| **Total** | **~3000** | **~800** | **~50** |

---

**END OF IMPLEMENTATION PLAN**
