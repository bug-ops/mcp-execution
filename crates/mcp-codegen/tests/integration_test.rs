//! End-to-end integration tests for mcp-codegen.
//!
//! Tests the complete workflow:
//! 1. Create ServerInfo (from mcp-introspector)
//! 2. Generate code (mcp-codegen)
//! 3. Load into VFS (mcp-vfs)
//! 4. Verify all files exist and are valid

use mcp_codegen::CodeGenerator;
use mcp_core::{ServerId, ToolName};
use mcp_introspector::{ServerCapabilities, ServerInfo, ToolInfo};
use mcp_vfs::VfsBuilder;
use serde_json::json;

/// Creates a realistic test server info for vkteams-bot.
fn create_vkteams_server_info() -> ServerInfo {
    ServerInfo {
        id: ServerId::new("vkteams-bot"),
        name: "VK Teams Bot".to_string(),
        version: "2.1.0".to_string(),
        tools: vec![
            ToolInfo {
                name: ToolName::new("send_message"),
                description: "Sends a message to a VK Teams chat".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "string",
                            "description": "Chat ID to send message to"
                        },
                        "text": {
                            "type": "string",
                            "description": "Message text"
                        },
                        "silent": {
                            "type": "boolean",
                            "description": "Send silently without notification"
                        }
                    },
                    "required": ["chat_id", "text"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "message_id": {"type": "string"}
                    }
                })),
            },
            ToolInfo {
                name: ToolName::new("get_user"),
                description: "Gets user information by ID".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "user_id": {
                            "type": "string",
                            "description": "User ID to query"
                        }
                    },
                    "required": ["user_id"]
                }),
                output_schema: Some(json!({
                    "type": "object",
                    "properties": {
                        "name": {"type": "string"},
                        "email": {"type": "string"}
                    }
                })),
            },
            ToolInfo {
                name: ToolName::new("get_chat"),
                description: "Gets chat information by ID".to_string(),
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "chat_id": {
                            "type": "string",
                            "description": "Chat ID to query"
                        }
                    },
                    "required": ["chat_id"]
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

/// Creates a minimal test server with no tools.
fn create_empty_server_info() -> ServerInfo {
    ServerInfo {
        id: ServerId::new("empty-server"),
        name: "Empty Server".to_string(),
        version: "1.0.0".to_string(),
        tools: vec![],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
}

/// Creates a server with complex tool schemas.
fn create_complex_server_info() -> ServerInfo {
    ServerInfo {
        id: ServerId::new("complex-server"),
        name: "Complex Server".to_string(),
        version: "3.0.0".to_string(),
        tools: vec![ToolInfo {
            name: ToolName::new("complex_operation"),
            description: "Complex operation with nested schemas".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string"},
                    "metadata": {
                        "type": "object",
                        "properties": {
                            "tags": {
                                "type": "array",
                                "items": {"type": "string"}
                            },
                            "priority": {"type": "number"}
                        }
                    },
                    "options": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "key": {"type": "string"},
                                "value": {"type": "string"}
                            }
                        }
                    }
                },
                "required": ["id"]
            }),
            output_schema: None,
        }],
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    }
}

#[test]
fn test_complete_generation_workflow_vkteams() {
    // Step 1: Create server info
    let server_info = create_vkteams_server_info();

    // Step 2: Generate code
    let generator = CodeGenerator::new().expect("Generator should initialize");
    let generated = generator
        .generate(&server_info)
        .expect("Code generation should succeed");

    // Step 3: Verify generated files
    assert_eq!(
        generated.file_count(),
        6,
        "Should generate 6 files: manifest, types, index, and 3 tools"
    );

    let paths: Vec<_> = generated.files.iter().map(|f| &f.path).collect();
    assert!(paths.contains(&&"manifest.json".to_string()));
    assert!(paths.contains(&&"types.ts".to_string()));
    assert!(paths.contains(&&"index.ts".to_string()));
    assert!(paths.contains(&&"tools/sendMessage.ts".to_string()));
    assert!(paths.contains(&&"tools/getUser.ts".to_string()));
    assert!(paths.contains(&&"tools/getChat.ts".to_string()));

    // Step 4: Load into VFS
    let vfs = VfsBuilder::from_generated_code(generated, "/mcp-tools/servers/vkteams-bot")
        .build()
        .expect("VFS build should succeed");

    // Step 5: Verify files exist in VFS
    assert!(vfs.exists("/mcp-tools/servers/vkteams-bot/manifest.json"));
    assert!(vfs.exists("/mcp-tools/servers/vkteams-bot/types.ts"));
    assert!(vfs.exists("/mcp-tools/servers/vkteams-bot/index.ts"));
    assert!(vfs.exists("/mcp-tools/servers/vkteams-bot/tools/sendMessage.ts"));
    assert!(vfs.exists("/mcp-tools/servers/vkteams-bot/tools/getUser.ts"));
    assert!(vfs.exists("/mcp-tools/servers/vkteams-bot/tools/getChat.ts"));

    // Step 6: Verify manifest content
    let manifest_content = vfs
        .read_file("/mcp-tools/servers/vkteams-bot/manifest.json")
        .expect("Should read manifest");
    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_content).expect("Manifest should be valid JSON");

    assert_eq!(manifest["name"], "VK Teams Bot");
    assert_eq!(manifest["version"], "2.1.0");
    assert_eq!(manifest["tools"].as_array().unwrap().len(), 3);

    // Step 7: Verify types.ts content
    let types_content = vfs
        .read_file("/mcp-tools/servers/vkteams-bot/types.ts")
        .expect("Should read types");
    assert!(types_content.contains("export interface ToolResult"));
    assert!(types_content.contains("'send_message'"));
    assert!(types_content.contains("'get_user'"));
    assert!(types_content.contains("'get_chat'"));

    // Step 8: Verify tool file content
    let send_message_content = vfs
        .read_file("/mcp-tools/servers/vkteams-bot/tools/sendMessage.ts")
        .expect("Should read sendMessage");
    assert!(send_message_content.contains("export async function sendMessage"));
    assert!(send_message_content.contains("chat_id: string"));
    assert!(send_message_content.contains("text: string"));
    assert!(send_message_content.contains("silent?: boolean"));
}

