//! Plugin Persistence Workflow Example
//!
//! Demonstrates end-to-end plugin lifecycle:
//! 1. Create plugin data (simulating code generation)
//! 2. Save as a reusable plugin with checksums
//! 3. List available plugins
//! 4. Load plugin from disk with integrity verification
//! 5. Verify all checksums match
//! 6. Remove plugin
//!
//! Run with: cargo run --example `plugin_workflow`

use anyhow::{Context, Result};
use mcp_plugin_store::{PluginStore, ServerInfo, ToolInfo};
use mcp_vfs::VfsBuilder;
use tempfile::TempDir;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::fmt()
        .with_env_filter("plugin_workflow=info,mcp_plugin_store=debug")
        .init();

    println!("=== MCP Plugin Persistence Workflow ===\n");

    // 1. Create temporary plugin directory for this demo
    let temp_dir = TempDir::new().context("failed to create temp directory")?;
    let plugin_dir = temp_dir.path().to_path_buf();
    println!("📁 Plugin directory: {}", plugin_dir.display());

    // 2. Create mock server info (simulating real server like vkteams-bot)
    println!("\n🔍 Step 1: Creating plugin data...");
    let server_name = "vkteams-bot";
    let server_info = ServerInfo {
        name: server_name.to_string(),
        version: "1.0.0".to_string(),
        protocol_version: "2024-11-05".to_string(),
    };

    // Create tool metadata
    let tools = vec![
        ToolInfo {
            name: "send_message".to_string(),
            description: "Sends a message to a chat".to_string(),
        },
        ToolInfo {
            name: "edit_message".to_string(),
            description: "Edits an existing message".to_string(),
        },
        ToolInfo {
            name: "delete_message".to_string(),
            description: "Deletes a message".to_string(),
        },
        ToolInfo {
            name: "get_chat_info".to_string(),
            description: "Gets information about a chat".to_string(),
        },
        ToolInfo {
            name: "send_file".to_string(),
            description: "Sends a file to a chat".to_string(),
        },
    ];

    println!("  ✓ Server: {} v{}", server_info.name, server_info.version);
    println!("  ✓ Tools: {}", tools.len());

    // 3. Create mock generated TypeScript files (simulating code generation)
    println!("\n📝 Step 2: Building virtual filesystem...");
    let generated_files = vec![
        (
            "/index.ts".to_string(),
            "export * from './tools';\n".to_string(),
        ),
        (
            "/types.ts".to_string(),
            "export interface Message { id: string; text: string; }\n".to_string(),
        ),
        (
            "/tools/send_message.ts".to_string(),
            "export async function sendMessage(chatId: string, text: string) { /* ... */ }\n"
                .to_string(),
        ),
        (
            "/tools/edit_message.ts".to_string(),
            "export async function editMessage(messageId: string, text: string) { /* ... */ }\n"
                .to_string(),
        ),
        (
            "/tools/delete_message.ts".to_string(),
            "export async function deleteMessage(messageId: string) { /* ... */ }\n".to_string(),
        ),
        (
            "/tools/get_chat_info.ts".to_string(),
            "export async function getChatInfo(chatId: string) { /* ... */ }\n".to_string(),
        ),
        (
            "/tools/send_file.ts".to_string(),
            "export async function sendFile(chatId: string, file: File) { /* ... */ }\n"
                .to_string(),
        ),
    ];

    let mut vfs_builder = VfsBuilder::new();
    for (path, content) in &generated_files {
        vfs_builder = vfs_builder.add_file(path.clone(), content.clone());
    }
    let vfs = vfs_builder.build().context("failed to build VFS")?;

    println!("  ✓ VFS created with {} files", vfs.file_count());

    // 4. Create mock WASM module (in real scenario, this would be compiled TypeScript)
    println!("\n⚙️  Step 3: Creating WASM module...");
    let wasm_module = create_mock_wasm_module();
    println!("  ✓ WASM module created ({} bytes)", wasm_module.len());

    // 5. Save plugin to disk
    println!("\n💾 Step 4: Saving plugin...");
    let store = PluginStore::new(&plugin_dir).context("failed to create plugin store")?;

    let metadata = store
        .save_plugin(server_name, &vfs, &wasm_module, server_info, tools.clone())
        .context("failed to save plugin")?;

    println!("  ✓ Plugin saved: {server_name}");
    println!("  ✓ Format version: {}", metadata.format_version);
    println!("  ✓ Generator version: {}", metadata.generator_version);
    println!("  ✓ Generated at: {}", metadata.generated_at);
    println!("  ✓ WASM checksum: {}...", &metadata.checksums.wasm[..24]);
    println!(
        "  ✓ VFS files checksummed: {}",
        metadata.checksums.generated.len()
    );

    // 6. List available plugins
    println!("\n📋 Step 5: Listing plugins...");
    let plugins = store.list_plugins().context("failed to list plugins")?;
    println!("  ✓ Found {} plugin(s)", plugins.len());
    for plugin_info in &plugins {
        println!("    - {} v{}", plugin_info.server_name, plugin_info.version);
        println!("      Tools: {}", plugin_info.tool_count);
        println!("      Generated: {}", plugin_info.generated_at);
    }

    // 7. Load plugin from disk
    println!("\n📦 Step 6: Loading plugin...");
    let loaded = store
        .load_plugin(server_name)
        .context("failed to load plugin")?;

    println!("  ✓ Plugin loaded successfully");
    println!(
        "  ✓ WASM size: {} bytes (checksum verified ✓)",
        loaded.wasm_module.len()
    );
    println!(
        "  ✓ VFS files: {} (all verified ✓)",
        loaded.vfs.file_count()
    );
    println!("  ✓ Tools: {}", loaded.metadata.tools.len());

    // 8. Verify loaded data matches original
    println!("\n🔍 Step 7: Verifying integrity...");
    assert_eq!(
        loaded.wasm_module.len(),
        wasm_module.len(),
        "WASM size mismatch"
    );
    assert_eq!(
        loaded.vfs.file_count(),
        vfs.file_count(),
        "VFS file count mismatch"
    );
    assert_eq!(
        loaded.metadata.tools.len(),
        tools.len(),
        "Tool count mismatch"
    );

    // Verify WASM bytes match exactly
    assert_eq!(loaded.wasm_module, wasm_module, "WASM content mismatch");

    // Verify file contents by reading from both VFS instances
    for (file_path, expected_content) in &generated_files {
        let loaded_content = loaded
            .vfs
            .read_file(file_path)
            .context(format!("failed to read file: {file_path}"))?;
        assert_eq!(
            loaded_content,
            expected_content.as_str(),
            "File content mismatch: {file_path}"
        );
    }

    println!("  ✓ All checksums verified");
    println!("  ✓ All {} files match original", generated_files.len());
    println!("  ✓ WASM bytes match exactly");
    println!("  ✓ Metadata matches");

    // 9. Show plugin info
    println!("\n📊 Step 8: Plugin information:");
    println!(
        "  Server: {} v{}",
        loaded.metadata.server.name, loaded.metadata.server.version
    );
    println!("  Protocol: {}", loaded.metadata.server.protocol_version);
    println!("  Format: v{}", loaded.metadata.format_version);
    println!("  Generator: v{}", loaded.metadata.generator_version);
    println!("\n  Tools ({}):", loaded.metadata.tools.len());
    for tool in &loaded.metadata.tools {
        println!("    • {} - {}", tool.name, tool.description);
    }

    // 10. Check if plugin exists
    println!("\n🔍 Step 9: Checking plugin existence...");
    assert!(
        store
            .plugin_exists(server_name)
            .context("plugin_exists failed")?,
        "Plugin should exist"
    );
    assert!(
        !store
            .plugin_exists("nonexistent")
            .context("plugin_exists failed")?,
        "Nonexistent plugin should not exist"
    );
    println!("  ✓ Existence checks passed");

    // 11. Remove plugin
    println!("\n🗑️  Step 10: Removing plugin...");
    store
        .remove_plugin(server_name)
        .context("failed to remove plugin")?;
    println!("  ✓ Plugin removed: {server_name}");

    // 12. Verify removal
    println!("\n✅ Step 11: Verifying removal...");
    assert!(
        !store
            .plugin_exists(server_name)
            .context("plugin_exists failed")?,
        "Plugin should not exist after removal"
    );
    let removed_list = store.list_plugins().context("failed to list plugins")?;
    assert!(removed_list.is_empty(), "Plugin list should be empty");
    println!("  ✓ Plugin successfully removed");
    println!("  ✓ Plugin directory cleaned up");

    // 13. Try to load removed plugin (should fail)
    println!("\n🔍 Step 12: Confirming plugin is gone...");
    let load_result = store.load_plugin(server_name);
    assert!(load_result.is_err(), "Loading removed plugin should fail");
    println!("  ✓ Loading removed plugin correctly fails");

    println!("\n=== Workflow Complete! ===");
    println!("\n📚 Summary:");
    println!("  ✅ Created plugin data (server info + tools)");
    println!("  ✅ Built VFS with {} files", vfs.file_count());
    println!("  ✅ Created WASM module ({} bytes)", wasm_module.len());
    println!("  ✅ Saved plugin with Blake3 checksums");
    println!("  ✅ Listed plugins");
    println!("  ✅ Loaded plugin from disk");
    println!("  ✅ Verified integrity (all checksums match)");
    println!("  ✅ Removed plugin cleanly");
    println!("\n🎉 All operations successful!");

    // Temp directory automatically cleaned up on drop
    Ok(())
}

/// Creates a mock WASM module for demonstration.
///
/// In a real scenario, this would be the result of compiling
/// TypeScript to WASM using tools like `AssemblyScript` or `QuickJS`.
fn create_mock_wasm_module() -> Vec<u8> {
    // Simple WASM module that exports a function returning 42
    // (module
    //   (func (export "main") (result i32)
    //     i32.const 42
    //   )
    // )
    vec![
        0x00, 0x61, 0x73, 0x6d, // WASM magic number
        0x01, 0x00, 0x00, 0x00, // WASM version
        0x01, 0x05, 0x01, 0x60, 0x00, 0x01, 0x7f, // Type section
        0x03, 0x02, 0x01, 0x00, // Function section
        0x07, 0x08, 0x01, 0x04, 0x6d, 0x61, 0x69, 0x6e, 0x00, 0x00, // Export section
        0x0a, 0x06, 0x01, 0x04, 0x00, 0x41, 0x2a, 0x0b, // Code section (returns 42)
    ]
}
