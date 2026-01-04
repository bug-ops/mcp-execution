---
applyTo: "crates/mcp-execution-codegen/**/*.rs"
---
# Copilot Instructions: mcp-execution-codegen

This crate provides **code generation** from MCP tool schemas using Handlebars templates. It supports multiple output formats via feature flags.

## Feature Flags - CRITICAL

The crate has **two mutually exclusive features**:

```toml
[features]
default = ["wasm"]
wasm = []     # Generate TypeScript for WASM
skills = []   # Generate executable scripts for Claude Code Skills
all = ["wasm", "skills"]  # Generate both formats
```

**Code must be feature-gated appropriately**:

```rust
// ✅ GOOD: Feature-gated code
#[cfg(feature = "wasm")]
pub mod wasm;

#[cfg(feature = "skills")]
pub mod skills;

// ✅ GOOD: Conditional compilation
impl CodeGenerator {
    pub fn generate(&self, server_info: &ServerInfo, output: &Path) -> Result<()> {
        #[cfg(feature = "wasm")]
        {
            self.generate_wasm(server_info, output)?;
        }

        #[cfg(feature = "skills")]
        {
            self.generate_skills(server_info, output)?;
        }

        Ok(())
    }
}
```

## Error Handling

**Use `thiserror`**:

```rust
use thiserror::Error;

#[derive(Error, Debug)]
pub enum CodegenError {
    #[error("template error: {0}")]
    TemplateError(#[from] handlebars::TemplateError),

    #[error("render error: {0}")]
    RenderError(#[from] handlebars::RenderError),

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("invalid schema for tool '{tool}': {reason}")]
    InvalidSchema { tool: String, reason: String },

    #[error("unsupported type '{type_name}' in tool '{tool}'")]
    UnsupportedType { tool: String, type_name: String },
}

pub type Result<T> = std::result::Result<T, CodegenError>;
```

## Handlebars Integration

Use Handlebars for all template rendering:

```rust
use handlebars::Handlebars;
use serde::Serialize;

pub struct CodeGenerator {
    handlebars: Handlebars<'static>,
}

impl CodeGenerator {
    pub fn new() -> Result<Self> {
        let mut handlebars = Handlebars::new();

        // Register templates
        #[cfg(feature = "wasm")]
        {
            handlebars.register_template_string(
                "wasm_module",
                include_str!("../templates/wasm/module.hbs"),
            )?;

            handlebars.register_template_string(
                "wasm_tool",
                include_str!("../templates/wasm/tool.hbs"),
            )?;
        }

        #[cfg(feature = "skills")]
        {
            handlebars.register_template_string(
                "skill",
                include_str!("../templates/skills/skill.hbs"),
            )?;
        }

        // Register helpers
        handlebars.register_helper("to_typescript_type", Box::new(typescript_type_helper));
        handlebars.register_helper("to_camel_case", Box::new(camel_case_helper));

        Ok(Self { handlebars })
    }

    pub fn render_tool(&self, template: &str, data: &impl Serialize) -> Result<String> {
        self.handlebars
            .render(template, data)
            .map_err(CodegenError::from)
    }
}
```

## Custom Helpers

Implement Handlebars helpers for common transformations:

```rust
use handlebars::{Context, Handlebars, Helper, HelperResult, Output, RenderContext};

// Convert JSON Schema type to TypeScript type
fn typescript_type_helper(
    h: &Helper,
    _: &Handlebars,
    _: &Context,
    _: &mut RenderContext,
    out: &mut dyn Output,
) -> HelperResult {
    let param = h
        .param(0)
        .and_then(|v| v.value().as_str())
        .ok_or_else(|| handlebars::RenderError::new("param not found"))?;

    let ts_type = match param {
        "string" => "string",
        "number" | "integer" => "number",
        "boolean" => "boolean",
        "array" => "any[]",
        "object" => "any",
        _ => "any",
    };

    out.write(ts_type)?;
    Ok(())
}

// Convert snake_case to camelCase
fn camel_case_helper(
    h: &Helper,
    _: &Handlebars,
    _: &Context,
    _: &mut RenderContext,
    out: &mut dyn Output,
) -> HelperResult {
    let param = h
        .param(0)
        .and_then(|v| v.value().as_str())
        .ok_or_else(|| handlebars::RenderError::new("param not found"))?;

    let camel = param
        .split('_')
        .enumerate()
        .map(|(i, part)| {
            if i == 0 {
                part.to_string()
            } else {
                let mut chars = part.chars();
                chars.next().map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default() + chars.as_str()
            }
        })
        .collect::<String>();

    out.write(&camel)?;
    Ok(())
}
```

## Template Data Structures

Define serializable data for templates:

```rust
use serde::Serialize;

#[derive(Serialize)]
pub struct ToolData {
    pub name: String,
    pub description: String,
    pub parameters: Vec<ParameterData>,
}

#[derive(Serialize)]
pub struct ParameterData {
    pub name: String,
    pub type_: String,
    pub required: bool,
    pub description: Option<String>,
}

impl From<&ToolInfo> for ToolData {
    fn from(tool: &ToolInfo) -> Self {
        let parameters = tool
            .input_schema
            .properties
            .iter()
            .map(|prop| ParameterData {
                name: prop.name.clone(),
                type_: prop.type_.clone(),
                required: prop.required,
                description: prop.description.clone(),
            })
            .collect();

        Self {
            name: tool.name.clone(),
            description: tool.description.clone(),
            parameters,
        }
    }
}
```

## WASM Code Generation

**Only compiled with `wasm` feature**:

