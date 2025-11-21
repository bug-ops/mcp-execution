# ADR-006: Plugin Persistence Design

**Date**: 2025-11-21
**Status**: Accepted (Implemented in Phase 8.1)
**Deciders**: MCP Execution Team
**Related**: Phase 8.1 Plugin Persistence Implementation

## Context

After completing code generation (Phase 3) and WASM runtime (Phase 4), we observed that generating and compiling code for the same MCP server repeatedly wastes significant resources:

**Problem**:
- Introspection takes 50-100ms per server
- Code generation takes 1-5ms for typical servers
- WASM compilation takes 15-20ms per module
- **Total**: 66-125ms overhead on every invocation

For frequently-used servers, this overhead is unnecessary:
- Same server = same tools = same generated code
- WASM modules are deterministic (same input → same output)
- Users shouldn't wait for regeneration every time

**Requirements**:
1. Save generated code and WASM modules to disk
2. Load plugins faster than generating from scratch
3. Verify integrity (prevent tampering)
4. Support multiple servers
5. Handle updates (server adds/removes tools)
6. Cross-platform compatibility (Linux, macOS, Windows)
7. Security: no arbitrary code execution

## Decision

We will implement **disk-based plugin persistence** with the following design:

### Storage Structure

```
plugins/
└── <server-name>/          # e.g., "vkteams-bot"
    ├── metadata.json       # PluginMetadata (server info, timestamps, version)
    ├── vfs.json            # Complete VFS structure (all generated code)
    ├── module.wasm         # Compiled WASM module
    └── checksum.blake3     # Blake3 checksum of all files
```

**Rationale**:
- **Directory per server**: Natural isolation, easy to list/remove
- **JSON format**: Human-readable metadata, easy debugging
- **Separate WASM file**: Binary data separate from text
- **Blake3 checksum**: Fast (10x faster than SHA256), cryptographically secure

### Component: mcp-plugin-store

**New crate** `mcp-plugin-store` with the following API:

```rust
pub struct PluginStore {
    base_dir: PathBuf,  // e.g., "./plugins" or "~/.mcp/plugins"
}

impl PluginStore {
    pub fn new(base_dir: PathBuf) -> Result<Self>;

    // Save plugin to disk
    pub async fn save(
        &self,
        server_name: &str,
        metadata: PluginMetadata,
        vfs: &VirtualFilesystem,
        wasm_module: &[u8],
    ) -> Result<()>;

    // Load plugin from disk
    pub async fn load(&self, server_name: &str) -> Result<Plugin>;

    // List all saved plugins
    pub async fn list(&self) -> Result<Vec<PluginInfo>>;

    // Get plugin metadata
    pub async fn info(&self, server_name: &str) -> Result<PluginMetadata>;

    // Remove plugin from disk
    pub async fn remove(&self, server_name: &str) -> Result<()>;

    // Verify plugin integrity
    pub async fn verify(&self, server_name: &str) -> Result<bool>;
}
```

### Security Design

**Integrity Verification**:
```rust
// Blake3 checksum of metadata + vfs + wasm
let checksum = {
    let mut hasher = Blake3::new();
    hasher.update(metadata_json.as_bytes());
    hasher.update(vfs_json.as_bytes());
    hasher.update(wasm_bytes);
    hasher.finalize()
};
```

**Constant-Time Comparison**:
```rust
use secrecy::ExposeSecret;
use subtle::ConstantTimeEq;

// Prevent timing attacks on checksum verification
let expected = stored_checksum.expose_secret();
let actual = calculated_checksum;
let valid = expected.ct_eq(&actual).into();
```

**Path Validation**:
```rust
// Prevent directory traversal
fn validate_server_name(name: &str) -> Result<()> {
    if name.contains("..") || name.contains('/') || name.contains('\\') {
        return Err(Error::InvalidPluginName);
    }
    Ok(())
}
```

**File Permissions**:
- Metadata: `0o644` (read-write owner, read-only others)
- Checksums: `0o600` (read-write owner only)
- WASM modules: `0o644` (executable content but read-only)

**Atomic Operations**:
```rust
// Write to temporary file first, then atomic rename
let temp_path = path.with_extension(".tmp");
fs::write(&temp_path, data)?;
fs::rename(&temp_path, &path)?;  // Atomic on POSIX
```

### CLI Integration

```bash
# Generate and save plugin
$ mcp-cli generate vkteams-bot --save-plugin --plugin-dir ./plugins

# List saved plugins
$ mcp-cli plugin list
vkteams-bot  saved 2025-11-21  12 tools  1.2MB

# Load and use plugin (skip generation)
$ mcp-cli plugin load vkteams-bot

# Get plugin info
$ mcp-cli plugin info vkteams-bot

# Remove plugin
$ mcp-cli plugin remove vkteams-bot
```

### Performance Benefits

**Before** (no persistence):
```
Introspection: 50ms
Code Generation: 2ms
WASM Compilation: 15ms
Total: 67ms
```

**After** (with persistence):
```
Plugin Load: 1-3ms
Integrity Check: <1ms
Total: ~2-4ms
```

**Speedup**: ~16-33x faster for cached plugins

## Alternatives Considered

### Alternative 1: In-Memory Cache Only

**Pros**:
- Simpler implementation
- No disk I/O
- No file permission issues

**Cons**:
- ❌ Lost on process restart
- ❌ No benefit across sessions
- ❌ Wastes resources on frequent restarts

**Decision**: Rejected - users would still regenerate frequently

### Alternative 2: SQLite Database

**Pros**:
- Transactional guarantees
- Query capabilities
- Compact storage

