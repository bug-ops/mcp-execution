//! Codegen's real output, written to disk, goes through the skill scanner with no hand-built
//! intermediate fixtures: a drift between the `_meta.json` writer and its reader fails here.
#![expect(
    clippy::expect_used,
    clippy::unwrap_used,
    reason = "test, bench and example code may panic on failure"
)]

use mcp_execution_codegen::PackageGenerator;
use mcp_execution_core::metadata::MAX_TOOL_COUNT;
use mcp_execution_core::{ServerConfig, ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use mcp_execution_skill::scan_tools_directory;
use serde_json::json;

fn tool(index: usize) -> ToolInfo {
    ToolInfo {
        name: ToolName::new(format!("tool_{index:04}")).unwrap(),
        description: format!("Tool number {index}"),
        input_schema: json!({
            "type": "object",
            "properties": {
                "repo": {"type": "string", "description": "Repository"},
                "per-page": {"type": "integer"}
            },
            "required": ["repo"]
        }),
        output_schema: None,
        annotations: None,
    }
}

async fn generate_and_scan(tool_count: usize) -> mcp_execution_skill::ScanResult {
    let server = ServerInfo {
        id: ServerId::new("acme").unwrap(),
        name: "Acme".to_string(),
        version: "1.0.0".to_string(),
        tools: (0..tool_count).map(tool).collect(),
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };
    let config = ServerConfig::builder()
        .command("test-command".to_string())
        .build()
        .unwrap();
    let code = PackageGenerator::new()
        .unwrap()
        .generate(&server, &config)
        .expect("generation succeeds");

    let dir = tempfile::tempdir().unwrap();
    for file in &code.files {
        let path = dir.path().join(&file.path);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, &file.content).unwrap();
    }

    scan_tools_directory(dir.path())
        .await
        .expect("the scanner accepts what the generator wrote")
}

#[tokio::test]
async fn scanner_accepts_the_generators_output() {
    let result = generate_and_scan(3).await;

    assert_eq!(result.tools.len(), 3);
    assert!(result.warnings.is_empty(), "{:?}", result.warnings);
    assert!(
        result.tools[0]
            .parameters
            .iter()
            .any(|p| p.name == "per-page")
    );
}

#[tokio::test]
async fn scanner_accepts_a_server_at_the_introspection_tool_bound() {
    let result = generate_and_scan(MAX_TOOL_COUNT).await;

    assert_eq!(result.tools.len(), MAX_TOOL_COUNT);
}