```rust
#[cfg(feature = "wasm")]
pub mod wasm {
    use super::*;

    pub fn generate_typescript_module(
        generator: &CodeGenerator,
        server_info: &ServerInfo,
    ) -> Result<String> {
        let data = WasmModuleData {
            server_name: server_info.name.clone(),
            tools: server_info.tools.iter().map(ToolData::from).collect(),
        };

        generator.render_tool("wasm_module", &data)
    }

    #[derive(Serialize)]
    struct WasmModuleData {
        server_name: String,
        tools: Vec<ToolData>,
    }
}
```

**Template example** (`templates/wasm/module.hbs`):

```typescript
// Generated from MCP server: {{server_name}}

{{#each tools}}
/**
 * {{description}}
 {{#each parameters}}
 * @param {{name}} {{#if description}}{{description}}{{/if}}
 {{/each}}
 */
export async function {{to_camel_case name}}(
  {{#each parameters}}
  {{name}}{{#unless required}}?{{/unless}}: {{to_typescript_type type_}},
  {{/each}}
): Promise<any> {
  const params = {
    {{#each parameters}}
    {{name}},
    {{/each}}
  };

  return await callMcpTool("{{name}}", params);
}

{{/each}}

// Internal bridge function
declare function callMcpTool(name: string, params: any): Promise<any>;
```

## Skills Code Generation

**Only compiled with `skills` feature**:

```rust
#[cfg(feature = "skills")]
pub mod skills {
    use super::*;

    pub fn generate_skill(
        generator: &CodeGenerator,
        tool: &ToolInfo,
    ) -> Result<String> {
        let data = ToolData::from(tool);
        generator.render_tool("skill", &data)
    }
}
```

**Template example** (`templates/skills/skill.hbs`):

```bash
#!/bin/bash
# {{name}}: {{description}}
#
# Parameters:
{{#each parameters}}
# - {{name}} ({{type_}}{{#if required}}, required{{/if}}): {{description}}
{{/each}}

# Parse arguments
{{#each parameters}}
{{#if required}}
{{name}}=""
{{else}}
{{name}}="${{{to_uppercase name}}:-}"
{{/if}}
{{/each}}

# ... rest of skill implementation
```

## File Writing

Generate and write files:

```rust
use std::fs;
use std::path::Path;

impl CodeGenerator {
    pub fn write_generated(
        &self,
        content: &str,
        output_path: &Path,
    ) -> Result<()> {
        // Create parent directories
        if let Some(parent) = output_path.parent() {
            fs::create_dir_all(parent)?;
        }

        // Write file
        fs::write(output_path, content)?;

        Ok(())
    }

    #[cfg(feature = "wasm")]
    pub fn generate_wasm(
        &self,
        server_info: &ServerInfo,
        output_dir: &Path,
    ) -> Result<()> {
        let ts_code = wasm::generate_typescript_module(self, server_info)?;

        let output_file = output_dir.join(format!("{}.ts", server_info.name));
        self.write_generated(&ts_code, &output_file)?;

        Ok(())
    }

    #[cfg(feature = "skills")]
    pub fn generate_skills(
        &self,
        server_info: &ServerInfo,
        output_dir: &Path,
    ) -> Result<()> {
        for tool in &server_info.tools {
            let skill_code = skills::generate_skill(self, tool)?;

            let output_file = output_dir.join(format!("{}.sh", tool.name));
            self.write_generated(&skill_code, &output_file)?;

            // Make executable
            #[cfg(unix)]
            {
                use std::os::unix::fs::PermissionsExt;
                let mut perms = fs::metadata(&output_file)?.permissions();
                perms.set_mode(0o755);
                fs::set_permissions(&output_file, perms)?;
            }
        }

        Ok(())
    }
}
```

## Testing

Test code generation with fixtures:

```rust
#[cfg(test)]
mod tests {
    use super::*;

    fn test_server_info() -> ServerInfo {
        ServerInfo {
            name: "test-server".to_string(),
            version: "1.0.0".to_string(),
            tools: vec![
                ToolInfo {
                    name: "send_message".to_string(),
                    description: "Send a message".to_string(),
                    input_schema: Schema {
                        properties: vec![
                            SchemaProperty {
                                name: "chat_id".to_string(),
                                type_: "string".to_string(),
                                required: true,
                                description: Some("Chat ID".to_string()),
                            },
                        ],
                    },
                },
            ],
            resources: vec![],
            prompts: vec![],
        }
    }

    #[test]
    #[cfg(feature = "wasm")]
    fn test_generate_wasm() {
        let generator = CodeGenerator::new().unwrap();
        let server_info = test_server_info();

        let result = wasm::generate_typescript_module(&generator, &server_info);
        assert!(result.is_ok());

        let code = result.unwrap();
        assert!(code.contains("send_message"));
        assert!(code.contains("chat_id"));
    }

    #[test]
    #[cfg(feature = "skills")]
    fn test_generate_skill() {
        let generator = CodeGenerator::new().unwrap();
        let tool = test_server_info().tools[0].clone();

        let result = skills::generate_skill(&generator, &tool);
        assert!(result.is_ok());

        let code = result.unwrap();
        assert!(code.contains("#!/bin/bash"));
        assert!(code.contains("send_message"));
    }
}
```

## Summary

- **Feature flags** control output format (`wasm` vs `skills`)
- **Handlebars templates** for all code generation
- **Custom helpers** for type conversion and naming
- **Use `thiserror`** for all errors
- **Serialize data** for template rendering
- **Test with fixtures** to avoid external dependencies
