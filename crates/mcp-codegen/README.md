# mcp-codegen

Code generation for MCP (Model Context Protocol) tools.

Transforms MCP tool schemas into executable TypeScript or Rust code using Handlebars templates.

## Features

This crate supports multiple code generation targets via feature flags:

- **`wasm`** (default): Generate TypeScript for WebAssembly execution
- **`skills`**: Generate executable scripts for Claude Code Skills
- **`progressive`**: Generate progressive loading files (one file per tool)
- **`all`**: Enable all features

## Installation

### Default (WASM)

```toml
[dependencies]
mcp-codegen = "0.4"
```

### Skills Generation

```toml
[dependencies]
mcp-codegen = { version = "0.4", features = ["skills"], default-features = false }
```

### Progressive Loading

```toml
[dependencies]
mcp-codegen = { version = "0.4", features = ["progressive"], default-features = false }
```

### All Features

```toml
[dependencies]
mcp-codegen = { version = "0.4", features = ["all"] }
```

## Usage

### WASM Code Generation

```rust
use mcp_codegen::CodeGenerator;
use mcp_introspector::{Introspector, ServerInfo};
use mcp_core::{ServerId, ServerConfig};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Introspect server
    let mut introspector = Introspector::new();
    let server_id = ServerId::new("github");
    let config = ServerConfig::builder()
        .command("/path/to/github-server".to_string())
        .build();
    let info = introspector.discover_server(server_id, &config).await?;

    // Generate TypeScript code
    let generator = CodeGenerator::new()?;
    let code = generator.generate(&info)?;

    println!("Generated {} files", code.file_count());
    Ok(())
}
```

### Progressive Loading

Progressive loading generates one file per tool, enabling Claude Code to load only what it needs:

```rust
use mcp_codegen::progressive::ProgressiveGenerator;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let generator = ProgressiveGenerator::new()?;
    let code = generator.generate(&server_info)?;

    // Files generated:
    // - index.ts (re-exports)
    // - createIssue.ts
    // - updateIssue.ts
    // - _runtime/mcp-bridge.ts

    Ok(())
}
```

**Token Savings**: Progressive loading achieves 98% token savings compared to loading all tools upfront.

## Architecture

### Code Generation Pipeline

1. **Introspection**: MCP server analysis using `mcp-introspector`
2. **Template Selection**: Choose templates based on feature flags
3. **Code Generation**: Handlebars template rendering
4. **Type Conversion**: JSON Schema → TypeScript types
5. **Output**: Generated files ready for VFS or disk

### Template System

Templates are organized by feature:

```
templates/
├── wasm/           # WASM execution templates
│   ├── tool.ts.hbs
│   ├── types.ts.hbs
│   ├── index.ts.hbs
│   └── manifest.json.hbs
├── skills/         # Claude Code Skills templates
│   ├── skill.md.hbs
│   └── category.md.hbs
└── progressive/    # Progressive loading templates
    ├── tool.ts.hbs
    ├── index.ts.hbs
    └── runtime-bridge.ts.hbs
```

## License

See workspace LICENSE file.
