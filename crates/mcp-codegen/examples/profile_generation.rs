//! Profiling example for code generation.
//!
//! Generates code for a large number of tools to capture performance profile.
//!
//! Run with: cargo flamegraph --example profile_generation

use mcp_codegen::CodeGenerator;
use mcp_core::{ServerId, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;

fn create_moderate_tool(index: usize) -> ToolInfo {
    ToolInfo {
        name: ToolName::new(&format!("tool_{}", index)),
        description: format!("Tool {}", index),
        input_schema: json!({
            "type": "object",
            "properties": {
                "id": {"type": "string"},
                "name": {"type": "string"},
                "count": {"type": "number"},
                "active": {"type": "boolean"},
                "tags": {
                    "type": "array",
                    "items": {"type": "string"}
                }
            },
            "required": ["id", "name"]
        }),
        output_schema: Some(json!({
            "type": "object",
            "properties": {
                "result": {"type": "string"},
                "code": {"type": "number"}
            }
        })),
    }
}

fn main() {
    println!("Starting code generation profiling...");

    // Create server with 500 tools
    let tools: Vec<_> = (0..500).map(create_moderate_tool).collect();

    let server_info = ServerInfo {
        id: ServerId::new("profile-server"),
        name: "Profiling Server".to_string(),
        version: "1.0.0".to_string(),
        tools,
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().expect("Generator should initialize");

    // Generate code 10 times to capture representative profile
    for iteration in 0..10 {
        let generated = generator
            .generate(&server_info)
            .expect("Generation should succeed");
        println!(
            "Iteration {}: Generated {} files",
            iteration,
            generated.file_count()
        );
    }

    println!("Profiling complete!");
}