#[test]
fn test_empty_server_generation() {
    let server_info = create_empty_server_info();
    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    // Should still generate base files even with no tools
    assert_eq!(
        generated.file_count(),
        3,
        "Should generate manifest, types, and index"
    );

    let vfs = VfsBuilder::from_generated_code(generated, "/mcp-tools/servers/empty")
        .build()
        .unwrap();

    assert!(vfs.exists("/mcp-tools/servers/empty/manifest.json"));
    assert!(vfs.exists("/mcp-tools/servers/empty/types.ts"));
    assert!(vfs.exists("/mcp-tools/servers/empty/index.ts"));

    // Verify empty tools array
    let manifest_content = vfs
        .read_file("/mcp-tools/servers/empty/manifest.json")
        .unwrap();
    let manifest: serde_json::Value = serde_json::from_str(&manifest_content).unwrap();
    assert_eq!(manifest["tools"].as_array().unwrap().len(), 0);
}

#[test]
fn test_complex_schema_generation() {
    let server_info = create_complex_server_info();
    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    assert_eq!(generated.file_count(), 4);

    let vfs = VfsBuilder::from_generated_code(generated, "/mcp-tools/servers/complex")
        .build()
        .unwrap();

    // Verify complex tool generation
    let tool_content = vfs
        .read_file("/mcp-tools/servers/complex/tools/complexOperation.ts")
        .unwrap();

    assert!(tool_content.contains("export async function complexOperation"));
    assert!(tool_content.contains("id: string"));
    assert!(tool_content.contains("metadata?:"));
    assert!(tool_content.contains("options?:"));
}

#[test]
fn test_multiple_servers_in_vfs() {
    // Generate code for multiple servers
    let server1 = create_vkteams_server_info();
    let server2 = create_empty_server_info();

    let generator = CodeGenerator::new().unwrap();
    let code1 = generator.generate(&server1).unwrap();
    let code2 = generator.generate(&server2).unwrap();

    // Build VFS with both servers - need to create two separate builders
    let mut builder = VfsBuilder::from_generated_code(code1, "/mcp-tools/servers/vkteams-bot");

    // Manually add files from code2
    for file in code2.files {
        let full_path = format!("/mcp-tools/servers/empty/{}", file.path);
        builder = builder.add_file(full_path, file.content);
    }

    let vfs = builder.build().unwrap();

    // Verify both servers exist
    assert!(vfs.exists("/mcp-tools/servers/vkteams-bot/manifest.json"));
    assert!(vfs.exists("/mcp-tools/servers/empty/manifest.json"));

    // Verify total file count
    assert!(vfs.file_count() >= 9, "Should have files from both servers");
}

#[test]
fn test_generated_typescript_is_syntactically_valid() {
    let server_info = create_vkteams_server_info();
    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    for file in &generated.files {
        if file.path.ends_with(".ts") {
            // Basic TypeScript syntax checks
            let content = &file.content;

            // Should not have unmatched braces
            let open_braces = content.matches('{').count();
            let close_braces = content.matches('}').count();
            assert_eq!(
                open_braces, close_braces,
                "Unmatched braces in {}",
                file.path
            );

            // Should not have unmatched parentheses
            let open_parens = content.matches('(').count();
            let close_parens = content.matches(')').count();
            assert_eq!(
                open_parens, close_parens,
                "Unmatched parentheses in {}",
                file.path
            );

            // Should have proper exports
            if file.path.starts_with("tools/") {
                assert!(
                    content.contains("export async function")
                        || content.contains("export function")
                        || content.contains("export interface"),
                    "Tool file should have exports: {}",
                    file.path
                );
            }
        }
    }
}

