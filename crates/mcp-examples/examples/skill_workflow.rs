//! Skill Persistence Workflow Example
//!
//! Demonstrates end-to-end skill lifecycle:
//! 1. Create skill data (simulating code generation)
//! 2. Save as a reusable skill with checksums
//! 3. List available skills
//! 4. Load skill from disk with integrity verification
//! 5. Verify all checksums match
//! 6. Remove skill
//!
//! Run with: cargo run --example `skill_workflow`

use anyhow::{Context, Result};
use mcp_skill_store::{ServerInfo, SkillStore, ToolInfo};
use mcp_vfs::VfsBuilder;
use tempfile::TempDir;

#[tokio::main]
async fn main() -> Result<()> {
    // Setup logging
    tracing_subscriber::fmt()
        .with_env_filter("skill_workflow=info,mcp_skill_store=debug")
        .init();

    println!("=== MCP Skill Persistence Workflow ===\n");

    // 1. Create temporary skill directory for this demo
    let temp_dir = TempDir::new().context("failed to create temp directory")?;
    let skill_dir = temp_dir.path().to_path_buf();
    println!("📁 Skill directory: {}", skill_dir.display());

    // 2. Create mock server info (simulating real server like github)
    println!("\n🔍 Step 1: Creating skill data...");
    let server_name = "github";
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

    // 5. Save skill to disk
    println!("\n💾 Step 4: Saving skill...");
    let store = SkillStore::new(&skill_dir).context("failed to create skill store")?;

    let metadata = store
        .save_skill(server_name, &vfs, &wasm_module, server_info, tools.clone())
        .context("failed to save skill")?;

    println!("  ✓ Skill saved: {server_name}");
    println!("  ✓ Format version: {}", metadata.format_version);
    println!("  ✓ Generator version: {}", metadata.generator_version);
    println!("  ✓ Generated at: {}", metadata.generated_at);
    println!("  ✓ WASM checksum: {}...", &metadata.checksums.wasm[..24]);
    println!(
        "  ✓ VFS files checksummed: {}",
        metadata.checksums.generated.len()
    );

    // 6. List available skills
    println!("\n📋 Step 5: Listing skills...");
    let skills = store.list_skills().context("failed to list skills")?;
    println!("  ✓ Found {} skill(s)", skills.len());
    for skill_info in &skills {
        println!("    - {} v{}", skill_info.server_name, skill_info.version);
        println!("      Tools: {}", skill_info.tool_count);
        println!("      Generated: {}", skill_info.generated_at);
    }

    // 7. Load skill from disk
    println!("\n📦 Step 6: Loading skill...");
    let loaded = store
        .load_skill(server_name)
        .context("failed to load skill")?;

    println!("  ✓ Skill loaded successfully");
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

    // 9. Show skill info
    println!("\n📊 Step 8: Skill information:");
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

    // 10. Check if skill exists
    println!("\n🔍 Step 9: Checking skill existence...");
    assert!(
        store
            .skill_exists(server_name)
            .context("skill_exists failed")?,
        "Skill should exist"
    );
    assert!(
        !store
            .skill_exists("nonexistent")
            .context("skill_exists failed")?,
        "Nonexistent skill should not exist"
    );
    println!("  ✓ Existence checks passed");

    // 11. Remove skill
    println!("\n🗑️  Step 10: Removing skill...");
    store
        .remove_skill(server_name)
        .context("failed to remove skill")?;
    println!("  ✓ Skill removed: {server_name}");

    // 12. Verify removal
    println!("\n✅ Step 11: Verifying removal...");
    assert!(
        !store
            .skill_exists(server_name)
            .context("skill_exists failed")?,
        "Skill should not exist after removal"
    );
    let removed_list = store.list_skills().context("failed to list skills")?;
    assert!(removed_list.is_empty(), "Skill list should be empty");
    println!("  ✓ Skill successfully removed");
    println!("  ✓ Skill directory cleaned up");

    // 13. Try to load removed skill (should fail)
    println!("\n🔍 Step 12: Confirming skill is gone...");
    let load_result = store.load_skill(server_name);
    assert!(load_result.is_err(), "Loading removed skill should fail");
    println!("  ✓ Loading removed skill correctly fails");

    println!("\n=== Workflow Complete! ===");
    println!("\n📚 Summary:");
    println!("  ✅ Created skill data (server info + tools)");
    println!("  ✅ Built VFS with {} files", vfs.file_count());
    println!("  ✅ Created WASM module ({} bytes)", wasm_module.len());
    println!("  ✅ Saved skill with Blake3 checksums");
    println!("  ✅ Listed plugins");
    println!("  ✅ Loaded skill from disk");
    println!("  ✅ Verified integrity (all checksums match)");
    println!("  ✅ Removed skill cleanly");
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
