# ADR-012: Claude Agent SDK Code Generator

## Status

**Proposed** (2025-01-27)

Extends: ADR-010 (Progressive Loading)

## Context

The MCP Code Execution project currently generates TypeScript files for progressive loading pattern. This enables Claude Code to discover and load MCP tools on-demand, achieving 98% token savings.

### New Requirement

Anthropic has released the [Claude Agent SDK](https://platform.claude.com/docs/en/agent-sdk/typescript), which provides a programmatic way to build AI agents using Claude. The SDK includes support for custom MCP tools via the `createSdkMcpServer()` and `tool()` functions.

### Claude Agent SDK Tool Format

The SDK uses Zod schemas for type-safe tool definitions:

```typescript
import { createSdkMcpServer, tool } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";

const server = createSdkMcpServer({
  name: "my-server",
  version: "1.0.0",
  tools: [
    tool(
      "tool_name",
      "Tool description",
      {
        param1: z.string().describe("Parameter description"),
        param2: z.number().optional()
      },
      async (args) => {
        // Implementation
        return {
          content: [{ type: "text", text: "Result" }]
        };
      }
    )
  ]
});
```

### Opportunity

We can extend our code generation to produce Claude Agent SDK-compatible TypeScript, allowing users to:

1. Integrate MCP tools directly into their Claude Agent SDK applications
2. Use type-safe Zod schemas instead of raw JSON Schema
3. Build custom agents that combine MCP tools with business logic

## Decision

**Add a new `claude_agent` generator module alongside the existing `progressive` module.**

### Architecture

```
crates/mcp-codegen/
├── src/
│   ├── lib.rs                    # Add: pub mod claude_agent;
│   ├── progressive/              # Existing (unchanged)
│   └── claude_agent/             # NEW
│       ├── mod.rs
│       ├── generator.rs          # ClaudeAgentGenerator
│       └── types.rs              # Context types for templates
└── templates/
    ├── progressive/              # Existing (unchanged)
    └── claude_agent/             # NEW
        ├── tool.ts.hbs           # Individual tool definition
        ├── server.ts.hbs         # Complete MCP server
        └── index.ts.hbs          # Entry point with exports
```

### Generated Output Structure

For a server with 3 tools, generates:

```
~/.claude/agent-sdk/{server-id}/
├── index.ts              # Entry point, exports server and types
├── server.ts             # createSdkMcpServer() with all tools
├── tools/
│   ├── createIssue.ts   # Individual tool with Zod schema
│   ├── updateIssue.ts
│   └── deleteIssue.ts
└── types.ts              # Shared type definitions
```

### Template Design

#### Individual Tool (`tools/*.ts`)

```typescript
import { tool } from "@anthropic-ai/claude-agent-sdk";
import { z } from "zod";

/**
 * {{description}}
 */
export const {{typescript_name}} = tool(
  "{{name}}",
  "{{description}}",
  {
{{#each properties}}
    {{name}}: z.{{zod_type}}(){{#if description}}.describe("{{description}}"){{/if}}{{#unless required}}.optional(){{/unless}},
{{/each}}
  },
  async (args) => {
    // TODO: Implement tool logic
    // This is a stub - actual implementation depends on MCP bridge
    return {
      content: [{
        type: "text" as const,
        text: JSON.stringify({ tool: "{{name}}", args })
      }]
    };
  }
);

export type {{pascal_name}}Args = z.infer<typeof {{typescript_name}}.inputSchema>;
```

#### Server File (`server.ts`)

```typescript
import { createSdkMcpServer } from "@anthropic-ai/claude-agent-sdk";
{{#each tools}}
import { {{typescript_name}} } from "./tools/{{typescript_name}}";
{{/each}}

export const {{server_variable_name}}Server = createSdkMcpServer({
  name: "{{server_name}}",
  version: "{{server_version}}",
  tools: [
{{#each tools}}
    {{typescript_name}},
{{/each}}
  ]
});
```

### Key Components

1. **ClaudeAgentGenerator**: Main generator class
   - Converts `ServerInfo` → `GeneratedCode`
   - Uses Handlebars templates
   - Parallel to `ProgressiveGenerator`

2. **JSON Schema → Zod Mapping**:
   - `string` → `z.string()`
   - `number` → `z.number()`
   - `integer` → `z.number().int()`
   - `boolean` → `z.boolean()`
   - `array` → `z.array(z.unknown())`
   - `object` → `z.object({...})`
   - Enum support: `z.enum([...])`
   - Format hints: `.email()`, `.url()`, etc.

3. **CLI Integration**:
   - New `--format claude-agent` flag for `generate` command
   - Default remains `progressive`

## Consequences

### Positive

1. **Extended Ecosystem Support**:
   - Users can integrate MCP tools into Claude Agent SDK applications
   - Type-safe tool definitions with Zod

2. **Code Reuse**:
   - Shares `ServerInfo` from `mcp-introspector`
   - Shares `GeneratedCode`, `GeneratedFile` from `common`
   - Shares `TemplateEngine` infrastructure

3. **Minimal Complexity Increase**:
   - New module parallel to existing
   - No changes to progressive loading
   - Clear separation of concerns

4. **Better Developer Experience**:
   - Zod provides runtime validation
   - TypeScript inference from schemas
   - IDE autocomplete for tool arguments

### Negative

1. **Increased Maintenance**:
   - New templates to maintain
   - JSON Schema → Zod mapping logic
   - Additional integration tests

2. **Dependency on External SDK**:
   - Generated code requires `@anthropic-ai/claude-agent-sdk`
   - Generated code requires `zod`

3. **Implementation Stub**:
   - Generated tool handlers are stubs
   - Actual MCP bridge integration TBD

### Neutral

1. **Output Location**:
   - Uses `~/.claude/agent-sdk/` instead of `~/.claude/servers/`
   - Clear separation from progressive loading output

## Implementation Plan

### Phase 1: Core Generator

1. Create `claude_agent` module structure
2. Implement `ClaudeAgentGenerator`
3. Create Handlebars templates
4. Add JSON Schema → Zod type mapping

### Phase 2: CLI Integration

1. Add `--format` flag to `generate` command
2. Support `progressive` (default) and `claude-agent`
3. Update help text and documentation

### Phase 3: Testing

1. Unit tests for generator
2. Unit tests for Zod type mapping
3. Integration tests with real MCP servers

### Phase 4: Documentation

1. Update README with new format
2. Add usage examples
3. Document generated file structure

## Alternatives Considered

### Alternative 1: Modify Progressive Loading

Modify existing progressive generator to output Claude Agent SDK format.

**Pros**: Less code duplication
**Cons**: Conflates two different output formats, harder to maintain

**Rejected**: Clean separation is better for maintainability.

### Alternative 2: External Post-Processor

Create external tool that transforms progressive output to Claude Agent SDK format.

**Pros**: Decoupled, could be separate npm package
**Cons**: Extra step, harder to maintain consistency

**Rejected**: Integrated solution provides better user experience.

### Alternative 3: Generate Only Tool Definitions

Generate only the Zod schemas, let users create server.ts manually.

**Pros**: Simpler, more flexible
**Cons**: More work for users, less value add

**Rejected**: Full generation provides more value.

## References

- [Claude Agent SDK - TypeScript](https://platform.claude.com/docs/en/agent-sdk/typescript)
- [Claude Agent SDK - Custom Tools](https://platform.claude.com/docs/en/agent-sdk/custom-tools)
- [Zod Documentation](https://zod.dev/)
- [ADR-010: Progressive Loading Only](./010-simplify-to-progressive-only.md)
