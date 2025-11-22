//! Integration tests for CLI skill management commands.
//!
//! Tests the full workflow from command handlers to output formatting,
//! ensuring all skill management operations work correctly end-to-end.

use mcp_cli::commands::skill::{
    SkillAction, list_skills, load_skill, remove_skill, run, show_skill_info,
};
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_skill_store::{ServerInfo, SkillStore, ToolInfo};
use mcp_vfs::VfsBuilder;
use tempfile::TempDir;

/// Helper function to create a test VFS with sample files.
fn create_test_vfs() -> mcp_vfs::Vfs {
    VfsBuilder::new()
        .add_file("/index.ts", "export * from './tools';")
        .add_file("/tools/sendMessage.ts", "export function sendMessage() {}")
        .add_file("/tools/getChatInfo.ts", "export function getChatInfo() {}")
        .add_file("/types.ts", "export type Message = { id: string };")
        .build()
        .unwrap()
}

/// Helper function to create test server info.
fn create_test_server_info(name: &str) -> ServerInfo {
    ServerInfo {
        name: name.to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
    }
}

/// Helper function to create test tools.
fn create_test_tools() -> Vec<ToolInfo> {
    vec![
        ToolInfo {
            name: "send_message".to_string(),
            description: "Sends a message to a chat".to_string(),
        },
        ToolInfo {
            name: "get_chat_info".to_string(),
            description: "Gets information about a chat".to_string(),
        },
    ]
}

/// Helper function to save a test skill.
fn save_test_skill(
    store: &SkillStore,
    name: &str,
    vfs: &mcp_vfs::Vfs,
    wasm: &[u8],
) -> mcp_skill_store::Result<mcp_skill_store::SkillMetadata> {
    let server_info = create_test_server_info(name);
    let tools = create_test_tools();
    store.save_skill(name, vfs, wasm, server_info, tools)
}

// ============================================================================
// List Command Tests
// ============================================================================

#[test]
fn test_list_skills_empty_directory() {
    let temp = TempDir::new().unwrap();

    let result = list_skills(&temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_list_skills_single_plugin() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D]; // WASM magic bytes
    save_test_skill(&store, "test-plugin", &vfs, &wasm).unwrap();

    // List plugins
    let result = list_skills(&temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_list_skills_multiple_plugins() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save multiple plugins
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];

    save_test_skill(&store, "plugin1", &vfs, &wasm).unwrap();
    save_test_skill(&store, "plugin2", &vfs, &wasm).unwrap();
    save_test_skill(&store, "plugin3", &vfs, &wasm).unwrap();

    // List plugins with JSON format
    let result = list_skills(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_list_skills_all_formats() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "format-test", &vfs, &wasm).unwrap();

    // Test all output formats
    for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
        let result = list_skills(&temp.path().to_path_buf(), format);
        assert!(result.is_ok(), "List should succeed with {format:?} format");
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}

#[test]
fn test_list_skills_invalid_directory() {
    let temp = TempDir::new().unwrap();
    let invalid_path = temp.path().join("nonexistent");

    // Should still succeed but list will be empty
    let result = list_skills(&invalid_path, OutputFormat::Json);
    assert!(result.is_ok());
}

// ============================================================================
// Load Command Tests
// ============================================================================

#[test]
fn test_load_skill_success() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "loadable-plugin", &vfs, &wasm).unwrap();

    // Load plugin
    let result = load_skill(
        "loadable-plugin",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_load_skill_not_found() {
    let temp = TempDir::new().unwrap();

    let result = load_skill(
        "nonexistent-plugin",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_err());
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("nonexistent-plugin"));
}

#[test]
fn test_load_skill_all_formats() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "format-load-test", &vfs, &wasm).unwrap();

    // Test all output formats
    for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
        let result = load_skill("format-load-test", &temp.path().to_path_buf(), format);
        assert!(result.is_ok(), "Load should succeed with {format:?} format");
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}

