//! Integration tests for progressive loading code generation.
//!
//! Tests the full pipeline from `ServerInfo` to generated TypeScript files
//! for progressive loading pattern.

use mcp_codegen::progressive::ProgressiveGenerator;
use mcp_core::{ServerId, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;

/// Creates a mock server info for testing.
fn create_test_server_info() -> ServerInfo {
    ServerInfo {
        id: ServerId::new("github"),
        name: "GitHub".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![
            ToolInfo {
                name: ToolName::new("create_issue"),
                description: "Creates a new issue".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string",
                            "description": "Repository name"
                        },
                        "title": {
                            "type": "string",
                            "description": "Issue title"
                        },
                        "body": {
                            "type": "string",
                            "description": "Issue body"
                        }
                    },
                    "required": ["repo", "title"]
                }),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("update_issue"),
                description: "Updates an existing issue".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string"
                        },
                        "issue_number": {
                            "type": "number"
                        },
                        "title": {
                            "type": "string"
                        }
                    },
                    "required": ["repo", "issue_number"]
                }),
                output_schema: None,
            },
            ToolInfo {
                name: ToolName::new("get_issue"),
                description: "Gets issue information".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "repo": {
                            "type": "string"
                        },
                        "issue_number": {
                            "type": "number"
                        }
                    },
                    "required": ["repo", "issue_number"]
                }),
                output_schema: None,
            },
        ],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
}

#[test]
fn test_progressive_generator_creates_correct_number_of_files() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    // Should generate:
    // - 3 tool files (createIssue.ts, updateIssue.ts, getIssue.ts)
    // - 1 index.ts
    // - 1 runtime bridge (_runtime/mcp-bridge.ts)
    assert_eq!(code.file_count(), 5);
}

#[test]
fn test_progressive_tool_files_exist() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    let file_paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();

    // Check tool files
    assert!(
        file_paths.contains(&"createIssue.ts"),
        "Missing createIssue.ts"
    );
    assert!(
        file_paths.contains(&"updateIssue.ts"),
        "Missing updateIssue.ts"
    );
    assert!(file_paths.contains(&"getIssue.ts"), "Missing getIssue.ts");

    // Check infrastructure files
    assert!(file_paths.contains(&"index.ts"), "Missing index.ts");
    assert!(
        file_paths.contains(&"_runtime/mcp-bridge.ts"),
        "Missing _runtime/mcp-bridge.ts"
    );
}

#[test]
fn test_progressive_tool_file_structure() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    let create_issue_file = code
        .files
        .iter()
        .find(|f| f.path == "createIssue.ts")
        .expect("createIssue.ts not found");

    let content = &create_issue_file.content;

    // Should contain function export
    assert!(
        content.contains("export async function createIssue"),
        "Missing function export"
    );

    // Should contain parameter interface
    assert!(
        content.contains("export interface createIssueParams"),
        "Missing Params interface"
    );

    // Should contain result interface
    assert!(
        content.contains("export interface createIssueResult"),
        "Missing Result interface"
    );

    // Should call callMCPTool
    assert!(content.contains("callMCPTool"), "Missing callMCPTool call");

    // Should include server_id and tool name
    assert!(
        content.contains("'github'"),
        "Missing server_id in callMCPTool"
    );
    assert!(
        content.contains("'create_issue'"),
        "Missing tool name in callMCPTool"
    );
}

#[test]
fn test_progressive_tool_file_has_proper_types() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    let create_issue_file = code
        .files
        .iter()
        .find(|f| f.path == "createIssue.ts")
        .expect("createIssue.ts not found");

    let content = &create_issue_file.content;

    // Should have required fields without optional marker
    assert!(
        content.contains("repo: string;"),
        "Missing required repo field"
    );
    assert!(
        content.contains("title: string;"),
        "Missing required title field"
    );

    // Should have optional field with ? marker
    assert!(
        content.contains("body?: string;"),
        "Missing optional body field"
    );
}