**Cons**:
- ❌ Adds dependency (rusqlite)
- ❌ Overkill for simple key-value storage
- ❌ Harder to debug (binary format)
- ❌ Migration complexity

**Decision**: Rejected - too complex for simple use case

### Alternative 3: Single File with All Plugins

**Example**: `plugins.tar` or `plugins.zip`

**Pros**:
- Single file to manage
- Natural compression

**Cons**:
- ❌ Must rewrite entire file for updates
- ❌ Larger blast radius for corruption
- ❌ Harder to remove individual plugins
- ❌ Concurrent access issues

**Decision**: Rejected - directory-per-plugin is simpler

### Alternative 4: Git Repository for Plugins

**Pros**:
- Version history
- Diff capabilities
- Branching/merging

**Cons**:
- ❌ Massive overkill
- ❌ Requires git dependency
- ❌ Complex for non-developers
- ❌ Unnecessary overhead

**Decision**: Rejected - not a version control problem

### Alternative 5: No Checksum Verification

**Pros**:
- Simpler code
- Slightly faster

**Cons**:
- ❌ No integrity guarantees
- ❌ Tampering undetected
- ❌ Corruption undetected
- ❌ Security vulnerability

**Decision**: Rejected - security is critical

## Consequences

### Positive

✅ **Faster plugin loading**: 16-33x faster than regeneration
✅ **Persistent across sessions**: No re-generation needed
✅ **Simple file-based storage**: Easy to debug, backup, share
✅ **Strong integrity guarantees**: Blake3 checksums prevent tampering
✅ **Security hardened**: Constant-time comparison, path validation, atomic operations
✅ **Cross-platform**: Works on Linux, macOS, Windows
✅ **Minimal dependencies**: Only `blake3` and `secrecy` added
✅ **Human-readable metadata**: JSON format easy to inspect

### Negative

⚠️ **Disk space usage**: ~1-5MB per plugin (acceptable tradeoff)
⚠️ **Stale plugins**: User must manually update if server changes (documented)
⚠️ **File system dependency**: Requires writable directory (most systems OK)

### Neutral

- Adds new crate `mcp-plugin-store` (~800 LOC)
- Adds 70 tests (38 unit + 32 integration)
- Requires user to manage plugin directory

## Implementation Notes

### Phase 8.1 Deliverables

**Completed**:
- [x] `mcp-plugin-store` crate with full API
- [x] Blake3 checksum generation and verification
- [x] Constant-time comparison for security
- [x] Path validation and sanitization
- [x] Atomic file operations
- [x] CLI integration (`plugin` subcommand)
- [x] 38 unit tests (>90% coverage)
- [x] 32 integration tests (E2E workflows)
- [x] Security audit (5/5 stars, zero vulnerabilities)
- [x] Documentation (`PHASE-8-PLUGIN-PERSISTENCE-GUIDE.md`)
- [x] E2E example (`plugin_workflow.rs`)

### Testing Strategy

**Unit Tests** (38 tests):
- File I/O operations
- Checksum generation/verification
- Path validation
- Metadata serialization
- Error handling

**Integration Tests** (32 tests):
- Full save/load cycle
- Multiple plugins
- Concurrent access
- Corruption detection
- Platform-specific edge cases

**Security Tests**:
- Directory traversal attempts
- Checksum tampering detection
- Timing attack resistance (constant-time ops)
- File permission validation

### Performance Validation

**Benchmarks** (on M1 MacBook Pro):
```
Plugin Save:     2.3ms ± 0.5ms
Plugin Load:     1.8ms ± 0.3ms
Checksum Calc:   0.6ms ± 0.1ms
Integrity Check: 0.9ms ± 0.2ms
```

**Speedup vs Regeneration**: 16-33x faster

### Security Audit Results

**Rating**: 5/5 stars

**Findings**:
- ✅ Zero critical vulnerabilities
- ✅ Zero high-severity issues
- ✅ Zero medium-severity issues
- ✅ Constant-time operations prevent timing attacks
- ✅ Path validation prevents directory traversal
- ✅ Blake3 provides strong integrity guarantees
- ✅ Atomic operations prevent corruption

## Future Considerations

### Potential Enhancements (Not in Phase 8.1)

1. **Compression**: gzip/zstd for smaller plugin files
2. **Encryption**: Encrypt WASM modules with user key
3. **Signatures**: Digital signatures for plugin authenticity
4. **Remote storage**: S3/HTTP backend for shared plugins
5. **Version management**: Keep multiple versions of same plugin
6. **Auto-update**: Detect server changes and regenerate
7. **Plugin sharing**: Export/import plugins between machines

**Decision**: Deferred until user demand exists

## References

### Internal Documents
- `.local/PHASE-8-PLUGIN-PERSISTENCE-GUIDE.md` - User guide
- `.local/plugin-persistence-design.md` - Detailed design
- `.local/SECURITY-AUDIT-PLUGIN-STORE.md` - Security audit
- `.local/PERFORMANCE-REVIEW-PLUGIN-STORE.md` - Performance analysis

### Code
- `crates/mcp-plugin-store/` - Implementation
- `crates/mcp-cli/src/commands/plugin.rs` - CLI integration
- `crates/mcp-examples/examples/plugin_workflow.rs` - E2E example

### External References
- [Blake3](https://github.com/BLAKE3-team/BLAKE3) - Fast cryptographic hash
- [Secrecy](https://docs.rs/secrecy) - Protecting sensitive data
- [Subtle](https://docs.rs/subtle) - Constant-time operations

---

**Decision Status**: ✅ Accepted and Implemented
**Implementation**: Phase 8.1 (November 2025)
**Review Date**: After v0.1.0 release (evaluate for enhancements)