#[test]
fn test_load_skill_with_large_wasm() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin with larger WASM module
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D]
        .into_iter()
        .chain(vec![0xFF; 1024 * 100]) // 100KB WASM
        .collect::<Vec<_>>();

    save_test_skill(&store, "large-wasm", &vfs, &wasm).unwrap();

    // Load plugin
    let result = load_skill("large-wasm", &temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_load_skill_empty_vfs() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin with empty VFS
    let vfs = VfsBuilder::new().build().unwrap();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    let server_info = create_test_server_info("empty-vfs");

    store
        .save_skill("empty-vfs", &vfs, &wasm, server_info, vec![])
        .unwrap();

    // Load plugin
    let result = load_skill("empty-vfs", &temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

// ============================================================================
// Info Command Tests
// ============================================================================

#[test]
fn test_info_plugin_success() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "info-test", &vfs, &wasm).unwrap();

    // Show info
    let result = show_skill_info("info-test", &temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_info_plugin_not_found() {
    let temp = TempDir::new().unwrap();

    let result = show_skill_info(
        "nonexistent",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_info_plugin_all_formats() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "format-info-test", &vfs, &wasm).unwrap();

    // Test all output formats
    for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
        let result = show_skill_info("format-info-test", &temp.path().to_path_buf(), format);
        assert!(result.is_ok(), "Info should succeed with {format:?} format");
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}

#[test]
fn test_info_plugin_with_many_tools() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Create plugin with many tools
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    let server_info = create_test_server_info("many-tools");
    let tools: Vec<ToolInfo> = (0..20)
        .map(|i| ToolInfo {
            name: format!("tool_{i}"),
            description: format!("Tool number {i}"),
        })
        .collect();

    store
        .save_skill("many-tools", &vfs, &wasm, server_info, tools)
        .unwrap();

    // Show info
    let result = show_skill_info("many-tools", &temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

// ============================================================================
// Remove Command Tests
// ============================================================================

#[test]
fn test_remove_skill_with_yes_flag() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "removable", &vfs, &wasm).unwrap();

    // Remove with --yes flag (skip confirmation)
    let result = remove_skill(
        "removable",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);

    // Verify plugin was removed
    assert!(!store.skill_exists("removable").unwrap());
}

#[test]
fn test_remove_skill_not_found() {
    let temp = TempDir::new().unwrap();

    let result = remove_skill(
        "nonexistent",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_remove_skill_all_formats() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save multiple plugins for different formats
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];

    // Test each output format with a separate plugin
    for (i, format) in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty]
        .iter()
        .enumerate()
    {
        let plugin_name = format!("remove-format-{i}");
        save_test_skill(&store, &plugin_name, &vfs, &wasm).unwrap();

        let result = remove_skill(&plugin_name, &temp.path().to_path_buf(), true, *format);
        assert!(
            result.is_ok(),
            "Remove should succeed with {format:?} format"
        );
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);

        // Verify removal
        assert!(!store.skill_exists(&plugin_name).unwrap());
    }
}

#[test]
fn test_remove_skill_and_reload_fails() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "to-remove", &vfs, &wasm).unwrap();

    // Remove it
    let result = remove_skill(
        "to-remove",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );
    assert!(result.is_ok());

    // Try to load - should fail
    let load_result = load_skill("to-remove", &temp.path().to_path_buf(), OutputFormat::Json);
    assert!(load_result.is_err());
}

// ============================================================================
// Integration: Run Function Tests
// ============================================================================

#[tokio::test]
async fn test_run_load_action() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "run-load-test", &vfs, &wasm).unwrap();

    // Test via run function
    let action = SkillAction::Load {
        name: "run-load-test".to_string(),
        skill_dir: temp.path().to_path_buf(),
    };

    let result = run(action, OutputFormat::Json).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[tokio::test]
async fn test_run_list_action() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save some plugins
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "run-list-1", &vfs, &wasm).unwrap();
    save_test_skill(&store, "run-list-2", &vfs, &wasm).unwrap();

    // Test via run function
    let action = SkillAction::List {
        skill_dir: temp.path().to_path_buf(),
    };

    let result = run(action, OutputFormat::Json).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[tokio::test]
async fn test_run_info_action() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "run-info-test", &vfs, &wasm).unwrap();

    // Test via run function
    let action = SkillAction::Info {
        name: "run-info-test".to_string(),
        skill_dir: temp.path().to_path_buf(),
    };

    let result = run(action, OutputFormat::Json).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[tokio::test]
async fn test_run_remove_action() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "run-remove-test", &vfs, &wasm).unwrap();

    // Test via run function
    let action = SkillAction::Remove {
        name: "run-remove-test".to_string(),
        skill_dir: temp.path().to_path_buf(),
        yes: true,
    };

    let result = run(action, OutputFormat::Json).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

// ============================================================================
// Edge Cases and Error Scenarios
// ============================================================================

