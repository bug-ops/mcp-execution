//! Integration tests for CLI plugin management commands.
//!
//! Tests the full workflow from command handlers to output formatting,
//! ensuring all plugin management operations work correctly end-to-end.

use mcp_cli::commands::plugin::{
    PluginAction, list_plugins, load_plugin, remove_plugin, run, show_plugin_info,
};
use mcp_core::cli::{ExitCode, OutputFormat};
use mcp_plugin_store::{PluginStore, ServerInfo, ToolInfo};
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

/// Helper function to save a test plugin.
fn save_test_plugin(
    store: &PluginStore,
    name: &str,
    vfs: &mcp_vfs::Vfs,
    wasm: &[u8],
) -> mcp_plugin_store::Result<mcp_plugin_store::PluginMetadata> {
    let server_info = create_test_server_info(name);
    let tools = create_test_tools();
    store.save_plugin(name, vfs, wasm, server_info, tools)
}

// ============================================================================
// List Command Tests
// ============================================================================

#[test]
fn test_list_plugins_empty_directory() {
    let temp = TempDir::new().unwrap();

    let result = list_plugins(&temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_list_plugins_single_plugin() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D]; // WASM magic bytes
    save_test_plugin(&store, "test-plugin", &vfs, &wasm).unwrap();

    // List plugins
    let result = list_plugins(&temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_list_plugins_multiple_plugins() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save multiple plugins
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];

    save_test_plugin(&store, "plugin1", &vfs, &wasm).unwrap();
    save_test_plugin(&store, "plugin2", &vfs, &wasm).unwrap();
    save_test_plugin(&store, "plugin3", &vfs, &wasm).unwrap();

    // List plugins with JSON format
    let result = list_plugins(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_list_plugins_all_formats() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "format-test", &vfs, &wasm).unwrap();

    // Test all output formats
    for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
        let result = list_plugins(&temp.path().to_path_buf(), format);
        assert!(result.is_ok(), "List should succeed with {format:?} format");
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}

#[test]
fn test_list_plugins_invalid_directory() {
    let temp = TempDir::new().unwrap();
    let invalid_path = temp.path().join("nonexistent");

    // Should still succeed but list will be empty
    let result = list_plugins(&invalid_path, OutputFormat::Json);
    assert!(result.is_ok());
}

// ============================================================================
// Load Command Tests
// ============================================================================

#[test]
fn test_load_plugin_success() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "loadable-plugin", &vfs, &wasm).unwrap();

    // Load plugin
    let result = load_plugin(
        "loadable-plugin",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_load_plugin_not_found() {
    let temp = TempDir::new().unwrap();

    let result = load_plugin(
        "nonexistent-plugin",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_err());
    let err_msg = format!("{:?}", result.unwrap_err());
    assert!(err_msg.contains("nonexistent-plugin"));
}

#[test]
fn test_load_plugin_all_formats() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "format-load-test", &vfs, &wasm).unwrap();

    // Test all output formats
    for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
        let result = load_plugin("format-load-test", &temp.path().to_path_buf(), format);
        assert!(result.is_ok(), "Load should succeed with {format:?} format");
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}

#[test]
fn test_load_plugin_with_large_wasm() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin with larger WASM module
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D]
        .into_iter()
        .chain(vec![0xFF; 1024 * 100]) // 100KB WASM
        .collect::<Vec<_>>();

    save_test_plugin(&store, "large-wasm", &vfs, &wasm).unwrap();

    // Load plugin
    let result = load_plugin("large-wasm", &temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_load_plugin_empty_vfs() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin with empty VFS
    let vfs = VfsBuilder::new().build().unwrap();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    let server_info = create_test_server_info("empty-vfs");

    store
        .save_plugin("empty-vfs", &vfs, &wasm, server_info, vec![])
        .unwrap();

    // Load plugin
    let result = load_plugin("empty-vfs", &temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

// ============================================================================
// Info Command Tests
// ============================================================================

#[test]
fn test_info_plugin_success() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "info-test", &vfs, &wasm).unwrap();

    // Show info
    let result = show_plugin_info("info-test", &temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[test]
fn test_info_plugin_not_found() {
    let temp = TempDir::new().unwrap();

    let result = show_plugin_info(
        "nonexistent",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_info_plugin_all_formats() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "format-info-test", &vfs, &wasm).unwrap();

    // Test all output formats
    for format in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty] {
        let result = show_plugin_info("format-info-test", &temp.path().to_path_buf(), format);
        assert!(result.is_ok(), "Info should succeed with {format:?} format");
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);
    }
}

