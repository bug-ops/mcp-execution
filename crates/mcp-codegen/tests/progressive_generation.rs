//! Integration tests for progressive loading code generation.
//!
//! Tests the full pipeline from `ServerInfo` to generated TypeScript files
//! for progressive loading pattern.

use mcp_execution_codegen::progressive::ProgressiveGenerator;
use mcp_execution_core::{ServerId, ToolName};
use mcp_execution_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use serde_json::json;
use std::process::Command;

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
    // - 1 package.json
    // - 1 _meta.json
    assert_eq!(code.file_count(), 7);
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

    // Should contain parameter type alias (not an interface — interfaces are not
    // structurally assignable to Record<string, unknown>, which callMCPTool requires)
    assert!(
        content.contains("export type createIssueParams = {"),
        "Missing Params type alias"
    );

    // Should contain result interface
    assert!(
        content.contains("export interface createIssueResult"),
        "Missing Result interface"
    );

    // Should call callMCPTool
    assert!(content.contains("callMCPTool"), "Missing callMCPTool call");

    // Should cast callMCPTool's `unknown` return to the Result type — `unknown` is never
    // assignable to a concrete type without a cast, regardless of interface vs type alias
    assert!(
        content.contains(") as createIssueResult;"),
        "Missing cast of callMCPTool's return value to createIssueResult"
    );

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
fn test_progressive_index_doc_comment_not_prematurely_closed() {
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

    let doc_start = content.find("/**").expect("Missing opening JSDoc block");
    let doc_end = content[doc_start..]
        .find("*/")
        .map(|i| doc_start + i)
        .expect("Missing JSDoc close");

    assert!(
        content[doc_start..doc_end].contains("@packageDocumentation"),
        "Top-level JSDoc block closed prematurely before @packageDocumentation \
         — likely a nested /* ... */ inside the doc comment (regression for #139)"
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
    // - 1 package.json
    // - 1 _meta.json
    assert_eq!(code.file_count(), 4);

    let file_paths: Vec<_> = code.files.iter().map(|f| f.path.as_str()).collect();
    assert!(file_paths.contains(&"index.ts"));
    assert!(file_paths.contains(&"_runtime/mcp-bridge.ts"));
    assert!(file_paths.contains(&"package.json"));
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

/// Regression guard for #176: generated tool wrappers must actually type-check under
/// `tsc --strict --noEmit`, not merely contain the right substrings. Type-checks the real
/// generated `createIssue.ts` against a minimal stub of `callMCPTool` (mirroring the
/// signature declared in `runtime-bridge.ts.hbs`) rather than the full runtime bridge, so
/// the test stays offline and doesn't depend on `@types/node` being installed.
///
/// Skips (does not fail) when `tsc` is not on `PATH`, since the TypeScript toolchain isn't
/// guaranteed to be present in every environment this suite runs in.
#[test]
fn test_generated_tool_passes_tsc_noemit() {
    if Command::new("tsc").arg("--version").output().is_err() {
        eprintln!("skipping test_generated_tool_passes_tsc_noemit: `tsc` not found on PATH");
        return;
    }

    // Guard against the stub below silently going stale if callMCPTool's signature changes.
    let bridge_template = include_str!("../templates/progressive/runtime-bridge.ts.hbs");
    assert!(
        bridge_template.contains("params: Record<string, unknown>")
            && bridge_template.contains("): Promise<unknown> {"),
        "callMCPTool signature in runtime-bridge.ts.hbs changed — update the stub in \
         test_generated_tool_passes_tsc_noemit to match"
    );

    let generator = ProgressiveGenerator::new().expect("Failed to create generator");
    let server_info = create_test_server_info();
    let code = generator
        .generate(&server_info)
        .expect("Failed to generate code");

    let tool_file = code
        .files
        .iter()
        .find(|f| f.path == "createIssue.ts")
        .expect("createIssue.ts not found");
    let package_json = code
        .files
        .iter()
        .find(|f| f.path == "package.json")
        .expect("package.json not found");

    let dir = tempfile::tempdir().expect("Failed to create temp dir");

    std::fs::write(dir.path().join("createIssue.ts"), &tool_file.content)
        .expect("Failed to write createIssue.ts");
    // Real package.json declares `"type": "module"`, which is what makes `import.meta.url`
    // in the generated CLI-mode block legal under NodeNext module resolution.
    std::fs::write(dir.path().join("package.json"), &package_json.content)
        .expect("Failed to write package.json");
    // Minimal ambient declaration for the subset of the Node `process` global the generated
    // CLI-mode block references, so the test doesn't depend on `@types/node` being installed.
    std::fs::write(
        dir.path().join("globals.d.ts"),
        "declare const process: {\n\
         \x20\x20argv: string[];\n\
         \x20\x20exit(code?: number): never;\n\
         };\n",
    )
    .expect("Failed to write globals.d.ts");

    let runtime_dir = dir.path().join("_runtime");
    std::fs::create_dir_all(&runtime_dir).expect("Failed to create _runtime dir");
    std::fs::write(
        runtime_dir.join("mcp-bridge.ts"),
        "export async function callMCPTool(\n\
         \x20\x20serverId: string,\n\
         \x20\x20toolName: string,\n\
         \x20\x20params: Record<string, unknown>\n\
         ): Promise<unknown> {\n\
         \x20\x20void serverId;\n\
         \x20\x20void toolName;\n\
         \x20\x20void params;\n\
         \x20\x20return undefined;\n\
         }\n",
    )
    .expect("Failed to write mcp-bridge.ts stub");

    std::fs::write(
        dir.path().join("tsconfig.json"),
        r#"{
  "compilerOptions": {
    "target": "ES2022",
    "module": "NodeNext",
    "moduleResolution": "NodeNext",
    "strict": true,
    "noEmit": true,
    "allowImportingTsExtensions": true,
    "skipLibCheck": true
  },
  "include": ["**/*.ts"]
}
"#,
    )
    .expect("Failed to write tsconfig.json");

    let output = Command::new("tsc")
        .arg("--noEmit")
        .arg("-p")
        .arg(dir.path().join("tsconfig.json"))
        .output()
        .expect("Failed to run tsc");

    assert!(
        output.status.success(),
        "tsc --noEmit failed on generated createIssue.ts:\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}