#[test]
fn test_load_skill_invalid_name() {
    let temp = TempDir::new().unwrap();

    // Try to load with invalid server name (path traversal)
    let result = load_skill(
        "../etc/passwd",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_info_plugin_invalid_name() {
    let temp = TempDir::new().unwrap();

    // Try to get info with invalid server name
    let result = show_skill_info(
        "invalid/name",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_remove_skill_invalid_name() {
    let temp = TempDir::new().unwrap();

    // Try to remove with invalid server name
    let result = remove_skill(
        "invalid\\name",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_list_skills_with_corrupted_metadata() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a valid plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "valid-plugin", &vfs, &wasm).unwrap();

    // Create a corrupted plugin directory
    let corrupt_dir = temp.path().join("corrupt-plugin");
    std::fs::create_dir_all(&corrupt_dir).unwrap();
    std::fs::write(corrupt_dir.join("skill.json"), "invalid json").unwrap();

    // List should still work but may skip corrupted plugins
    let result = list_skills(&temp.path().to_path_buf(), OutputFormat::Json);

    // The result should either succeed or fail gracefully
    // Implementation may choose to skip corrupted plugins or error out
    if result.is_err() {
        let err_msg = format!("{:?}", result.unwrap_err());
        assert!(
            err_msg.contains("JSON") || err_msg.contains("metadata"),
            "Error should be about invalid metadata"
        );
    }
}

#[test]
fn test_load_skill_with_nested_vfs_paths() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Create VFS with deeply nested paths
    let vfs = VfsBuilder::new()
        .add_file("/a/b/c/d/e/deep.ts", "export const DEEP = true;")
        .add_file("/x/y/z/nested.ts", "export const NESTED = true;")
        .build()
        .unwrap();

    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    let server_info = create_test_server_info("nested-paths");

    store
        .save_skill("nested-paths", &vfs, &wasm, server_info, vec![])
        .unwrap();

    // Load should succeed
    let result = load_skill(
        "nested-paths",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

// ============================================================================
// Workflow Tests (End-to-End)
// ============================================================================

#[test]
fn test_full_plugin_lifecycle() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // 1. Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "lifecycle-test", &vfs, &wasm).unwrap();

    // 2. List plugins - should find it
    let list_result = list_skills(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(list_result.is_ok());

    // 3. Load plugin - should succeed
    let load_result = load_skill(
        "lifecycle-test",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );
    assert!(load_result.is_ok());

    // 4. Show info - should succeed
    let info_result = show_skill_info(
        "lifecycle-test",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );
    assert!(info_result.is_ok());

    // 5. Remove plugin - should succeed
    let remove_result = remove_skill(
        "lifecycle-test",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );
    assert!(remove_result.is_ok());

    // 6. List again - should be empty or not include removed plugin
    let list_after_result = list_skills(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(list_after_result.is_ok());

    // 7. Try to load removed plugin - should fail
    let load_after_result = load_skill(
        "lifecycle-test",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );
    assert!(load_after_result.is_err());
}

#[test]
fn test_multiple_plugins_independent_operations() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save multiple plugins
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];

    save_test_skill(&store, "plugin-a", &vfs, &wasm).unwrap();
    save_test_skill(&store, "plugin-b", &vfs, &wasm).unwrap();
    save_test_skill(&store, "plugin-c", &vfs, &wasm).unwrap();

    // Load one
    let load_a = load_skill("plugin-a", &temp.path().to_path_buf(), OutputFormat::Json);
    assert!(load_a.is_ok());

    // Remove another
    let remove_b = remove_skill(
        "plugin-b",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );
    assert!(remove_b.is_ok());

    // Show info for third
    let info_c = show_skill_info("plugin-c", &temp.path().to_path_buf(), OutputFormat::Json);
    assert!(info_c.is_ok());

    // Verify states
    assert!(store.skill_exists("plugin-a").unwrap());
    assert!(!store.skill_exists("plugin-b").unwrap());
    assert!(store.skill_exists("plugin-c").unwrap());

    // List should show remaining plugins
    let list_result = list_skills(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(list_result.is_ok());
}

// ============================================================================
// Output Format Validation
// ============================================================================

#[test]
fn test_load_output_contains_expected_fields() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "field-test", &vfs, &wasm).unwrap();

    // Capture the actual function behavior by calling the underlying store
    let loaded = store.load_skill("field-test").unwrap();

    // Verify expected fields exist in loaded plugin
    assert_eq!(loaded.metadata.server.name, "field-test");
    assert_eq!(loaded.metadata.server.version, "1.0.0");
    assert_eq!(loaded.metadata.tools.len(), 2);
    assert_eq!(loaded.vfs.file_count(), 4);
    assert!(!loaded.wasm_module.is_empty());
}

#[test]
fn test_list_empty_returns_empty_list() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    let plugins = store.list_skills().unwrap();
    assert_eq!(plugins.len(), 0);

    let result = list_skills(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(result.is_ok());
}

#[test]
fn test_info_output_includes_tool_details() {
    let temp = TempDir::new().unwrap();
    let store = SkillStore::new(temp.path()).unwrap();

    // Save a plugin with specific tools
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_skill(&store, "tool-details", &vfs, &wasm).unwrap();

    // Load to verify tool information is preserved
    let loaded = store.load_skill("tool-details").unwrap();

    assert_eq!(loaded.metadata.tools.len(), 2);
    assert_eq!(loaded.metadata.tools[0].name, "send_message");
    assert_eq!(loaded.metadata.tools[1].name, "get_chat_info");
}