#[test]
fn test_progressive_index_structure() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    let index_file = code
        .files
        .iter()
        .find(|f| f.path == "index.ts")
        .expect("index.ts not found");

    let content = &index_file.content;

    // Should re-export all tools
    assert!(
        content.contains("export { createIssue"),
        "Missing createIssue export"
    );
    assert!(
        content.contains("export { updateIssue"),
        "Missing updateIssue export"
    );
    assert!(
        content.contains("export { getIssue"),
        "Missing getIssue export"
    );

    // Should export types
    assert!(
        content.contains("createIssueParams"),
        "Missing Params type export"
    );
    assert!(
        content.contains("createIssueResult"),
        "Missing Result type export"
    );

    // Should have tool count in documentation
    assert!(
        content.contains("3 tools"),
        "Missing tool count in documentation"
    );

    // Should re-export runtime bridge
    assert!(
        content.contains("export { callMCPTool }"),
        "Missing callMCPTool export"
    );
}

#[test]
fn test_progressive_runtime_bridge_structure() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    let bridge_file = code
        .files
        .iter()
        .find(|f| f.path == "_runtime/mcp-bridge.ts")
        .expect("_runtime/mcp-bridge.ts not found");

    let content = &bridge_file.content;

    // Should export callMCPTool function
    assert!(
        content.contains("export async function callMCPTool"),
        "Missing callMCPTool export"
    );

    // Should have proper function signature
    assert!(
        content.contains("serverId: string"),
        "Missing serverId parameter"
    );
    assert!(
        content.contains("toolName: string"),
        "Missing toolName parameter"
    );
    assert!(
        content.contains("params: Record<string, unknown>"),
        "Missing params parameter"
    );

    // Should have JSDoc documentation
    assert!(
        content.contains("@param serverId"),
        "Missing serverId JSDoc"
    );
    assert!(
        content.contains("@param toolName"),
        "Missing toolName JSDoc"
    );
    assert!(content.contains("@param params"), "Missing params JSDoc");
}

#[test]
fn test_progressive_generator_with_empty_server() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo {
        id: ServerId::new("empty"),
        name: "Empty Server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    // Should generate:
    // - 0 tool files
    // - 1 index.ts
    // - 1 runtime bridge
    assert_eq!(code.file_count(), 2);

    let file_paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();
    assert!(file_paths.contains(&"index.ts"));
    assert!(file_paths.contains(&"_runtime/mcp-bridge.ts"));
}

#[test]
fn test_progressive_tool_camel_case_conversion() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("send_test_message"),
            description: "Test tool".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {},
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    // Should convert snake_case to camelCase for filename
    assert!(
        code.files
            .iter()
            .any(|f| f.path.as_str() == "sendTestMessage.ts")
    );

    let tool_file = code
        .files
        .iter()
        .find(|f| f.path == "sendTestMessage.ts")
        .expect("sendTestMessage.ts not found");

    // Should use camelCase in function name
    assert!(tool_file.content.contains("function sendTestMessage"));
}

#[test]
fn test_progressive_tool_with_complex_types() {
    let generator = ProgressiveGenerator::new().expect("Failed to create generator");

    let server_info = ServerInfo {
        id: ServerId::new("test"),
        name: "Test".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("complex_tool"),
            description: "Tool with complex types".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "items": {
                        "type": "array",
                        "items": {"type": "string"}
                    },
                    "config": {
                        "type": "object"
                    },
                    "count": {
                        "type": "number"
                    },
                    "enabled": {
                        "type": "boolean"
                    }
                },
                "required": ["items"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    let tool_file = code
        .files
        .iter()
        .find(|f| f.path == "complexTool.ts")
        .expect("complexTool.ts not found");

    let content = &tool_file.content;

    // Should handle array type
    assert!(content.contains("items: string[]"), "Missing array type");

    // Should handle object type
    assert!(
        content.contains("config?: Record<string, unknown>"),
        "Missing object type"
    );

    // Should handle number type
    assert!(content.contains("count?: number"), "Missing number type");

    // Should handle boolean type
    assert!(
        content.contains("enabled?: boolean"),
        "Missing boolean type"
    );
}
