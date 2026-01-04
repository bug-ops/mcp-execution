#![allow(clippy::format_push_string)]

use mcp_execution_files::FilesBuilder;

#[cfg(feature = "dhat-heap")]
#[global_allocator]
static ALLOC: dhat::Alloc = dhat::Alloc;

fn main() {
    #[cfg(feature = "dhat-heap")]
    let _profiler = dhat::Profiler::new_heap();

    println!("=== Memory Profiling: mcp-vfs ===\n");

    // Scenario 1: Typical usage (100 files, 5KB each)
    println!("Scenario 1: 100 files (typical usage)");
    scenario_typical();

    // Scenario 2: Large deployment (1000 files)
    println!("\nScenario 2: 1000 files (large deployment)");
    scenario_large();

    // Scenario 3: Stress test (10000 files)
    #[cfg(not(debug_assertions))]
    {
        println!("\nScenario 3: 10000 files (stress test)");
        scenario_stress();
    }

    #[cfg(feature = "dhat-heap")]
    println!(
        "\n=== Profiling complete. Check dhat-heap.json ===\n\
         View at: https://nnethercote.github.io/dh_view/dh_view.html"
    );

    #[cfg(not(feature = "dhat-heap"))]
    println!(
        "\n=== To enable DHAT profiling: ===\n\
         1. Add to Cargo.toml: dhat = \"0.3\"\n\
         2. Run: cargo run --example profile_memory --features dhat-heap --release"
    );
}

fn scenario_typical() {
    let vfs = FilesBuilder::new()
        .add_files((0..100).map(|i| {
            (
                format!("/mcp-tools/servers/test/file_{i}.ts"),
                generate_typescript_file(i, 5000), // ~5KB per file
            )
        }))
        .build()
        .unwrap();

    // Perform typical operations
    println!("  - Created VFS with {} files", vfs.file_count());

    // Read all files
    for i in 0..100 {
        let path = format!("/mcp-tools/servers/test/file_{i}.ts");
        let _ = vfs.read_file(&path).unwrap();
    }
    println!("  - Read all files");

    // Check existence
    for i in 0..100 {
        let path = format!("/mcp-tools/servers/test/file_{i}.ts");
        assert!(vfs.exists(&path));
    }
    println!("  - Checked existence of all files");

    // List directory
    let entries = vfs.list_dir("/mcp-tools/servers/test").unwrap();
    println!("  - Listed directory: {} entries", entries.len());

    // Get all paths
    let paths = vfs.all_paths();
    println!("  - Retrieved all paths: {}", paths.len());

    let total_size: usize = (0..100)
        .map(|i| {
            let path = format!("/mcp-tools/servers/test/file_{i}.ts");
            vfs.read_file(&path).unwrap().len()
        })
        .sum();

    println!("  - Total content size: {} KB", total_size / 1024);
    println!(
        "  - Expected memory: ~{} KB (content + overhead)",
        (total_size + 100 * 64) / 1024
    );
}

fn scenario_large() {
    let vfs = FilesBuilder::new()
        .add_files((0..1000).map(|i| {
            (
                format!("/mcp-tools/servers/test/file_{i}.ts"),
                generate_typescript_file(i, 5000), // ~5KB per file
            )
        }))
        .build()
        .unwrap();

    println!("  - Created VFS with {} files", vfs.file_count());

    // Sample read operations (10% of files)
    for i in (0..1000).step_by(10) {
        let path = format!("/mcp-tools/servers/test/file_{i}.ts");
        let _ = vfs.read_file(&path).unwrap();
    }
    println!("  - Read 10% of files (100 reads)");

    let entries = vfs.list_dir("/mcp-tools/servers/test").unwrap();
    println!("  - Listed directory: {} entries", entries.len());

    let total_size: usize = (0..1000)
        .map(|i| {
            let path = format!("/mcp-tools/servers/test/file_{i}.ts");
            vfs.read_file(&path).unwrap().len()
        })
        .sum();

    println!("  - Total content size: {} MB", total_size / 1024 / 1024);
    println!(
        "  - Expected memory: ~{} MB (content + overhead)",
        (total_size + 1000 * 64) / 1024 / 1024
    );
}

#[cfg(not(debug_assertions))]
fn scenario_stress() {
    println!("  - Building VFS with 10000 files...");

    let vfs = FilesBuilder::new()
        .add_files((0..10000).map(|i| {
            (
                format!("/mcp-tools/servers/test/file_{}.ts", i),
                generate_typescript_file(i, 5000), // ~5KB per file
            )
        }))
        .build()
        .unwrap();

    println!("  - Created VFS with {} files", vfs.file_count());

    // Sample operations
    for i in (0..10000).step_by(100) {
        let path = format!("/mcp-tools/servers/test/file_{}.ts", i);
        let _ = vfs.read_file(&path).unwrap();
    }
    println!("  - Read 1% of files (100 reads)");

    let entries = vfs.list_dir("/mcp-tools/servers/test").unwrap();
    println!("  - Listed directory: {} entries", entries.len());

    let total_size: usize = (0..10000)
        .map(|i| {
            let path = format!("/mcp-tools/servers/test/file_{}.ts", i);
            vfs.read_file(&path).unwrap().len()
        })
        .sum();

    println!("  - Total content size: {} MB", total_size / 1024 / 1024);
    println!(
        "  - Expected memory: ~{} MB (content + overhead)",
        (total_size + 10000 * 64) / 1024 / 1024
    );
}

/// Generate realistic TypeScript file content
fn generate_typescript_file(index: usize, target_size: usize) -> String {
    let mut content = String::with_capacity(target_size);

    // File header
    content.push_str(&format!(
        "// Generated TypeScript file #{index}\n\
         // This is a sample MCP tool definition\n\n"
    ));

    // Type definitions
    content.push_str(&format!(
        "export interface Params{index} {{\n\
         \x20\x20chatId: string;\n\
         \x20\x20messageId?: string;\n\
         \x20\x20text: string;\n\
         \x20\x20attachments?: Attachment[];\n\
         }}\n\n"
    ));

    // Function definition
    content.push_str(&format!(
        "export async function tool_{index}(params: Params{index}): Promise<Result> {{\n\
         \x20\x20// Tool implementation\n\
         \x20\x20const result = await mcpBridge.callTool(\n\
         \x20\x20\x20\x20'tool_{index}',\n\
         \x20\x20\x20\x20JSON.stringify(params)\n\
         \x20\x20);\n\
         \x20\x20return JSON.parse(result) as Result;\n\
         }}\n\n"
    ));

    // Add comments to reach target size
    while content.len() < target_size {
        content.push_str(&format!(
            "// Additional documentation line {}\n",
            content.len()
        ));
    }

    content
}
