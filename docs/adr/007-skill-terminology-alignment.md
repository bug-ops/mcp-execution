# ADR-007: Skill Terminology Alignment with Anthropic Specification

**Date**: 2025-11-22
**Status**: Accepted
**Deciders**: MCP Execution Team, Rust Architect
**Related**: ADR-005 (Skill Generation), ADR-006 (Plugin Persistence - TO BE RENAMED)

## Context

### Problem Discovery

During Phase 8.1 implementation, we used the term "plugin" for the persistence layer that saves and loads generated MCP tool code. However, comprehensive analysis of official Anthropic documentation revealed a **critical terminology mismatch**.

**Official Anthropic Documentation** ([Agent Skills Overview](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview)) explicitly defines:

> **Skills**: "Reusable, filesystem-based resources that provide Claude with domain-specific expertise"

> **Skills vs. Plugins**: "Skills are distinct from traditional plugins—Skills integrate filesystem-based resources and progressive disclosure rather than loading all content immediately."

### Current State

Our implementation in Phase 8.1:
- ✅ **IS** filesystem-based (stores VFS + WASM on disk)
- ✅ **DOES** support progressive disclosure (through VFS lazy loading)
- ✅ **INTEGRATES** with Claude Code (native skill system)
- ✅ **FOLLOWS** skill structure (metadata + resources)

But we call it **"plugin"** - which Anthropic explicitly distinguishes as a **different concept**.

### Terminology Inconsistency

**In our codebase:**

| Component | Current Name | Correct Name | Status |
|-----------|-------------|--------------|---------|
| **Crate** | `mcp-plugin-store` | `mcp-skill-store` | ❌ Wrong |
| **Types** | `PluginStore`, `PluginMetadata` | `SkillStore`, `SkillMetadata` | ❌ Wrong |
| **CLI** | `mcp-cli plugin list` | `mcp-cli skill list` | ❌ Wrong |
| **ADR** | ADR-006: "Plugin Persistence" | "Skill Persistence" | ❌ Wrong |
| **Generator** | `mcp-skill-generator` | `mcp-skill-generator` | ✅ Correct |

**Conceptual confusion:**
- Phase 6/ADR-005: "Skill Generation" (generates SKILL.md files)
- Phase 8/ADR-006: "Plugin Persistence" (saves generated code)

These are **the same concept** - both work with Skills! Using two different terms creates architectural confusion.

### Official Skill Specification

**Structure Requirements:**

```yaml
# SKILL.md (mandatory)
---
name: skill-name              # Max 64 chars, lowercase, hyphens
description: "When to use..." # Max 1024 chars
---

# Skill Title

## Instructions
[Step-by-step guidance]
```

**Progressive Disclosure (3 levels):**

| Level | Loading | Token Cost | Content |
|-------|---------|------------|---------|
| Metadata | Always | ~100 tokens | YAML frontmatter |
| Instructions | When triggered | <5k tokens | SKILL.md body |
| Resources | As needed | Minimal | Bundled files/scripts |

**Key Quote:**
> "You can install many Skills without context penalty; Claude only knows each Skill exists and when to use it."

Our implementation **exactly matches** this specification but uses wrong terminology.

## Decision

We will **rename all "plugin" references to "skill"** throughout the codebase to align with official Anthropic specification.

### Scope of Changes

**1. Crate Renaming:**
```
crates/mcp-plugin-store/ → crates/mcp-skill-store/
```

**2. Type Renaming:**
```rust
PluginStore      → SkillStore
PluginMetadata   → SkillMetadata
PluginInfo       → SkillInfo
PluginError      → SkillError
PluginSummary    → SkillSummary
save_plugin()    → save_skill()
load_plugin()    → load_skill()
list_plugins()   → list_skills()
plugin_exists()  → skill_exists()
remove_plugin()  → remove_skill()
verify_plugin()  → verify_skill()
```

**3. CLI Command Renaming:**
```bash
# Old (wrong)
mcp-cli plugin list
mcp-cli plugin load
mcp-cli plugin info
mcp-cli plugin remove

# New (correct)
mcp-cli skill list
mcp-cli skill load
mcp-cli skill info
mcp-cli skill remove
```

**4. Directory Structure:**
```
# Old (wrong)
./plugins/
  └── server-name/
      ├── metadata.json
      ├── vfs.json
      └── module.wasm

# New (correct)
./skills/
  └── skill-name/
      ├── metadata.json
      ├── vfs.json
      └── module.wasm
```

**5. CLI Flags:**
```
--plugin-dir → --skill-dir
```