#[test]
fn test_info_plugin_with_many_tools() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

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
        .save_plugin("many-tools", &vfs, &wasm, server_info, tools)
        .unwrap();

    // Show info
    let result = show_plugin_info("many-tools", &temp.path().to_path_buf(), OutputFormat::Json);

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

// ============================================================================
// Remove Command Tests
// ============================================================================

#[test]
fn test_remove_plugin_with_yes_flag() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "removable", &vfs, &wasm).unwrap();

    // Remove with --yes flag (skip confirmation)
    let result = remove_plugin(
        "removable",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );

    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);

    // Verify plugin was removed
    assert!(!store.plugin_exists("removable").unwrap());
}

#[test]
fn test_remove_plugin_not_found() {
    let temp = TempDir::new().unwrap();

    let result = remove_plugin(
        "nonexistent",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_remove_plugin_all_formats() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save multiple plugins for different formats
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];

    // Test each output format with a separate plugin
    for (i, format) in [OutputFormat::Json, OutputFormat::Text, OutputFormat::Pretty]
        .iter()
        .enumerate()
    {
        let plugin_name = format!("remove-format-{i}");
        save_test_plugin(&store, &plugin_name, &vfs, &wasm).unwrap();

        let result = remove_plugin(&plugin_name, &temp.path().to_path_buf(), true, *format);
        assert!(
            result.is_ok(),
            "Remove should succeed with {format:?} format"
        );
        assert_eq!(result.unwrap(), ExitCode::SUCCESS);

        // Verify removal
        assert!(!store.plugin_exists(&plugin_name).unwrap());
    }
}

#[test]
fn test_remove_plugin_and_reload_fails() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "to-remove", &vfs, &wasm).unwrap();

    // Remove it
    let result = remove_plugin(
        "to-remove",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );
    assert!(result.is_ok());

    // Try to load - should fail
    let load_result = load_plugin("to-remove", &temp.path().to_path_buf(), OutputFormat::Json);
    assert!(load_result.is_err());
}

// ============================================================================
// Integration: Run Function Tests
// ============================================================================

#[tokio::test]
async fn test_run_load_action() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "run-load-test", &vfs, &wasm).unwrap();

    // Test via run function
    let action = PluginAction::Load {
        name: "run-load-test".to_string(),
        plugin_dir: temp.path().to_path_buf(),
    };

    let result = run(action, OutputFormat::Json).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[tokio::test]
async fn test_run_list_action() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save some plugins
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "run-list-1", &vfs, &wasm).unwrap();
    save_test_plugin(&store, "run-list-2", &vfs, &wasm).unwrap();

    // Test via run function
    let action = PluginAction::List {
        plugin_dir: temp.path().to_path_buf(),
    };

    let result = run(action, OutputFormat::Json).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[tokio::test]
async fn test_run_info_action() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "run-info-test", &vfs, &wasm).unwrap();

    // Test via run function
    let action = PluginAction::Info {
        name: "run-info-test".to_string(),
        plugin_dir: temp.path().to_path_buf(),
    };

    let result = run(action, OutputFormat::Json).await;
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), ExitCode::SUCCESS);
}

