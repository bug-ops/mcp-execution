# ADR-005: Claude Code Skill Generation Architecture

## Status

Proposed

## Context

Claude Code supports skills defined in `.claude/skills/` directories. Skills are SKILL.md files containing YAML frontmatter and Markdown content that extend Claude's capabilities. MCP servers expose similar functionality through tool definitions but require manual skill creation.

**Problem**: Creating Claude Code skills from MCP servers is manual, time-consuming, and error-prone:
- Extracting tool schemas from MCP servers requires manual inspection
- Writing SKILL.md files requires understanding both MCP and Claude Code formats
- Keeping skills synchronized with server changes requires manual updates
- Tool parameter documentation must be manually extracted from JSON schemas

**Opportunity**: The mcp-execution project already has infrastructure for MCP server introspection (mcp-introspector) and code generation (mcp-codegen). We can leverage these to automatically generate Claude Code skills from any MCP server.

**Requirements**:
1. Generate valid Claude Code SKILL.md files from MCP server definitions
2. Support multiple skill formats (minimal, comprehensive)
3. Validate generated skills against Claude Code requirements
4. Provide CLI interface for easy usage
5. Maintain type safety throughout the generation process
6. Integrate seamlessly with existing workspace architecture

## Decision

We will implement a new `mcp-skill-generator` crate that automatically generates Claude Code skills from MCP server information discovered by `mcp-introspector`.

### Architecture Components

**1. New Crate: mcp-skill-generator**

Separate crate with responsibilities:
- Skill template rendering (Handlebars)
- Metadata extraction and formatting
- Skill validation
- File generation and installation

**2. Core Types**

```rust
pub struct SkillName(String);  // Validated skill name
pub struct GeneratedSkill;      // Generated skill + metadata
pub struct SkillTemplateContext; // Template rendering context
pub struct ToolDocumentation;   // Structured tool docs
```

**3. Template System**

Two built-in templates:
- **Basic**: Minimal skill (tool list + parameters)
- **Advanced**: Comprehensive skill (examples + troubleshooting)

Users can provide custom templates.

**4. CLI Integration**

Extend `mcp-cli` with skill subcommand:
```bash
mcp-cli skill generate <server>  # Generate to stdout
mcp-cli skill install <server>   # Generate + install
mcp-cli skill validate <path>    # Validate existing
mcp-cli skill list               # List installed
```

**5. Validation**

Two-layer validation:
- Compile-time: `SkillName` type enforces naming rules
- Runtime: `SkillValidator` validates SKILL.md format

### Data Flow

```
User Input: MCP Server Command/URI
         ↓
    mcp-introspector::discover_server()
         ↓ (ServerInfo)
    mcp-skill-generator::generate()
         ↓ (GeneratedSkill)
    Write to ~/.claude/skills/<name>/SKILL.md
```

### Integration Points

**Reuse Existing Infrastructure**:
- **mcp-introspector**: Server discovery and tool extraction (input)
- **mcp-codegen**: Template engine pattern (architectural reuse)
- **mcp-core**: Types, errors, validation utilities (extend)
- **mcp-cli**: User interface (extend with subcommand)

**New Dependencies**:
- `regex` (1.11): Skill name validation
- `convert_case` (0.6): Name normalization (snake_case → kebab-case)

### Template Design

Skills follow Claude Code format:

```markdown
---
name: skill-name
description: Brief description (max 1024 chars)
---

# Skill Title

## Instructions

Clear, step-by-step guidance...

## Available Tools

### tool_name

Description and parameters...
```

Templates use Handlebars with custom helpers:
- Markdown escaping
- Parameter table formatting
- Example formatting

## Rationale

### Why Separate Crate?

**Pros**:
- Clear separation of concerns
- Independent versioning and evolution
- Clean dependency graph (no circular dependencies)
- Can be published independently if desired
- Faster incremental compilation

**Cons**:
- Additional crate to maintain
- Slightly more boilerplate

**Decision**: Benefits outweigh costs. Separation aligns with workspace design philosophy (ADR-001).

### Why Template-Based Generation?

**Alternatives Considered**:

1. **String Concatenation**: Rejected - inflexible, hard to maintain
2. **AST-Based Generation** (syn/quote): Rejected - overkill for Markdown
3. **Handlebars Templates**: Chosen

**Pros**:
- Flexible and customizable
- Separation of logic and presentation
- Users can provide custom templates
- Reuses proven pattern from mcp-codegen
- Logic-less templates prevent security issues

**Cons**:
- Template syntax to learn
- Runtime template errors possible

**Decision**: Templates provide best balance of flexibility and maintainability.

### Why Strong Types (SkillName)?

**Alternatives Considered**:

1. **Plain String**: Rejected - error-prone, no validation
2. **Runtime Validation Only**: Rejected - errors caught late
3. **SkillName Type**: Chosen

**Pros**:
- Compile-time enforcement of naming rules
- Self-documenting API
- Centralized validation logic
- Type system prevents invalid states
- Follows Microsoft Rust Guidelines (ADR-003)

**Cons**:
- Slightly more verbose API
- Conversion overhead (minimal)

**Decision**: Type safety is paramount. Aligns with project philosophy.

### Why CLI Integration in mcp-cli?

**Alternatives Considered**:

1. **Separate Binary**: Rejected - fragmented UX
2. **Library Only**: Rejected - not user-facing
3. **Subcommand in mcp-cli**: Chosen