**6. Documentation:**
- Rename ADR-006: "Plugin Persistence" → "Skill Persistence"
- Update ARCHITECTURE.md, README.md, CHANGELOG.md
- Update all .local/ documentation
- Rename Phase 8 guide

## Rationale

### Why This Matters

**1. Specification Compliance**
- Official Anthropic docs **explicitly differentiate** Skills from Plugins
- Using wrong term violates official specification
- May cause confusion in Anthropic ecosystem

**2. Conceptual Clarity**
- One concept (skills) should have one name
- Current split (skill-generator + plugin-store) obscures that they work with the same thing
- Unified terminology makes architecture clearer

**3. Future Integration**
- Claude Code uses Skills natively
- Correct terminology ensures smooth integration
- Aligns with broader Anthropic ecosystem

**4. Professional Standards**
- Following official specifications is professional best practice
- Demonstrates attention to detail
- Prevents technical debt from terminology mismatch

### Why Rename (vs. Keep "Plugin")

**Alternatives Considered:**

**Option 1: Keep "plugin" terminology**
- ❌ Violates official specification
- ❌ Confuses two distinct concepts (Skills ≠ Plugins)
- ❌ Creates architectural ambiguity
- ❌ May cause integration issues

**Option 2: Rename to "skill" (CHOSEN)**
- ✅ Matches official Anthropic specification
- ✅ Conceptually accurate (we implement Skills, not Plugins)
- ✅ Unifies terminology (skill-generator + skill-store)
- ✅ Future-proof for ecosystem integration
- ⚠️ Requires refactoring (~8-11 hours)

**Decision:** Option 2. Specification compliance and conceptual accuracy outweigh refactoring cost.

### Why Now (vs. Later)

**Timing Considerations:**

**If we rename NOW:**
- ✅ Phase 8.1 not yet merged to master
- ✅ No external users affected (no breaking changes)
- ✅ Team has fresh context
- ✅ Cheaper to fix before release

**If we rename LATER:**
- ❌ Breaking changes for users
- ❌ Complex migration required
- ❌ More code to update
- ❌ Technical debt accumulates

**Decision:** Rename NOW before v0.1.0 release.

## Consequences

### Positive

1. ✅ **Specification Compliance**: Aligns with official Anthropic terminology
2. ✅ **Conceptual Clarity**: One term (skill) for one concept
3. ✅ **Better Integration**: Natural fit with Claude Code skills
4. ✅ **Professional Quality**: Demonstrates adherence to standards
5. ✅ **Future-Proof**: Correct foundation for ecosystem growth
6. ✅ **No Breaking Changes**: Done before v0.1.0 release

### Negative

1. ⚠️ **Refactoring Cost**: ~8-11 hours of development work
2. ⚠️ **Git History**: Some tools may lose tracking (mitigated with `git mv`)
3. ⚠️ **Testing Required**: Full test suite must pass after changes

### Neutral

- Updates 8 crates (mcp-skill-store, mcp-cli, mcp-examples)
- Updates 6 documentation files (ADR-006, ARCHITECTURE, README, etc.)
- Updates ~15-20 files in .local/ documentation
- No external API changes (pre-release)

## Implementation Plan

### Phase 1: Preparation (30 min)

- [x] Create branch `refactor/rename-plugin-to-skill`
- [x] Create ADR-007 (this document)
- [ ] Create detailed plan in `.local/REFACTORING-PLUGIN-TO-SKILL-PLAN.md`

### Phase 2: Code Refactoring (4-6 hours)

**Priority Order:**

1. **Rename crate** (use `git mv` for history preservation):
   ```bash
   git mv crates/mcp-plugin-store crates/mcp-skill-store
   ```

2. **Update Cargo.toml files** (5 files):
   - `Cargo.toml` (workspace)
   - `crates/mcp-skill-store/Cargo.toml`
   - `crates/mcp-cli/Cargo.toml`
   - `crates/mcp-examples/Cargo.toml`

3. **Refactor types in mcp-skill-store** (~30 types/functions):
   - All struct/enum names
   - All function names
   - All module documentation

4. **Rename CLI module**:
   ```bash
   git mv crates/mcp-cli/src/commands/plugin.rs \
          crates/mcp-cli/src/commands/skill.rs
   ```

5. **Update CLI integration**:
   - `main.rs`: Command enum
   - `commands/mod.rs`: Module declaration
   - All help text and messages

6. **Rename examples**:
   ```bash
   git mv crates/mcp-examples/examples/plugin_workflow.rs \
          crates/mcp-examples/examples/skill_workflow.rs
   ```