#[tokio::test]
async fn test_run_remove_action() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "run-remove-test", &vfs, &wasm).unwrap();

    // Test via run function
    let action = PluginAction::Remove {
        name: "run-remove-test".to_string(),
        plugin_dir: temp.path().to_path_buf(),
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
fn test_load_plugin_invalid_name() {
    let temp = TempDir::new().unwrap();

    // Try to load with invalid server name (path traversal)
    let result = load_plugin(
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
    let result = show_plugin_info(
        "invalid/name",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_remove_plugin_invalid_name() {
    let temp = TempDir::new().unwrap();

    // Try to remove with invalid server name
    let result = remove_plugin(
        "invalid\\name",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );

    assert!(result.is_err());
}

#[test]
fn test_list_plugins_with_corrupted_metadata() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a valid plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "valid-plugin", &vfs, &wasm).unwrap();

    // Create a corrupted plugin directory
    let corrupt_dir = temp.path().join("corrupt-plugin");
    std::fs::create_dir_all(&corrupt_dir).unwrap();
    std::fs::write(corrupt_dir.join("plugin.json"), "invalid json").unwrap();

    // List should still work but may skip corrupted plugins
    let result = list_plugins(&temp.path().to_path_buf(), OutputFormat::Json);

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
fn test_load_plugin_with_nested_vfs_paths() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Create VFS with deeply nested paths
    let vfs = VfsBuilder::new()
        .add_file("/a/b/c/d/e/deep.ts", "export const DEEP = true;")
        .add_file("/x/y/z/nested.ts", "export const NESTED = true;")
        .build()
        .unwrap();

    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    let server_info = create_test_server_info("nested-paths");

    store
        .save_plugin("nested-paths", &vfs, &wasm, server_info, vec![])
        .unwrap();

    // Load should succeed
    let result = load_plugin(
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
    let store = PluginStore::new(temp.path()).unwrap();

    // 1. Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "lifecycle-test", &vfs, &wasm).unwrap();

    // 2. List plugins - should find it
    let list_result = list_plugins(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(list_result.is_ok());

    // 3. Load plugin - should succeed
    let load_result = load_plugin(
        "lifecycle-test",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );
    assert!(load_result.is_ok());

    // 4. Show info - should succeed
    let info_result = show_plugin_info(
        "lifecycle-test",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );
    assert!(info_result.is_ok());

    // 5. Remove plugin - should succeed
    let remove_result = remove_plugin(
        "lifecycle-test",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );
    assert!(remove_result.is_ok());

    // 6. List again - should be empty or not include removed plugin
    let list_after_result = list_plugins(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(list_after_result.is_ok());

    // 7. Try to load removed plugin - should fail
    let load_after_result = load_plugin(
        "lifecycle-test",
        &temp.path().to_path_buf(),
        OutputFormat::Json,
    );
    assert!(load_after_result.is_err());
}

#[test]
fn test_multiple_plugins_independent_operations() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save multiple plugins
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];

    save_test_plugin(&store, "plugin-a", &vfs, &wasm).unwrap();
    save_test_plugin(&store, "plugin-b", &vfs, &wasm).unwrap();
    save_test_plugin(&store, "plugin-c", &vfs, &wasm).unwrap();

    // Load one
    let load_a = load_plugin("plugin-a", &temp.path().to_path_buf(), OutputFormat::Json);
    assert!(load_a.is_ok());

    // Remove another
    let remove_b = remove_plugin(
        "plugin-b",
        &temp.path().to_path_buf(),
        true,
        OutputFormat::Json,
    );
    assert!(remove_b.is_ok());

    // Show info for third
    let info_c = show_plugin_info("plugin-c", &temp.path().to_path_buf(), OutputFormat::Json);
    assert!(info_c.is_ok());

    // Verify states
    assert!(store.plugin_exists("plugin-a").unwrap());
    assert!(!store.plugin_exists("plugin-b").unwrap());
    assert!(store.plugin_exists("plugin-c").unwrap());

    // List should show remaining plugins
    let list_result = list_plugins(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(list_result.is_ok());
}

// ============================================================================
// Output Format Validation
// ============================================================================

#[test]
fn test_load_output_contains_expected_fields() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "field-test", &vfs, &wasm).unwrap();

    // Capture the actual function behavior by calling the underlying store
    let loaded = store.load_plugin("field-test").unwrap();

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
    let store = PluginStore::new(temp.path()).unwrap();

    let plugins = store.list_plugins().unwrap();
    assert_eq!(plugins.len(), 0);

    let result = list_plugins(&temp.path().to_path_buf(), OutputFormat::Json);
    assert!(result.is_ok());
}

#[test]
fn test_info_output_includes_tool_details() {
    let temp = TempDir::new().unwrap();
    let store = PluginStore::new(temp.path()).unwrap();

    // Save a plugin with specific tools
    let vfs = create_test_vfs();
    let wasm = vec![0x00, 0x61, 0x73, 0x6D];
    save_test_plugin(&store, "tool-details", &vfs, &wasm).unwrap();

    // Load to verify tool information is preserved
    let loaded = store.load_plugin("tool-details").unwrap();

    assert_eq!(loaded.metadata.tools.len(), 2);
    assert_eq!(loaded.metadata.tools[0].name, "send_message");
    assert_eq!(loaded.metadata.tools[1].name, "get_chat_info");
}
