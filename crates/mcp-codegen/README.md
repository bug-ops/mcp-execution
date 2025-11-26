# mcp-codegen

Progressive loading TypeScript code generation for MCP (Model Context Protocol) tools.

Transforms MCP tool schemas into TypeScript files using the progressive loading pattern, achieving 98% token savings by generating one file per tool.

## Features

- **One File Per Tool**: Separate TypeScript file for each MCP tool
- **Type-Safe Interfaces**: Full TypeScript parameter and result types
- **JSDoc Documentation**: Complete documentation from MCP schemas
- **98% Token Savings**: Load only the tools you need
- **Virtual Filesystem**: Optional VFS integration for in-memory generation

## Installation

```toml
[dependencies]
mcp-codegen = "0.4"
```

## Usage

### Progressive Loading Generation

```rust
use mcp_codegen::progressive::ProgressiveGenerator;
use mcp_introspector::{Introspector, ServerInfo};
use mcp_core::{ServerId, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Introspect MCP server
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("github");
    let config = ServerConfig::builder()
        .command("github-mcp-server".to_string())
        .build();
    let info = introspector.discover_server(server_id, &config).await?;

    // 2. Generate progressive loading files
    let generator = ProgressiveGenerator::new()?;
    let code = generator.generate(&info)?;

    // Files generated:
    // - createIssue.ts (one tool)
    // - updateIssue.ts (another tool)
    // - index.ts (re-exports all)
    // - _runtime/mcp-bridge.ts (runtime helper)

    println!("Generated {} files", code.file_count());
    Ok(())
}
```

### Token Savings Example

**Traditional approach** (load all tools):
```typescript
// index.ts - contains all 45 tools
// Token cost: ~30,000 tokens
```

**Progressive loading** (load one tool):
```typescript
// createIssue.ts - one tool only
// Token cost: ~500-1,500 tokens
// Savings: 98%! 🎉
```

## Generated TypeScript Structure

Each tool file includes:

```typescript
/**
 * Creates a new issue in a GitHub repository
 * @param params - Tool parameters
 * @returns Tool execution result
 */
export async function createIssue(
  params: CreateIssueParams
): Promise<CreateIssueResult> {
  return await callMCPTool('github', 'create_issue', params);
}

/** Parameters for createIssue tool */
export interface CreateIssueParams {
  /** Repository in format "owner/repo" */
  repo: string;

  /** Issue title */
  title: string;

  /** Issue body (optional) */
  body?: string;

  /** Labels to apply (optional) */
  labels?: string[];
}

/** Result type for createIssue tool */
export interface CreateIssueResult {
  [key: string]: unknown;
}
```

## Architecture

### Code Generation Pipeline

1. **Introspection**: MCP server analysis using `mcp-introspector`
2. **Template Rendering**: Handlebars templates for each tool
3. **Type Conversion**: JSON Schema → TypeScript interfaces
4. **File Generation**: One `.ts` file per tool + index + runtime
5. **Output**: VFS structure or disk files

### Template System

```
templates/
└── progressive/              # Progressive loading templates
    ├── tool.ts.hbs          # Individual tool template
    ├── index.ts.hbs         # Re-exports all tools
    └── runtime-bridge.ts.hbs # Runtime helper (stub)
```

### Type Conversion

JSON Schema types are converted to TypeScript:

| JSON Schema | TypeScript |
|-------------|------------|
| `string` | `string` |
| `number` | `number` |
| `boolean` | `boolean` |
| `array` | `T[]` |
| `object` | `{ [key: string]: T }` |

Optional parameters use `?` suffix in TypeScript interfaces.

## Integration

### With Virtual Filesystem

```rust
use mcp_vfs::VirtualFilesystem;
use mcp_codegen::progressive::ProgressiveGenerator;

let generator = ProgressiveGenerator::new()?;
let code = generator.generate(&server_info)?;

// Export to VFS
let mut vfs = VirtualFilesystem::new();
vfs.add_directory("github")?;
for (path, content) in code.files() {
    vfs.add_file(&format!("github/{}", path), content)?;
}
```

### Output to Disk

```rust
use std::fs;
use std::path::Path;

let output_dir = Path::new("~/.claude/servers/github");
fs::create_dir_all(output_dir)?;

for (path, content) in code.files() {
    let file_path = output_dir.join(path);
    fs::write(file_path, content)?;
}
```

## Current Limitations

⚠️ **Runtime Bridge Not Implemented**: The `callMCPTool()` function in `_runtime/mcp-bridge.ts` is currently a stub. Implementation planned for Phase 2.3.

**What Works**:
- ✅ Type-safe interface generation
- ✅ JSDoc documentation
- ✅ 98% token savings
- ✅ Progressive loading pattern

**Planned**:
- 🔵 Functional `callMCPTool()` via `mcp-execution-cli bridge` command

## Performance

From benchmarks (M1 MacBook Pro):

| Metric | Target | Achieved | Status |
|--------|--------|----------|--------|
| Generate 10 tools | <100ms | **0.19ms** | ✅ 526x faster |
| Generate 50 tools | <20ms | **0.97ms** | ✅ 20.6x faster |
| VFS export | <10ms | **1.2ms** | ✅ 8.3x faster |

## Examples

See [examples/progressive-loading-usage.md](../../examples/progressive-loading-usage.md) for complete tutorial.

## License

See workspace LICENSE file.