**Pros**:
- Consistent user experience
- Reuses existing CLI infrastructure
- Natural discoverability (`mcp-cli --help`)
- Enables workflow integration

**Cons**:
- Increases mcp-cli scope
- Tighter coupling

**Decision**: UX consistency justifies coupling.

### Why Multiple Templates?

**Alternatives Considered**:

1. **Single Template**: Rejected - doesn't fit all use cases
2. **Highly Configurable Template**: Rejected - too complex
3. **Multiple Templates**: Chosen

**Pros**:
- Basic template for simple servers
- Advanced template for production use
- Users choose based on needs
- Custom templates for special cases

**Cons**:
- Template maintenance burden
- More testing required

**Decision**: Flexibility is worth the cost.

## Consequences

### Positive

1. **90% Time Savings**: Automates manual skill creation process
2. **Consistency**: All generated skills follow same format
3. **Automatic Updates**: Regenerate when server changes
4. **Type Safety**: Compile-time validation prevents errors
5. **Reuses Infrastructure**: Leverages existing mcp-introspector and patterns
6. **Extensible**: Users can provide custom templates
7. **Testable**: Clear interfaces enable comprehensive testing
8. **Documentation**: Auto-generates tool documentation from schemas

### Negative

1. **Maintenance Burden**: New crate to maintain
2. **Template Maintenance**: Templates need updates when Claude Code format changes
3. **Edge Cases**: Generated skills may need manual tweaks for complex servers
4. **Dependency Addition**: Two new dependencies (regex, convert_case)
5. **Learning Curve**: Users need to understand templates for customization

### Risks and Mitigations

**Risk**: Claude Code changes SKILL.md format
**Mitigation**: Version templates, document format assumptions, provide migration guide

**Risk**: Generated skills have errors
**Mitigation**: Comprehensive validation, extensive testing with fixtures, real-world testing with vkteams-bot

**Risk**: Performance issues with large servers
**Mitigation**: Set limits (max 100 tools), implement timeouts, profile and optimize

**Risk**: Security issues (path traversal, command injection)
**Mitigation**: Reuse `validate_command()`, sanitize paths, validate all inputs, security audit

## Implementation Plan

**Timeline**: 2-3 weeks (86 hours)

### Phase 6.1: Foundation (16h)
- Create crate structure
- Implement core types
- Add error types
- Unit tests

### Phase 6.2: Templates (14h)
- Template engine
- Basic/Advanced templates
- Metadata builder
- Context conversion

### Phase 6.3: Generator (14h)
- SkillGenerator implementation
- File writing utilities
- Integration tests

### Phase 6.4: Validator (8h)
- SkillValidator implementation
- Validation rules
- Error reporting

### Phase 6.5: CLI (14h)
- CLI subcommands
- User interaction
- Progress indicators

### Phase 6.6: Examples & Docs (12h)
- Working examples
- Documentation
- Integration guides

### Phase 6.7: Testing & Polish (8h)
- Comprehensive tests
- Performance testing
- Code review

## Alternatives Considered

### Alternative 1: Embed in mcp-codegen

**Description**: Add skill generation to existing mcp-codegen crate

**Pros**:
- Fewer crates
- Shared template engine

**Cons**:
- Violates single responsibility
- Skill generation semantically different from TypeScript generation
- Would blur crate boundaries

**Rejection Reason**: Architectural clarity more important than crate count.

### Alternative 2: No Validation

**Description**: Generate skills without validation, trust templates

**Pros**:
- Simpler implementation
- Fewer dependencies

**Cons**:
- Silent failures possible
- No feedback on invalid skills
- Harder to debug issues

**Rejection Reason**: Validation is critical for quality and UX.

### Alternative 3: Manual Skill Creation

**Description**: Don't implement automation, document manual process

**Pros**:
- No code to maintain
- Maximum flexibility

**Cons**:
- High user effort
- Error-prone
- Doesn't scale
- Misses opportunity for automation

**Rejection Reason**: Automation is core value proposition.

## Related Decisions

- **ADR-001**: Multi-Crate Workspace (establishes crate separation pattern)
- **ADR-003**: Strong Types Over Primitives (establishes type safety pattern)
- **ADR-004**: Use rmcp Official SDK (provides MCP integration layer)

## References

- [Claude Code Skills Documentation](https://docs.claude.com/en/docs/claude-code/skills)
- [Claude Code Skill Structure](https://mikhail.io/2025/10/claude-code-skills/)
- [MCP Specification](https://spec.modelcontextprotocol.io/)
- [Handlebars Documentation](https://handlebarsjs.com/)
- [Full Architecture](.local/skill-generator-architecture.md)

## Notes

### Skill Naming Rules (Claude Code)

- Lowercase letters, numbers, hyphens only
- Max 64 characters
- Must start with letter
- Example: `vkteams-bot`, `github-api`, `slack-integration`

### SKILL.md Format

Two required fields in YAML frontmatter:
- `name`: Skill identifier
- `description`: Brief description (max 1024 chars)

Progressive disclosure: Claude loads only name/description at startup, full content on demand.

### Future Enhancements

Potential future additions (not in scope for initial release):
- Multi-server skills (composite)
- Interactive generation (TUI)
- Skill marketplace/registry
- Version management
- Live sync with server changes

---

**Decision Date**: 2025-11-13
**Authors**: Rust Project Architect
**Reviewers**: TBD
**Status**: Awaiting Review