7. **Update default directories**:
   - CLI flags: `--plugin-dir` → `--skill-dir`
   - Default paths: `./plugins` → `./skills`

### Phase 3: Documentation (2-3 hours)

1. **Rename ADR-006**:
   ```bash
   git mv docs/adr/006-plugin-persistence.md \
          docs/adr/006-skill-persistence.md
   ```

2. **Update core documentation** (3 files):
   - `docs/ARCHITECTURE.md`
   - `README.md`
   - `CHANGELOG.md`

3. **Update .local/ documentation** (~15 files):
   - Rename `PHASE-8-PLUGIN-PERSISTENCE-GUIDE.md` → `PHASE-8-SKILL-PERSISTENCE-GUIDE.md`
   - Update INDEX.md
   - Update PROJECT-STATUS.md
   - Update security/performance audits

### Phase 4: Verification (1-2 hours)

**Testing:**
```bash
# All tests must pass
cargo nextest run --workspace

# Doc tests
cargo test --doc --workspace

# Linting
cargo clippy --workspace -- -D warnings

# Formatting
cargo +nightly fmt --workspace --check
```

**Performance verification** (with rust-performance-engineer):
- Benchmark suite must show no regressions
- Load times should remain <3ms

**Security audit** (with rust-security-maintenance):
- No new vulnerabilities introduced
- Path validation still works
- Checksum verification intact

**Test coverage** (with rust-testing-engineer):
- Coverage should remain >90%
- All edge cases covered

**Code review** (with rust-code-reviewer):
- Naming consistency check
- Documentation completeness
- No breaking changes

### Phase 5: Migration Support (1 hour)

**Optional CLI migration command** (if users have existing data):

```rust
// mcp-skill-store/src/migration.rs
pub fn migrate_from_plugin_dir(
    old_dir: &Path,  // ./plugins
    new_dir: &Path,  // ./skills
) -> Result<MigrationReport>
```

```bash
mcp-cli skill migrate --from ./plugins --to ./skills
```

This is **optional** since we're pre-v0.1.0 release.

## Verification Checklist

Before merging to master:

- [ ] All tests passing (cargo nextest run --workspace)
- [ ] No clippy warnings (cargo clippy --workspace -- -D warnings)
- [ ] Formatted correctly (cargo +nightly fmt --workspace --check)
- [ ] No "plugin" references in new code (grep verification)
- [ ] All "skill" references correct (grep verification)
- [ ] Documentation updated (8+ files)
- [ ] ADR-006 renamed and updated
- [ ] Performance benchmarks pass (rust-performance-engineer)
- [ ] Security audit clean (rust-security-maintenance)
- [ ] Test coverage >90% (rust-testing-engineer)
- [ ] Code review approved (rust-code-reviewer)

## Success Criteria

**Must Have:**
1. ✅ All `Plugin*` types renamed to `Skill*`
2. ✅ Crate renamed: `mcp-plugin-store` → `mcp-skill-store`
3. ✅ CLI commands: `plugin` → `skill`
4. ✅ All tests passing
5. ✅ No clippy warnings
6. ✅ Documentation consistent

**Nice to Have:**
- ✅ Migration command for existing users
- ✅ Updated examples
- ✅ Performance benchmarks unchanged

## Related Decisions

- **ADR-001**: Multi-Crate Workspace (establishes crate separation)
- **ADR-003**: Strong Types Over Primitives (type naming matters!)
- **ADR-005**: Claude Code Skill Generation (correct terminology used)
- **ADR-006**: Plugin Persistence Design (TO BE RENAMED to Skill Persistence)

## References

### Official Anthropic Documentation
- [Agent Skills Overview](https://platform.claude.com/docs/en/agents-and-tools/agent-skills/overview)
- [Code Execution with MCP](https://www.anthropic.com/engineering/code-execution-with-mcp)

### Internal Documentation
- ADR-005: Claude Code Skill Generation
- ADR-006: Plugin Persistence Design (to be renamed)

### Key Quotes

**From Anthropic Documentation:**
> "Skills are reusable, filesystem-based resources that provide Claude with domain-specific expertise"

> "Skills are distinct from traditional plugins—Skills integrate filesystem-based resources and progressive disclosure"

**Why This Matters:**
Our implementation IS a Skill by official definition. Using "plugin" terminology is factually incorrect per specification.

---

**Decision Status**: ✅ Accepted
**Implementation**: Refactoring in progress (Branch: `refactor/rename-plugin-to-skill`)
**Target Completion**: 2025-11-22
**Review Required**: Yes (rust-code-reviewer before merge)
**Breaking Changes**: None (pre-v0.1.0 release)

---