#[test]
fn test_index_exports_all_tools() {
    let server_info = create_vkteams_server_info();
    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let index_file = generated
        .files
        .iter()
        .find(|f| f.path == "index.ts")
        .expect("Should have index.ts");

    // Should export types
    assert!(index_file.content.contains("export * from './types'"));

    // Should export all tools
    for tool_info in &server_info.tools {
        let ts_name = mcp_codegen::common::typescript::to_camel_case(tool_info.name.as_str());
        assert!(
            index_file.content.contains(&ts_name),
            "Index should export {}",
            ts_name
        );
    }
}

#[test]
fn test_manifest_contains_all_metadata() {
    let server_info = create_vkteams_server_info();
    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let manifest_file = generated
        .files
        .iter()
        .find(|f| f.path == "manifest.json")
        .expect("Should have manifest.json");

    let manifest: serde_json::Value =
        serde_json::from_str(&manifest_file.content).expect("Valid JSON");

    assert_eq!(manifest["name"], "VK Teams Bot");
    assert_eq!(manifest["version"], "2.1.0");
    assert!(manifest["generated_at"].is_string());
    assert_eq!(manifest["generator"], "mcp-codegen");

    let tools = manifest["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);

    // Verify tool metadata
    let send_message_tool = &tools
        .iter()
        .find(|t| t["name"] == "send_message")
        .expect("Should have send_message");

    assert_eq!(send_message_tool["typescript_name"], "sendMessage");
    assert!(send_message_tool["description"].is_string());
}

#[test]
fn test_vfs_directory_listing() {
    let server_info = create_vkteams_server_info();
    let generator = CodeGenerator::new().unwrap();
    let generated = generator.generate(&server_info).unwrap();

    let vfs = VfsBuilder::from_generated_code(generated, "/mcp-tools/servers/vkteams-bot")
        .build()
        .unwrap();

    // List tools directory
    let tools_entries = vfs
        .list_dir("/mcp-tools/servers/vkteams-bot/tools")
        .expect("Should list tools directory");

    assert_eq!(tools_entries.len(), 3, "Should have 3 tool files");

    // Convert VfsPath to file names for comparison
    let entry_names: Vec<String> = tools_entries
        .iter()
        .filter_map(|p| {
            p.as_path()
                .file_name()
                .and_then(|n| n.to_str())
                .map(String::from)
        })
        .collect();

    assert!(entry_names.contains(&"sendMessage.ts".to_string()));
    assert!(entry_names.contains(&"getUser.ts".to_string()));
    assert!(entry_names.contains(&"getChat.ts".to_string()));
}

#[test]
fn test_performance_large_server() {
    use std::time::Instant;

    // Create a server with many tools
    let mut tools = Vec::new();
    for i in 0..50 {
        tools.push(ToolInfo {
            name: ToolName::new(&format!("tool_{}", i)),
            description: format!("Tool number {}", i),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "param1": {"type": "string"},
                    "param2": {"type": "number"}
                },
                "required": ["param1"]
            }),
            output_schema: None,
        });
    }

    let server_info = ServerInfo {
        id: ServerId::new("large-server"),
        name: "Large Server".to_string(),
        version: "1.0.0".to_string(),
        tools,
        capabilities: ServerCapabilities {
            supports_tools: true,
            supports_resources: false,
            supports_prompts: false,
        },
    };

    let generator = CodeGenerator::new().unwrap();

    let start = Instant::now();
    let generated = generator.generate(&server_info).unwrap();
    let duration = start.elapsed();

    // Should complete in reasonable time (<100ms for 50 tools)
    assert!(
        duration.as_millis() < 100,
        "Generation took too long: {:?}",
        duration
    );

    // Should generate all files
    assert_eq!(
        generated.file_count(),
        53,
        "50 tools + manifest + types + index"
    );

    // Verify VFS loading is also fast
    let start = Instant::now();
    let vfs = VfsBuilder::from_generated_code(generated, "/mcp-tools/servers/large")
        .build()
        .unwrap();
    let vfs_duration = start.elapsed();

    assert!(
        vfs_duration.as_millis() < 50,
        "VFS loading took too long: {:?}",
        vfs_duration
    );

    assert_eq!(vfs.file_count(), 53);
}
